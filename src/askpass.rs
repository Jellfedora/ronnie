//! Typing saved SSH passwords, safely. ssh runs Ronnie as its SSH_ASKPASS helper; that helper doesn't
//! read the keychain itself (any program could run it and get the passwords): it asks the Ronnie window
//! over a private local socket. The window answers only when the asking process is the child of an ssh
//! it started itself in one of its panes — so a jump host's ssh (a grandchild) doesn't get the target's
//! password — and only once per connection: after a wrong password, ssh asks again and the user types.
//!
//! Unix only; on Windows the helper falls back to the keychain (see `ssh::run_askpass`).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use uuid::Uuid;

/// Socket the helper connects to, set once the window listens (passed to ssh in its environment).
static SOCKET: OnceLock<PathBuf> = OnceLock::new();

pub fn socket_path() -> Option<&'static Path> {
    SOCKET.get().map(PathBuf::as_path)
}

#[derive(Default)]
struct State {
    /// ssh processes started by this window, and the host whose password they may get.
    allowed: HashMap<u32, Uuid>,
    /// Those that already got it once.
    answered: HashSet<u32>,
    /// Those without a terminal (SFTP): their other prompts are asked in the window.
    interactive: HashSet<u32>,
    /// Those whose question the user cancelled: not asked again (ssh retries a few times).
    refused: HashSet<u32>,
    /// Host names, to show which server asks.
    names: HashMap<Uuid, String>,
    /// Where those questions go (the window), and how to wake it up.
    prompter: Option<(std::sync::mpsc::Sender<Prompt>, egui::Context)>,
}

/// A question from ssh (password, new host key...) for the user, in the window.
pub struct Prompt {
    /// The server asking (the text itself comes from the server and could pretend anything).
    pub host: String,
    pub text: String,
    /// Typed text should be hidden (a password, not a yes/no).
    pub secret: bool,
    /// The answer, or None if the user cancelled.
    pub reply: std::sync::mpsc::Sender<Option<String>>,
}

/// The window's side: listens for helpers and knows which ssh processes it started.
#[derive(Clone, Default)]
pub struct Server {
    state: Arc<Mutex<State>>,
}

impl Server {
    /// Questions from ssh processes without a terminal go to `prompts`; `ctx` is woken up for them.
    pub fn set_prompter(&self, prompts: std::sync::mpsc::Sender<Prompt>, ctx: &egui::Context) {
        self.state.lock().unwrap().prompter = Some((prompts, ctx.clone()));
    }

    /// `ssh_pid` (SFTP, no terminal) may get the saved password of `host`, and its other prompts are
    /// asked in the window.
    pub fn allow_interactive(&self, ssh_pid: u32, host: Uuid, name: &str) {
        self.allow(ssh_pid, host);
        let mut state = self.state.lock().unwrap();
        state.interactive.insert(ssh_pid);
        state.refused.remove(&ssh_pid);
        state.names.insert(host, name.to_owned());
    }

    /// Starts listening in the background. Without a socket, helpers ask on the terminal.
    pub fn start() -> Self {
        let server = Self::default();
        let Some(path) = pick_path() else { return server };
        let _ = std::fs::remove_file(&path);
        let Ok(listener) = UnixListener::bind(&path) else { return server };
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        let _ = SOCKET.set(path);
        let state = server.state.clone();
        let _ = thread::Builder::new().name("askpass".into()).spawn(move || {
            // One thread per request: a question waiting for the user mustn't hold the others.
            for stream in listener.incoming().flatten() {
                let state = state.clone();
                let _ = thread::Builder::new().name("askpass-request".into()).spawn(move || answer(stream, &state));
            }
        });
        server
    }

    /// `ssh_pid` (started in a pane) may get the saved password of `host`.
    pub fn allow(&self, ssh_pid: u32, host: Uuid) {
        let mut state = self.state.lock().unwrap();
        // A new process (possibly reusing an old pid) gets its own single try.
        if state.allowed.insert(ssh_pid, host) != Some(host) {
            state.answered.remove(&ssh_pid);
        }
    }
}

/// Removes the socket file (on quit).
pub fn cleanup() {
    if let Some(path) = SOCKET.get() {
        let _ = std::fs::remove_file(path);
    }
}

/// A short path (socket paths are limited to about 100 bytes) in a directory only the user can read.
fn pick_path() -> Option<PathBuf> {
    let name = format!("ronnie-askpass-{}.sock", std::process::id());
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Some(PathBuf::from(dir).join(name));
    }
    // macOS: the per-user temporary directory is private.
    if cfg!(target_os = "macos") {
        return Some(std::env::temp_dir().join(name));
    }
    crate::config::config_dir().map(|d| d.join(name))
}

/// One helper request: a prompt line in, "OK\n<password>\n" or "NO\n" out.
fn answer(stream: UnixStream, state: &Mutex<State>) -> std::io::Result<()> {
    let helper = peer_pid(&stream);
    // Who asks is checked before reading anything, and an idle connection doesn't hold a thread.
    if helper.and_then(parent_pid).is_none_or(|ssh| !state.lock().unwrap().allowed.contains_key(&ssh)) {
        return (&stream).write_all(b"NO\n");
    }
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut prompt = String::new();
    BufReader::new((&stream).take(4096)).read_line(&mut prompt)?;
    let prompt = prompt.trim_end_matches(['\r', '\n']).to_owned();
    let ssh = helper.and_then(parent_pid);
    let (host, interactive, prompter) = {
        let mut state = state.lock().unwrap();
        let Some(ssh) = ssh.filter(|ssh| state.allowed.contains_key(ssh)) else {
            drop(state);
            return (&stream).write_all(b"NO\n");
        };
        // One automatic try per connection, and only for a password prompt.
        let lower = prompt.to_lowercase();
        let first_password = lower.contains("password") && !lower.contains("passphrase") && state.answered.insert(ssh);
        let id = state.allowed[&ssh];
        let host = first_password.then_some(id);
        let name = state.names.get(&id).cloned().unwrap_or_default();
        let interactive = state.interactive.contains(&ssh) && !state.refused.contains(&ssh);
        (host, interactive, state.prompter.clone().map(|p| (p, name, ssh)))
    };
    let answer = host.and_then(crate::ssh::load_password).or_else(|| {
        // No terminal to ask on: ask in the window, and wait for the user.
        let ((prompts, ctx), name, ssh) = prompter.filter(|_| interactive)?;
        let (reply, answer) = std::sync::mpsc::channel();
        let secret = !prompt.to_lowercase().contains("yes/no");
        // Shown as plain text: no control or text-direction characters, and not endless.
        let text: String = prompt.chars().filter(|c| !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')).take(500).collect();
        prompts.send(Prompt { host: name, text, secret, reply }).ok()?;
        ctx.request_repaint();
        let answer = answer.recv_timeout(std::time::Duration::from_secs(300)).ok().flatten();
        if answer.is_none() {
            state.lock().unwrap().refused.insert(ssh);
        }
        answer
    });
    let mut out = &stream;
    match answer {
        Some(answer) => write!(out, "OK\n{answer}\n"),
        None => out.write_all(b"NO\n"),
    }
}

/// The helper's side: asks the window. None when it has no answer (then ask on the terminal).
pub fn ask_window(socket: &Path, prompt: &str) -> Option<String> {
    let mut stream = UnixStream::connect(socket).ok()?;
    writeln!(stream, "{}", prompt.replace('\n', " ")).ok()?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status).ok()?;
    if status.trim_end() != "OK" {
        return None;
    }
    let mut password = String::new();
    reader.read_line(&mut password).ok()?;
    Some(password.trim_end_matches(['\r', '\n']).to_owned())
}

#[cfg(target_os = "macos")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: `pid` and `len` are valid for writing; the fd is an open socket.
    let ok = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&raw mut pid).cast(), &mut len) } == 0;
    (ok && pid > 0).then_some(pid as u32)
}

#[cfg(not(target_os = "macos"))]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for writing; the fd is an open socket.
    let ok = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut cred).cast(), &mut len) } == 0;
    (ok && cred.pid > 0).then_some(cred.pid as u32)
}

#[cfg(target_os = "macos")]
fn parent_pid(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable proc_bsdinfo.
    let n = unsafe { libc::proc_pidinfo(pid as libc::pid_t, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (n == size).then_some(info.pbi_ppid)
}

#[cfg(not(target_os = "macos"))]
fn parent_pid(pid: u32) -> Option<u32> {
    // /proc/<pid>/stat: "pid (comm) state ppid ...", comm may hold spaces and parentheses.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_own_parent() {
        assert_eq!(parent_pid(std::process::id()), Some(unsafe { libc::getppid() } as u32));
    }

    #[test]
    fn answers_only_allowed_ssh_once() {
        let state = Arc::new(Mutex::new(State::default()));
        let (ours, theirs) = UnixStream::pair().unwrap();
        // The "helper" is this test process; pretend its parent is an ssh started in a pane, whose host
        // has no saved password: the window answers NO, and the next try isn't even looked up.
        state.lock().unwrap().allowed.insert(unsafe { libc::getppid() } as u32, Uuid::new_v4());
        let mut helper = theirs;
        writeln!(helper, "demo@host's password: ").unwrap();
        answer(ours, &state).unwrap();
        let mut reply = String::new();
        helper.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "NO\n");
        assert_eq!(state.lock().unwrap().answered.len(), 1, "counted as tried");

        let (ours, mut helper) = UnixStream::pair().unwrap();
        state.lock().unwrap().allowed.clear();
        writeln!(helper, "password: ").unwrap();
        answer(ours, &state).unwrap();
        let mut reply = String::new();
        helper.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "NO\n", "a process no pane started gets nothing");
    }
}
