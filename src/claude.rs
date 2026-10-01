//! Claude's plan usage, shown above the panes where Claude Code runs. Claude Code hands its status line
//! command the usage of the 5-hour session and of the week (claude.ai Pro and Max plans, see
//! https://code.claude.com/docs/en/statusline): `ronnie claude-statusline` is that command. It keeps
//! them in claude-usage.json for the window, then prints the status line, or runs the one configured
//! before Ronnie's (kept aside, and put back when Ronnie's is removed).

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
    let exe = crate::update::appimage().or_else(|| std::env::current_exe().ok()).map(|p| p.display().to_string()).unwrap_or_else(|| "ronnie".into());
    let quoted = if cfg!(windows) {
        format!("\"{exe}\"")
    } else if exe.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        exe
    } else {
        format!("'{}'", exe.replace('\'', "'\\''"))
    };
    format!("{quoted} claude-statusline")
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
}
