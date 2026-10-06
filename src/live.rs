//! The game for two, on floor: who is online, the invitations, the Puissance 4 match played. Only with
//! a pseudo (see floor.rs).
//!
//! A thread talks to floor while the app runs: a sign of life every few seconds (the player is online
//! while it comes, and learns who else is, and the invitations received), every second while a match
//! is watched. The app sends it what the player does, and takes what floor answered at each frame.

use std::collections::HashSet;
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
    /// "puissance-4" or "ronnie-io".
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

/// A match, as floor sees it for this player.
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Match {
    pub id: String,
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
    /// "line", "draw" or "forfeit".
    pub reason: Option<String>,
    pub line: Option<Vec<[usize; 2]>>,
    pub rematch: Rematch,
    /// The rematch, once both asked.
    pub next: Option<String>,
}

impl Match {
    /// The other player's name.
    pub fn them(&self) -> Option<&str> {
        self.players.get(2 - self.you as usize).and_then(|n| n.as_deref())
    }
}

#[derive(Deserialize)]
struct Ping {
    online: Vec<Online>,
    invites: Vec<Invite>,
    sent: Option<Sent>,
    #[serde(rename = "match")]
    match_id: Option<String>,
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
    /// The match to fetch every second (None: none, out of the page).
    Watch(Option<String>),
    Board,
}

enum Event {
    Ping(Ping),
    Match(Match),
    Board(Vec<(String, u32)>),
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
    /// The board of online wins (name, wins), when fetched.
    pub board: Option<Vec<(String, u32)>>,
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
                }
                // The match watched (or the rematch, which the thread watches in its turn).
                Event::Match(m) => {
                    if self.game.as_ref().is_none_or(|g| g.id == m.id || !m.over) {
                        self.game = Some(m);
                    }
                }
                Event::Board(b) => self.board = Some(b),
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

    /// To a game: "puissance-4" or "ronnie-io".
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
        if game.over || game.turn != game.you || game.columns.get(c).is_none_or(|col| col.len() >= crate::connect4::ROWS) {
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

    /// The match shown in the page (None: the page left it).
    pub fn watch(&mut self, id: Option<String>) {
        if id.is_none() {
            self.game = None;
        }
        self.send(Cmd::Watch(id));
    }

    /// The board of wins, fetched again.
    pub fn fetch_board(&mut self) {
        self.send(Cmd::Board);
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
                if let Some(m) = result(call(&agent, "POST", format!("{base}/invites/{id}/accept"), token, None), &mut out).and_then(|v| serde_json::from_value::<Match>(v).ok()) {
                    watched = Some(m.id.clone());
                    out.push(Event::Match(m));
                }
                last_ping = f64::NEG_INFINITY;
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
            Some(Cmd::Watch(id)) => {
                watched = id;
                last_match = f64::NEG_INFINITY;
            }
            Some(Cmd::Board) => {
                #[derive(Deserialize)]
                struct Row {
                    name: Option<String>,
                    score: u32,
                }
                #[derive(Deserialize)]
                struct Board {
                    entries: Vec<Row>,
                }
                let board = agent.get(format!("{url}/v1/games/puissance-4/leaderboard")).query("limit", "10").call().ok().and_then(|mut r| r.body_mut().read_json::<Board>().ok());
                if let Some(b) = board {
                    out.push(Event::Board(b.entries.into_iter().map(|r| (r.name.unwrap_or_default(), r.score)).collect()));
                }
            }
            None => {}
        }
        if now() - last_ping >= if busy || watched.is_some() { PING_BUSY } else { PING_IDLE } {
            last_ping = now();
            if let Some(p) = result(call(&agent, "POST", format!("{base}/ping"), token, None), &mut out).and_then(|v| serde_json::from_value::<Ping>(v).ok()) {
                busy = !p.invites.is_empty() || p.match_id.is_some() || p.sent.as_ref().is_some_and(|s| s.status == "pending");
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
            FloorAccount { url: url.clone(), id: made["id"].as_str().unwrap().into(), token: made["token"].as_str().unwrap().into(), name: Some(name) }
        };
        let (a, b) = (account(format!("Dio{stamp}")), account(format!("Ozzy{stamp}")));
        let (mut dio, mut ozzy) = (Live::default(), Live::default());
        let mut until = |what: &str, dio: &mut Live, ozzy: &mut Live, done: &dyn Fn(&Live, &Live) -> bool| {
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
}
