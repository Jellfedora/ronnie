//! No dictation on this system.

use std::sync::{Arc, Mutex};

use super::{Shared, Wake};

pub fn supported() -> bool {
    false
}

pub struct Dictation;

impl Dictation {
    pub fn start(_language: &str, shared: Arc<Mutex<Shared>>, _wake: Wake) -> Self {
        shared.lock().unwrap().error = Some(super::Error::Unavailable);
        Self
    }

    pub fn tick(&mut self) {}

    pub fn finish(&mut self) {}

    pub fn cancel(&mut self) {}
}
