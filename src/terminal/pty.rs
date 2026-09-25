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
    pub fn spawn(cols: u16, rows: u16, cwd: Option<&Path>, launch: Option<&Launch>, history: Option<&Path>) -> Result<(Self, Box<dyn Read + Send>)> {
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
            None => {
                let mut cmd = CommandBuilder::new_default_prog();
                if let Some(history) = history {
                    crate::shell::use_history(&mut cmd, history);
                }
                cmd
            }
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "ronnie");
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
        #[cfg(not(unix))]
        None
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
        let _ = self.child.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn sees_foreground_program() {
        use super::super::Backend;
        let (mut pty, mut reader) = super::LocalPty::spawn(80, 24, None, None, None).unwrap();
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
