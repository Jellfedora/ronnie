//! Dictation: what is said into the microphone, as text (to send to Claude Code). Listening stops by
//! itself once the speaker is silent.
//!
//! macOS: the Speech framework (in the installed app only: the system kills an app that listens without
//! saying why in its Info.plist). Windows: Windows.Media.SpeechRecognition. Linux: none.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "none.rs"]
mod imp;

/// Silent this long once something was said: what was heard is sent.
const SILENCE: Duration = Duration::from_millis(1800);
/// Nothing said at all for this long: given up.
const NOTHING: Duration = Duration::from_secs(8);
/// After the end of the audio, the final transcription is awaited this long at most.
const FINAL: Duration = Duration::from_millis(1500);

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Each system has some of them.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Error {
    /// Speech recognition (or the microphone) not allowed in the system's settings.
    Denied,
    /// No recognition for this language, or not right now.
    Unavailable,
    /// Windows: dictation needs "Online speech recognition" on.
    Online,
    Other(String),
}

pub enum Outcome {
    Text(String),
    /// Nothing was said.
    Nothing,
    Failed(Error),
}

/// What the recognition threads tell.
#[derive(Default)]
struct Shared {
    /// The transcription so far.
    text: String,
    /// When it last changed (None: nothing heard yet).
    changed: Option<Instant>,
    /// Sound level of the microphone, 0 to 1.
    level: f32,
    final_text: Option<String>,
    error: Option<Error>,
    /// macOS: the answer to the permission request.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    allowed: Option<bool>,
}

/// Whether dictation can work here.
pub fn supported() -> bool {
    imp::supported()
}

pub struct Dictation {
    imp: imp::Dictation,
    shared: Arc<Mutex<Shared>>,
    started: Instant,
    /// The audio ended, waiting for the final transcription since then.
    finishing: Option<Instant>,
}

impl Dictation {
    /// Starts listening, in `language` ("fr-FR"); `wake` repaints the window when something is heard.
    pub fn start(language: &str, wake: Wake) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let imp = imp::Dictation::start(language, shared.clone(), wake);
        Self { imp, shared, started: Instant::now(), finishing: None }
    }

    /// Called every frame while listening: the outcome, once there is one.
    pub fn poll(&mut self) -> Option<Outcome> {
        self.imp.tick();
        let mut s = self.shared.lock().unwrap();
        if let Some(e) = s.error.take() {
            return Some(Outcome::Failed(e));
        }
        if let Some(text) = s.final_text.take() {
            return Some(outcome(text));
        }
        match self.finishing {
            None => {
                let silent = s.changed.is_some_and(|at| at.elapsed() >= SILENCE);
                let nothing = s.changed.is_none() && self.started.elapsed() >= NOTHING;
                if nothing {
                    return Some(Outcome::Nothing);
                }
                if silent {
                    drop(s);
                    self.imp.finish();
                    self.finishing = Some(Instant::now());
                }
                None
            }
            Some(at) if at.elapsed() >= FINAL => Some(outcome(std::mem::take(&mut s.text))),
            Some(_) => None,
        }
    }

    /// What was heard so far.
    pub fn text(&self) -> String {
        self.shared.lock().unwrap().text.clone()
    }

    pub fn level(&self) -> f32 {
        self.shared.lock().unwrap().level
    }
}

impl Drop for Dictation {
    fn drop(&mut self) {
        self.imp.cancel();
    }
}

fn outcome(text: String) -> Outcome {
    let text = text.trim().to_owned();
    if text.is_empty() { Outcome::Nothing } else { Outcome::Text(text) }
}

/// The sound level of samples, 0 to 1 (their RMS, loud speech near 1).
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    (rms * 8.0).min(1.0)
}
