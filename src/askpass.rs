//! Typing saved SSH passwords, safely. ssh runs Ronnie as its SSH_ASKPASS helper; that helper doesn't
//! read the saved passwords itself (any program could run it and get the passwords): it asks the Ronnie window
//! over a private local socket. The window answers only when the asking process is the child of an ssh
//! it started itself in one of its panes — so a jump host's ssh (a grandchild) doesn't get the target's
//! password — and only once per connection: after a wrong password, ssh asks again and the user types.
//! With a key file, the saved password also unlocks the key (ssh's own passphrase prompt), once too.
//!
//! Unix: a socket file only the user can open, the asking process told by the kernel. Windows: a named
//! pipe (local clients only), the asking process told by the system the same way.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use uuid::Uuid;

/// Socket (Windows: pipe) the helper connects to, set once the window listens (passed to ssh in its
/// environment).
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
    /// Those whose host doesn't use its saved password (asked each time, or a key).
    unsaved: HashSet<u32>,
    /// Those whose host logs in with a key file: the saved password is also its passphrase.
    key: HashSet<u32>,
    /// Those that already got it as the key's passphrase.
    unlocked: HashSet<u32>,
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

    /// `ssh_pid` (SFTP, no terminal) may get the saved password of `host` if the host logs in with
    /// it, and its other prompts are asked in the window.
    pub fn allow_interactive(&self, ssh_pid: u32, host: &crate::ssh::SshHost) {
        self.allow(ssh_pid, host);
        let mut state = self.state.lock().unwrap();
        if host.uses_saved_password() {
            state.unsaved.remove(&ssh_pid);
        } else {
            state.unsaved.insert(ssh_pid);
        }
        state.interactive.insert(ssh_pid);
        state.refused.remove(&ssh_pid);
        state.names.insert(host.id, host.name.clone());
    }

    /// Starts listening in the background. Without a socket, helpers ask on the terminal.
    #[cfg(unix)]
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
                let _ = thread::Builder::new().name("askpass-request".into()).spawn(move || {
                    // An idle connection doesn't hold a thread for long.
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                    answer(&stream, peer_pid(&stream), &state)
                });
            }
        });
        server
    }

    /// Starts listening in the background, on a named pipe. Without it, helpers ask on the terminal.
    #[cfg(windows)]
    pub fn start() -> Self {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX};
        use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};

        let server = Self::default();
        // A random name: another program can't have created it first to listen instead (and the first
        // instance is required to be new).
        let path = PathBuf::from(format!(r"\\.\pipe\ronnie-askpass-{}-{}", std::process::id(), Uuid::new_v4()));
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `name` is NUL-terminated UTF-16; no security attributes (the default: the user only
        // may write to it).
        let create = move |first: bool| unsafe {
            let mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
            CreateNamedPipeW(name.as_ptr(), mode, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, 4096, 4096, 0, std::ptr::null())
        };
        let first = create(true);
        if first == INVALID_HANDLE_VALUE {
            return server;
        }
        let _ = SOCKET.set(path);
        let state = server.state.clone();
        // Handles as numbers: raw pointers can't cross threads.
        let first = first as usize;
        let _ = thread::Builder::new().name("askpass".into()).spawn(move || {
            let mut next = first as windows_sys::Win32::Foundation::HANDLE;
            loop {
                // SAFETY: `next` is a pipe instance we created and own.
                let connected = unsafe { ConnectNamedPipe(next, std::ptr::null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                let mut helper = 0u32;
                // SAFETY: as above; `helper` is valid for writing.
                let known = connected && unsafe { GetNamedPipeClientProcessId(next, &mut helper) } != 0 && helper != 0;
                // SAFETY: the handle is ours and is closed by the File from now on.
                let pipe = unsafe { std::fs::File::from_raw_handle(next as _) };
                // The next helper connects to a new instance while this one is answered.
                next = create(false);
                if connected {
                    let state = state.clone();
                    let _ = thread::Builder::new().name("askpass-request".into()).spawn(move || {
                        let result = answer(&pipe, known.then_some(helper), &state);
                        // The helper reads the whole answer before the pipe closes.
                        // SAFETY: the handle is open for as long as `pipe` lives.
                        unsafe { FlushFileBuffers(pipe.as_raw_handle() as _) };
                        result
                    });
                }
                if next == INVALID_HANDLE_VALUE {
                    break;
                }
            }
        });
        server
    }

    /// `ssh_pid` (started in a pane) may get the saved password of `host`.
    pub fn allow(&self, ssh_pid: u32, host: &crate::ssh::SshHost) {
        let mut state = self.state.lock().unwrap();
        // A new process (possibly reusing an old pid) gets its own single try.
        if state.allowed.insert(ssh_pid, host.id) != Some(host.id) {
            state.answered.remove(&ssh_pid);
            state.unlocked.remove(&ssh_pid);
            state.refused.remove(&ssh_pid);
        }
        // Its saved password, if any, is typed for it; otherwise only its host key is asked in the window.
        if host.uses_saved_password() {
            state.unsaved.remove(&ssh_pid);
        } else {
            state.unsaved.insert(ssh_pid);
        }
        state.names.insert(host.id, host.name.clone());
        if host.auth_method() == crate::ssh::SshAuth::Key {
            state.key.insert(ssh_pid);
        } else {
            state.key.remove(&ssh_pid);
        }
    }
}

/// Removes the socket file (on quit; a pipe goes away by itself).
pub fn cleanup() {
    #[cfg(unix)]
    if let Some(path) = SOCKET.get() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
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

/// One helper request from process `helper`: a prompt line in, "OK\n<password>\n" or "NO\n" out.
fn answer<S: Read + Write>(mut stream: S, helper: Option<u32>, state: &Mutex<State>) -> std::io::Result<()> {
    let mut prompt = String::new();
    // Read even when the answer will be no: closing with unread data would reset the connection
    // (the helper would see an error instead of the answer).
    BufReader::new((&mut stream).take(4096)).read_line(&mut prompt)?;
    // Who asks is checked before anything else is looked up or shown.
    if helper.and_then(parent_pid).is_none_or(|ssh| !state.lock().unwrap().allowed.contains_key(&ssh)) {
        return stream.write_all(b"NO\n");
    }
    let prompt = prompt.trim_end_matches(['\r', '\n']).to_owned();
    let ssh = helper.and_then(parent_pid);
    let (host, interactive, prompter) = {
        let mut state = state.lock().unwrap();
        let Some(ssh) = ssh.filter(|ssh| state.allowed.contains_key(ssh)) else {
            drop(state);
            return stream.write_all(b"NO\n");
        };
        // One automatic try per connection, and only for a password prompt, or with a key file for
        // ssh's own passphrase prompt (not a server's question that would merely mention a passphrase).
        let lower = prompt.to_lowercase();
        let first_password = lower.contains("password") && !lower.contains("passphrase") && state.answered.insert(ssh);
        let first_passphrase = crate::ssh::is_key_passphrase_prompt(&prompt) && state.key.contains(&ssh) && state.unlocked.insert(ssh);
        let id = state.allowed[&ssh];
        let host = ((first_password || first_passphrase) && !state.unsaved.contains(&ssh)).then_some(id);
        let name = state.names.get(&id).cloned().unwrap_or_default();
        // A terminal's ssh asks there, except to trust a host's key: asked in the window.
        let interactive = (state.interactive.contains(&ssh) || is_host_key_prompt(&prompt)) && !state.refused.contains(&ssh);
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
            // Not trusted: ssh stops (instead of asking again on the terminal).
            if is_host_key_prompt(&prompt) {
                return Some("no".to_owned());
            }
        }
        answer
    });
    match answer {
        Some(answer) => write!(stream, "OK\n{answer}\n"),
        None => stream.write_all(b"NO\n"),
    }
}

/// ssh asking whether to trust a host it doesn't know yet.
pub fn is_host_key_prompt(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    lower.contains("continue connecting") && lower.contains("yes/no")
}

/// The fingerprint of the key in such a question ("SHA256:…"), and its type (ED25519...).
pub fn host_key_fingerprint(prompt: &str) -> Option<(String, String)> {
    let at = prompt.find("SHA256:").or_else(|| prompt.find("MD5:"))?;
    let fingerprint: String = prompt[at..].chars().take_while(|c| !c.is_whitespace()).collect::<String>().trim_end_matches('.').to_owned();
    let kind = prompt[..at].split_whitespace().rev().find(|w| w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-') && w.len() > 2).unwrap_or_default().to_owned();
    Some((fingerprint, kind))
}

/// The helper's side: asks the window. None when it has no answer (then ask on the terminal).
pub fn ask_window(socket: &Path, prompt: &str) -> Option<String> {
    #[cfg(unix)]
    let mut stream = UnixStream::connect(socket).ok()?;
    #[cfg(windows)]
    let mut stream = {
        // Every instance busy (another helper asking right now): wait a moment for a free one.
        const ERROR_PIPE_BUSY: i32 = 231;
        let mut tries = 0;
        loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(socket) {
                Ok(pipe) => break pipe,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 40 => {
                    tries += 1;
                    thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => return None,
            }
        }
    };
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

#[cfg(windows)]
fn parent_pid(pid: u32) -> Option<u32> {
    crate::winproc::parent_pid(pid)
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

#[cfg(all(unix, not(target_os = "macos")))]
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

#[cfg(all(unix, not(target_os = "macos")))]
fn parent_pid(pid: u32) -> Option<u32> {
    // /proc/<pid>/stat: "pid (comm) state ppid ...", comm may hold spaces and parentheses.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(all(test, unix))]
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
        answer(&ours, peer_pid(&ours), &state).unwrap();
        drop(ours);
        let mut reply = String::new();
        helper.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "NO\n");
        assert_eq!(state.lock().unwrap().answered.len(), 1, "counted as tried");

        let (ours, mut helper) = UnixStream::pair().unwrap();
        state.lock().unwrap().allowed.clear();
        writeln!(helper, "password: ").unwrap();
        answer(&ours, peer_pid(&ours), &state).unwrap();
        drop(ours);
        let mut reply = String::new();
        helper.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "NO\n", "a process no pane started gets nothing");
    }

    #[test]
    fn unlocks_the_key_once_only_with_a_key_file() {
        let ssh = unsafe { libc::getppid() } as u32;
        let ask = |state: &Arc<Mutex<State>>, prompt: &str| {
            let (ours, mut helper) = UnixStream::pair().unwrap();
            writeln!(helper, "{prompt}").unwrap();
            answer(&ours, peer_pid(&ours), state).unwrap();
        };
        let prompt = "Enter passphrase for key '/home/demo/.ssh/id_ed25519': ";
        let state = Arc::new(Mutex::new(State::default()));
        state.lock().unwrap().allowed.insert(ssh, Uuid::new_v4());
        ask(&state, prompt);
        assert!(state.lock().unwrap().unlocked.is_empty(), "not tried without a key file");

        state.lock().unwrap().key.insert(ssh);
        ask(&state, "(demo@host) Your passphrase: ");
        assert!(state.lock().unwrap().unlocked.is_empty(), "a server's question isn't the key's");
        ask(&state, prompt);
        assert!(state.lock().unwrap().unlocked.contains(&ssh), "counted as tried");
        assert!(state.lock().unwrap().answered.is_empty(), "the password's try is left");
    }
}

#[cfg(test)]
mod host_key_tests {
    use super::*;

    #[test]
    fn reads_a_host_key_question() {
        let prompt = "The authenticity of host '185.245.143.111 (185.245.143.111)' can't be established. ED25519 key fingerprint is: SHA256:4kHgJSXhIIHlitBVIfNqg3GhDV+sfnQoc5nXTTdSaHA This key is not known by any other names. Are you sure you want to continue connecting (yes/no/[fingerprint])? ";
        assert!(is_host_key_prompt(prompt));
        assert_eq!(host_key_fingerprint(prompt), Some(("SHA256:4kHgJSXhIIHlitBVIfNqg3GhDV+sfnQoc5nXTTdSaHA".into(), "ED25519".into())));
        assert!(!is_host_key_prompt("user@host's password: "));
    }
}
