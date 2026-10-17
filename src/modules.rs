//! The modules: apps of their own (Serj, the comics reader…) that Ronnie installs on demand from its
//! Downloads page and starts in their own window. floor lists them (`/v1/modules`), so a new one shows
//! without a new Ronnie; each publishes its versions in its GitHub repository's releases, like Ronnie,
//! with an archive per system: `<id>-<target>.tar.gz` (`.zip` on Windows) holding `<app>` on macOS,
//! `<exe>` (`<exe>.exe`) elsewhere.
//!
//! Installed in `<config dir>/modules/<id>/`: `app/` (what the archive holds), `module.json` (the
//! catalog's entry, to list it offline), `version`, `icon.png`. Nothing outside Ronnie's folder.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::update::{self, ARCHIVE_EXT, Asset, TARGET};

/// A module, as floor's catalog describes it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Module {
    pub id: String,
    pub name: String,
    pub description: String,
    /// "owner/name" on GitHub, public: its releases are downloaded without an account.
    pub repo: String,
    /// The PNG shown on its card.
    #[serde(default)]
    pub icon: Option<String>,
    /// The bundle in the macOS archive ("Serj.app").
    pub app: String,
    /// The executable elsewhere ("serj").
    pub exe: String,
}

#[derive(Deserialize)]
struct Catalog {
    modules: Vec<Module>,
}

/// The newest release of a module, with the archive for this system.
#[derive(Clone)]
pub struct Latest {
    pub version: String,
    /// Its release page, with the changes.
    pub url: String,
    asset: Asset,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

/// Ids are folder names: nothing that could climb out of the modules folder.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

/// Where the modules are installed.
pub fn root() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("modules"))
}

fn dir(id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| root().map(|r| r.join(id))).flatten()
}

/// The modules floor offers (those whose fields make sense).
pub fn catalog() -> Result<Vec<Module>> {
    let url = crate::floor::url().context("no floor server")?;
    let catalog: Catalog = update::agent(Duration::from_secs(20))
        .get(format!("{url}/v1/modules"))
        .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("floor unreachable")?
        .body_mut()
        .read_json()?;
    Ok(catalog.modules.into_iter().filter(|m| valid_id(&m.id) && valid_name(&m.app) && valid_name(&m.exe) && m.repo.split('/').count() == 2).collect())
}

/// The archive name for this system.
fn asset_name(module: &Module) -> String {
    format!("{}-{TARGET}.{ARCHIVE_EXT}", module.id)
}

/// The newest release of `module` with an archive for this system; None when it has published none
/// (yet) for it.
pub fn latest(module: &Module) -> Result<Option<Latest>> {
    let response = update::agent(Duration::from_secs(20))
        .get(format!("https://api.github.com/repos/{}/releases/latest", module.repo))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
        .call();
    let mut response = match response {
        Ok(r) => r,
        // No release at all yet.
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e).context("GitHub unreachable"),
    };
    let release: Release = response.body_mut().read_json()?;
    let name = asset_name(module);
    let Some(asset) = release.assets.into_iter().find(|a| a.name == name) else { return Ok(None) };
    Ok(Some(Latest { version: release.tag_name.trim_start_matches('v').to_owned(), url: release.html_url, asset }))
}

/// The installed version of module `id`, if it is.
pub fn installed_version(id: &str) -> Option<String> {
    let dir = dir(id)?;
    let version = std::fs::read_to_string(dir.join("version")).ok()?.trim().to_owned();
    (dir.join("app").is_dir() && !version.is_empty()).then_some(version)
}

/// The modules installed, as the catalog described them when they were: listed even when floor
/// can't be reached.
pub fn installed() -> Vec<Module> {
    let Some(root) = root() else { return Vec::new() };
    let mut found: Vec<Module> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join("module.json")).ok())
        .filter_map(|bytes| serde_json::from_slice::<Module>(&bytes).ok())
        .filter(|m| valid_id(&m.id) && installed_version(&m.id).is_some())
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Downloads `latest` of `module` (checked against GitHub's SHA-256) and puts it in place of the
/// installed version, if any. `progress`: bytes received and the total.
pub fn install(module: &Module, latest: &Latest, progress: Arc<Mutex<(u64, u64)>>, ctx: &egui::Context) -> Result<()> {
    let dir = dir(&module.id).context("invalid module id")?;
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let archive = update::download(&latest.asset, &update::Progress { shared: progress, ctx: ctx.clone() })?;
    let staging = dir.join(".staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)?;
    let result = (|| {
        update::extract(&archive, &staging)?;
        // What Ronnie starts must be there, or the archive isn't the module's.
        let wanted = if cfg!(target_os = "macos") { module.app.clone() } else { exe_name(module) };
        let found = update::find(&staging, &wanted).with_context(|| format!("{wanted} missing from the archive"))?;
        #[cfg(unix)]
        if !cfg!(target_os = "macos") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&found, std::fs::Permissions::from_mode(0o755))?;
        }
        let _ = found;
        // The old version out, the new one in, the old one deleted.
        let app = dir.join("app");
        let old = dir.join(".previous");
        let _ = std::fs::remove_dir_all(&old);
        if app.exists() {
            std::fs::rename(&app, &old).context("the installed version is in use")?;
        }
        if let Err(e) = std::fs::rename(&staging, &app) {
            let _ = std::fs::rename(&old, &app);
            return Err(e).context("could not install the new version");
        }
        let _ = std::fs::remove_dir_all(&old);
        std::fs::write(dir.join("version"), &latest.version)?;
        std::fs::write(dir.join("module.json"), serde_json::to_vec_pretty(module)?)?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Deletes module `id` (its own settings, kept by the module elsewhere, stay).
pub fn uninstall(id: &str) -> Result<()> {
    let dir = dir(id).context("invalid module id")?;
    std::fs::remove_dir_all(&dir).with_context(|| format!("cannot delete {}", dir.display()))
}

fn exe_name(module: &Module) -> String {
    if cfg!(windows) { format!("{}.exe", module.exe) } else { module.exe.clone() }
}

/// Starts module `module` in its own window, apart from Ronnie (its own process group: closing Ronnie
/// leaves it open).
pub fn launch(module: &Module) -> Result<()> {
    let app = dir(&module.id).context("invalid module id")?.join("app");
    #[cfg(target_os = "macos")]
    {
        let bundle = update::find(&app, &module.app).with_context(|| format!("{} is not installed", module.name))?;
        // `open`: the app shows in the Dock with its icon, and comes forward if already running.
        let status = Command::new("/usr/bin/open").arg(&bundle).status()?;
        if !status.success() {
            bail!("{} did not start", module.name);
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let exe = update::find(&app, &exe_name(module)).with_context(|| format!("{} is not installed", module.name))?;
        let mut command = Command::new(&exe);
        command.current_dir(exe.parent().unwrap_or(Path::new("."))).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command.spawn().with_context(|| format!("{} did not start", module.name))?;
        Ok(())
    }
}

/// The module's icon (PNG), from its folder or else downloaded (and kept there when installed).
pub fn icon(module: &Module) -> Option<Vec<u8>> {
    let cached = dir(&module.id).map(|d| d.join("icon.png"));
    if let Some(bytes) = cached.as_ref().and_then(|p| std::fs::read(p).ok()) {
        return Some(bytes);
    }
    let url = module.icon.as_deref().filter(|u| u.starts_with("https://"))?;
    let mut response = update::agent(Duration::from_secs(20)).get(url).header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION"))).call().ok()?;
    let bytes = response.body_mut().with_config().limit(4 * 1024 * 1024).read_to_vec().ok()?;
    if !bytes.starts_with(b"\x89PNG") {
        return None;
    }
    if let Some(path) = cached.filter(|p| p.parent().is_some_and(Path::is_dir)) {
        let _ = std::fs::write(path, &bytes);
    }
    Some(bytes)
}

/// Whether `latest` is newer than the installed `version`.
pub fn is_update(latest: &Latest, installed: &str) -> bool {
    update::is_newer(&latest.version, installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_stay_in_their_folder() {
        assert!(valid_id("serj"));
        assert!(valid_id("ronnheim-2"));
        assert!(!valid_id(""));
        assert!(!valid_id("../x"));
        assert!(!valid_id("Serj"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(".."));
        assert!(valid_name("Serj.app"));
    }

    /// The whole way, on the network: floor's catalog, the GitHub release, the download checked,
    /// installed in a temporary config folder. `cargo test -- --ignored installs_serj`.
    #[test]
    #[ignore]
    fn installs_serj() {
        let dir = std::env::temp_dir().join(format!("ronnie-modules-{}", std::process::id()));
        // SAFETY: set before any thread reads it, in a test run on its own.
        unsafe {
            std::env::set_var("RONNIE_CONFIG_DIR", &dir);
            std::env::set_var("RONNIE_FLOOR_URL", "https://fedoheim.hopto.org/floor");
        }
        let serj = catalog().unwrap().into_iter().find(|m| m.id == "serj").expect("serj in the catalog");
        let latest = latest(&serj).unwrap().expect("a release with this system's archive");
        let progress = Arc::new(Mutex::new((0, 0)));
        install(&serj, &latest, progress.clone(), &egui::Context::default()).unwrap();
        assert_eq!(installed_version("serj").as_deref(), Some(latest.version.as_str()));
        assert_eq!(installed().len(), 1);
        let wanted = if cfg!(target_os = "macos") { serj.app.clone() } else { exe_name(&serj) };
        let found = update::find(&dir.join("modules/serj/app"), &wanted).expect("installed app");
        let exe = if cfg!(target_os = "macos") { found.join("Contents/MacOS").join(&serj.exe) } else { found };
        let out = Command::new(exe).arg("--version").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("serj {}", latest.version));
        assert!(icon(&serj).is_some_and(|b| b.starts_with(b"\x89PNG")));
        uninstall("serj").unwrap();
        assert!(installed_version("serj").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_the_catalog() {
        let json = r#"{"modules":[{"id":"serj","name":"Serj","description":"BD","repo":"Jellfedora/serj","icon":"https://x/icon.png","app":"Serj.app","exe":"serj"}]}"#;
        let catalog: Catalog = serde_json::from_str(json).unwrap();
        assert_eq!(catalog.modules[0].app, "Serj.app");
        assert_eq!(asset_name(&catalog.modules[0]), format!("serj-{TARGET}.{ARCHIVE_EXT}"));
    }
}
