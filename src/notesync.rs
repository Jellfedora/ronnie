//! The notes kept on floor with the account (`/v1/notes`), when the player chose to (off at first),
//! so that every machine signed in to it has the same notes.
//!
//! Each note and folder travels on its own, with the date of its last change: the most recent wins,
//! on floor as here. A note deleted for good leaves a mark (its id, when) so that the others delete
//! it too. This machine remembers, per account, floor's number for the last change it saw (only what
//! changed since is asked) and the date of each item as floor has it (what changed here since is sent).
//! The files the notes cite (`notes-files/…`) go up and come down by their name, which never changes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::FloorAccount;
use crate::floor::{account_agent, url, USER_AGENT};

/// Sent in batches of so many at most (floor takes 500).
const BATCH: usize = 200;
/// And of so many bytes at most (floor takes 8 MB).
const BATCH_BYTES: usize = 4 * 1024 * 1024;
/// floor asked again this often, in case another machine changed a note.
const EVERY: f64 = 60.0;
/// A change here sent after this long (the typing goes on meanwhile).
const AFTER_CHANGE: f64 = 5.0;

/// A note or a folder as it travels (`data` None: deleted for good).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: String,
    pub id: String,
    pub data: Option<Value>,
    pub updated: i64,
}

/// What this machine remembers of its last sync with an account (in notes-sync.json).
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct State {
    account: String,
    /// floor's number for the last change seen.
    seq: i64,
    /// Each item's date as floor has it.
    known: HashMap<String, i64>,
    /// Files floor refused (their type): not sent again.
    refused: HashSet<String>,
}

fn state_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("notes-sync.json"))
}

fn load_state(account: &str) -> State {
    let state: State = state_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    // Another account: everything from the start.
    if state.account == account { state } else { State { account: account.to_owned(), ..State::default() } }
}

fn save_state(state: &State) {
    if let (Some(path), Ok(bytes)) = (state_path(), serde_json::to_vec(state)) {
        let _ = std::fs::write(path, bytes);
    }
}

/// A sync ended.
pub struct Synced {
    /// Items from floor, to merge (the most recent wins).
    pub items: Vec<Item>,
    /// Files come down: the images showing them to read again.
    pub files: bool,
}

#[derive(Default)]
struct Shared {
    busy: bool,
    done: Option<Synced>,
    /// The last sync's error (None: it went well).
    error: Option<String>,
    /// When the last one went well (seconds since 1970).
    at: Option<i64>,
}

/// Kept by the app: the sync running, when the last one was.
#[derive(Default)]
pub struct NoteSync {
    shared: Arc<Mutex<Shared>>,
    last: Option<f64>,
    /// When the notes changed here and weren't sent yet.
    changed: Option<f64>,
    focused: bool,
}

impl NoteSync {
    pub fn poll(&mut self) -> Option<Synced> {
        self.shared.lock().ok()?.done.take()
    }

    /// The notes changed here (saved): sent in a moment.
    pub fn touched(&mut self, now: f64) {
        self.changed.get_or_insert(now);
    }

    /// The last sync: when it went well, its error.
    pub fn status(&self) -> (Option<i64>, Option<String>, bool) {
        self.shared.lock().map(|s| (s.at, s.error.clone(), s.busy)).unwrap_or_default()
    }

    /// Asked now (the switch just turned on).
    pub fn soon(&mut self) {
        self.last = None;
    }

    /// Whether to sync now: the first time, the window back in front, a change made here a moment
    /// ago, or `EVERY` seconds since the last.
    pub fn due(&mut self, now: f64, focused: bool) -> bool {
        if self.shared.lock().map_or(true, |s| s.busy) {
            return false;
        }
        let back = focused && !std::mem::replace(&mut self.focused, focused);
        let failed = self.shared.lock().is_ok_and(|s| s.error.is_some());
        match self.last {
            None => true,
            Some(last) if now - last >= EVERY => true,
            // floor out of reach: not asked again before `EVERY`.
            _ if failed => false,
            _ => back || self.changed.is_some_and(|at| now - at >= AFTER_CHANGE),
        }
    }

    /// The local items sent and floor's taken, in the background; `cited`: the files the notes cite.
    pub fn sync(&mut self, ctx: &egui::Context, now: f64, account: &FloorAccount, local: Vec<Item>, cited: HashSet<String>) {
        let Some(url) = url().filter(|u| *u == account.url) else { return };
        {
            let Ok(mut shared) = self.shared.lock() else { return };
            if shared.busy {
                return;
            }
            shared.busy = true;
        }
        self.last = Some(now);
        self.changed = None;
        let (shared, ctx, token, id) = (self.shared.clone(), ctx.clone(), account.token.clone(), account.id.clone());
        std::thread::spawn(move || {
            let mut state = load_state(&id);
            let done = exchange(&url, &token, &mut state, local, cited);
            save_state(&state);
            if let Ok(mut shared) = shared.lock() {
                match done {
                    Ok(done) => {
                        shared.done = Some(done);
                        shared.error = None;
                        shared.at = Some(chrono::Utc::now().timestamp());
                    }
                    Err(e) => {
                        crate::log::info(&format!("notes sync: {e}"));
                        shared.error = Some(e);
                    }
                }
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }
}

/// The items changed here since floor last had them.
fn to_send<'a>(state: &State, local: &'a [Item]) -> Vec<&'a Item> {
    local.iter().filter(|i| state.known.get(&i.id).is_none_or(|&k| i.updated > k)).collect()
}

/// The files cited in a note's text (`notes-files/<name>`).
pub fn cited_in(text: &str, out: &mut HashSet<String>) {
    let mut rest = text;
    while let Some(at) = rest.find("notes-files/") {
        rest = &rest[at + "notes-files/".len()..];
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
        if !name.is_empty() {
            out.insert(name);
        }
    }
}

fn exchange(url: &str, token: &str, state: &mut State, local: Vec<Item>, mut cited: HashSet<String>) -> Result<Synced, String> {
    let agent = account_agent();
    let auth = format!("Bearer {token}");
    let check = |status: u16| if (200..300).contains(&status) { Ok(()) } else { Err(format!("floor answered {status}")) };

    // What changed here, in batches.
    let send = to_send(state, &local);
    let mut batch: Vec<&Item> = Vec::new();
    let mut bytes = 0;
    let flush = |batch: &mut Vec<&Item>, state: &mut State| -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let res = agent.post(format!("{url}/v1/notes/items")).header("User-Agent", USER_AGENT).header("Authorization", &auth).send_json(serde_json::json!({ "items": batch })).map_err(|e| e.to_string())?;
        check(res.status().as_u16())?;
        for i in batch.drain(..) {
            let k = state.known.entry(i.id.clone()).or_insert(i.updated);
            *k = (*k).max(i.updated);
        }
        Ok(())
    };
    for item in send {
        let size = item.data.as_ref().map_or(64, |d| d.to_string().len());
        if !batch.is_empty() && (batch.len() >= BATCH || bytes + size > BATCH_BYTES) {
            flush(&mut batch, state)?;
            bytes = 0;
        }
        bytes += size;
        batch.push(item);
    }
    flush(&mut batch, state)?;

    // What changed on floor (one's own just sent among it: the same date, nothing to do).
    #[derive(Deserialize)]
    struct Page {
        items: Vec<Item>,
        seq: i64,
        more: bool,
    }
    let mut items = Vec::new();
    loop {
        let mut res = agent.get(format!("{url}/v1/notes/items")).query("since", state.seq.to_string()).header("User-Agent", USER_AGENT).header("Authorization", &auth).call().map_err(|e| e.to_string())?;
        check(res.status().as_u16())?;
        let page: Page = res.body_mut().with_config().limit(64 * 1024 * 1024).read_json().map_err(|e| e.to_string())?;
        for i in &page.items {
            let k = state.known.entry(i.id.clone()).or_insert(i.updated);
            *k = (*k).max(i.updated);
            if let Some(text) = i.data.as_ref().and_then(|d| d.get("text")).and_then(Value::as_str) {
                cited_in(text, &mut cited);
            }
        }
        state.seq = page.seq;
        items.extend(page.items);
        if !page.more {
            break;
        }
    }

    // The files: those floor lacks go up, those this machine lacks come down.
    let dir = crate::config::config_dir().ok_or("no config folder")?.join("notes-files");
    #[derive(Deserialize)]
    struct Files {
        files: Vec<String>,
    }
    let mut res = agent.get(format!("{url}/v1/notes/files")).header("User-Agent", USER_AGENT).header("Authorization", &auth).call().map_err(|e| e.to_string())?;
    check(res.status().as_u16())?;
    let there: HashSet<String> = res.body_mut().read_json::<Files>().map_err(|e| e.to_string())?.files.into_iter().collect();
    let mut came = false;
    for name in &cited {
        let path = dir.join(name);
        if path.is_file() {
            if !there.contains(name) && !state.refused.contains(name) {
                let Ok(body) = std::fs::read(&path) else { continue };
                let res = agent.put(format!("{url}/v1/notes/files/{name}")).header("User-Agent", USER_AGENT).header("Authorization", &auth).header("Content-Type", "application/octet-stream").send(&body[..]).map_err(|e| e.to_string())?;
                match res.status().as_u16() {
                    200..=299 => {}
                    // Not a type floor keeps, or too big: stays on this machine.
                    400 | 413 | 415 => {
                        state.refused.insert(name.clone());
                    }
                    status => return Err(format!("floor answered {status}")),
                }
            }
        } else if there.contains(name) {
            let mut res = agent.get(format!("{url}/v1/notes/files/{name}")).header("User-Agent", USER_AGENT).header("Authorization", &auth).call().map_err(|e| e.to_string())?;
            check(res.status().as_u16())?;
            let body = res.body_mut().with_config().limit(32 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            // Written aside, then moved: never half a file under its name.
            let part = dir.join(format!(".{name}.part"));
            std::fs::write(&part, body).map_err(|e| e.to_string())?;
            std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
            came = true;
        }
    }
    Ok(Synced { items, files: came })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sent() {
        let item = |id: &str, updated| Item { kind: "note".into(), id: id.into(), data: None, updated };
        let state = State { known: HashMap::from([("a".into(), 10), ("b".into(), 10)]), ..State::default() };
        let local = [item("a", 10), item("b", 11), item("c", 1)];
        assert_eq!(to_send(&state, &local).iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    }

    #[test]
    fn files_cited() {
        let mut out = HashSet::new();
        cited_in("![x](notes-files/ab12cd34-image.png) et [doc](notes-files/ff00ee11-rapport.pdf).", &mut out);
        assert_eq!(out, HashSet::from(["ab12cd34-image.png".to_owned(), "ff00ee11-rapport.pdf".to_owned()]));
    }
}

/// Two machines on a floor running (RONNIE_FLOOR_URL, RONNIE_TEST_TOKEN: an account's session):
/// a note and its image written on one, found on the other.
#[cfg(test)]
#[test]
#[ignore]
fn two_machines() {
    let (Ok(url), Ok(token)) = (std::env::var("RONNIE_FLOOR_URL"), std::env::var("RONNIE_TEST_TOKEN")) else { return };
    let png = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 7, 7];
    let (one, two) = (std::env::temp_dir().join(format!("ns-one-{}", std::process::id())), std::env::temp_dir().join(format!("ns-two-{}", std::process::id())));
    std::fs::create_dir_all(one.join("notes-files")).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(one.join("notes-files/ab12cd34-image.png"), png).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let item = Item { kind: "note".into(), id: id.clone(), data: Some(serde_json::json!({ "id": id, "text": "Salut ![i](notes-files/ab12cd34-image.png)", "created": 1, "updated": 5 })), updated: 5 };
    // SAFETY: this test alone sets it, run on its own.
    unsafe { std::env::set_var("RONNIE_CONFIG_DIR", &one) };
    let mut cited = HashSet::new();
    cited_in("notes-files/ab12cd34-image.png", &mut cited);
    let mut state = State::default();
    exchange(&url, &token, &mut state, vec![item.clone()], cited).unwrap();
    assert!(to_send(&state, std::slice::from_ref(&item)).is_empty(), "sent: not again");
    unsafe { std::env::set_var("RONNIE_CONFIG_DIR", &two) };
    let mut state = State::default();
    let done = exchange(&url, &token, &mut state, Vec::new(), HashSet::new()).unwrap();
    assert!(done.items.iter().any(|i| i.id == id));
    assert!(done.files);
    assert_eq!(std::fs::read(two.join("notes-files/ab12cd34-image.png")).unwrap(), png);
}
