//! floor: the server of the games' leaderboards (its own repository, next to Ronnie's). It keeps the
//! best round of each player; the typing game shows its board, and its own rounds when it can't reach it.
//!
//! The account is made when the player picks a pseudo (a claim): floor keeps each pseudo for one player.
//! Without one, nothing goes to floor: the rounds stay here.
//!
//! The work is a sync, in the background, after which the board and the account are up to date: the
//! board is fetched, and the round just finished sent if it beats the player's line on floor.
//!
//! Only a round ended in the game is sent, never the scores kept in the settings (a file anyone can
//! edit). One floor couldn't take is tried again at the next syncs, while the app runs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{FloorAccount, TypingScore};

const GAME: &str = "speed-metal";
/// Places shown on the board.
pub const BOARD: usize = 10;
/// The names floor takes, at most.
const NAME_MAX: usize = 24;

/// Where floor runs (on Beemo).
const URL: &str = "https://fedoheim.hopto.org/floor";

/// floor's address: RONNIE_FLOOR_URL when given (at run time, or when built); a dev build, a local floor.
pub fn url() -> Option<String> {
    let url = std::env::var("RONNIE_FLOOR_URL").ok().or_else(|| option_env!("RONNIE_FLOOR_URL").map(str::to_owned));
    let url = url.unwrap_or_else(|| if crate::config::OFFICIAL { URL } else { "http://localhost:5002" }.to_owned());
    let url = url.trim().trim_end_matches('/');
    (!url.is_empty()).then(|| url.to_owned())
}

/// A line of the board.
#[derive(Clone, Debug)]
pub struct Entry {
    /// None: the player gave none.
    pub name: Option<String>,
    pub score: TypingScore,
    /// The line of this player.
    pub me: bool,
}

#[derive(Deserialize)]
struct Board {
    entries: Vec<WireEntry>,
}

#[derive(Deserialize)]
struct WireEntry {
    name: Option<String>,
    score: u32,
    details: Details,
    /// Milliseconds since 1970.
    at: i64,
    me: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Details {
    letters: u32,
    words: u32,
    wpm: u32,
    accuracy: u32,
    combo: u32,
}

#[derive(Deserialize)]
struct Created {
    id: String,
    token: String,
}

/// The pseudo of the account, when it was made on this floor: without one, no online scores.
pub fn pseudo(account: Option<&FloorAccount>) -> Option<&str> {
    let url = url()?;
    account.filter(|a| a.url == url)?.name.as_deref()
}

/// Why a pseudo wasn't taken.
#[derive(Clone, Debug, PartialEq)]
pub enum ClaimError {
    /// Another player's.
    Taken,
    /// Refused by floor (its message).
    Refused(String),
    /// floor didn't answer.
    Unreachable,
}

/// The name as floor keeps it: its spaces folded, cut to its length (None: no name).
pub fn floor_name(name: &str) -> Option<String> {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    let name = name.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// What a sync brings back.
#[derive(Default)]
struct Synced {
    board: Option<Vec<Entry>>,
    /// The account changed (made, or forgotten by floor): to save.
    account: Option<Option<FloorAccount>>,
    /// The round to send, not sent (floor unreachable): tried again.
    unsent: Option<TypingScore>,
}

#[derive(Default)]
struct Shared {
    busy: bool,
    /// A sync asked while one runs: another one after it.
    again: bool,
    done: Option<Synced>,
    /// The pseudo typed last asked about, and floor's answer once it came: free or not (None: floor
    /// didn't answer).
    checked: Option<(String, Option<Option<bool>>)>,
    /// A pseudo being claimed, then floor's answer.
    claiming: bool,
    claimed: Option<Result<FloorAccount, ClaimError>>,
}

/// Kept by the app: the last board, the sync running.
#[derive(Default)]
pub struct Floor {
    shared: Arc<Mutex<Shared>>,
    board: Option<Vec<Entry>>,
    /// When the last sync started (app time): the board is fetched again a minute later.
    last: Option<f64>,
    /// A round finished, not sent yet (kept in memory only).
    pending: Option<TypingScore>,
}

impl Floor {
    /// floor's board when it answered, the best first.
    pub fn board(&self) -> Option<&[Entry]> {
        self.board.as_deref()
    }

    /// Takes what a sync brought back: the account, when it changed (to save).
    pub fn poll(&mut self) -> Option<Option<FloorAccount>> {
        let mut shared = self.shared.lock().ok()?;
        let done = shared.done.take()?;
        // Asked during the sync: another one, with what is known now, at the next frame.
        if std::mem::take(&mut shared.again) {
            self.last = None;
        }
        drop(shared);
        if let Some(round) = done.unsent {
            self.keep(round);
        }
        if done.board.is_some() {
            self.board = done.board;
        }
        done.account
    }

    /// A round just finished in the game: sent at the next sync (the best one, when several wait).
    pub fn finished(&mut self, round: TypingScore) {
        self.keep(round);
    }

    fn keep(&mut self, round: TypingScore) {
        if self.pending.as_ref().is_none_or(|p| round.letters > p.letters) {
            self.pending = Some(round);
        }
    }

    /// The pseudo asked to floor, in the background: the account renamed (its scores kept), or made.
    pub fn claim(&mut self, ctx: &egui::Context, account: Option<&FloorAccount>, name: &str) {
        let Some(url) = url() else { return };
        let Some(name) = floor_name(name) else { return };
        {
            let Ok(mut shared) = self.shared.lock() else { return };
            if shared.claiming {
                return;
            }
            shared.claiming = true;
            shared.claimed = None;
        }
        let account = account.filter(|a| a.url == url).cloned();
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let claimed = claim(&url, account, &name);
            if let Ok(mut shared) = shared.lock() {
                shared.claimed = Some(claimed);
                shared.claiming = false;
            }
            // The board, with the new name.
            ctx.request_repaint();
        });
    }

    /// Whether the pseudo typed is free, asked in the background (the player's own is).
    pub fn check(&mut self, ctx: &egui::Context, account: Option<&FloorAccount>, name: &str) {
        let (Some(url), Some(name)) = (url(), floor_name(name)) else { return };
        let Ok(mut shared) = self.shared.lock() else { return };
        shared.checked = Some((name.clone(), None));
        drop(shared);
        let token = account.filter(|a| a.url == url).map(|a| a.token.clone());
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            #[derive(Deserialize)]
            struct Available {
                free: bool,
            }
            let mut request = agent().get(format!("{url}/v1/players/available")).query("name", &name).header("User-Agent", USER_AGENT);
            if let Some(token) = token {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            let free = request.call().ok().and_then(|mut r| r.body_mut().read_json::<Available>().ok()).map(|a| a.free);
            if let Ok(mut shared) = shared.lock() {
                // Only the last one asked (another may have been typed since).
                if let Some((asked, answer)) = &mut shared.checked {
                    if *asked == name {
                        *answer = Some(free);
                    }
                }
            }
            ctx.request_repaint();
        });
    }

    /// floor's answer for this pseudo, once it came: free or taken (None inside: floor didn't answer).
    pub fn checked(&self, name: &str) -> Option<Option<bool>> {
        let shared = self.shared.lock().ok()?;
        let (asked, answer) = shared.checked.as_ref()?;
        (floor_name(name).as_ref() == Some(asked)).then_some(*answer)?
    }

    /// A pseudo being claimed.
    pub fn claiming(&self) -> bool {
        self.shared.lock().is_ok_and(|s| s.claiming)
    }

    /// floor's answer to a claim: the account to save, or why not.
    pub fn claimed(&mut self) -> Option<Result<FloorAccount, ClaimError>> {
        let claimed = self.shared.lock().ok()?.claimed.take()?;
        if claimed.is_ok() {
            self.last = None;
        }
        Some(claimed)
    }

    /// A sync, if the last one is older than `every` seconds (None: now). Without a pseudo, only the
    /// board is fetched.
    pub fn sync(&mut self, ctx: &egui::Context, now: f64, every: Option<f64>, account: Option<&FloorAccount>) {
        if every.is_some_and(|every| self.last.is_some_and(|last| now - last < every)) {
            return;
        }
        let Some(url) = url() else { return };
        self.last = Some(now);
        {
            let Ok(mut shared) = self.shared.lock() else { return };
            if shared.busy {
                shared.again = true;
                return;
            }
            shared.busy = true;
        }
        // The account made for another floor (a dev build sharing the installed app's settings...) isn't used.
        let account = account.filter(|a| a.url == url && a.name.is_some()).cloned();
        let round = self.pending.take().filter(|_| account.is_some());
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let synced = sync(&url, account, round.clone()).unwrap_or_else(|e| {
                crate::log::info(&format!("floor: {e}"));
                // Refused by floor (out of its bounds): not tried again.
                let refused = matches!(e, ureq::Error::StatusCode(400..=499));
                Synced { unsent: round.filter(|_| !refused), ..Default::default() }
            });
            if let Ok(mut shared) = shared.lock() {
                shared.done = Some(synced);
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }
}

fn agent() -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(15))).build().new_agent()
}

const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));

fn board(agent: &ureq::Agent, url: &str, account: Option<&FloorAccount>) -> Result<(Vec<Entry>, Option<WireEntry>), ureq::Error> {
    #[derive(Deserialize)]
    struct Answer {
        #[serde(flatten)]
        board: Board,
        me: Option<WireEntry>,
    }
    let mut request = agent.get(format!("{url}/v1/games/{GAME}/leaderboard")).query("limit", BOARD.to_string()).header("User-Agent", USER_AGENT);
    if let Some(a) = account {
        request = request.header("Authorization", format!("Bearer {}", a.token));
    }
    let answer: Answer = request.call()?.body_mut().read_json()?;
    let entries = answer.board.entries.into_iter().map(entry).collect();
    Ok((entries, answer.me))
}

fn entry(e: WireEntry) -> Entry {
    let d = e.details;
    Entry { name: e.name, me: e.me, score: TypingScore { letters: e.score, words: d.words, wpm: d.wpm, accuracy: d.accuracy, combo: d.combo, at: e.at / 1000 } }
}

fn sync(url: &str, account: Option<FloorAccount>, round: Option<TypingScore>) -> Result<Synced, ureq::Error> {
    let agent = agent();
    let mut synced = Synced::default();
    let (mut entries, me) = board(&agent, url, account.as_ref())?;
    // The round, if it beats the player's line (floor keeps the best one anyway).
    if let (Some(account), Some(best)) = (&account, round.filter(|r| me.as_ref().is_none_or(|m| r.letters > m.score))) {
        let details = Details { letters: best.letters, words: best.words, wpm: best.wpm, accuracy: best.accuracy, combo: best.combo };
        match agent.post(format!("{url}/v1/games/{GAME}/scores")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", account.token)).send_json(&details) {
            Ok(_) => entries = board(&agent, url, Some(account))?.0,
            // An account floor doesn't know (its base started over): the pseudo is to pick again.
            Err(ureq::Error::StatusCode(401)) => synced.account = Some(None),
            Err(e) => return Err(e),
        }
    }
    synced.board = Some(entries);
    Ok(synced)
}

fn claim(url: &str, account: Option<FloorAccount>, name: &str) -> Result<FloorAccount, ClaimError> {
    let agent = agent();
    let body = serde_json::json!({ "name": name });
    let failed = |e: ureq::Error| {
        crate::log::info(&format!("floor: {e}"));
        match e {
            ureq::Error::StatusCode(409) => ClaimError::Taken,
            ureq::Error::StatusCode(code) if code < 500 => ClaimError::Refused(code.to_string()),
            _ => ClaimError::Unreachable,
        }
    };
    if let Some(mut a) = account {
        match agent.patch(format!("{url}/v1/players/me")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", a.token)).send_json(&body) {
            Ok(_) => {
                a.name = Some(name.to_owned());
                return Ok(a);
            }
            // Forgotten by floor: a new account.
            Err(ureq::Error::StatusCode(401)) => {}
            Err(e) => return Err(failed(e)),
        }
    }
    let made: Created = agent.post(format!("{url}/v1/players")).header("User-Agent", USER_AGENT).send_json(&body).map_err(failed)?.body_mut().read_json().map_err(failed)?;
    Ok(FloorAccount { url: url.to_owned(), id: made.id, token: made.token, name: Some(name.to_owned()) })
}

/// The board to show: floor's when it answered and the player has a pseudo, with the player's line
/// taken from the rounds played here (the best one, if floor doesn't have it yet); else the rounds
/// played here.
pub fn shown(floor: Option<&[Entry]>, local: &[TypingScore], pseudo: Option<&str>) -> Vec<Entry> {
    let name = pseudo.map(str::to_owned);
    let Some(floor) = floor.filter(|_| pseudo.is_some()) else {
        return local.iter().map(|s| Entry { name: name.clone(), score: s.clone(), me: true }).collect();
    };
    let mut rows: Vec<Entry> = floor.to_vec();
    if let Some(best) = local.first() {
        let mine = rows.iter().position(|r| r.me);
        if mine.is_none_or(|k| best.letters >= rows[k].score.letters) {
            if let Some(k) = mine {
                rows.remove(k);
            }
            // Below the rounds it ties with.
            let place = rows.iter().filter(|r| r.score.letters >= best.letters).count();
            rows.insert(place, Entry { name: name.clone(), score: best.clone(), me: true });
        }
    }
    for r in rows.iter_mut().filter(|r| r.me) {
        r.name = name.clone();
    }
    rows.truncate(BOARD);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(letters: u32, at: i64) -> TypingScore {
        TypingScore { letters, at, ..Default::default() }
    }

    fn row(name: &str, letters: u32, me: bool) -> Entry {
        Entry { name: Some(name.into()), score: round(letters, 0), me }
    }

    #[test]
    fn the_local_best_takes_the_players_line() {
        let floor = [row("Dio", 500, false), row("moi", 300, true), row("Ozzy", 200, false)];
        let rows = shown(Some(&floor), &[round(400, 7)], Some("Ronnie James"));
        let names: Vec<_> = rows.iter().map(|r| (r.name.clone().unwrap(), r.score.letters)).collect();
        assert_eq!(names, [("Dio".into(), 500), ("Ronnie James".into(), 400), ("Ozzy".into(), 200)]);
        assert_eq!(rows[1].score.at, 7);
        // Not better than floor's: floor's line stays, renamed.
        let rows = shown(Some(&floor), &[round(100, 7)], Some("Dio"));
        assert_eq!(rows[1].score.letters, 300);
        assert_eq!(rows[1].name.as_deref(), Some("Dio"));
    }

    #[test]
    fn offline_the_rounds_played_here() {
        let rows = shown(None, &[round(400, 1), round(300, 2)], Some("Dio"));
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.me && r.name.as_deref() == Some("Dio")));
        // Without a pseudo, the same, floor's board not shown.
        let floor = [row("Ozzy", 200, false)];
        let rows = shown(Some(&floor), &[round(400, 1)], None);
        assert!(rows.len() == 1 && rows[0].me && rows[0].name.is_none());
    }

    #[test]
    fn names_as_floor_keeps_them() {
        assert_eq!(floor_name("   "), None);
        assert_eq!(floor_name(&"x".repeat(30)).unwrap().len(), NAME_MAX);
    }
}
