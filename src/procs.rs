//! The machine's processes, for the Dev category: what each one uses (CPU, memory, disk), the TCP
//! ports it listens on, and a way to stop it. Read every two seconds by a thread of their own while
//! the page shows.
//!
//! The ports: `lsof` on macOS, /proc on Linux, `netstat` on Windows.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How often the thread reads.
const TICK: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Proc {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    /// Its command line, arguments included (empty when the system won't tell: another user's).
    pub command: String,
    pub exe: Option<String>,
    /// Percent of the whole machine (all its cores).
    pub cpu: f32,
    pub memory: u64,
    /// Bytes a second.
    pub read: u64,
    pub written: u64,
    /// Seconds.
    pub run_time: u64,
    /// Started by the user running Ronnie (the others usually can't be stopped).
    pub mine: bool,
    /// The TCP ports it listens on, sorted.
    pub ports: Vec<Port>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Port {
    pub port: u16,
    /// Reachable from this machine only (127.0.0.1, ::1).
    pub local: bool,
}

#[derive(Default)]
pub struct Snapshot {
    pub procs: Vec<Proc>,
    /// Percent.
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    /// Counts the readings: what depends on the list is worked out again when it changes.
    pub generation: u64,
}

/// The reading thread: it stops when this is dropped.
pub struct Monitor {
    snapshot: Arc<Mutex<Arc<Snapshot>>>,
    stop: Arc<AtomicBool>,
    /// Read again now (after a process was stopped).
    now: Arc<AtomicBool>,
}

impl Monitor {
    pub fn start(ctx: &egui::Context) -> Self {
        let snapshot = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let stop = Arc::new(AtomicBool::new(false));
        let now = Arc::new(AtomicBool::new(false));
        let (shared, stopped, again, ctx) = (snapshot.clone(), stop.clone(), now.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("procs".into()).spawn(move || run(&shared, &stopped, &again, &ctx));
        Self { snapshot, stop, now }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn refresh(&self) {
        self.now.store(true, Ordering::Relaxed);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(shared: &Mutex<Arc<Snapshot>>, stop: &AtomicBool, now: &AtomicBool, ctx: &egui::Context) {
    let mut system = System::new();
    let kind = ProcessRefreshKind::nothing().with_memory().with_cpu().with_disk_usage().with_exe(UpdateKind::OnlyIfNotSet).with_cmd(UpdateKind::OnlyIfNotSet).with_user(UpdateKind::OnlyIfNotSet);
    let me = sysinfo::get_current_pid().ok();
    let mut generation = 0;
    let mut last = Instant::now();
    // The first reading at once (its CPU is zero: a rate needs two), the next one soon after.
    let mut wait = Duration::from_millis(500);
    loop {
        system.refresh_cpu_usage();
        system.refresh_memory();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        let elapsed = last.elapsed().as_secs_f64().max(0.001);
        last = Instant::now();
        let mut listened = listening();
        let cores = system.cpus().len().max(1) as f32;
        let my_user = me.and_then(|p| system.process(p)).and_then(|p| p.user_id()).cloned();
        let procs = system
            .processes()
            .values()
            // Linux's threads come as processes too.
            .filter(|p| p.thread_kind().is_none_or(|k| k != sysinfo::ThreadKind::Userland))
            .map(|p| {
                let usage = p.disk_usage();
                let mut ports = listened.remove(&p.pid().as_u32()).unwrap_or_default();
                ports.sort();
                ports.dedup_by_key(|p| p.port);
                Proc {
                    pid: p.pid().as_u32(),
                    parent: p.parent().map(Pid::as_u32),
                    name: p.name().to_string_lossy().into_owned(),
                    command: p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "),
                    exe: p.exe().map(|e| e.display().to_string()),
                    cpu: p.cpu_usage() / cores,
                    memory: p.memory(),
                    read: (usage.read_bytes as f64 / elapsed) as u64,
                    written: (usage.written_bytes as f64 / elapsed) as u64,
                    run_time: p.run_time(),
                    mine: my_user.is_some() && p.user_id() == my_user.as_ref(),
                    ports,
                }
            })
            .collect();
        generation += 1;
        *shared.lock().unwrap() = Arc::new(Snapshot { procs, cpu: system.global_cpu_usage(), ram_used: system.used_memory(), ram_total: system.total_memory(), generation });
        ctx.request_repaint();
        let until = Instant::now() + wait;
        wait = TICK;
        while Instant::now() < until {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if now.swap(false, Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Asks process `pid` to stop (`force`: kills it, it can't refuse). Windows: always killed, a
/// console program can't be asked.
pub fn stop(pid: u32, force: bool) -> Result<(), String> {
    #[cfg(unix)]
    {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: a plain system call, on a number.
        if unsafe { libc::kill(pid as libc::pid_t, signal) } == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error().to_string())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = force;
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/PID", &pid.to_string()]).creation_flags(0x0800_0000);
        let out = cmd.output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(());
        }
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// The TCP ports listened on, by process.
#[cfg(target_os = "macos")]
fn listening() -> HashMap<u32, Vec<Port>> {
    let Ok(out) = std::process::Command::new("lsof").args(["-nP", "-iTCP", "-sTCP:LISTEN", "-FpPn"]).output() else { return HashMap::new() };
    parse_lsof(&String::from_utf8_lossy(&out.stdout))
}

/// `lsof -F` lines: "p1234" starts a process, "n127.0.0.1:3000" (or "n*:3000", "n[::1]:3000") is a
/// socket of it.
#[cfg(any(target_os = "macos", test))]
fn parse_lsof(text: &str) -> HashMap<u32, Vec<Port>> {
    let mut ports: HashMap<u32, Vec<Port>> = HashMap::new();
    let mut pid = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse::<u32>().ok();
        } else if let (Some(address), Some(pid)) = (line.strip_prefix('n'), pid)
            && let Some((host, port)) = address.rsplit_once(':')
            && let Ok(port) = port.parse::<u16>()
        {
            ports.entry(pid).or_default().push(Port { port, local: loopback(host.trim_start_matches('[').trim_end_matches(']')) });
        }
    }
    ports
}

/// /proc/net/tcp{,6}: the listening sockets (state 0A) and their inodes, then which process holds
/// each inode among its files (only the user's own processes can be looked into).
#[cfg(target_os = "linux")]
fn listening() -> HashMap<u32, Vec<Port>> {
    let mut sockets: HashMap<u64, Port> = HashMap::new();
    for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 || fields[3] != "0A" {
                continue;
            }
            let Some((host, port)) = fields[1].split_once(':') else { continue };
            let (Ok(port), Ok(inode)) = (u16::from_str_radix(port, 16), fields[9].parse::<u64>()) else { continue };
            // Hex, in the machine's order: 0100007F is 127.0.0.1; ::1 ends with 01000000.
            let local = host == "0100007F" || host == "00000000000000000000000001000000";
            sockets.insert(inode, Port { port, local });
        }
    }
    let mut ports: HashMap<u32, Vec<Port>> = HashMap::new();
    if sockets.is_empty() {
        return ports;
    }
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        for fd in std::fs::read_dir(entry.path().join("fd")).into_iter().flatten().flatten() {
            let Ok(link) = std::fs::read_link(fd.path()) else { continue };
            let link = link.to_string_lossy();
            if let Some(inode) = link.strip_prefix("socket:[").and_then(|l| l.strip_suffix(']')).and_then(|i| i.parse::<u64>().ok())
                && let Some(port) = sockets.get(&inode)
            {
                ports.entry(pid).or_default().push(*port);
            }
        }
    }
    ports
}

#[cfg(windows)]
fn listening() -> HashMap<u32, Vec<Port>> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("netstat");
    cmd.args(["-ano", "-p", "TCP"]).creation_flags(0x0800_0000);
    let mut text = cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut cmd = std::process::Command::new("netstat");
    cmd.args(["-ano", "-p", "TCPv6"]).creation_flags(0x0800_0000);
    text += &cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    parse_netstat(&text)
}

/// `netstat -ano` lines: "TCP 127.0.0.1:3000 0.0.0.0:0 LISTENING 1234". The state's word is
/// translated with Windows: a listening socket is the one with no remote address.
#[cfg(any(windows, test))]
fn parse_netstat(text: &str) -> HashMap<u32, Vec<Port>> {
    let mut ports: HashMap<u32, Vec<Port>> = HashMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || !fields[0].eq_ignore_ascii_case("TCP") || !matches!(fields[2], "0.0.0.0:0" | "[::]:0") {
            continue;
        }
        let (Some((host, port)), Ok(pid)) = (fields[1].rsplit_once(':'), fields[fields.len() - 1].parse::<u32>()) else { continue };
        let Ok(port) = port.parse::<u16>() else { continue };
        ports.entry(pid).or_default().push(Port { port, local: loopback(host.trim_start_matches('[').trim_end_matches(']')) });
    }
    ports
}

#[cfg(any(target_os = "macos", windows, test))]
fn loopback(host: &str) -> bool {
    host.starts_with("127.") || host == "::1" || host == "localhost"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lsof() {
        let ports = parse_lsof("p748\nf13\nPTCP\nn*:57809\nf14\nPTCP\nn127.0.0.1:3000\np802\nf10\nPTCP\nn[::1]:5173\n");
        assert_eq!(ports[&748], vec![Port { port: 57809, local: false }, Port { port: 3000, local: true }]);
        assert_eq!(ports[&802], vec![Port { port: 5173, local: true }]);
    }

    #[test]
    fn reads_netstat() {
        let text = "\nConnexions actives\n\n  Proto  Adresse locale         Adresse distante       État\n  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1100\n  TCP    127.0.0.1:3000         0.0.0.0:0              LISTENING       42\n  TCP    127.0.0.1:3000         127.0.0.1:50000        ESTABLISHED     42\n  TCP    [::1]:8080             [::]:0                 LISTENING       7\n";
        let ports = parse_netstat(text);
        assert_eq!(ports[&1100], vec![Port { port: 135, local: false }]);
        assert_eq!(ports[&42], vec![Port { port: 3000, local: true }]);
        assert_eq!(ports[&7], vec![Port { port: 8080, local: true }]);
    }
}
