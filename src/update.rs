//! Updates from GitHub releases. A background check finds a newer release; once the user accepts, the
//! archive built for this platform is downloaded, checked against the digest GitHub publishes, and
//! swapped in place of the running app, which then restarts.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPO: &str = "Jellfedora/ronnie";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Target in the release archive names; macOS gets a single universal (arm64 + x86_64) app.
const TARGET: &str = if cfg!(target_os = "macos") { "universal-apple-darwin" } else { env!("TARGET") };
const ARCHIVE_EXT: &str = if cfg!(windows) { "zip" } else { "tar.gz" };
/// Refuse absurd downloads (the archive is a few tens of MB).
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// "sha256:<hex>", computed by GitHub on upload.
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Clone)]
pub struct Available {
    pub version: String,
    /// Release page, with the changelog.
    pub url: String,
    asset: Asset,
}

#[derive(Clone, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Available),
    Installing(String),
    /// Installed version, active after a restart.
    Installed(String),
    Failed(String),
}

/// Shared with the background threads doing the network work.
pub struct Updater {
    state: Arc<Mutex<State>>,
    /// Path of the running executable, read at startup: on Linux it points to a deleted file once replaced.
    exe: Option<PathBuf>,
}

impl Updater {
    pub fn new() -> Self {
        Self { state: Arc::default(), exe: std::env::current_exe().ok() }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    pub fn busy(&self) -> bool {
        matches!(self.state(), State::Checking | State::Installing(_))
    }

    /// Looks for a newer release in the background.
    pub fn check(&self, ctx: &egui::Context) {
        if self.busy() || matches!(self.state(), State::Installed(_)) {
            return;
        }
        *self.state.lock().unwrap() = State::Checking;
        let (state, ctx) = (self.state.clone(), ctx.clone());
        thread::spawn(move || {
            let result = match latest() {
                Ok(Some(available)) => State::Available(available),
                Ok(None) => State::UpToDate,
                Err(e) => State::Failed(format!("{e:#}")),
            };
            *state.lock().unwrap() = result;
            ctx.request_repaint();
        });
    }

    /// Downloads and installs `available` in the background.
    pub fn install(&self, ctx: &egui::Context, available: Available) {
        let Some(exe) = self.exe.clone() else { return };
        *self.state.lock().unwrap() = State::Installing(available.version.clone());
        let (state, ctx) = (self.state.clone(), ctx.clone());
        thread::spawn(move || {
            let result = match install(&exe, &available.asset) {
                Ok(()) => State::Installed(available.version),
                Err(e) => State::Failed(format!("{e:#}")),
            };
            *state.lock().unwrap() = result;
            ctx.request_repaint();
        });
    }

    /// Starts the (updated) app again; the caller then closes this instance.
    pub fn relaunch(&self) -> Result<()> {
        let exe = self.exe.as_deref().context("chemin de l'exécutable inconnu")?;
        #[cfg(target_os = "macos")]
        if let Some(bundle) = app_bundle(exe) {
            Command::new("open").arg("-n").arg(bundle).spawn()?;
            return Ok(());
        }
        Command::new(exe).spawn()?;
        Ok(())
    }
}

/// The latest release, if it is newer than this build and has an archive for this platform.
fn latest() -> Result<Option<Available>> {
    let release: Release = ureq::get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("GitHub injoignable")?
        .body_mut()
        .read_json()?;
    let version = release.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&version, VERSION) {
        return Ok(None);
    }
    let name = format!("ronnie-{TARGET}.{ARCHIVE_EXT}");
    let asset = release.assets.into_iter().find(|a| a.name == name);
    Ok(asset.map(|asset| Available { version, url: release.html_url, asset }))
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.split(['.', '-', '+']).map(str::parse::<u64>);
    Some((parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?))
}

fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

fn download(asset: &Asset) -> Result<Vec<u8>> {
    let bytes = ureq::get(&asset.browser_download_url)
        .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("téléchargement impossible")?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()?;
    if let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("l'archive téléchargée est corrompue (empreinte SHA-256 différente)");
        }
    }
    Ok(bytes)
}

fn install(exe: &Path, asset: &Asset) -> Result<()> {
    let archive = download(asset)?;
    #[cfg(target_os = "macos")]
    if let Some(bundle) = app_bundle(exe) {
        return replace_bundle(&bundle, &archive);
    }
    let staging = tempdir(exe.parent().context("dossier de l'exécutable inconnu")?)?;
    let result = (|| {
        extract(&archive, &staging)?;
        let name = if cfg!(windows) { "ronnie.exe" } else { "ronnie" };
        let new = find(&staging, name).with_context(|| format!("{name} absent de l'archive"))?;
        self_replace::self_replace(&new).context("remplacement de l'exécutable impossible")
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// A fresh directory next to `near` (same filesystem, so moves are renames).
fn tempdir(near: &Path) -> Result<PathBuf> {
    let dir = near.join(format!(".ronnie-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).with_context(|| format!("impossible d'écrire dans {}", near.display()))?;
    Ok(dir)
}

fn extract(archive: &[u8], into: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        zip::ZipArchive::new(std::io::Cursor::new(archive))?.extract(into)?;
    }
    #[cfg(not(windows))]
    {
        tar::Archive::new(flate2::read::GzDecoder::new(archive)).unpack(into)?;
    }
    Ok(())
}

/// First file or directory called `name` under `dir`.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if entry.file_name() == name {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

/// The `.app` bundle containing `exe`, when running from one.
#[cfg(target_os = "macos")]
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// Swaps the whole bundle (executable, Info.plist, icon), rolling back if the new one can't be moved in.
#[cfg(target_os = "macos")]
fn replace_bundle(bundle: &Path, archive: &[u8]) -> Result<()> {
    let parent = bundle.parent().context("dossier de l'application inconnu")?;
    let staging = tempdir(parent)?;
    let result = (|| {
        extract(archive, &staging)?;
        let new = find(&staging, "Ronnie.app").context("Ronnie.app absent de l'archive")?;
        let old = staging.join("previous.app");
        std::fs::rename(bundle, &old).with_context(|| format!("impossible de remplacer {}", bundle.display()))?;
        if let Err(e) = std::fs::rename(&new, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(e).context("installation de la nouvelle version impossible");
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.10.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn finds_nested_file() {
        let dir = std::env::temp_dir().join(format!("ronnie-find-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/ronnie"), b"x").unwrap();
        assert_eq!(find(&dir, "ronnie"), Some(dir.join("a/b/ronnie")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
