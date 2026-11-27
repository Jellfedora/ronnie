//! floor: Ronnie's server (its own repository, next to Ronnie's): the accounts (a pseudo and a
//! password, a session per machine), their avatars, and what the other features keep there (the notes
//! in sync, the project tracking, the usage statistics, the releases).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::config::FloorAccount;

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

/// The pseudo of the account, when it was made on this floor.
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

#[derive(Default)]
struct Shared {
    /// The pseudo typed last asked about, and floor's answer once it came: free or not (None: floor
    /// didn't answer).
    checked: Option<(String, Option<Option<bool>>)>,
    /// A pseudo being claimed, then floor's answer.
    claiming: bool,
    claimed: Option<Result<AccountDone, ClaimError>>,
}

/// Kept by the app: what the account window asked floor, and its answers.
#[derive(Default)]
pub struct Floor {
    shared: Arc<Mutex<Shared>>,
}

impl Floor {
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
        self.shared.lock().ok()?.claimed.take()
    }

}

pub(crate) fn agent() -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(15))).build().new_agent()
}

pub(crate) const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));

/// The account's requests: an error status is read (its message), not an error.
pub(crate) fn account_agent() -> ureq::Agent {
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

/// Whether floor's administrator has allowed the account yet (None: floor didn't say).
pub(crate) fn approved(account: &FloorAccount) -> Option<bool> {
    #[derive(Deserialize)]
    struct Me {
        #[serde(default)]
        approved: Option<bool>,
    }
    let mut res = account_agent().get(format!("{}/v1/players/me", account.url.trim_end_matches('/'))).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {}", account.token)).call().ok()?;
    if !res.status().is_success() {
        return None;
    }
    // A floor from before the approval: everyone is.
    Some(res.body_mut().read_json::<Me>().ok()?.approved.unwrap_or(true))
}

pub(crate) fn account_ask(url: &str, account: Option<&FloorAccount>, ask: AccountAsk) -> Result<AccountDone, ClaimError> {
    #[derive(Deserialize)]
    struct Signed {
        id: String,
        name: String,
        token: String,
        #[serde(default)]
        admin: bool,
        #[serde(default)]
        avatar: Option<String>,
        /// Allowed by the administrator (a floor from before: always).
        #[serde(default = "yes")]
        approved: bool,
    }
    fn yes() -> bool {
        true
    }
    #[derive(Deserialize)]
    struct Avatar {
        avatar: Option<String>,
    }
    let agent = account_agent();
    let bad = |e: ureq::Error| {
        crate::log::info(&format!("floor: {e}"));
        ClaimError::Unreachable
    };
    let signed = |res: ureq::http::Response<ureq::Body>| -> Result<AccountDone, ClaimError> {
        let mut res = res;
        let s: Signed = res.body_mut().read_json().map_err(bad)?;
        Ok(AccountDone::Account(FloorAccount { url: url.to_owned(), id: s.id, token: s.token, name: Some(s.name), admin: s.admin, avatar: s.avatar, pending: !s.approved }))
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
    let path = format!("{url}/v1/players/{id}/avatar");
    let mut res = agent().get(path).query("v", version).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call().ok()?;
    let bytes = res.body_mut().with_config().limit(512 * 1024).read_to_vec().ok()?;
    let image = image::load_from_memory(&bytes).ok()?.thumbnail(256, 256).to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw()))
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

    #[test]
    fn names_as_floor_keeps_them() {
        assert_eq!(floor_name("   "), None);
        assert_eq!(floor_name(&"x".repeat(30)).unwrap().len(), NAME_MAX);
    }
}
