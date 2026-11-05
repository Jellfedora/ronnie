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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{FloorAccount, TypingScore};

const GAME: &str = "speed-metal";
/// Places shown on the board.
pub const BOARD: usize = 10;
/// The names floor takes, at most.
const NAME_MAX: usize = 24;

/// Where floor runs (its own VM: no domain name, a Let's Encrypt certificate for the IP). An account
/// made on another address (the Beemo's, before) doesn't count here: the pseudo is asked again.
const URL: &str = "https://185.245.143.130/floor";

/// floor's address: RONNIE_FLOOR_URL when given (at run time, or when built); a dev build, a local floor.
pub fn url() -> Option<String> {
    let url = std::env::var("RONNIE_FLOOR_URL").ok().or_else(|| option_env!("RONNIE_FLOOR_URL").map(str::to_owned));
    let url = url.unwrap_or_else(|| if crate::config::OFFICIAL { URL } else { "http://localhost:5002" }.to_owned());
    let url = url.trim().trim_end_matches('/');
    (!url.is_empty()).then(|| url.to_owned())
}

/// The download page on floor's server (its root, floor being served under /floor), or floor's own
/// page (a floor without the prefix, as in development).
pub fn site() -> Option<String> {
    let url = url()?;
    Some(match url.strip_suffix("/floor") {
        Some(root) => format!("{root}/"),
        None => format!("{url}/download"),
    })
}

/// A line of the board.
#[derive(Clone, Debug)]
pub struct Entry {
    /// None: the player gave none.
    pub name: Option<String>,
    pub score: TypingScore,
    /// The line of this player.
    pub me: bool,
    /// The player's id, avatar version and frame (an older floor, a round played here: none).
    pub id: Option<String>,
    pub avatar: Option<String>,
    pub frame: Option<String>,
    /// Their face, once fetched (`App::faced`), to draw on the board.
    pub face: Option<Face>,
}

/// A face fetched, on a line of a board.
#[derive(Clone)]
pub struct Face(pub egui::TextureHandle);

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Face")
    }
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
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    frame: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Details {
    letters: u32,
    words: u32,
    wpm: u32,
    accuracy: u32,
    combo: u32,
}

/// The pseudo of the account, when it was made on this floor: without one, no online scores.
pub fn pseudo(account: Option<&FloorAccount>) -> Option<&str> {
    let url = url()?;
    account.filter(|a| a.url == url)?.name.as_deref()
}

/// Why floor said no to the account window.
#[derive(Clone, Debug, PartialEq)]
pub enum ClaimError {
    /// The pseudo is another player's.
    Taken,
    /// Refused by floor (its message: a password too short, an image it doesn't take…).
    Refused(String),
    /// floor didn't answer.
    Unreachable,
    /// Signing in: a wrong pseudo or password (floor doesn't say which).
    WrongPassword,
    /// Signing in: the account is blocked.
    Blocked,
    /// Signing in: too many tries for this pseudo, a quarter of an hour to wait.
    TooMany,
    /// Changing the password: the current one is wrong.
    WrongCurrent,
    /// The session is over (signed out elsewhere, the password changed, the account blocked): to sign in again.
    Expired,
    /// The avatar chosen isn't an image this can read.
    Unreadable,
}

/// What the account window asks floor.
#[derive(Clone, Debug)]
pub enum AccountAsk {
    SignIn { name: String, password: String },
    SignUp { name: String, password: String },
    Rename { name: String },
    Password { current: String, new: String },
    SignOut,
    /// The avatar: an image file's bytes (made small before it goes), or none.
    Avatar(Option<Vec<u8>>),
    /// The frame around it (`app::frames`), or none.
    Frame(Option<String>),
}

/// What floor did.
#[derive(Clone, Debug, PartialEq)]
pub enum AccountDone {
    /// Signed in, signed up, renamed, its avatar changed: the account to keep.
    Account(FloorAccount),
    SignedOut,
    PasswordChanged,
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
    claimed: Option<Result<AccountDone, ClaimError>>,
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

    /// Asked to floor in the background (sign in, sign up, rename…): its answer comes to `claimed`.
    pub fn ask(&mut self, ctx: &egui::Context, account: Option<&FloorAccount>, ask: AccountAsk) {
        let Some(url) = url() else { return };
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
            let claimed = account_ask(&url, account.as_ref(), ask);
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

    /// floor's answer to what was asked: what it did, or why not.
    pub fn claimed(&mut self) -> Option<Result<AccountDone, ClaimError>> {
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

pub(crate) fn agent() -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(15))).build().new_agent()
}

pub(crate) const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));

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
    Entry { name: e.name, me: e.me, id: e.id, avatar: e.avatar, frame: e.frame, face: None, score: TypingScore { letters: e.score, words: d.words, wpm: d.wpm, accuracy: d.accuracy, combo: d.combo, at: e.at / 1000 } }
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

/// The account's requests: an error status is read (its message), not an error.
fn account_agent() -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().new_agent()
}

/// How this machine's session is named on floor.
fn device() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("Ronnie, {os}")
}

/// floor's answer, or why not: `status` turns an error status into its meaning (else floor's message).
fn answered(res: Result<ureq::http::Response<ureq::Body>, ureq::Error>, status: impl Fn(u16) -> Option<ClaimError>) -> Result<ureq::http::Response<ureq::Body>, ClaimError> {
    #[derive(Deserialize)]
    struct Refusal {
        error: String,
    }
    let mut res = res.map_err(|e| {
        crate::log::info(&format!("floor: {e}"));
        ClaimError::Unreachable
    })?;
    let code = res.status().as_u16();
    if res.status().is_success() {
        return Ok(res);
    }
    if code >= 500 {
        return Err(ClaimError::Unreachable);
    }
    if let Some(e) = status(code) {
        return Err(e);
    }
    let message = res.body_mut().read_json::<Refusal>().map(|r| r.error).unwrap_or_else(|_| code.to_string());
    Err(ClaimError::Refused(message))
}

fn account_ask(url: &str, account: Option<&FloorAccount>, ask: AccountAsk) -> Result<AccountDone, ClaimError> {
    #[derive(Deserialize)]
    struct Signed {
        id: String,
        name: String,
        token: String,
        #[serde(default)]
        admin: bool,
        #[serde(default)]
        avatar: Option<String>,
        #[serde(default)]
        frame: Option<String>,
    }
    #[derive(Deserialize)]
    struct Avatar {
        avatar: Option<String>,
    }
    #[derive(Deserialize)]
    struct Framed {
        frame: Option<String>,
    }
    let agent = account_agent();
    let bad = |e: ureq::Error| {
        crate::log::info(&format!("floor: {e}"));
        ClaimError::Unreachable
    };
    let signed = |res: ureq::http::Response<ureq::Body>| -> Result<AccountDone, ClaimError> {
        let mut res = res;
        let s: Signed = res.body_mut().read_json().map_err(bad)?;
        Ok(AccountDone::Account(FloorAccount { url: url.to_owned(), id: s.id, token: s.token, name: Some(s.name), admin: s.admin, avatar: s.avatar, frame: s.frame }))
    };
    // A request with the session's token: none, or floor no longer knows it, and it is over.
    let token = || account.map(|a| format!("Bearer {}", a.token)).ok_or(ClaimError::Expired);
    let expired = |code: u16| (code == 401).then_some(ClaimError::Expired);
    match ask {
        AccountAsk::SignIn { name, password } => {
            let body = serde_json::json!({ "name": name, "password": password, "device": device() });
            let res = agent.post(format!("{url}/v1/sessions")).header("User-Agent", USER_AGENT).send_json(&body);
            signed(answered(res, |code| match code {
                401 => Some(ClaimError::WrongPassword),
                403 => Some(ClaimError::Blocked),
                429 => Some(ClaimError::TooMany),
                _ => None,
            })?)
        }
        AccountAsk::SignUp { name, password } => {
            let body = serde_json::json!({ "name": name, "password": password, "device": device() });
            let res = agent.post(format!("{url}/v1/accounts")).header("User-Agent", USER_AGENT).send_json(&body);
            signed(answered(res, |code| match code {
                409 => Some(ClaimError::Taken),
                429 => Some(ClaimError::TooMany),
                _ => None,
            })?)
        }
        AccountAsk::Rename { name } => {
            let res = agent.patch(format!("{url}/v1/players/me")).header("User-Agent", USER_AGENT).header("Authorization", token()?).send_json(serde_json::json!({ "name": name }));
            answered(res, |code| if code == 409 { Some(ClaimError::Taken) } else { expired(code) })?;
            let mut a = account.cloned().ok_or(ClaimError::Expired)?;
            a.name = Some(name);
            Ok(AccountDone::Account(a))
        }
        AccountAsk::Password { current, new } => {
            let body = serde_json::json!({ "current": current, "password": new });
            let res = agent.put(format!("{url}/v1/players/me/password")).header("User-Agent", USER_AGENT).header("Authorization", token()?).send_json(&body);
            answered(res, |code| if code == 403 { Some(ClaimError::WrongCurrent) } else { expired(code) })?;
            Ok(AccountDone::PasswordChanged)
        }
        AccountAsk::SignOut => {
            // Signed out here whatever floor says (unreachable, it forgets the session in 90 days).
            if let Ok(token) = token() {
                let _ = agent.delete(format!("{url}/v1/sessions/current")).header("User-Agent", USER_AGENT).header("Authorization", token).call();
            }
            Ok(AccountDone::SignedOut)
        }
        AccountAsk::Avatar(image) => {
            let target = format!("{url}/v1/players/me/avatar");
            let image = match image {
                Some(bytes) => Some(avatar_image(&bytes).ok_or(ClaimError::Unreadable)?),
                None => None,
            };
            let res = match &image {
                Some(bytes) => agent.put(&target).header("User-Agent", USER_AGENT).header("Authorization", token()?).header("Content-Type", "application/octet-stream").send(&bytes[..]),
                None => agent.delete(&target).header("User-Agent", USER_AGENT).header("Authorization", token()?).call(),
            };
            let mut res = answered(res, expired)?;
            let answer: Avatar = res.body_mut().read_json().map_err(bad)?;
            let mut a = account.cloned().ok_or(ClaimError::Expired)?;
            a.avatar = answer.avatar;
            Ok(AccountDone::Account(a))
        }
        AccountAsk::Frame(frame) => {
            let res = agent.put(format!("{url}/v1/players/me/frame")).header("User-Agent", USER_AGENT).header("Authorization", token()?).send_json(serde_json::json!({ "frame": frame }));
            let mut res = answered(res, expired)?;
            let answer: Framed = res.body_mut().read_json().map_err(bad)?;
            let mut a = account.cloned().ok_or(ClaimError::Expired)?;
            a.frame = answer.frame;
            Ok(AccountDone::Account(a))
        }
    }
}

/// An avatar made from an image file: a centered square, 256 pixels, as PNG (JPEG when a photo makes
/// the PNG too big for floor). None: not an image this can read.
pub fn avatar_image(bytes: &[u8]) -> Option<Vec<u8>> {
    const SIDE: u32 = 256;
    const MAX: usize = 250 * 1024;
    let image = image::load_from_memory(bytes).ok()?;
    let side = image.width().min(image.height());
    let square = image.crop_imm((image.width() - side) / 2, (image.height() - side) / 2, side, side);
    let small = square.resize_exact(SIDE, SIDE, image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    if png.len() <= MAX {
        return Some(png);
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode_image(&small.to_rgb8()).ok()?;
    (jpeg.len() <= MAX).then_some(jpeg)
}

/// How an avatar is coming.
enum Load {
    Loading,
    Ready(egui::ColorImage),
    /// Made into a texture (kept in `Avatars::textures`).
    Done,
    Failed,
}

/// The players' avatars, fetched from floor when first shown, by player id (a new version: fetched again).
#[derive(Default)]
pub struct Avatars {
    loads: Arc<Mutex<HashMap<String, (String, Load)>>>,
    /// Behind a lock: fetched from anywhere a face shows, borrowed or not.
    textures: Mutex<HashMap<String, (String, egui::TextureHandle)>>,
}

impl Avatars {
    /// This player's avatar at this version, once fetched (meanwhile, and without one, None: the initial).
    pub fn get(&self, ctx: &egui::Context, account: Option<&FloorAccount>, id: &str, version: Option<&str>) -> Option<egui::TextureHandle> {
        let version = version?;
        if let Some((v, texture)) = self.textures.lock().ok()?.get(id)
            && v == version
        {
            return Some(texture.clone());
        }
        let url = url()?;
        let account = account.filter(|a| a.url == url)?;
        let mut loads = self.loads.lock().ok()?;
        match loads.get_mut(id) {
            Some((v, load)) if v == version => {
                if let Load::Ready(_) = load {
                    let Load::Ready(image) = std::mem::replace(load, Load::Done) else { unreachable!() };
                    let texture = ctx.load_texture(format!("avatar-{id}-{version}"), image, egui::TextureOptions::LINEAR);
                    self.textures.lock().ok()?.insert(id.to_owned(), (version.to_owned(), texture.clone()));
                    return Some(texture);
                }
                None
            }
            _ => {
                loads.insert(id.to_owned(), (version.to_owned(), Load::Loading));
                let (loads, ctx, token, id, version) = (self.loads.clone(), ctx.clone(), account.token.clone(), id.to_owned(), version.to_owned());
                std::thread::spawn(move || {
                    let load = fetch_avatar(&url, &token, &id, &version).map_or(Load::Failed, Load::Ready);
                    if let Ok(mut loads) = loads.lock()
                        && loads.get(&id).is_some_and(|(v, _)| *v == version)
                    {
                        loads.insert(id, (version, load));
                    }
                    ctx.request_repaint();
                });
                None
            }
        }
    }
}

/// An avatar fetched from floor (another player's, or one's own), as a picture.
pub fn fetch_avatar(url: &str, token: &str, id: &str, version: &str) -> Option<egui::ColorImage> {
    let mut res = agent().get(format!("{url}/v1/players/{id}/avatar")).query("v", version).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call().ok()?;
    let bytes = res.body_mut().with_config().limit(512 * 1024).read_to_vec().ok()?;
    let image = image::load_from_memory(&bytes).ok()?.thumbnail(256, 256).to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw()))
}

/// The board to show: floor's when it answered and the player has a pseudo, with the player's line
/// taken from the rounds played here (the best one, if floor doesn't have it yet); else the rounds
/// played here.
pub fn shown(floor: Option<&[Entry]>, local: &[TypingScore], pseudo: Option<&str>) -> Vec<Entry> {
    let name = pseudo.map(str::to_owned);
    let Some(floor) = floor.filter(|_| pseudo.is_some()) else {
        return local.iter().map(|s| Entry { name: name.clone(), score: s.clone(), me: true, id: None, avatar: None, frame: None, face: None }).collect();
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
            rows.insert(place, Entry { name: name.clone(), score: best.clone(), me: true, id: None, avatar: None, frame: None, face: None });
        }
    }
    for r in rows.iter_mut().filter(|r| r.me) {
        r.name = name.clone();
    }
    rows.truncate(BOARD);
    rows
}

/// A game's board on floor, kept as `Floor` keeps Speed Metal's: fetched in the background, the round
/// just played sent with it. The minesweeper's (the fastest), Ronnie Run's (the farthest).
pub struct GameFloor {
    game: &'static str,
    /// The smaller score wins (a time).
    lower_wins: bool,
    shared: Arc<Mutex<GameShared>>,
    board: Option<Vec<GameEntry>>,
    last: Option<f64>,
    pending: Option<GameRound>,
}

/// A round to send: its score, and the body floor reads it from.
#[derive(Clone, Debug)]
pub struct GameRound {
    pub score: u32,
    pub details: serde_json::Value,
}

/// A line of a game's board.
#[derive(Clone, Debug)]
pub struct GameEntry {
    /// Its place on floor's board (floor decides the ties).
    pub rank: u32,
    pub name: Option<String>,
    pub score: u32,
    pub me: bool,
    /// What floor keeps with the score (Bug Hunt's wave...).
    pub details: serde_json::Value,
    /// The player's id, avatar version and frame (an older floor: none).
    pub id: Option<String>,
    pub avatar: Option<String>,
    pub frame: Option<String>,
}

#[derive(Default)]
struct GameShared {
    busy: bool,
    /// The board, and the round not sent (floor unreachable).
    done: Option<(Option<Vec<GameEntry>>, Option<GameRound>)>,
}

impl GameFloor {
    pub fn new(game: &'static str, lower_wins: bool) -> Self {
        Self { game, lower_wins, shared: Default::default(), board: None, last: None, pending: None }
    }

    /// floor's board when it answered, the best first.
    pub fn board(&self) -> Option<&[GameEntry]> {
        self.board.as_deref()
    }

    /// A round played: sent at the next sync (the best one, when several wait).
    pub fn finished(&mut self, round: GameRound) {
        let better = |p: &GameRound| if self.lower_wins { round.score < p.score } else { round.score > p.score };
        if self.pending.as_ref().is_none_or(better) {
            self.pending = Some(round);
        }
    }

    /// Takes what a sync brought back.
    pub fn poll(&mut self) {
        let Some((board, unsent)) = self.shared.lock().ok().and_then(|mut s| s.done.take()) else { return };
        if let Some(round) = unsent {
            self.finished(round);
        }
        if board.is_some() {
            self.board = board;
        }
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
                return;
            }
            shared.busy = true;
        }
        let account = account.filter(|a| a.url == url && a.name.is_some()).cloned();
        let round = self.pending.take().filter(|_| account.is_some());
        let (game, shared, ctx) = (self.game, self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let done = match game_sync(&url, game, account.as_ref(), round.as_ref()) {
                Ok(board) => (Some(board), None),
                Err(e) => {
                    crate::log::info(&format!("floor: {e}"));
                    // Refused by floor (out of its bounds): not tried again.
                    (None, round.filter(|_| !matches!(e, ureq::Error::StatusCode(400..=499))))
                }
            };
            if let Ok(mut shared) = shared.lock() {
                shared.done = Some(done);
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }
}

fn game_sync(url: &str, game: &str, account: Option<&FloorAccount>, round: Option<&GameRound>) -> Result<Vec<GameEntry>, ureq::Error> {
    #[derive(Deserialize)]
    struct Line {
        rank: u32,
        name: Option<String>,
        score: u32,
        me: bool,
        #[serde(default)]
        details: serde_json::Value,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        avatar: Option<String>,
        #[serde(default)]
        frame: Option<String>,
    }
    #[derive(Deserialize)]
    struct Answer {
        entries: Vec<Line>,
    }
    let agent = agent();
    // floor keeps the best one anyway.
    if let (Some(account), Some(round)) = (account, round) {
        agent.post(format!("{url}/v1/games/{game}/scores")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", account.token)).send_json(&round.details)?;
    }
    let mut request = agent.get(format!("{url}/v1/games/{game}/leaderboard")).query("limit", BOARD.to_string()).header("User-Agent", USER_AGENT);
    if let Some(a) = account {
        request = request.header("Authorization", format!("Bearer {}", a.token));
    }
    let answer: Answer = request.call()?.body_mut().read_json()?;
    Ok(answer.entries.into_iter().map(|l| GameEntry { rank: l.rank, name: l.name, score: l.score, me: l.me, details: l.details, id: l.id, avatar: l.avatar, frame: l.frame }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An account the whole way, on a floor running (RONNIE_FLOOR_URL): signed up, wrong password,
    /// signed in elsewhere, renamed, an avatar (a photo made small, a script refused), the password
    /// changed (the other session over), signed out.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn an_account() {
        let url = url().expect("RONNIE_FLOOR_URL");
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let name = format!("Dio{stamp}");
        let account = |done: Result<AccountDone, ClaimError>| match done {
            Ok(AccountDone::Account(a)) => a,
            other => panic!("{other:?}"),
        };
        let mac = account(account_ask(&url, None, AccountAsk::SignUp { name: name.clone(), password: "holy diver".into() }));
        assert_eq!(mac.name.as_deref(), Some(name.as_str()));
        assert_eq!(account_ask(&url, None, AccountAsk::SignUp { name: name.clone(), password: "holy diver".into() }), Err(ClaimError::Taken));
        assert!(matches!(account_ask(&url, None, AccountAsk::SignUp { name: format!("Ozzy{stamp}"), password: "court".into() }), Err(ClaimError::Refused(m)) if m.contains("trop court")));
        assert_eq!(account_ask(&url, None, AccountAsk::SignIn { name: name.clone(), password: "mauvais".into() }), Err(ClaimError::WrongPassword));
        let pc = account(account_ask(&url, None, AccountAsk::SignIn { name: name.to_lowercase(), password: "holy diver".into() }));
        assert_ne!(pc.token, mac.token);

        let renamed = account(account_ask(&url, Some(&mac), AccountAsk::Rename { name: format!("{name}x") }));
        assert_eq!(renamed.name, Some(format!("{name}x")));

        // A photo, not square: made a 256 square; floor keeps it, the other machine sees it.
        let photo = image::RgbImage::from_fn(900, 500, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8]));
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(photo).write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
        let with = account(account_ask(&url, Some(&renamed), AccountAsk::Avatar(Some(jpeg))));
        let version = with.avatar.clone().expect("a version");
        let seen = fetch_avatar(&url, &pc.token, &with.id, &version).expect("the avatar");
        assert_eq!(seen.size, [128, 128]);
        assert_eq!(account_ask(&url, Some(&with), AccountAsk::Avatar(Some(b"#!/bin/sh\nrm -rf /".to_vec()))), Err(ClaimError::Unreadable));

        // The password changed on the Mac: the PC's session is over.
        assert_eq!(account_ask(&url, Some(&with), AccountAsk::Password { current: "faux".into(), new: "last in line".into() }), Err(ClaimError::WrongCurrent));
        assert_eq!(account_ask(&url, Some(&with), AccountAsk::Password { current: "holy diver".into(), new: "last in line".into() }), Ok(AccountDone::PasswordChanged));
        assert_eq!(account_ask(&url, Some(&pc), AccountAsk::Rename { name: format!("{name}y") }), Err(ClaimError::Expired));

        assert_eq!(account_ask(&url, Some(&with), AccountAsk::SignOut), Ok(AccountDone::SignedOut));
        assert_eq!(account_ask(&url, Some(&with), AccountAsk::Avatar(None)), Err(ClaimError::Expired));
    }

    fn round(letters: u32, at: i64) -> TypingScore {
        TypingScore { letters, at, ..Default::default() }
    }

    fn row(name: &str, letters: u32, me: bool) -> Entry {
        Entry { name: Some(name.into()), score: round(letters, 0), me, id: None, avatar: None, frame: None, face: None }
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
