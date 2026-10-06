//! Images and files pasted or dropped into an SSH terminal. The server can't read this computer's
//! clipboard or files: they are sent to it over SFTP (a session of their own), then their paths typed
//! at the prompt, where Claude Code attaches the images as it does for a local terminal.

use std::path::PathBuf;

use super::*;
use crate::sftp;

/// Pasted images go there on the server (readable by the user's programs, cleaned by the system).
const REMOTE_IMAGES: &str = "/tmp";

/// A transfer for one pane.
pub(super) struct PaneUpload {
    pane: PaneId,
    conn: sftp::Connection,
    local: Vec<PathBuf>,
    /// Where on the server: None for the session's home.
    dir: Option<String>,
    sent_to: Option<String>,
    /// Local copies made for it (a pasted image), removed once sent.
    temp: Vec<PathBuf>,
}

/// What the clipboard holds that a server can't read: an image (saved to a file here), or files.
pub(super) fn clipboard_files() -> Option<(Vec<PathBuf>, bool)> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if let Ok(files) = clipboard.get().file_list() {
        let files: Vec<PathBuf> = files.into_iter().filter(|p| p.is_file()).collect();
        if !files.is_empty() {
            return Some((files, false));
        }
    }
    let image = clipboard.get_image().ok()?;
    let rgba = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = std::env::temp_dir().join(format!("ronnie-image-{stamp}.png"));
    rgba.save_with_format(&path, image::ImageFormat::Png).ok()?;
    Some((vec![path], true))
}

impl App {
    /// Sends `files` to the server of tab `index`, then types their paths in `pane`. `image`: pasted
    /// images (temporary files here), sent to /tmp; otherwise into `dir` (the pane's directory), or home.
    pub(super) fn upload_to_pane(&mut self, ctx: &egui::Context, index: usize, pane: PaneId, files: Vec<PathBuf>, image: bool, dir: Option<String>) {
        let Some(host) = self.tabs.get(index).and_then(|t| t.ssh).and_then(|id| self.config.ssh.iter().find(|h| h.id == id)).cloned() else { return };
        let launch = host.sftp_command();
        match sftp::Connection::open(ctx, &launch.program, &launch.args, &launch.env) {
            Ok(conn) => {
                if let Some(pid) = conn.pid {
                    self.askpass.allow_interactive(pid, &host);
                }
                let dir = if image { Some(REMOTE_IMAGES.to_owned()) } else { dir };
                let temp = if image { files.clone() } else { Vec::new() };
                self.uploads.push(PaneUpload { pane, conn, local: files, dir, sent_to: None, temp });
            }
            Err(e) => self.error = Some(format!("SFTP : {e:#}")),
        }
    }

    /// Follows the transfers: once done, the paths are typed in their pane.
    pub(super) fn poll_uploads(&mut self) {
        let mut done = Vec::new();
        for (k, up) in self.uploads.iter_mut().enumerate() {
            for event in up.conn.poll() {
                match event {
                    sftp::Event::Connected { home } => {
                        let dir = up.dir.clone().unwrap_or(home);
                        up.conn.send(sftp::Request::Upload { id: 1, local: up.local.clone(), remote_dir: dir.clone(), overwrite: true });
                        up.sent_to = Some(dir);
                    }
                    sftp::Event::Finished { result: Ok(_), .. } => {
                        let dir = up.sent_to.clone().unwrap_or_default();
                        let paths: Vec<String> = up.local.iter().filter_map(|p| p.file_name()).map(|n| complete::completion(&sftp::join(&dir, &n.to_string_lossy()), "", false)).collect();
                        done.push((k, Ok(paths)));
                    }
                    sftp::Event::Finished { result: Err(e), .. } | sftp::Event::Error(e) | sftp::Event::Closed(e) => done.push((k, Err(e))),
                    _ => {}
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        for (k, result) in done.into_iter().rev().filter(|(k, _)| seen.insert(*k)) {
            let up = self.uploads.remove(k);
            for t in &up.temp {
                let _ = std::fs::remove_file(t);
            }
            match result {
                Ok(paths) => {
                    if let Some(term) = self.tabs.iter_mut().find_map(|t| t.panes.get_mut(&up.pane)) {
                        term.paste_text(&format!("{} ", paths.join(" ")));
                    }
                }
                Err(e) => self.error = Some(format!("{} : {e}", self.t().upload_failed)),
            }
        }
    }
}
