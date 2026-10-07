//! Ronnie.io online: the world runs on floor (its src/blob), Ronnie shows it. A thread keeps a
//! WebSocket to floor while the game's page shows: it sends what the player does (the pointer, a split,
//! some mass ejected), and hands the app what floor sends, about 20 times a second (what the player
//! sees), each second (the leaderboard), and as the game goes (the deaths, the events, the bonuses).
//! Only with a pseudo. The connection lost in the middle of a life comes back by itself: floor keeps
//! the player's cells a few seconds.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tungstenite::stream::MaybeTlsStream;

use crate::config::FloorAccount;

/// Effects, in bits (as floor's).
pub const FX_SPEED: u8 = 1;
pub const FX_MAGNET: u8 = 2;
pub const FX_SHIELD: u8 = 4;
/// The player put the game on pause: protected, still.
pub const FX_PAUSED: u8 = 8;
/// The pellets' color during the golden rain.
pub const GOLD: u8 = 99;
/// floor's delay is measured this often.
const PING_EVERY: Duration = Duration::from_secs(2);
/// Kept in the kill feed.
const FEED: usize = 6;

/// A cell, at the last frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub id: u32,
    /// Its player.
    pub owner: u32,
    pub x: f32,
    pub y: f32,
    pub mass: f32,
    /// Its player's effects (FX_*).
    pub fx: u8,
}

/// What the player sees, at the last frame (the pellets are kept apart: floor sends only the changes).
#[derive(Clone, Debug)]
pub struct Frame {
    /// The player in the world (None: watching).
    pub me: Option<u32>,
    /// The center of the view, and the height of the world it shows.
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub cells: Vec<Cell>,
    /// The mass ejected: x, y, color.
    pub ejected: Vec<(f32, f32, u8)>,
    pub viruses: Vec<(f32, f32)>,
    /// x, y, kind (0 speed, 1 magnet, 2 shield).
    pub bonuses: Vec<(f32, f32, u8)>,
    /// The player's effects: seconds left of speed, magnet, shield.
    pub fx: Option<[f32; 3]>,
    /// The event going on ("rain", "boss"), and its seconds left.
    pub event: Option<(String, u32)>,
    /// When it came.
    pub at: Instant,
}

/// The leaderboard: name, mass, the player's line, a bot.
#[derive(Clone, Debug, Default)]
pub struct Board {
    pub leaders: Vec<(String, u32, bool, bool)>,
    /// The player's place and mass, in the world.
    pub me: Option<(u32, u32)>,
    /// Players connected (in the world or watching).
    pub online: u32,
}

/// The records: each player's largest mass on floor, the best first (name, mass, the player's); the
/// player's place and mass.
#[derive(Clone, Debug, Default)]
pub struct Records {
    pub leaders: Vec<(String, u32, bool)>,
    pub me: Option<(u32, u32)>,
}

/// A skin: its id, its goal ("games" played, "best" mass in one game, "kills" in all, "badge"), the
/// number to reach, unlocked, where the player is.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Skin(pub String, pub String, pub u32, pub bool, pub u32);

/// The end of a life.
#[derive(Clone, Debug)]
pub struct Death {
    pub by: String,
    /// The largest mass reached.
    pub best: u32,
    pub kills: u32,
    /// Seconds alive.
    pub time: u32,
    /// The player's best on floor's board.
    pub record: bool,
    /// Not eaten: the pause lasted too long.
    pub expired: bool,
}

/// A line of the kill feed: who ate whom.
#[derive(Clone, Debug)]
pub struct Kill {
    pub by: (String, u32),
    pub victim: (String, u32),
    /// "boss" or "ronnie".
    pub special: Option<String>,
    pub at: Instant,
}

/// An event that started or ended ("rain", "boss", "ronnie").
#[derive(Clone, Debug)]
pub struct Notice {
    pub kind: String,
    pub on: bool,
    pub at: Instant,
}

/// What a frame of the app gets once.
#[derive(Default)]
pub struct News {
    pub death: Option<Death>,
    /// The player ate another (the kills just announced).
    pub ate: u32,
    /// A bonus taken ("speed", "magnet", "shield").
    pub bonus: Option<String>,
    /// The Ronnie badge won.
    pub badge: bool,
    /// Ronnie appeared.
    pub ronnie: bool,
    /// Skins just unlocked.
    pub unlocked: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Off,
    Connecting,
    Ready,
    /// The connection fell: again soon (the number of tries so far).
    Reconnecting(u32),
    /// Refused (floor's message), or unreachable (None).
    Failed(Option<String>),
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum Wire {
    #[serde(rename = "welcome")]
    Welcome { size: f32, skins: Vec<Skin> },
    #[serde(rename = "joined")]
    Joined {},
    #[serde(rename = "s")]
    Frame {
        me: Option<u32>,
        x: f32,
        y: f32,
        h: f32,
        c: Vec<f32>,
        g: Vec<f32>,
        gx: Vec<u32>,
        e: Vec<f32>,
        v: Vec<f32>,
        b: Vec<f32>,
        n: HashMap<String, (String, u8, String)>,
        fx: Option<[f32; 3]>,
        ev: Option<(String, u32)>,
    },
    #[serde(rename = "board")]
    Board { l: Vec<(String, u32, bool, bool)>, me: Option<(u32, u32)>, online: u32 },
    #[serde(rename = "records")]
    Records { l: Vec<(String, u32, bool)>, me: Option<(u32, u32)>, skins: Vec<Skin> },
    #[serde(rename = "dead")]
    Dead {
        by: String,
        best: u32,
        kills: u32,
        time: u32,
        record: bool,
        #[serde(default)]
        expired: bool,
    },
    #[serde(rename = "kill")]
    Kill { by: (String, u32), victim: (String, u32), special: Option<String> },
    #[serde(rename = "event")]
    Event { kind: String, on: bool },
    #[serde(rename = "bonus")]
    Bonus { kind: String },
    #[serde(rename = "badge")]
    Badge {},
    #[serde(rename = "pong")]
    Pong { n: u64 },
    #[serde(rename = "error")]
    Error { error: String },
}

/// From the thread.
enum Event {
    Message(Box<Wire>, Instant),
    Closed(Option<String>),
}

/// The thread's ends.
struct Link {
    tx: mpsc::Sender<serde_json::Value>,
    rx: mpsc::Receiver<Event>,
}

/// Kept by the app: the connection, and what floor last sent.
#[derive(Default)]
pub struct Online {
    link: Option<Link>,
    /// What the connection was opened with: kept to open it again.
    with: Option<(egui::Context, FloorAccount, f32)>,
    /// When to try again, after a fall.
    retry_at: Option<Instant>,
    pub status: Status,
    /// The world's side.
    pub size: f32,
    pub frame: Option<Frame>,
    /// The pellets seen: id → x, y, color.
    pub pellets: HashMap<u32, (f32, f32, u8)>,
    /// The players seen: name, color, skin.
    pub names: HashMap<u32, (String, u8, String)>,
    pub board: Option<Board>,
    pub records: Option<Records>,
    pub skins: Vec<Skin>,
    /// The life that just ended (until the next one starts).
    pub death: Option<Death>,
    pub feed: Vec<Kill>,
    pub notices: Vec<Notice>,
    /// A join asked, not answered yet.
    pub joining: bool,
    /// floor's delay, there and back (seconds).
    pub rtt: f32,
    pings: HashMap<u64, Instant>,
    next_ping: u64,
    last_ping: Option<Instant>,
}

/// floor's WebSocket address, from its HTTP one.
fn ws_url(base: &str) -> String {
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_owned()
    };
    format!("{base}/v1/blob/ws")
}

/// Flat numbers in groups of `n`.
fn groups(v: &[f32], n: usize) -> impl Iterator<Item = &[f32]> {
    v.chunks_exact(n)
}

impl Online {
    pub fn connected(&self) -> bool {
        self.link.is_some() || self.retry_at.is_some()
    }

    /// To floor, with the account (its pseudo); `aspect`: the view's width over its height.
    pub fn connect(&mut self, ctx: &egui::Context, account: &FloorAccount, aspect: f32) {
        *self = Online { with: Some((ctx.clone(), account.clone(), aspect)), status: Status::Connecting, rtt: 0.1, ..Default::default() };
        self.open();
    }

    fn open(&mut self) {
        let Some(base) = crate::floor::url() else { return };
        let Some((ctx, account, aspect)) = self.with.clone() else { return };
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let (url, token) = (ws_url(&base), account.token.clone());
        std::thread::Builder::new()
            .name("ronnie-io".into())
            .spawn(move || run(&url, &token, aspect, &commands, &events, &ctx))
            .ok();
        self.link = Some(Link { tx, rx });
        self.retry_at = None;
        self.last_ping = None;
        self.pings.clear();
    }

    /// Closed (the page left): floor takes the player out of the world at once.
    pub fn disconnect(&mut self) {
        if self.connected() {
            self.leave();
            *self = Online::default();
        }
    }

    /// The page left during a game: on pause (floor keeps the cells 30 min, protected), and closed.
    /// Coming back connects again and finds them.
    pub fn suspend(&mut self) {
        if self.alive() && !self.paused() {
            self.pause();
        }
        // The pause goes before the connection closes (the thread sends what is queued first).
        *self = Online::default();
    }

    pub fn pause(&self) {
        self.send(json!({ "t": "pause" }));
    }

    pub fn resume(&self) {
        self.send(json!({ "t": "resume" }));
    }

    /// The player's game is on pause.
    pub fn paused(&self) -> bool {
        self.frame.as_ref().is_some_and(|f| f.me.is_some_and(|me| f.cells.iter().any(|c| c.owner == me && c.fx & FX_PAUSED != 0)))
    }

    fn send(&self, msg: serde_json::Value) {
        if let Some(link) = &self.link {
            let _ = link.tx.send(msg);
        }
    }

    /// In the world, with this skin (floor checks it is unlocked).
    pub fn join(&mut self, skin: &str) {
        if !self.joining {
            self.joining = true;
            self.send(json!({ "t": "join", "skin": skin }));
        }
    }

    pub fn aim(&self, x: f32, y: f32, aspect: f32) {
        self.send(json!({ "t": "aim", "x": x.round(), "y": y.round(), "aspect": (aspect * 100.0).round() / 100.0 }));
    }

    pub fn split(&self) {
        self.send(json!({ "t": "split" }));
    }

    pub fn eject(&self) {
        self.send(json!({ "t": "eject" }));
    }

    /// Out of the world, to watch.
    pub fn leave(&self) {
        self.send(json!({ "t": "leave" }));
    }

    /// The player is in the world.
    pub fn alive(&self) -> bool {
        self.frame.as_ref().is_some_and(|f| f.me.is_some())
    }

    /// What the thread got, at each frame: what happened once, for the app.
    pub fn poll(&mut self) -> News {
        let mut news = News::default();
        // Fallen: again when it is time.
        if self.retry_at.is_some_and(|at| Instant::now() >= at) {
            self.open();
        }
        if self.link.is_some() && self.status == Status::Ready && self.last_ping.is_none_or(|at| at.elapsed() >= PING_EVERY) {
            self.next_ping += 1;
            self.pings.insert(self.next_ping, Instant::now());
            self.send(json!({ "t": "ping", "n": self.next_ping }));
            self.last_ping = Some(Instant::now());
        }
        let Some(link) = &self.link else { return news };
        let mut closed = None;
        let mut messages = Vec::new();
        loop {
            match link.rx.try_recv() {
                Ok(Event::Closed(why)) => closed = Some(why),
                Ok(Event::Message(wire, at)) => messages.push((wire, at)),
                Err(mpsc::TryRecvError::Empty) => break,
                // The thread died without a word (a panic): closed all the same, not "connecting" forever.
                Err(mpsc::TryRecvError::Disconnected) => {
                    closed.get_or_insert(None);
                    break;
                }
            }
        }
        for (wire, at) in messages {
            self.take(*wire, at, &mut news, &mut closed);
        }
        if let Some(why) = closed {
            self.link = None;
            match why {
                // Lost after it worked: again in a moment (floor keeps the cells a few seconds).
                None if matches!(self.status, Status::Ready | Status::Reconnecting(_)) => {
                    let tries = match self.status {
                        Status::Reconnecting(n) => n + 1,
                        _ => 1,
                    };
                    self.status = Status::Reconnecting(tries);
                    self.retry_at = Some(Instant::now() + Duration::from_millis((500 * u64::from(tries)).min(4000)));
                    self.joining = false;
                }
                why => {
                    let with = self.with.take();
                    *self = Online { status: Status::Failed(why), with, ..Default::default() };
                }
            }
        }
        news
    }

    fn take(&mut self, wire: Wire, at: Instant, news: &mut News, closed: &mut Option<Option<String>>) {
        match wire {
            Wire::Welcome { size, skins } => {
                self.size = size;
                self.skins = skins;
                self.status = Status::Ready;
                // A new connection: floor sends the pellets and the names again.
                self.pellets.clear();
                self.names.clear();
            }
            Wire::Joined {} => {
                self.joining = false;
                self.death = None;
            }
            Wire::Frame { me, x, y, h, c, g, gx, e, v, b, n, fx, ev } => {
                for (id, named) in n {
                    if let Ok(id) = id.parse() {
                        self.names.insert(id, named);
                    }
                }
                for id in gx {
                    self.pellets.remove(&id);
                }
                for k in groups(&g, 4) {
                    self.pellets.insert(k[0] as u32, (k[1], k[2], k[3] as u8));
                }
                let cells = groups(&c, 6).map(|k| Cell { id: k[0] as u32, owner: k[1] as u32, x: k[2], y: k[3], mass: k[4], fx: k[5] as u8 }).collect();
                let ejected = groups(&e, 3).map(|k| (k[0], k[1], k[2] as u8)).collect();
                let viruses = groups(&v, 2).map(|k| (k[0], k[1])).collect();
                let bonuses = groups(&b, 3).map(|k| (k[0], k[1], k[2] as u8)).collect();
                self.frame = Some(Frame { me, x, y, height: h, cells, ejected, viruses, bonuses, fx, event: ev, at });
            }
            Wire::Board { l, me, online } => self.board = Some(Board { leaders: l, me, online }),
            Wire::Records { l, me, skins } => {
                self.records = Some(Records { leaders: l, me });
                let had: Vec<&str> = self.skins.iter().filter(|s| s.3).map(|s| s.0.as_str()).collect();
                if !had.is_empty() {
                    news.unlocked.extend(skins.iter().filter(|s| s.3 && !had.contains(&s.0.as_str())).map(|s| s.0.clone()));
                }
                self.skins = skins;
            }
            Wire::Dead { by, best, kills, time, record, expired } => {
                let death = Death { by, best, kills, time, record, expired };
                news.death = Some(death.clone());
                self.death = Some(death);
            }
            Wire::Kill { by, victim, special } => {
                if self.frame.as_ref().and_then(|f| f.me).is_some_and(|me| me == by.1) {
                    news.ate += 1;
                }
                self.feed.push(Kill { by, victim, special, at });
                let extra = self.feed.len().saturating_sub(FEED);
                self.feed.drain(..extra);
            }
            Wire::Event { kind, on } => {
                news.ronnie |= on && kind == "ronnie";
                self.notices.retain(|n| n.kind != kind);
                self.notices.push(Notice { kind, on, at });
            }
            Wire::Bonus { kind } => news.bonus = Some(kind),
            Wire::Badge {} => news.badge = true,
            Wire::Pong { n } => {
                if let Some(sent) = self.pings.remove(&n) {
                    // Eased: one slow answer doesn't throw the prediction.
                    self.rtt = self.rtt * 0.7 + sent.elapsed().as_secs_f32() * 0.3;
                }
            }
            Wire::Error { error } => *closed = Some(Some(error)),
        }
    }
}

/// The thread: connects, then sends the commands and reads floor's messages until either side closes.
fn run(url: &str, token: &str, aspect: f32, commands: &mpsc::Receiver<serde_json::Value>, events: &mpsc::Sender<Event>, ctx: &egui::Context) {
    let closed = |why: Option<String>| {
        let _ = events.send(Event::Closed(why));
        ctx.request_repaint();
    };
    let mut socket = match tungstenite::connect(url) {
        Ok((socket, _)) => socket,
        Err(e) => {
            crate::log::info(&format!("Ronnie.io: {url}: {e}"));
            return closed(None);
        }
    };
    // Short reads, to send the commands in between; no waiting to fill packets.
    let wait = Some(Duration::from_millis(5));
    let _ = match socket.get_mut() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(wait).and_then(|()| s.set_nodelay(true)),
        MaybeTlsStream::Rustls(s) => s.get_mut().set_read_timeout(wait).and_then(|()| s.get_mut().set_nodelay(true)),
        _ => Ok(()),
    };
    let hello = json!({ "t": "hello", "token": token, "aspect": aspect });
    if socket.send(tungstenite::Message::text(hello.to_string())).is_err() {
        return closed(None);
    }
    loop {
        loop {
            match commands.try_recv() {
                Ok(msg) => {
                    if socket.send(tungstenite::Message::text(msg.to_string())).is_err() {
                        return closed(None);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                // The app let go: the page left (or it starts another connection).
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return;
                }
            }
        }
        match socket.read() {
            Ok(tungstenite::Message::Text(text)) => {
                if let Ok(wire) = serde_json::from_str::<Wire>(&text) {
                    if events.send(Event::Message(Box::new(wire), Instant::now())).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            }
            Ok(tungstenite::Message::Close(_)) => return closed(None),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return closed(None),
            Err(e) => {
                crate::log::info(&format!("Ronnie.io: {e}"));
                return closed(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(ws_url("https://fedoheim.hopto.org/floor"), "wss://fedoheim.hopto.org/floor/v1/blob/ws");
        assert_eq!(ws_url("http://localhost:5002"), "ws://localhost:5002/v1/blob/ws");
    }

    #[test]
    fn frames() {
        let mut o = Online::default();
        let at = Instant::now();
        let mut news = News::default();
        let mut closed = None;
        let text = r#"{"t":"s","me":3,"x":100,"y":200,"h":1000,"c":[7,3,110,190,12,4],"g":[1,10,20,4,2,30,40,5],"gx":[],"e":[],"v":[500,500],"b":[50,60,2],"n":{"3":["Dio",5,"flames"]},"fx":[0,0,3.5],"ev":["rain",12]}"#;
        o.take(serde_json::from_str(text).unwrap(), at, &mut news, &mut closed);
        let f = o.frame.clone().unwrap();
        assert_eq!(f.cells[0], Cell { id: 7, owner: 3, x: 110.0, y: 190.0, mass: 12.0, fx: FX_SHIELD });
        assert_eq!(o.pellets.len(), 2);
        assert_eq!(o.names[&3], ("Dio".to_owned(), 5, "flames".to_owned()));
        assert_eq!(f.bonuses, vec![(50.0, 60.0, 2)]);
        assert_eq!(f.event, Some(("rain".to_owned(), 12)));
        // The next frame: only the changes.
        let text = r#"{"t":"s","me":3,"x":100,"y":200,"h":1000,"c":[],"g":[9,1,2,3],"gx":[1],"e":[],"v":[],"b":[],"n":{},"fx":null,"ev":null}"#;
        o.take(serde_json::from_str(text).unwrap(), at, &mut news, &mut closed);
        let mut ids: Vec<_> = o.pellets.keys().copied().collect();
        ids.sort();
        assert_eq!(ids, vec![2, 9]);
        // A kill by the player.
        o.take(serde_json::from_str(r#"{"t":"kill","by":["Dio",3],"victim":["Lemmy",8],"special":null}"#).unwrap(), at, &mut news, &mut closed);
        assert_eq!(news.ate, 1);
        assert!(matches!(serde_json::from_str::<Wire>(r#"{"t":"joined","id":4,"resumed":true}"#), Ok(Wire::Joined {})));
        let skins: Wire = serde_json::from_str(r#"{"t":"welcome","size":4000,"skins":[["plain","games",0,true,0],["checker","games",5,false,2]]}"#).unwrap();
        o.take(skins, at, &mut news, &mut closed);
        assert_eq!(o.skins[1], Skin("checker".into(), "games".into(), 5, false, 2));
        // A game more: the checkerboard unlocked, announced.
        let records = r#"{"t":"records","l":[],"me":null,"skins":[["plain","games",0,true,0],["checker","games",5,true,5]]}"#;
        o.take(serde_json::from_str(records).unwrap(), at, &mut news, &mut closed);
        assert_eq!(news.unlocked, vec!["checker".to_owned()]);
    }
}
