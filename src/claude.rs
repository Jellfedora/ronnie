//! Claude's plan usage, shown above the panes where Claude Code runs. Claude Code hands its status line
//! command the usage of the 5-hour session and of the week (claude.ai Pro and Max plans, see
//! https://code.claude.com/docs/en/statusline): `ronnie claude-statusline` is that command. It keeps
//! them in claude-usage.json for the window, then prints the status line, or runs the one configured
//! before Ronnie's (kept aside, and put back when Ronnie's is removed).
//!
//! Claude Code's hooks tell when Claude is done or waits for the user: `ronnie claude-hook` leaves an
//! event for the Ronnie whose pane it runs in (RONNIE_PANE, RONNIE_CLAUDE_EVENTS), which tells the user
//! when that pane isn't on screen. And as a session starts in a Ronnie pane, it tells Claude what Ronnie
//! offers (its Node.js versions, its mail catcher, its SSH servers…), for Claude to suggest them, and
//! the user's other projects lately (see `crate::worklog`, fed by the requests made in the panes).

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One usage window: how much of it is used, and when it starts again.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Window {
    /// 0 to 100.
    pub used_percentage: f32,
    /// Unix seconds.
    pub resets_at: i64,
}

impl Window {
    /// Used now: nothing once the window started again (Claude Code tells the new figure at its next
    /// answer).
    pub fn used_now(&self, now: i64) -> f32 {
        if now >= self.resets_at { 0.0 } else { self.used_percentage.clamp(0.0, 100.0) }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
    /// When Claude Code last told (Unix seconds).
    #[serde(default)]
    pub updated: i64,
}

impl Usage {
    /// From the status line input: None when it has no usage (not a Pro / Max plan, or before the
    /// session's first answer).
    fn from_status(input: &Value, now: i64) -> Option<Self> {
        let limits = input.get("rate_limits")?;
        let window = |key: &str| serde_json::from_value::<Window>(limits.get(key)?.clone()).ok();
        let usage = Usage { five_hour: window("five_hour"), seven_day: window("seven_day"), updated: now };
        (usage.five_hour.is_some() || usage.seven_day.is_some()).then_some(usage)
    }

    /// What a session just told, with what was kept (`older`, maybe told by another session): each
    /// session tells the figures of its own last answer, and an idle one tells them again every minute.
    /// In the same window the highest figure is the latest (usage only grows in it); a later window
    /// than the one told is kept (that session's figures are older).
    fn merged(self, older: Option<&Usage>) -> Usage {
        let Some(older) = older else { return self };
        // Claude Code may tell a reset a little apart from one answer to the next.
        const SAME: i64 = 600;
        let pick = |new: Option<Window>, old: Option<Window>| match (new, old) {
            (Some(n), Some(o)) if o.resets_at > n.resets_at + SAME => Some(o),
            (Some(n), Some(o)) if (o.resets_at - n.resets_at).abs() <= SAME => Some(Window { used_percentage: n.used_percentage.max(o.used_percentage), ..n }),
            (n, o) => n.or(o),
        };
        Usage { five_hour: pick(self.five_hour, older.five_hour), seven_day: pick(self.seven_day, older.seven_day), updated: self.updated }
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// In the installed app's folder, also for a dev build: the usage is the account's, and Claude Code runs
/// the installed app's status line command.
fn usage_path() -> Option<PathBuf> {
    crate::config::official_dir().map(|d| d.join("claude-usage.json"))
}

/// The status line configured before Ronnie's, kept to run it and to put it back.
fn previous_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("claude-statusline-previous.json"))
}

/// The last usage Claude Code told.
pub fn read_usage() -> Option<Usage> {
    serde_json::from_str(&std::fs::read_to_string(usage_path()?).ok()?).ok()
}

/// Replaces `path` with `text` all at once (a reader never sees half a file).
pub fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Claude Code's user settings.
fn settings_path() -> Option<PathBuf> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => directories::BaseDirs::new()?.home_dir().join(".claude"),
    };
    Some(dir.join("settings.json"))
}

fn read_settings() -> Result<serde_json::Map<String, Value>, String> {
    let Some(path) = settings_path() else { return Err("no home directory".into()) };
    match std::fs::read_to_string(&path) {
        Ok(text) if text.trim().is_empty() => Ok(Default::default()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            _ => Err(format!("{} : JSON invalide", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(format!("{} : {e}", path.display())),
    }
}

fn write_settings(map: serde_json::Map<String, Value>) -> Result<(), String> {
    let path = settings_path().ok_or("no home directory")?;
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())? + "\n";
    write_atomic(&path, &text).map_err(|e| format!("{} : {e}", path.display()))
}

/// The command Claude Code runs for its status line: this Ronnie.
fn status_command() -> String {
    ronnie_command("claude-statusline")
}

/// This Ronnie run with `arg`, quoted for the shell Claude Code runs it with.
fn ronnie_command(arg: &str) -> String {
    let exe = crate::update::appimage().or_else(|| std::env::current_exe().ok()).map(|p| p.display().to_string()).unwrap_or_else(|| "ronnie".into());
    let quoted = if cfg!(windows) {
        format!("\"{exe}\"")
    } else if exe.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        exe
    } else {
        format!("'{}'", exe.replace('\'', "'\\''"))
    };
    format!("{quoted} {arg}")
}

fn is_ours(status_line: Option<&Value>) -> bool {
    status_line.and_then(|s| s.get("command")).and_then(Value::as_str).is_some_and(|c| c.trim_end().ends_with(" claude-statusline"))
}

/// Claude Code tells Ronnie its usage (Ronnie's status line command is set).
pub fn connected() -> bool {
    read_settings().is_ok_and(|s| is_ours(s.get("statusLine")))
}

/// At launch (official builds, unless the user removed it): Ronnie set as Claude Code's status line
/// command, or its path updated if the app moved. Nothing when Claude Code isn't installed.
pub fn ensure_connected() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let settings = read_settings()?;
    let command = settings.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str);
    if command == Some(status_command().as_str()) {
        return Ok(());
    }
    connect()
}

/// Makes Ronnie Claude Code's status line command (the one set before still runs, through it).
pub fn connect() -> Result<(), String> {
    let mut settings = read_settings()?;
    if let Some(previous) = settings.get("statusLine").filter(|s| !is_ours(Some(s))) {
        let path = previous_path().ok_or("no config directory")?;
        write_atomic(&path, &previous.to_string()).map_err(|e| e.to_string())?;
    }
    // Refreshed every minute too: the percentages move while the session waits.
    settings.insert("statusLine".into(), json!({ "type": "command", "command": status_command(), "refreshInterval": 60 }));
    write_settings(settings)?;
    crate::log::info("claude: status line set");
    Ok(())
}

/// Puts back the status line set before Ronnie's (or none).
pub fn disconnect() -> Result<(), String> {
    let mut settings = read_settings()?;
    let previous = previous_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    match previous {
        Some(previous) => settings.insert("statusLine".into(), previous),
        None => settings.remove("statusLine"),
    };
    write_settings(settings)?;
    if let Some(p) = previous_path() {
        let _ = std::fs::remove_file(p);
    }
    crate::log::info("claude: status line removed");
    Ok(())
}

/// `ronnie claude-statusline`: run by Claude Code with the session's state on stdin.
pub fn run_statusline() {
    let mut input = Vec::new();
    let _ = std::io::stdin().take(4 << 20).read_to_end(&mut input);
    let status: Value = serde_json::from_slice(&input).unwrap_or(Value::Null);
    if let (Some(usage), Some(path)) = (Usage::from_status(&status, now()), usage_path()) {
        let usage = usage.merged(read_usage().as_ref());
        if let Ok(text) = serde_json::to_string(&usage) {
            let _ = write_atomic(&path, &text);
        }
    }
    // The status line set before Ronnie's, as it was.
    let previous = previous_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    if let Some(command) = previous.as_ref().and_then(|p| p.get("command")).and_then(Value::as_str) {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };
        if let Ok(mut child) = cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::inherit()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&input);
            }
            let _ = child.wait();
        }
        return;
    }
    println!("{}", status_text(&status));
}

/// The hooks Ronnie sets to tell when Claude is done, or asks the user (a permission, a question).
const NOTIFY_HOOKS: [(&str, Option<&str>); 2] = [("Stop", None), ("Notification", Some("permission_prompt|elicitation_dialog"))];
/// The hook telling Claude what Ronnie offers and the other projects, as its session starts (again
/// after a /clear or a compaction).
const CONTEXT_HOOK: (&str, Option<&str>) = ("SessionStart", None);
/// The hook noting what the user asks, for the other sessions (see `crate::worklog`).
const PROMPT_HOOK: (&str, Option<&str>) = ("UserPromptSubmit", None);

/// The hooks wanted: those telling when Claude is done (`notify`), the one telling Claude about Ronnie
/// (`context`), those sharing what the user works on between the sessions (`worklog`).
fn wanted_hooks(notify: bool, context: bool, worklog: bool) -> Vec<(&'static str, Option<&'static str>)> {
    let mut hooks = if notify { NOTIFY_HOOKS.to_vec() } else { Vec::new() };
    if context || worklog {
        hooks.push(CONTEXT_HOOK);
    }
    if worklog {
        hooks.push(PROMPT_HOOK);
    }
    hooks
}

fn is_our_hook(hook: &Value) -> bool {
    hook.get("command").and_then(Value::as_str).is_some_and(|c| c.trim_end().ends_with(" claude-hook"))
}

/// The commands of the hooks set for `event`.
fn hook_commands<'a>(settings: &'a serde_json::Map<String, Value>, event: &str) -> impl Iterator<Item = &'a Value> {
    let groups = settings.get("hooks").and_then(|h| h.get(event)).and_then(Value::as_array);
    groups.into_iter().flatten().filter_map(|g| g.get("hooks").and_then(Value::as_array)).flatten()
}

/// Ronnie's hooks taken out of the settings, the user's own kept (and nothing left empty).
fn strip_hooks(settings: &mut serde_json::Map<String, Value>) {
    let Some(Value::Object(hooks)) = settings.get_mut("hooks") else { return };
    for groups in hooks.values_mut() {
        let Value::Array(groups) = groups else { continue };
        for group in groups.iter_mut() {
            if let Some(Value::Array(list)) = group.get_mut("hooks") {
                list.retain(|h| !is_our_hook(h));
            }
        }
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|l| !l.is_empty()));
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
    if hooks.is_empty() {
        settings.remove("hooks");
    }
}

/// Claude Code is installed (its settings folder is there).
pub fn installed() -> bool {
    settings_path().and_then(|p| p.parent().map(std::path::Path::exists)).unwrap_or(false)
}

/// At launch (official builds, unless the user turned them off): Ronnie's hooks set, or their path
/// updated if the app moved. Nothing when Claude Code isn't installed.
pub fn ensure_hooks(notify: bool, context: bool, worklog: bool) -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let settings = read_settings()?;
    let command = ronnie_command("claude-hook");
    let set = |event: &str| hook_commands(&settings, event).any(|h| h.get("command").and_then(Value::as_str) == Some(command.as_str()));
    if wanted_hooks(notify, context, worklog).iter().all(|(event, _)| set(event)) {
        return Ok(());
    }
    set_hooks(notify, context, worklog)
}

/// Sets the hooks wanted (next to the user's own), and only those of Ronnie's.
pub fn set_hooks(notify: bool, context: bool, worklog: bool) -> Result<(), String> {
    let mut settings = read_settings()?;
    strip_hooks(&mut settings);
    let wanted = wanted_hooks(notify, context, worklog);
    if !wanted.is_empty() {
        let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
        let Value::Object(hooks) = hooks else { return Err("~/.claude/settings.json : \"hooks\" n'est pas un objet".into()) };
        for (event, matcher) in wanted {
            let mut group = json!({ "hooks": [{ "type": "command", "command": ronnie_command("claude-hook"), "timeout": 10 }] });
            if let Some(m) = matcher {
                group["matcher"] = json!(m);
            }
            match hooks.entry(event).or_insert_with(|| json!([])) {
                Value::Array(groups) => groups.push(group),
                other => *other = json!([group]),
            }
        }
    }
    write_settings(settings)?;
    crate::log::info(&format!("claude: hooks set (notify {notify}, context {context}, worklog {worklog})"));
    Ok(())
}

/// What a hook told: Claude is done, or waits for the user, in a pane of a Ronnie.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    /// The pane's RONNIE_PANE.
    pub pane: String,
    /// Claude asks something (a permission, a question) rather than being done.
    pub waiting: bool,
    /// What Claude asks, or the start of its last answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Event {
    /// From a hook's input: None for the events Ronnie doesn't tell.
    fn from_hook(input: &Value, pane: &str) -> Option<Self> {
        let text = |key: &str| input.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
        let (waiting, message) = match text("hook_event_name")? {
            "Stop" => (false, text("last_assistant_message")),
            "Notification" if matches!(text("notification_type"), Some("permission_prompt" | "elicitation_dialog")) => (true, text("message")),
            _ => return None,
        };
        // A line of it: the notification is short.
        let message = message.and_then(|m| m.lines().map(str::trim).find(|l| !l.is_empty())).map(|line| {
            let mut short: String = line.chars().take(140).collect();
            if short.len() < line.len() {
                short.push('…');
            }
            short
        });
        Some(Event { pane: pane.to_owned(), waiting, message })
    }
}

/// The variables a local pane gets, for the hook to reach this Ronnie: the pane's token, and where
/// to leave events (a folder of this process: another Ronnie on the same config has its own).
pub fn pane_env(token: &str) -> Vec<(&'static str, String)> {
    let mut env = vec![("RONNIE_PANE", token.to_owned())];
    if let Some(dir) = events_dir() {
        env.push(("RONNIE_CLAUDE_EVENTS", dir.display().to_string()));
    }
    env
}

fn events_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("claude-events").join(std::process::id().to_string()))
}

/// `ronnie claude-hook`: run by Claude Code with the hook's input on stdin. Outside a Ronnie pane,
/// nothing.
pub fn run_hook() {
    let Ok(pane) = std::env::var("RONNIE_PANE") else { return };
    let mut input = Vec::new();
    let _ = std::io::stdin().take(4 << 20).read_to_end(&mut input);
    let input: Value = serde_json::from_slice(&input).unwrap_or(Value::Null);
    let text = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or_default();
    let cwd = Some(PathBuf::from(text("cwd"))).filter(|c| c.is_absolute()).or_else(|| std::env::current_dir().ok());
    match text("hook_event_name") {
        "SessionStart" => {
            let config = crate::config::load_config().ok();
            // Each part as set (a hook left by an older setting tells nothing more).
            let settings = config.as_ref().map(|c| &c.settings);
            let mut parts = Vec::new();
            if settings.is_none_or(|s| s.claude_context) {
                let node = std::env::var_os("RONNIE_NODE_FILE").and_then(|f| std::fs::read_to_string(f).ok());
                let installed: Vec<(String, PathBuf)> = crate::node::installed().into_iter().filter_map(|v| Some((v.clone(), crate::node::bin_dir(&v)?))).collect();
                parts.push(context(config.as_ref(), node.as_deref().map(str::trim).filter(|b| !b.is_empty()), &installed));
            }
            if settings.is_none_or(|s| s.claude_worklog)
                && let Some(others) = cwd.as_deref().and_then(crate::worklog::session_started)
            {
                parts.push(others);
            }
            if !parts.is_empty() {
                println!("{}", json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": parts.join("\n\n") } }));
            }
            return;
        }
        // Noted only: nothing printed (it would be added to the request).
        "UserPromptSubmit" => {
            if let Some(cwd) = cwd
                && crate::config::load_config().is_ok_and(|c| c.settings.claude_worklog)
            {
                crate::worklog::prompted(&cwd, text("session_id"), text("prompt"));
            }
            return;
        }
        _ => {}
    }
    let Some(dir) = std::env::var_os("RONNIE_CLAUDE_EVENTS") else { return };
    let Some(event) = Event::from_hook(&input, &pane) else { return };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    if let Ok(text) = serde_json::to_string(&event) {
        let _ = write_atomic(&PathBuf::from(dir).join(format!("{nanos}-{}.json", std::process::id())), &text);
    }
}

/// What Claude is told about Ronnie as its session starts in a pane: what Ronnie offers, and where it is
/// in Ronnie (in the user's language), then what this pane and this Ronnie have (the pane's Node.js
/// version, `node_bin`, the versions installed, the mail catcher, the servers saved: their names only).
fn context(config: Option<&crate::config::Config>, node_bin: Option<&str>, installed: &[(String, PathBuf)]) -> String {
    let t = config.map_or_else(crate::i18n::Lang::default, |c| c.settings.language).strings();
    let port = config.map_or(crate::mailbox::DEFAULT_PORT, |c| c.settings.mail_port);
    let mut text = format!(
        "This Claude Code session runs in a terminal pane of Ronnie, a terminal app with development tools of its own. \
When one of them fits what the user is doing, suggest it (saying where it is in Ronnie) rather than an outside tool; \
don't list them unasked. Ronnie's labels below are in the user's language.

- Node.js versions, nvm-style, installed in Ronnie's own folder (nothing on the system): each local pane uses one, first in \
its PATH. The user picks it from the pane's bar (green Node badge, \"{node_manage}\") or in the \"{dev}\" category of \
the left column, which installs any nodejs.org version. When a project needs another version, ask the user to pick it in \
Ronnie rather than installing nvm, fnm, Volta or a system Node; for one command, an installed version's folder can be put \
first in PATH (`PATH=\"<bin folder>:$PATH\" npm test`{path_note}).
- A mail catcher (\"{dev}\" > \"{mail}\"): an SMTP server on 127.0.0.1:{port} (no TLS, any login accepted) that keeps \
every mail sent to it, delivering none, and shows them in Ronnie (HTML, text, headers, source, attachments). To test the \
mails of a project, point its SMTP settings at it instead of suggesting Mailpit or MailHog; the user starts it in Ronnie.
- SSH servers (\"{hosts}\"): saved with their login (key, password, jump host), imported from ~/.ssh/config; each opens \
a terminal and a file browser (upload, download, edit, permissions) with the server's health. You can reach them \
yourself: those logging in with a key take the `ssh` command given below (non-interactive, `ssh … 'command'`); those \
logging in with a password only go through Ronnie, so for them the user runs the commands in the server's terminal.
- Databases (\"{dbs}\"): MariaDB / MySQL and SQL Server, directly or through an SSH server; tables to browse and edit, \
an SQL editor with completion. You can query them too, with their client (`mysql`, `sqlcmd`), through an `ssh -L` port \
forward for those reached through a server; their passwords stay in Ronnie: take the project's own settings (`.env`…) \
or ask the user.
- When the user's question is about a server or a database, say you can look into it and do so, rather than saying \
you have no access. With care: reading (logs, status, files, SELECT) as needed, but anything changing a server or a \
database (writing or deleting files, UPDATE / DELETE / ALTER, migrations, restarts, deploys) is shown to the user \
first and run only once they agree, one change at a time; on production (a name with \"prod\"…) even more so.
- Terminals (\"{local}\"): tabs, splits and profiles (a project's tab saved with its folder and panes); beside them the \
profile's files, its git (changes, history, issues) and an editor. Ronnie tells the user when you are done or wait for \
them while this pane isn't on screen.",
        // Claude Code runs its commands in Git Bash there: C:\Users\… is written /c/Users/….
        path_note = if cfg!(windows) { "; in Git Bash, the folder written /c/Users/…" } else { "" },
        node_manage = t.node_manage,
        dev = t.cat_dev,
        mail = t.mail_title,
        hosts = t.home_hosts,
        dbs = t.home_databases,
        local = t.cat_local,
    );
    text.push_str("\n\nThis pane and this Ronnie now:");
    // The version is its folder's name: <versions>/v22.11.0/bin (on Windows, the version's folder itself).
    let pane_version = node_bin.map(|bin| {
        let path = std::path::Path::new(bin);
        let dir = if path.file_name().is_some_and(|n| n == "bin") { path.parent().unwrap_or(path) } else { path };
        dir.file_name().map_or_else(|| bin.to_owned(), |n| n.to_string_lossy().into_owned())
    });
    text.push_str(&match pane_version {
        Some(v) => format!("\n- This pane's Node.js: {v} (Ronnie's)."),
        None => "\n- This pane uses no Node.js version of Ronnie's (the system's, if any).".to_owned(),
    });
    if installed.is_empty() {
        text.push_str("\n- No Node.js version installed in Ronnie yet.");
    } else {
        let list: Vec<String> = installed.iter().map(|(v, bin)| format!("{v} ({})", bin.display())).collect();
        text.push_str(&format!("\n- Node.js versions installed in Ronnie, with their bin folder: {}.", list.join(", ")));
    }
    let running = config.is_some_and(|c| c.settings.mail_running);
    text.push_str(&format!("\n- The mail catcher is {} (port {port}).", if running { "running" } else { "stopped" }));
    if let Some(config) = config {
        if !config.ssh.is_empty() {
            text.push_str("\n- SSH servers saved in Ronnie:");
            for host in &config.ssh {
                text.push_str(&format!("\n  - {}: {}", host.name, ssh_line(host)));
            }
        }
        if !config.databases.is_empty() {
            text.push_str("\n- Databases saved in Ronnie:");
            for db in &config.databases {
                let mut line = format!("\n  - {} ({}): {}", db.name, db.engine.label(), db.address());
                if let Some(database) = &db.database {
                    line.push_str(&format!(", database {database}"));
                }
                if let Some(host) = db.ssh.and_then(|id| config.ssh.iter().find(|h| h.id == id)) {
                    line.push_str(&format!(", through the SSH server {} (host and port as seen from it)", host.name));
                }
                text.push_str(&line);
            }
        }
    }
    text
}

/// How Claude reaches a saved SSH host: the `ssh` command for one logging in with a key (or ssh's
/// defaults), without prompts; for one needing a password, the user's terminal in Ronnie.
fn ssh_line(host: &crate::ssh::SshHost) -> String {
    use crate::ssh::SshAuth;
    let key = host.identity_file.as_deref().filter(|_| matches!(host.auth_method(), SshAuth::Key));
    let password = match host.auth_method() {
        SshAuth::Password | SshAuth::Ask | SshAuth::Interactive => true,
        // A protected key: its passphrase is typed by Ronnie (as for a PuTTY key's copy).
        SshAuth::Key => host.password_saved || key.is_some_and(crate::ssh::is_putty_key),
        SshAuth::Auto => false,
    };
    if password {
        return format!("{} (password: through the user)", host.address());
    }
    let mut args = vec!["ssh".to_owned(), "-o".to_owned(), "BatchMode=yes".to_owned()];
    if let Some(port) = host.port.filter(|p| *p != 22) {
        args.extend(["-p".to_owned(), port.to_string()]);
    }
    if let Some(key) = key {
        args.extend(["-i".to_owned(), shell_word(&crate::ssh::expand_home(key).display().to_string()), "-o".to_owned(), "IdentitiesOnly=yes".to_owned()]);
    }
    if let Some(jump) = &host.jump {
        args.extend(["-J".to_owned(), shell_word(jump)]);
    }
    for option in &host.options {
        if let Some((key, value)) = option.trim().split_once(char::is_whitespace) {
            args.extend(["-o".to_owned(), shell_word(&format!("{key}={}", value.trim()))]);
        }
    }
    let target = match &host.user {
        Some(user) => format!("{user}@{}", host.host),
        None => host.host.clone(),
    };
    args.push(shell_word(&target));
    format!("`{}`", args.join(" "))
}

/// A word as the shell reads it: quoted when it holds anything but plain characters.
fn shell_word(word: &str) -> String {
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./_-~".contains(c)) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// Watches this Ronnie's events folder (twice a second): what comes goes to `queue`, and the window
/// is redrawn. Folders left by Ronnies no longer running go away (those are empty).
pub fn watch_events(ctx: egui::Context, queue: std::sync::Arc<std::sync::Mutex<Vec<(Event, std::time::Instant)>>>) {
    let Some(dir) = events_dir() else { return };
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(parent) = dir.parent() {
        for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
            let _ = std::fs::remove_dir(entry.path());
        }
    }
    let _ = std::thread::Builder::new().name("claude-events".into()).spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut events = Vec::new();
            for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")) {
                if let Some(event) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Event>(&t).ok()) {
                    events.push(event);
                }
                let _ = std::fs::remove_file(&path);
            }
            if !events.is_empty() {
                let now = std::time::Instant::now();
                queue.lock().unwrap().extend(events.into_iter().map(|e| (e, now)));
                ctx.request_repaint();
            }
        }
    });
}

/// Ronnie's own status line: the model, the context used, and the plan's usage.
fn status_text(status: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(model) = status.pointer("/model/display_name").and_then(Value::as_str) {
        parts.push(model.to_owned());
    }
    if let Some(ctx) = status.pointer("/context_window/used_percentage").and_then(Value::as_f64) {
        parts.push(format!("ctx {ctx:.0} %"));
    }
    if let Some(usage) = Usage::from_status(status, now()) {
        parts.push(usage_text(&usage, now()));
    }
    parts.join(" · ")
}

/// "5 h 23 % · 7 j 41 %".
pub fn usage_text(usage: &Usage, now: i64) -> String {
    let mut parts = Vec::new();
    if let Some(w) = usage.five_hour {
        parts.push(format!("5 h {:.0} %", w.used_now(now)));
    }
    if let Some(w) = usage.seven_day {
        parts.push(format!("7 j {:.0} %", w.used_now(now)));
    }
    parts.join(" · ")
}

/// Claude Code runs in the foreground (`claude`, or its native build's version-named binary).
pub fn is_claude(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program).trim_end_matches(".exe");
    name.eq_ignore_ascii_case("claude") || (name.split('.').count() == 3 && name.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_status_line_input() {
        let status = json!({
            "model": { "display_name": "Opus" },
            "context_window": { "used_percentage": 12.4 },
            "rate_limits": { "five_hour": { "used_percentage": 23.5, "resets_at": 2000 }, "seven_day": { "used_percentage": 41.2, "resets_at": 9000 } },
        });
        let usage = Usage::from_status(&status, 1000).unwrap();
        assert_eq!(usage.five_hour, Some(Window { used_percentage: 23.5, resets_at: 2000 }));
        assert_eq!(usage_text(&usage, 1000), "5 h 24 % · 7 j 41 %");
        // Past its reset: nothing used yet in the new window.
        assert_eq!(usage_text(&usage, 5000), "5 h 0 % · 7 j 41 %");
        assert!(Usage::from_status(&json!({ "model": {} }), 1000).is_none());
        assert!(status_text(&status).starts_with("Opus · ctx 12 % · 5 h"));
        assert!(is_claude("claude") && is_claude("2.1.286") && !is_claude("node") && !is_claude("vim"));
        assert!(is_ours(Some(&json!({ "type": "command", "command": "/Applications/Ronnie.app/Contents/MacOS/ronnie claude-statusline" }))));
        assert!(!is_ours(Some(&json!({ "type": "command", "command": "~/bin/status.sh" }))));
    }

    #[test]
    fn keeps_the_latest_usage() {
        let w = |used_percentage: f32, resets_at: i64| Some(Window { used_percentage, resets_at });
        let usage = |five_hour, seven_day| Usage { five_hour, seven_day, updated: 0 };
        // An idle session telling its old figures again: the higher ones of the same window stay.
        let kept = usage(w(40.0, 18000), w(12.0, 600000));
        let merged = usage(w(25.0, 18000), w(11.0, 600030)).merged(Some(&kept));
        assert_eq!((merged.five_hour, merged.seven_day), (w(40.0, 18000), w(12.0, 600030)));
        // A session still telling the window before: the later one kept.
        let merged = usage(w(90.0, 0), w(12.0, 600000)).merged(Some(&kept));
        assert_eq!(merged.five_hour, w(40.0, 18000));
        // A new window: its figures, lower as they may be.
        let merged = usage(w(3.0, 36000), None).merged(Some(&kept));
        assert_eq!((merged.five_hour, merged.seven_day), (w(3.0, 36000), w(12.0, 600000)));
        assert_eq!(usage(w(3.0, 36000), None).merged(None).five_hour, w(3.0, 36000));
    }

    #[test]
    fn reads_the_hooks_input() {
        let stop = json!({ "hook_event_name": "Stop", "last_assistant_message": "\n  C'est fait : les tests passent.\nDétails…" });
        assert_eq!(Event::from_hook(&stop, "p"), Some(Event { pane: "p".into(), waiting: false, message: Some("C'est fait : les tests passent.".into()) }));
        let ask = json!({ "hook_event_name": "Notification", "notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash" });
        assert!(Event::from_hook(&ask, "p").is_some_and(|e| e.waiting));
        // Idle for a minute after its answer: already told by Stop.
        assert!(Event::from_hook(&json!({ "hook_event_name": "Notification", "notification_type": "idle_prompt" }), "p").is_none());
        assert!(Event::from_hook(&json!({ "hook_event_name": "Stop" }), "p").is_some_and(|e| e.message.is_none()));
    }

    #[test]
    fn keeps_the_users_hooks() {
        let mut settings = json!({
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "/x/ronnie claude-hook" }, { "type": "command", "command": "say done" }] }],
                "Notification": [{ "matcher": "permission_prompt", "hooks": [{ "type": "command", "command": "'/a b/ronnie' claude-hook" }] }],
            },
            "model": "opus",
        });
        let Value::Object(map) = &mut settings else { unreachable!() };
        strip_hooks(map);
        assert_eq!(settings, json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }] }, "model": "opus" }));
        assert_eq!(wanted_hooks(false, true, false), vec![("SessionStart", None)]);
        assert_eq!(wanted_hooks(true, false, false).len(), 2);
        assert_eq!(wanted_hooks(false, false, true), vec![("SessionStart", None), ("UserPromptSubmit", None)]);
    }

    #[test]
    fn tells_claude_about_ronnie() {
        let installed = [("v22.11.0".to_owned(), PathBuf::from("/r/node/versions/v22.11.0/bin")), ("v18.20.4".to_owned(), PathBuf::from("/r/node/versions/v18.20.4/bin"))];
        let text = context(None, Some("/r/node/versions/v22.11.0/bin"), &installed);
        assert!(text.contains("127.0.0.1:1025"));
        assert!(text.contains("This pane's Node.js: v22.11.0 (Ronnie's)."));
        assert!(text.contains("v18.20.4 (/r/node/versions/v18.20.4/bin)"));
        assert!(text.contains("mail catcher is stopped"));
        let text = context(None, None, &[]);
        assert!(text.contains("no Node.js version of Ronnie's") && text.contains("No Node.js version installed"));
        let mut host = crate::ssh::SshHost::new();
        (host.host, host.user, host.port, host.jump) = ("example.org".into(), Some("deploy".into()), Some(2222), Some("bastion".into()));
        assert_eq!(ssh_line(&host), "`ssh -o BatchMode=yes -p 2222 -J bastion deploy@example.org`");
        host.auth = Some(crate::ssh::SshAuth::Password);
        assert_eq!(ssh_line(&host), "deploy@example.org:2222 (password: through the user)");
        assert_eq!(shell_word("/a b/it's"), "'/a b/it'\\''s'");
    }
}
