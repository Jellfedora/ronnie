//! Dictation: what is said into the microphone, as text (to send to Claude Code). Each sentence goes
//! once the speaker is silent; listening goes on until they say "stop micro". A few words said at the end
//! of a sentence are orders (see `order`).
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

    /// Stops listening now (a key released): what was heard comes as the outcome.
    pub fn finish(&mut self) {
        if self.finishing.is_none() {
            self.imp.finish();
            self.finishing = Some(Instant::now());
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

/// What a sentence ends with, said aloud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    /// Nothing: the sentence is sent.
    Send,
    /// "stop micro": sent, then listening stops.
    Stop,
    /// "nouvelle ligne": a line break, nothing sent yet.
    Newline,
    /// "annule": the sentence (and the lines not sent yet) is dropped.
    Cancel,
    /// "échap": the Escape key (interrupts Claude), the sentence dropped.
    Escape,
}

/// The sentence heard, without the order ending it, and "nouvelle ligne" in it as line breaks.
pub fn order(text: &str) -> (String, Order) {
    let bare = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    // French punctuation stands apart ("micro !"): not a word.
    let mut words: Vec<&str> = text.split_whitespace().filter(|w| !bare(w).is_empty()).collect();
    let pair = |words: &[&str], i: usize, first: &[&str], second: &[&str]| first.contains(&bare(words[i]).as_str()) && second.contains(&bare(words[i + 1]).as_str());
    let newline = |words: &[&str], i: usize| pair(words, i, &["nouvelle", "new"], &["ligne", "line"]);
    let n = words.len();
    let order = match n {
        _ if n >= 2 && pair(&words, n - 2, &["stop"], &["micro", "micros", "mic"]) => Order::Stop,
        _ if n >= 2 && newline(&words, n - 2) => Order::Newline,
        _ if n >= 1 && ["annule", "annuler", "cancel"].contains(&bare(words[n - 1]).as_str()) => Order::Cancel,
        _ if n >= 1 && ["échap", "echap", "échappe", "escape"].contains(&bare(words[n - 1]).as_str()) => Order::Escape,
        _ => Order::Send,
    };
    match order {
        Order::Stop => words.truncate(n - 2),
        Order::Newline => words.truncate(n - 2),
        Order::Cancel | Order::Escape => return (String::new(), order),
        Order::Send => {}
    }
    let mut out = String::new();
    let mut i = 0;
    while i < words.len() {
        if i + 1 < words.len() && newline(&words, i) {
            out = out.trim_end_matches(|c: char| c == ',' || c == ' ').to_owned();
            out.push('\n');
            i += 2;
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(words[i]);
        i += 1;
    }
    (out.trim_end_matches(|c: char| c == ',' || c.is_whitespace()).to_owned(), order)
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

#[cfg(test)]
mod tests {
    use super::{order, Order};

    #[test]
    fn spoken_orders() {
        let said = |text: &str, expected: &str, o: Order| assert_eq!(order(text), (expected.to_owned(), o), "{text}");
        said("Lance les tests.", "Lance les tests.", Order::Send);
        said("Stop micro.", "", Order::Stop);
        said("Lance les tests, stop micro", "Lance les tests", Order::Stop);
        said("Lance les tests. Stop Micro !", "Lance les tests.", Order::Stop);
        said("Ne mets pas de stop micro ici ensuite", "Ne mets pas de stop micro ici ensuite", Order::Send);
        said("Fais ça, nouvelle ligne", "Fais ça", Order::Newline);
        said("Fais ça, nouvelle ligne puis ça", "Fais ça\npuis ça", Order::Send);
        said("Non en fait annule", "", Order::Cancel);
        said("Échap", "", Order::Escape);
        said("Annule le dernier commit", "Annule le dernier commit", Order::Send);
    }
}
