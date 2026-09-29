//! A small log file in the config directory (ronnie.log), and crash reports: enough to understand a
//! problem after the fact, including on Windows where release builds have no console.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Past this size the log starts over (the previous one is kept as ronnie.log.old).
const MAX_LOG: u64 = 1024 * 1024;

pub fn log_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("ronnie.log"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn write(level: &str, message: &str) {
    let Some(path) = log_path() else { return };
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    if let Ok(mut file) = options.open(&path) {
        let _ = writeln!(file, "{} {level} {message}", now());
    }
}

pub fn info(message: &str) {
    write("INFO", message);
}

pub fn error(message: &str) {
    write("ERROR", message);
}

/// Writes panics to crash-<time>.log (and the log) before the default handler runs.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = format!("Ronnie {} crashed: {info}\n\n{}", crate::update::VERSION, std::backtrace::Backtrace::force_capture());
        if let Some(dir) = crate::config::config_dir() {
            let mut options = fs::OpenOptions::new();
            options.create(true).write(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            if let Ok(mut file) = options.open(dir.join(format!("crash-{}.log", now()))) {
                let _ = file.write_all(report.as_bytes());
            }
        }
        error(&report);
        default(info);
    }));
}

/// A line of the log, for the settings page.
pub struct Entry {
    /// When it was written (seconds since 1970); none for the rest of a message spanning lines.
    pub time: Option<i64>,
    pub error: bool,
    pub text: String,
}

/// The log's lines, oldest first: those of the previous file, then of the current one.
pub fn entries() -> Vec<Entry> {
    let Some(path) = log_path() else { return Vec::new() };
    let old = fs::read_to_string(path.with_extension("log.old")).unwrap_or_default();
    let current = fs::read_to_string(&path).unwrap_or_default();
    old.lines().chain(current.lines()).filter(|l| !l.trim().is_empty()).map(parse).collect()
}

/// "<seconds> <LEVEL> <message>", as `write` puts it.
fn parse(line: &str) -> Entry {
    let mut parts = line.splitn(3, ' ');
    let (time, level, text) = (parts.next(), parts.next(), parts.next());
    match (time.and_then(|t| t.parse().ok()), level, text) {
        (Some(time), Some(level @ ("INFO" | "ERROR")), text) => Entry { time: Some(time), error: level == "ERROR", text: text.unwrap_or_default().to_owned() },
        _ => Entry { time: None, error: false, text: line.to_owned() },
    }
}

/// Empties the log, the previous file included.
pub fn clear() -> std::io::Result<()> {
    let Some(path) = log_path() else { return Ok(()) };
    for file in [path.with_extension("log.old"), path] {
        match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines() {
        let e = parse("1790667376 ERROR Trousseau : mot de passe introuvable");
        assert_eq!((e.time, e.error, e.text.as_str()), (Some(1790667376), true, "Trousseau : mot de passe introuvable"));
        let e = parse("   at src/main.rs:12");
        assert_eq!((e.time, e.error, e.text.as_str()), (None, false, "   at src/main.rs:12"));
    }
}
