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

/// What to apply: in each item's mode, `clear` bits are removed, then `set` bits added.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct ModeChange {
    pub set: u32,
    pub clear: u32,
}

impl ModeChange {
    pub fn apply(self, mode: u32) -> u32 {
        (mode & !self.clear | self.set) & 0o7777
    }
}

/// Which items a recursive change reaches.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Scope {
    #[default]
    All,
    Files,
    Dirs,
}

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
    /// Each item gets `change` applied to its own mode (recursively: those `scope` picks).
    Chmod { paths: Vec<String>, change: ModeChange, recursive: bool, scope: Scope },
    Download { id: u64, remote: Vec<String>, local_dir: PathBuf, overwrite: bool },
    Upload { id: u64, local: Vec<PathBuf>, remote_dir: String, overwrite: bool },
    /// Moves `from` (paths) into the directory `dir`. Items already there: a folder is merged (its
    /// contents moved into the one there), a file replaced if `overwrite` is true, kept (the one moved
    /// stays) if false; None: nothing is moved, MoveConflict tells which exist.
    Move { from: Vec<String>, dir: String, overwrite: Option<bool> },
    /// The total size of what these folders hold (each: DirSize).
    DirSize(Vec<String>),
    /// An empty file (refused if the name is taken).
    CreateFile(String),
    /// Copies a file or folder (recursively) on the server, to a name that must be free.
    Duplicate { from: String, to: String },
    /// Part of a file (the viewer of big files): `len` bytes from `offset`, or the last `len` if `tail`.
    ReadRange { path: String, offset: u64, len: u64, tail: bool },
    /// A file to edit (at most `MAX_EDIT` bytes).
    ReadFile(String),
    /// Saves an edited file in place (keeping its owner and permissions). Unless `force`, refused when it
    /// changed on the server since it was read (its modification time is no longer `mtime`).
    WriteFile { path: String, data: Vec<u8>, mtime: Option<i64>, force: bool },
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
    /// The total size of the files under a folder, or why it couldn't be counted.
    DirSize { path: String, result: Result<u64, String> },
    /// Nothing moved: these names already exist in `dir`.
    MoveConflict { from: Vec<String>, dir: String, existing: Vec<String> },
    Error(String),
    Progress { id: u64, done: u64, total: u64, current: String },
    /// Done: how many files were skipped because they already existed, or why it failed.
    Finished { id: u64, result: Result<u64, String> },
    /// Part of a file: where it starts, and the file's size now.
    Range { path: String, result: Result<(u64, Vec<u8>, u64), String> },
    /// How much of a file to edit has arrived.
    FileProgress { path: String, done: u64, total: u64 },
    /// A file to edit, with its modification time, or why it can't be read.
    FileRead { path: String, result: Result<(Vec<u8>, Option<i64>), String> },
    /// A file saved: its new modification time, or why it wasn't (`conflict`: it changed on the server).
    FileWritten { path: String, result: Result<Option<i64>, String>, conflict: bool },
    /// The connection is over (with ssh's last words, if any).
    Closed(String),
}

/// Largest file the editor opens.
pub const MAX_EDIT: u64 = 200 * 1024 * 1024;

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
                    Request::Move { from, dir, overwrite } => move_items(&sftp, from, dir, overwrite, &emit).await,
                    // In the background: a big tree takes a while to go through.
                    Request::DirSize(paths) => {
                        let (sftp, emit) = (sftp.clone(), emit.clone());
                        tokio::spawn(async move {
                            for path in paths {
                                let result = dir_size(&sftp, &path).await.map_err(|e| format!("{e:#}"));
                                emit.send(Event::DirSize { path, result });
                            }
                        });
                        Ok(())
                    }
                    Request::Remove(paths) => remove(&sftp, &paths).await.map(|_| emit.send(Event::Changed)),
                    Request::Chmod { paths, change, recursive, scope } => chmod(&sftp, &paths, change, recursive, scope).await.map(|_| emit.send(Event::Changed)),
                    Request::CreateFile(path) => {
                        let flags = russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::EXCLUDE | russh_sftp::protocol::OpenFlags::WRITE;
                        match sftp.open_with_flags(path.clone(), flags).await {
                            Ok(file) => file.close().await.map_err(anyhow::Error::from).map(|_| emit.send(Event::Changed)),
                            Err(e) => Err(anyhow::Error::from(e).context(format!("creating {path}"))),
                        }
                    }
                    // In the background: copying goes through this computer and may take a while.
                    Request::Duplicate { from, to } => {
                        let (sftp, emit) = (sftp.clone(), emit.clone());
                        tokio::spawn(async move {
                            let result = duplicate(&sftp, &from, &to).await;
                            if let Err(e) = result {
                                emit.send(Event::Error(format!("{e:#}")));
                            }
                            emit.send(Event::Changed);
                        });
                        Ok(())
                    }
                    Request::ReadRange { path, offset, len, tail } => {
                        let result = read_range(&sftp, &path, offset, len, tail).await.map_err(|e| format!("{e:#}"));
                        emit.send(Event::Range { path, result });
                        Ok(())
                    }
                    Request::ReadFile(path) => {
                        let result = read_file(&sftp, &path, &emit).await.map_err(|e| format!("{e:#}"));
                        emit.send(Event::FileRead { path, result });
                        Ok(())
                    }
                    Request::WriteFile { path, data, mtime, force } => {
                        let (result, conflict) = match write_file(&sftp, &path, &data, mtime, force).await {
                            Ok(Some(mtime)) => (Ok(mtime), false),
                            Ok(None) => (Err(String::new()), true),
                            Err(e) => (Err(format!("{e:#}")), false),
                        };
                        emit.send(Event::FileWritten { path, result, conflict });
                        Ok(())
                    }
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

/// The size of the files under `root` (links not followed).
async fn dir_size(sftp: &SftpSession, root: &str) -> Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![root.to_owned()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        if !visited.insert(dir.clone()) {
            continue;
        }
        for item in sftp.read_dir(dir.clone()).await.with_context(|| format!("listing {dir}"))? {
            let name = item.file_name();
            if name == "." || name == ".." || !valid_name(&name) {
                continue;
            }
            let attrs = item.metadata();
            if attrs.is_dir() && !attrs.is_symlink() {
                stack.push(join(&dir, &name));
            } else if !attrs.is_symlink() {
                total += attrs.size.unwrap_or(0);
            }
        }
    }
    Ok(total)
}

/// The last part of a remote path.
fn base_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

async fn move_items(sftp: &SftpSession, from: Vec<String>, dir: String, overwrite: Option<bool>, emit: &Emitter) -> Result<()> {
    let Some(overwrite) = overwrite else {
        let mut existing = Vec::new();
        for path in &from {
            let name = base_name(path);
            if sftp.try_exists(join(&dir, name)).await.unwrap_or(false) {
                existing.push(name.to_owned());
            }
        }
        if !existing.is_empty() {
            emit.send(Event::MoveConflict { from, dir, existing });
            return Ok(());
        }
        return Box::pin(move_items(sftp, from, dir, Some(false), emit)).await;
    };
    let mut result = Ok(());
    for path in &from {
        let to = join(&dir, base_name(path));
        if let Err(e) = move_entry(sftp, path.clone(), to, overwrite).await {
            result = Err(e);
            break;
        }
    }
    emit.send(Event::Changed);
    result.map(|_| ())
}

/// Moves `from` to `to`: renamed if `to` is free; folders merged; a file there replaced if `overwrite`.
/// How many items stayed (not replaced).
fn move_entry(sftp: &SftpSession, from: String, to: String, overwrite: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
    Box::pin(async move {
        let Ok(target) = sftp.symlink_metadata(to.clone()).await else {
            sftp.rename(from.clone(), to.clone()).await.with_context(|| format!("moving {from}"))?;
            return Ok(0);
        };
        let source = sftp.symlink_metadata(from.clone()).await.with_context(|| format!("reading {from}"))?;
        if source.is_dir() && target.is_dir() {
            let mut kept = 0;
            for item in sftp.read_dir(from.clone()).await.with_context(|| format!("listing {from}"))? {
                let name = item.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                kept += move_entry(sftp, join(&from, &name), join(&to, &name), overwrite).await?;
            }
            // Emptied: the folder moved goes.
            if kept == 0 {
                let _ = sftp.remove_dir(from).await;
            }
            return Ok(kept);
        }
        if !overwrite {
            return Ok(1);
        }
        remove(sftp, std::slice::from_ref(&to)).await?;
        sftp.rename(from.clone(), to).await.with_context(|| format!("moving {from}"))?;
        Ok(0)
    })
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

async fn chmod(sftp: &SftpSession, paths: &[String], change: ModeChange, recursive: bool, scope: Scope) -> Result<()> {
    for root in paths {
        // Recursively, links are skipped: chmod follows them, and they may point anywhere (~/.ssh...).
        let targets: Vec<(String, FileAttributes)> = if recursive {
            walk(sftp, root, false).await?.into_iter().filter(|(_, a)| !a.is_symlink()).filter(|(_, a)| match scope {
                Scope::All => true,
                Scope::Files => !a.is_dir(),
                Scope::Dirs => a.is_dir(),
            }).collect()
        } else {
            vec![(root.clone(), sftp.metadata(root.clone()).await.with_context(|| format!("reading {root}"))?)]
        };
        for (path, attrs) in targets {
            let mode = change.apply(attrs.permissions.unwrap_or(0o644));
            let attrs = FileAttributes { permissions: Some(mode), ..FileAttributes::empty() };
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

/// Copies `from` (a file, or a folder and everything in it, symbolic links aside) to `to`, which must
/// not exist yet.
async fn duplicate(sftp: &SftpSession, from: &str, to: &str) -> Result<()> {
    anyhow::ensure!(!sftp.try_exists(to).await.unwrap_or(true), "{to} already exists");
    let mut buf = vec![0u8; CHUNK];
    // Parents come before what they hold.
    for (path, attrs) in walk(sftp, from, false).await? {
        let relative = path.strip_prefix(from).unwrap_or("").trim_start_matches('/');
        let target = if relative.is_empty() { to.to_owned() } else { join(to, relative) };
        // Links are left out: servers disagree on the order of SSH_FXP_SYMLINK's arguments (OpenSSH has
        // them reversed), and a link made the wrong way round would land elsewhere.
        if attrs.is_symlink() {
            continue;
        }
        if attrs.is_dir() {
            sftp.create_dir(target.clone()).await.with_context(|| format!("creating {target}"))?;
        } else {
            let flags = russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::EXCLUDE | russh_sftp::protocol::OpenFlags::WRITE;
            let mut src = sftp.open(path.clone()).await.with_context(|| format!("opening {path}"))?;
            let mut dst = sftp.open_with_flags(target.clone(), flags).await.with_context(|| format!("creating {target}"))?;
            loop {
                let n = src.read(&mut buf).await.with_context(|| format!("reading {path}"))?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n]).await.with_context(|| format!("writing {target}"))?;
            }
            dst.close().await?;
        }
        if let Some(mode) = attrs.permissions {
            let _ = sftp.set_metadata(target, FileAttributes { permissions: Some(mode & 0o7777), ..FileAttributes::empty() }).await;
        }
    }
    Ok(())
}

/// Largest part of a file read at once.
pub const MAX_RANGE: u64 = 8 * 1024 * 1024;

/// (offset, bytes, file size).
async fn read_range(sftp: &SftpSession, path: &str, offset: u64, len: u64, tail: bool) -> Result<(u64, Vec<u8>, u64)> {
    use tokio::io::AsyncSeekExt;
    let mut file = sftp.open(path).await.with_context(|| format!("opening {path}"))?;
    let size = file.metadata().await.ok().and_then(|a| a.size).unwrap_or(0);
    let len = len.min(MAX_RANGE);
    let offset = if tail { size.saturating_sub(len) } else { offset.min(size) };
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut data = Vec::with_capacity(len as usize);
    (&mut file).take(len).read_to_end(&mut data).await.with_context(|| format!("reading {path}"))?;
    Ok((offset, data, size))
}

async fn read_file(sftp: &SftpSession, path: &str, emit: &Emitter) -> Result<(Vec<u8>, Option<i64>)> {
    let attrs = sftp.metadata(path).await.with_context(|| format!("reading {path}"))?;
    anyhow::ensure!(!attrs.is_dir(), "{path} is a folder");
    anyhow::ensure!(attrs.size.unwrap_or(0) <= MAX_EDIT, "too big to edit ({} MB at most)", MAX_EDIT / (1024 * 1024));
    let mut file = sftp.open(path).await.with_context(|| format!("opening {path}"))?;
    let total = attrs.size.unwrap_or(0);
    let mut data = Vec::with_capacity(total as usize);
    let mut buf = vec![0u8; CHUNK];
    let mut last = Instant::now();
    loop {
        let n = file.read(&mut buf).await.with_context(|| format!("reading {path}"))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        // Not more than the limit (a hostile server could send endless data).
        anyhow::ensure!(data.len() as u64 <= MAX_EDIT, "too big to edit");
        if last.elapsed() > Duration::from_millis(150) {
            last = Instant::now();
            emit.send(Event::FileProgress { path: path.to_owned(), done: data.len() as u64, total });
        }
    }
    anyhow::ensure!(data.len() as u64 <= MAX_EDIT, "too big to edit");
    Ok((data, attrs.mtime.map(i64::from)))
}

/// The new modification time; Ok(None) when the file changed on the server meanwhile (not written).
async fn write_file(sftp: &SftpSession, path: &str, data: &[u8], mtime: Option<i64>, force: bool) -> Result<Option<Option<i64>>> {
    if !force {
        let now = sftp.metadata(path).await.ok().and_then(|a| a.mtime.map(i64::from));
        if now != mtime {
            return Ok(None);
        }
    }
    // Written in place, so the file keeps its owner and permissions.
    let flags = russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::TRUNCATE | russh_sftp::protocol::OpenFlags::WRITE;
    let mut file = sftp.open_with_flags(path, flags).await.with_context(|| format!("opening {path}"))?;
    file.write_all(data).await.with_context(|| format!("writing {path}"))?;
    file.close().await?;
    Ok(Some(sftp.metadata(path).await.ok().and_then(|a| a.mtime.map(i64::from))))
}

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
        conn.send(Request::Chmod { paths: vec![join(&remote_s, "dir/a.txt")], change: ModeChange { set: 0o640, clear: 0o7777 }, recursive: false, scope: Scope::All });
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
    fn edits_a_file_in_place() {
        if server().is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("ronnie-edit-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("conf.yml");
        std::fs::write(&file, b"a: 1\nb: 22\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        let path = file.canonicalize().unwrap().display().to_string();

        let conn = connect();
        wait(&conn, |e| matches!(e, Event::Connected { .. }));
        conn.send(Request::ReadFile(path.clone()));
        let Event::FileRead { result, .. } = wait(&conn, |e| matches!(e, Event::FileRead { .. })) else { unreachable!() };
        let (data, mtime) = result.unwrap();
        assert_eq!(data, b"a: 1\nb: 22\n");

        // Shorter than before: the rest is cut, the permissions stay.
        conn.send(Request::WriteFile { path: path.clone(), data: b"a: 2\n".to_vec(), mtime, force: false });
        let Event::FileWritten { result, conflict, .. } = wait(&conn, |e| matches!(e, Event::FileWritten { .. })) else { unreachable!() };
        assert!(!conflict);
        result.unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"a: 2\n");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o640);

        // Changed meanwhile (another modification time): refused unless forced.
        conn.send(Request::WriteFile { path: path.clone(), data: b"x".to_vec(), mtime: Some(1), force: false });
        let Event::FileWritten { conflict, .. } = wait(&conn, |e| matches!(e, Event::FileWritten { .. })) else { unreachable!() };
        assert!(conflict);
        assert_eq!(std::fs::read(&file).unwrap(), b"a: 2\n");
        conn.send(Request::WriteFile { path: path.clone(), data: b"x".to_vec(), mtime: Some(1), force: true });
        let Event::FileWritten { result, .. } = wait(&conn, |e| matches!(e, Event::FileWritten { .. })) else { unreachable!() };
        result.unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"x");

        // Parts of a file (the viewer): from an offset, and the end.
        std::fs::write(&file, b"0123456789").unwrap();
        conn.send(Request::ReadRange { path: path.clone(), offset: 3, len: 4, tail: false });
        let Event::Range { result, .. } = wait(&conn, |e| matches!(e, Event::Range { .. })) else { unreachable!() };
        assert_eq!(result.unwrap(), (3, b"3456".to_vec(), 10));
        conn.send(Request::ReadRange { path: path.clone(), offset: 0, len: 3, tail: true });
        let Event::Range { result, .. } = wait(&conn, |e| matches!(e, Event::Range { .. })) else { unreachable!() };
        assert_eq!(result.unwrap(), (7, b"789".to_vec(), 10));

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn creates_and_duplicates() {
        if server().is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("ronnie-dup-{}", std::process::id()));
        std::fs::create_dir_all(base.join("dir/sub")).unwrap();
        std::fs::write(base.join("dir/sub/a.txt"), b"hello").unwrap();
        let root = base.canonicalize().unwrap().display().to_string();

        let conn = connect();
        wait(&conn, |e| matches!(e, Event::Connected { .. }));
        conn.send(Request::CreateFile(join(&root, "new.txt")));
        wait(&conn, |e| matches!(e, Event::Changed));
        assert_eq!(std::fs::read(base.join("new.txt")).unwrap(), b"");

        conn.send(Request::Duplicate { from: join(&root, "dir"), to: join(&root, "dir copy") });
        wait(&conn, |e| matches!(e, Event::Changed));
        assert_eq!(std::fs::read(base.join("dir copy/sub/a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(base.join("dir/sub/a.txt")).unwrap(), b"hello");

        // Never over an existing name.
        conn.send(Request::CreateFile(join(&root, "new.txt")));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !conn.poll().iter().any(|e| matches!(e, Event::Error(_))) {
            assert!(Instant::now() < deadline, "no error for an existing name");
            std::thread::sleep(Duration::from_millis(20));
        }
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
