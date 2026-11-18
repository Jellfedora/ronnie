//! A conversation with Claude Code from a tracked project's page ("Suivi"): each message runs
//! `claude -p` in the folder of the profile tied to the project (the conversation resumed by its
//! session id), its answer read as it comes (stream-json). Claude reads freely; what it would change
//! (a file written, a command run) is refused at first and comes back as a request the user allows or
//! not: allowed, the conversation is resumed with those very actions granted.
//!
//! Each project's conversation is kept on this machine, in `<config>/track-chats/<project>.json`.

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Kept: so many items at most (the oldest go).
const KEEP: usize = 400;

/// Something Claude would do that needs the user's leave.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Denial {
    /// The tool (Write, Edit, Bash…).
    pub tool: String,
    /// What it is about: a file, a command.
    pub detail: String,
    /// The rule that allows it (`--allowedTools`).
    pub rule: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
    User {
        text: String,
        /// The files sent with it (copied in `attachments_dir`), by name.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    },
    Claude { text: String },
    /// A tool Claude used: its name and what on (a file, a command, a search).
    Tool { name: String, detail: String },
    /// Actions waiting for the user's leave (None: not answered yet).
    Ask { denials: Vec<Denial>, answered: Option<bool> },
    Error { text: String },
    /// Stopped by the user.
    Stopped,
}

/// A project's conversation.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Chat {
    /// Claude Code's session, to resume.
    #[serde(default)]
    pub session: Option<String>,
    /// File changes made without asking (`acceptEdits`).
    #[serde(default)]
    pub accept_edits: bool,
    #[serde(default)]
    pub items: Vec<Item>,
}

fn dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("track-chats"))
}

/// Where the files sent to Claude in a project's conversation are kept (images pasted, files dropped
/// or picked): Claude Code is given the right to read there.
pub fn attachments_dir(project: &str) -> Option<PathBuf> {
    let name: String = project.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    (!name.is_empty()).then(|| dir().map(|d| d.join("attachments").join(name))).flatten()
}

/// `file` copied in `dir` under a name of its own (a number before it when taken): its path.
pub fn keep_file(dir: &Path, file: &Path) -> std::io::Result<PathBuf> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "fichier".into());
    let out = free_name(dir, &name)?;
    std::fs::copy(file, &out)?;
    Ok(out)
}

/// A PNG kept in `dir` (an image pasted): its path.
pub fn keep_png(dir: &Path, png: &[u8]) -> std::io::Result<PathBuf> {
    let out = free_name(dir, &format!("image-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S")))?;
    std::fs::write(&out, png)?;
    Ok(out)
}

fn free_name(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut out = dir.join(name);
    let mut k = 2;
    while out.exists() {
        out = dir.join(format!("{k}-{name}"));
        k += 1;
    }
    Ok(out)
}

/// A project id as a file name (floor's ids are UUIDs: anything else is dropped).
fn file_of(project: &str) -> Option<PathBuf> {
    let name: String = project.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    (!name.is_empty()).then(|| dir().map(|d| d.join(format!("{name}.json")))).flatten()
}

impl Chat {
    pub fn load(project: &str) -> Chat {
        file_of(project).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&mut self, project: &str) {
        if self.items.len() > KEEP {
            self.items.drain(..self.items.len() - KEEP);
        }
        if let Some(path) = file_of(project) {
            let _ = crate::config::save(&path, self);
        }
    }

    /// Forgotten: a new conversation (Claude Code's session too).
    pub fn clear(project: &str) {
        if let Some(path) = file_of(project) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// What comes from a running conversation.
#[derive(Debug, PartialEq)]
pub enum Event {
    Session(String),
    Item(Item),
    /// The run ended (an error said, when it failed).
    Done(Option<String>),
}

/// Claude Code's command: on the PATH, else where its installers put it (an app started from the
/// Dock doesn't have the shell's PATH).
pub fn binary() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".claude/local"));
        dirs.push(home.join(".npm-global/bin"));
        if cfg!(windows) {
            dirs.push(home.join("AppData/Roaming/npm"));
        }
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// A path said from the working folder when it is in it.
fn short_path(path: &str, cwd: &Path) -> String {
    Path::new(path).strip_prefix(cwd).map_or_else(|_| path.to_owned(), |p| p.display().to_string())
}

/// What a tool is about, from its input: a file, a command, a pattern, an address.
pub fn tool_detail(name: &str, input: &Value, cwd: &Path) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    match name {
        "Read" | "Write" | "Edit" | "MultiEdit" => short_path(&s("file_path"), cwd),
        "NotebookEdit" => short_path(&s("notebook_path"), cwd),
        "Bash" => s("command"),
        "Glob" | "Grep" => s("pattern"),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        "Task" | "Agent" => s("description"),
        _ => String::new(),
    }
}

/// The rule that lets Claude do what was refused.
fn rule_of(name: &str, input: &Value) -> String {
    match name {
        "Bash" => format!("Bash({})", input.get("command").and_then(Value::as_str).unwrap_or("")),
        other => other.to_owned(),
    }
}

/// One line of `claude -p --output-format stream-json`, as events.
pub fn parse_line(line: &str, cwd: &Path) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    let mut out = Vec::new();
    match v.get("type").and_then(Value::as_str) {
        Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
            if let Some(id) = v.get("session_id").and_then(Value::as_str) {
                out.push(Event::Session(id.to_owned()));
            }
        }
        Some("assistant") => {
            for block in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = block.get("text").and_then(Value::as_str).unwrap_or("").trim();
                        if !text.is_empty() {
                            out.push(Event::Item(Item::Claude { text: text.to_owned() }));
                        }
                    }
                    Some("tool_use") => {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("?").to_owned();
                        let detail = tool_detail(&name, block.get("input").unwrap_or(&Value::Null), cwd);
                        out.push(Event::Item(Item::Tool { name, detail }));
                    }
                    _ => {}
                }
            }
        }
        Some("result") => {
            let denials: Vec<Denial> = v.get("permission_denials").and_then(Value::as_array).into_iter().flatten().filter_map(|d| {
                let tool = d.get("tool_name").and_then(Value::as_str)?.to_owned();
                let input = d.get("tool_input").unwrap_or(&Value::Null);
                Some(Denial { detail: tool_detail(&tool, input, cwd), rule: rule_of(&tool, input), tool })
            }).fold(Vec::new(), |mut all, d| {
                if !all.contains(&d) {
                    all.push(d);
                }
                all
            });
            if !denials.is_empty() {
                out.push(Event::Item(Item::Ask { denials, answered: None }));
            }
            let failed = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let error = failed.then(|| v.get("result").and_then(Value::as_str).unwrap_or("Claude Code failed").to_owned());
            out.push(Event::Done(error));
        }
        _ => {}
    }
    out
}

/// A run going on: stopped by killing its process.
#[derive(Clone)]
pub struct Running(Arc<Mutex<Option<Child>>>);

impl Running {
    pub fn stop(&self) {
        if let Some(mut child) = self.0.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// What a message is sent with.
pub struct Ask {
    pub project: String,
    pub cwd: PathBuf,
    pub prompt: String,
    pub session: Option<String>,
    pub accept_edits: bool,
    /// Rules granted for this run (actions the user allowed), besides those always granted.
    pub allowed: Vec<String>,
    /// Told to Claude besides its usual prompt (the project, how to log tasks).
    pub system: String,
    /// Folders Claude may read besides its own (the files sent).
    pub add_dirs: Vec<PathBuf>,
}

/// `claude -p` started for `ask`; its events go to `tx` with the project's id, `wake` called after each.
pub fn start(ask: Ask, tx: Sender<(String, Event)>, wake: impl Fn() + Send + 'static) -> Result<Running, String> {
    let claude = binary().ok_or("Claude Code is not installed (https://claude.ai/install.sh)")?;
    let mut cmd = Command::new(&claude);
    cmd.current_dir(&ask.cwd).args(["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", if ask.accept_edits { "acceptEdits" } else { "default" }]);
    cmd.args(["--append-system-prompt", &ask.system]);
    if let Some(session) = &ask.session {
        cmd.args(["--resume", session]);
    }
    // Always granted: logging time and reading the projects (the user asks it here).
    let mut allowed = vec![format!("Bash({}:*)", crate::claude::ronnie_command("track").trim())];
    allowed.extend(ask.allowed.iter().cloned());
    if !ask.add_dirs.is_empty() {
        cmd.arg("--add-dir").args(&ask.add_dirs);
    }
    // Last: it takes all the words after it.
    cmd.arg("--allowedTools").args(&allowed);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // The folder of Claude Code's own command first in PATH (an app from the Dock lacks the shell's).
    if let Some(bin) = claude.parent() {
        let mut paths = vec![bin.to_path_buf()];
        paths.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
        if let Ok(path) = std::env::join_paths(paths) {
            cmd.env("PATH", path);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", claude.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = ask.prompt.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }
    let stdout = child.stdout.take().ok_or("no output")?;
    let stderr = child.stderr.take();
    let running = Running(Arc::new(Mutex::new(Some(child))));
    // Its real path (macOS: /var is /private/var), for the tools' paths to be said from it.
    let cwd = std::fs::canonicalize(&ask.cwd).unwrap_or(ask.cwd);
    let (project, held) = (ask.project, running.clone());
    let _ = std::thread::Builder::new().name("track-claude".into()).spawn(move || {
        let mut ended = false;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            for event in parse_line(&line, &cwd) {
                ended |= matches!(event, Event::Done(_));
                let _ = tx.send((project.clone(), event));
            }
            wake();
        }
        // No result: stopped, or failed before answering (what it said on stderr).
        if !ended {
            let mut said = String::new();
            if let Some(mut err) = stderr {
                let _ = std::io::Read::read_to_string(&mut err, &mut said);
            }
            let stopped = held.0.lock().unwrap().is_none();
            let error = if stopped { None } else { Some(said.trim().lines().last().unwrap_or("Claude Code stopped").to_owned()) };
            let _ = tx.send((project.clone(), Event::Done(error)));
        }
        if let Some(mut child) = held.0.lock().unwrap().take() {
            let _ = child.wait();
        }
        wake();
    });
    Ok(running)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_stream() {
        let cwd = Path::new("/w");
        assert_eq!(parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/w"}"#, cwd), vec![Event::Session("s1".into())]);
        assert_eq!(
            parse_line(r#"{"type":"assistant","message":{"content":[{"type":"text","text":" Voilà "},{"type":"tool_use","name":"Edit","input":{"file_path":"/w/src/a.php"}}]}}"#, cwd),
            vec![Event::Item(Item::Claude { text: "Voilà".into() }), Event::Item(Item::Tool { name: "Edit".into(), detail: "src/a.php".into() })]
        );
        let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","permission_denials":[
            {"tool_name":"Write","tool_input":{"file_path":"/w/b.txt","content":"x"}},
            {"tool_name":"Bash","tool_input":{"command":"npm test"}},
            {"tool_name":"Bash","tool_input":{"command":"npm test"}}]}"#;
        assert_eq!(
            parse_line(result, cwd),
            vec![
                Event::Item(Item::Ask {
                    denials: vec![
                        Denial { tool: "Write".into(), detail: "b.txt".into(), rule: "Write".into() },
                        Denial { tool: "Bash".into(), detail: "npm test".into(), rule: "Bash(npm test)".into() },
                    ],
                    answered: None
                }),
                Event::Done(None)
            ]
        );
        assert_eq!(parse_line(r#"{"type":"result","is_error":true,"result":"boom"}"#, cwd), vec![Event::Done(Some("boom".into()))]);
        assert_eq!(parse_line("not json", cwd), vec![]);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// With the real Claude Code (`cargo test live_round -- --ignored`): a write refused, then allowed.
    #[test]
    #[ignore]
    fn live_round() {
        let dir = std::env::temp_dir().join(format!("ronnie-trackchat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |prompt: &str, session: Option<String>, allowed: Vec<String>| {
            let (tx, rx) = std::sync::mpsc::channel();
            let ask = Ask { add_dirs: Vec::new(), project: "p".into(), cwd: dir.clone(), prompt: prompt.into(), session, accept_edits: false, allowed, system: "Answer in one short sentence.".into() };
            start(ask, tx, || {}).unwrap();
            let mut events = Vec::new();
            while let Ok((_, e)) = rx.recv_timeout(std::time::Duration::from_secs(180)) {
                let done = matches!(e, Event::Done(_));
                events.push(e);
                if done {
                    break;
                }
            }
            events
        };
        let first = run("Create the file hello.txt containing exactly: salut", None, Vec::new());
        println!("{first:#?}");
        let session = first.iter().find_map(|e| if let Event::Session(s) = e { Some(s.clone()) } else { None }).unwrap();
        let rules: Vec<String> = first.iter().find_map(|e| if let Event::Item(Item::Ask { denials, .. }) = e { Some(denials.iter().map(|d| d.rule.clone()).collect()) } else { None }).expect("the write refused");
        assert!(!dir.join("hello.txt").exists());
        let second = run("The user allowed it: do it now.", Some(session), rules);
        println!("{second:#?}");
        assert_eq!(std::fs::read_to_string(dir.join("hello.txt")).unwrap().trim(), "salut");
    }
}
