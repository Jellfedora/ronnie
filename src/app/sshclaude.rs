//! Claude Code on an SSH server, from the Claude button of its pane, leaving no conversation there.
//!
//! On the server, Claude Code runs with its own folder, `~/.ronnie-claude`, which keeps only the login
//! (`.credentials.json`) and Claude Code's settings (`.claude.json`, without the folders it was used
//! in). Each run gets a folder of its own in `runs/<id>` (the login copied in), where Claude Code writes
//! its conversations. Ronnie sends a small script there and types `sh …/go` at the pane's prompt; the
//! script installs Claude Code if it is missing (claude.ai's installer, in `~/.local/bin`), runs it, and
//! says when it ends with an escape sequence (`terminal::osc`). Ronnie then brings back the runs that
//! ended (none still running: their script's pid is gone): their conversations go to this machine, in
//! `ssh-claude/<host>` of Ronnie's folder, the login refreshed by Claude Code is kept, and the run's
//! folder is deleted. Runs left behind (connection lost, Ronnie closed) are brought back at the next
//! click. A conversation taken up again is sent back to its run, and comes back with it.
//!
//! The conversations brought back are listed, renamed and deleted like local ones (`history`).

use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::ssh::{SshHost, sh_quote};

/// Claude Code's folder on the server, in its home.
const BASE: &str = ".ronnie-claude";
/// What the terminal clears before typing (the end of the line, then all of it), as for local panes.
const CLEAR: &str = "\x05\x15";

/// Where the conversations brought back from host `id` are kept, as Claude Code keeps its own
/// (`projects/<folder>/<id>.jsonl`).
pub(super) fn root(id: Uuid) -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("ssh-claude").join(id.to_string()))
}

/// A run's id: a uuid's characters, nothing else (it goes in paths and scripts).
fn valid_run(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// What happened, for the window.
pub(super) enum Event {
    /// The run is ready: this is to type in the pane.
    Ready { pane: crate::pane::PaneId, typed: String },
    Failed(String),
}

#[derive(Default)]
struct Shared {
    events: Vec<Event>,
    /// The servers' home folders, learned at a run's start (to place a `~/…` folder).
    homes: HashMap<Uuid, String>,
    /// Hosts being swept, and whether to sweep again once done.
    sweeping: HashMap<Uuid, bool>,
    /// The panes where a run was started and hasn't said its end yet.
    running: std::collections::HashSet<crate::pane::PaneId>,
}

#[derive(Default)]
pub(super) struct Remote {
    shared: Arc<Mutex<Shared>>,
}

/// The texts the script prints, in the user's language.
pub(super) struct Texts {
    pub installing: &'static str,
    pub missing: &'static str,
}

impl Remote {
    /// What happened since last asked.
    pub(super) fn take(&self) -> Vec<Event> {
        self.shared.lock().map(|mut s| std::mem::take(&mut s.events)).unwrap_or_default()
    }

    /// Claude Code was started in `pane` and hasn't ended yet.
    pub(super) fn runs_in(&self, pane: crate::pane::PaneId) -> bool {
        self.shared.lock().is_ok_and(|s| s.running.contains(&pane))
    }

    /// The run in `pane` ended.
    pub(super) fn ended(&self, pane: crate::pane::PaneId) {
        if let Ok(mut s) = self.shared.lock() {
            s.running.remove(&pane);
        }
    }

    pub(super) fn fail(&self, text: String) {
        if let Ok(mut s) = self.shared.lock() {
            s.events.push(Event::Failed(text));
        }
    }

    /// Folder `dir` on host `id` as an absolute path (`~` replaced, once its home is known).
    pub(super) fn absolute(&self, id: Uuid, dir: &str) -> Option<String> {
        if dir.starts_with('/') {
            return Some(dir.to_owned());
        }
        let rest = dir.strip_prefix('~')?;
        let home = self.shared.lock().ok()?.homes.get(&id)?.clone();
        Some(format!("{}{rest}", home.trim_end_matches('/')))
    }

    /// Claude Code started in `pane`, taking up conversation `session` (its file in `resume`), once the
    /// runs left behind are brought back.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start(&self, ctx: &egui::Context, host: SshHost, askpass: crate::askpass::Server, pane: crate::pane::PaneId, session: Option<String>, resume: Option<PathBuf>, texts: Texts) {
        let Some(root) = root(host.id) else { return };
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            sweep_now(&shared, &host, &askpass, &root);
            let run = Uuid::new_v4().to_string();
            let session = session.filter(|s| valid_run(s));
            let resume = resume.filter(|_| session.is_some()).and_then(|p| std::fs::read(p).ok());
            let go = go_script(&run, session.as_deref().filter(|_| resume.is_some()), &texts);
            let event = match prepare(&host, &askpass, &run, &go, resume.as_deref()) {
                Ok(home) => {
                    if let Ok(mut s) = shared.lock() {
                        if let Some(home) = home {
                            s.homes.insert(host.id, home);
                        }
                        s.running.insert(pane);
                    }
                    Event::Ready { pane, typed: format!("{CLEAR} sh ~/{BASE}/runs/{run}/go\r") }
                }
                Err(e) => Event::Failed(e),
            };
            if let Ok(mut s) = shared.lock() {
                s.events.push(event);
            }
            ctx.request_repaint();
        });
    }

    /// The runs that ended on `host` brought back and wiped.
    pub(super) fn sweep(&self, ctx: &egui::Context, host: SshHost, askpass: crate::askpass::Server) {
        let Some(root) = root(host.id) else { return };
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            sweep_now(&shared, &host, &askpass, &root);
            ctx.request_repaint();
        });
    }
}

/// Sweeps `host`, again if asked meanwhile; another sweep of it running: it will.
fn sweep_now(shared: &Arc<Mutex<Shared>>, host: &SshHost, askpass: &crate::askpass::Server, root: &Path) {
    {
        let Ok(mut s) = shared.lock() else { return };
        if let Some(again) = s.sweeping.get_mut(&host.id) {
            *again = true;
            return;
        }
        s.sweeping.insert(host.id, false);
    }
    loop {
        if let Err(e) = sweep(host, askpass, root) {
            crate::log::info(&format!("claude on {}: {e}", host.host));
            if let Ok(mut s) = shared.lock() {
                s.events.push(Event::Failed(e));
            }
        }
        let Ok(mut s) = shared.lock() else { return };
        if s.sweeping.get(&host.id) == Some(&true) {
            s.sweeping.insert(host.id, false);
        } else {
            s.sweeping.remove(&host.id);
            return;
        }
    }
}

/// Runs `script` with sh on the server, `input` on its standard input; its output.
fn run(host: &SshHost, askpass: &crate::askpass::Server, script: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    let launch = host.exec_command(&format!("sh -c {}", sh_quote(script)));
    let mut cmd = std::process::Command::new(&launch.program);
    cmd.args(&launch.args).envs(launch.env.iter().cloned()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("ssh : {e}"))?;
    askpass.allow_interactive(child.id(), host);
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_owned();
        return Err(if line.is_empty() { format!("ssh : {}", out.status) } else { line });
    }
    Ok(out.stdout)
}

/// The run's folder made, its script (and the conversation taken up) put in; the server's home.
fn prepare(host: &SshHost, askpass: &crate::askpass::Server, run_id: &str, go: &str, resume: Option<&[u8]>) -> Result<Option<String>, String> {
    let mut archive = tar::Builder::new(Vec::new());
    let mut add = |name: &str, data: &[u8], mode: u32| {
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_mtime(chrono::Utc::now().timestamp().max(0) as u64);
        header.set_cksum();
        archive.append_data(&mut header, name, data).map_err(|e| e.to_string())
    };
    add("go", go.as_bytes(), 0o700)?;
    if let Some(resume) = resume {
        add("resume.jsonl", resume, 0o600)?;
    }
    let tar = archive.into_inner().map_err(|e| e.to_string())?;
    let script = format!("umask 077\nB=\"$HOME/{BASE}\"\nmkdir -p \"$B/runs/{run_id}\" && chmod 700 \"$B\" && cd \"$B/runs/{run_id}\" && tar xf - || exit 1\necho \"$HOME\"");
    let out = run(host, askpass, &script, &tar)?;
    Ok(String::from_utf8_lossy(&out).lines().rev().map(str::trim).find(|l| l.starts_with('/')).map(str::to_owned))
}

/// The script of a run: its pid noted (alive: the run isn't swept), the login copied in, Claude Code
/// installed if missing, run in the shell's folder (taking up `session`, put where Claude Code looks for
/// it), then the end said to Ronnie.
fn go_script(run: &str, session: Option<&str>, texts: &Texts) -> String {
    let resume = match session {
        Some(id) => format!("  P=\"$R/projects/$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')\"\n  mkdir -p \"$P\" && mv -f \"$R/resume.jsonl\" \"$P/{id}.jsonl\"\n  CLAUDE_CONFIG_DIR=\"$R\" \"$c\" --resume {id}\n"),
        None => "  CLAUDE_CONFIG_DIR=\"$R\" \"$c\"\n".to_owned(),
    };
    format!(
        "B=\"$HOME/{BASE}\"\nR=\"$B/runs/{run}\"\necho $$ > \"$R/pid\"\n\
        for f in .credentials.json .claude.json; do [ -f \"$B/$f\" ] && cp -p \"$B/$f\" \"$R/$f\"; done\n\
        c=$(command -v claude 2>/dev/null) || c=\"$HOME/.local/bin/claude\"\n\
        if [ ! -x \"$c\" ]; then\n  echo {installing}\n  if command -v curl >/dev/null 2>&1; then curl -fsSL https://claude.ai/install.sh | bash; else wget -qO- https://claude.ai/install.sh | bash; fi\n  c=\"$HOME/.local/bin/claude\"\nfi\n\
        if [ -x \"$c\" ]; then\n{resume}else\n  echo {missing}\nfi\n\
        printf '\\033]1337;RonnieClaude=%s\\007' {run}\n",
        installing = sh_quote(texts.installing),
        missing = sh_quote(texts.missing),
    )
}

/// Lists the runs that ended (their script's pid gone; none yet: made more than 10 minutes ago) and
/// prints, as a tar archive, what is kept of them: their script, conversations and settings.
const SWEEP: &str = r#"cd "$HOME/.ronnie-claude/runs" 2>/dev/null || exit 0
L=
for r in *; do
  [ -d "$r" ] || continue
  if [ -f "$r/pid" ]; then
    kill -0 "$(cat "$r/pid")" 2>/dev/null && continue
  elif [ -n "$(find "$r" -maxdepth 0 -mmin -10 2>/dev/null)" ]; then
    continue
  fi
  L="$L $r/go"
  [ -d "$r/projects" ] && L="$L $r/projects"
  [ -f "$r/.claude.json" ] && L="$L $r/.claude.json"
done
[ -z "$L" ] || exec tar cf - $L"#;

/// Deletes `runs`, keeping the newest login among them and the settings read on the standard input
/// (if any).
fn wipe_script(runs: &BTreeSet<String>) -> String {
    let list = runs.iter().filter(|r| valid_run(r)).cloned().collect::<Vec<_>>().join(" ");
    format!(
        "umask 077\nB=\"$HOME/{BASE}\"\n\
        for r in {list}; do f=\"$B/runs/$r/.credentials.json\"; [ -f \"$f\" ] && {{ [ ! -f \"$B/.credentials.json\" ] || [ \"$f\" -nt \"$B/.credentials.json\" ]; }} && cp -p \"$f\" \"$B/.credentials.json\"; done\n\
        cat > \"$B/.claude.json.new\"\n\
        if [ -s \"$B/.claude.json.new\" ]; then mv -f \"$B/.claude.json.new\" \"$B/.claude.json\"; else rm -f \"$B/.claude.json.new\"; fi\n\
        for r in {list}; do rm -rf \"$B/runs/$r\"; done"
    )
}

/// Claude Code's settings without the folders it was used in.
fn clean_settings(data: &[u8]) -> Option<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(data).ok()?;
    let map = value.as_object_mut()?;
    for key in ["projects", "githubRepoPaths"] {
        map.remove(key);
    }
    serde_json::to_vec_pretty(&value).ok()
}

impl super::App {
    /// What the runs on servers did: the line typed in their pane once it is ready, a failure told.
    pub(super) fn ssh_claude_poll(&mut self) {
        for event in self.ssh_claude.take() {
            match event {
                Event::Ready { pane, typed } => {
                    self.claude_typing.push((pane, typed, std::time::Instant::now()));
                    self.focus_terminal = true;
                }
                Event::Failed(body) => {
                    let title = self.t().ssh_claude_failed.to_owned();
                    self.push_toast(super::Toast { ok: false, title, body, tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
                }
            }
        }
    }
}

/// The runs that ended on `host`: their conversations kept in `root`, then wiped from the server.
fn sweep(host: &SshHost, askpass: &crate::askpass::Server, root: &Path) -> Result<usize, String> {
    let tar = run(host, askpass, SWEEP, b"")?;
    if tar.is_empty() {
        return Ok(0);
    }
    let (runs, settings) = unpack(&tar, root)?;
    let settings = settings.as_deref().and_then(clean_settings).unwrap_or_default();
    run(host, askpass, &wipe_script(&runs), &settings)?;
    Ok(runs.len())
}

/// The sweep's archive opened: conversations written in `root/projects`; the runs found, and the newest
/// settings among them.
fn unpack(tar: &[u8], root: &Path) -> Result<(BTreeSet<String>, Option<Vec<u8>>), String> {
    let mut runs = BTreeSet::new();
    let mut settings: Option<(u64, Vec<u8>)> = None;
    let mut archive = tar::Archive::new(tar);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        // Plain names only: nothing outside `root`.
        let Some(parts) = path.components().map(|c| if let Component::Normal(p) = c { Some(p.to_string_lossy().into_owned()) } else { None }).collect::<Option<Vec<_>>>() else { continue };
        let Some(run) = parts.first().filter(|r| valid_run(r)) else { continue };
        runs.insert(run.clone());
        let file = entry.header().entry_type().is_file();
        match parts.get(1).map(String::as_str) {
            Some("projects") if file && parts.len() > 2 => {
                let dest = parts[1..].iter().fold(root.to_path_buf(), |p, part| p.join(part));
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&dest, data).map_err(|e| e.to_string())?;
            }
            Some(".claude.json") if file && parts.len() == 2 => {
                let at = entry.header().mtime().unwrap_or(0);
                if settings.as_ref().is_none_or(|(t, _)| at >= *t) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
                    settings = Some((at, data));
                }
            }
            _ => {}
        }
    }
    Ok((runs, settings.map(|(_, d)| d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o600);
            h.set_cksum();
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap()
    }

    #[test]
    fn brings_back_the_conversations_only() {
        let root = std::env::temp_dir().join(format!("ronnie-sshclaude-{}", Uuid::new_v4()));
        let run = "0b1c2d3e-0000-4000-8000-000000000001";
        let tar = archive(&[
            (&format!("{run}/go"), b"#"),
            (&format!("{run}/projects/-home-joe-app/abc-1.jsonl"), b"{}\n"),
            (&format!("{run}/projects/-home-joe-app/abc-1/subagents/x.jsonl"), b"{}\n"),
            (&format!("{run}/.claude.json"), br#"{"theme":"dark","projects":{"/home/joe/app":{}}}"#),
            ("not-a-run!/projects/x.jsonl", b"no"),
        ]);
        let (runs, settings) = unpack(&tar, &root).unwrap();
        assert_eq!(runs.into_iter().collect::<Vec<_>>(), vec![run.to_owned()]);
        assert!(root.join("projects/-home-joe-app/abc-1.jsonl").is_file());
        assert!(root.join("projects/-home-joe-app/abc-1/subagents/x.jsonl").is_file());
        let clean: serde_json::Value = serde_json::from_slice(&clean_settings(&settings.unwrap()).unwrap()).unwrap();
        assert_eq!(clean, serde_json::json!({ "theme": "dark" }));
        // Listed like local conversations, in the server's folder.
        assert!(crate::claude::has_sessions(Some(&root), Path::new("/home/joe/app")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The scripts run here, as the server would: a fake `claude` writing a conversation, its login
    /// refreshed and its settings.
    #[cfg(unix)]
    #[test]
    fn a_run_from_start_to_wipe() {
        let tmp = std::env::temp_dir().join(format!("ronnie-sshclaude-run-{}", Uuid::new_v4()));
        let (home, bin, root) = (tmp.join("home"), tmp.join("bin"), tmp.join("kept"));
        std::fs::create_dir_all(home.join(BASE)).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join("app")).unwrap();
        std::fs::write(home.join(BASE).join(".credentials.json"), "old").unwrap();
        let claude = "#!/bin/sh\nset -e\nd=\"$CLAUDE_CONFIG_DIR/projects/$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')\"\nmkdir -p \"$d\"\n[ \"$(cat \"$CLAUDE_CONFIG_DIR/.credentials.json\")\" = old ]\nsleep 1\necho '{\"type\":\"user\"}' >> \"$d/${2:-s1}.jsonl\"\necho new > \"$CLAUDE_CONFIG_DIR/.credentials.json\"\necho '{\"theme\":\"d\",\"projects\":{\"/app\":{}}}' > \"$CLAUDE_CONFIG_DIR/.claude.json\"\n";
        std::fs::write(bin.join("claude"), claude).unwrap();
        std::process::Command::new("chmod").arg("+x").arg(bin.join("claude")).status().unwrap();
        let sh = |script: &str, input: &[u8], dir: &Path| {
            let mut child = std::process::Command::new("sh").arg("-c").arg(script).current_dir(dir).env("HOME", &home).env("PATH", format!("{}:/usr/bin:/bin", bin.display())).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "{script}");
            out.stdout
        };
        let texts = Texts { installing: "installing", missing: "missing" };
        let run_id = Uuid::new_v4().to_string();
        let go = go_script(&run_id, None, &texts);
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_ustar();
        h.set_size(go.len() as u64);
        h.set_mode(0o700);
        h.set_cksum();
        b.append_data(&mut h, "go", go.as_bytes()).unwrap();
        let prep = format!("umask 077\nB=\"$HOME/{BASE}\"\nmkdir -p \"$B/runs/{run_id}\" && chmod 700 \"$B\" && cd \"$B/runs/{run_id}\" && tar xf - || exit 1\necho \"$HOME\"");
        sh(&prep, &b.into_inner().unwrap(), &tmp);
        let out = sh(&format!("sh ~/{BASE}/runs/{run_id}/go"), b"", &home.join("app"));
        assert!(String::from_utf8_lossy(&out).contains(&format!("\x1b]1337;RonnieClaude={run_id}\x07")));

        let (runs, settings) = unpack(&sh(SWEEP, b"", &tmp), &root).unwrap();
        assert_eq!(runs.len(), 1);
        let app = std::fs::canonicalize(home.join("app")).unwrap();
        assert!(crate::claude::has_sessions(Some(&root), &app));
        sh(&wipe_script(&runs), &clean_settings(&settings.unwrap()).unwrap(), &tmp);
        let base = home.join(BASE);
        assert_eq!(std::fs::read_to_string(base.join(".credentials.json")).unwrap().trim(), "new");
        assert!(!std::fs::read_to_string(base.join(".claude.json")).unwrap().contains("projects"));
        assert_eq!(std::fs::read_dir(base.join("runs")).unwrap().count(), 0);
        // Nothing more to bring back.
        assert!(sh(SWEEP, b"", &tmp).is_empty());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn scripts_quote_and_check() {
        let texts = Texts { installing: "l'installation…", missing: "absent" };
        let go = go_script("abc-1", Some("def-2"), &texts);
        assert!(go.contains("'l'\\''installation…'") && go.contains("--resume def-2") && go.contains("RonnieClaude=%s"));
        let wipe = wipe_script(&["abc-1".to_owned(), "x; rm -rf /".to_owned()].into());
        assert!(wipe.contains("for r in abc-1;") && !wipe.contains("rm -rf /"));
    }
}
