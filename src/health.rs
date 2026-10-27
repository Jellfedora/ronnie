//! Health of the SSH servers open in a tab: one quiet ssh per server runs a small sh loop that prints
//! its processor, memory, disks and uptime every few seconds (read from /proc on Linux; other
//! systems tell what they can).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::ssh::SshHost;

/// Seconds between two readings on the server.
const EVERY: u64 = 5;
/// After a dropped connection, the next try.
const RETRY: Duration = Duration::from_secs(30);
/// Starts each line of the loop's output (anything else, like a login message, is skipped).
const TAG: &str = "RONNIE_HEALTH";

/// The loop run on the server. Each line: tag | first line of /proc/stat | total and available memory
/// (kB) | total and used kB, then mount point, of the fullest disk | uptime (s) | 1-minute load | cores
/// | every disk the same way, separated by ";" (each device once).
fn script() -> String {
    format!(
        r#"while :; do
c=$(head -n 1 /proc/stat 2>/dev/null)
m=$(awk '/^MemTotal:/{{t=$2}} /^MemAvailable:/{{a=$2}} END{{if (t) print t, a}}' /proc/meminfo 2>/dev/null)
d=$(df -Pk 2>/dev/null | awk 'NR>1 && $1 ~ /^\/dev\// && $1 !~ /loop/ && $2>0 {{p=$3/$2; if (p>=b) {{b=p; r=$2" "$3" "$6}}}} END{{print r}}')
[ -n "$d" ] || d=$(df -Pk / 2>/dev/null | awk 'NR==2{{print $2" "$3" "$6}}')
u=$(cut -d ' ' -f 1 /proc/uptime 2>/dev/null)
l=$(cut -d ' ' -f 1 /proc/loadavg 2>/dev/null || sysctl -n vm.loadavg 2>/dev/null | awk '{{print $2}}')
n=$(nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null)
a=$(df -Pk 2>/dev/null | awk 'NR>1 && $1 ~ /^\/dev\// && $1 !~ /loop/ && $2>0 && !s[$1]++ {{m=$6; for (i=7; i<=NF; i++) m=m" "$i; printf "%s%s %s %s", (k++ ? ";" : ""), $2, $3, m}}')
echo "{TAG}|$c|$m|$d|$u|$l|$n|$a" || exit
sleep {EVERY}
done"#
    )
}

/// What the server last told.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Processor busy since the reading before (0 to 100), once two readings came.
    pub cpu: Option<f32>,
    pub load: Option<f32>,
    pub cores: Option<u32>,
    /// Memory used and total, in bytes (used: what isn't available).
    pub mem: Option<(u64, u64)>,
    /// The fullest disk: used and total bytes, mount point.
    pub disk: Option<(u64, u64, String)>,
    /// Every disk, as the server lists them: used and total bytes, mount point.
    pub disks: Vec<(u64, u64, String)>,
    pub uptime: Option<Duration>,
}

impl Stats {
    pub fn mem_percent(&self) -> Option<f32> {
        self.mem.filter(|(_, t)| *t > 0).map(|(u, t)| u as f32 * 100.0 / t as f32)
    }

    pub fn disk_percent(&self) -> Option<f32> {
        self.disk.as_ref().filter(|(_, t, _)| *t > 0).map(|(u, t, _)| *u as f32 * 100.0 / *t as f32)
    }
}

/// /proc/stat's "cpu" line: (busy, total) jiffies.
fn cpu_times(line: &str) -> Option<(u64, u64)> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    // user nice system idle iowait irq softirq steal (guest is already counted in user).
    let v: Vec<u64> = fields.take(8).filter_map(|f| f.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// One line of the loop, with the processor times of the reading before (to compare with).
fn parse(line: &str, before: Option<(u64, u64)>) -> Option<(Stats, Option<(u64, u64)>)> {
    let rest = line.trim_end().strip_prefix(TAG)?.strip_prefix('|')?;
    let f: Vec<&str> = rest.split('|').collect();
    if f.len() < 6 {
        return None;
    }
    let times = cpu_times(f[0]);
    let cpu = match (before, times) {
        (Some((b0, t0)), Some((b1, t1))) if t1 > t0 => Some(((b1.saturating_sub(b0)) as f32 * 100.0 / (t1 - t0) as f32).clamp(0.0, 100.0)),
        _ => None,
    };
    let nums = |s: &str| s.split_whitespace().map(|n| n.parse::<u64>().ok()).collect::<Vec<_>>();
    let mem = match nums(f[1])[..] {
        [Some(total), Some(available)] if total > 0 => Some((total.saturating_sub(available) * 1024, total * 1024)),
        _ => None,
    };
    let disk_of = |s: &str| {
        let mut parts = s.splitn(3, ' ');
        match (parts.next().and_then(|n| n.parse::<u64>().ok()), parts.next().and_then(|n| n.parse::<u64>().ok()), parts.next()) {
            (Some(total), Some(used), Some(mount)) if total > 0 => Some((used * 1024, total * 1024, mount.trim().to_owned())),
            _ => None,
        }
    };
    let disk = disk_of(f[2]);
    // Not told by a loop started before Ronnie asked for it.
    let disks = f.get(6).map_or_else(Vec::new, |a| a.split(';').filter_map(disk_of).collect());
    let uptime = f[3].trim().parse::<f64>().ok().map(Duration::from_secs_f64);
    let load = f[4].trim().replace(',', ".").parse().ok();
    let cores = f[5].trim().parse().ok().filter(|n| *n > 0);
    Some((Stats { cpu, load, cores, mem, disk, disks, uptime }, times))
}

#[derive(Default)]
struct Shared {
    stats: Option<Stats>,
    /// When the last reading came.
    at: Option<Instant>,
    /// The server refused the login without asking anyone: not tried again (each try would count as a
    /// failed login).
    refused: bool,
}

/// One server watched in the background; dropped, its ssh stops.
pub struct Monitor {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Monitor {
    fn start(host: SshHost, askpass: crate::askpass::Server, ctx: egui::Context) -> Self {
        let monitor = Self { shared: Default::default(), stop: Default::default(), child: Default::default() };
        let (shared, stop, child) = (monitor.shared.clone(), monitor.stop.clone(), monitor.child.clone());
        let _ = std::thread::Builder::new().name("health".into()).spawn(move || run(&host, &askpass, &ctx, &shared, &stop, &child));
        monitor
    }

    /// The last reading, and how old it is.
    pub fn stats(&self) -> Option<(Stats, Duration)> {
        let shared = self.shared.lock().unwrap();
        Some((shared.stats.clone()?, shared.at?.elapsed()))
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }
}

/// The ssh running the loop: the host's options, no terminal, and nothing asked to anyone (the saved
/// password, if the host uses one, is all it gets; an unknown host key fails instead of asking).
fn command(host: &SshHost) -> crate::ssh::Launch {
    let mut launch = host.exec_command(&format!("sh -c {}", crate::ssh::sh_quote(&script())));
    // It ends with "-T", "--", host, the command.
    let at = launch.args.len().saturating_sub(4);
    let options = ["StrictHostKeyChecking=yes", "NumberOfPasswordPrompts=1", "LogLevel=ERROR"];
    for (i, option) in options.iter().enumerate() {
        launch.args.splice(at + 2 * i..at + 2 * i, ["-o".to_owned(), (*option).to_owned()]);
    }
    launch
}

fn run(host: &SshHost, askpass: &crate::askpass::Server, ctx: &egui::Context, shared: &Mutex<Shared>, stop: &AtomicBool, slot: &Mutex<Option<Child>>) {
    let launch = command(host);
    while !stop.load(Ordering::SeqCst) {
        let mut cmd = Command::new(&launch.program);
        cmd.args(&launch.args).envs(launch.env.iter().cloned()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let Ok(mut child) = cmd.spawn() else { return };
        // Not interactive: only the saved password, never a question in the window.
        askpass.allow(child.id(), host);
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        *slot.lock().unwrap() = Some(child);
        // Dropped while starting: the kill missed it.
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(stderr) = stderr {
                let _ = stderr.take(16 * 1024).read_to_string(&mut text);
            }
            text
        });
        let mut before = None;
        for line in stdout.into_iter().flat_map(|s| BufReader::new(s).lines()).map_while(Result::ok) {
            if let Some((stats, times)) = parse(&line, before) {
                before = times;
                let mut shared = shared.lock().unwrap();
                shared.stats = Some(stats);
                shared.at = Some(Instant::now());
                drop(shared);
                ctx.request_repaint();
            }
        }
        if let Some(mut child) = slot.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let errors = errors.join().unwrap_or_default().to_lowercase();
        if errors.contains("permission denied") || errors.contains("too many authentication failures") {
            shared.lock().unwrap().refused = true;
            crate::log::info(&format!("health {}: login refused", host.name));
            return;
        }
        // Dropped network, server restarting, host key not trusted yet: later.
        let until = Instant::now() + RETRY;
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

fn same_login(a: &SshHost, b: &SshHost) -> bool {
    (&a.host, a.port, &a.user, a.auth, &a.identity_file, &a.jump, &a.options, a.password_saved) == (&b.host, b.port, &b.user, b.auth, &b.identity_file, &b.jump, &b.options, b.password_saved)
}

/// The servers watched, by host.
#[derive(Default)]
pub struct Monitors {
    watched: HashMap<Uuid, (SshHost, Monitor)>,
    /// When each host's ssh panes were first seen connected: watched only after a few seconds, once
    /// their own login (and any question it asks) is through.
    seen: HashMap<Uuid, Instant>,
}

impl Monitors {
    /// Watches exactly the hosts in `open` (hosts with a live SSH pane).
    pub fn keep(&mut self, open: &[&SshHost], askpass: &crate::askpass::Server, ctx: &egui::Context) {
        self.seen.retain(|id, _| open.iter().any(|h| h.id == *id));
        // A host whose login changed starts over with the new one (not for the file manager's folders
        // or its saved commands).
        self.watched.retain(|id, (host, _)| open.iter().any(|h| h.id == *id && same_login(h, host)));
        for host in open {
            // A login it can't do without asking: nothing to show.
            if matches!(host.auth_method(), crate::ssh::SshAuth::Ask | crate::ssh::SshAuth::Interactive) || self.watched.contains_key(&host.id) {
                continue;
            }
            let since = *self.seen.entry(host.id).or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(4) {
                self.watched.insert(host.id, ((*host).clone(), Monitor::start((*host).clone(), askpass.clone(), ctx.clone())));
            } else {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }
    }

    /// Forgets the hosts whose login was refused, so that a reconnection tries again.
    pub fn retry(&mut self, host: Uuid) {
        if self.watched.get(&host).is_some_and(|(_, m)| m.shared.lock().unwrap().refused) {
            self.watched.remove(&host);
            self.seen.remove(&host);
        }
    }

    pub fn get(&self, host: Uuid) -> Option<&Monitor> {
        self.watched.get(&host).map(|(_, m)| m)
    }

    pub fn clear(&mut self) {
        self.watched.clear();
        self.seen.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SshHost, &Monitor)> {
        self.watched.values().map(|(h, m)| (h, m))
    }
}

/// "3,4 Go", "512 Mo"...
pub fn bytes(n: u64, units: [&str; 4]) -> String {
    let n = n as f64;
    let (value, unit) = if n >= 1e12 { (n / 1e12, units[3]) } else if n >= 1e9 { (n / 1e9, units[2]) } else if n >= 1e6 { (n / 1e6, units[1]) } else { (n / 1e3, units[0]) };
    if value >= 100.0 { format!("{value:.0} {unit}") } else { format!("{value:.1} {unit}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_linux_line() {
        let first = format!("{TAG}|cpu  100 0 100 800 0 0 0 0 0 0|8000000 2000000|61000000 30500000 /|12345.67|0.52|4");
        let (stats, times) = parse(&first, None).unwrap();
        assert_eq!(stats.cpu, None);
        assert_eq!(stats.mem, Some((6000000 * 1024, 8000000 * 1024)));
        assert_eq!(stats.disk, Some((30500000 * 1024, 61000000 * 1024, "/".to_owned())));
        assert_eq!(stats.disk_percent().map(f32::round), Some(50.0));
        assert_eq!(stats.uptime.map(|d| d.as_secs()), Some(12345));
        assert_eq!((stats.load, stats.cores), (Some(0.52), Some(4)));
        // 100 more busy jiffies out of 400: 25 %.
        let second = format!("{TAG}|cpu  150 0 150 1100 0 0 0 0 0 0|8000000 2000000|61000000 30500000 /data disk|12350|0.5|4");
        let (stats, _) = parse(&second, times).unwrap();
        assert_eq!(stats.cpu, Some(25.0));
        assert_eq!(stats.disk.unwrap().2, "/data disk");
        assert!(stats.disks.is_empty());
        let third = format!("{TAG}|cpu  150 0 150 1100 0 0 0 0 0 0|8000000 2000000|61000000 30500000 /|12350|0.5|4|61000000 30500000 /;20000000 1000000 /mnt/my backups");
        let (stats, _) = parse(&third, None).unwrap();
        assert_eq!(stats.disks, vec![(30500000 * 1024, 61000000 * 1024, "/".to_owned()), (1000000 * 1024, 20000000 * 1024, "/mnt/my backups".to_owned())]);
    }

    #[test]
    fn reads_what_another_system_tells() {
        let (stats, _) = parse(&format!("{TAG}|||250000000 100000000 /System/Volumes/Data||1,85|8"), None).unwrap();
        assert_eq!((stats.cpu, stats.mem, stats.uptime), (None, None, None));
        assert_eq!((stats.load, stats.cores), (Some(1.85), Some(8)));
        assert!(stats.disk.is_some());
        assert!(parse("Welcome to the server", None).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn script_runs_here() {
        // The command as ssh hands it to the server's shell, on this computer (Linux or macOS): two
        // readings, the second with the processor.
        let mut child = Command::new("sh").arg("-c").arg(format!("sh -c {}", crate::ssh::sh_quote(&script()))).stdout(Stdio::piped()).spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let first = lines.next().unwrap().unwrap();
        let (stats, times) = parse(&first, None).unwrap();
        assert!(stats.disk.is_some() && !stats.disks.is_empty() && stats.cores.is_some() && stats.load.is_some(), "{first}");
        if cfg!(target_os = "linux") {
            let (stats, _) = parse(&lines.next().unwrap().unwrap(), times).unwrap();
            assert!(stats.cpu.is_some() && stats.mem.is_some() && stats.uptime.is_some());
        }
        let _ = child.kill();
    }
}
