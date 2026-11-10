//! The notes kept on floor with the account (`/v1/notes`), when the player chose to (off at first),
//! so that every machine signed in to it has the same notes, encrypted end to end.
//!
//! The key is drawn at random by the first machine; floor keeps it wrapped (sealed) with a key drawn
//! from the account's password (Argon2id, with a salt kept beside it), so that a new machine gets it
//! back with the password alone. floor checks the password before taking a wrap (a typo would lose
//! the key), and the key is wrapped again when the password changes. Each machine keeps the key
//! sealed as the SSH passwords are (`ssh::save_password`).
//! Each note and folder travels on its own, sealed (XChaCha20-Poly1305, bound to its kind and id),
//! with the date of its last change: the most recent wins, on floor as here. A note deleted for good
//! leaves a mark so that the others delete it too. The files the notes cite go sealed too, under
//! opaque names (an HMAC of their name), which floor alone sees, to know which are still cited.
//!
//! This machine remembers, per account, floor's number for the last change it saw (only what changed
//! since is asked) and the date of each item as floor has it (what changed here since is sent).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::XChaCha20Poly1305;
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
/// A file sealed starts so (then its nonce, then the sealed bytes).
const FILE_MAGIC: &[u8] = b"RNF1";
/// Files bigger than this stay on their machine (floor takes 10 MB).
const FILE_MAX: u64 = 10 * 1024 * 1024;
/// What the wrap is bound to.
const WRAP: &[u8] = b"ronnie-notes-key-v1";

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// A note or a folder, in clear (`data` None: deleted for good).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: String,
    pub id: String,
    pub data: Option<Value>,
    pub updated: i64,
}

/// An item as floor keeps it: its data sealed, the opaque names of the files it cites.
#[derive(Serialize, Deserialize, Debug)]
struct Wire {
    kind: String,
    id: String,
    data: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<String>,
    updated: i64,
}

/// The key the notes are sealed with.
#[derive(Clone)]
struct Key([u8; 32]);

impl Key {
    fn random() -> Key {
        use chacha20poly1305::aead::rand_core::RngCore;
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Key(key)
    }

    fn derive(phrase: &str, salt: &[u8]) -> Result<Key, String> {
        let mut key = [0u8; 32];
        argon2::Argon2::default().hash_password_into(phrase.as_bytes(), salt, &mut key).map_err(|e| e.to_string())?;
        Ok(Key(key))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(&self.0))
    }

    /// `plain` sealed, bound to `aad`: nonce then sealed bytes.
    fn seal(&self, plain: &[u8], aad: &[u8]) -> Vec<u8> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = self.cipher().encrypt(&nonce, Payload { msg: plain, aad }).expect("sealing in memory");
        [nonce.as_slice(), &sealed].concat()
    }

    fn open(&self, bytes: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
        let (nonce, sealed) = bytes.split_at_checked(24)?;
        self.cipher().decrypt(chacha20poly1305::XNonce::from_slice(nonce), Payload { msg: sealed, aad }).ok()
    }

    /// A file's name as floor sees it.
    fn opaque(&self, name: &str) -> String {
        use hmac::Mac;
        let mut mac = <hmac::Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(&self.0).expect("any key length");
        mac.update(b"ronnie-notes-file:");
        mac.update(name.as_bytes());
        mac.finalize().into_bytes()[..16].iter().map(|b| format!("{b:02x}")).collect()
    }

    fn seal_item(&self, item: &Item) -> Wire {
        let aad = format!("{}:{}", item.kind, item.id);
        let mut files = Vec::new();
        let data = item.data.as_ref().map(|d| {
            if let Some(text) = d.get("text").and_then(Value::as_str) {
                let mut cited = HashSet::new();
                cited_in(text, &mut cited);
                files = cited.iter().map(|n| self.opaque(n)).collect();
                files.sort();
            }
            let sealed = self.seal(d.to_string().as_bytes(), aad.as_bytes());
            serde_json::json!({ "v": 1, "box": B64.encode(sealed) })
        });
        Wire { kind: item.kind.clone(), id: item.id.clone(), data, files, updated: item.updated }
    }

    /// None: not opened (another key, or damaged).
    fn open_item(&self, wire: Wire) -> Option<Item> {
        let data = match wire.data {
            None => None,
            Some(d) => {
                let sealed = B64.decode(d.get("box")?.as_str()?).ok()?;
                let plain = self.open(&sealed, format!("{}:{}", wire.kind, wire.id).as_bytes())?;
                Some(serde_json::from_slice(&plain).ok()?)
            }
        };
        Some(Item { kind: wire.kind, id: wire.id, data, updated: wire.updated })
    }
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
    /// Where the key is kept, sealed (among the SSH passwords: `ssh::save_password`).
    key_slot: Option<uuid::Uuid>,
}

fn state_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("notes-sync.json"))
}

fn load_state(account: &str) -> State {
    let state: State = state_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    // Another account: everything from the start (its key forgotten).
    if state.account == account {
        state
    } else {
        if let Some(slot) = state.key_slot {
            crate::ssh::delete_password(slot);
        }
        State { account: account.to_owned(), ..State::default() }
    }
}

fn save_state(state: &State) {
    if let (Some(path), Ok(bytes)) = (state_path(), serde_json::to_vec(state)) {
        let _ = std::fs::write(path, bytes);
    }
}

fn local_key(state: &State) -> Option<Key> {
    let b64 = crate::ssh::load_password(state.key_slot?)?;
    let bytes: [u8; 32] = B64.decode(b64).ok()?.try_into().ok()?;
    Some(Key(bytes))
}

fn keep_key(state: &mut State, key: &Key) -> Result<(), String> {
    let slot = *state.key_slot.get_or_insert_with(uuid::Uuid::new_v4);
    crate::ssh::save_password(slot, &B64.encode(key.0)).map_err(|e| e.to_string())
}

/// Whether this machine has the key for this account (the phrase given here once).
pub fn has_key(account: &FloorAccount) -> bool {
    let state: Option<State> = state_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok());
    state.filter(|s| s.account == account.id).as_ref().and_then(local_key).is_some()
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
    /// Whether floor has a phrase for the account (None: not asked yet).
    remote_key: Option<bool>,
    /// The password given opened the key: kept here.
    unlocked: bool,
    /// The password given didn't open it.
    wrong: bool,
}

/// Kept by the app: the sync running, when the last one was.
#[derive(Default)]
pub struct NoteSync {
    shared: Arc<Mutex<Shared>>,
    last: Option<f64>,
    /// When the notes changed here and weren't sent yet.
    changed: Option<f64>,
    focused: bool,
    /// The key known to be here (looked for once, then set by an unlock).
    keyed: Option<bool>,
}

/// What the settings show of the sync.
pub struct Status {
    pub at: Option<i64>,
    pub error: Option<String>,
    pub busy: bool,
    /// Whether floor has a key already (None: not known yet).
    pub remote_key: Option<bool>,
    pub wrong: bool,
}

impl NoteSync {
    pub fn poll(&mut self) -> Option<Synced> {
        let mut shared = self.shared.lock().ok()?;
        if std::mem::take(&mut shared.unlocked) {
            self.keyed = Some(true);
            self.last = None;
        }
        shared.done.take()
    }

    /// The notes changed here (saved): sent in a moment.
    pub fn touched(&mut self, now: f64) {
        self.changed.get_or_insert(now);
    }

    pub fn status(&self) -> Status {
        self.shared.lock().map(|s| Status { at: s.at, error: s.error.clone(), busy: s.busy, remote_key: s.remote_key, wrong: s.wrong }).unwrap_or(Status { at: None, error: None, busy: false, remote_key: None, wrong: false })
    }

    /// Whether this machine has the key (the password given here once).
    pub fn keyed(&mut self, account: &FloorAccount) -> bool {
        *self.keyed.get_or_insert_with(|| has_key(account))
    }

    /// Whether floor has a key for the account, asked in the background (once).
    pub fn ask_remote(&mut self, ctx: &egui::Context, account: &FloorAccount) {
        let Some(url) = url().filter(|u| *u == account.url) else { return };
        {
            let Ok(mut shared) = self.shared.lock() else { return };
            if shared.busy || shared.remote_key.is_some() || shared.error.is_some() {
                return;
            }
            shared.busy = true;
        }
        let (shared, ctx, token) = (self.shared.clone(), ctx.clone(), account.token.clone());
        std::thread::spawn(move || {
            let got = remote_key(&url, &token);
            if let Ok(mut shared) = shared.lock() {
                match got {
                    Ok(k) => {
                        shared.remote_key = Some(k.is_some());
                        shared.error = None;
                    }
                    Err(e) => shared.error = Some(e),
                }
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }

    /// The account's password given: the key taken out of floor's wrap (made and wrapped the first
    /// time; wrapped again if the password changed meanwhile and this machine has it), kept here.
    pub fn unlock(&mut self, ctx: &egui::Context, account: &FloorAccount, password: String) {
        let Some(url) = url().filter(|u| *u == account.url) else { return };
        if !self.start() {
            return;
        }
        let (shared, ctx, token, id) = (self.shared.clone(), ctx.clone(), account.token.clone(), account.id.clone());
        std::thread::spawn(move || {
            let mut state = load_state(&id);
            let done = make_key(&url, &token, &password, local_key(&state)).and_then(|key| match key {
                Some(key) => keep_key(&mut state, &key).map(|()| true),
                None => Ok(false),
            });
            save_state(&state);
            if let Ok(mut shared) = shared.lock() {
                match done {
                    Ok(ok) => {
                        shared.unlocked = ok;
                        shared.wrong = !ok;
                        shared.remote_key = Some(true);
                        shared.error = None;
                    }
                    Err(e) => shared.error = Some(e),
                }
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }

    /// The key lost (the password reset while no machine had it): everything on floor erased (notes,
    /// files, key), this machine's key and memory of floor too; the notes here sent again with a new key.
    pub fn reset(&mut self, ctx: &egui::Context, account: &FloorAccount) {
        let Some(url) = url().filter(|u| *u == account.url) else { return };
        if !self.start() {
            return;
        }
        self.keyed = Some(false);
        let (shared, ctx, token, id) = (self.shared.clone(), ctx.clone(), account.token.clone(), account.id.clone());
        std::thread::spawn(move || {
            let res = account_agent().delete(format!("{url}/v1/notes")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call();
            let done = match res {
                Ok(r) if r.status().is_success() => Ok(()),
                Ok(r) => Err(format!("floor answered {}", r.status().as_u16())),
                Err(e) => Err(e.to_string()),
            };
            if done.is_ok() {
                let state = load_state(&id);
                if let Some(slot) = state.key_slot {
                    crate::ssh::delete_password(slot);
                }
                save_state(&State { account: id, ..State::default() });
            }
            if let Ok(mut shared) = shared.lock() {
                match done {
                    Ok(()) => {
                        shared.remote_key = Some(false);
                        shared.wrong = false;
                        shared.error = None;
                        shared.at = None;
                    }
                    Err(e) => shared.error = Some(e),
                }
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }

    /// The account's password changed (on this machine): the key wrapped again with the new one, so
    /// that the other machines and the new ones still get it (sync on here or not).
    pub fn rewrap(&mut self, account: &FloorAccount, current: String, new: String) {
        let Some(url) = url().filter(|u| *u == account.url) else { return };
        let (token, id) = (account.token.clone(), account.id.clone());
        std::thread::spawn(move || {
            let state = load_state(&id);
            let done = remote_key(&url, &token).and_then(|remote| {
                let Some((salt, wrap)) = remote else { return Ok(()) };
                let key = local_key(&state).or_else(|| Key::derive(&current, &salt).ok()?.open(&wrap, WRAP).and_then(|k| Some(Key(k.try_into().ok()?))));
                match key {
                    Some(key) => put_wrap(&url, &token, &new, &key).map(|_| ()),
                    None => Err("the notes' key wasn't opened with the former password".into()),
                }
            });
            if let Err(e) = done {
                crate::log::info(&format!("notes key: {e}"));
            }
        });
    }

    fn start(&self) -> bool {
        let Ok(mut shared) = self.shared.lock() else { return false };
        if shared.busy {
            return false;
        }
        shared.busy = true;
        true
    }

    /// Asked now (the switch just turned on).
    pub fn soon(&mut self) {
        self.last = None;
        if let Ok(mut shared) = self.shared.lock() {
            shared.remote_key = None;
            shared.error = None;
        }
    }

    /// Whether to sync now: the first time, the window back in front, a change made here a moment
    /// ago, or `EVERY` seconds since the last.
    pub fn due(&mut self, now: f64, focused: bool) -> bool {
        let Ok(shared) = self.shared.lock() else { return false };
        if shared.busy {
            return false;
        }
        let failed = shared.error.is_some();
        drop(shared);
        let back = focused && !std::mem::replace(&mut self.focused, focused);
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
        if !self.start() {
            return;
        }
        self.last = Some(now);
        self.changed = None;
        let (shared, ctx, token, id) = (self.shared.clone(), ctx.clone(), account.token.clone(), account.id.clone());
        std::thread::spawn(move || {
            let mut state = load_state(&id);
            let done = match local_key(&state) {
                Some(key) => exchange(&url, &token, &key, &mut state, local, cited),
                None => Err("no key on this computer".into()),
            };
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

#[derive(Deserialize)]
struct RemoteKey {
    salt: Option<String>,
    wrap: Option<String>,
}

/// floor's salt and wrap for the account (None: no key yet).
fn remote_key(url: &str, token: &str) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
    let mut res = account_agent().get(format!("{url}/v1/notes/key")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call().map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("floor answered {}", res.status().as_u16()));
    }
    let k: RemoteKey = res.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(match (k.salt, k.wrap) {
        (Some(s), Some(c)) => Some((B64.decode(s).map_err(|e| e.to_string())?, B64.decode(c).map_err(|e| e.to_string())?)),
        _ => None,
    })
}

/// `key` wrapped with `password` and given to floor (which checks the password: false when wrong).
fn put_wrap(url: &str, token: &str, password: &str, key: &Key) -> Result<bool, String> {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let wrap = Key::derive(password, &salt)?.seal(&key.0, WRAP);
    let res = account_agent()
        .put(format!("{url}/v1/notes/key"))
        .header("User-Agent", USER_AGENT)
        .header("Authorization", format!("Bearer {token}"))
        .send_json(serde_json::json!({ "password": password, "salt": B64.encode(salt), "wrap": B64.encode(&wrap) }))
        .map_err(|e| e.to_string())?;
    match res.status().as_u16() {
        200..=299 => Ok(true),
        403 => Ok(false),
        status => Err(format!("floor answered {status}")),
    }
}

/// The key with the account's `password`: out of floor's wrap; wrapped again when it doesn't open
/// with it but this machine has the key (the password changed elsewhere); drawn and wrapped the first
/// time. None: the password is wrong (or changed while no machine had the key).
fn make_key(url: &str, token: &str, password: &str, here: Option<Key>) -> Result<Option<Key>, String> {
    if let Some((salt, wrap)) = remote_key(url, token)? {
        if let Some(bytes) = Key::derive(password, &salt)?.open(&wrap, WRAP) {
            return Ok(<[u8; 32]>::try_from(bytes).ok().map(Key));
        }
        return match here {
            Some(key) => Ok(put_wrap(url, token, password, &key)?.then_some(key)),
            None => Ok(None),
        };
    }
    let key = here.unwrap_or_else(Key::random);
    Ok(put_wrap(url, token, password, &key)?.then_some(key))
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

fn exchange(url: &str, token: &str, key: &Key, state: &mut State, local: Vec<Item>, mut cited: HashSet<String>) -> Result<Synced, String> {
    let agent = account_agent();
    let auth = format!("Bearer {token}");
    let check = |status: u16| if (200..300).contains(&status) { Ok(()) } else { Err(format!("floor answered {status}")) };

    // What changed here, sealed, in batches.
    let send: Vec<Wire> = to_send(state, &local).into_iter().map(|i| key.seal_item(i)).collect();
    let mut batch: Vec<&Wire> = Vec::new();
    let mut bytes = 0;
    let flush = |batch: &mut Vec<&Wire>, state: &mut State| -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let res = agent.post(format!("{url}/v1/notes/items")).header("User-Agent", USER_AGENT).header("Authorization", &auth).send_json(serde_json::json!({ "items": batch })).map_err(|e| e.to_string())?;
        check(res.status().as_u16())?;
        for w in batch.drain(..) {
            let k = state.known.entry(w.id.clone()).or_insert(w.updated);
            *k = (*k).max(w.updated);
        }
        Ok(())
    };
    for wire in &send {
        let size = wire.data.as_ref().map_or(64, |d| d.to_string().len());
        if !batch.is_empty() && (batch.len() >= BATCH || bytes + size > BATCH_BYTES) {
            flush(&mut batch, state)?;
            bytes = 0;
        }
        bytes += size;
        batch.push(wire);
    }
    flush(&mut batch, state)?;

    // What changed on floor (one's own just sent among it: the same date, nothing to do).
    #[derive(Deserialize)]
    struct Page {
        items: Vec<Wire>,
        seq: i64,
        more: bool,
    }
    let mut items = Vec::new();
    let mut unread = 0;
    loop {
        let mut res = agent.get(format!("{url}/v1/notes/items")).query("since", state.seq.to_string()).header("User-Agent", USER_AGENT).header("Authorization", &auth).call().map_err(|e| e.to_string())?;
        check(res.status().as_u16())?;
        let page: Page = res.body_mut().with_config().limit(64 * 1024 * 1024).read_json().map_err(|e| e.to_string())?;
        for wire in page.items {
            let (id, updated) = (wire.id.clone(), wire.updated);
            let Some(item) = key.open_item(wire) else {
                unread += 1;
                continue;
            };
            let k = state.known.entry(id).or_insert(updated);
            *k = (*k).max(updated);
            if let Some(text) = item.data.as_ref().and_then(|d| d.get("text")).and_then(Value::as_str) {
                cited_in(text, &mut cited);
            }
            items.push(item);
        }
        state.seq = page.seq;
        if !page.more {
            break;
        }
    }
    if unread > 0 {
        crate::log::info(&format!("notes sync: {unread} item(s) not opened (another key?)"));
    }

    // The files: those floor lacks go up, those this machine lacks come down (sealed both ways).
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
        let opaque = key.opaque(name);
        if path.is_file() {
            if there.contains(&opaque) || path.metadata().map_or(true, |m| m.len() > FILE_MAX) {
                continue;
            }
            let Ok(plain) = std::fs::read(&path) else { continue };
            let body = [FILE_MAGIC, &key.seal(&plain, opaque.as_bytes())].concat();
            let res = agent.put(format!("{url}/v1/notes/files/{opaque}")).header("User-Agent", USER_AGENT).header("Authorization", &auth).header("Content-Type", "application/octet-stream").send(&body[..]).map_err(|e| e.to_string())?;
            check(res.status().as_u16())?;
        } else if there.contains(&opaque) {
            let mut res = agent.get(format!("{url}/v1/notes/files/{opaque}")).header("User-Agent", USER_AGENT).header("Authorization", &auth).call().map_err(|e| e.to_string())?;
            check(res.status().as_u16())?;
            let body = res.body_mut().with_config().limit(32 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
            let Some(plain) = body.strip_prefix(FILE_MAGIC).and_then(|b| key.open(b, opaque.as_bytes())) else {
                crate::log::info(&format!("notes sync: {name} not opened"));
                continue;
            };
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            // Written aside, then moved: never half a file under its name.
            let part = dir.join(format!(".{name}.part"));
            std::fs::write(&part, plain).map_err(|e| e.to_string())?;
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

    #[test]
    fn sealed_items() {
        let key = Key::derive("une phrase assez longue", b"0123456789abcdef").unwrap();
        let item = Item { kind: "note".into(), id: "n1".into(), data: Some(serde_json::json!({ "text": "Secret ![i](notes-files/ab12cd34-image.png)" })), updated: 5 };
        let wire = key.seal_item(&item);
        let shown = serde_json::to_string(&wire).unwrap();
        assert!(!shown.contains("Secret") && !shown.contains("image.png"), "nothing in clear: {shown}");
        assert_eq!(wire.files, [key.opaque("ab12cd34-image.png")]);
        assert_eq!(key.open_item(wire).unwrap(), item);
        // Another key, or the box moved to another id: not opened.
        let other = Key::derive("une autre phrase longue", b"0123456789abcdef").unwrap();
        assert!(other.open_item(key.seal_item(&item)).is_none());
        let mut moved = key.seal_item(&item);
        moved.id = "n2".into();
        assert!(key.open_item(moved).is_none());
    }
}

/// Two machines on a floor running (RONNIE_FLOOR_URL, RONNIE_TEST_TOKEN: an account's session):
/// (RONNIE_TEST_PASSWORD: its password) the key made on one, opened on the other (a wrong password
/// refused), a note and its image found.
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
    let password = std::env::var("RONNIE_TEST_PASSWORD").unwrap();
    let key = make_key(&url, &token, &password, None).unwrap().expect("made");
    assert!(make_key(&url, &token, "pas le bon mot de passe", None).unwrap().is_none(), "a wrong password refused");
    let again = make_key(&url, &token, &password, None).unwrap().expect("opened");
    assert_eq!(again.0, key.0);
    // SAFETY: this test alone sets it, run on its own.
    unsafe { std::env::set_var("RONNIE_CONFIG_DIR", &one) };
    let mut cited = HashSet::new();
    cited_in("notes-files/ab12cd34-image.png", &mut cited);
    let mut state = State::default();
    exchange(&url, &token, &key, &mut state, vec![item.clone()], cited).unwrap();
    assert!(to_send(&state, std::slice::from_ref(&item)).is_empty(), "sent: not again");
    unsafe { std::env::set_var("RONNIE_CONFIG_DIR", &two) };
    let mut state = State::default();
    let done = exchange(&url, &token, &again, &mut state, Vec::new(), HashSet::new()).unwrap();
    assert_eq!(done.items.iter().find(|i| i.id == id), Some(&item));
    assert!(done.files);
    assert_eq!(std::fs::read(two.join("notes-files/ab12cd34-image.png")).unwrap(), png);
}
