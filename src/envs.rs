//! The environments the projects run on (see `crate::projects`), installed in Ronnie's own folder
//! (`<config dir>/env/<php|nginx>/<version>`), à la `crate::node`: nothing goes into the system.
//!
//! PHP comes from windows.php.net on Windows (its "non thread safe" builds, with `php-cgi`), from
//! static-php-cli's "bulk" builds elsewhere (a `php` and a `php-fpm` each, with the extensions
//! WordPress and PrestaShop want built in). nginx comes from nginx.org on Windows, from
//! jirutka/nginx-binaries on Apple's Macs (its PCRE2, the only library it needs, taken from Homebrew's
//! bottle and put beside it); elsewhere it isn't published, the system's is used (`apt install
//! nginx`...).
//!
//! The databases: MySQL from mysql.com on Windows and macOS (its versions listed from its GitHub tags,
//! its download page shutting robots out), MariaDB from mariadb.org on Windows and Linux; elsewhere
//! the system's (`brew install mariadb`, `apt install mysql-server`...): see `system_only`.

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
const NGINX_MAC: &str = "https://jirutka.github.io/nginx-binaries";
const BREW_API: &str = "https://formulae.brew.sh/api/formula";
const MYSQL_TAGS: &str = "https://api.github.com/repos/mysql/mysql-server/git/matching-refs/tags/mysql-";
const MYSQL_CDN: &str = "https://cdn.mysql.com";
const MARIADB: &str = "https://downloads.mariadb.org/rest-api/mariadb";
/// In a MySQL archive's name for macOS, the system it is built for ("macos15"): not in its list,
/// tried at the download.
const MACOS: &str = "{macos}";
/// The PCRE2 library nginx's Mac builds are linked against (Homebrew's place), and its name.
const PCRE2_BREW: &str = "/opt/homebrew/opt/pcre2/lib/";
const PCRE2_LIB: &str = "libpcre2-8.0.dylib";
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
            Kind::Nginx if cfg!(target_os = "macos") => "jirutka/nginx-binaries",
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

/// Whether this is one of Apple's Macs (nginx is published for them, not for Intel's).
fn apple_silicon() -> bool {
    cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64"
}

/// When nothing of `kind` is published for this machine (the system's is used): how to install it on
/// the system.
pub fn system_only(kind: Kind) -> Option<&'static str> {
    let (mac, linux_x64) = (cfg!(target_os = "macos"), cfg!(target_os = "linux") && std::env::consts::ARCH == "x86_64");
    match kind {
        _ if cfg!(windows) => None,
        Kind::Php => None,
        Kind::Nginx if apple_silicon() => None,
        Kind::Nginx => Some(if mac { "brew install nginx" } else { "sudo apt install nginx" }),
        Kind::Mysql if mac => None,
        Kind::Mysql => Some("sudo apt install mysql-server"),
        Kind::Mariadb if linux_x64 => None,
        Kind::Mariadb => Some(if mac { "brew install mariadb" } else { "sudo apt install mariadb-server" }),
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
    if let Some(how) = system_only(kind) {
        bail!("{} isn't published for this system: install it on the system ({how}), Ronnie uses it", kind.name());
    }
    let mut all = match kind {
        Kind::Php if cfg!(windows) => php_windows_releases()?,
        Kind::Php => php_static_releases()?,
        Kind::Nginx if cfg!(windows) => nginx_releases()?,
        Kind::Nginx => nginx_mac_releases()?,
        Kind::Mysql => mysql_releases()?,
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
struct NginxBinaries {
    contents: Vec<NginxBinary>,
}

#[derive(Deserialize)]
struct NginxBinary {
    name: String,
    version: String,
    variant: String,
    arch: String,
    os: String,
    filename: String,
    /// "sha1:…".
    checksum: String,
}

/// jirutka/nginx-binaries' Mac builds, the newest of each of its last lines (the stable, even, and the
/// mainline), each with Homebrew's PCRE2 it is linked against.
fn nginx_mac_releases() -> Result<Vec<Release>> {
    let index: NginxBinaries = serde_json::from_str(&get_text(&format!("{NGINX_MAC}/index.json"))?).context("nginx-binaries' list unreadable")?;
    let mut newest: std::collections::BTreeMap<(u64, u64), NginxBinary> = std::collections::BTreeMap::new();
    for bin in index.contents {
        if bin.name != "nginx" || bin.os != "darwin" || bin.arch != "arm64" || !bin.variant.is_empty() || !safe_version(&bin.version) {
            continue;
        }
        let (major, minor, _, _) = parse(&bin.version);
        if newest.get(&(major, minor)).is_none_or(|b| parse(&bin.version) > parse(&b.version)) {
            newest.insert((major, minor), bin);
        }
    }
    let pcre2 = brew_bottle("pcre2")?;
    Ok(newest
        .into_values()
        .rev()
        .take(4)
        .map(|bin| {
            let note = if parse(&bin.version).1.is_multiple_of(2) { "stable" } else { "mainline" }.to_owned();
            Release { files: vec![(format!("{NGINX_MAC}/{}", bin.filename), Some(bin.checksum)), pcre2.clone()], version: bin.version, note }
        })
        .collect())
}

#[derive(Deserialize)]
struct Formula {
    bottle: FormulaBottles,
}

#[derive(Deserialize)]
struct FormulaBottles {
    stable: FormulaBottle,
}

#[derive(Deserialize)]
struct FormulaBottle {
    files: std::collections::BTreeMap<String, BottleFile>,
}

#[derive(Deserialize)]
struct BottleFile {
    url: String,
    sha256: String,
}

/// The place of Homebrew's bottle of `formula` for Apple's Macs, the oldest system's (it runs on the
/// newer), and its SHA-256.
fn brew_bottle(formula: &str) -> Result<(String, Option<String>)> {
    let found: Formula = serde_json::from_str(&get_text(&format!("{BREW_API}/{formula}.json"))?).context("Homebrew's answer unreadable")?;
    let files = found.bottle.stable.files;
    let tag = ["all", "arm64_sonoma", "arm64_sequoia", "arm64_tahoe"]
        .iter()
        .map(|t| t.to_string())
        .find(|t| files.contains_key(t))
        .or_else(|| files.keys().find(|t| t.starts_with("arm64_") && !t.contains("linux")).cloned())
        .with_context(|| format!("Homebrew has no {formula} for this Mac"))?;
    let file = &files[&tag];
    Ok((file.url.clone(), Some(file.sha256.clone())))
}

#[derive(Deserialize)]
struct GitRef {
    #[serde(rename = "ref")]
    name: String,
}

/// MySQL's newest release of its long-term lines (8.0, 8.4, 9.7...) and of its newest line, from its
/// GitHub tags; the archive's place (`MySQL-8.4/mysql-8.4.13-winx64.zip`,
/// `MySQL-8.4/mysql-8.4.13-macos15-arm64.tar.gz`) under the CDN's `Downloads`, or its `archives` once
/// superseded.
fn mysql_releases() -> Result<Vec<Release>> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "winx64.zip",
        ("macos", "aarch64") => "{macos}-arm64.tar.gz",
        ("macos", "x86_64") => "{macos}-x86_64.tar.gz",
        _ => bail!("mysql.com publishes MySQL for Windows and macOS only: install it on the system (sudo apt install mysql-server), Ronnie uses it"),
    };
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
            Release { files: vec![(format!("MySQL-{major}.{minor}/mysql-{version}-{platform}"), None)], version, note }
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
    let mut request = agent.get(url).header("User-Agent", USER_AGENT);
    if url.starts_with("https://ghcr.io/") {
        // Homebrew's bottles: its packages are public, read with the anonymous token.
        request = request.header("Authorization", "Bearer QQ==");
    }
    let mut response = request.call().with_context(|| format!("download failed: {url}"))?;
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
            Kind::Nginx if cfg!(windows) => download(&agent, &format!("{NGINX}/download/{file}"), progress, ctx, received, 0)?,
            Kind::Nginx => download(&agent, file, progress, ctx, received, 0)?,
            Kind::Mysql => mysql_download(&agent, file, progress, ctx, received)?,
            Kind::Mariadb => download(&agent, file, progress, ctx, received, 0)?,
        };
        if let Some(expected) = sha256 {
            let actual: String = match expected.strip_prefix("sha1:") {
                Some(_) => sha1::Sha1::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect(),
                None => Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect(),
            };
            let expected = expected.strip_prefix("sha1:").unwrap_or(expected);
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
        if kind == Kind::Nginx && !cfg!(windows) {
            nginx_mac_unpack(&staging, &archives)?;
        }
        for (file, bytes) in archives.iter().filter(|_| kind != Kind::Nginx || cfg!(windows)) {
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

/// A MySQL archive, under the CDN's `Downloads` or its `archives`; for macOS, built for one of the
/// systems it is published for (its name says which: "macos15").
fn mysql_download(agent: &ureq::Agent, file: &str, progress: &Arc<Mutex<Progress>>, ctx: &egui::Context, received: u64) -> Result<Vec<u8>> {
    let names: Vec<String> = if file.contains(MACOS) { ["macos15", "macos26", "macos27", "macos14"].map(|os| file.replace(MACOS, os)).to_vec() } else { vec![file.to_owned()] };
    let mut failed = None;
    for name in &names {
        for url in [format!("{MYSQL_CDN}/Downloads/{name}"), format!("{MYSQL_CDN}/archives/{}", name.to_lowercase())] {
            match download(agent, &url, progress, ctx, received, 0) {
                Ok(bytes) => return Ok(bytes),
                Err(e) => failed = Some(e),
            }
        }
    }
    Err(failed.unwrap_or_else(|| anyhow::anyhow!("MySQL isn't published for this system")))
}

/// nginx for the Mac put together in `staging`: its program (`archives`' first), Homebrew's PCRE2
/// beside it (in the bottle, the second), the program sent to it there rather than to Homebrew's
/// folder, then signed again (macOS runs nothing whose signature doesn't match).
fn nginx_mac_unpack(staging: &Path, archives: &[(String, Vec<u8>)]) -> Result<()> {
    let [(_, program), (_, bottle)] = archives else { bail!("nginx: its files are missing") };
    let mut program = program.clone();
    let lib = staging.join(PCRE2_LIB);
    let mut found = false;
    for entry in tar::Archive::new(flate2::read::GzDecoder::new(&bottle[..])).entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        // `pcre2/10.49/lib/libpcre2-8.0.dylib`: the file itself (the others are links to it).
        if path.file_name().is_some_and(|n| n == PCRE2_LIB) && entry.header().entry_type().is_file() {
            entry.unpack(&lib)?;
            found = true;
            break;
        }
    }
    if !found {
        bail!("Homebrew's PCRE2 doesn't hold {PCRE2_LIB}");
    }
    if relink(&mut program, PCRE2_BREW, "@executable_path/") == 0 {
        bail!("nginx isn't linked against Homebrew's PCRE2 any more");
    }
    let nginx = staging.join("nginx");
    std::fs::write(&nginx, &program)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&nginx, std::fs::Permissions::from_mode(0o755))?;
    }
    for file in [&lib, &nginx] {
        let out = std::process::Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(file).stdin(std::process::Stdio::null()).output().context("codesign missing")?;
        if !out.status.success() {
            bail!("{} can't be signed: {}", file.display(), String::from_utf8_lossy(&out.stderr).trim());
        }
    }
    let out = std::process::Command::new(&nginx).arg("-v").stdin(std::process::Stdio::null()).output().context("nginx doesn't start")?;
    if !out.status.success() {
        bail!("nginx doesn't start: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// In a program, the libraries it loads from folder `from` ("/opt/homebrew/opt/pcre2/lib/") loaded from
/// `to` instead ("@executable_path/"), as long as no longer (their place in it is fixed: what is left
/// is zeros); how many.
fn relink(program: &mut [u8], from: &str, to: &str) -> usize {
    let (from, to) = (from.as_bytes(), to.as_bytes());
    let mut count = 0;
    let mut at = 0;
    while let Some(found) = program[at..].windows(from.len()).position(|w| w == from) {
        let start = at + found;
        let end = start + program[start..].iter().position(|&b| b == 0).unwrap_or(program.len() - start);
        let name = program[start + from.len()..end].to_vec();
        let new: Vec<u8> = to.iter().chain(&name).copied().collect();
        if new.len() <= end - start {
            program[start..start + new.len()].copy_from_slice(&new);
            program[start + new.len()..end].fill(0);
            count += 1;
        }
        at = end;
    }
    count
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

    #[test]
    fn relinked() {
        let mut program = b"xx\0/opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib\0yy /opt/homebrew/opt/pcre/include".to_vec();
        let len = program.len();
        assert_eq!(relink(&mut program, PCRE2_BREW, "@executable_path/"), 1);
        assert_eq!(program.len(), len);
        assert!(program.starts_with(b"xx\0@executable_path/libpcre2-8.0.dylib\0\0"));
        assert!(program.ends_with(b"\0yy /opt/homebrew/opt/pcre/include"));
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
