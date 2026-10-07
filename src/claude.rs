//! Claude's plan usage, shown above the panes where Claude Code runs. Claude Code hands its status line
//! command the usage of the 5-hour session and of the week (claude.ai Pro and Max plans, see
//! https://code.claude.com/docs/en/statusline): `ronnie claude-statusline` is that command. It keeps
//! them in claude-usage.json for the window, then prints the status line, or runs the one configured
//! before Ronnie's (kept aside, and put back when Ronnie's is removed).
//!
//! Claude Code's hooks tell when Claude is done or waits for the user: `ronnie claude-hook` leaves an
//! event for the Ronnie whose pane it runs in (RONNIE_PANE, RONNIE_CLAUDE_EVENTS), which tells the user
//! when that pane isn't on screen.

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
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn usage_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("claude-usage.json"))
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
fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
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
    if !settings_path().and_then(|p| p.parent().map(std::path::Path::exists)).unwrap_or(false) {
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

/// The hooks Ronnie sets: Claude is done, or asks the user (a permission, a question).
const HOOKS: [(&str, Option<&str>); 2] = [("Stop", None), ("Notification", Some("permission_prompt|elicitation_dialog"))];

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

/// At launch (official builds, unless the user turned it off): Ronnie's hooks set, or their path
/// updated if the app moved. Nothing when Claude Code isn't installed.
pub fn ensure_hooks() -> Result<(), String> {
    if !settings_path().and_then(|p| p.parent().map(std::path::Path::exists)).unwrap_or(false) {
        return Ok(());
    }
    let settings = read_settings()?;
    let command = ronnie_command("claude-hook");
    let set = |event: &str| hook_commands(&settings, event).any(|h| h.get("command").and_then(Value::as_str) == Some(command.as_str()));
    if HOOKS.iter().all(|(event, _)| set(event)) {
        return Ok(());
    }
    connect_hooks()
}

/// Sets Ronnie's hooks (next to the user's own).
pub fn connect_hooks() -> Result<(), String> {
    let mut settings = read_settings()?;
    strip_hooks(&mut settings);
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    let Value::Object(hooks) = hooks else { return Err("~/.claude/settings.json : \"hooks\" n'est pas un objet".into()) };
    for (event, matcher) in HOOKS {
        let mut group = json!({ "hooks": [{ "type": "command", "command": ronnie_command("claude-hook"), "timeout": 10 }] });
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        match hooks.entry(event).or_insert_with(|| json!([])) {
            Value::Array(groups) => groups.push(group),
            other => *other = json!([group]),
        }
    }
    write_settings(settings)?;
    crate::log::info("claude: hooks set");
    Ok(())
}

/// Takes Ronnie's hooks out.
pub fn disconnect_hooks() -> Result<(), String> {
    let mut settings = read_settings()?;
    strip_hooks(&mut settings);
    write_settings(settings)?;
    crate::log::info("claude: hooks removed");
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
    let (Some(dir), Some(pane)) = (std::env::var_os("RONNIE_CLAUDE_EVENTS"), std::env::var("RONNIE_PANE").ok()) else { return };
    let mut input = Vec::new();
    let _ = std::io::stdin().take(4 << 20).read_to_end(&mut input);
    let input: Value = serde_json::from_slice(&input).unwrap_or(Value::Null);
    let Some(event) = Event::from_hook(&input, &pane) else { return };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    if let Ok(text) = serde_json::to_string(&event) {
        let _ = write_atomic(&PathBuf::from(dir).join(format!("{nanos}-{}.json", std::process::id())), &text);
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
    }
}
