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
    /// A member of the executive committee: three of them, the final boss (wave 20, on the roof).
    Board,
    /// In a video call: its picture lags, it freezes then jumps ahead (from wave 7).
    Visio,
    /// The accountant: a big binder for a shield, shots from the front barely hurt (from wave 9).
    Accountant,
    /// The wellness coach: stays back and heals the zombies around (from wave 12).
    Coach,
    /// The IT guy: down the first time, he reboots and comes back (from wave 14).
    It,
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
            7 => Kind::Board,
            8 => Kind::Visio,
            9 => Kind::Accountant,
            10 => Kind::Coach,
            11 => Kind::It,
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
            Kind::Board => 28.0,
            Kind::Visio => 17.0,
            Kind::Accountant => 20.0,
            Kind::Coach | Kind::It => 18.0,
        }
    }
}

/// Floor's steps the rewards hang on (career.js): colleagues helped up for the bite, escorts of
/// Agnès for her card, her letters.
pub const REVIVES_BITE: u32 = 20;
pub const ESCORTS_CARD: u32 = 10;
pub const LETTERS: u32 = 12;
/// Daily challenges done for the avatar frame (career.js DAILIES_FRAME).
pub const DAILIES_FRAME: u32 = 7;
/// The last wave: the executive committee on the roof; held, the game is won.
pub const LAST_WAVE: u32 = 20;

/// The grades, in floor's order (career.js GRADES): the names it sends.
pub const GRADES: [&str; 8] = ["Stagiaire", "CDD", "CDI", "Chef d’équipe", "Manager", "Directeur", "DG", "PDG"];
/// The grade each thing of the coffee machine opens at (career.js SHOP_GRADE).
pub const SHOP_GRADE: [(&str, u32); 6] = [("mug", 0), ("dmg", 0), ("mag", 0), ("extinguisher", 1), ("turret", 3), ("confetti", 4)];

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
    /// Dashing (RTT).
    pub dashing: bool,
    /// The weapons they have (bits, in WEAPONS' order).
    pub owned: u8,
    /// Their weapon's upgrades: damage, magazine (levels).
    pub ups: (u8, u8),
    /// Each weapon's upgrades (damage, magazine) and ammo, in WEAPONS' order.
    pub all_ups: [(u8, u8); 4],
    pub all_ammo: [u32; 4],
    pub max_hp: f32,
    /// Seconds before the next dash, the next teleport.
    pub dash_in: u32,
    pub teleport_in: u32,
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
    /// Where it heads (radians).
    pub heading: f32,
    /// In a video call, frozen before it jumps.
    pub frozen: bool,
    /// The coach, healing a zombie around.
    pub healing: bool,
    /// The IT guy, rebooting.
    pub rebooting: bool,
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
    pub agnes: Option<Agnes>,
    /// Gérard, and whether he found her.
    pub gerard: Option<(f32, f32, bool)>,
    /// The photocopier turrets: x, y, aim, seconds left.
    pub turrets: Vec<(f32, f32, f32, u32)>,
    /// Agnès' letters lying on the floor.
    pub letters: Vec<(f32, f32)>,
    /// Weapons dropped by zombies: x, y, weapon (index in WEAPONS), seconds left.
    pub drops: Vec<(f32, f32, usize, u32)>,
    /// First-aid kits on the floor: x, y, seconds left.
    pub kits: Vec<(f32, f32, u32)>,
    /// The floor of the tower (0 the hall, 1 the open space, 2 the boardroom, 3 the roof).
    pub floor: usize,
    /// Paused (alone, Escape or another page).
    pub paused: bool,
    pub at: Instant,
}

/// Agnès, from the reception desk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Agnes {
    pub x: f32,
    pub y: f32,
    pub state: AgnesState,
    /// Sitting: seconds of help received.
    pub help: f32,
    /// Her life, 0 to 1, and where she heads (radians).
    pub life: f32,
    pub heading: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgnesState {
    Walking,
    /// Hit by a shot: to be helped up.
    Sitting,
    /// At the coffee machine, everyone served.
    Served,
}

/// The player's career, kept by floor.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Career {
    pub xp: u32,
    /// The grade (0 intern … 7 CEO), its name, the stars past CEO, the XP it starts at, the next one's.
    pub grade: u32,
    pub name: String,
    pub stars: u32,
    pub from: u32,
    pub next: u32,
    /// Perks to pick before a game (how many, which ones).
    pub slots: u32,
    pub perks: Vec<String>,
    /// What the coffee machine sells them.
    pub shop: Vec<String>,
    /// By weapon: mastery level, zombies.
    pub mastery: std::collections::HashMap<String, (u32, u32)>,
    pub games: u32,
    pub kills: u32,
    pub revives: u32,
    pub escorts: u32,
    pub letters: u32,
    #[serde(rename = "bestWave")]
    pub best_wave: u32,
    pub horde: bool,
    /// The week's goals: id, done so far, needed, reached.
    pub week: Vec<(String, u32, u32, bool)>,
    /// The day's challenge: id, done so far, needed, reached; the challenges done, the games won.
    #[serde(default)]
    pub daily: Option<(String, u32, u32, bool)>,
    #[serde(default)]
    pub dailies: u32,
    #[serde(default)]
    pub wins: u32,
}

/// A step passed at the end of a game (what Ronnie announces, and hangs its rewards on).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Milestone {
    Grade { value: String, #[serde(default)] stars: u32 },
    Mastery { weapon: String, level: u32 },
    Wave { value: u32 },
    Revives { value: u32 },
    Escorts { value: u32 },
    Letter { value: u32 },
    Horde,
    Weekly { id: String },
    /// The executive committee beaten, the first time.
    Final,
    Daily { id: String },
    Dailies { value: u32 },
}

/// A floor of the tower, from floor (sent once): its furniture, the coffee machine, the doors.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Map {
    /// 'hall', 'openspace', 'board', 'roof'.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub w: f32,
    #[serde(default)]
    pub h: f32,
    #[serde(default)]
    pub wall: f32,
    pub desks: Vec<Desk>,
    pub coffee: Spot,
    pub doors: Vec<Door>,
    /// The reception's door, Agnès' only, and her way to the coffee.
    #[serde(default)]
    pub reception: Option<Door>,
    #[serde(default)]
    pub agnes_path: Vec<Spot>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Desk {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// What it is ('desk', 'table', 'sofa', 'plant', 'heli'...), drawn as such.
    #[serde(default)]
    pub k: String,
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
    #[serde(default)]
    pub escorts: u32,
    #[serde(default)]
    pub letters: u32,
    #[serde(default)]
    pub xp: u32,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    /// The executive committee beaten: the game won.
    #[serde(default)]
    pub won: bool,
}

/// What happened once, for the app (sounds, toasts, little texts on the field).
#[derive(Clone, Debug, PartialEq)]
pub enum Happened {
    Wave { wave: u32, boss: bool },
    Cleared(u32),
    Boss,
    /// The executive committee comes in.
    FinalBoss,
    /// Up a floor (its number).
    Floor(usize),
    /// The executive committee beaten.
    Won,
    Paused(bool),
    Recruit,
    Kill { kind: Kind, x: f32, y: f32, by: u32 },
    Down(u32),
    Revived { player: u32, by: u32 },
    Dead(u32),
    Back(u32),
    /// Agnès came in by the reception, Gérard by the stairs.
    Agnes,
    Gerard,
    /// A shot made her sit down (by whom); helped up.
    AgnesHit(u32),
    AgnesUp(u32),
    /// Bitten until she sat down.
    AgnesDown,
    /// A weapon dropped by a zombie; picked up (by whom, which, and what it gave: new, dmg, mag, ammo).
    Drop(usize),
    Picked { player: u32, weapon: usize, gain: String },
    /// She served the coffee (who escorted her).
    Coffee(Vec<u32>),
    /// Wave 10 held with her standing: the post-it comes off.
    PostIt,
    /// Gérard found her.
    Reunion,
    LetterDrop,
    /// A letter picked up (by whom).
    Letter(u32),
    Bought { player: u32, item: String },
    Dash(u32),
    Teleport(u32),
    /// The IT guy down for the first time: he reboots, there.
    Reboot { x: f32, y: f32 },
    /// A first-aid kit picked up (by whom, where).
    Healed { player: u32, x: f32, y: f32 },
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
    Welcome {
        map: Map,
        #[serde(default)]
        floors: Vec<Map>,
        #[serde(default)]
        shop: std::collections::HashMap<String, serde_json::Value>,
    },
    #[serde(rename = "profile")]
    Profile(Career),
    #[serde(rename = "perks")]
    Perks { list: Vec<String> },
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
    Frame {
        ph: u8,
        w: u32,
        tl: u32,
        zl: u32,
        tk: u32,
        p: Vec<f32>,
        z: Vec<f32>,
        b: Vec<f32>,
        #[serde(default)]
        a: Option<Vec<f32>>,
        #[serde(default)]
        g: Option<Vec<f32>>,
        #[serde(default)]
        tu: Vec<f32>,
        #[serde(default)]
        l: Vec<f32>,
        #[serde(default)]
        d: Vec<f32>,
        #[serde(default)]
        m: Vec<f32>,
        #[serde(default)]
        fl: usize,
        #[serde(default)]
        ps: u8,
    },
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
    /// The tower's floors (the map's size on each).
    pub floors: Vec<Map>,
    pub lobby: Vec<Open>,
    pub room: Option<Room>,
    pub frame: Option<Frame>,
    /// The frame before (to move things smoothly between the two).
    pub before: Option<Frame>,
    pub chat: Vec<(String, String)>,
    /// A room to join as soon as connected (the host's pseudo, after their invitation).
    pub join_host: Option<String>,
    pub career: Option<Career>,
    /// The perks floor kept for the next game.
    pub perks: Vec<String>,
    /// What the coffee machine costs: item → price (the upgrades, level by level).
    pub prices: std::collections::HashMap<String, serde_json::Value>,
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

    /// Out of the game under way: alone, it ends now (and counts); with others, the character stays.
    pub fn quit(&mut self) {
        self.send(json!({ "t": "quit" }));
    }

    /// Paused or going again (alone only).
    pub fn pause(&self, on: bool) {
        self.send(json!({ "t": "pause", "on": on }));
    }

    /// The floor `k` of the tower (the first one, before floor sent them).
    pub fn floor_map(&self, k: usize) -> Option<&Map> {
        self.floors.get(k).or(self.map.as_ref())
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

    pub fn buy(&self, item: &str) {
        self.send(json!({ "t": "buy", "item": item }));
    }

    pub fn dash(&self) {
        self.send(json!({ "t": "dash" }));
    }

    pub fn teleport(&self) {
        self.send(json!({ "t": "tp" }));
    }

    /// The perks for the next game (before it starts).
    pub fn choose_perks(&self, list: &[String]) {
        self.send(json!({ "t": "perks", "list": list }));
    }

    /// What `item` costs now for `player` (an upgrade: its next level; None: maxed out).
    pub fn price(&self, item: &str, player: &Player) -> Option<u32> {
        let v = self.prices.get(item)?;
        match v {
            serde_json::Value::Array(levels) => {
                let level = if item == "dmg" { player.ups.0 } else { player.ups.1 };
                levels.get(level as usize).and_then(serde_json::Value::as_u64).map(|n| n as u32)
            }
            v => v.as_u64().map(|n| n as u32),
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
            Wire::Profile(career) => self.career = Some(career),
            Wire::Perks { list } => self.perks = list,
            Wire::Welcome { map, floors, shop } => {
                self.floors = floors.into_iter().map(|f| Map { w: map.w, h: map.h, wall: map.wall, ..f }).collect();
                self.map = Some(map);
                self.prices = shop;
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
            Wire::Frame { ph, w, tl, zl, tk, p, z, b, a, g, tu, l, d, m, fl, ps } => {
                let players = p
                    .chunks_exact(22)
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
                        dashing: v[11] as u32 & 4 != 0,
                        owned: v[12] as u8,
                        ups: ((v[13] as u32 / 4) as u8, (v[13] as u32 % 4) as u8),
                        max_hp: v[14],
                        dash_in: v[15] as u32,
                        teleport_in: v[16] as u32,
                        all_ups: std::array::from_fn(|k| {
                            let n = (v[17] as u32 >> (4 * k)) & 15;
                            ((n / 4) as u8, (n % 4) as u8)
                        }),
                        all_ammo: std::array::from_fn(|k| v[18 + k] as u32),
                    })
                    .collect();
                let zombies = z
                    .chunks_exact(7)
                    .map(|v| {
                        let fx = v[5] as u32;
                        Zombie { id: v[0] as u32, kind: Kind::from(v[1]), x: v[2], y: v[3], life: v[4] / 100.0, slowed: fx & 1 != 0, charging: fx & 2 != 0, winding: fx & 4 != 0, heading: v[6] / 100.0, frozen: fx & 8 != 0, healing: fx & 16 != 0, rebooting: fx & 32 != 0 }
                    })
                    .collect();
                let shots = b.chunks_exact(4).map(|v| Shot { x: v[0], y: v[1], angle: v[2] / 100.0, weapon: (v[3].max(0.0) as usize).min(WEAPONS.len() - 1) }).collect();
                let agnes = a.filter(|v| v.len() >= 4).map(|v| Agnes {
                    x: v[0],
                    y: v[1],
                    state: match v[2] as u32 {
                        1 => AgnesState::Sitting,
                        2 => AgnesState::Served,
                        _ => AgnesState::Walking,
                    },
                    help: v[3] / 10.0,
                    life: v.get(4).map_or(1.0, |l| l / 100.0),
                    heading: v.get(5).map_or(0.0, |h| h / 100.0),
                });
                let gerard = g.filter(|v| v.len() >= 3).map(|v| (v[0], v[1], v[2] > 0.0));
                let turrets = tu.chunks_exact(4).map(|v| (v[0], v[1], v[2] / 100.0, v[3] as u32)).collect();
                let letters = l.chunks_exact(2).map(|v| (v[0], v[1])).collect();
                let drops = d.chunks_exact(4).map(|v| (v[0], v[1], (v[2].max(0.0) as usize).min(WEAPONS.len() - 1), v[3] as u32)).collect();
                let kits = m.chunks_exact(3).map(|v| (v[0], v[1], v[2] as u32)).collect();
                self.before = self.frame.take();
                self.frame = Some(Frame { wave_on: ph == 1, wave: w, break_left: tl, left: zl, tickets: tk, players, zombies, shots, agnes, gerard, turrets, letters, drops, kits, floor: fl.min(3), paused: ps == 1, at });
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
        "boss" if v.get("final").and_then(serde_json::Value::as_bool) == Some(true) => Happened::FinalBoss,
        "boss" => Happened::Boss,
        "floor" => Happened::Floor(u("floor") as usize),
        "won" => Happened::Won,
        "paused" => Happened::Paused(v.get("on").and_then(serde_json::Value::as_bool).unwrap_or(false)),
        "recruit" => Happened::Recruit,
        "kill" => {
            let kind = match v.get("kind")?.as_str()? {
                "stag" => Kind::Intern,
                "reu" => Kind::Meeting,
                "com" => Kind::Sales,
                "reply" => Kind::ReplyAll,
                "cc" => Kind::Cc,
                "drh" => Kind::Hr,
                "codir" => Kind::Board,
                "visio" => Kind::Visio,
                "compta" => Kind::Accountant,
                "coach" => Kind::Coach,
                "info" => Kind::It,
                _ => Kind::Base,
            };
            Happened::Kill { kind, x: n("x") as f32, y: n("y") as f32, by: u("by") }
        }
        "down" => Happened::Down(u("player")),
        "revived" => Happened::Revived { player: u("player"), by: u("by") },
        "dead" => Happened::Dead(u("player")),
        "back" => Happened::Back(u("player")),
        "agnes" => Happened::Agnes,
        "gerard" => Happened::Gerard,
        "agnes-hit" => Happened::AgnesHit(u("by")),
        "agnes-up" => Happened::AgnesUp(u("by")),
        "agnes-down" => Happened::AgnesDown,
        "drop" => Happened::Drop(WEAPONS.iter().position(|w| Some(*w) == v.get("weapon").and_then(serde_json::Value::as_str))?),
        "picked" => Happened::Picked {
            player: u("player"),
            weapon: WEAPONS.iter().position(|w| Some(*w) == v.get("weapon").and_then(serde_json::Value::as_str))?,
            gain: v.get("gain")?.as_str()?.to_owned(),
        },
        "coffee" => Happened::Coffee(v.get("escorts").and_then(serde_json::Value::as_array).map(|l| l.iter().filter_map(serde_json::Value::as_u64).map(|n| n as u32).collect()).unwrap_or_default()),
        "postit" => Happened::PostIt,
        "reunion" => Happened::Reunion,
        "letter-drop" => Happened::LetterDrop,
        "letter" => Happened::Letter(u("player")),
        "bought" => Happened::Bought { player: u("player"), item: v.get("item")?.as_str()?.to_owned() },
        "dash" => Happened::Dash(u("player")),
        "teleport" => Happened::Teleport(u("player")),
        "reboot" => Happened::Reboot { x: n("x") as f32, y: n("y") as f32 },
        "heal" => Happened::Healed { player: u("id"), x: n("x") as f32, y: n("y") as f32 },
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
    fn the_tower_the_final_boss_the_day() {
        let mut online = Online::default();
        take(&mut online, r#"{"t":"welcome","map":{"w":1600,"h":1000,"wall":20,"desks":[],"coffee":{"x":90,"y":500},"doors":[]},"floors":[{"id":"hall","desks":[{"x":1,"y":2,"w":3,"h":4,"k":"sofa"}],"coffee":{"x":90,"y":500},"doors":[]},{"id":"roof","desks":[],"coffee":{"x":90,"y":500},"doors":[]}],"last":20}"#);
        assert_eq!(online.floors.len(), 2);
        assert_eq!((online.floors[1].id.as_str(), online.floors[1].w), ("roof", 1600.0));
        assert_eq!(online.floor_map(0).unwrap().desks[0].k, "sofa");
        assert_eq!(online.floor_map(9).unwrap().id, "");
        take(&mut online, r#"{"t":"s","ph":0,"w":16,"tl":5,"zl":0,"tk":0,"p":[],"z":[1,7,10,10,90,0,0],"b":[],"fl":3,"ps":1}"#);
        let f = online.frame.as_ref().unwrap();
        assert_eq!((f.floor, f.paused, f.zombies[0].kind), (3, true, Kind::Board));
        let news = take(&mut online, r#"{"t":"news","l":[{"t":"boss","final":true},{"t":"boss"},{"t":"floor","floor":2},{"t":"won"},{"t":"paused","on":true}]}"#);
        assert_eq!(news.happened, vec![Happened::FinalBoss, Happened::Boss, Happened::Floor(2), Happened::Won, Happened::Paused(true)]);
        let news = take(&mut online, r#"{"t":"over","wave":20,"kills":1,"revives":0,"won":true,"milestones":[{"kind":"final"},{"kind":"daily","id":"kills"},{"kind":"dailies","value":7}]}"#);
        let over = news.over.unwrap();
        assert!(over.won);
        assert_eq!(over.milestones, vec![Milestone::Final, Milestone::Daily { id: "kills".into() }, Milestone::Dailies { value: 7 }]);
        // Higher in the tower: the zombie on a call frozen, the coach healing, the IT guy rebooting; a kit.
        take(&mut online, r#"{"t":"s","ph":1,"w":14,"tl":0,"zl":3,"tk":0,"p":[],"z":[1,8,10,10,90,8,0,2,9,10,10,90,0,0,3,10,10,10,90,16,0,4,11,10,10,50,32,0],"b":[],"m":[300,400,18],"fl":2,"ps":0}"#);
        let f = online.frame.as_ref().unwrap();
        let z: Vec<(Kind, bool, bool, bool)> = f.zombies.iter().map(|z| (z.kind, z.frozen, z.healing, z.rebooting)).collect();
        assert_eq!(z, vec![(Kind::Visio, true, false, false), (Kind::Accountant, false, false, false), (Kind::Coach, false, true, false), (Kind::It, false, false, true)]);
        assert_eq!(f.kits, vec![(300.0, 400.0, 18)]);
        let news = take(&mut online, r#"{"t":"news","l":[{"t":"reboot","x":5,"y":6},{"t":"heal","id":7,"x":1,"y":2},{"t":"kill","kind":"compta","x":1,"y":2,"by":7}]}"#);
        assert_eq!(news.happened, vec![Happened::Reboot { x: 5.0, y: 6.0 }, Happened::Healed { player: 7, x: 1.0, y: 2.0 }, Happened::Kill { kind: Kind::Accountant, x: 1.0, y: 2.0, by: 7 }]);
        take(&mut online, r#"{"t":"profile","xp":0,"grade":0,"name":"Stagiaire","stars":0,"from":0,"next":500,"slots":0,"perks":[],"shop":[],"mastery":{},"games":0,"kills":0,"revives":0,"escorts":0,"letters":0,"bestWave":0,"horde":false,"week":[],"daily":["kills",40,120,false],"dailies":3,"wins":1}"#);
        let c = online.career.as_ref().unwrap();
        assert_eq!((c.daily.clone(), c.dailies, c.wins), (Some(("kills".into(), 40, 120, false)), 3, 1));
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
        take(&mut online, r#"{"t":"s","ph":1,"w":2,"tl":0,"zl":9,"tk":40,"p":[7,100,200,157,80,1,1,9,0,12,250,2,3,5,125,2,0,857,30,9,0,0],"z":[5,3,300,400,50,6,157],"b":[1,2,314,0],"a":[10,20,1,15,40,314],"g":null,"tu":[1,2,100,30],"l":[],"d":[500,600,3,12]}"#);
        let f = online.frame.as_ref().unwrap();
        assert!(f.wave_on);
        let p = f.players[0];
        assert_eq!((p.state, p.weapon, p.ammo, p.offline), (State::Down, 1, 9, true));
        assert!((p.revive - 1.2).abs() < 1e-4 && (p.bleed - 25.0).abs() < 1e-4);
        assert_eq!((p.owned, p.ups, p.max_hp, p.dash_in), (3, (1, 1), 125.0, 2));
        assert_eq!(f.agnes.map(|a| (a.state, a.life)), Some((AgnesState::Sitting, 0.4)));
        assert_eq!(p.all_ups, [(2, 1), (1, 1), (0, 3), (0, 0)]);
        assert_eq!(p.all_ammo, [30, 9, 0, 0]);
        assert_eq!(f.drops, vec![(500.0, 600.0, 3, 12)]);
        assert_eq!(f.turrets, vec![(1.0, 2.0, 1.0, 30)]);
        let z = f.zombies[0];
        assert_eq!((z.kind, z.charging, z.winding), (Kind::Sales, true, true));
        let news = take(&mut online, r#"{"t":"news","l":[{"t":"wave","wave":3,"boss":false},{"t":"kill","kind":"drh","x":1,"y":2,"by":7},{"t":"nope"}]}"#);
        assert_eq!(news.happened, vec![Happened::Wave { wave: 3, boss: false }, Happened::Kill { kind: Kind::Hr, x: 1.0, y: 2.0, by: 7 }]);
        let news = take(&mut online, r#"{"t":"over","wave":4,"kills":12,"revives":1,"tickets":300,"seconds":200,"record":true,"xp":500,"milestones":[{"kind":"grade","value":"CDD","stars":0},{"kind":"horde"},{"kind":"mastery","weapon":"staple","level":1}]}"#);
        let over = news.over.unwrap();
        assert_eq!((over.wave, over.record, over.xp), (4, true, 500));
        assert_eq!(over.milestones[1], Milestone::Horde);
        take(&mut online, r#"{"t":"profile","xp":600,"grade":1,"name":"CDD","stars":0,"from":500,"next":1500,"slots":0,"perks":[],"shop":["mug"],"mastery":{"staple":[1,55]},"games":1,"kills":12,"revives":1,"escorts":0,"letters":0,"bestWave":4,"horde":false,"week":[["kills",12,300,false]]}"#);
        assert_eq!(online.career.as_ref().map(|c| (c.grade, c.mastery["staple"])), Some((1, (1, 55))));
        take(&mut online, r#"{"t":"room","id":null}"#);
        assert!(online.room.is_none());
    }
}
