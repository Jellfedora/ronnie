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
    Cancel(u64),
}

#[derive(Debug)]
pub enum Event {
    /// The session is ready; `home` is the remote home directory.
    Connected { home: String },
    Listing { path: String, entries: Vec<Entry> },
    /// A change (mkdir, rename, remove, chmod) is done: the listing may be out of date.
    Changed,
    Error(String),
    Progress { id: u64, done: u64, total: u64, current: String },
    Finished { id: u64, result: Result<(), String> },
    /// The connection is over (with ssh's last words, if any).
    Closed(String),
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("sftp").enable_all().build().expect("tokio runtime"))
}

/// A running SFTP session.
pub struct Connection {
    requests: tmpsc::UnboundedSender<Request>,
    events: Receiver<Event>,
    /// ssh's process id: the askpass helper answers it (see askpass.rs).
    pub pid: Option<u32>,
}

impl Connection {
    /// Starts `program args` (ssh with the subsystem, or an sftp-server in tests) and opens a session
    /// over its standard input and output.
    pub fn open(ctx: &egui::Context, program: &str, args: &[String], env: &[(String, String)]) -> Result<Self> {
        let _guard = runtime().enter();
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args).envs(env.iter().cloned()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        let mut child = cmd.spawn().with_context(|| format!("starting {program}"))?;
        let pid = child.id();
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;

        let (requests, mut incoming) = tmpsc::unbounded_channel();
        let (tx, events) = mpsc::channel();
        let emit = Emitter { tx, ctx: ctx.clone() };
        // ssh's error output, for the "connection closed" message.
        let last_words = Arc::new(Mutex::new(String::new()));
        {
            let last_words = last_words.clone();
            runtime().spawn(async move {
                let mut stderr = stderr;
                let mut buf = Vec::new();
                let _ = stderr.read_to_end(&mut buf).await;
                *last_words.lock().unwrap() = String::from_utf8_lossy(&buf).trim().to_owned();
            });
        }
        runtime().spawn(async move {
            let sftp = match SftpSession::new(tokio::io::join(stdout, stdin)).await {
                Ok(sftp) => Arc::new(sftp),
                Err(e) => {
                    let _ = child.wait().await;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let words = last_words.lock().unwrap().clone();
                    emit.send(Event::Closed(if words.is_empty() { e.to_string() } else { words }));
                    return;
                }
            };
            let home = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into());
            emit.send(Event::Connected { home });

            // Transfers run one after the other in their own task, so that browsing stays responsive.
            let cancels: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>> = Arc::default();
            let (jobs, mut queue) = tmpsc::unbounded_channel::<Request>();
            {
                let (sftp, emit, cancels) = (sftp.clone(), emit.clone(), cancels.clone());
                tokio::spawn(async move {
                    while let Some(job) = queue.recv().await {
                        let (id, result) = match job {
                            Request::Download { id, remote, local_dir, overwrite } => {
                                let cancel = cancels.lock().unwrap().get(&id).cloned().unwrap_or_default();
                                (id, download(&sftp, &remote, &local_dir, overwrite, id, &emit, &cancel).await)
                            }
                            Request::Upload { id, local, remote_dir, overwrite } => {
                                let cancel = cancels.lock().unwrap().get(&id).cloned().unwrap_or_default();
                                (id, upload(&sftp, &local, &remote_dir, overwrite, id, &emit, &cancel).await)
                            }
                            _ => continue,
                        };
                        cancels.lock().unwrap().remove(&id);
                        emit.send(Event::Finished { id, result: result.map_err(|e| format!("{e:#}")) });
                        emit.send(Event::Changed);
                    }
                });
            }

            while let Some(request) = incoming.recv().await {
                let result = match request {
                    Request::List(path) => list(&sftp, &path).await.map(|entries| emit.send(Event::Listing { path, entries })),
                    Request::Mkdir(path) => sftp.create_dir(path).await.map_err(anyhow::Error::from).map(|_| emit.send(Event::Changed)),
                    Request::Rename(from, to) => sftp.rename(from, to).await.map_err(anyhow::Error::from).map(|_| emit.send(Event::Changed)),
                    Request::Remove(paths) => remove(&sftp, &paths).await.map(|_| emit.send(Event::Changed)),
                    Request::Chmod { paths, mode, recursive } => chmod(&sftp, &paths, mode, recursive).await.map(|_| emit.send(Event::Changed)),
                    Request::Cancel(id) => {
                        if let Some(flag) = cancels.lock().unwrap().get(&id) {
                            flag.store(true, Ordering::Relaxed);
                        }
                        Ok(())
                    }
                    job @ (Request::Download { .. } | Request::Upload { .. }) => {
                        let id = match &job {
                            Request::Download { id, .. } | Request::Upload { id, .. } => *id,
                            _ => unreachable!(),
                        };
                        cancels.lock().unwrap().insert(id, Arc::default());
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

        // Tell the file manager when ssh exits (network drop, server closed the session...).
        Ok(Self { requests, events, pid })
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
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

async fn list(sftp: &SftpSession, path: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in sftp.read_dir(path).await.with_context(|| format!("listing {path}"))? {
        let name = item.file_name();
        if name == "." || name == ".." {
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

/// Every path under `root` (itself included), directories before their contents.
async fn walk(sftp: &SftpSession, root: &str) -> Result<Vec<(String, FileAttributes)>> {
    let attrs = sftp.symlink_metadata(root).await.with_context(|| format!("reading {root}"))?;
    let mut out = vec![(root.to_owned(), attrs.clone())];
    let mut stack = if attrs.is_dir() { vec![root.to_owned()] } else { Vec::new() };
    while let Some(dir) = stack.pop() {
        for item in sftp.read_dir(dir.clone()).await? {
            let name = item.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let path = join(&dir, &name);
            let attrs = item.metadata();
            if attrs.is_dir() {
                stack.push(path.clone());
            }
            out.push((path, attrs));
        }
    }
    Ok(out)
}

async fn remove(sftp: &SftpSession, paths: &[String]) -> Result<()> {
    for root in paths {
        let mut all = walk(sftp, root).await?;
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
        let targets = if recursive { walk(sftp, root).await?.into_iter().map(|(p, _)| p).collect() } else { vec![root.clone()] };
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

async fn download(sftp: &SftpSession, remote: &[String], local_dir: &Path, overwrite: bool, id: u64, emit: &Emitter, cancel: &AtomicBool) -> Result<()> {
    // Everything to copy, first: the total size gives a real progress bar.
    let mut plan = Vec::new();
    for root in remote {
        let base = parent(root);
        for (path, attrs) in walk(sftp, root).await? {
            let relative = path.strip_prefix(&base).unwrap_or(&path).trim_start_matches('/').to_owned();
            plan.push((path, local_dir.join(relative), attrs));
        }
    }
    let total = plan.iter().filter(|(_, _, a)| !a.is_dir()).map(|(_, _, a)| a.size.unwrap_or(0)).sum();
    let mut progress = Progress { id, emit, total, done: 0, last: Instant::now() };
    let mut buf = vec![0u8; CHUNK];
    for (remote, local, attrs) in plan {
        if attrs.is_dir() {
            tokio::fs::create_dir_all(&local).await.with_context(|| format!("creating {}", local.display()))?;
            continue;
        }
        if attrs.is_symlink() {
            continue;
        }
        if !overwrite && local.exists() {
            progress.add(attrs.size.unwrap_or(0), &remote);
            continue;
        }
        let mut src = sftp.open(remote.clone()).await.with_context(|| format!("opening {remote}"))?;
        let mut dst = tokio::fs::File::create(&local).await.with_context(|| format!("creating {}", local.display()))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(dst);
                let _ = tokio::fs::remove_file(&local).await;
                anyhow::bail!("cancelled");
            }
            let n = src.read(&mut buf).await.with_context(|| format!("reading {remote}"))?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            progress.add(n as u64, &remote);
        }
        dst.flush().await?;
        #[cfg(unix)]
        if let Some(mode) = attrs.permissions {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&local, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    emit.send(Event::Progress { id, done: total, total, current: String::new() });
    Ok(())
}

async fn upload(sftp: &SftpSession, local: &[PathBuf], remote_dir: &str, overwrite: bool, id: u64, emit: &Emitter, cancel: &AtomicBool) -> Result<()> {
    let mut plan: Vec<(PathBuf, String, bool, u64)> = Vec::new();
    for root in local {
        let base = root.parent().unwrap_or(Path::new("/"));
        let mut stack = vec![root.clone()];
        while let Some(path) = stack.pop() {
            let meta = std::fs::symlink_metadata(&path).with_context(|| format!("reading {}", path.display()))?;
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
    for (local, remote, is_dir, size) in plan {
        if is_dir {
            // Already there is fine.
            if sftp.metadata(remote.clone()).await.is_err() {
                sftp.create_dir(remote.clone()).await.with_context(|| format!("creating {remote}"))?;
            }
            continue;
        }
        if !overwrite && sftp.metadata(remote.clone()).await.is_ok() {
            progress.add(size, &remote);
            continue;
        }
        let mut src = tokio::fs::File::open(&local).await.with_context(|| format!("opening {}", local.display()))?;
        let mut dst = sftp.create(remote.clone()).await.with_context(|| format!("creating {remote}"))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(dst);
                let _ = sftp.remove_file(remote.clone()).await;
                anyhow::bail!("cancelled");
            }
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await.with_context(|| format!("writing {remote}"))?;
            progress.add(n as u64, &remote);
        }
        dst.shutdown().await.with_context(|| format!("writing {remote}"))?;
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
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// The macOS sftp-server speaks SFTP over stdio: a real server without ssh or network.
    fn connect() -> Connection {
        Connection::open(&egui::Context::default(), "/usr/libexec/sftp-server", &[], &[]).unwrap()
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
    fn remote_paths() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/var/www", "html"), "/var/www/html");
        assert_eq!(parent("/var/www/html"), "/var/www");
        assert_eq!(parent("/var"), "/");
        assert_eq!(parent("/"), "/");
    }
}
