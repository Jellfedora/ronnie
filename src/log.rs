//! A small log file in the config directory (ronnie.log), and crash reports: enough to understand a
//! problem after the fact, including on Windows where release builds have no console.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Past this size the log starts over (the previous one is kept as ronnie.log.old).
const MAX_LOG: u64 = 1024 * 1024;

fn log_path() -> Option<PathBuf> {
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
