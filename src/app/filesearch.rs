//! The search of the file manager: under a panel's folder, by name (a part of it, or a pattern with `*`
//! and `?`) or in the files' text. On this computer, a walk on another thread; on a server, its own `find`
//! or `grep` through ssh (fast, even on big trees), or a walk through SFTP when the account runs no
//! commands (names only). Hidden items and node_modules are skipped unless the hidden files are shown.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ssh::SshHost;
use crate::sftp::{name_matches, MAX_FOUND};

/// Files whose text is searched on this computer: at most this big.
const MAX_TEXT: u64 = 20 * 1024 * 1024;
/// The part of a matching line kept, in characters.
const LINE_CHARS: usize = 300;
/// Printed first by the server's script: missing, no shell ran it.
const MARK: &str = "RONNIE_FIND";

/// One item found.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Hit {
    /// Its full path (a local path, or an absolute remote one).
    pub path: String,
    pub is_dir: bool,
    /// A search in the text: the first matching line (its number from 1, and the line).
    pub line: Option<(usize, String)>,
}

/// What a running search sends.
enum Found {
    Hits(Vec<Hit>),
    Done(Result<(), String>),
    /// The server ran no command (an account limited to SFTP).
    NoShell,
}

/// A search going on: dropped, it stops (the server's command is killed).
struct Run {
    cancel: Arc<AtomicBool>,
    rx: Receiver<Found>,
    child: Option<Arc<Mutex<std::process::Child>>>,
}

impl Drop for Run {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(child) = &self.child {
            let _ = child.lock().unwrap().kill();
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(super) enum State {
    /// Nothing asked yet (the field empty).
    Idle,
    Running,
    /// Over; `full`: stopped at MAX_FOUND.
    Done { full: bool },
    Failed(String),
}

/// A panel's search: shown in place of its listing while open.
pub(super) struct Search {
    pub query: String,
    /// In the files' text, not their names.
    pub content: bool,
    /// The folder searched (the panel's when the search was opened, or last run).
    pub root: String,
    pub hits: Vec<Hit>,
    pub selected: Option<usize>,
    pub state: State,
    /// The field takes the keyboard at the next frame.
    pub focus: bool,
    run: Option<Run>,
    /// The SFTP walk going on (a server running no commands): its request's id.
    pub sftp: Option<u64>,
    /// When the query or the option last changed: the search starts once typing pauses.
    touched: Option<Instant>,
    /// What the hits are for (query, content, root, hidden).
    searched: Option<(String, bool, String, bool)>,
}

/// How long typing pauses before the search starts.
const PAUSE: Duration = Duration::from_millis(350);

impl Search {
    pub fn new(root: String) -> Self {
        Self { query: String::new(), content: false, root, hits: Vec::new(), selected: None, state: State::Idle, focus: true, run: None, sftp: None, touched: None, searched: None }
    }

    /// The query or the option changed: searched again once typing pauses.
    pub fn touch(&mut self) {
        self.touched = Some(Instant::now());
    }

    /// Searched again right away (Enter).
    pub fn now(&mut self) {
        self.touched = Some(Instant::now() - PAUSE);
    }

    /// A change waiting for typing to pause.
    pub fn waiting(&self) -> bool {
        self.touched.is_some()
    }

    pub fn running(&self) -> bool {
        self.state == State::Running
    }

    /// Whether the search should start now for this folder and hidden setting (then forgets the change).
    /// A query too short (or blank) clears the hits.
    pub fn due(&mut self, root: &str, hidden: bool) -> bool {
        if root != self.root {
            self.root = root.to_owned();
            self.touched.get_or_insert(Instant::now() - PAUSE);
        }
        let wanted = (self.query.trim().to_owned(), self.content, self.root.clone(), hidden);
        if self.searched.as_ref().is_some_and(|s| s.3 != hidden) {
            self.touched.get_or_insert(Instant::now() - PAUSE);
        }
        let Some(at) = self.touched else { return false };
        if at.elapsed() < PAUSE {
            return false;
        }
        self.touched = None;
        if self.searched.as_ref() == Some(&wanted) {
            return false;
        }
        self.stop();
        self.hits.clear();
        self.selected = None;
        let short = wanted.0.chars().count() < if self.content { 2 } else { 1 };
        self.searched = Some(wanted);
        if short {
            self.state = State::Idle;
            return false;
        }
        self.state = State::Running;
        true
    }

    /// Stops what is going on (the SFTP walk is cancelled by the caller, through `sftp`).
    pub fn stop(&mut self) {
        self.run = None;
        if self.state == State::Running {
            self.state = State::Done { full: false };
        }
    }

    /// Searches this computer, from `root`.
    pub fn start_local(&mut self, hidden: bool, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (query, content, root) = (self.query.trim().to_owned(), self.content, PathBuf::from(&self.root));
        let (flag, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut out = Batcher::new(tx, ctx);
            let result = walk_local(&root, &query, content, hidden, &flag, &mut out);
            out.done(result);
        });
        self.run = Some(Run { cancel, rx, child: None });
    }

    /// Searches the server with its find or grep, through ssh. Err: ssh couldn't start.
    pub fn start_remote(&mut self, host: &SshHost, askpass: Option<&crate::askpass::Server>, hidden: bool, ctx: &egui::Context) {
        let (query, content) = (self.query.trim().to_owned(), self.content);
        let script = if content { grep_script(&self.root, &query, hidden) } else { find_script(&self.root, &query, hidden) };
        let launch = host.exec_command(&format!("sh -c {}", super::files::sh_quote(&format!("echo {MARK}\n{script}"))));
        let mut cmd = std::process::Command::new(&launch.program);
        cmd.args(&launch.args).envs(launch.env.iter().cloned()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                self.state = State::Failed(format!("ssh : {e}"));
                return;
            }
        };
        if let Some(askpass) = askpass {
            askpass.allow_interactive(child.id(), host);
        }
        let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else { return };
        let child = Arc::new(Mutex::new(child));
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (flag, ctx, waiter) = (cancel.clone(), ctx.clone(), child.clone());
        std::thread::spawn(move || {
            let mut out = Batcher::new(tx, ctx);
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = Vec::new();
            let mut marked = false;
            let mut full = false;
            while !flag.load(Ordering::Relaxed) {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if line.ends_with(b"\n") {
                    line.pop();
                }
                if !marked {
                    marked = line == MARK.as_bytes();
                    continue;
                }
                if let Some(hit) = if content { grep_hit(&line) } else { find_hit(&line) }
                    && out.push(hit)
                {
                    full = true;
                    break;
                }
            }
            if full || flag.load(Ordering::Relaxed) {
                let _ = waiter.lock().unwrap().kill();
            }
            let mut errors = String::new();
            let _ = stderr.read_to_string(&mut errors);
            let status = waiter.lock().unwrap().wait();
            if !marked && !flag.load(Ordering::Relaxed) {
                let _ = out.tx.send(Found::NoShell);
                out.ctx.request_repaint();
                return;
            }
            // grep: 1 when nothing matched; 2 with an error (an unreadable folder) though it went on.
            let failed = !full && !flag.load(Ordering::Relaxed) && status.is_ok_and(|s| s.code().is_some_and(|c| c > 2 && c != 141));
            out.done(if failed { Err(errors.lines().last().unwrap_or("").to_owned()) } else { Ok(()) });
        });
        self.run = Some(Run { cancel, rx, child: Some(child) });
    }

    /// Takes what the search sent. True: the server runs no commands (the caller walks through SFTP
    /// instead, if it can).
    pub fn poll(&mut self) -> bool {
        let Some(run) = &self.run else { return false };
        let mut no_shell = false;
        let mut over = None;
        for found in run.rx.try_iter() {
            match found {
                Found::Hits(hits) => self.hits.extend(hits),
                Found::Done(result) => over = Some(result),
                Found::NoShell => no_shell = true,
            }
        }
        if no_shell {
            self.run = None;
            return true;
        }
        if let Some(result) = over {
            self.run = None;
            self.finish(result);
        }
        false
    }

    /// Paths from the SFTP walk.
    pub fn found(&mut self, hits: Vec<(String, bool)>, done: Option<Result<(), String>>) {
        self.hits.extend(hits.into_iter().map(|(path, is_dir)| Hit { path, is_dir, line: None }));
        self.hits.truncate(MAX_FOUND);
        if let Some(result) = done {
            self.sftp = None;
            self.finish(result.map_err(|e| if e.contains("cancelled") { String::new() } else { e }));
        }
    }

    pub fn finish(&mut self, result: Result<(), String>) {
        self.state = match result {
            Ok(()) | Err(_) if self.hits.len() >= MAX_FOUND => State::Done { full: true },
            Err(e) if !e.is_empty() => State::Failed(e),
            _ => State::Done { full: false },
        };
    }

    /// A hit's path from the folder searched, for display.
    pub fn relative<'a>(&self, path: &'a str) -> &'a str {
        let root = self.root.trim_end_matches(['/', '\\']);
        path.strip_prefix(root).map_or(path, |rest| rest.trim_start_matches(['/', '\\']))
    }
}

/// Sends the hits by batches (a few per frame rather than one by one).
struct Batcher {
    tx: Sender<Found>,
    ctx: egui::Context,
    batch: Vec<Hit>,
    count: usize,
    last: Instant,
}

impl Batcher {
    fn new(tx: Sender<Found>, ctx: egui::Context) -> Self {
        Self { tx, ctx, batch: Vec::new(), count: 0, last: Instant::now() }
    }

    /// Adds a hit; true once there are enough.
    fn push(&mut self, hit: Hit) -> bool {
        self.batch.push(hit);
        self.count += 1;
        if self.last.elapsed() > Duration::from_millis(150) {
            self.flush();
        }
        self.count >= MAX_FOUND
    }

    fn flush(&mut self) {
        if !self.batch.is_empty() {
            let _ = self.tx.send(Found::Hits(std::mem::take(&mut self.batch)));
            self.ctx.request_repaint();
        }
        self.last = Instant::now();
    }

    fn done(mut self, result: Result<(), String>) {
        self.flush();
        let _ = self.tx.send(Found::Done(result));
        self.ctx.request_repaint();
    }
}

/// Skipped while walking: hidden items and node_modules, unless the hidden files are shown.
fn skipped(name: &str, hidden: bool) -> bool {
    !hidden && (name.starts_with('.') || name == "node_modules")
}

/// The walk of this computer's folder `root`, a folder after the other (links not followed).
fn walk_local(root: &Path, query: &str, content: bool, hidden: bool, cancel: &AtomicBool, out: &mut Batcher) -> Result<(), String> {
    std::fs::read_dir(root).map_err(|e| format!("{} : {e}", root.display()))?;
    let needle = query.to_lowercase();
    let mut queue = std::collections::VecDeque::from([root.to_path_buf()]);
    while let Some(dir) = queue.pop_front() {
        let Ok(items) = std::fs::read_dir(&dir) else { continue };
        let mut items: Vec<_> = items.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let name = item.file_name().to_string_lossy().into_owned();
            if skipped(&name, hidden) {
                continue;
            }
            let Ok(kind) = item.file_type() else { continue };
            let path = item.path();
            if kind.is_dir() {
                queue.push_back(path.clone());
            }
            let hit = if content {
                (kind.is_file()).then(|| text_match(&path, &needle)).flatten().map(|line| Hit { path: path.display().to_string(), is_dir: false, line: Some(line) })
            } else {
                name_matches(query, &name).then(|| Hit { path: path.display().to_string(), is_dir: kind.is_dir(), line: None })
            };
            if let Some(hit) = hit
                && out.push(hit)
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The first line of the file at `path` holding `needle` (lowercase), unless the file is too big or
/// not text (a NUL byte in its start).
fn text_match(path: &Path, needle: &str) -> Option<(usize, String)> {
    if std::fs::metadata(path).ok()?.len() > MAX_TEXT {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    text.lines().enumerate().find(|(_, l)| l.to_lowercase().contains(needle)).map(|(i, l)| (i + 1, excerpt(l, needle)))
}

/// The matching line, cut around its first match when it is long (minified files).
fn excerpt(line: &str, needle: &str) -> String {
    let line = line.trim();
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= LINE_CHARS {
        return line.to_owned();
    }
    let lower = line.to_lowercase();
    let at = lower.find(needle).map_or(0, |b| lower[..b].chars().count());
    let start = at.saturating_sub(60).min(chars.len().saturating_sub(LINE_CHARS));
    let end = (start + LINE_CHARS).min(chars.len());
    format!("{}{}{}", if start > 0 { "…" } else { "" }, chars[start..end].iter().collect::<String>(), if end < chars.len() { "…" } else { "" })
}

/// `query` as find's -iname pattern: as is with wildcards, else a part of the name (its special
/// characters escaped).
fn find_pattern(query: &str) -> String {
    if query.contains(['*', '?']) {
        return query.to_owned();
    }
    let escaped: String = query.chars().flat_map(|c| if matches!(c, '[' | ']' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
    format!("*{escaped}*")
}

/// Prints "d <path>" or "f <path>" for each item under `root` whose name matches.
fn find_script(root: &str, query: &str, hidden: bool) -> String {
    let q = super::files::sh_quote;
    let prune = if hidden { String::new() } else { "\\( -name '.*' -o -name node_modules \\) -prune -o ".to_owned() };
    format!(
        "find {} -mindepth 1 {prune}-iname {} -exec sh -c 'for p; do if [ -d \"$p\" ] && [ ! -L \"$p\" ]; then echo \"d $p\"; else echo \"f $p\"; fi; done' sh {{}} + 2>/dev/null",
        q(root),
        q(&find_pattern(query))
    )
}

/// Prints, for each text file under `root` holding `query` (case ignored), its path, a NUL, its first
/// matching line's number and the line.
fn grep_script(root: &str, query: &str, hidden: bool) -> String {
    let q = super::files::sh_quote;
    let skip = if hidden { String::new() } else { "--exclude-dir='.*' --exclude='.*' --exclude-dir=node_modules ".to_owned() };
    format!("grep -rnIiF --null -m 1 {skip}-e {} -- {} 2>/dev/null", q(query), q(root))
}

fn find_hit(line: &[u8]) -> Option<Hit> {
    let line = String::from_utf8_lossy(line);
    let (kind, path) = line.split_once(' ')?;
    (path.starts_with('/') && matches!(kind, "d" | "f")).then(|| Hit { path: path.to_owned(), is_dir: kind == "d", line: None })
}

fn grep_hit(line: &[u8]) -> Option<Hit> {
    let nul = line.iter().position(|&b| b == 0)?;
    let path = String::from_utf8_lossy(&line[..nul]).into_owned();
    let rest = String::from_utf8_lossy(&line[nul + 1..]);
    let (number, text) = rest.split_once(':')?;
    let number = number.parse().ok()?;
    Some(Hit { path, is_dir: false, line: Some((number, excerpt(text, ""))) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_names() {
        assert!(name_matches("conf", "nginx.CONF.bak"));
        assert!(!name_matches("conf", "nginx"));
        assert!(name_matches("*.php", "index.PHP"));
        assert!(!name_matches("*.php", "index.php.bak"));
        assert!(name_matches("a?c*", "abcdef"));
        assert!(name_matches("*", ""));
        assert_eq!(find_pattern("a[1]"), "*a\\[1\\]*");
        assert_eq!(find_pattern("*.log"), "*.log");
    }

    #[test]
    fn reads_the_servers_lines() {
        assert_eq!(find_hit(b"d /var/www/a b"), Some(Hit { path: "/var/www/a b".into(), is_dir: true, line: None }));
        assert_eq!(find_hit(b"RONNIE x"), None);
        assert_eq!(grep_hit(b"/srv/a:b.txt\x0012:  let x = 1;"), Some(Hit { path: "/srv/a:b.txt".into(), is_dir: false, line: Some((12, "let x = 1;".into())) }));
        let long = format!("{}needle{}", "a".repeat(1000), "b".repeat(1000));
        let cut = excerpt(&long, "needle");
        assert!(cut.contains("needle") && cut.starts_with('…') && cut.ends_with('…'));
    }

    /// The walk here, and the same scripts run by this computer's sh (find and grep as on a server).
    #[test]
    fn searches_here_and_with_server_commands() {
        let dir = std::env::temp_dir().join(format!("ronnie-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("src/deep/Config.php"), "<?php\n$db = 'Needle';\n").unwrap();
        std::fs::write(dir.join("src/readme.md"), "nothing\n").unwrap();
        std::fs::write(dir.join("node_modules/x/config.js"), "needle").unwrap();
        std::fs::write(dir.join(".git/config"), "needle").unwrap();
        std::fs::write(dir.join("src/bin.dat"), b"needle\0\0").unwrap();
        let ctx = egui::Context::default();
        let run = |query: &str, content: bool, hidden: bool| {
            let mut s = Search::new(dir.display().to_string());
            s.query = query.into();
            s.content = content;
            s.start_local(hidden, &ctx);
            let start = Instant::now();
            while s.run.is_some() && start.elapsed() < Duration::from_secs(10) {
                s.poll();
                std::thread::sleep(Duration::from_millis(10));
            }
            let mut hits: Vec<(String, Option<usize>)> = s.hits.iter().map(|h| (s.relative(&h.path).replace('\\', "/"), h.line.as_ref().map(|l| l.0))).collect();
            hits.sort();
            hits
        };
        assert_eq!(run("config", false, false), vec![("src/deep/Config.php".into(), None)]);
        assert_eq!(run("needle", true, false), vec![("src/deep/Config.php".into(), Some(2))]);
        assert_eq!(run("config", false, true).len(), 3);
        assert_eq!(run("deep", false, false), vec![("src/deep".into(), None)]);
        #[cfg(unix)]
        {
            let sh = |script: String| std::process::Command::new("sh").arg("-c").arg(script).output().unwrap().stdout;
            let root = dir.display().to_string();
            let found: Vec<Hit> = sh(find_script(&root, "config", false)).split(|&b| b == b'\n').filter_map(find_hit).collect();
            assert_eq!(found, vec![Hit { path: format!("{root}/src/deep/Config.php"), is_dir: false, line: None }]);
            let found: Vec<Hit> = sh(find_script(&root, "dee*", false)).split(|&b| b == b'\n').filter_map(find_hit).collect();
            assert_eq!(found, vec![Hit { path: format!("{root}/src/deep"), is_dir: true, line: None }]);
            let found: Vec<Hit> = sh(grep_script(&root, "needle", false)).split(|&b| b == b'\n').filter_map(grep_hit).collect();
            assert_eq!(found, vec![Hit { path: format!("{root}/src/deep/Config.php"), is_dir: false, line: Some((2, "$db = 'Needle';".into())) }]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
