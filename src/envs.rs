//! The environments the projects run on (see `crate::projects`), installed in Ronnie's own folder
//! (`<config dir>/env/<php|nginx>/<version>`), à la `crate::node`: nothing goes into the system.
//!
//! PHP comes from windows.php.net on Windows (its "non thread safe" builds, with `php-cgi`), from
//! static-php-cli's "bulk" builds elsewhere (a `php` and a `php-fpm` each, with the extensions
//! WordPress and PrestaShop want built in). nginx comes from nginx.org on Windows; elsewhere it isn't
//! published, the system's is used (`brew install nginx`, `apt install nginx`...).
//!
//! The databases: MySQL from mysql.com on Windows (its versions listed from its GitHub tags, its
//! download page shutting robots out), MariaDB from mariadb.org on Windows and Linux; elsewhere the
//! system's (`brew install mysql`...).

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::node::Progress;

/// A MySQL archive is some 300 MB, the others 10 to 100: more is not one.
const MAX_DOWNLOAD: u64 = 800 * 1024 * 1024;
const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));
const PHP_WINDOWS: &str = "https://windows.php.net/downloads/releases";
const STATIC_PHP: &str = "https://dl.static-php.dev/static-php-cli/bulk";
const NGINX: &str = "https://nginx.org";
const MYSQL_TAGS: &str = "https://api.github.com/repos/mysql/mysql-server/git/matching-refs/tags/mysql-";
const MYSQL_CDN: &str = "https://cdn.mysql.com";
const MARIADB: &str = "https://downloads.mariadb.org/rest-api/mariadb";
/// The version name of the system's program (where none is published for it).
pub const SYSTEM: &str = "system";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Php,
    Nginx,
    Mysql,
    Mariadb,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Php, Kind::Nginx, Kind::Mysql, Kind::Mariadb];
    pub const DATABASES: [Kind; 2] = [Kind::Mysql, Kind::Mariadb];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Php => "PHP",
            Kind::Nginx => "nginx",
            Kind::Mysql => "MySQL",
            Kind::Mariadb => "MariaDB",
        }
    }

    /// Where its versions are downloaded from.
    pub fn source(self) -> &'static str {
        match self {
            Kind::Php if cfg!(windows) => "windows.php.net",
            Kind::Php => "static-php.dev",
            Kind::Nginx => "nginx.org",
            Kind::Mysql => "mysql.com",
            Kind::Mariadb => "mariadb.org",
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Kind::Php => "php",
            Kind::Nginx => "nginx",
            Kind::Mysql => "mysql",
            Kind::Mariadb => "mariadb",
        }
    }

    /// Its server's program on the system's PATH.
    fn system_name(self) -> &'static str {
        match self {
            Kind::Php => "php-fpm",
            Kind::Nginx => "nginx",
            Kind::Mysql => "mysqld",
            Kind::Mariadb => "mariadbd",
        }
    }
}

fn versions_dir(kind: Kind) -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("env").join(kind.folder()))
}

/// Where `version` of `kind` is installed.
pub fn dir(kind: Kind, version: &str) -> Option<PathBuf> {
    safe_version(version).then(|| versions_dir(kind).map(|d| d.join(version))).flatten()
}

/// A version name that can't leave the versions' folder: "8.3.36-1", "1.30.6".
fn safe_version(version: &str) -> bool {
    version.chars().next().is_some_and(|c| c.is_ascii_digit()) && version.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') && !version.contains("..")
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

/// PHP's command line program of `version`.
pub fn php_cli(version: &str) -> Option<PathBuf> {
    dir(Kind::Php, version).map(|d| d.join(exe("php"))).filter(|p| p.is_file())
}

/// What serves PHP to nginx for `version`: `php-cgi` on Windows (no PHP-FPM there), `php-fpm`
/// elsewhere.
pub fn php_server(version: &str) -> Option<PathBuf> {
    dir(Kind::Php, version).map(|d| d.join(exe(if cfg!(windows) { "php-cgi" } else { "php-fpm" }))).filter(|p| p.is_file())
}

/// nginx's program of `version` (`SYSTEM`: the one installed on the system).
pub fn nginx(version: &str) -> Option<PathBuf> {
    if version == SYSTEM {
        return system_nginx();
    }
    dir(Kind::Nginx, version).map(|d| d.join(exe("nginx"))).filter(|p| p.is_file())
}

/// The system's nginx, where nginx.org publishes none.
pub fn system_nginx() -> Option<PathBuf> {
    system_program(Kind::Nginx.system_name())
}

/// A program of the system (outside Windows, where none is used): on the PATH, or where Homebrew
/// and the Linux packages put it.
pub fn system_program(name: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/local/sbin", "/usr/bin", "/usr/sbin"].map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// A program of a database's version: in its `bin` (or `scripts`) folder, the first of `names` there
/// (MariaDB renamed its programs, keeping the old names a while); for the system's, on the PATH.
fn db_program(kind: Kind, version: &str, names: &[&str]) -> Option<PathBuf> {
    if version == SYSTEM {
        return names.iter().find_map(|n| system_program(n));
    }
    let dir = dir(kind, version)?;
    names.iter().flat_map(|n| [dir.join("bin").join(exe(n)), dir.join("scripts").join(n)]).find(|p| p.is_file())
}

/// A database's server.
pub fn db_server(kind: Kind, version: &str) -> Option<PathBuf> {
    match kind {
        Kind::Mariadb => db_program(kind, version, &["mariadbd", "mysqld"]),
        _ => db_program(kind, version, &["mysqld"]),
    }
}

/// What stops a database's server cleanly.
pub fn db_admin(kind: Kind, version: &str) -> Option<PathBuf> {
    match kind {
        Kind::Mariadb => db_program(kind, version, &["mariadb-admin", "mysqladmin"]),
        _ => db_program(kind, version, &["mysqladmin"]),
    }
}

/// A database server's client (`mysql`, `mariadb`).
pub fn db_client(kind: Kind, version: &str) -> Option<PathBuf> {
    match kind {
        Kind::Mariadb => db_program(kind, version, &["mariadb", "mysql"]),
        _ => db_program(kind, version, &["mysql"]),
    }
}

/// What makes MariaDB's first data (MySQL's server does it itself).
pub fn mariadb_install_db(version: &str) -> Option<PathBuf> {
    db_program(Kind::Mariadb, version, &["mariadb-install-db", "mysql_install_db"])
}

pub fn is_installed(kind: Kind, version: &str) -> bool {
    match kind {
        Kind::Php => php_server(version).is_some() && php_cli(version).is_some(),
        Kind::Nginx => nginx(version).is_some(),
        Kind::Mysql | Kind::Mariadb => db_server(kind, version).is_some(),
    }
}

/// The versions installed, the newest first (nginx: the system's last, if there is one).
pub fn installed(kind: Kind) -> Vec<String> {
    let mut all: Vec<String> = std::fs::read_dir(versions_dir(kind).unwrap_or_default())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|v| safe_version(v) && is_installed(kind, v))
        .collect();
    all.sort_by_key(|v| std::cmp::Reverse(parse(v)));
    if kind != Kind::Php && !cfg!(windows) && is_installed(kind, SYSTEM) {
        all.push(SYSTEM.to_owned());
    }
    all
}

/// "8.3.36-1" → (8, 3, 36, 1).
pub fn parse(version: &str) -> (u64, u64, u64, u64) {
    let mut parts = version.split(['.', '-']).map(|p| p.parse().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// "8.3.36-1" → "8.3".
pub fn line(version: &str) -> String {
    let (major, minor, _, _) = parse(version);
    format!("{major}.{minor}")
}

/// Deletes an installed version.
pub fn uninstall(kind: Kind, version: &str) -> Result<()> {
    let dir = dir(kind, version).with_context(|| format!("invalid version {version}"))?;
    std::fs::remove_dir_all(&dir).with_context(|| format!("cannot delete {}", dir.display()))
}

/// A version published, to install.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// What it is: "8.3", "stable", "mainline".
    pub note: String,
    /// The files to download (relative to the source), with their SHA-256 when it is published.
    files: Vec<(String, Option<String>)>,
}

fn agent(total: Duration) -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(15))).timeout_global(Some(total)).build().new_agent()
}

fn get_text(url: &str) -> Result<String> {
    Ok(agent(Duration::from_secs(30)).get(url).header("User-Agent", USER_AGENT).call().with_context(|| format!("{url} unreachable"))?.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string()?)
}

/// The versions published for this machine, the newest first.
pub fn releases(kind: Kind) -> Result<Vec<Release>> {
    let mut all = match kind {
        Kind::Php if cfg!(windows) => php_windows_releases()?,
        Kind::Php => php_static_releases()?,
        Kind::Nginx if cfg!(windows) => nginx_releases()?,
        Kind::Nginx => bail!("nginx.org publishes nginx for Windows only: install it on the system (brew install nginx, apt install nginx...), Ronnie uses it"),
        Kind::Mysql if cfg!(windows) => mysql_releases()?,
        Kind::Mysql => bail!("Ronnie installs MySQL on Windows only: install it on the system (brew install mysql, apt install mysql-server...), Ronnie uses it"),
        Kind::Mariadb => mariadb_releases()?,
    };
    all.sort_by_key(|r| std::cmp::Reverse(parse(&r.version)));
    Ok(all)
}

#[derive(Deserialize)]
struct PhpZip {
    path: String,
    sha256: String,
}

/// windows.php.net's newest release of each PHP line, its 64-bit "non thread safe" build.
fn php_windows_releases() -> Result<Vec<Release>> {
    let text = get_text(&format!("{PHP_WINDOWS}/releases.json"))?;
    let lines: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).context("windows.php.net's list unreadable")?;
    let mut out = Vec::new();
    for (line, entry) in lines {
        let Some(version) = entry["version"].as_str().filter(|v| safe_version(v)) else { continue };
        let Some(obj) = entry.as_object() else { continue };
        let Some(zip) = obj.iter().find(|(k, _)| k.starts_with("nts-") && k.ends_with("-x64")).and_then(|(_, v)| serde_json::from_value::<PhpZip>(v["zip"].clone()).ok()) else { continue };
        out.push(Release { version: version.to_owned(), note: line, files: vec![(zip.path, Some(zip.sha256))] });
    }
    Ok(out)
}

/// static-php-cli's platform name and architecture for this machine.
fn static_platform() -> Option<(&'static str, &'static str)> {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return None,
    };
    Some((os, arch))
}

#[derive(Deserialize)]
struct Listed {
    name: String,
}

/// static-php-cli's newest release of each PHP line from 8.1, with both its `php` and `php-fpm`.
fn php_static_releases() -> Result<Vec<Release>> {
    let (os, arch) = static_platform().context("PHP isn't published for this system")?;
    let listed: Vec<Listed> = serde_json::from_str(&get_text(&format!("{STATIC_PHP}/?format=json"))?).context("static-php.dev's list unreadable")?;
    let names: std::collections::HashSet<&str> = listed.iter().map(|l| l.name.as_str()).collect();
    let mut newest: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for l in &listed {
        let Some(version) = l.name.strip_prefix("php-").and_then(|n| n.strip_suffix(&format!("-fpm-{os}-{arch}.tar.gz"))) else { continue };
        if !safe_version(version) || parse(version) < (8, 1, 0, 0) || !names.contains(format!("php-{version}-cli-{os}-{arch}.tar.gz").as_str()) {
            continue;
        }
        let entry = newest.entry(line(version)).or_insert_with(|| version.to_owned());
        if parse(version) > parse(entry) {
            *entry = version.to_owned();
        }
    }
    Ok(newest
        .into_iter()
        .map(|(line, version)| {
            let files = ["cli", "fpm"].map(|part| (format!("php-{version}-{part}-{os}-{arch}.tar.gz"), None)).to_vec();
            Release { version, note: line, files }
        })
        .collect())
}

/// nginx.org's Windows builds, from its download page: the stable lines (even) and the mainline.
fn nginx_releases() -> Result<Vec<Release>> {
    let page = get_text(&format!("{NGINX}/en/download.html"))?;
    let mut seen = std::collections::BTreeSet::new();
    for part in page.split("/download/nginx-").skip(1) {
        if let Some(version) = part.split(".zip").next().filter(|v| safe_version(v) && v.len() < 12) {
            seen.insert(version.to_owned());
        }
    }
    Ok(seen
        .into_iter()
        .map(|version| {
            let note = if parse(&version).1.is_multiple_of(2) { "stable" } else { "mainline" }.to_owned();
            Release { files: vec![(format!("nginx-{version}.zip"), None)], version, note }
        })
        .collect())
}

#[derive(Deserialize)]
struct GitRef {
    #[serde(rename = "ref")]
    name: String,
}

/// MySQL's newest release of its long-term lines (8.0, 8.4, 9.7...) and of its newest line, from its
/// GitHub tags; the archive's place (`MySQL-8.4/mysql-8.4.13-winx64.zip`) under the CDN's
/// `Downloads`, or its `archives` once superseded.
fn mysql_releases() -> Result<Vec<Release>> {
    let refs: Vec<GitRef> = serde_json::from_str(&get_text(MYSQL_TAGS)?).context("MySQL's list unreadable")?;
    let mut newest: std::collections::BTreeMap<(u64, u64), String> = std::collections::BTreeMap::new();
    for r in &refs {
        let Some(version) = r.name.strip_prefix("refs/tags/mysql-").filter(|v| safe_version(v) && !v.contains('-')) else { continue };
        let (major, minor, _, _) = parse(version);
        if major < 8 {
            continue;
        }
        let entry = newest.entry((major, minor)).or_insert_with(|| version.to_owned());
        if parse(version) > parse(entry) {
            *entry = version.to_owned();
        }
    }
    let last = newest.keys().next_back().copied();
    // The long-term lines: 8.0, then the ".4" of 8 and the ".7" of the others (9.7, 26.7...).
    let lts = |(major, minor): (u64, u64)| (major, minor) == (8, 0) || (major == 8 && minor == 4) || (major > 8 && minor == 7);
    Ok(newest
        .into_iter()
        .filter(|(line, _)| lts(*line) || Some(*line) == last)
        .map(|((major, minor), version)| {
            let note = if lts((major, minor)) { "LTS" } else { "innovation" }.to_owned();
            Release { files: vec![(format!("MySQL-{major}.{minor}/mysql-{version}-winx64.zip"), None)], version, note }
        })
        .collect())
}

#[derive(Deserialize)]
struct MariadbMajors {
    major_releases: Vec<MariadbMajor>,
}

#[derive(Deserialize)]
struct MariadbMajor {
    release_id: String,
    release_status: String,
    release_support_type: Option<String>,
}

#[derive(Deserialize)]
struct MariadbLatest {
    releases: std::collections::BTreeMap<String, MariadbRelease>,
}

#[derive(Deserialize)]
struct MariadbRelease {
    files: Vec<MariadbFile>,
}

#[derive(Deserialize)]
struct MariadbFile {
    file_name: String,
    file_download_url: String,
    checksum: MariadbChecksum,
}

#[derive(Deserialize)]
struct MariadbChecksum {
    sha256sum: Option<String>,
}

/// MariaDB's newest release of each stable line, its archive for this machine (Windows, Linux).
fn mariadb_releases() -> Result<Vec<Release>> {
    let suffix = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "-winx64.zip",
        ("linux", "x86_64") => "-linux-systemd-x86_64.tar.gz",
        _ => bail!("mariadb.org publishes MariaDB for Windows and Linux only: install it on the system (brew install mariadb), Ronnie uses it"),
    };
    let majors: MariadbMajors = serde_json::from_str(&get_text(&format!("{MARIADB}/"))?).context("mariadb.org's list unreadable")?;
    let mut out = Vec::new();
    for major in majors.major_releases.into_iter().filter(|m| m.release_status == "Stable") {
        let Ok(text) = get_text(&format!("{MARIADB}/{}/latest/", major.release_id)) else { continue };
        let Ok(latest) = serde_json::from_str::<MariadbLatest>(&text) else { continue };
        for (version, release) in latest.releases {
            let Some(file) = release.files.into_iter().find(|f| f.file_name.ends_with(suffix)) else { continue };
            if !safe_version(&version) {
                continue;
            }
            let note = if major.release_support_type.as_deref() == Some("Long Term Support") { "LTS" } else { "stable" }.to_owned();
            out.push(Release { version, note, files: vec![(file.file_download_url.replacen("http://", "https://", 1), file.checksum.sha256sum)] });
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct MariadbOne {
    release_data: std::collections::BTreeMap<String, MariadbRelease>,
}

/// A database's exact version, to install (the one WAMP has): MySQL's from its CDN, MariaDB's from
/// mariadb.org (Windows).
pub fn release_of(kind: Kind, version: &str) -> Result<Release> {
    if !safe_version(version) {
        bail!("invalid version {version}");
    }
    let note = "WAMP".to_owned();
    match kind {
        Kind::Mysql => {
            let (major, minor, _, _) = parse(version);
            Ok(Release { version: version.to_owned(), note, files: vec![(format!("MySQL-{major}.{minor}/mysql-{version}-winx64.zip"), None)] })
        }
        Kind::Mariadb => {
            let one: MariadbOne = serde_json::from_str(&get_text(&format!("{MARIADB}/{version}/"))?).context("mariadb.org's answer unreadable")?;
            let file = one.release_data.into_values().flat_map(|r| r.files).find(|f| f.file_name.ends_with("-winx64.zip")).with_context(|| format!("MariaDB {version} isn't published for Windows"))?;
            Ok(Release { version: version.to_owned(), note, files: vec![(file.file_download_url.replacen("http://", "https://", 1), file.checksum.sha256sum)] })
        }
        _ => bail!("{} isn't a database", kind.name()),
    }
}

/// Downloads `url` whole (at most `MAX_DOWNLOAD`), telling how far it went from `before` bytes on
/// (the files of a version add up).
fn download(agent: &ureq::Agent, url: &str, progress: &Arc<Mutex<Progress>>, ctx: &egui::Context, before: u64, total_hint: u64) -> Result<Vec<u8>> {
    let mut response = agent.get(url).header("User-Agent", USER_AGENT).call().with_context(|| format!("download failed: {url}"))?;
    let size: u64 = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    let total = total_hint.max(before + size);
    let mut reader = response.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
    let mut bytes = Vec::with_capacity(size.min(MAX_DOWNLOAD) as usize);
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
            *progress.lock().unwrap() = Progress { received: before + bytes.len() as u64, total, unpacking: false };
            ctx.request_repaint();
        }
    }
    Ok(bytes)
}

/// Downloads `release` of `kind`, checks it against its published SHA-256 if there is one, and
/// installs it.
pub fn install(kind: Kind, release: &Release, progress: &Arc<Mutex<Progress>>, ctx: &egui::Context) -> Result<()> {
    let version = &release.version;
    if !safe_version(version) {
        bail!("invalid version {version}");
    }
    let agent = agent(Duration::from_secs(1800));
    let mut archives = Vec::new();
    let mut received = 0;
    for (file, sha256) in &release.files {
        let bytes = match kind {
            // A release superseded moves to the archives.
            Kind::Php if cfg!(windows) => download(&agent, &format!("{PHP_WINDOWS}/{file}"), progress, ctx, received, 0).or_else(|_| download(&agent, &format!("{PHP_WINDOWS}/archives/{file}"), progress, ctx, received, 0))?,
            Kind::Php => download(&agent, &format!("{STATIC_PHP}/{file}"), progress, ctx, received, 0)?,
            Kind::Nginx => download(&agent, &format!("{NGINX}/download/{file}"), progress, ctx, received, 0)?,
            Kind::Mysql => download(&agent, &format!("{MYSQL_CDN}/Downloads/{file}"), progress, ctx, received, 0)
                .or_else(|_| download(&agent, &format!("{MYSQL_CDN}/archives/{}", file.to_lowercase()), progress, ctx, received, 0))?,
            Kind::Mariadb => download(&agent, file, progress, ctx, received, 0)?,
        };
        if let Some(expected) = sha256 {
            let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
            if !actual.eq_ignore_ascii_case(expected) {
                bail!("the downloaded archive is corrupted (SHA-256 mismatch)");
            }
        }
        received += bytes.len() as u64;
        archives.push((file.clone(), bytes));
    }
    *progress.lock().unwrap() = Progress { received, total: received, unpacking: true };
    ctx.request_repaint();

    // Unpacked aside, then moved in place: a version is there whole or not at all.
    let versions = versions_dir(kind).context("no config folder")?;
    std::fs::create_dir_all(&versions)?;
    let staging = versions.join(format!(".unpack-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)?;
    let result = (|| {
        for (file, bytes) in &archives {
            if file.ends_with(".zip") {
                zip::ZipArchive::new(std::io::Cursor::new(bytes))?.extract(&staging)?;
            } else {
                tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..])).unpack(&staging)?;
            }
        }
        // nginx's and the databases' archives hold a folder ("nginx-1.30.6", "mysql-8.4.13-winx64");
        // PHP's are their files.
        let only: Vec<PathBuf> = std::fs::read_dir(&staging)?.flatten().map(|e| e.path()).collect();
        let unpacked = match only.as_slice() {
            [one] if one.is_dir() => one.clone(),
            _ => staging.clone(),
        };
        let target = versions.join(version);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&unpacked, &target).with_context(|| format!("cannot write {}", target.display()))?;
        if !is_installed(kind, version) {
            let _ = std::fs::remove_dir_all(&target);
            bail!("{} {version}: the archive doesn't hold what was expected", kind.name());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// The installed version of PHP's line `line` ("8.3"), the newest of it.
pub fn php_of_line(line_wanted: &str) -> Option<String> {
    installed(Kind::Php).into_iter().find(|v| line(v) == line_wanted)
}

/// A folder written with forward slashes (nginx and PHP read them on Windows too).
pub fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(safe_version("8.3.36-1") && safe_version("1.30.6"));
        assert!(!safe_version("../8.3") && !safe_version("8..3") && !safe_version("") && !safe_version("system"));
        assert!(parse("8.3.36-1") > parse("8.3.36") && parse("8.10.0") > parse("8.9.9"));
        assert_eq!(line("8.3.36-1"), "8.3");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// The lists published (network): `cargo test envs_releases -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn envs_releases() {
        for kind in Kind::ALL {
            match releases(kind) {
                Ok(r) => println!("{}: {}", kind.name(), r.iter().map(|r| format!("{} ({})", r.version, r.note)).collect::<Vec<_>>().join(", ")),
                Err(e) => println!("{}: {e:#}", kind.name()),
            }
        }
    }
}
