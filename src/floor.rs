//! floor: the server of the games' leaderboards (its own repository, next to Ronnie's). It keeps the
//! best round of each player; the typing game shows its board, and its own rounds when it can't reach it.
//!
//! The work is a sync, in the background, after which the board and the account are up to date: the
//! board is fetched, and the round just finished sent if it beats the player's line on floor, as the
//! player's name if it changed. The account is made on the first round sent.
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

/// The name as floor keeps it: its spaces folded, cut to its length (None: no name).
fn floor_name(name: &str) -> Option<String> {
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

    /// A sync, if the last one is older than `every` seconds (None: now).
    pub fn sync(&mut self, ctx: &egui::Context, now: f64, every: Option<f64>, account: Option<&FloorAccount>, name: &str) {
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
        let account = account.filter(|a| a.url == url).cloned();
        let (name, round, shared, ctx) = (floor_name(name), self.pending.take(), self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let synced = sync(&url, account, name, round.clone()).unwrap_or_else(|e| {
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

fn sync(url: &str, mut account: Option<FloorAccount>, name: Option<String>, round: Option<TypingScore>) -> Result<Synced, ureq::Error> {
    let agent = agent();
    let mut synced = Synced::default();
    let (mut entries, me) = board(&agent, url, account.as_ref())?;
    // An account floor doesn't know (its base started over): a new one.
    if account.is_some() && me.is_none() && round.is_some() {
        let known = agent.patch(format!("{url}/v1/players/me")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", account.as_ref().map_or("", |a| &a.token))).send_json(serde_json::json!({ "name": name }));
        if matches!(known, Err(ureq::Error::StatusCode(401))) {
            account = None;
            synced.account = Some(None);
        }
    }
    let mut changed = false;
    // The round, if it beats the player's line (floor keeps the best one anyway).
    if let Some(best) = round.filter(|r| me.as_ref().is_none_or(|m| r.letters > m.score)) {
        let account = match &account {
            Some(a) => a.clone(),
            None => {
                let made: Created = agent.post(format!("{url}/v1/players")).header("User-Agent", USER_AGENT).send_json(serde_json::json!({ "name": name }))?.body_mut().read_json()?;
                let made = FloorAccount { url: url.to_owned(), id: made.id, token: made.token };
                synced.account = Some(Some(made.clone()));
                account = Some(made.clone());
                made
            }
        };
        let details = Details { letters: best.letters, words: best.words, wpm: best.wpm, accuracy: best.accuracy, combo: best.combo };
        agent.post(format!("{url}/v1/games/{GAME}/scores")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", account.token)).send_json(&details)?;
        changed = true;
    } else if let (Some(a), Some(m)) = (&account, &me) {
        // The name changed since.
        if m.name != name {
            agent.patch(format!("{url}/v1/players/me")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", a.token)).send_json(serde_json::json!({ "name": name }))?;
            changed = true;
        }
    }
    if changed {
        entries = board(&agent, url, account.as_ref())?.0;
    }
    synced.board = Some(entries);
    Ok(synced)
}

/// The board to show: floor's when it answered, with the player's line taken from the rounds played here
/// (the best one, if floor doesn't have it yet, and the name as it is now); else the rounds played here.
pub fn shown(floor: Option<&[Entry]>, local: &[TypingScore], name: &str) -> Vec<Entry> {
    let name = floor_name(name);
    let Some(floor) = floor else {
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
        let rows = shown(Some(&floor), &[round(400, 7)], "  Ronnie  James ");
        let names: Vec<_> = rows.iter().map(|r| (r.name.clone().unwrap(), r.score.letters)).collect();
        assert_eq!(names, [("Dio".into(), 500), ("Ronnie James".into(), 400), ("Ozzy".into(), 200)]);
        assert_eq!(rows[1].score.at, 7);
        // Not better than floor's: floor's line stays, renamed.
        let rows = shown(Some(&floor), &[round(100, 7)], "");
        assert_eq!(rows[1].score.letters, 300);
        assert_eq!(rows[1].name, None);
    }

    #[test]
    fn offline_the_rounds_played_here() {
        let rows = shown(None, &[round(400, 1), round(300, 2)], "Dio");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.me && r.name.as_deref() == Some("Dio")));
    }

    #[test]
    fn names_as_floor_keeps_them() {
        assert_eq!(floor_name("   "), None);
        assert_eq!(floor_name(&"x".repeat(30)).unwrap().len(), NAME_MAX);
    }
}
