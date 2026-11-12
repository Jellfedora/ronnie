//! Lundi matin online: the game runs on floor (its src/zombie), Ronnie shows it. A thread keeps a
//! WebSocket to floor while the game's page shows: the rooms first (open one, join one), then, in a
//! game, what the player does (moving, aiming, firing) one way and about 20 times a second what
//! everyone sees the other way. Only with a pseudo. A connection lost during a game comes back by
//! itself: floor keeps the player's character until the room ends.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tungstenite::stream::MaybeTlsStream;

use crate::config::FloorAccount;

const PING_EVERY: Duration = Duration::from_secs(2);
/// Kept of the room's chat.
const CHAT: usize = 8;

/// The zombies' kinds, in floor's order (its KIND_IDS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Base,
    /// The intern: small, fast.
    Intern,
    /// The meeting-itis: slow, slows down who's near.
    Meeting,
    /// The salesman: stops, then charges.
    Sales,
    /// "Reply all": splits in two when it dies.
    ReplyAll,
    /// A copy of a reply-all.
    Cc,
    /// The HR director, every fifth wave.
    Hr,
}

impl Kind {
    fn from(n: f32) -> Kind {
        match n as u32 {
            1 => Kind::Intern,
            2 => Kind::Meeting,
            3 => Kind::Sales,
            4 => Kind::ReplyAll,
            5 => Kind::Cc,
            6 => Kind::Hr,
            _ => Kind::Base,
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            Kind::Base | Kind::ReplyAll => 18.0,
            Kind::Intern => 14.0,
            Kind::Meeting => 22.0,
            Kind::Sales => 20.0,
            Kind::Cc => 12.0,
            Kind::Hr => 40.0,
        }
    }
}

/// The weapons, in floor's order (its WEAPON_IDS).
pub const WEAPONS: [&str; 4] = ["staple", "postit", "extinguisher", "confetti"];

/// A player, at the last frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// Where they aim (radians).
    pub aim: f32,
    pub hp: f32,
    pub state: State,
    /// Their weapon (index in WEAPONS), its ammo.
    pub weapon: usize,
    pub ammo: u32,
    /// Seconds of reloading left.
    pub reload: f32,
    /// Down: seconds of help received (REVIVE needed), seconds before bleeding out.
    pub revive: f32,
    pub bleed: f32,
    pub ready: bool,
    pub offline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Up,
    /// Down, to be helped up.
    Down,
    /// Waits for the next wave.
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zombie {
    pub id: u32,
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    /// Its life, 0 to 1.
    pub life: f32,
    pub slowed: bool,
    pub charging: bool,
    /// Gathering its momentum before a charge.
    pub winding: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub weapon: usize,
}

/// What everyone sees, at the last frame.
#[derive(Clone, Debug)]
pub struct Frame {
    /// A wave under way (else the coffee break).
    pub wave_on: bool,
    pub wave: u32,
    /// Seconds of break left.
    pub break_left: u32,
    /// Zombies still to come or standing.
    pub left: u32,
    /// The team's meal tickets.
    pub tickets: u32,
    pub players: Vec<Player>,
    pub zombies: Vec<Zombie>,
    pub shots: Vec<Shot>,
    pub at: Instant,
}

/// The office, from floor (sent once).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Map {
    pub w: f32,
    pub h: f32,
    pub wall: f32,
    pub desks: Vec<Desk>,
    pub coffee: Spot,
    pub doors: Vec<Door>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct Desk {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Door {
    pub x: f32,
    pub y: f32,
    pub along: String,
    pub span: f32,
}

/// The player's room: who's in it (their id in the game, name, color), its host, started or not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Room {
    pub id: u32,
    pub host: String,
    pub players: Vec<(u32, String, u8)>,
    pub started: bool,
}

/// A room seen from the lobby.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub id: u32,
    pub host: String,
    pub players: u32,
    pub started: bool,
}

/// The game just over, for the player.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Over {
    pub wave: u32,
    pub kills: u32,
    pub revives: u32,
    #[serde(default)]
    pub tickets: u32,
    #[serde(default)]
    pub seconds: u32,
    #[serde(default)]
    pub record: bool,
}

/// What happened once, for the app (sounds, toasts, little texts on the field).
#[derive(Clone, Debug, PartialEq)]
pub enum Happened {
    Wave { wave: u32, boss: bool },
    Cleared(u32),
    Boss,
    Recruit,
    Kill { kind: Kind, x: f32, y: f32, by: u32 },
    Down(u32),
    Revived { player: u32, by: u32 },
    Dead(u32),
    Back(u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Off,
    Connecting,
    Ready,
    Reconnecting(u32),
    /// Refused or lost for good, and why when floor said.
    Failed(Option<String>),
}

#[derive(Default)]
pub struct News {
    pub happened: Vec<Happened>,
    pub over: Option<Over>,
    /// An error floor sent that doesn't close (a room full, gone...).
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum Wire {
    #[serde(rename = "welcome")]
    Welcome { map: Map },
    #[serde(rename = "lobby")]
    Lobby { rooms: Vec<(u32, String, u32, bool)> },
    #[serde(rename = "room")]
    Room {
        id: Option<u32>,
        #[serde(default)]
        host: String,
        #[serde(default)]
        players: Vec<(u32, String, u8)>,
        #[serde(default)]
        started: bool,
    },
    #[serde(rename = "s")]
    Frame { ph: u8, w: u32, tl: u32, zl: u32, tk: u32, p: Vec<f32>, z: Vec<f32>, b: Vec<f32> },
    #[serde(rename = "news")]
    News { l: Vec<serde_json::Value> },
    #[serde(rename = "over")]
    Over(Over),
    #[serde(rename = "chat")]
    Chat { from: String, text: String },
    #[serde(rename = "pong")]
    Pong { n: u64 },
    #[serde(rename = "error")]
    Error { error: String },
}

enum Event {
    Message(Box<Wire>, Instant),
    Closed(Option<String>),
}

struct Link {
    tx: mpsc::Sender<serde_json::Value>,
    rx: mpsc::Receiver<Event>,
}

/// Kept by the app: the connection, and what floor last sent.
#[derive(Default)]
pub struct Online {
    link: Option<Link>,
    with: Option<(egui::Context, FloorAccount)>,
    retry_at: Option<Instant>,
    pub status: Status,
    pub map: Option<Map>,
    pub lobby: Vec<Open>,
    pub room: Option<Room>,
    pub frame: Option<Frame>,
    /// The frame before (to move things smoothly between the two).
    pub before: Option<Frame>,
    pub chat: Vec<(String, String)>,
    /// A room to join as soon as connected (the host's pseudo, after their invitation).
    pub join_host: Option<String>,
    pub rtt: f32,
    pings: Vec<(u64, Instant)>,
    next_ping: u64,
    last_ping: Option<Instant>,
}

fn ws_url(base: &str) -> String {
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_owned()
    };
    format!("{base}/v1/zombie/ws")
}

impl Online {
    pub fn connected(&self) -> bool {
        self.link.is_some() || self.retry_at.is_some()
    }

    pub fn connect(&mut self, ctx: &egui::Context, account: &FloorAccount) {
        let join_host = self.join_host.take();
        *self = Online { with: Some((ctx.clone(), account.clone())), status: Status::Connecting, rtt: 0.1, join_host, ..Default::default() };
        self.open();
    }

    fn open(&mut self) {
        let Some(base) = crate::floor::url() else { return };
        let Some((ctx, account)) = self.with.clone() else { return };
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let (url, token) = (ws_url(&base), account.token.clone());
        std::thread::Builder::new().name("lundi-matin".into()).spawn(move || run(&url, &token, &commands, &events, &ctx)).ok();
        self.link = Some(Link { tx, rx });
        self.retry_at = None;
        self.last_ping = None;
        self.pings.clear();
    }

    /// The page left: out of the room (a game under way keeps the character until it ends).
    pub fn disconnect(&mut self) {
        if self.connected() {
            *self = Online::default();
        }
    }

    fn send(&self, msg: serde_json::Value) {
        if let Some(link) = &self.link {
            let _ = link.tx.send(msg);
        }
    }

    pub fn host(&self) {
        self.send(json!({ "t": "host" }));
    }

    pub fn join(&self, room: u32) {
        self.send(json!({ "t": "join", "room": room }));
    }

    pub fn join_host(&self, host: &str) {
        self.send(json!({ "t": "join", "host": host }));
    }

    pub fn start(&self) {
        self.send(json!({ "t": "start" }));
    }

    pub fn leave(&mut self) {
        self.send(json!({ "t": "leave" }));
    }

    /// Moving (x, y between -1 and 1), aiming (radians), firing.
    pub fn input(&self, mx: f32, my: f32, aim: f32, fire: bool) {
        self.send(json!({ "t": "input", "mx": (mx * 100.0).round() / 100.0, "my": (my * 100.0).round() / 100.0, "aim": (aim * 1000.0).round() / 1000.0, "fire": fire }));
    }

    pub fn pick(&self, weapon: usize) {
        if let Some(w) = WEAPONS.get(weapon) {
            self.send(json!({ "t": "pick", "weapon": w }));
        }
    }

    pub fn reload(&self) {
        self.send(json!({ "t": "reload" }));
    }

    pub fn ready(&self, on: bool) {
        self.send(json!({ "t": "ready", "on": on }));
    }

    /// The player's id in the game under way.
    pub fn me(&self, name: &str) -> Option<u32> {
        self.room.as_ref().filter(|r| r.started).and_then(|r| r.players.iter().find(|(_, n, _)| n == name)).map(|(id, _, _)| *id)
    }

    pub fn playing(&self) -> bool {
        self.room.as_ref().is_some_and(|r| r.started)
    }

    pub fn poll(&mut self) -> News {
        let mut news = News::default();
        if self.retry_at.is_some_and(|at| Instant::now() >= at) {
            self.open();
        }
        if self.link.is_some() && self.status == Status::Ready && self.last_ping.is_none_or(|at| at.elapsed() >= PING_EVERY) {
            self.next_ping += 1;
            self.pings.push((self.next_ping, Instant::now()));
            self.pings.retain(|(_, at)| at.elapsed() < Duration::from_secs(10));
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
                None if matches!(self.status, Status::Ready | Status::Reconnecting(_)) => {
                    let tries = match self.status {
                        Status::Reconnecting(n) => n + 1,
                        _ => 1,
                    };
                    self.status = Status::Reconnecting(tries);
                    self.retry_at = Some(Instant::now() + Duration::from_millis((500 * u64::from(tries)).min(4000)));
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
            Wire::Welcome { map } => {
                self.map = Some(map);
                self.status = Status::Ready;
                if let Some(host) = self.join_host.take() {
                    self.join_host(&host);
                }
            }
            Wire::Lobby { rooms } => {
                self.lobby = rooms.into_iter().map(|(id, host, players, started)| Open { id, host, players, started }).collect();
            }
            Wire::Room { id, host, players, started } => {
                let was = self.playing();
                self.room = id.map(|id| Room { id, host, players, started });
                // A game starts (or ends): the old frames are another's.
                if was != self.playing() {
                    self.frame = None;
                    self.before = None;
                }
            }
            Wire::Frame { ph, w, tl, zl, tk, p, z, b } => {
                let players = p
                    .chunks_exact(12)
                    .map(|v| Player {
                        id: v[0] as u32,
                        x: v[1],
                        y: v[2],
                        aim: v[3] / 100.0,
                        hp: v[4],
                        state: match v[5] as u32 {
                            1 => State::Down,
                            2 => State::Dead,
                            _ => State::Up,
                        },
                        weapon: (v[6].max(0.0) as usize).min(WEAPONS.len() - 1),
                        ammo: v[7] as u32,
                        reload: v[8] / 10.0,
                        revive: v[9] / 10.0,
                        bleed: v[10] / 10.0,
                        ready: v[11] as u32 & 1 != 0,
                        offline: v[11] as u32 & 2 != 0,
                    })
                    .collect();
                let zombies = z
                    .chunks_exact(6)
                    .map(|v| {
                        let fx = v[5] as u32;
                        Zombie { id: v[0] as u32, kind: Kind::from(v[1]), x: v[2], y: v[3], life: v[4] / 100.0, slowed: fx & 1 != 0, charging: fx & 2 != 0, winding: fx & 4 != 0 }
                    })
                    .collect();
                let shots = b.chunks_exact(4).map(|v| Shot { x: v[0], y: v[1], angle: v[2] / 100.0, weapon: (v[3].max(0.0) as usize).min(WEAPONS.len() - 1) }).collect();
                self.before = self.frame.take();
                self.frame = Some(Frame { wave_on: ph == 1, wave: w, break_left: tl, left: zl, tickets: tk, players, zombies, shots, at });
            }
            Wire::News { l } => news.happened.extend(l.iter().filter_map(happened)),
            Wire::Over(over) => news.over = Some(over),
            Wire::Chat { from, text } => {
                self.chat.push((from, text));
                if self.chat.len() > CHAT {
                    self.chat.remove(0);
                }
            }
            Wire::Pong { n } => {
                if let Some(k) = self.pings.iter().position(|(m, _)| *m == n) {
                    let rtt = self.pings.remove(k).1.elapsed().as_secs_f32();
                    self.rtt = self.rtt * 0.7 + rtt * 0.3;
                }
            }
            // Refused at hello (no pseudo...) closes; in the lobby (a room full...) only said.
            Wire::Error { error } => {
                if self.status == Status::Ready {
                    news.error = Some(error);
                } else {
                    *closed = Some(Some(error));
                }
            }
        }
    }
}

fn happened(v: &serde_json::Value) -> Option<Happened> {
    let n = |k: &str| v.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let u = |k: &str| n(k) as u32;
    Some(match v.get("t")?.as_str()? {
        "wave" => Happened::Wave { wave: u("wave"), boss: v.get("boss").and_then(serde_json::Value::as_bool).unwrap_or(false) },
        "cleared" => Happened::Cleared(u("wave")),
        "boss" => Happened::Boss,
        "recruit" => Happened::Recruit,
        "kill" => {
            let kind = match v.get("kind")?.as_str()? {
                "stag" => Kind::Intern,
                "reu" => Kind::Meeting,
                "com" => Kind::Sales,
                "reply" => Kind::ReplyAll,
                "cc" => Kind::Cc,
                "drh" => Kind::Hr,
                _ => Kind::Base,
            };
            Happened::Kill { kind, x: n("x") as f32, y: n("y") as f32, by: u("by") }
        }
        "down" => Happened::Down(u("player")),
        "revived" => Happened::Revived { player: u("player"), by: u("by") },
        "dead" => Happened::Dead(u("player")),
        "back" => Happened::Back(u("player")),
        _ => return None,
    })
}

fn run(url: &str, token: &str, commands: &mpsc::Receiver<serde_json::Value>, events: &mpsc::Sender<Event>, ctx: &egui::Context) {
    let closed = |why: Option<String>| {
        let _ = events.send(Event::Closed(why));
        ctx.request_repaint();
    };
    let mut socket = match tungstenite::connect(url) {
        Ok((socket, _)) => socket,
        Err(e) => {
            crate::log::info(&format!("Lundi matin: {url}: {e}"));
            return closed(None);
        }
    };
    let wait = Some(Duration::from_millis(5));
    let _ = match socket.get_mut() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(wait).and_then(|()| s.set_nodelay(true)),
        MaybeTlsStream::Rustls(s) => s.get_mut().set_read_timeout(wait).and_then(|()| s.get_mut().set_nodelay(true)),
        _ => Ok(()),
    };
    let hello = json!({ "t": "hello", "token": token });
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
                crate::log::info(&format!("Lundi matin: {e}"));
                return closed(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(online: &mut Online, text: &str) -> News {
        let mut news = News::default();
        let mut closed = None;
        online.take(serde_json::from_str(text).unwrap(), Instant::now(), &mut news, &mut closed);
        assert!(closed.is_none());
        news
    }

    #[test]
    fn urls() {
        assert_eq!(ws_url("https://floor.example/floor"), "wss://floor.example/floor/v1/zombie/ws");
        assert_eq!(ws_url("http://localhost:5002"), "ws://localhost:5002/v1/zombie/ws");
    }

    #[test]
    fn rooms_and_frames() {
        let mut online = Online::default();
        take(&mut online, r#"{"t":"welcome","map":{"w":1600,"h":1000,"wall":20,"desks":[{"x":1,"y":2,"w":3,"h":4}],"coffee":{"x":90,"y":500},"doors":[{"x":1,"y":2,"along":"y","span":340}]},"weapons":[],"kinds":[]}"#);
        assert_eq!(online.status, Status::Ready);
        assert_eq!(online.map.as_ref().unwrap().desks.len(), 1);
        take(&mut online, r#"{"t":"lobby","rooms":[[3,"Ada",1,false]]}"#);
        assert_eq!(online.lobby, vec![Open { id: 3, host: "Ada".into(), players: 1, started: false }]);
        take(&mut online, r#"{"t":"room","id":3,"host":"Ada","players":[[7,"Ada",0],[8,"Bob",1]],"started":true}"#);
        assert_eq!(online.me("Bob"), Some(8));
        take(&mut online, r#"{"t":"s","ph":1,"w":2,"tl":0,"zl":9,"tk":40,"p":[7,100,200,157,80,1,1,9,0,12,250,2],"z":[5,3,300,400,50,6],"b":[1,2,314,0]}"#);
        let f = online.frame.as_ref().unwrap();
        assert!(f.wave_on);
        let p = f.players[0];
        assert_eq!((p.state, p.weapon, p.ammo, p.offline), (State::Down, 1, 9, true));
        assert!((p.revive - 1.2).abs() < 1e-4 && (p.bleed - 25.0).abs() < 1e-4);
        let z = f.zombies[0];
        assert_eq!((z.kind, z.charging, z.winding), (Kind::Sales, true, true));
        let news = take(&mut online, r#"{"t":"news","l":[{"t":"wave","wave":3,"boss":false},{"t":"kill","kind":"drh","x":1,"y":2,"by":7},{"t":"nope"}]}"#);
        assert_eq!(news.happened, vec![Happened::Wave { wave: 3, boss: false }, Happened::Kill { kind: Kind::Hr, x: 1.0, y: 2.0, by: 7 }]);
        let news = take(&mut online, r#"{"t":"over","wave":4,"kills":12,"revives":1,"tickets":300,"seconds":200,"record":true}"#);
        assert_eq!(news.over.map(|o| (o.wave, o.record)), Some((4, true)));
        take(&mut online, r#"{"t":"room","id":null}"#);
        assert!(online.room.is_none());
    }
}
