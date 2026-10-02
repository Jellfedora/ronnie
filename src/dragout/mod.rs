//! Files dragged out of the window, onto the Finder, the Explorer or a Linux file manager.
//!
//! The file manager drags its rows within the window itself (egui); once the pointer leaves the
//! window with the button still down, the drag is handed over to the system here. Files of this
//! computer are dragged as they are. Remote files: on macOS as file promises (downloaded where they
//! are dropped, once dropped); elsewhere they are downloaded to a temporary folder first, then dragged
//! from there.
//!
//! Nothing here knows about egui: `wake` asks for a repaint, and the window is the raw handle given
//! to `init`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

/// A remote file or folder dragged out (macOS: promised, downloaded once dropped).
#[derive(Clone, Debug)]
pub struct RemoteItem {
    pub name: String,
    pub is_dir: bool,
}

/// What a drag of remote files asks of the file manager.
pub enum Ask {
    /// Download the item `index` of the drag into `dir`; `done` is told when it's over.
    Download { index: usize, dir: PathBuf, done: Sender<Result<(), String>> },
}

/// Asks for a repaint (from any thread).
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Set when a system drag took the mouse button over: the window never hears it released.
static RELEASED: AtomicBool = AtomicBool::new(false);

fn took_button() {
    RELEASED.store(true, Ordering::Relaxed);
}

/// True once after a system drag took the mouse button over: the app should treat it as released.
pub fn take_release() -> bool {
    RELEASED.swap(false, Ordering::Relaxed)
}

/// Remembers the main window (X11) and the display (Wayland). Called once, on the UI thread.
pub fn init(window: raw_window_handle::RawWindowHandle, display: raw_window_handle::RawDisplayHandle) {
    #[cfg(all(unix, not(target_os = "macos")))]
    match display {
        raw_window_handle::RawDisplayHandle::Wayland(d) => wayland::init(d.display.as_ptr()),
        _ => x11::init(window),
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = (window, display);
}

/// Remote files can be promised (macOS): dragged at once and downloaded where they are dropped.
pub const PROMISES: bool = cfg!(target_os = "macos");

/// Whether the left mouse button is down, asked of the system (the window doesn't know once the
/// pointer is out of it on some systems).
pub fn button_down() -> bool {
    #[cfg(target_os = "macos")]
    return macos::button_down();
    #[cfg(windows)]
    return windows::button_down();
    #[cfg(all(unix, not(target_os = "macos")))]
    return if wayland::active() { wayland::button_down() } else { x11::button_down() };
}

/// Drags these files (of this computer) out of the window. False if the system refused.
pub fn start_files(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    #[cfg(target_os = "macos")]
    return macos::start(macos::Payload::Files(paths));
    #[cfg(windows)]
    return windows::start(paths);
    #[cfg(all(unix, not(target_os = "macos")))]
    return if wayland::active() { wayland::start(paths) } else { x11::start(paths) };
}

/// Drags remote items out as promises (see `PROMISES`): the file manager hears from the receiver
/// when to download them, and where. None if the system refused (or can't).
pub fn start_promises(items: Vec<RemoteItem>, wake: Wake) -> Option<Receiver<Ask>> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        macos::start(macos::Payload::Promises { items, asks: tx, wake }).then_some(rx)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (items, wake);
        None
    }
}

/// "file:///a%20b" for "/a b": what text/uri-list holds.
#[cfg(all(unix, not(target_os = "macos")))]
fn file_uri(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// The paths as text/uri-list.
#[cfg(all(unix, not(target_os = "macos")))]
fn uri_list(paths: &[PathBuf]) -> Vec<u8> {
    paths.iter().map(|p| file_uri(p) + "\r\n").collect::<String>().into_bytes()
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    #[test]
    fn uris_are_escaped() {
        assert_eq!(super::file_uri(std::path::Path::new("/tmp/a b/é.txt")), "file:///tmp/a%20b/%C3%A9.txt");
    }
}
