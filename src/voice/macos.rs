//! Dictation on macOS: the microphone through AVAudioEngine, its sound handed to the Speech framework,
//! which transcribes as it goes (with punctuation from macOS 13).

use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{sel, AnyThread};
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use objc2_foundation::{NSBundle, NSError, NSLocale, NSString};
use objc2_speech::{SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask, SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus};

use super::{Error, Shared, Wake};

/// The Info.plist says why Ronnie listens: without it, the system kills the app at the first try
/// (cargo run, a binary outside the app bundle).
pub fn supported() -> bool {
    let bundle = NSBundle::mainBundle();
    ["NSSpeechRecognitionUsageDescription", "NSMicrophoneUsageDescription"].iter().all(|key| bundle.objectForInfoDictionaryKey(&NSString::from_str(key)).is_some())
}

pub struct Dictation {
    language: String,
    shared: Arc<Mutex<Shared>>,
    wake: Wake,
    /// Listening (the permission given): what must stay alive meanwhile.
    running: Option<Running>,
    asked: bool,
}

struct Running {
    engine: Retained<AVAudioEngine>,
    request: Retained<SFSpeechAudioBufferRecognitionRequest>,
    task: Retained<SFSpeechRecognitionTask>,
    _recognizer: Retained<SFSpeechRecognizer>,
    stopped: bool,
}

impl Dictation {
    pub fn start(language: &str, shared: Arc<Mutex<Shared>>, wake: Wake) -> Self {
        let mut d = Self { language: language.to_owned(), shared, wake, running: None, asked: false };
        let status = unsafe { SFSpeechRecognizer::authorizationStatus() };
        if status == SFSpeechRecognizerAuthorizationStatus::Authorized {
            d.begin();
        } else if status == SFSpeechRecognizerAuthorizationStatus::NotDetermined {
            // The system asks the user; the answer comes on another thread.
            d.asked = true;
            let (shared, wake) = (d.shared.clone(), d.wake.clone());
            let handler = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
                shared.lock().unwrap().allowed = Some(status == SFSpeechRecognizerAuthorizationStatus::Authorized);
                wake();
            });
            unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
        } else {
            d.shared.lock().unwrap().error = Some(Error::Denied);
        }
        d
    }

    /// Starts once the permission is given.
    pub fn tick(&mut self) {
        if !self.asked {
            return;
        }
        let allowed = self.shared.lock().unwrap().allowed;
        match allowed {
            Some(true) => {
                self.asked = false;
                self.begin();
            }
            Some(false) => {
                self.asked = false;
                self.shared.lock().unwrap().error = Some(Error::Denied);
            }
            None => {}
        }
    }

    fn begin(&mut self) {
        match unsafe { self.listen() } {
            Ok(running) => self.running = Some(running),
            Err(e) => self.shared.lock().unwrap().error = Some(e),
        }
    }

    unsafe fn listen(&self) -> Result<Running, Error> {
        unsafe {
            let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(&self.language));
            let recognizer = SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale).ok_or(Error::Unavailable)?;
            if !recognizer.isAvailable() {
                return Err(Error::Unavailable);
            }
            let request = SFSpeechAudioBufferRecognitionRequest::new();
            request.setShouldReportPartialResults(true);
            // macOS 13 and later.
            if request.respondsToSelector(sel!(setAddsPunctuation:)) {
                request.setAddsPunctuation(true);
            }

            // The microphone's sound, to the request (on the audio thread).
            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            let format = input.outputFormatForBus(0);
            let (sink, shared) = (request.clone(), self.shared.clone());
            let tap = RcBlock::new(move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                let buffer = buffer.as_ref();
                sink.appendAudioPCMBuffer(buffer);
                let channels = buffer.floatChannelData();
                if !channels.is_null() {
                    let samples = std::slice::from_raw_parts((*channels).as_ptr(), buffer.frameLength() as usize);
                    shared.lock().unwrap().level = super::level(samples);
                }
            });
            input.installTapOnBus_bufferSize_format_block(0, 1024, Some(&format), RcBlock::as_ptr(&tap));
            engine.prepare();
            if let Err(e) = engine.startAndReturnError() {
                input.removeTapOnBus(0);
                return Err(Error::Other(e.localizedDescription().to_string()));
            }

            let (shared, wake) = (self.shared.clone(), self.wake.clone());
            let handler = RcBlock::new(move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                let mut s = shared.lock().unwrap();
                if let Some(result) = result.as_ref() {
                    let text = result.bestTranscription().formattedString().to_string();
                    if text != s.text {
                        s.text = text.clone();
                        s.changed = Some(Instant::now());
                    }
                    if result.isFinal() {
                        s.final_text = Some(text);
                    }
                } else if let Some(error) = error.as_ref() {
                    // Nothing said (kAFAssistantErrorDomain 1110), or cancelled: what was heard, if any.
                    if s.final_text.is_none() {
                        if error.code() == 1110 || error.code() == 216 || error.code() == 301 {
                            s.final_text = Some(s.text.clone());
                        } else {
                            s.error = Some(Error::Other(error.localizedDescription().to_string()));
                        }
                    }
                }
                drop(s);
                wake();
            });
            let task = recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler);
            Ok(Running { engine, request, task, _recognizer: recognizer, stopped: false })
        }
    }

    /// The end of the audio: the final transcription comes next.
    pub fn finish(&mut self) {
        if let Some(r) = &mut self.running
            && !r.stopped
        {
            r.stopped = true;
            unsafe {
                r.engine.stop();
                r.engine.inputNode().removeTapOnBus(0);
                r.request.endAudio();
            }
        }
    }

    pub fn cancel(&mut self) {
        self.finish();
        if let Some(r) = self.running.take() {
            unsafe { r.task.cancel() };
        }
    }
}
