//! Updates from GitHub releases. A background check finds a newer release; once the user accepts, the
//! archive built for this platform is downloaded, checked against the digest GitHub publishes, and
//! swapped in place of the running app, which then restarts.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "Jellfedora/ronnie";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Target in the release archive names. macOS: Apple Silicon since 0.9.1 (up to 0.9.0, a universal
/// app, whose archive name the releases still carry for those versions).
const TARGET: &str = if cfg!(target_os = "macos") { "aarch64-apple-darwin" } else { env!("TARGET") };
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
    /// The AppImage file Ronnie runs from, if so: the update replaces it, not the (read-only) executable.
    appimage: Option<PathBuf>,
    /// Bytes downloaded so far, and how many there are (0: unknown).
    progress: Arc<Mutex<(u64, u64)>>,
}

/// The AppImage file this process runs from. Its runtime says so in APPIMAGE and APPDIR; a program
/// started from Ronnie's terminal inherits them, hence the check that the executable is in APPDIR.
pub fn appimage() -> Option<PathBuf> {
    let (file, dir) = (std::env::var_os("APPIMAGE")?, std::env::var_os("APPDIR")?);
    let exe = std::env::current_exe().ok()?;
    exe.starts_with(&dir).then(|| PathBuf::from(file))
}

/// Release file for this platform: an AppImage replaces an AppImage, the archive the other installs.
fn asset_name(appimage: bool) -> String {
    if appimage { format!("Ronnie-{}.AppImage", std::env::consts::ARCH) } else { format!("ronnie-{TARGET}.{ARCHIVE_EXT}") }
}

impl Updater {
    pub fn new() -> Self {
        Self { state: Arc::default(), exe: std::env::current_exe().ok(), appimage: appimage(), progress: Arc::default() }
    }

    /// How far the download of an update went: bytes received, and the total (0: unknown).
    pub fn progress(&self) -> (u64, u64) {
        *self.progress.lock().unwrap()
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
        let (state, ctx, appimage) = (self.state.clone(), ctx.clone(), self.appimage.is_some());
        thread::spawn(move || {
            let result = match latest(appimage) {
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
        let (state, ctx, appimage) = (self.state.clone(), ctx.clone(), self.appimage.clone());
        let progress = Progress { shared: self.progress.clone(), ctx: ctx.clone() };
        *progress.shared.lock().unwrap() = (0, 0);
        thread::spawn(move || {
            let installed = match &appimage {
                Some(file) => replace_appimage(file, &available.asset, &progress),
                None => install(&exe, &available.asset, &progress),
            };
            let result = match installed {
                Ok(()) => State::Installed(available.version),
                Err(e) => State::Failed(format!("{e:#}")),
            };
            *state.lock().unwrap() = result;
            ctx.request_repaint();
        });
    }

    /// Starts the (updated) app again; the caller then closes this instance.
    pub fn relaunch(&self) -> Result<()> {
        if let Some(file) = &self.appimage {
            return start_after_exit(file, &[]);
        }
        let exe = self.exe.as_deref().context("unknown executable path")?;
        #[cfg(target_os = "macos")]
        if let Some(bundle) = app_bundle(exe) {
            return start_after_exit(Path::new("/usr/bin/open"), &[std::ffi::OsStr::new("-n"), bundle.as_os_str()]);
        }
        start_after_exit(exe, &[])
    }
}

/// Starts `program` once this process has quit (or after 10 s, if it lingers): started while this one
/// still runs, the new instance could be taken for it (macOS just brings the quitting app forward) or
/// find its files still in use.
#[cfg(unix)]
fn start_after_exit(program: &Path, args: &[&std::ffi::OsStr]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    // Also notes in ronnie.log when it starts the new instance, and how long the old one took to quit.
    const SCRIPT: &str = r#"pid=$1; log=$2; shift 2; i=0
while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 100 ]; do sleep 0.1; i=$((i + 1)); done
[ -n "$log" ] && echo "$(date +%s) INFO restart: starting the new instance after $((i / 10)).$((i % 10)) s" >> "$log"
exec "$@""#;
    crate::log::info(&format!("restart: {} will start once this instance quits", program.display()));
    let log = crate::log::log_path().unwrap_or_default();
    Command::new("/bin/sh")
        .args(["-c", SCRIPT, "sh", &std::process::id().to_string()])
        .arg(log)
        .arg(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // Its own process group: a signal to this app's group (its terminal closing...) spares it.
        .process_group(0)
        .spawn()
        .with_context(|| format!("cannot start {}", program.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn start_after_exit(program: &Path, args: &[&std::ffi::OsStr]) -> Result<()> {
    Command::new(program).args(args).spawn()?;
    Ok(())
}

/// The latest release, if it is newer than this build and has an archive for this platform.
/// An HTTP client that gives up on a stalled connection instead of waiting forever.
fn agent(total: Duration) -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(15))).timeout_global(Some(total)).build().new_agent()
}

/// The latest release, if it is newer than this build. A release without an archive for this platform
/// is an error (reported) rather than "up to date".
fn latest(appimage: bool) -> Result<Option<Available>> {
    let release: Release = agent(Duration::from_secs(30))
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("GitHub unreachable")?
        .body_mut()
        .read_json()?;
    let version = release.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&version, VERSION) {
        return Ok(None);
    }
    let name = asset_name(appimage);
    let asset = release.assets.into_iter().find(|a| a.name == name).with_context(|| format!("version {version} has no {name} archive"))?;
    Ok(Some(Available { version, url: release.html_url, asset }))
}

/// (major, minor, patch, is a pre-release such as "1.2.0-beta").
fn parse_version(v: &str) -> Option<(u64, u64, u64, bool)> {
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((core, rest)) => (core, v.as_bytes()[core.len()] == b'-' && !rest.is_empty()),
        None => (v, false),
    };
    let mut parts = core.split('.').map(str::parse::<u64>);
    Some((parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?, pre))
}

fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        // Same numbers: the final release is newer than its pre-releases.
        (Some((a, b, c, pa)), Some((x, y, z, px))) => (a, b, c) > (x, y, z) || ((a, b, c) == (x, y, z) && px && !pa),
        _ => false,
    }
}

/// Where a download says how far it went (and wakes the window up to show it).
struct Progress {
    shared: Arc<Mutex<(u64, u64)>>,
    ctx: egui::Context,
}

fn download(asset: &Asset, progress: &Progress) -> Result<Vec<u8>> {
    use std::io::Read as _;
    // Without GitHub's digest the archive can't be checked: don't install it.
    let expected = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).context("the release publishes no SHA-256 digest for its archive")?;
    let mut response = agent(Duration::from_secs(900))
        .get(&asset.browser_download_url)
        .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("download failed")?;
    let total: u64 = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut reader = response.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
    let mut bytes = Vec::with_capacity(total.min(MAX_DOWNLOAD) as usize);
    let mut buf = vec![0u8; 64 * 1024];
    let mut shown = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf).context("download failed")?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if shown.elapsed() > Duration::from_millis(200) {
            shown = std::time::Instant::now();
            *progress.shared.lock().unwrap() = (bytes.len() as u64, total);
            progress.ctx.request_repaint();
        }
    }
    *progress.shared.lock().unwrap() = (bytes.len() as u64, total);
    let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("the downloaded archive is corrupted (SHA-256 mismatch)");
    }
    Ok(bytes)
}

fn install(exe: &Path, asset: &Asset, progress: &Progress) -> Result<()> {
    let archive = download(asset, progress)?;
    #[cfg(target_os = "macos")]
    if let Some(bundle) = app_bundle(exe) {
        return replace_bundle(&bundle, &archive);
    }
    let staging = tempdir(exe.parent().context("unknown executable directory")?)?;
    let result = (|| {
        extract(&archive, &staging)?;
        let name = if cfg!(windows) { "ronnie.exe" } else { "ronnie" };
        let new = find(&staging, name).with_context(|| format!("{name} absent de l'archive"))?;
        self_replace::self_replace(&new).context("could not replace the executable")
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Writes the new AppImage next to the running one, then renames it over: the running app keeps its
/// (still mounted) old file until it quits.
fn replace_appimage(file: &Path, asset: &Asset, progress: &Progress) -> Result<()> {
    let bytes = download(asset, progress)?;
    let dir = file.parent().context("unknown AppImage directory")?;
    let name = file.file_name().context("unknown AppImage name")?.to_string_lossy();
    let temp = dir.join(format!(".{name}.update-{}", std::process::id()));
    let result = (|| {
        let mut out = std::fs::File::create(&temp).with_context(|| format!("cannot write in {}", dir.display()))?;
        std::io::Write::write_all(&mut out, &bytes)?;
        out.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&temp, file).with_context(|| format!("impossible de remplacer {}", file.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// A fresh directory next to `near` (same filesystem, so moves are renames).
fn tempdir(near: &Path) -> Result<PathBuf> {
    let dir = near.join(format!(".ronnie-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).with_context(|| format!("cannot write in {}", near.display()))?;
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
    let parent = bundle.parent().context("unknown application directory")?;
    let staging = tempdir(parent)?;
    let result = (|| {
        extract(archive, &staging)?;
        let new = find(&staging, "Ronnie.app").context("Ronnie.app missing from the archive")?;
        let old = staging.join("previous.app");
        std::fs::rename(bundle, &old).with_context(|| format!("impossible de remplacer {}", bundle.display()))?;
        if let Err(e) = std::fs::rename(&new, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(e).context("could not install the new version");
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
        assert!(is_newer("0.2.0", "0.2.0-beta.1"), "final after its pre-release");
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(is_newer("0.2.1", "0.2.0+build5"));
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
