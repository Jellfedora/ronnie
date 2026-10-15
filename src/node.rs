//! Node.js versions, installed in Ronnie's own folder (`<config dir>/node/versions/<v22.11.0>`), à la
//! nvm: nothing goes into the system. Each local pane can use one: its shell gets that version's
//! folder first in its PATH.
//!
//! How a pane gets it: the version's bin folder is written to a file of the pane
//! (`<config dir>/node/panes/<history id>`, see `pane_file`), which Ronnie's shell integration reads
//! when the shell starts, before each prompt and before each command line runs: a version changed
//! from Ronnie applies to the next command, without restarting the shell.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const DIST: &str = "https://nodejs.org/dist";
/// A Node.js archive is about 50 MB: more is not one.
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;

fn root() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("node"))
}

fn versions_dir() -> Option<PathBuf> {
    root().map(|d| d.join("versions"))
}

/// The file telling the shell of the pane with history `id` which Node.js to use (its bin folder;
/// none or empty: the system's).
pub fn pane_file(id: Uuid) -> Option<PathBuf> {
    root().map(|d| d.join("panes").join(id.to_string()))
}

/// The folder holding `node`, `npm` and `npx` of `version` ("v22.11.0").
pub fn bin_dir(version: &str) -> Option<PathBuf> {
    let dir = versions_dir()?.join(version);
    Some(if cfg!(windows) { dir } else { dir.join("bin") })
}

/// Makes the pane with history `id` use `version` (None: the system's), from its next command.
pub fn set_pane(id: Uuid, version: Option<&str>) {
    let Some(path) = pane_file(id) else { return };
    match version.filter(|v| is_installed(v)).and_then(bin_dir) {
        Some(bin) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, bin.to_string_lossy().as_bytes());
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// The bin folder the pane with history `id` uses, if any (for the PATH its shell starts with).
pub fn pane_bin(id: Uuid) -> Option<String> {
    let text = std::fs::read_to_string(pane_file(id)?).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Deletes the files of panes that no longer exist (not in `keep`).
pub fn forget_others(keep: &std::collections::HashSet<Uuid>) {
    let Some(dir) = root().map(|d| d.join("panes")) else { return };
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let id = entry.file_name().to_str().and_then(|n| Uuid::parse_str(n).ok());
        if id.is_some_and(|id| !keep.contains(&id)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn is_installed(version: &str) -> bool {
    safe_version(version) && bin_dir(version).is_some_and(|d| d.join(if cfg!(windows) { "node.exe" } else { "node" }).is_file())
}

/// The versions installed, the newest first.
pub fn installed() -> Vec<String> {
    let mut all: Vec<String> = std::fs::read_dir(versions_dir().unwrap_or_default())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|v| is_installed(v))
        .collect();
    all.sort_by_key(|v| std::cmp::Reverse(parse(v)));
    all
}

/// "v22.11.0" → (22, 11, 0).
pub fn parse(version: &str) -> (u64, u64, u64) {
    let mut parts = version.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// A version name that can't leave the versions' folder.
fn safe_version(version: &str) -> bool {
    version.starts_with('v') && version.len() > 1 && version[1..].chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Deletes an installed version.
pub fn uninstall(version: &str) -> Result<()> {
    if !safe_version(version) {
        bail!("invalid version {version}");
    }
    let dir = versions_dir().context("no config folder")?.join(version);
    std::fs::remove_dir_all(&dir).with_context(|| format!("cannot delete {}", dir.display()))
}

/// A version published on nodejs.org.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// Its release date, "2024-10-29".
    pub date: String,
    /// The LTS line's name ("Jod"), for long-term support versions.
    pub lts: Option<String>,
}

#[derive(Deserialize)]
struct IndexEntry {
    version: String,
    date: String,
    #[serde(default)]
    files: Vec<String>,
    lts: serde_json::Value,
}

/// (os, arch, the key nodejs.org's index lists the archive under, its extension), for this machine.
fn platform() -> Option<(&'static str, &'static str, String, &'static str)> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    if cfg!(target_os = "macos") {
        Some(("darwin", arch, format!("osx-{arch}-tar"), "tar.gz"))
    } else if cfg!(windows) {
        Some(("win", arch, format!("win-{arch}-zip"), "zip"))
    } else if cfg!(target_os = "linux") {
        Some(("linux", arch, format!("linux-{arch}"), "tar.gz"))
    } else {
        None
    }
}

fn agent(total: Duration) -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(15))).timeout_global(Some(total)).build().new_agent()
}

const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));

/// The versions published for this machine, the newest first.
pub fn releases() -> Result<Vec<Release>> {
    let (_, _, key, _) = platform().context("Node.js isn't published for this system")?;
    let index: Vec<IndexEntry> = agent(Duration::from_secs(30))
        .get(format!("{DIST}/index.json"))
        .header("User-Agent", USER_AGENT)
        .call()
        .context("nodejs.org unreachable")?
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_json()?;
    Ok(index
        .into_iter()
        .filter(|e| e.files.contains(&key) && safe_version(&e.version))
        .map(|e| Release { version: e.version, date: e.date, lts: e.lts.as_str().map(str::to_owned) })
        .collect())
}

/// How far an install went: bytes received and expected (0: unknown), then unpacking.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
    pub unpacking: bool,
}

/// Downloads `version`, checks it against nodejs.org's SHA-256 sums and installs it.
pub fn install(version: &str, progress: &Arc<Mutex<Progress>>, ctx: &egui::Context) -> Result<()> {
    if !safe_version(version) {
        bail!("invalid version {version}");
    }
    let (os, arch, _, ext) = platform().context("Node.js isn't published for this system")?;
    let name = format!("node-{version}-{os}-{arch}");
    let file = format!("{name}.{ext}");
    let agent = agent(Duration::from_secs(1800));
    let sums = agent
        .get(format!("{DIST}/{version}/SHASUMS256.txt"))
        .header("User-Agent", USER_AGENT)
        .call()
        .context("nodejs.org unreachable")?
        .body_mut()
        .read_to_string()?;
    let expected = sums.lines().find_map(|l| l.split_once("  ").filter(|(_, f)| *f == file).map(|(h, _)| h.to_owned())).with_context(|| format!("{file} isn't in the SHA-256 sums"))?;

    let mut response = agent.get(format!("{DIST}/{version}/{file}")).header("User-Agent", USER_AGENT).call().context("download failed")?;
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
        if shown.elapsed() > Duration::from_millis(150) {
            shown = std::time::Instant::now();
            *progress.lock().unwrap() = Progress { received: bytes.len() as u64, total, unpacking: false };
            ctx.request_repaint();
        }
    }
    *progress.lock().unwrap() = Progress { received: bytes.len() as u64, total, unpacking: true };
    ctx.request_repaint();
    let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("the downloaded archive is corrupted (SHA-256 mismatch)");
    }

    // Unpacked aside, then moved in place: a version is there whole or not at all.
    let versions = versions_dir().context("no config folder")?;
    std::fs::create_dir_all(&versions)?;
    let staging = versions.join(format!(".unpack-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)?;
    let result = (|| {
        unpack(&bytes, &staging)?;
        let unpacked = staging.join(&name);
        if !unpacked.is_dir() {
            bail!("{name} isn't in the archive");
        }
        let target = versions.join(version);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&unpacked, &target).with_context(|| format!("cannot write {}", target.display()))
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn unpack(archive: &[u8], into: &Path) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse("v22.11.0"), (22, 11, 0));
        assert!(parse("v22.11.0") > parse("v9.11.2"));
        assert!(safe_version("v20.1.0"));
        assert!(!safe_version("../v20"));
        assert!(!safe_version("v20/../.."));
        assert!(!safe_version("v"));
    }
}

#[cfg(test)]
mod live_tests {
    /// Downloads and installs a real version (network): `RONNIE_CONFIG_DIR=<dir> cargo test node_install -- --ignored`.
    #[test]
    #[ignore]
    fn node_install() {
        let releases = super::releases().unwrap();
        let lts = releases.iter().find(|r| r.lts.is_some()).unwrap();
        println!("{} releases, newest LTS {} ({:?})", releases.len(), lts.version, lts.lts);
        let progress = std::sync::Arc::new(std::sync::Mutex::new(super::Progress::default()));
        super::install(&lts.version, &progress, &egui::Context::default()).unwrap();
        assert!(super::installed().contains(&lts.version));
        let id = uuid::Uuid::new_v4();
        super::set_pane(id, Some(&lts.version));
        let bin = super::pane_bin(id).unwrap();
        let out = std::process::Command::new(std::path::Path::new(&bin).join("node")).arg("-v").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), lts.version);
        let out = std::process::Command::new(std::path::Path::new(&bin).join("npm")).arg("-v").env("PATH", format!("{bin}:/usr/bin:/bin")).output().unwrap();
        println!("npm {}", String::from_utf8_lossy(&out.stdout).trim());
        assert!(out.status.success());
    }
}
