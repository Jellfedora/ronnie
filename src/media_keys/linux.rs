//! Linux: not yet.

use super::{NowPlaying, Sender};

pub struct Imp;

impl Imp {
    pub fn start(_sender: Sender, _window: Option<raw_window_handle::RawWindowHandle>) -> Result<Self, String> {
        Err("no media keys on this system yet".into())
    }

    pub fn show(&mut self, _song: &NowPlaying) {}
}
