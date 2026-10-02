//! Dictation on Windows: Windows.Media.SpeechRecognition, on a thread of its own. Windows hears the
//! end of the sentence by itself; what it guesses meanwhile shows as it comes.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::core::HSTRING;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Globalization::Language;
use windows::Media::SpeechRecognition::{SpeechRecognitionResultStatus, SpeechRecognizer};

use super::{Error, Shared, Wake};

/// Windows' "Online speech recognition" privacy setting is off (dictation needs it).
const PRIVACY_POLICY_NOT_ACCEPTED: i32 = 0x80045509_u32 as i32;

pub fn supported() -> bool {
    true
}

pub struct Dictation {
    recognizer: Arc<Mutex<Option<SpeechRecognizer>>>,
}

impl Dictation {
    pub fn start(language: &str, shared: Arc<Mutex<Shared>>, wake: Wake) -> Self {
        let recognizer = Arc::new(Mutex::new(None));
        let (language, slot) = (language.to_owned(), recognizer.clone());
        let _ = std::thread::Builder::new().name("dictation".into()).spawn(move || {
            let result = listen(&language, &shared, &wake, &slot);
            let mut s = shared.lock().unwrap();
            match result {
                Ok(text) => s.final_text = Some(text),
                Err(e) if s.final_text.is_none() => s.error = Some(e),
                Err(_) => {}
            }
            drop(s);
            wake();
        });
        Self { recognizer }
    }

    pub fn tick(&mut self) {}

    /// Stops listening: what was recognized comes as the result.
    pub fn finish(&mut self) {
        if let Some(r) = self.recognizer.lock().unwrap().as_ref() {
            let _ = r.StopRecognitionAsync();
        }
    }

    pub fn cancel(&mut self) {
        if let Some(r) = self.recognizer.lock().unwrap().take() {
            let _ = r.Close();
        }
    }
}

fn listen(language: &str, shared: &Arc<Mutex<Shared>>, wake: &Wake, slot: &Mutex<Option<SpeechRecognizer>>) -> Result<String, Error> {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let other = |e: windows::core::Error| if e.code().0 == PRIVACY_POLICY_NOT_ACCEPTED { Error::Online } else { Error::Other(e.message()) };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let lang = Language::CreateLanguage(&HSTRING::from(language)).map_err(other)?;
    let recognizer = SpeechRecognizer::Create(&lang).or_else(|_| SpeechRecognizer::new()).map_err(|_| Error::Unavailable)?;
    // 100 ns units: a pause like on macOS ends the sentence; nothing said, given up.
    let timeouts = recognizer.Timeouts().map_err(other)?;
    let _ = timeouts.SetEndSilenceTimeout(TimeSpan { Duration: 18_000_000 });
    let _ = timeouts.SetInitialSilenceTimeout(TimeSpan { Duration: 80_000_000 });
    let compiled = recognizer.CompileConstraintsAsync().map_err(other)?.join().map_err(other)?;
    if compiled.Status().map_err(other)? != SpeechRecognitionResultStatus::Success {
        return Err(Error::Unavailable);
    }
    let (partial, wake_partial) = (shared.clone(), wake.clone());
    let _ = recognizer.HypothesisGenerated(&TypedEventHandler::new(move |_, args: windows::core::Ref<windows::Media::SpeechRecognition::SpeechRecognitionHypothesisGeneratedEventArgs>| {
        if let Some(args) = args.as_ref() {
            let text = args.Hypothesis()?.Text()?.to_string();
            let mut s = partial.lock().unwrap();
            if text != s.text {
                s.text = text;
                s.changed = Some(Instant::now());
            }
            drop(s);
            wake_partial();
        }
        Ok(())
    }));
    *slot.lock().unwrap() = Some(recognizer.clone());
    let result = recognizer.RecognizeAsync().map_err(other)?.join().map_err(other)?;
    slot.lock().unwrap().take();
    match result.Status().map_err(other)? {
        SpeechRecognitionResultStatus::Success => Ok(result.Text().map_err(other)?.to_string()),
        SpeechRecognitionResultStatus::UserCanceled => Ok(shared.lock().unwrap().text.clone()),
        SpeechRecognitionResultStatus::MicrophoneUnavailable => Err(Error::Denied),
        _ => Ok(String::new()),
    }
}
