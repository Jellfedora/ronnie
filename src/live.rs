//! The games online, on floor: who is online, the invitations, the match played for two (Puissance 4,
//! a battle of Riff Hero, Larsen), the table of the Président (3 to 6). Only with a pseudo (see floor.rs).
//!
//! A thread talks to floor while the app runs: a sign of life every few seconds (the player is online
//! while it comes, and learns who else is, and the invitations received), every second while a match
//! is watched. The app sends it what the player does, and takes what floor answered at each frame.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::FloorAccount;

/// A sign of life this often, out of a match (in seconds); with an invitation going on, faster.
const PING_IDLE: f64 = 5.0;
const PING_BUSY: f64 = 2.0;
/// The match watched, fetched this often.
const MATCH_EVERY: f64 = 1.0;

/// Another player online.
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Online {
    pub name: String,
    /// In a match.
    pub busy: bool,
}

/// An invitation received.
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Invite {
    pub id: String,
    pub from: Option<String>,
    /// "puissance-4", "ronnie-io" or "riff-hero".
    #[serde(default)]
    pub game: Option<String>,
}

/// The last invitation sent: "pending", "accepted" (and its match), "declined", "expired", "cancelled".
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Sent {
    pub id: String,
    pub to: Option<String>,
    pub status: String,
    #[serde(rename = "match")]
    pub match_id: Option<String>,
    /// "puissance-4" or "ronnie-io".
    #[serde(default)]
    pub game: Option<String>,
}

#[derive(Clone, Deserialize, Debug, Default, PartialEq)]
pub struct Rematch {
    pub you: bool,
    pub them: bool,
}

/// Where a player of a Riff Hero battle is in the song.
#[derive(Clone, Copy, Deserialize, Debug, Default, PartialEq)]
pub struct RiffSide {
    pub score: u32,
    pub combo: u32,
    pub hits: u32,
    pub misses: u32,
    /// Played to the end: the score is final.
    pub done: bool,
}

fn puissance_4() -> String {
    "puissance-4".into()
}

/// A piece of Larsen's gear (the amp wall, the drums...): its first cell, its length, which way it lies.
#[derive(Clone, Copy, Deserialize, serde::Serialize, Debug, PartialEq, Eq)]
pub struct Piece {
    pub x: u8,
    pub y: u8,
    pub length: u8,
    pub horizontal: bool,
    /// Fried (the player's own pieces, as floor says).
    #[serde(default, skip_serializing)]
    pub fried: bool,
}

impl Piece {
    /// Its cells.
    pub fn cells(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        (0..self.length).map(|k| if self.horizontal { (self.x + k, self.y) } else { (self.x, self.y + k) })
    }
}

/// Who set up their gear.
#[derive(Clone, Copy, Deserialize, Debug, Default, PartialEq)]
pub struct Placed {
    pub you: bool,
    pub them: bool,
}

/// The gear still standing on each side.
#[derive(Clone, Copy, Deserialize, Debug, Default, PartialEq)]
pub struct Standing {
    pub you: u8,
    pub them: u8,
}

/// Larsen's stages as the player sees them: their gear and the feedback it took; of the other's, their
/// shots and the pieces fried (never the gear standing).
#[derive(Clone, Deserialize, Debug, Default, PartialEq)]
pub struct Stage {
    pub placed: Placed,
    pub gear: Option<Vec<Piece>>,
    /// Shots: x, y, hit.
    pub received: Vec<(u8, u8, bool)>,
    pub fired: Vec<(u8, u8, bool)>,
    pub fried: Vec<Piece>,
    pub standing: Standing,
}

/// A match, as floor sees it for this player.
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Match {
    pub id: String,
    /// "puissance-4", "riff-hero" or "larsen".
    #[serde(default = "puissance_4")]
    pub game: String,
    /// Larsen: both stages as the player sees them.
    #[serde(default)]
    pub stage: Option<Stage>,
    /// Riff Hero: the song's seed, in how many milliseconds it starts (from `at`; negative: started),
    /// where both players are.
    #[serde(default)]
    pub seed: u32,
    #[serde(default)]
    pub starts_in: i64,
    #[serde(default)]
    pub riff: [RiffSide; 2],
    /// When floor's answer came.
    #[serde(skip, default = "Instant::now")]
    pub at: Instant,
    /// Player 1 (who starts), player 2.
    pub players: [Option<String>; 2],
    /// 1 or 2.
    pub you: u8,
    /// A stack per column, from the bottom.
    pub columns: Vec<Vec<u8>>,
    pub turn: u8,
    pub moves: u32,
    /// The last disc dropped: column, row.
    pub last: Option<[usize; 2]>,
    pub over: bool,
    /// 1 or 2, 3: a draw.
    pub winner: Option<u8>,
    /// "line", "draw", "forfeit", "score" (Riff Hero) or "fried" (Larsen).
    pub reason: Option<String>,
    pub line: Option<Vec<[usize; 2]>>,
    pub rematch: Rematch,
    /// The rematch, once both asked.
    pub next: Option<String>,
    /// The chat: who wrote (their pseudo), what, the player's own.
    #[serde(default)]
    pub chat: Vec<(Option<String>, String, bool)>,
}

impl Match {
    /// A battle of Riff Hero.
    pub fn riff(&self) -> bool {
        self.game == "riff-hero"
    }

    /// A match of Puissance 4.
    pub fn four(&self) -> bool {
        self.game == "puissance-4"
    }

    /// A match of Larsen.
    pub fn larsen(&self) -> bool {
        self.game == "larsen"
    }

    /// The other player's name.
    pub fn them(&self) -> Option<&str> {
        self.players.get(2 - self.you as usize).and_then(|n| n.as_deref())
    }
}

/// A seat at a table of the Président, as floor shows it.
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Seat {
    pub name: Option<String>,
    /// Cards left in hand.
    pub cards: usize,
    /// Its role in the round before ("president", "vice", "neutre", "vice-trou", "trou").
    pub role: Option<String>,
    /// Out of the trick.
    pub passed: bool,
    /// Its place out of cards in this round (-1: still in).
    pub place: i32,
    pub online: bool,
}

/// What lies on the trick: its value and how many cards of it.
#[derive(Clone, Copy, Deserialize, Debug, PartialEq)]
pub struct TableTop {
    pub rank: u8,
    pub count: u8,
}

/// The cards the player gives back (`k`), to whom (a seat).
#[derive(Clone, Copy, Deserialize, Debug, PartialEq)]
pub struct Give {
    pub k: usize,
    pub to: usize,
}

/// What the player gave and got at the last exchange, and with whom.
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Swap {
    pub gave: Vec<[u8; 2]>,
    pub got: Vec<[u8; 2]>,
    pub with: usize,
}

/// A table of the Président, as floor sees it for this player (a card: [value 0 to 12, suit 0 to 3]).
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Table {
    pub id: String,
    /// The player's seat, and the host's.
    pub you: i32,
    pub host: i32,
    /// "waiting", "playing", "over", "give" or "ended".
    pub status: String,
    /// Why it ended: "closed", "left", "away"; who left.
    pub reason: Option<String>,
    pub left: Option<String>,
    pub seats: Vec<Seat>,
    /// Invited, not answered yet.
    #[serde(default)]
    pub invited: Vec<Option<String>>,
    pub hand: Vec<[u8; 2]>,
    pub turn: Option<usize>,
    pub top: Option<TableTop>,
    pub laid: Vec<[u8; 2]>,
    pub leader: Option<usize>,
    pub out: Vec<usize>,
    /// The roles of the round just over (for the exchange to come).
    pub next_roles: Option<Vec<String>>,
    pub give: Option<Give>,
    /// The seats still to give back.
    pub waiting: Vec<usize>,
    pub swap: Option<Swap>,
    pub rounds: u32,
    pub presidencies: Vec<u32>,
    /// Changes with each news.
    pub moves: u32,
    #[serde(default)]
    pub chat: Vec<(Option<String>, String, bool)>,
}

impl Table {
    /// The player hosts it.
    pub fn hosting(&self) -> bool {
        self.you == self.host
    }
}

#[derive(Deserialize)]
struct Ping {
    online: Vec<Online>,
    invites: Vec<Invite>,
    sent: Option<Sent>,
    #[serde(rename = "match")]
    match_id: Option<String>,
    /// The table of the Président the player sits at.
    #[serde(default)]
    table: Option<String>,
}

/// What the player does at a table of the Président.
enum TableCmd {
    Start,
    Play(Vec<[u8; 2]>),
    Pass,
    Next,
    Give(Vec<[u8; 2]>),
    Leave,
    Say(String),
}

/// What the player does.
enum Cmd {
    /// The player, the game.
    Invite(String, &'static str),
    Accept(String),
    Decline(String),
    Cancel(String),
    Move(String, usize),
    Leave(String),
    Rematch(String),
    /// Riff Hero: where the player is in the song.
    Riff(String, RiffSide),
    /// A message to the other player, in the match.
    Say(String, String),
    /// Larsen: the player's gear, a shot.
    Gear(String, Vec<Piece>),
    Shot(String, u8, u8),
    /// The match to fetch every second (None: none, out of the page).
    Watch(Option<String>),
    /// At the table (its id).
    Table(String, TableCmd),
    /// The table to fetch every second (None: none, out of the page).
    WatchTable(Option<String>),
    /// The board of online wins of a game.
    Board(&'static str),
}

enum Event {
    Ping(Ping),
    Match(Match),
    Table(Box<Table>),
    /// The table watched is gone from floor.
    TableGone,
    Board(&'static str, Vec<(String, u32)>),
    /// floor refused (its message).
    Refused(String),
    /// floor answered, or not.
    Reachable(bool),
    /// floor doesn't know the account (or it has no pseudo): the pseudo is to pick again.
    Forgotten,
}

struct Worker {
    /// The floor and the token it talks with.
    key: (String, String),
    tx: mpsc::Sender<Cmd>,
    rx: mpsc::Receiver<Event>,
}

/// Kept by the app.
#[derive(Default)]
pub struct Live {
    worker: Option<Worker>,
    pub online: Vec<Online>,
    /// Invitations received, not answered yet.
    pub invites: Vec<Invite>,
    pub sent: Option<Sent>,
    /// The match the player is in, for floor (finished: None).
    pub playing: Option<String>,
    /// The match watched (the page shows it).
    pub game: Option<Match>,
    /// The table of the Président the player sits at, for floor; the one watched (the page shows it).
    pub seated: Option<String>,
    pub table: Option<Table>,
    /// The boards of online wins (name, wins), by game, when fetched.
    pub boards: HashMap<&'static str, Vec<(String, u32)>>,
    /// floor answered (None: not asked yet).
    pub reachable: Option<bool>,
    /// What floor refused last (shown until the next action).
    pub refused: Option<String>,
    /// Invitations already announced (notified once).
    announced: HashSet<String>,
}

impl Live {
    /// Started with a pseudo, stopped without; what floor answered taken. True: floor forgot the
    /// account (the pseudo is to pick again).
    pub fn frame(&mut self, ctx: &egui::Context, account: Option<&FloorAccount>) -> bool {
        let key = crate::floor::url().zip(account.filter(|_| crate::floor::pseudo(account).is_some())).map(|(url, a)| (url, a.token.clone()));
        if self.worker.as_ref().map(|w| &w.key) != key.as_ref() {
            // Another account, or none: what was known goes.
            *self = Live { announced: std::mem::take(&mut self.announced), ..Live::default() };
            self.worker = key.map(|key| spawn(ctx, key));
        }
        let mut forgotten = false;
        let Some(worker) = &self.worker else { return false };
        while let Ok(event) = worker.rx.try_recv() {
            match event {
                Event::Ping(p) => {
                    self.online = p.online;
                    self.invites = p.invites;
                    self.sent = p.sent;
                    self.playing = p.match_id;
                    self.seated = p.table;
                }
                Event::Table(t) => {
                    if self.table.as_ref().is_none_or(|old| old.id != t.id || old.moves <= t.moves) {
                        self.table = Some(*t);
                    }
                }
                Event::TableGone => self.table = None,
                // The match watched (or the rematch, which the thread watches in its turn).
                Event::Match(m) => {
                    if self.game.as_ref().is_none_or(|g| g.id == m.id || !m.over) {
                        self.game = Some(m);
                    }
                }
                Event::Board(game, b) => {
                    self.boards.insert(game, b);
                }
                Event::Refused(e) => self.refused = Some(e),
                Event::Reachable(r) => self.reachable = Some(r),
                Event::Forgotten => forgotten = true,
            }
        }
        if forgotten {
            self.worker = None;
        }
        forgotten
    }

    /// Invitations received not announced yet (each one once).
    pub fn new_invites(&mut self) -> Vec<Invite> {
        let new: Vec<Invite> = self.invites.iter().filter(|i| !self.announced.contains(&i.id)).cloned().collect();
        self.announced.extend(new.iter().map(|i| i.id.clone()));
        new
    }

    fn send(&mut self, cmd: Cmd) {
        self.refused = None;
        if let Some(w) = &self.worker {
            let _ = w.tx.send(cmd);
        }
    }

    /// To a game of Puissance 4.
    pub fn invite(&mut self, name: &str) {
        self.invite_to(name, "puissance-4");
    }

    /// To a game: "puissance-4", "ronnie-io" or "riff-hero".
    pub fn invite_to(&mut self, name: &str, game: &'static str) {
        self.send(Cmd::Invite(name.to_owned(), game));
    }

    /// Accepted: its match watched.
    pub fn accept(&mut self, id: &str) {
        self.invites.retain(|i| i.id != id);
        self.send(Cmd::Accept(id.to_owned()));
    }

    pub fn decline(&mut self, id: &str) {
        self.invites.retain(|i| i.id != id);
        self.send(Cmd::Decline(id.to_owned()));
    }

    pub fn cancel(&mut self) {
        if let Some(sent) = self.sent.take().filter(|s| s.status == "pending") {
            self.send(Cmd::Cancel(sent.id));
        }
    }

    /// The disc dropped in column `c`, shown at once (floor's answer follows).
    pub fn play(&mut self, c: usize) {
        let Some(game) = &mut self.game else { return };
        if !game.four() || game.over || game.turn != game.you || game.columns.get(c).is_none_or(|col| col.len() >= crate::connect4::ROWS) {
            return;
        }
        game.columns[c].push(game.you);
        game.moves += 1;
        game.turn = 3 - game.you;
        let id = game.id.clone();
        self.send(Cmd::Move(id, c));
    }

    pub fn leave(&mut self) {
        if let Some(id) = self.game.as_ref().map(|g| g.id.clone()) {
            self.send(Cmd::Leave(id));
        }
    }

    pub fn rematch(&mut self) {
        if let Some(game) = &mut self.game {
            game.rematch.you = true;
            let id = game.id.clone();
            self.send(Cmd::Rematch(id));
        }
    }

    /// Riff Hero: where the player is in the song, sent to the other (`done`: the final score).
    pub fn riff(&mut self, side: RiffSide) {
        let Some(game) = self.game.as_mut().filter(|g| g.riff() && !g.over) else { return };
        let k = game.you as usize - 1;
        if game.riff[k].done {
            return;
        }
        game.riff[k] = side;
        let id = game.id.clone();
        self.send(Cmd::Riff(id, side));
    }

    /// Larsen: the player's gear, shown set up at once (floor's answer follows).
    pub fn gear(&mut self, gear: Vec<Piece>) {
        let Some(game) = self.game.as_mut().filter(|g| g.larsen() && !g.over) else { return };
        let stage = game.stage.get_or_insert_with(Stage::default);
        if stage.placed.you {
            return;
        }
        stage.placed.you = true;
        stage.gear = Some(gear.clone());
        let id = game.id.clone();
        self.send(Cmd::Gear(id, gear));
    }

    /// Larsen: feedback at (x, y) on the other's stage, on the player's turn (floor says if it hit).
    pub fn shoot(&mut self, x: u8, y: u8) {
        let Some(game) = self.game.as_mut().filter(|g| g.larsen() && !g.over && g.turn == g.you) else { return };
        if !game.stage.as_ref().is_some_and(|n| n.placed.you && n.placed.them && !n.fired.iter().any(|s| (s.0, s.1) == (x, y))) {
            return;
        }
        game.turn = 3 - game.you;
        let id = game.id.clone();
        self.send(Cmd::Shot(id, x, y));
    }

    /// A message in the match's chat, shown at once (floor's answer follows).
    pub fn say(&mut self, text: &str) {
        if let Some(game) = &mut self.game {
            game.chat.push((None, text.to_owned(), true));
            let id = game.id.clone();
            self.send(Cmd::Say(id, text.to_owned()));
        }
    }

    /// The match shown in the page (None: the page left it).
    pub fn watch(&mut self, id: Option<String>) {
        if id.is_none() {
            self.game = None;
        }
        self.send(Cmd::Watch(id));
    }

    /// A page left its match of `game` ("puissance-4", "riff-hero", "larsen"). Another game's
    /// match, watched meanwhile, stays.
    pub fn unwatch(&mut self, game: &str) {
        if self.game.as_ref().is_none_or(|g| g.game == game) {
            self.watch(None);
        }
    }

    /// The table shown in the page (None: the page left it).
    pub fn watch_table(&mut self, id: Option<String>) {
        if id.is_none() {
            self.table = None;
        }
        self.send(Cmd::WatchTable(id));
    }

    fn at_table(&mut self, cmd: TableCmd) {
        if let Some(id) = self.table.as_ref().map(|t| t.id.clone()).or_else(|| self.seated.clone()) {
            self.send(Cmd::Table(id, cmd));
        }
    }

    /// The host starts the game (3 at least).
    pub fn table_start(&mut self) {
        self.at_table(TableCmd::Start);
    }

    /// Cards laid on the trick (floor checks them).
    pub fn table_play(&mut self, cards: Vec<[u8; 2]>) {
        self.at_table(TableCmd::Play(cards));
    }

    pub fn table_pass(&mut self) {
        self.at_table(TableCmd::Pass);
    }

    /// The host deals the next round.
    pub fn table_next(&mut self) {
        self.at_table(TableCmd::Next);
    }

    /// The cards the Président (or the Vice) gives back.
    pub fn table_give(&mut self, cards: Vec<[u8; 2]>) {
        self.at_table(TableCmd::Give(cards));
    }

    /// Up from the table: before the game, the seat freed (the host closes it); during, it stops.
    pub fn table_leave(&mut self) {
        self.at_table(TableCmd::Leave);
    }

    /// A message to the table, shown at once (floor's answer follows).
    pub fn table_say(&mut self, text: &str) {
        if let Some(t) = &mut self.table {
            t.chat.push((None, text.to_owned(), true));
        }
        self.at_table(TableCmd::Say(text.to_owned()));
    }

    /// The board of wins of `game` ("puissance-4", "riff-hero", "larsen"), fetched again.
    pub fn fetch_board(&mut self, game: &'static str) {
        self.send(Cmd::Board(game));
    }
}

fn spawn(ctx: &egui::Context, key: (String, String)) -> Worker {
    let (tx, cmds) = mpsc::channel();
    let (events, rx) = mpsc::channel();
    let (url, token, ctx) = (key.0.clone(), key.1.clone(), ctx.clone());
    std::thread::Builder::new()
        .name("floor-live".into())
        .spawn(move || run(&url, &token, &cmds, &events, &ctx))
        .ok();
    Worker { key, tx, rx }
}

/// floor's answer: its JSON, or the event saying why not.
fn call(agent: &ureq::Agent, method: &str, url: String, token: &str, body: Option<Value>) -> Result<Value, Event> {
    let auth = format!("Bearer {token}");
    let user_agent = concat!("ronnie/", env!("CARGO_PKG_VERSION"));
    let answer = match (method, body) {
        ("GET", _) => agent.get(&url).header("Authorization", &auth).header("User-Agent", user_agent).call(),
        ("DELETE", _) => agent.delete(&url).header("Authorization", &auth).header("User-Agent", user_agent).call(),
        (_, body) => agent.post(&url).header("Authorization", &auth).header("User-Agent", user_agent).send_json(body.unwrap_or_else(|| json!({}))),
    };
    let mut answer = answer.map_err(|e| {
        crate::log::info(&format!("floor live: {e}"));
        Event::Reachable(false)
    })?;
    let status = answer.status().as_u16();
    let value: Value = answer.body_mut().read_json().unwrap_or(Value::Null);
    match status {
        200..=299 => Ok(value),
        401 | 403 => Err(Event::Forgotten),
        400..=499 => Err(Event::Refused(value["error"].as_str().unwrap_or("?").to_owned())),
        _ => Err(Event::Reachable(false)),
    }
}

fn run(url: &str, token: &str, cmds: &mpsc::Receiver<Cmd>, events: &mpsc::Sender<Event>, ctx: &egui::Context) {
    let agent: ureq::Agent = ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(10))).http_status_as_error(false).build().into();
    let base = format!("{url}/v1/live");
    let start = Instant::now();
    let now = || start.elapsed().as_secs_f64();
    let (mut last_ping, mut last_match) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut watched: Option<String> = None;
    // The table watched, and when last fetched.
    let mut watched_table: Option<String> = None;
    let mut last_table = f64::NEG_INFINITY;
    let mut busy = false;
    let mut reachable = None;
    loop {
        let cmd = match cmds.recv_timeout(Duration::from_millis(250)) {
            Ok(cmd) => Some(cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            // The app stopped it (no pseudo any more, or another account).
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let mut out = Vec::new();
        let mut result = |r: Result<Value, Event>, out: &mut Vec<Event>| match r {
            Ok(v) => {
                if reachable != Some(true) {
                    reachable = Some(true);
                    out.push(Event::Reachable(true));
                }
                Some(v)
            }
            Err(Event::Reachable(false)) => {
                if reachable != Some(false) {
                    reachable = Some(false);
                    out.push(Event::Reachable(false));
                }
                None
            }
            Err(e) => {
                out.push(e);
                None
            }
        };
        match cmd {
            Some(Cmd::Invite(name, game)) => {
                result(call(&agent, "POST", format!("{base}/invites"), token, Some(json!({ "to": name, "game": game }))), &mut out);
                last_ping = f64::NEG_INFINITY;
            }
            Some(Cmd::Accept(id)) => {
                let answer = result(call(&agent, "POST", format!("{base}/invites/{id}/accept"), token, None), &mut out);
                // The Président: a seat at a table; the others: a match.
                if let Some(t) = answer.as_ref().filter(|v| v["game"] == "president").and_then(|v| serde_json::from_value::<Table>(v.clone()).ok()) {
                    watched_table = Some(t.id.clone());
                    last_table = now();
                    out.push(Event::Table(Box::new(t)));
                } else if let Some(m) = answer.and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    watched = Some(m.id.clone());
                    out.push(Event::Match(m));
                }
                last_ping = f64::NEG_INFINITY;
            }
            Some(Cmd::Table(id, cmd)) => {
                let (path, body) = match cmd {
                    TableCmd::Start => ("start", None),
                    TableCmd::Play(cards) => ("play", Some(json!({ "cards": cards }))),
                    TableCmd::Pass => ("pass", None),
                    TableCmd::Next => ("next", None),
                    TableCmd::Give(cards) => ("give", Some(json!({ "cards": cards }))),
                    TableCmd::Leave => ("leave", None),
                    TableCmd::Say(text) => ("chat", Some(json!({ "text": text }))),
                };
                let answer = result(call(&agent, "POST", format!("{base}/tables/{id}/{path}"), token, body), &mut out);
                match answer.and_then(|v| serde_json::from_value::<Table>(v).ok()) {
                    Some(t) => {
                        out.push(Event::Table(Box::new(t)));
                        last_table = now();
                    }
                    // Refused (or lost): floor's table again.
                    None => last_table = f64::NEG_INFINITY,
                }
                if path == "leave" {
                    last_ping = f64::NEG_INFINITY;
                }
            }
            Some(Cmd::WatchTable(id)) => {
                watched_table = id;
                last_table = f64::NEG_INFINITY;
            }
            Some(Cmd::Decline(id)) => {
                result(call(&agent, "POST", format!("{base}/invites/{id}/decline"), token, None), &mut out);
            }
            Some(Cmd::Cancel(id)) => {
                result(call(&agent, "DELETE", format!("{base}/invites/{id}"), token, None), &mut out);
                last_ping = f64::NEG_INFINITY;
            }
            Some(Cmd::Move(id, c)) => {
                let answer = result(call(&agent, "POST", format!("{base}/matches/{id}/moves"), token, Some(json!({ "column": c }))), &mut out);
                match answer.and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    Some(m) => {
                        out.push(Event::Match(m));
                        last_match = now();
                    }
                    // Refused (or lost): floor's grid again.
                    None => last_match = f64::NEG_INFINITY,
                }
            }
            Some(Cmd::Leave(id)) => {
                if let Some(m) = result(call(&agent, "POST", format!("{base}/matches/{id}/leave"), token, None), &mut out).and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    out.push(Event::Match(m));
                }
                last_ping = f64::NEG_INFINITY;
            }
            Some(Cmd::Rematch(id)) => {
                if let Some(m) = result(call(&agent, "POST", format!("{base}/matches/{id}/rematch"), token, None), &mut out).and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    out.push(Event::Match(m));
                }
            }
            Some(Cmd::Riff(id, side)) => {
                let body = json!({ "score": side.score, "combo": side.combo, "hits": side.hits, "misses": side.misses, "done": side.done });
                if let Some(m) = result(call(&agent, "POST", format!("{base}/matches/{id}/riff"), token, Some(body)), &mut out).and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    out.push(Event::Match(m));
                    last_match = now();
                }
            }
            Some(Cmd::Say(id, text)) => {
                if let Some(m) = result(call(&agent, "POST", format!("{base}/matches/{id}/chat"), token, Some(json!({ "text": text }))), &mut out).and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    out.push(Event::Match(m));
                }
            }
            Some(Cmd::Gear(id, gear)) => {
                let answer = result(call(&agent, "POST", format!("{base}/matches/{id}/gear"), token, Some(json!({ "gear": gear }))), &mut out);
                match answer.and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    Some(m) => {
                        out.push(Event::Match(m));
                        last_match = now();
                    }
                    None => last_match = f64::NEG_INFINITY,
                }
            }
            Some(Cmd::Shot(id, x, y)) => {
                let answer = result(call(&agent, "POST", format!("{base}/matches/{id}/shots"), token, Some(json!({ "x": x, "y": y }))), &mut out);
                match answer.and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    Some(m) => {
                        out.push(Event::Match(m));
                        last_match = now();
                    }
                    // Refused (or lost): floor's state again.
                    None => last_match = f64::NEG_INFINITY,
                }
            }
            Some(Cmd::Watch(id)) => {
                watched = id;
                last_match = f64::NEG_INFINITY;
            }
            Some(Cmd::Board(game)) => {
                #[derive(Deserialize)]
                struct Row {
                    name: Option<String>,
                    score: u32,
                }
                #[derive(Deserialize)]
                struct Board {
                    entries: Vec<Row>,
                }
                let board = agent.get(format!("{url}/v1/games/{game}/leaderboard")).query("limit", "10").call().ok().and_then(|mut r| r.body_mut().read_json::<Board>().ok());
                if let Some(b) = board {
                    out.push(Event::Board(game, b.entries.into_iter().map(|r| (r.name.unwrap_or_default(), r.score)).collect()));
                }
            }
            None => {}
        }
        if now() - last_ping >= if busy || watched.is_some() { PING_BUSY } else { PING_IDLE } {
            last_ping = now();
            if let Some(p) = result(call(&agent, "POST", format!("{base}/ping"), token, None), &mut out).and_then(|v| serde_json::from_value::<Ping>(v).ok()) {
                busy = !p.invites.is_empty() || p.match_id.is_some() || p.table.is_some() || p.sent.as_ref().is_some_and(|s| s.status == "pending");
                // Seated at a table (the one opened by inviting, or joined): it is the one watched.
                if let Some(id) = p.table.clone().filter(|id| watched_table.as_ref() != Some(id)) {
                    watched_table = Some(id);
                    last_table = f64::NEG_INFINITY;
                }
                // The invitation sent was accepted: its match watched.
                if let Some(id) = p.sent.as_ref().and_then(|s| s.match_id.clone()).filter(|id| p.match_id.as_ref() == Some(id)) {
                    if watched.is_none() {
                        watched = Some(id);
                        last_match = f64::NEG_INFINITY;
                    }
                }
                out.push(Event::Ping(p));
            }
        }
        if let Some(id) = watched.clone().filter(|_| now() - last_match >= MATCH_EVERY) {
            last_match = now();
            match call(&agent, "GET", format!("{base}/matches/{id}"), token, None) {
                Ok(v) => {
                    if let Ok(m) = serde_json::from_value::<Match>(v) {
                        // Both asked for a rematch: it is the one watched now.
                        if let Some(next) = m.next.clone().filter(|_| m.rematch.you) {
                            watched = Some(next);
                            last_match = f64::NEG_INFINITY;
                        }
                        out.push(Event::Match(m));
                    }
                }
                // Gone from floor (restarted, or kept too long): not watched any more.
                Err(Event::Refused(_)) => watched = None,
                Err(e) => {
                    result(Err(e), &mut out);
                }
            }
        }
        if let Some(id) = watched_table.clone().filter(|_| now() - last_table >= MATCH_EVERY) {
            last_table = now();
            match call(&agent, "GET", format!("{base}/tables/{id}"), token, None) {
                Ok(v) => {
                    if let Ok(t) = serde_json::from_value::<Table>(v) {
                        out.push(Event::Table(Box::new(t)));
                    }
                }
                // Gone from floor (restarted, or kept too long).
                Err(Event::Refused(_)) => {
                    watched_table = None;
                    out.push(Event::TableGone);
                }
                Err(e) => {
                    result(Err(e), &mut out);
                }
            }
        }
        if !out.is_empty() {
            for e in out {
                if events.send(e).is_err() {
                    return;
                }
            }
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two players on a floor running (RONNIE_FLOOR_URL): online, an invitation, a match won.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn two_players_play_a_match() {
        let url = crate::floor::url().expect("RONNIE_FLOOR_URL");
        let ctx = egui::Context::default();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let account = |name: String| {
            let made: Value = ureq::post(format!("{url}/v1/players")).send_json(json!({ "name": name })).unwrap().body_mut().read_json().unwrap();
            FloorAccount { url: url.clone(), id: made["id"].as_str().unwrap().into(), token: made["token"].as_str().unwrap().into(), admin: false, avatar: None, name: Some(name) }
        };
        let (a, b) = (account(format!("Dio{stamp}")), account(format!("Ozzy{stamp}")));
        let (mut dio, mut ozzy) = (Live::default(), Live::default());
        let until = |what: &str, dio: &mut Live, ozzy: &mut Live, done: &dyn Fn(&Live, &Live) -> bool| {
            let start = Instant::now();
            while !done(dio, ozzy) {
                assert!(start.elapsed() < Duration::from_secs(20), "waiting for: {what}");
                dio.frame(&ctx, Some(&a));
                ozzy.frame(&ctx, Some(&b));
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        let ozzy_name = b.name.clone().unwrap();
        until("each other online", &mut dio, &mut ozzy, &|d, o| d.online.iter().any(|p| p.name == ozzy_name) && !o.online.is_empty());
        dio.invite(&ozzy_name);
        until("the invitation", &mut dio, &mut ozzy, &|_, o| !o.invites.is_empty());
        let id = ozzy.invites[0].id.clone();
        ozzy.accept(&id);
        until("the match, both sides", &mut dio, &mut ozzy, &|d, o| d.game.is_some() && o.game.is_some());
        assert_eq!((dio.game.as_ref().unwrap().you, ozzy.game.as_ref().unwrap().you), (1, 2));
        // Dio lines up four at the bottom, Ozzy plays above.
        for c in 0..4 {
            let moves = 2 * c as u32;
            until("Dio's turn", &mut dio, &mut ozzy, &|d, _| d.game.as_ref().is_some_and(|g| g.turn == 1 && g.moves == moves));
            dio.play(c);
            if c < 3 {
                until("Ozzy's turn", &mut dio, &mut ozzy, &|_, o| o.game.as_ref().is_some_and(|g| g.turn == 2 && g.moves == moves + 1));
                ozzy.play(c);
            }
        }
        until("the end", &mut dio, &mut ozzy, &|d, o| d.game.as_ref().is_some_and(|g| g.over) && o.game.as_ref().is_some_and(|g| g.over));
        assert_eq!(ozzy.game.as_ref().unwrap().winner, Some(1));
        assert_eq!(ozzy.game.as_ref().unwrap().line.as_ref().map(Vec::len), Some(4));
        // A rematch: Ozzy starts it.
        dio.rematch();
        ozzy.rematch();
        until("the rematch", &mut dio, &mut ozzy, &|d, o| d.game.as_ref().is_some_and(|g| !g.over) && o.game.as_ref().is_some_and(|g| !g.over));
        assert_eq!(ozzy.game.as_ref().unwrap().you, 1);
        ozzy.leave();
        until("the forfeit", &mut dio, &mut ozzy, &|d, _| d.game.as_ref().is_some_and(|g| g.over && g.reason.as_deref() == Some("forfeit")));
    }

    /// A match of Larsen on a floor running (RONNIE_FLOOR_URL): the gear set up, shots in turn, all the
    /// gear fried wins; neither ever sees the other's gear standing.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn two_players_play_larsen() {
        let url = crate::floor::url().expect("RONNIE_FLOOR_URL");
        let ctx = egui::Context::default();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let account = |name: String| {
            let made: Value = ureq::post(format!("{url}/v1/players")).send_json(json!({ "name": name })).unwrap().body_mut().read_json().unwrap();
            FloorAccount { url: url.clone(), id: made["id"].as_str().unwrap().into(), token: made["token"].as_str().unwrap().into(), admin: false, avatar: None, name: Some(name) }
        };
        let (a, b) = (account(format!("Lars{stamp}")), account(format!("Kirk{stamp}")));
        let (mut lars, mut kirk) = (Live::default(), Live::default());
        let until = |what: &str, lars: &mut Live, kirk: &mut Live, done: &dyn Fn(&Live, &Live) -> bool| {
            let start = Instant::now();
            while !done(lars, kirk) {
                assert!(start.elapsed() < Duration::from_secs(20), "waiting for: {what}");
                lars.frame(&ctx, Some(&a));
                kirk.frame(&ctx, Some(&b));
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        let kirk_name = b.name.clone().unwrap();
        until("each other online", &mut lars, &mut kirk, &|l, k| l.online.iter().any(|p| p.name == kirk_name) && !k.online.is_empty());
        lars.invite_to(&kirk_name, "larsen");
        until("the invitation", &mut lars, &mut kirk, &|_, k| !k.invites.is_empty());
        assert_eq!(kirk.invites[0].game.as_deref(), Some("larsen"));
        let id = kirk.invites[0].id.clone();
        kirk.accept(&id);
        until("the match, both sides", &mut lars, &mut kirk, &|l, k| l.game.as_ref().is_some_and(Match::larsen) && k.game.as_ref().is_some_and(Match::larsen));
        let gear: Vec<Piece> = [(0, 0, 5), (0, 2, 4), (0, 4, 3), (0, 6, 3), (0, 8, 2)].map(|(x, y, length)| Piece { x, y, length, horizontal: true, fried: false }).to_vec();
        lars.gear(gear.clone());
        kirk.gear(gear.clone());
        let placed = |g: &Option<Match>| g.as_ref().and_then(|g| g.stage.as_ref()).is_some_and(|n| n.placed.you && n.placed.them);
        until("both gears set up", &mut lars, &mut kirk, &|l, k| placed(&l.game) && placed(&k.game));
        // Kirk sees nothing of Lars's gear.
        let stage = kirk.game.as_ref().unwrap().stage.clone().unwrap();
        assert_eq!((stage.fried.len(), stage.fired.len(), stage.gear.as_ref().map(Vec::len)), (0, 0, Some(5)));
        // Lars fries it all, Kirk falls in silence meanwhile.
        let targets: Vec<(u8, u8)> = gear.iter().flat_map(|s| s.cells().collect::<Vec<_>>()).collect();
        for (k, &(x, y)) in targets.iter().enumerate() {
            let shots = 2 * k as u32;
            until("Lars's turn", &mut lars, &mut kirk, &|l, _| l.game.as_ref().is_some_and(|g| g.turn == 1 && g.moves == shots));
            lars.shoot(x, y);
            if k + 1 < targets.len() {
                until("Kirk's turn", &mut lars, &mut kirk, &|_, k2| k2.game.as_ref().is_some_and(|g| g.turn == 2 && g.moves == shots + 1));
                kirk.shoot(6 + k as u8 / 10, k as u8 % 10);
            }
        }
        until("the end", &mut lars, &mut kirk, &|l, k| l.game.as_ref().is_some_and(|g| g.over) && k.game.as_ref().is_some_and(|g| g.over));
        let end = kirk.game.clone().unwrap();
        assert_eq!((end.winner, end.reason.as_deref()), (Some(1), Some("fried")));
        let seen = end.stage.unwrap();
        assert!(seen.gear.unwrap().iter().all(|s| s.fried));
        assert_eq!((seen.standing.you, seen.received.len()), (0, 17));
        let won = lars.game.clone().unwrap().stage.unwrap();
        assert_eq!((won.fried.len(), won.fired.iter().filter(|s| s.2).count()), (5, 17));
    }

    /// Three players at a table of the Président, on a floor running (RONNIE_FLOOR_URL): the host invites
    /// two, starts, and each one plays its weakest card that goes (or passes) until the round is over.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn three_players_play_president() {
        let url = crate::floor::url().expect("RONNIE_FLOOR_URL");
        let ctx = egui::Context::default();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let account = |name: String| {
            let made: Value = ureq::post(format!("{url}/v1/players")).send_json(json!({ "name": name })).unwrap().body_mut().read_json().unwrap();
            FloorAccount { url: url.clone(), id: made["id"].as_str().unwrap().into(), token: made["token"].as_str().unwrap().into(), admin: false, avatar: None, name: Some(name) }
        };
        let accounts = [account(format!("Angus{stamp}")), account(format!("Bon{stamp}")), account(format!("Cliff{stamp}"))];
        let mut lives: [Live; 3] = Default::default();
        let until = |what: &str, lives: &mut [Live; 3], done: &dyn Fn(&[Live; 3]) -> bool| {
            let start = Instant::now();
            while !done(lives) {
                assert!(start.elapsed() < Duration::from_secs(30), "waiting for: {what}");
                for (live, a) in lives.iter_mut().zip(&accounts) {
                    live.frame(&ctx, Some(a));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        let names: Vec<String> = accounts.iter().map(|a| a.name.clone().unwrap()).collect();
        until("all online", &mut lives, &|l| l[0].online.len() >= 2 && names[1..].iter().all(|n| l[0].online.iter().any(|p| &p.name == n)));
        lives[0].invite_to(&names[1], "president");
        lives[0].invite_to(&names[2], "president");
        until("the invitations", &mut lives, &|l| !l[1].invites.is_empty() && !l[2].invites.is_empty());
        for k in [1, 2] {
            assert_eq!(lives[k].invites[0].game.as_deref(), Some("president"));
            let id = lives[k].invites[0].id.clone();
            lives[k].accept(&id);
        }
        until("three at the table", &mut lives, &|l| l.iter().all(|x| x.table.as_ref().is_some_and(|t| t.seats.len() == 3)));
        assert!(lives[0].table.as_ref().unwrap().hosting());
        lives[0].table_start();
        until("the game, everywhere", &mut lives, &|l| l.iter().all(|x| x.table.as_ref().is_some_and(|t| t.status == "playing" && t.hand.len() >= 17)));
        // Each one plays its weakest that goes, or passes, on its turn.
        let mut moves = 0;
        loop {
            until("a turn", &mut lives, &|l| {
                let t = l[0].table.as_ref().unwrap();
                t.status != "playing" || (t.moves >= moves && t.turn.is_some_and(|p| l[p].table.as_ref().is_some_and(|m| m.moves == t.moves)))
            });
            let t = lives[0].table.clone().unwrap();
            if t.status != "playing" {
                break;
            }
            moves = t.moves + 1;
            let p = t.turn.unwrap();
            let mine = lives[p].table.clone().unwrap();
            let need = mine.top.map_or(1, |top| top.count as usize);
            let pick = (0..13u8).find_map(|rank| {
                let same: Vec<[u8; 2]> = mine.hand.iter().copied().filter(|c| c[0] == rank).collect();
                (same.len() >= need && mine.top.is_none_or(|top| rank > top.rank)).then(|| same[..need].to_vec())
            });
            match pick {
                Some(cards) => lives[p].table_play(cards),
                None => lives[p].table_pass(),
            }
            until("the move taken", &mut lives, &|l| l[0].table.as_ref().is_some_and(|t| t.moves >= moves));
        }
        let end = lives[1].table.clone().unwrap();
        assert_eq!((end.status.as_str(), end.out.len(), end.presidencies.iter().sum::<u32>()), ("over", 3, 1));
    }
}
