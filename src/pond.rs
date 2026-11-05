//! The pond on floor (its `/v1/pond`): who is fishing at the same time, at which place on the bank,
//! where their line is, what they just caught. Ronnie says where its line is every second while the
//! page is shown (at once when it changes), and gets the others back; a catch is sent as it comes.
//! Only with a pseudo; without floor, the player fishes alone.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::config::FloorAccount;

/// Seconds between two signs of life.
const EVERY: f64 = 1.0;

/// Where a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Line {
    /// Out of the water.
    #[default]
    Idle,
    /// The float in the water.
    Wait,
    /// Something bit.
    Bite,
    /// Fighting it.
    Reel,
}

/// A catch, as the others see it.
#[derive(Clone, Debug, Deserialize)]
pub struct Catch {
    pub fish: String,
    pub size: u32,
    #[serde(default)]
    pub shiny: bool,
    /// Milliseconds ago.
    pub ago: f64,
}

/// Someone at the pond.
#[derive(Clone, Debug, Deserialize)]
pub struct Fisher {
    pub name: String,
    pub spot: usize,
    pub state: Line,
    /// How far the float went (0 to 1).
    pub cast: f32,
    pub me: bool,
    pub catch: Option<Catch>,
}

/// A catch in the news.
#[derive(Clone, Debug, Deserialize)]
pub struct News {
    pub name: String,
    pub fish: String,
    pub size: u32,
    #[serde(default)]
    pub shiny: bool,
    pub ago: f64,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct View {
    /// The player's place.
    pub you: Option<usize>,
    pub fishers: Vec<Fisher>,
    pub feed: Vec<News>,
}

#[derive(Serialize, Clone)]
struct Sent {
    fish: String,
    size: u32,
    shiny: bool,
}

#[derive(Default)]
struct Shared {
    busy: bool,
    done: Option<Option<View>>,
}

#[derive(Default)]
pub struct Pond {
    shared: Arc<Mutex<Shared>>,
    /// What floor said last (None: never answered, or not reachable).
    pub view: Option<View>,
    /// When it answered (app time).
    pub at: f64,
    last: Option<f64>,
    sent: Option<(Line, u8)>,
    catches: Vec<Sent>,
    /// The player is at the pond on floor (to leave it).
    present: bool,
}

impl Pond {
    /// Takes what floor answered.
    pub fn poll(&mut self, now: f64) {
        let Some(done) = self.shared.lock().ok().and_then(|mut s| s.done.take()) else { return };
        if done.is_some() {
            self.at = now;
        }
        self.view = done;
    }

    /// A catch to tell the others.
    pub fn caught(&mut self, fish: &str, size: u32, shiny: bool) {
        self.catches.push(Sent { fish: fish.to_owned(), size, shiny });
    }

    /// The page is shown, the line at `line`: floor told every second, at once when it changed.
    pub fn tick(&mut self, ctx: &egui::Context, now: f64, account: Option<&FloorAccount>, line: Line, cast: f32) {
        let Some(url) = crate::floor::url() else { return };
        let Some(account) = account.filter(|a| a.url == url && a.name.is_some()).cloned() else {
            self.view = None;
            return;
        };
        let key = (line, (cast * 20.0) as u8);
        let changed = self.sent != Some(key) || !self.catches.is_empty();
        if !changed && self.last.is_some_and(|last| now - last < EVERY) {
            return;
        }
        {
            let Ok(mut shared) = self.shared.lock() else { return };
            if shared.busy {
                return;
            }
            shared.busy = true;
        }
        self.last = Some(now);
        self.sent = Some(key);
        self.present = true;
        let catches = std::mem::take(&mut self.catches);
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let view = send(&url, &account, &catches, line, cast).map_err(|e| crate::log::info(&format!("pond: {e}"))).ok();
            if let Ok(mut shared) = shared.lock() {
                shared.done = Some(view);
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }

    /// The page left: the player leaves the pond (the others see them go at once).
    pub fn leave(&mut self, account: Option<&FloorAccount>) {
        if !std::mem::take(&mut self.present) {
            return;
        }
        self.view = None;
        self.sent = None;
        self.last = None;
        let (Some(url), Some(account)) = (crate::floor::url(), account.cloned()) else { return };
        std::thread::spawn(move || {
            let _ = crate::floor::agent().post(format!("{url}/v1/pond/leave")).header("User-Agent", crate::floor::USER_AGENT).header("Authorization", format!("Bearer {}", account.token)).send_empty();
        });
    }
}

fn send(url: &str, account: &FloorAccount, catches: &[Sent], line: Line, cast: f32) -> Result<View, ureq::Error> {
    let agent = crate::floor::agent();
    let auth = format!("Bearer {}", account.token);
    for c in catches {
        agent.post(format!("{url}/v1/pond/catch")).header("User-Agent", crate::floor::USER_AGENT).header("Authorization", &auth).send_json(c)?;
    }
    let body = serde_json::json!({ "state": line, "cast": cast.clamp(0.0, 1.0) });
    agent.post(format!("{url}/v1/pond/state")).header("User-Agent", crate::floor::USER_AGENT).header("Authorization", &auth).send_json(&body)?.body_mut().read_json()
}
