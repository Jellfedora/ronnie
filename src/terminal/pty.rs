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
    /// Spawns `launch` (the user's default shell if none) in `cwd` (home if unset). Returns the backend and a
    /// reader for its output.
    pub fn spawn(cols: u16, rows: u16, cwd: Option<&Path>, launch: Option<&Launch>) -> Result<(Self, Box<dyn Read + Send>)> {
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("ouverture du PTY")?;

        let mut cmd = match launch {
            Some(l) => {
                let mut cmd = CommandBuilder::new(&l.program);
                cmd.args(&l.args);
                for (key, value) in &l.env {
                    cmd.env(key, value);
                }
                cmd
            }
            None => CommandBuilder::new_default_prog(),
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "ronnie");
        let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
        if let Some(dir) = cwd.filter(|d| d.is_dir()).map(Path::to_path_buf).or(home) {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd).context("lancement du shell")?;
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
    fn reads_own_cwd() {
        let pid = std::process::id() as libc::pid_t;
        let expected = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(super::process_cwd(pid).unwrap().canonicalize().unwrap(), expected);
    }
}
