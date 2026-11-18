//! Anonymous usage statistics, sent to floor (on unless turned off in the settings): how many times
//! each feature served a day, the launches, the minutes Ronnie was open, the version and the system,
//! and the crashes. Tied to an id drawn at random for this installation (telemetry.json), never to the
//! account, and without names, paths, hosts or contents: events are fixed names (`count("ssh.connect")`).
//!
//! The counts are kept per day in telemetry.json and sent every half-hour as the day's totals (floor
//! replaces the previous ones); a past day is dropped once floor has it. Crash files (crash-*.log) are
//! sent once, the home folder taken out.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How often the counts go to floor.
const SEND_EVERY: Duration = Duration::from_secs(30 * 60);
/// Days sent at most at once (floor takes as many).
const DAYS_MAX: usize = 31;
/// Crashes sent at most at once, and older ones left aside.
const CRASHES_MAX: usize = 5;
const CRASH_DAYS: i64 = 30;
/// A crash report: so many lines at most (the rest of the backtrace says little in a release build).
const CRASH_LINES: usize = 40;

static ON: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<Stored>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Stored {
    /// This installation, drawn at random.
    install: Option<uuid::Uuid>,
    /// The counts not yet known to floor, by day ("2026-11-18", local).
    days: BTreeMap<String, Day>,
    /// The crash files already sent.
    crashes_sent: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Day {
    launches: u32,
    minutes: u32,
    events: BTreeMap<String, u32>,
}

fn path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("telemetry.json"))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The stored state, loaded the first time.
fn with<R>(f: impl FnOnce(&mut Stored) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(|| {
        path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    });
    f(state)
}

fn save_locked(state: &Stored) {
    let Some(path) = path() else { return };
    if let Ok(bytes) = serde_json::to_vec_pretty(state) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// One more use of `event` today (a fixed name: `category.notes`, `ssh.connect`…). Nothing when off.
pub fn count(event: &str) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    with(|s| {
        *s.days
            .entry(today())
            .or_default()
            .events
            .entry(event.to_owned())
            .or_default() += 1
    });
}

/// Turned on or off (the setting). Off: what was not sent yet is forgotten.
pub fn set_enabled(on: bool) {
    if ON.swap(on, Ordering::Relaxed) == on {
        return;
    }
    if !on {
        with(|s| {
            s.days.clear();
            save_locked(s);
        });
    }
}

/// At launch: counts it, then every minute open, and sends the counts every half-hour.
pub fn start(on: bool) {
    set_enabled(on);
    if on {
        with(|s| s.days.entry(today()).or_default().launches += 1);
    }
    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(|| {
            // The first sending a little after the launch (not during it).
            let mut since_sent = SEND_EVERY - Duration::from_secs(120);
            loop {
                std::thread::sleep(Duration::from_secs(60));
                if !ON.load(Ordering::Relaxed) {
                    continue;
                }
                with(|s| s.days.entry(today()).or_default().minutes += 1);
                since_sent += Duration::from_secs(60);
                if since_sent >= SEND_EVERY {
                    since_sent = Duration::ZERO;
                    send();
                }
            }
        })
        .ok();
}

/// Written to disk (on quitting).
pub fn save() {
    with(|s| save_locked(s));
}

#[derive(Serialize)]
struct Payload<'a> {
    install: String,
    version: &'a str,
    os: &'static str,
    days: Vec<WireDay>,
    crashes: Vec<Crash>,
}

#[derive(Serialize)]
struct WireDay {
    day: String,
    launches: u32,
    minutes: u32,
    events: BTreeMap<String, u32>,
}

#[derive(Serialize)]
struct Crash {
    at: i64,
    text: String,
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

/// The crash files not sent yet (newest first), with what to send of each.
fn pending_crashes(sent: &[String]) -> Vec<(String, Crash)> {
    let Some(dir) = crate::config::config_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let oldest = chrono::Utc::now().timestamp() - CRASH_DAYS * 24 * 3600;
    let mut found: Vec<(i64, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let secs: i64 = name
                .strip_prefix("crash-")?
                .strip_suffix(".log")?
                .parse()
                .ok()?;
            (secs >= oldest && !sent.contains(&name)).then_some((secs, name))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found
        .into_iter()
        .take(CRASHES_MAX)
        .filter_map(|(secs, name)| {
            let text = std::fs::read_to_string(dir.join(&name)).ok()?;
            Some((
                name,
                Crash {
                    at: secs * 1000,
                    text: scrub(&text),
                },
            ))
        })
        .collect()
}

/// A crash report without what would tell who sent it: the home folder (and so the user's name).
fn scrub(text: &str) -> String {
    let mut text: String = text
        .lines()
        .take(CRASH_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(home) = dirs_home() {
        for form in [home.clone(), home.replace('\\', "/")] {
            if form.len() > 1 {
                text = text.replace(&form, "~");
            }
        }
    }
    let user = std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).unwrap_or_default();
    if user.trim().len() > 2 {
        text = text.replace(user.trim(), "<user>");
    }
    text
}

fn dirs_home() -> Option<String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|h| h.to_string_lossy().into_owned())
}

/// The counts and the new crashes to floor; what it took is forgotten here.
fn send() {
    let Some(url) = crate::floor::url() else {
        return;
    };
    let today = today();
    let (payload, crash_names) = with(|s| {
        let install = *s.install.get_or_insert_with(uuid::Uuid::new_v4);
        // The oldest days beyond what floor takes at once wait for the next time.
        let days: Vec<WireDay> = s
            .days
            .iter()
            .rev()
            .take(DAYS_MAX)
            .map(|(day, d)| WireDay {
                day: day.clone(),
                launches: d.launches,
                minutes: d.minutes,
                events: d.events.clone(),
            })
            .collect();
        let crashes = pending_crashes(&s.crashes_sent);
        save_locked(s);
        let names: Vec<String> = crashes.iter().map(|(n, _)| n.clone()).collect();
        (
            Payload {
                install: install.to_string(),
                version: crate::update::VERSION,
                os: os(),
                days,
                crashes: crashes.into_iter().map(|(_, c)| c).collect(),
            },
            names,
        )
    });
    if payload.days.is_empty() && payload.crashes.is_empty() {
        return;
    }
    let sent: Vec<String> = payload.days.iter().map(|d| d.day.clone()).collect();
    let res = crate::floor::agent()
        .post(format!("{url}/v1/telemetry"))
        .header("User-Agent", crate::floor::USER_AGENT)
        .send_json(&payload);
    // Refused (400): sent again it would be refused again, so forgotten too.
    let taken = match res {
        Ok(_) => true,
        Err(ureq::Error::StatusCode(400)) => {
            crate::log::error("telemetry: refused by floor");
            true
        }
        Err(_) => false,
    };
    if !taken {
        return;
    }
    with(|s| {
        // Today goes on counting: kept (sent again with its new totals).
        s.days.retain(|day, _| *day >= today || !sent.contains(day));
        s.crashes_sent.extend(crash_names);
        // Those whose file is gone need no remembering.
        if let Some(dir) = crate::config::config_dir() {
            s.crashes_sent.retain(|n| dir.join(n).exists());
        }
        save_locked(s);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_takes_out_the_home_folder() {
        let Some(home) = dirs_home() else { return };
        let text = format!("Ronnie 1.0.0 crashed: panicked at {home}/x.rs\nline\n");
        let out = scrub(&text);
        assert!(!out.contains(&home), "{out}");
        assert!(out.starts_with("Ronnie 1.0.0 crashed: panicked at ~/x.rs"));
    }

    #[test]
    fn scrub_keeps_the_first_lines() {
        let text: String = (0..100).map(|n| format!("{n}\n")).collect();
        assert_eq!(scrub(&text).lines().count(), CRASH_LINES);
    }

    /// Against a real floor: `RONNIE_FLOOR_URL=http://localhost:5002 RONNIE_CONFIG_DIR=<empty dir> cargo test
    /// telemetry::tests::live_send -- --ignored`.
    #[test]
    #[ignore]
    fn live_send() {
        set_enabled(true);
        with(|s| s.days.entry(today()).or_default().launches += 1);
        count("ssh.open");
        count("ssh.open");
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        with(|s| s.days.entry(yesterday.clone()).or_default().minutes = 42);
        send();
        // Yesterday is floor's now; today goes on counting.
        with(|s| {
            assert!(s.install.is_some());
            assert!(!s.days.contains_key(&yesterday));
            assert_eq!(s.days[&today()].events["ssh.open"], 2);
        });
    }
}
