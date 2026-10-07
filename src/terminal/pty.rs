use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use super::Backend;
use crate::ssh::Launch;

/// A shell running on this machine behind a pseudo-terminal.
pub struct LocalPty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl LocalPty {
    /// Spawns `launch` (the user's default shell if none) in `cwd` (home if unset). The shell keeps its
    /// command history in `history`. Returns the backend and a reader for its output.
    /// `env`: variables of Ronnie's own (see `claude::pane_env`).
    pub fn spawn(cols: u16, rows: u16, cwd: Option<&Path>, launch: Option<&Launch>, history: Option<&Path>, env: &[(&str, String)]) -> Result<(Self, Box<dyn Read + Send>)> {
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("opening the PTY")?;

        let mut cmd = match launch {
            Some(l) => {
                let mut cmd = CommandBuilder::new(&l.program);
                cmd.args(&l.args);
                for (key, value) in &l.env {
                    cmd.env(key, value);
                }
                cmd
            }
            None => crate::shell::local_command(history),
        };
        // Ronnie's AppImage runtime variables would make programs started here think they are Ronnie.
        if crate::update::appimage().is_some() {
            for var in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
                cmd.env_remove(var);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "ronnie");
        for (key, value) in env {
            cmd.env(key, value);
        }
        let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
        if let Some(dir) = cwd.filter(|d| d.is_dir()).map(Path::to_path_buf).or(home) {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd).context("starting the shell")?;
        // The slave must be closed on our side, otherwise we never get EOF when the shell exits.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        Ok((Self { master: pair.master, writer, child }, reader))
    }
}

impl Backend for LocalPty {
    fn write(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data);
        let _ = self.writer.flush();
    }

    fn resize(&mut self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: cols * cell_w,
            pixel_height: rows * cell_h,
        });
    }

    fn cwd(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            // The foreground program first (it may have been started after a `cd`), then the shell.
            let shell = self.child.process_id().map(|p| p as libc::pid_t);
            [self.master.process_group_leader(), shell].into_iter().flatten().find_map(process_cwd)
        }
        #[cfg(not(unix))]
        None
    }

    fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    fn foreground(&self) -> Option<String> {
        #[cfg(unix)]
        {
            let leader = self.master.process_group_leader()?;
            let shell = self.child.process_id()? as libc::pid_t;
            (leader != shell).then(|| process_name(leader).unwrap_or_else(|| leader.to_string()))
        }
        #[cfg(windows)]
        {
            crate::winproc::foreground(self.child.process_id()?)
        }
    }
}

#[cfg(target_os = "macos")]
fn process_name(pid: libc::pid_t) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is writable for its whole length.
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_name(pid: libc::pid_t) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
fn process_cwd(pid: libc::pid_t) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable proc_vnodepathinfo.
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, (&raw mut info).cast(), size) };
    if n != size {
        return None;
    }
    let path = &info.pvi_cdir.vip_path;
    // SAFETY: the path is a contiguous [[c_char; 32]; 32] buffer.
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(path.as_ptr().cast(), size_of_val(path)) };
    let end = bytes.iter().position(|&b| b == 0)?;
    (end > 0).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end])))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_cwd(pid: libc::pid_t) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

impl Drop for LocalPty {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(shell) = self.child.process_id() {
            end_session(shell as libc::pid_t);
            return;
        }
        // Windows: closing the pseudo console (the master, dropped next) ends every console program
        // attached to it; windows opened from the pane (an editor...) stay, as on the other systems.
        let _ = self.child.kill();
    }
}

/// Ends what runs in a closed pane, like a terminal window closing: every process of the shell's
/// session (the shell leads it), the program in the foreground and those sent to the background alike.
/// Hung up first, killed if still there a moment later. In the background: closing doesn't wait.
///
/// The shell alone isn't enough: killed, it can't pass the hangup on, and the pseudo-terminal isn't
/// hung up by the kernel while Ronnie's reader still holds it.
/// Sessions of panes closed and possibly still ending (see `finish_ending`).
#[cfg(unix)]
static ENDING: std::sync::Mutex<Vec<libc::pid_t>> = std::sync::Mutex::new(Vec::new());

#[cfg(unix)]
fn end_session(shell: libc::pid_t) {
    if let Ok(mut ending) = ENDING.lock() {
        ending.retain(|&s| !session_members(s).is_empty());
        ending.push(shell);
    }
    let signal_all = move |signal| {
        for pid in session_members(shell) {
            // SAFETY: plain signal to a process of this pane's session.
            unsafe { libc::kill(pid, signal) };
        }
    };
    signal_all(libc::SIGHUP);
    let _ = std::thread::Builder::new().name("pane-end".into()).spawn(move || {
        let reap = || {
            // SAFETY: the shell is our child; WNOHANG never blocks.
            unsafe { libc::waitpid(shell, std::ptr::null_mut(), libc::WNOHANG) };
        };
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            reap();
            if session_members(shell).is_empty() {
                return;
            }
        }
        signal_all(libc::SIGKILL);
        std::thread::sleep(std::time::Duration::from_millis(100));
        reap();
    });
}

/// On quit: what closed panes ran gets a last moment to end, then is killed (the background threads
/// that would do it end with Ronnie).
pub fn finish_ending() {
    #[cfg(unix)]
    {
        let shells = ENDING.lock().map(|e| e.clone()).unwrap_or_default();
        let running = || shells.iter().flat_map(|&s| session_members(s)).collect::<Vec<_>>();
        for _ in 0..10 {
            if running().is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        for pid in running() {
            // SAFETY: plain signal to a process of a closed pane's session.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

/// The processes of session `sid` still running (zombies apart), the shell included.
#[cfg(unix)]
fn session_members(sid: libc::pid_t) -> Vec<libc::pid_t> {
    let own = std::process::id() as libc::pid_t;
    // SAFETY: getsid only reads the process table.
    let in_session = |pid: libc::pid_t| pid > 0 && pid != own && unsafe { libc::getsid(pid) } == sid && !is_zombie(pid);
    all_pids().into_iter().filter(|&pid| in_session(pid)).collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn all_pids() -> Vec<libc::pid_t> {
    std::fs::read_dir("/proc").map(|d| d.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect()).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn all_pids() -> Vec<libc::pid_t> {
    // SAFETY: a null buffer asks for the count; the second call fills at most `pids.len()` ids.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Vec::new();
    }
    let mut pids = vec![0 as libc::pid_t; count as usize + 64];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(n.max(0) as usize);
    pids
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_zombie(pid: libc::pid_t) -> bool {
    // "pid (comm) state ...": the state follows the last parenthesis.
    std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().and_then(|s| s.rsplit_once(')').and_then(|(_, rest)| rest.trim_start().chars().next())) == Some('Z')
}

#[cfg(target_os = "macos")]
fn is_zombie(pid: libc::pid_t) -> bool {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable proc_bsdinfo.
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    n == size && info.pbi_status == libc::SZOMB as u32
}

#[cfg(all(test, unix))]
mod tests {
    /// Programs that ignore the hangup too (dev servers often catch it), started in the foreground and
    /// in the background.
    #[test]
    fn closing_ends_what_runs_in_the_pane() {
        let launch = crate::ssh::Launch { program: "/bin/sh".into(), args: vec!["-c".into(), "trap '' HUP; sleep 1000 & sleep 1001".into()], env: Vec::new() };
        let (pty, mut reader) = super::LocalPty::spawn(80, 24, None, Some(&launch), None, &[]).unwrap();
        std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
        let shell = super::Backend::pid(&pty).unwrap() as libc::pid_t;
        // The shell and its two sleeps.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while super::session_members(shell).len() < 3 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let members = super::session_members(shell);
        assert_eq!(members.len(), 3, "{members:?}");
        drop(pty);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !super::session_members(shell).is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(super::session_members(shell).is_empty(), "still running: {:?}", super::session_members(shell));
    }

    #[test]
    fn sees_foreground_program() {
        use super::super::Backend;
        let (mut pty, mut reader) = super::LocalPty::spawn(80, 24, None, None, None, &[]).unwrap();
        // Drain the output so the shell never blocks on a full pipe.
        std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
        // Polls instead of fixed pauses: a loaded machine (CI) may take a while to start things.
        let wait_for = |pty: &super::LocalPty, want: Option<&str>| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while pty.foreground().as_deref() != want && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            pty.foreground()
        };
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(wait_for(&pty, None), None, "idle shell");
        pty.write(b"sleep 30\n");
        assert_eq!(wait_for(&pty, Some("sleep")).as_deref(), Some("sleep"));
    }

    #[test]
    fn names_own_process() {
        let name = super::process_name(std::process::id() as libc::pid_t).unwrap();
        assert!(name.starts_with("ronnie"), "{name}");
    }

    #[test]
    fn reads_own_cwd() {
        let pid = std::process::id() as libc::pid_t;
        let expected = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(super::process_cwd(pid).unwrap().canonicalize().unwrap(), expected);
    }
}
