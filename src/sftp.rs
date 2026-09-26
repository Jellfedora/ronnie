//! SFTP over the system's ssh: `ssh … -s host sftp` is started like the terminal sessions (same
//! ~/.ssh/config, agent, keys, jump hosts, known_hosts, saved passwords), and the SFTP protocol is
//! spoken over its standard input and output. Everything runs in the background: the file manager sends
//! requests and gets events back, and never waits on the network.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc as tmpsc;

/// One file or directory in a listing.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    /// Seconds since 1970.
    pub mtime: Option<i64>,
    /// Unix permission bits (the lower 12), when known.
    pub mode: Option<u32>,
    pub owner: Option<String>,
}

/// What the file manager asks for. Remote paths are absolute, with "/" separators.
#[derive(Debug)]
pub enum Request {
    List(String),
    Mkdir(String),
    Rename(String, String),
    /// Files and directories (recursively).
    Remove(Vec<String>),
    Chmod { paths: Vec<String>, mode: u32, recursive: bool },
    Download { id: u64, remote: Vec<String>, local_dir: PathBuf, overwrite: bool },
    Upload { id: u64, local: Vec<PathBuf>, remote_dir: String, overwrite: bool },
}

#[derive(Debug)]
pub enum Event {
    /// The session is ready; `home` is the remote home directory.
    Connected { home: String },
    Listing { path: String, entries: Vec<Entry> },
    /// A folder couldn't be listed (the panel goes back to where it was).
    ListFailed { path: String, error: String },
    /// A change (mkdir, rename, remove, chmod) is done: the listing may be out of date.
    Changed,
    Error(String),
    Progress { id: u64, done: u64, total: u64, current: String },
    /// Done: how many files were skipped because they already existed, or why it failed.
    Finished { id: u64, result: Result<u64, String> },
    /// The connection is over (with ssh's last words, if any).
    Closed(String),
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("sftp").enable_all().build().expect("tokio runtime"))
}

type Cancels = Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>;

/// A running SFTP session. Dropping it ends the session and stops ssh.
pub struct Connection {
    requests: tmpsc::UnboundedSender<Request>,
    events: Receiver<Event>,
    /// ssh's process id: the askpass helper answers it (see askpass.rs).
    pub pid: Option<u32>,
    /// Cancel flags of the transfers, set from the UI directly (not queued behind other requests).
    cancels: Cancels,
    stop: Arc<tokio::sync::Notify>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.notify_one();
    }
}

impl Connection {
    /// Starts `program args` (ssh with the subsystem, or an sftp-server in tests) and opens a session
    /// over its standard input and output.
    pub fn open(ctx: &egui::Context, program: &str, args: &[String], env: &[(String, String)]) -> Result<Self> {
        let _guard = runtime().enter();
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args).envs(env.iter().cloned()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        // Release builds have no console: don't let ssh open one.
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().with_context(|| format!("starting {program}"))?;
        let pid = child.id();
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;

        let (requests, mut incoming) = tmpsc::unbounded_channel();
        let (tx, events) = mpsc::channel();
        let emit = Emitter { tx, ctx: ctx.clone() };
        let stop = Arc::new(tokio::sync::Notify::new());
        let cancels: Cancels = Arc::default();

        // ssh's error output (its last few KB), for the "disconnected" message.
        let last_words = Arc::new(Mutex::new(String::new()));
        let stderr_done = {
            let last_words = last_words.clone();
            runtime().spawn(async move {
                let mut stderr = stderr;
                let mut buf = [0u8; 4096];
                while let Ok(n) = stderr.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let mut words = last_words.lock().unwrap();
                    words.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if words.len() > 4096 {
                        let cut = words.len() - 4096;
                        let cut = (cut..words.len()).find(|&i| words.is_char_boundary(i)).unwrap_or(0);
                        words.drain(..cut);
                    }
                }
            })
        };
        // Watches ssh: when it ends (network drop, server closing, auth refused), the file manager is told
        // why; when the connection is dropped (tab closed), ssh is stopped.
        {
            let (emit, stop, last_words) = (emit.clone(), stop.clone(), last_words.clone());
            runtime().spawn(async move {
                tokio::select! {
                    _ = child.wait() => {
                        let _ = tokio::time::timeout(Duration::from_millis(500), stderr_done).await;
                        let words = last_words.lock().unwrap().trim().to_owned();
                        emit.send(Event::Closed(if words.is_empty() { "ssh ended".into() } else { words }));
                    }
                    _ = stop.notified() => {
                        let _ = child.kill().await;
                    }
                }
            });
        }

        let session_cancels = cancels.clone();
        let session_stop = stop.clone();
        runtime().spawn(async move {
            // The handshake may wait for the user (password, new host key in a window): give it minutes,
            // then 30 s per request. 64 writes in flight (2 MB) instead of 16: uploads to a distant server
            // would otherwise be capped by the round trips (OpenSSH's sftp keeps about as much in flight).
            let config = russh_sftp::client::Config { max_concurrent_writes: 64, request_timeout_secs: 300, ..Default::default() };
            let sftp = match SftpSession::new_with_config(tokio::io::join(stdout, stdin), config).await {
                Ok(sftp) => Arc::new(sftp),
                Err(e) => {
                    // ssh still running (a prompt left unanswered...): stop it; its exit reports why.
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    if last_words.lock().unwrap().trim().is_empty() {
                        emit.send(Event::Closed(e.to_string()));
                    }
                    session_stop.notify_one();
                    return;
                }
            };
            sftp.set_timeout(30);
            let home = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into());
            emit.send(Event::Connected { home });

            // Transfers run one after the other in their own task, so that browsing stays responsive.
            let (jobs, mut queue) = tmpsc::unbounded_channel::<Request>();
            {
                let (sftp, emit, cancels) = (sftp.clone(), emit.clone(), session_cancels.clone());
                tokio::spawn(async move {
                    while let Some(job) = queue.recv().await {
                        let id = match &job {
                            Request::Download { id, .. } | Request::Upload { id, .. } => *id,
                            _ => continue,
                        };
                        let cancel = cancels.lock().unwrap().get(&id).cloned().unwrap_or_default();
                        let result = if cancel.load(Ordering::Relaxed) {
                            Err(anyhow::anyhow!("cancelled"))
                        } else {
                            match job {
                                Request::Download { remote, local_dir, overwrite, .. } => download(&sftp, &remote, &local_dir, overwrite, id, &emit, &cancel).await,
                                Request::Upload { local, remote_dir, overwrite, .. } => upload(&sftp, &local, &remote_dir, overwrite, id, &emit, &cancel).await,
                                _ => unreachable!(),
                            }
                        };
                        cancels.lock().unwrap().remove(&id);
                        emit.send(Event::Finished { id, result: result.map_err(|e| format!("{e:#}")) });
                        // Listings are refreshed once the queue is done, not after each of its items.
                        if queue.is_empty() {
                            emit.send(Event::Changed);
                        }
                    }
                });
            }

            while let Some(request) = incoming.recv().await {
                let result = match request {
                    Request::List(path) => match list(&sftp, &path).await {
                        Ok(entries) => {
                            emit.send(Event::Listing { path, entries });
                            Ok(())
                        }
                        Err(e) => {
                            emit.send(Event::ListFailed { path, error: format!("{e:#}") });
                            Ok(())
                        }
                    },
                    Request::Mkdir(path) => sftp.create_dir(path).await.map_err(anyhow::Error::from).map(|_| emit.send(Event::Changed)),
                    Request::Rename(from, to) => sftp.rename(from, to).await.map_err(anyhow::Error::from).map(|_| emit.send(Event::Changed)),
                    Request::Remove(paths) => remove(&sftp, &paths).await.map(|_| emit.send(Event::Changed)),
                    Request::Chmod { paths, mode, recursive } => chmod(&sftp, &paths, mode, recursive).await.map(|_| emit.send(Event::Changed)),
                    job @ (Request::Download { .. } | Request::Upload { .. }) => {
                        let _ = jobs.send(job);
                        Ok(())
                    }
                };
                if let Err(e) = result {
                    emit.send(Event::Error(format!("{e:#}")));
                }
            }
            let _ = sftp.close().await;
        });

        Ok(Self { requests, events, pid, cancels, stop })
    }

    pub fn send(&self, request: Request) {
        // A transfer's cancel flag exists from the start, so that cancelling it works even while queued.
        if let Request::Download { id, .. } | Request::Upload { id, .. } = &request {
            self.cancels.lock().unwrap().insert(*id, Arc::default());
        }
        let _ = self.requests.send(request);
    }

    /// Cancels a transfer, running or queued, right away.
    pub fn cancel(&self, id: u64) {
        if let Some(flag) = self.cancels.lock().unwrap().get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Events received since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

#[derive(Clone)]
struct Emitter {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl Emitter {
    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

/// Joins a remote directory and a name.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

/// The parent of a remote path ("/" stays "/").
pub fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((head, _)) => head.into(),
    }
}

fn entry(name: String, attrs: &FileAttributes) -> Entry {
    Entry {
        name,
        is_dir: attrs.is_dir(),
        is_link: attrs.is_symlink(),
        size: attrs.size.unwrap_or(0),
        mtime: attrs.mtime.map(i64::from),
        mode: attrs.permissions.map(|p| p & 0o7777),
        owner: attrs.user.clone().or_else(|| attrs.uid.map(|u| u.to_string())),
    }
}

/// Whether a name sent by the server is a plain file name. A hostile server could send "../../.zshenv"
/// or "a/b" to make a download write outside the chosen folder (OpenSSH's sftp refuses them too).
pub fn valid_name(name: &str) -> bool {
    let forbidden: &[char] = if cfg!(windows) { &['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|'] } else { &['/', '\0'] };
    !name.is_empty() && name != "." && name != ".." && !name.contains(forbidden)
}

/// Limits on a recursive walk, against endless or huge trees (a hostile server can claim anything).
const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: usize = 1_000_000;

async fn list(sftp: &SftpSession, path: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in sftp.read_dir(path).await.with_context(|| format!("listing {path}"))? {
        let name = item.file_name();
        // Suspect names aren't shown: every action on them would act on another path.
        if !valid_name(&name) {
            continue;
        }
        let mut e = entry(name, &item.metadata());
        // A link to a directory opens like a directory.
        if e.is_link {
            if let Ok(target) = sftp.metadata(join(path, &e.name)).await {
                e.is_dir = target.is_dir();
            }
        }
        entries.push(e);
    }
    Ok(entries)
}

/// Every path under `root` (itself included), directories before their contents. Never follows a
/// symbolic link (checked with lstat, whatever the listing claims), refuses suspect names, and stops
/// on trees too deep or too big.
async fn walk(sftp: &SftpSession, root: &str, follow_root: bool) -> Result<Vec<(String, FileAttributes)>> {
    let mut attrs = sftp.symlink_metadata(root).await.with_context(|| format!("reading {root}"))?;
    // A link chosen by the user (not one met inside a folder) is followed for transfers.
    if follow_root && attrs.is_symlink() {
        attrs = sftp.metadata(root).await.with_context(|| format!("reading {root}"))?;
    }
    let mut out = vec![(root.to_owned(), attrs.clone())];
    let mut stack = if attrs.is_dir() && !attrs.is_symlink() { vec![(root.to_owned(), 0)] } else { Vec::new() };
    let mut visited = std::collections::HashSet::new();
    while let Some((dir, depth)) = stack.pop() {
        if !visited.insert(dir.clone()) {
            continue;
        }
        for item in sftp.read_dir(dir.clone()).await.with_context(|| format!("listing {dir}"))? {
            let name = item.file_name();
            if name == "." || name == ".." {
                continue;
            }
            anyhow::ensure!(valid_name(&name), "the server sent a suspect file name in {dir}: {name:?}");
            let path = join(&dir, &name);
            let mut attrs = item.metadata();
            if attrs.is_dir() {
                // Some servers report the target's type for links: ask for the entry itself.
                attrs = sftp.symlink_metadata(path.clone()).await.unwrap_or(attrs);
                if attrs.is_dir() && !attrs.is_symlink() {
                    anyhow::ensure!(depth < MAX_DEPTH, "{path}: too deep (more than {MAX_DEPTH} levels)");
                    stack.push((path.clone(), depth + 1));
                }
            }
            out.push((path, attrs));
            anyhow::ensure!(out.len() <= MAX_ENTRIES, "{root}: more than {MAX_ENTRIES} items");
        }
    }
    Ok(out)
}

async fn remove(sftp: &SftpSession, paths: &[String]) -> Result<()> {
    for root in paths {
        let mut all = walk(sftp, root, false).await?;
        // Contents before their directory.
        all.reverse();
        for (path, attrs) in all {
            if attrs.is_dir() { sftp.remove_dir(path.clone()).await } else { sftp.remove_file(path.clone()).await }.with_context(|| format!("deleting {path}"))?;
        }
    }
    Ok(())
}

async fn chmod(sftp: &SftpSession, paths: &[String], mode: u32, recursive: bool) -> Result<()> {
    for root in paths {
        // Recursively, links are skipped: chmod follows them, and they may point anywhere (~/.ssh...).
        let targets = if recursive { walk(sftp, root, false).await?.into_iter().filter(|(_, a)| !a.is_symlink()).map(|(p, _)| p).collect() } else { vec![root.clone()] };
        for path in targets {
            let attrs = FileAttributes { permissions: Some(mode & 0o7777), ..FileAttributes::empty() };
            sftp.set_metadata(path.clone(), attrs).await.with_context(|| format!("changing the permissions of {path}"))?;
        }
    }
    Ok(())
}

/// Throttled progress reports.
struct Progress<'a> {
    id: u64,
    emit: &'a Emitter,
    total: u64,
    done: u64,
    last: Instant,
}

impl Progress<'_> {
    fn add(&mut self, n: u64, current: &str) {
        self.done += n;
        if self.last.elapsed() > Duration::from_millis(150) {
            self.last = Instant::now();
            self.emit.send(Event::Progress { id: self.id, done: self.done, total: self.total, current: current.to_owned() });
        }
    }
}

const CHUNK: usize = 256 * 1024;

/// Returns how many files were skipped (they existed and `overwrite` is off).
async fn download(sftp: &SftpSession, remote: &[String], local_dir: &Path, overwrite: bool, id: u64, emit: &Emitter, cancel: &AtomicBool) -> Result<u64> {
    // Everything to copy, first: the total size gives a real progress bar.
    let mut plan = Vec::new();
    for root in remote {
        let base = parent(root);
        for (path, attrs) in walk(sftp, root, true).await? {
            let relative = path.strip_prefix(&base).unwrap_or(&path).trim_start_matches('/').to_owned();
            let local = local_dir.join(&relative);
            // Second line of defence: the local path stays inside the chosen folder.
            let plain = std::path::Path::new(&relative).components().all(|c| matches!(c, std::path::Component::Normal(_)));
            anyhow::ensure!(plain && local.starts_with(local_dir), "refusing to write outside {}: {relative:?}", local_dir.display());
            plan.push((path, local, attrs));
        }
    }
    let total = plan.iter().filter(|(_, _, a)| !a.is_dir()).map(|(_, _, a)| a.size.unwrap_or(0)).sum();
    let mut progress = Progress { id, emit, total, done: 0, last: Instant::now() };
    let mut buf = vec![0u8; CHUNK];
    let mut skipped = 0;
    for (remote, local, attrs) in plan {
        anyhow::ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
        if attrs.is_dir() {
            tokio::fs::create_dir_all(&local).await.with_context(|| format!("creating {}", local.display()))?;
            continue;
        }
        if attrs.is_symlink() {
            continue;
        }
        // symlink_metadata: an existing link in the folder is never written through.
        let existing = std::fs::symlink_metadata(&local).ok();
        if existing.as_ref().is_some_and(|m| m.file_type().is_symlink()) {
            anyhow::bail!("{} is a symbolic link: not replaced", local.display());
        }
        if !overwrite && existing.is_some() {
            progress.add(attrs.size.unwrap_or(0), &remote);
            skipped += 1;
            continue;
        }
        // Written aside, then moved into place: an error or a cancel never leaves a truncated file.
        let part = local.with_file_name(format!(".{}.ronnie-part", local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        let expected = attrs.size.unwrap_or(u64::MAX);
        let mut src = sftp.open(remote.clone()).await.with_context(|| format!("opening {remote}"))?;
        let mut dst = tokio::fs::File::create(&part).await.with_context(|| format!("creating {}", part.display()))?;
        let mut received = 0u64;
        let copied: Result<()> = async {
            loop {
                anyhow::ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
                let n = src.read(&mut buf).await.with_context(|| format!("reading {remote}"))?;
                if n == 0 {
                    break;
                }
                received += n as u64;
                // A server sending more than it announced could fill the disk.
                anyhow::ensure!(received <= expected.saturating_add(1 << 20), "{remote}: the server sends more data than announced");
                dst.write_all(&buf[..n]).await?;
                progress.add(n as u64, &remote);
            }
            dst.flush().await?;
            Ok(())
        }
        .await;
        drop(dst);
        if let Err(e) = copied {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(e);
        }
        tokio::fs::rename(&part, &local).await.with_context(|| format!("writing {}", local.display()))?;
        #[cfg(unix)]
        if let Some(mode) = attrs.permissions {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&local, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    emit.send(Event::Progress { id, done: total, total, current: String::new() });
    Ok(skipped)
}

/// Returns how many files were skipped (they existed and `overwrite` is off).
async fn upload(sftp: &SftpSession, local: &[PathBuf], remote_dir: &str, overwrite: bool, id: u64, emit: &Emitter, cancel: &AtomicBool) -> Result<u64> {
    let mut plan: Vec<(PathBuf, String, bool, u64)> = Vec::new();
    for root in local {
        let base = root.parent().unwrap_or(Path::new("/"));
        let mut stack = vec![root.clone()];
        while let Some(path) = stack.pop() {
            // A link chosen by the user is followed; links met inside folders are not.
            let meta = if path == *root { std::fs::metadata(&path) } else { std::fs::symlink_metadata(&path) }.with_context(|| format!("reading {}", path.display()))?;
            let relative = path.strip_prefix(base).unwrap_or(&path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            let target = join(remote_dir, &relative);
            if meta.is_dir() {
                plan.push((path.clone(), target, true, 0));
                for child in std::fs::read_dir(&path)?.flatten() {
                    stack.push(child.path());
                }
            } else if meta.is_file() {
                plan.push((path, target, false, meta.len()));
            }
        }
    }
    let total = plan.iter().map(|(_, _, _, size)| size).sum();
    let mut progress = Progress { id, emit, total, done: 0, last: Instant::now() };
    let mut buf = vec![0u8; CHUNK];
    let mut skipped = 0;
    for (local, remote, is_dir, size) in plan {
        anyhow::ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
        if is_dir {
            // Already there is fine.
            if sftp.metadata(remote.clone()).await.is_err() {
                sftp.create_dir(remote.clone()).await.with_context(|| format!("creating {remote}"))?;
            }
            continue;
        }
        if !overwrite && sftp.metadata(remote.clone()).await.is_ok() {
            progress.add(size, &remote);
            skipped += 1;
            continue;
        }
        let mut src = tokio::fs::File::open(&local).await.with_context(|| format!("opening {}", local.display()))?;
        let mut dst = sftp.create(remote.clone()).await.with_context(|| format!("creating {remote}"))?;
        let copied: Result<()> = async {
            loop {
                anyhow::ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
                let n = src.read(&mut buf).await.with_context(|| format!("reading {}", local.display()))?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n]).await.with_context(|| format!("writing {remote}"))?;
                progress.add(n as u64, &remote);
            }
            dst.shutdown().await.with_context(|| format!("writing {remote}"))?;
            Ok(())
        }
        .await;
        // No truncated file left on the server (a later try would skip it as "existing").
        if let Err(e) = copied {
            drop(dst);
            let _ = sftp.remove_file(remote.clone()).await;
            return Err(e);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&local) {
                let attrs = FileAttributes { permissions: Some(meta.permissions().mode() & 0o777), ..FileAttributes::empty() };
                let _ = sftp.set_metadata(remote.clone(), attrs).await;
            }
        }
    }
    emit.send(Event::Progress { id, done: total, total, current: String::new() });
    Ok(skipped)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// OpenSSH's sftp-server speaks SFTP over stdio: a real server without ssh or network (macOS has
    /// it; on Linux it comes with the OpenSSH server).
    fn server() -> Option<&'static str> {
        ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server", "/usr/lib/ssh/sftp-server", "/usr/libexec/openssh/sftp-server"].into_iter().find(|p| std::path::Path::new(p).exists())
    }

    fn connect() -> Connection {
        Connection::open(&egui::Context::default(), server().expect("sftp-server"), &[], &[]).unwrap()
    }

    fn wait(conn: &Connection, mut pred: impl FnMut(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            for e in conn.poll() {
                if pred(&e) {
                    return e;
                }
                if let Event::Error(msg) | Event::Closed(msg) = &e {
                    panic!("{msg}");
                }
            }
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn lists_transfers_and_changes_permissions() {
        if server().is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("ronnie-sftp-{}", std::process::id()));
        let (local, remote) = (base.join("local"), base.join("remote"));
        std::fs::create_dir_all(local.join("dir/sub")).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::write(local.join("dir/a.txt"), b"hello").unwrap();
        std::fs::write(local.join("dir/sub/big.bin"), vec![7u8; 600_000]).unwrap();
        let remote_s = remote.canonicalize().unwrap().display().to_string();

        let conn = connect();
        wait(&conn, |e| matches!(e, Event::Connected { .. }));

        // Upload a directory tree.
        conn.send(Request::Upload { id: 1, local: vec![local.join("dir")], remote_dir: remote_s.clone(), overwrite: false });
        let Event::Finished { result, .. } = wait(&conn, |e| matches!(e, Event::Finished { id: 1, .. })) else { unreachable!() };
        result.unwrap();
        assert_eq!(std::fs::read(remote.join("dir/sub/big.bin")).unwrap().len(), 600_000);

        // List it.
        conn.send(Request::List(join(&remote_s, "dir")));
        let Event::Listing { entries, .. } = wait(&conn, |e| matches!(e, Event::Listing { .. })) else { unreachable!() };
        let mut names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
        names.sort();
        assert_eq!(names, [("a.txt", false), ("sub", true)]);

        // Permissions.
        conn.send(Request::Chmod { paths: vec![join(&remote_s, "dir/a.txt")], mode: 0o640, recursive: false });
        wait(&conn, |e| matches!(e, Event::Changed));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(remote.join("dir/a.txt")).unwrap().permissions().mode() & 0o777, 0o640);

        // Download back elsewhere, then rename and delete remotely.
        let back = base.join("back");
        std::fs::create_dir_all(&back).unwrap();
        conn.send(Request::Download { id: 2, remote: vec![join(&remote_s, "dir")], local_dir: back.clone(), overwrite: false });
        let Event::Finished { result, .. } = wait(&conn, |e| matches!(e, Event::Finished { id: 2, .. })) else { unreachable!() };
        result.unwrap();
        assert_eq!(std::fs::read(back.join("dir/a.txt")).unwrap(), b"hello");
        conn.send(Request::Rename(join(&remote_s, "dir"), join(&remote_s, "renamed")));
        wait(&conn, |e| matches!(e, Event::Changed));
        conn.send(Request::Remove(vec![join(&remote_s, "renamed")]));
        wait(&conn, |e| matches!(e, Event::Changed));
        assert!(std::fs::read_dir(&remote).unwrap().next().is_none(), "deleted recursively");

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn notices_a_dead_session() {
        if server().is_none() {
            return;
        }
        let conn = connect();
        wait(&conn, |e| matches!(e, Event::Connected { .. }));
        // The server goes away after connecting (network drop, server closing): it must be noticed.
        unsafe { libc::kill(conn.pid.unwrap() as libc::pid_t, libc::SIGKILL) };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if conn.poll().iter().any(|e| matches!(e, Event::Closed(_))) {
                break;
            }
            assert!(Instant::now() < deadline, "the dead session was not noticed");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn cancels_a_queued_transfer() {
        if server().is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("ronnie-sftp-cancel-{}", std::process::id()));
        std::fs::create_dir_all(base.join("remote")).unwrap();
        std::fs::write(base.join("f.bin"), vec![1u8; 4_000_000]).unwrap();
        let remote = base.join("remote").canonicalize().unwrap().display().to_string();
        let conn = connect();
        wait(&conn, |e| matches!(e, Event::Connected { .. }));
        conn.send(Request::Upload { id: 1, local: vec![base.join("f.bin")], remote_dir: remote.clone(), overwrite: true });
        conn.send(Request::Upload { id: 2, local: vec![base.join("f.bin")], remote_dir: join(&remote, "nope"), overwrite: true });
        conn.cancel(2);
        let Event::Finished { result, .. } = wait(&conn, |e| matches!(e, Event::Finished { id: 2, .. })) else { unreachable!() };
        assert_eq!(result.unwrap_err(), "cancelled");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refuses_suspect_names() {
        for bad in ["", ".", "..", "../../.zshenv", "a/b", "x\0y"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        for good in ["index.html", ".env", "..hidden", "a b", "été.txt"] {
            assert!(valid_name(good), "{good:?}");
        }
    }

    #[test]
    fn remote_paths() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/var/www", "html"), "/var/www/html");
        assert_eq!(parent("/var/www/html"), "/var/www");
        assert_eq!(parent("/var"), "/");
        assert_eq!(parent("/"), "/");
    }
}
