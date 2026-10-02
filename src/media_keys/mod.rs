//! The keyboard's media keys (play / pause, next…) and the system's "now playing": the radio shows there
//! and is driven from there, also when Ronnie is in the background.
//!
//! macOS: MPRemoteCommandCenter and MPNowPlayingInfoCenter. Windows: the system media transport
//! controls. Linux: MPRIS, on the session bus.
//!
//! Taken only while the radio plays: stopped, the keys go back to the other players.

use std::sync::mpsc;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "linux.rs"]
mod imp;

/// What a key (or the system's player controls) asks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Toggle,
    Play,
    Pause,
    Next,
    Previous,
    Stop,
    /// To this point of the song, in seconds.
    Seek(f64),
}

/// The song shown in the system's player.
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// In seconds.
    pub duration: f64,
    pub elapsed: f64,
    pub paused: bool,
}

/// Gives what is asked to the app, and wakes it up.
#[derive(Clone)]
pub struct Sender {
    tx: mpsc::Sender<Command>,
    ctx: egui::Context,
}

impl Sender {
    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
        self.ctx.request_repaint();
    }
}

pub struct MediaKeys {
    rx: mpsc::Receiver<Command>,
    sender: Sender,
    imp: Option<imp::Imp>,
    shown: Option<NowPlaying>,
    window: Option<raw_window_handle::RawWindowHandle>,
}

impl MediaKeys {
    /// `window`: the main window (Windows attaches the controls to it).
    pub fn new(ctx: &egui::Context, window: Option<raw_window_handle::RawWindowHandle>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { rx, sender: Sender { tx, ctx: ctx.clone() }, imp: None, shown: None, window }
    }

    /// What was asked since the last time.
    pub fn commands(&self) -> Vec<Command> {
        self.rx.try_iter().collect()
    }

    /// The song playing (None: stopped). Told to the system when it changed.
    /// The point in the song isn't compared: the system moves it on by itself.
    pub fn show(&mut self, now: Option<NowPlaying>) {
        let same = match (&now, &self.shown) {
            (Some(a), Some(b)) => (&a.title, &a.artist, &a.album, a.duration, a.paused) == (&b.title, &b.artist, &b.album, b.duration, b.paused),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        match &now {
            Some(song) => {
                if self.imp.is_none() {
                    self.imp = imp::Imp::start(self.sender.clone(), self.window).map_err(|e| crate::log::error(&format!("media keys: {e}"))).ok();
                }
                if let Some(imp) = &mut self.imp {
                    imp.show(song);
                }
            }
            // Let go of: the keys go back to the other players.
            None => self.imp = None,
        }
        self.shown = now;
    }

    /// The point in the song moved (a seek): told again.
    pub fn moved(&mut self) {
        self.shown = None;
    }
}
