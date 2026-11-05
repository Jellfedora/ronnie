//! The user's projects, served locally: each one a folder, a kind (WordPress, PrestaShop, Laravel, plain
//! PHP) and a PHP version, at `http://<name>.localhost:<port>` (browsers send any `*.localhost` to this
//! computer: no hosts file to edit, no administrator).
//!
//! One nginx serves them all, a site each; PHP runs a pool a version, shared by the projects on it:
//! PHP-FPM on macOS and Linux, a few `php-cgi` on Windows (where there is no PHP-FPM), both FastCGI to
//! nginx. What they read is written in `<config dir>/run`, their logs go to `<config dir>/run/logs`.
//! A pool stops once no project runs on it, nginx once none runs at all, everything when Ronnie
//! quits (and what a crash left running is stopped at the next start).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::envs::{self, slashed, Kind as Env};

/// The port nginx listens on, unless set otherwise.
pub const DEFAULT_PORT: u16 = 8080;
/// The ports the PHP pools take, from this one up.
const PHP_PORTS: u16 = 9300;
/// `php-cgi` serves one request at a time: so many of them a version, on Windows.
const CGI_WORKERS: u16 = 4;
/// Waiting for a program to answer on its port, at most.
const READY_WAIT: Duration = Duration::from_secs(15);
/// The port of the database servers, the usual one: the sites' settings ("localhost") work as they
/// are. A second server at once takes the next free one.
pub const DB_PORT: u16 = 3306;
/// A database server's first start after an import upgrades its data: a while.
const DB_READY_WAIT: Duration = Duration::from_secs(180);
/// PHP's first start: Windows checks its new programs and libraries (tens of MB) meanwhile.
const PHP_READY_WAIT: Duration = Duration::from_secs(60);

/// What a project is: it sets nginx's rules and where the site's root is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    Php,
    WordPress,
    PrestaShop,
    Laravel,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::WordPress, Kind::PrestaShop, Kind::Laravel, Kind::Php];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Php => "PHP",
            Kind::WordPress => "WordPress",
            Kind::PrestaShop => "PrestaShop",
            Kind::Laravel => "Laravel",
        }
    }
}

/// A project of the "My projects" category.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: Uuid,
    /// Shown, and its address: `<slug>.localhost`.
    pub name: String,
    /// Its folder.
    pub path: PathBuf,
    /// The site's root in it ("": the folder itself, "public" for Laravel).
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub kind: Kind,
    /// The PHP version it runs on (an installed one; None: the newest installed).
    #[serde(default)]
    pub php: Option<String>,
    /// The database server it runs with (None: none).
    #[serde(default)]
    pub db: Option<Db>,
}

/// A database server: MySQL or MariaDB, and its version.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Db {
    pub kind: Env,
    pub version: String,
}

impl Db {
    /// "MySQL 8.4.13".
    pub fn label(&self, system: &str) -> String {
        format!("{} {}", self.kind.name(), if self.version == envs::SYSTEM { system } else { &self.version })
    }
}

impl Project {
    /// A project of the folder `path`, its kind and root found in it; a database server if it has a
    /// database (the newest MySQL installed, or MariaDB).
    pub fn of_folder(path: &Path) -> Self {
        let found = detect(path);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "site".into());
        let php = found.php_line.or_else(wamp_php_line).as_deref().and_then(envs::php_of_line);
        let db = found.database.is_some().then(newest_db).flatten();
        Project { id: Uuid::new_v4(), name, path: path.to_path_buf(), root: found.root, kind: found.kind, php, db }
    }

    /// Its database server, if still installed.
    pub fn db_server(&self) -> Option<Db> {
        self.db.clone().filter(|db| envs::is_installed(db.kind, &db.version))
    }

    /// Its name in its address: lower case letters, digits and dashes.
    pub fn slug(&self) -> String {
        let mut slug = String::new();
        for c in self.name.to_lowercase().chars() {
            let c = match c {
                'a'..='z' | '0'..='9' => c,
                'à' | 'â' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'î' | 'ï' => 'i',
                'ô' | 'ö' => 'o',
                'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                _ => '-',
            };
            if c != '-' || !slug.ends_with('-') {
                slug.push(c);
            }
        }
        let slug = slug.trim_matches('-').to_owned();
        if slug.is_empty() { "site".into() } else { slug }
    }

    pub fn url(&self, port: u16) -> String {
        format!("http://{}.localhost:{port}", self.slug())
    }

    /// The folder nginx serves.
    pub fn web_root(&self) -> PathBuf {
        if self.root.is_empty() { self.path.clone() } else { self.path.join(&self.root) }
    }

    /// The PHP version it runs on: its own if still installed, otherwise WAMP's line if installed (the
    /// sites taken from it ran on it), otherwise the newest installed.
    pub fn php_version(&self) -> Option<String> {
        self.php
            .clone()
            .filter(|v| envs::is_installed(Env::Php, v))
            .or_else(|| wamp_php_line().and_then(|line| envs::php_of_line(&line)))
            .or_else(|| envs::installed(Env::Php).into_iter().next())
    }
}

/// The PHP line WAMP serves its sites with ("8.3"), from its settings (read once).
pub fn wamp_php_line() -> Option<String> {
    static LINE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    LINE.get_or_init(|| {
        if !cfg!(windows) {
            return None;
        }
        let text = ["C:\\wamp64\\wampmanager.conf", "C:\\wamp\\wampmanager.conf"].iter().find_map(|f| std::fs::read_to_string(f).ok())?;
        let version = text.lines().find_map(|l| l.trim().strip_prefix("phpVersion")).map(|v| v.trim_start_matches([' ', '=']).trim().trim_matches('"').to_owned())?;
        envs::parse(&version).0.gt(&0).then(|| envs::line(&version))
    })
    .clone()
}

/// The newest database server installed: MySQL, or else MariaDB.
pub fn newest_db() -> Option<Db> {
    Env::DATABASES.into_iter().find_map(|kind| envs::installed(kind).into_iter().next().map(|version| Db { kind, version }))
}

/// The folder of the projects unless another is picked: `Documents/Ronnie/Projets`.
pub fn default_dir() -> Option<PathBuf> {
    let user = directories::UserDirs::new()?;
    let documents = user.document_dir().map(Path::to_path_buf).unwrap_or_else(|| user.home_dir().join("Documents"));
    Some(documents.join("Ronnie").join("Projets"))
}

/// The projects of the folder of the projects: the folders in it (not the hidden ones, nor WAMP's
/// own when it is WAMP's `www`), by name.
pub fn folders_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            e.file_type().is_ok_and(|t| t.is_dir()) && !name.starts_with('.') && !name.starts_with("wamp")
        })
        .map(|e| e.path())
        .collect();
    folders.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    folders
}

/// A project from elsewhere copied into the folder of the projects (the original left as it is):
/// copied under a hidden name first, so that it is never read half copied. Its new folder.
pub fn import(from: &Path, dir: &Path) -> Result<PathBuf> {
    let name = from.file_name().with_context(|| format!("{} isn't a folder", from.display()))?;
    let target = dir.join(name);
    if target.exists() {
        bail!("{} already exists", target.display());
    }
    let staging = dir.join(format!(".{}.import", name.to_string_lossy()));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = copy_tree(from, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    std::fs::rename(&staging, &target).with_context(|| format!("cannot write {}", target.display()))?;
    Ok(target)
}

/// A folder copied whole.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("cannot write {}", to.display()))?;
    for entry in std::fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?.flatten() {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).with_context(|| format!("cannot copy {}", path.display()))?;
        }
    }
    Ok(())
}

/// What a folder holds.
#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub kind: Kind,
    pub root: String,
    /// "PrestaShop 9.1.4", "WordPress 6.9.7".
    pub label: Option<String>,
    /// Its database's name, from its configuration.
    pub database: Option<String>,
    /// The PHP line that suits it best ("8.1" for PrestaShop 8).
    pub php_line: Option<String>,
}

/// The first `'...'` or `"..."` in `text` after `after`.
fn quoted_after(text: &str, after: &str) -> Option<String> {
    let rest = &text[text.find(after)? + after.len()..];
    let start = rest.find(['\'', '"'])?;
    let quote = rest[start..].chars().next()?;
    let rest = &rest[start + 1..];
    Some(rest[..rest.find(quote)?].to_owned())
}

/// What `path` holds: WordPress, PrestaShop, Laravel or plain PHP, its version and database.
pub fn detect(path: &Path) -> Found {
    let read = |file: &str| std::fs::read_to_string(path.join(file)).unwrap_or_default();
    if path.join("wp-config.php").is_file() || path.join("wp-load.php").is_file() {
        let version = quoted_after(&read("wp-includes/version.php"), "$wp_version");
        let database = quoted_after(&read("wp-config.php"), "'DB_NAME'").or_else(|| quoted_after(&read("wp-config.php"), "\"DB_NAME\""));
        return Found { kind: Kind::WordPress, root: String::new(), label: Some(format!("WordPress {}", version.unwrap_or_default()).trim().to_owned()), database, php_line: None };
    }
    if path.join("config/defines.inc.php").is_file() && path.join("classes").is_dir() {
        let version = quoted_after(&read("src/Core/Version.php"), "const VERSION =").or_else(|| quoted_after(&read("app/AppKernel.php"), "const VERSION ="));
        let database = quoted_after(&read("app/config/parameters.php"), "'database_name'");
        // PrestaShop 8 runs up to PHP 8.1, PrestaShop 9 from 8.1.
        let php_line = version.as_deref().map(|v| if v.starts_with('8') || v.starts_with("1.7") { "8.1" } else { "8.3" }.to_owned());
        return Found { kind: Kind::PrestaShop, root: String::new(), label: Some(format!("PrestaShop {}", version.unwrap_or_default()).trim().to_owned()), database, php_line };
    }
    if path.join("artisan").is_file() && path.join("public/index.php").is_file() {
        let database = read(".env").lines().find_map(|l| l.strip_prefix("DB_DATABASE=")).map(|d| d.trim().trim_matches('"').to_owned());
        return Found { kind: Kind::Laravel, root: "public".into(), label: Some("Laravel".into()), database, php_line: None };
    }
    let root = if !path.join("index.php").is_file() && path.join("public/index.php").is_file() { "public" } else { "" };
    Found { kind: Kind::Php, root: root.into(), label: None, database: None, php_line: None }
}

/// Where what nginx and PHP read is written, and their logs.
fn run_dir() -> Result<PathBuf> {
    Ok(crate::config::config_dir().context("no config folder")?.join("run"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(run_dir()?.join("logs"))
}

/// The logs of a project's site: its errors, then its requests.
pub fn site_logs(project: &Project) -> Result<(PathBuf, PathBuf)> {
    let logs = logs_dir()?;
    Ok((logs.join(format!("{}.error.log", project.slug())), logs.join(format!("{}.access.log", project.slug()))))
}

/// A PHP version's own log.
pub fn php_log(version: &str) -> Result<PathBuf> {
    Ok(logs_dir()?.join(format!("php-{version}.log")))
}

/// The upstream name of a PHP version in nginx's configuration.
fn upstream(version: &str) -> String {
    format!("php_{}", version.replace(['.', '-'], "_"))
}

/// PHP's settings for the projects: what WordPress and PrestaShop want, the mails to Ronnie's mail
/// catcher (on Windows: elsewhere PHP sends through a sendmail program, to come).
fn php_ini(version: &str, mail_port: u16) -> Result<String> {
    let dir = envs::dir(Env::Php, version).context("PHP missing")?;
    let mut ini = String::from("; Written by Ronnie for its projects: changed at each start.\n");
    if cfg!(windows) {
        let ext = dir.join("ext");
        ini += &format!("extension_dir = \"{}\"\n", slashed(&ext));
        for name in ["bz2", "curl", "exif", "fileinfo", "gd", "gettext", "gmp", "intl", "mbstring", "mysqli", "openssl", "pdo_mysql", "pdo_sqlite", "soap", "sockets", "sodium", "sqlite3", "xsl", "zip"] {
            if ext.join(format!("php_{name}.dll")).is_file() {
                ini += &format!("extension = {name}\n");
            }
        }
        if ext.join("php_opcache.dll").is_file() {
            ini += "zend_extension = opcache\n";
        }
        ini += &format!("SMTP = 127.0.0.1\nsmtp_port = {mail_port}\nsendmail_from = ronnie@localhost\n");
    } else {
        // "localhost" is a socket for PHP there: the database's on the usual port.
        let socket = slashed(&run_dir()?.join("mysql.sock"));
        ini += &format!("mysqli.default_socket = \"{socket}\"\npdo_mysql.default_socket = \"{socket}\"\n");
        // Its mails to the mail catcher, not the system's sendmail (which may deliver them).
        if let Some(cli) = envs::php_cli(version) {
            let script = run_dir()?.join("sendmail.php");
            std::fs::create_dir_all(run_dir()?)?;
            std::fs::write(&script, SENDMAIL)?;
            ini += &format!("sendmail_path = \"{} -n {} {mail_port}\"\n", shell_quoted(&cli), shell_quoted(&script));
        }
    }
    ini += &format!(
        "memory_limit = 512M\nupload_max_filesize = 64M\npost_max_size = 64M\nmax_execution_time = 300\nmax_input_time = 300\nmax_input_vars = 5000\n\
         date.timezone = Europe/Paris\ndisplay_errors = On\nerror_reporting = E_ALL\nlog_errors = On\nerror_log = \"{}\"\n\
         opcache.enable = 1\nopcache.validate_timestamps = 1\nopcache.revalidate_freq = 0\nrealpath_cache_size = 4096K\n",
        slashed(&php_log(version)?)
    );
    Ok(ini)
}

/// A path for a shell (PHP runs `sendmail_path` through one): in single quotes.
fn shell_quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// PHP's `sendmail` outside Windows: the mail read on its input, sent to Ronnie's mail catcher (its port
/// the first argument), to the addresses of its To, Cc and Bcc (as `sendmail -t`); never delivered.
const SENDMAIL: &str = r#"<?php
// Written by Ronnie: PHP's mails to Ronnie's mail catcher, never delivered.
$port = (int) ($argv[1] ?? 1025);
$raw = str_replace("\r\n", "\n", (string) stream_get_contents(STDIN));
[$head, $body] = array_pad(explode("\n\n", $raw, 2), 2, '');
$to = [];
preg_match_all('/^(?:To|Cc|Bcc):(.*(?:\n[ \t].*)*)/mi', $head, $lines);
foreach ($lines[1] as $line) {
    preg_match_all('/[^\s<>,"]+@[^\s<>,"]+/', $line, $found);
    $to = array_merge($to, $found[0]);
}
$from = preg_match('/^From:.*?([^\s<>,"]+@[^\s<>,"]+)/mi', $head, $f) ? $f[1] : 'php@localhost';
$head = preg_replace('/^Bcc:.*(?:\n[ \t].*)*\n?/mi', '', $head);
$s = @fsockopen('127.0.0.1', $port, $errno, $error, 5);
if (!$s || !$to) {
    fwrite(STDERR, $s ? "no recipient\n" : "Ronnie's mail catcher isn't running (port $port)\n");
    exit(1);
}
$say = function ($line) use ($s) {
    if ($line !== null) {
        fwrite($s, $line . "\r\n");
    }
    do {
        $reply = fgets($s, 1024);
    } while ($reply !== false && isset($reply[3]) && $reply[3] === '-');
    return (int) substr((string) $reply, 0, 3);
};
$say(null);
$say('HELO localhost');
$say("MAIL FROM:<$from>");
foreach (array_unique($to) as $address) {
    $say("RCPT TO:<$address>");
}
$say('DATA');
$data = preg_replace('/^\./m', '..', rtrim($head, "\n") . "\n\n" . $body);
$code = $say(str_replace("\n", "\r\n", rtrim($data, "\n")) . "\r\n.");
$say('QUIT');
fclose($s);
exit($code === 250 ? 0 : 1);
"#;

/// PHP-FPM's configuration for a version (macOS, Linux).
fn fpm_conf(version: &str, port: u16, run: &Path) -> Result<String> {
    Ok(format!(
        "; Written by Ronnie for its projects: changed at each start.\n[global]\npid = \"{pid}\"\nerror_log = \"{log}\"\ndaemonize = no\n\n\
         [www]\nlisten = 127.0.0.1:{port}\npm = dynamic\npm.max_children = 8\npm.start_servers = 2\npm.min_spare_servers = 1\npm.max_spare_servers = 4\n\
         catch_workers_output = yes\nclear_env = no\n",
        pid = slashed(&run.join("php-fpm.pid")),
        log = slashed(&php_log(version)?),
    ))
}

/// nginx's own configuration: its folders in `run/nginx`, the sites of the projects running. In the
/// foreground (`daemon off`, outside Windows where it is anyway): Ronnie sees it stop.
fn nginx_conf(nginx_dir: &Path, logs: &Path, port: u16) -> String {
    format!(
        "# Written by Ronnie for its projects: changed at each start.\n{daemon}worker_processes 1;\npid logs/nginx.pid;\nerror_log \"{error}\" warn;\n\
         events {{\n    worker_connections 1024;\n}}\n\
         http {{\n    include \"{mime}\";\n    default_type application/octet-stream;\n    sendfile on;\n    keepalive_timeout 65;\n    \
         client_max_body_size 64m;\n    server_names_hash_bucket_size 128;\n    \
         client_body_temp_path temp/client_body;\n    proxy_temp_path temp/proxy;\n    fastcgi_temp_path temp/fastcgi;\n    \
         uwsgi_temp_path temp/uwsgi;\n    scgi_temp_path temp/scgi;\n    \
         include upstreams.conf;\n    include sites/*.conf;\n    \
         server {{\n        listen 127.0.0.1:{port} default_server;\n        return 404 \"No project of Ronnie at this address.\\n\";\n    }}\n}}\n",
        daemon = if cfg!(windows) { "" } else { "daemon off;\n" },
        error = slashed(&logs.join("nginx.error.log")),
        mime = slashed(&nginx_dir.join("mime.types")),
    )
}

/// The usual types, for nginx (the system's copy isn't always where expected).
const MIME_TYPES: &str = "types {\n    text/html html htm;\n    text/css css;\n    text/xml xml;\n    text/plain txt;\n    \
    application/javascript js mjs;\n    application/json json map;\n    application/pdf pdf;\n    application/zip zip;\n    \
    application/wasm wasm;\n    application/xml rss atom;\n    image/gif gif;\n    image/jpeg jpeg jpg;\n    image/png png;\n    \
    image/webp webp;\n    image/avif avif;\n    image/svg+xml svg svgz;\n    image/x-icon ico;\n    font/woff woff;\n    font/woff2 woff2;\n    \
    font/ttf ttf;\n    font/otf otf;\n    application/vnd.ms-fontobject eot;\n    audio/mpeg mp3;\n    audio/ogg ogg;\n    \
    video/mp4 mp4;\n    video/webm webm;\n    text/csv csv;\n}\n";

/// FastCGI's parameters, as nginx ships them.
const FASTCGI_PARAMS: &str = "fastcgi_param QUERY_STRING $query_string;\nfastcgi_param REQUEST_METHOD $request_method;\n\
    fastcgi_param CONTENT_TYPE $content_type;\nfastcgi_param CONTENT_LENGTH $content_length;\nfastcgi_param SCRIPT_NAME $fastcgi_script_name;\n\
    fastcgi_param REQUEST_URI $request_uri;\nfastcgi_param DOCUMENT_URI $document_uri;\nfastcgi_param DOCUMENT_ROOT $document_root;\n\
    fastcgi_param SERVER_PROTOCOL $server_protocol;\nfastcgi_param REQUEST_SCHEME $scheme;\nfastcgi_param HTTPS $https if_not_empty;\n\
    fastcgi_param GATEWAY_INTERFACE CGI/1.1;\nfastcgi_param SERVER_SOFTWARE nginx/$nginx_version;\nfastcgi_param REMOTE_ADDR $remote_addr;\n\
    fastcgi_param REMOTE_PORT $remote_port;\nfastcgi_param SERVER_ADDR $server_addr;\nfastcgi_param SERVER_PORT $server_port;\n\
    fastcgi_param SERVER_NAME $server_name;\nfastcgi_param REDIRECT_STATUS 200;\n";

/// PrestaShop's friendly image addresses ("/123-home_default/name.jpg"), as its own nginx guide has them.
fn prestashop_images() -> String {
    let mut rules = String::new();
    for digits in 1..=8 {
        let groups: String = (0..digits).map(|_| "(\\d)").collect();
        let dirs: String = (1..=digits).map(|k| format!("${k}/")).collect();
        let name: String = (1..=digits).map(|k| format!("${k}")).collect();
        rules += &format!("    rewrite ^/{groups}(-[\\w-]+)?/.+\\.jpg$ /img/p/{dirs}{name}${}.jpg last;\n", digits + 1);
    }
    rules + "    rewrite ^/c/([\\w.-]+)/.+\\.jpg$ /img/c/$1.jpg last;\n"
}

/// The site of a project for nginx: its address, root, rules for its kind, PHP through its version's
/// pool.
fn site_conf(project: &Project, version: &str, port: u16) -> Result<String> {
    let (errors, requests) = site_logs(project)?;
    let rules = match project.kind {
        Kind::WordPress | Kind::Php => "    location / {\n        try_files $uri $uri/ /index.php?$args;\n    }\n".to_owned(),
        Kind::Laravel => "    location / {\n        try_files $uri $uri/ /index.php?$query_string;\n    }\n".to_owned(),
        Kind::PrestaShop => format!(
            "{images}    location ~ ^/(admin[\\w-]*)/ {{\n        try_files $uri $uri/ /$1/index.php$is_args$args;\n    }}\n    \
             location / {{\n        try_files $uri $uri/ /index.php$is_args$args;\n    }}\n",
            images = prestashop_images()
        ),
    };
    Ok(format!(
        "# {name}: written by Ronnie, changed at each start.\nserver {{\n    listen 127.0.0.1:{port};\n    server_name {slug}.localhost;\n    \
         root \"{root}\";\n    index index.php index.html index.htm;\n    charset utf-8;\n    \
         access_log \"{requests}\";\n    error_log \"{errors}\" warn;\n    \
         location ~ /\\.(?!well-known) {{\n        deny all;\n    }}\n    \
         location ~ [^/]\\.php(/|$) {{\n        fastcgi_split_path_info ^(.+?\\.php)(/.*)$;\n        \
         if (!-f $document_root$fastcgi_script_name) {{\n            return 404;\n        }}\n        \
         fastcgi_pass {upstream};\n        fastcgi_index index.php;\n        include \"{params}\";\n        \
         fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n        fastcgi_param PATH_INFO $fastcgi_path_info;\n        \
         fastcgi_read_timeout 300;\n        fastcgi_buffers 16 32k;\n        fastcgi_buffer_size 64k;\n    }}\n{rules}}}\n",
        name = project.name,
        slug = project.slug(),
        root = slashed(&project.web_root()),
        requests = slashed(&requests),
        errors = slashed(&errors),
        upstream = upstream(version),
        params = slashed(&run_dir()?.join("nginx").join("fastcgi_params")),
    ))
}

/// Whether something answers on `port` (this computer).
fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200)).is_ok()
}

/// Whether `port` is free to listen on (this computer): nothing answers on it either (macOS lets
/// 127.0.0.1 be taken beside a server listening on all addresses, a MySQL of the system's).
fn free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok() && !answers(port)
}

/// A site's address in its database made Ronnie's (`<name>.localhost:<port>`), so that it doesn't send
/// to its old one (WAMP's `localhost/<name>/`): PrestaShop's shop address (and its cache emptied),
/// WordPress's `siteurl` and `home`. Whether it changed anything.
fn fix_address(project: &Project, db: &Db, db_port: u16, port: u16) -> Result<bool> {
    let Some(client) = envs::db_client(db.kind, &db.version) else { return Ok(false) };
    let read = |file: &str| std::fs::read_to_string(project.path.join(file)).unwrap_or_default();
    let host = format!("{}.localhost:{port}", project.slug());
    let sql = |database: &str, query: &str| run_sql(&client, db_port, Some(database), query);
    let safe = |prefix: &str| prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    match project.kind {
        Kind::PrestaShop => {
            let params = read("app/config/parameters.php");
            let (Some(database), Some(prefix)) = (quoted_after(&params, "'database_name'"), quoted_after(&params, "'database_prefix'")) else { return Ok(false) };
            if !safe(&prefix) {
                return Ok(false);
            }
            let now = sql(&database, &format!("SELECT CONCAT(domain, ' ', physical_uri) FROM `{prefix}shop_url` WHERE main = 1 LIMIT 1"))?;
            if now.is_empty() || now == format!("{host} /") {
                return Ok(false);
            }
            sql(
                &database,
                &format!(
                    "UPDATE `{prefix}shop_url` SET domain = '{host}', domain_ssl = '{host}', physical_uri = '/' WHERE main = 1; \
                     UPDATE `{prefix}configuration` SET value = '{host}' WHERE name IN ('PS_SHOP_DOMAIN', 'PS_SHOP_DOMAIN_SSL'); \
                     UPDATE `{prefix}configuration` SET value = '0' WHERE name IN ('PS_SSL_ENABLED', 'PS_SSL_ENABLED_EVERYWHERE');"
                ),
            )?;
            empty_cache(&project.path.join("var").join("cache"));
            crate::log::info(&format!("{}: its shop's address was {now}, now {host}/", project.name));
            Ok(true)
        }
        Kind::WordPress => {
            let config = read("wp-config.php");
            let database = quoted_after(&config, "'DB_NAME'").or_else(|| quoted_after(&config, "\"DB_NAME\""));
            let (Some(database), Some(prefix)) = (database, quoted_after(&config, "$table_prefix")) else { return Ok(false) };
            // Set in wp-config.php: what the database says doesn't count.
            if !safe(&prefix) || config.contains("WP_HOME") || config.contains("WP_SITEURL") {
                return Ok(false);
            }
            let url = format!("http://{host}");
            let now = sql(&database, &format!("SELECT option_value FROM `{prefix}options` WHERE option_name = 'siteurl'"))?;
            if now.is_empty() || now == url {
                return Ok(false);
            }
            sql(&database, &format!("UPDATE `{prefix}options` SET option_value = '{url}' WHERE option_name IN ('siteurl', 'home')"))?;
            crate::log::info(&format!("{}: its address was {now}, now {url}", project.name));
            Ok(true)
        }
        Kind::Laravel | Kind::Php => Ok(false),
    }
}

/// A query run by a database server's client, as root (on Ronnie's server on `port`), in `database`;
/// what it printed.
fn run_sql(client: &Path, port: u16, database: Option<&str>, query: &str) -> Result<String> {
    let mut cmd = Command::new(client);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.args(["--user=root", "--host=127.0.0.1", &format!("--port={port}"), "--batch", "--skip-column-names", "--default-character-set=utf8mb4"]);
    cmd.args(database).args(["-e", query]);
    let out = cmd.stdin(Stdio::null()).output().context("the database's client doesn't start")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// A new site's folder name: as typed, but what a folder can't be named (or would hide it).
pub fn valid_site_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.len() <= 64 && !name.starts_with('.') && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) && !name.to_lowercase().starts_with("wamp")
}

/// A new WordPress in the folder of the projects, as `name`: its newest version downloaded (in French
/// for `french`), its database made empty on `db` (started for it, then stopped unless something runs
/// on it), its `wp-config.php` written (root without a password, as WAMP has it). WordPress's own
/// install page does the rest: the site's title, its administrator. Put together aside, then moved in
/// place. Its folder; blocking.
pub fn new_wordpress(stack: &std::sync::Mutex<Stack>, dir: &Path, name: &str, db: &Db, french: bool) -> Result<PathBuf> {
    let name = name.trim();
    if !valid_site_name(name) {
        bail!("{name:?} can't be a folder's name");
    }
    let target = dir.join(name);
    if target.exists() {
        bail!("{} already exists", target.display());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("cannot write {}", dir.display()))?;
    let staging = dir.join(format!(".{name}.new"));
    let _ = std::fs::remove_dir_all(&staging);
    let result = (|| {
        let url = if french { "https://fr.wordpress.org/latest-fr_FR.zip" } else { "https://wordpress.org/latest.zip" };
        let agent: ureq::Agent = ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(15))).timeout_global(Some(Duration::from_secs(600))).build().new_agent();
        let zip = agent
            .get(url)
            .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
            .call()
            .with_context(|| format!("{url} unreachable"))?
            .body_mut()
            .with_config()
            .limit(200 * 1024 * 1024)
            .read_to_vec()
            .context("WordPress's download failed")?;
        zip::ZipArchive::new(std::io::Cursor::new(zip)).context("WordPress's archive unreadable")?.extract(&staging).context("WordPress's archive unreadable")?;
        let site = staging.join("wordpress");
        if !site.join("wp-settings.php").is_file() {
            bail!("WordPress's archive doesn't hold WordPress");
        }

        // Its database: the site's name ("mon-site" → "mon_site"), numbered if taken.
        let base: String = Project { id: Uuid::nil(), name: name.to_owned(), path: PathBuf::new(), root: String::new(), kind: Kind::WordPress, php: None, db: None }.slug().replace('-', "_");
        let base: String = base.chars().take(48).collect();
        let database = std::iter::once(base.clone()).chain((2..100).map(|n| format!("{base}_{n}"))).find(|d| !holds_database(db, d)).context("no database name left")?;
        let port = stack.lock().unwrap().create_database(db, &database)?;
        let host = if is_main_db(port) { "localhost".to_owned() } else { format!("127.0.0.1:{port}") };
        std::fs::write(site.join("wp-config.php"), wp_config(&database, &host)).context("wp-config.php can't be written")?;
        std::fs::rename(&site, &target).with_context(|| format!("cannot write {}", target.display()))?;
        Ok(target.clone())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// WordPress's settings: its database on Ronnie's server (root without a password), its keys drawn
/// at random.
fn wp_config(database: &str, host: &str) -> String {
    let mut text = String::from("<?php\n/**\n * WordPress's settings, written by Ronnie: the database is on Ronnie's server, root without a password.\n */\n\n");
    for (key, value) in [("DB_NAME", database), ("DB_USER", "root"), ("DB_PASSWORD", ""), ("DB_HOST", host), ("DB_CHARSET", "utf8mb4"), ("DB_COLLATE", "")] {
        text += &format!("define( '{key}', '{value}' );\n");
    }
    text.push('\n');
    for key in ["AUTH_KEY", "SECURE_AUTH_KEY", "LOGGED_IN_KEY", "NONCE_KEY", "AUTH_SALT", "SECURE_AUTH_SALT", "LOGGED_IN_SALT", "NONCE_SALT"] {
        text += &format!("define( '{key}', '{}{}' );\n", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    }
    text += "\n$table_prefix = 'wp_';\n\ndefine( 'WP_DEBUG', false );\n\nif ( ! defined( 'ABSPATH' ) ) {\n\tdefine( 'ABSPATH', __DIR__ . '/' );\n}\n\nrequire_once ABSPATH . 'wp-settings.php';\n";
    text
}

/// PrestaShop's cache emptied: moved aside at once (rebuilt from the next page), deleted in the
/// background (tens of thousands of files).
fn empty_cache(cache: &Path) {
    let Ok(entries) = std::fs::read_dir(cache) else { return };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let aside = cache.with_file_name(format!("cache-old-{stamp}"));
    let _ = std::fs::create_dir_all(&aside);
    for entry in entries.flatten() {
        let _ = std::fs::rename(entry.path(), aside.join(entry.file_name()));
    }
    std::thread::spawn(move || {
        let _ = std::fs::remove_dir_all(&aside);
    });
}

/// What listens on `port` on this computer: its program, and its Windows service if it is one
/// ("mysqld.exe (service wampmysqld64)").
fn port_owner(port: u16) -> Option<String> {
    let output = |program: &str, args: &[&str]| -> Option<String> {
        let mut cmd = Command::new(program);
        cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if cfg!(windows) {
        let netstat = output("netstat", &["-ano", "-p", "TCP"])?;
        let pid = netstat_listener(&netstat, port)?;
        let tasks = output("tasklist", &["/SVC", "/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])?;
        let fields: Vec<&str> = tasks.lines().next()?.split("\",\"").map(|f| f.trim_matches(['"', '\r'])).collect();
        let program = fields.first().filter(|p| !p.is_empty())?;
        Some(match fields.get(2).filter(|s| !s.is_empty() && **s != "N/A" && !s.contains('/')) {
            Some(service) => format!("{program} (service {service})"),
            None => format!("{program} (PID {pid})"),
        })
    } else {
        let lsof = output("lsof", &["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpc"])?;
        let pid = lsof.lines().find_map(|l| l.strip_prefix('p'))?.to_owned();
        let program = lsof.lines().find_map(|l| l.strip_prefix('c'))?;
        Some(format!("{program} (PID {pid})"))
    }
}

/// In `netstat -ano`'s output, the process listening on `port` (whatever the language: a listener's
/// remote address is all zeros).
fn netstat_listener(netstat: &str, port: u16) -> Option<u32> {
    netstat.lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (local, remote, pid) = (cols.get(1)?, cols.get(2)?, cols.last()?);
        let listening = matches!(*remote, "0.0.0.0:0" | "[::]:0" | "*:*");
        (cols.first() == Some(&"TCP") && listening && local.rsplit(':').next() == Some(&port.to_string())).then(|| pid.parse().ok()).flatten()
    })
}

/// A program of the projects, started without a window.
fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// A PHP pool's processes stopped: PHP-FPM asked first (SIGTERM: it stops its workers with it, which a
/// kill would leave running, holding its port), killed if it doesn't within 3 s; php-cgi killed.
fn end_pool(children: &mut [Child]) {
    #[cfg(unix)]
    {
        for child in children.iter() {
            if let Ok(pid) = libc::pid_t::try_from(child.id()) {
                // SAFETY: a signal to our own child, still ours until waited for.
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && children.iter_mut().any(|c| c.try_wait().ok().flatten().is_none()) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    for child in children.iter_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// A PHP version's pool: its processes and their ports.
struct Pool {
    children: Vec<Child>,
    ports: Vec<u16>,
}

/// nginx running.
struct Nginx {
    program: PathBuf,
    child: Child,
    port: u16,
}

/// A database server running.
struct DbServer {
    child: Child,
    port: u16,
}

/// What runs for the projects.
#[derive(Default)]
pub struct Stack {
    nginx: Option<Nginx>,
    pools: BTreeMap<String, Pool>,
    dbs: BTreeMap<Db, DbServer>,
    /// The projects running, the PHP version and database server each one runs on.
    running: BTreeMap<Uuid, (String, Option<Db>)>,
    /// The database servers started by hand (their data folders): they run without a project.
    manual: BTreeSet<PathBuf>,
    /// The project whose address in its database the last start made Ronnie's.
    pub fixed: Option<Uuid>,
}

/// A project to start: what it needs, found on the interface's thread.
pub struct Start {
    pub project: Project,
    pub php: String,
    pub nginx: String,
    pub db: Option<Db>,
    pub port: u16,
    pub mail_port: u16,
}

/// What a crash left running stopped: for the first Ronnie only (a second one, read only, leaves the
/// first's projects alone).
pub fn clean_up() {
    kill_leftovers();
}

impl Stack {
    /// Starts a project (its database server, PHP pool and nginx if they don't run yet), its site
    /// added to nginx; once they answer, returns the port of its database server. Blocking (up to
    /// `READY_WAIT`, more for a database's first start): off the interface's thread.
    pub fn start(&mut self, start: &Start) -> Result<Option<u16>> {
        let project = &start.project;
        if !project.web_root().is_dir() {
            bail!("the folder {} doesn't exist", project.web_root().display());
        }
        let run = run_dir()?;
        let logs = logs_dir()?;
        std::fs::create_dir_all(&logs)?;
        let db_port = match &start.db {
            Some(db) => Some(self.start_db(db, &run, &logs)?),
            None => None,
        };
        if let (Some(db), Some(db_port)) = (&start.db, db_port) {
            match fix_address(project, db, db_port, start.port) {
                Ok(true) => self.fixed = Some(project.id),
                Ok(false) => {}
                Err(e) => crate::log::info(&format!("{}: its address not checked: {e:#}", project.name)),
            }
        }
        if let Err(e) = self.start_pool(&start.php, start.mail_port, &run) {
            self.stop_unused_dbs();
            return Err(e);
        }
        // nginx's files, its site among them.
        let nginx_dir = run.join("nginx");
        for sub in ["sites", "logs", "temp"] {
            std::fs::create_dir_all(nginx_dir.join(sub))?;
        }
        std::fs::write(nginx_dir.join("mime.types"), MIME_TYPES)?;
        std::fs::write(nginx_dir.join("fastcgi_params"), FASTCGI_PARAMS)?;
        std::fs::write(nginx_dir.join("nginx.conf"), nginx_conf(&nginx_dir, &logs, start.port))?;
        std::fs::write(nginx_dir.join("sites").join(format!("{}.conf", project.id)), site_conf(project, &start.php, start.port)?)?;
        self.running.insert(project.id, (start.php.clone(), start.db.clone()));
        // The sites of projects not running (left by a Ronnie that didn't quit): their PHP doesn't run,
        // nginx would refuse them all.
        self.remove_stale_sites(&nginx_dir.join("sites"));
        self.write_upstreams(&nginx_dir)?;
        let alive = self.nginx.as_mut().is_some_and(|n| n.port == start.port && n.child.try_wait().ok().flatten().is_none());
        let done = match &self.nginx {
            Some(n) if alive => nginx_signal(&n.program, &nginx_dir, "reload"),
            _ => self.start_nginx(&start.nginx, &nginx_dir, start.port),
        };
        if let Err(e) = done {
            self.stop(project.id);
            return Err(e);
        }
        Ok(db_port)
    }

    /// Starts a database server by hand, without a project (to see its databases): on its own port
    /// only (WAMP's, when imported from it), else says what takes it. Its port; blocking.
    pub fn start_db_alone(&mut self, db: &Db) -> Result<u16> {
        let run = run_dir()?;
        let logs = logs_dir()?;
        std::fs::create_dir_all(&logs)?;
        let port = self.start_db(db, &run, &logs)?;
        self.manual.insert(data_dir(db)?);
        Ok(port)
    }

    /// A database server started by hand stopped, unless a project runs with it.
    pub fn stop_db_alone(&mut self, db: &Db) {
        if let Ok(data) = data_dir(db) {
            self.manual.remove(&data);
        }
        self.stop_unused_dbs();
    }

    /// Starts a database server unless it (or another version on the same data) runs, its data made
    /// first if there is none (its own port then picked); its port. On its own port only, the sites
    /// expect it there: what takes it is named instead.
    fn start_db(&mut self, db: &Db, run: &Path, logs: &Path) -> Result<u16> {
        let data = data_dir(db)?;
        let same: Vec<Db> = self.dbs.keys().filter(|k| data_dir(k).is_ok_and(|d| d == data)).cloned().collect();
        for key in same {
            if let Some(server) = self.dbs.get_mut(&key) {
                if server.child.try_wait().ok().flatten().is_none() {
                    return Ok(server.port);
                }
                self.dbs.remove(&key);
            }
        }
        let name = db.label("");
        let program = envs::db_server(db.kind, &db.version).with_context(|| format!("{name} isn't installed"))?;
        let log = logs.join(format!("{}.log", data.file_name().unwrap_or_default().to_string_lossy()));
        if !has_data(db) {
            init_data(db, &data, &log)?;
            give_port(db)?;
        }
        let port = wanted_port(db);
        if let Some((other, _)) = self.dbs.iter().find(|(_, s)| s.port == port) {
            bail!("the port {port} is already {}'s, run by Ronnie for another project: stop that project first", other.label("system"));
        }
        if !free(port) {
            let who = port_owner(port).map(|w| format!(" by {w}")).unwrap_or_else(|| " by another program".into());
            bail!("{name} wants the port {port}, taken{who}: stop it (WAMP: its icon › Stop all services), then start again");
        }
        let mut cmd = command(&program);
        // Its own settings (WAMP's, imported) first, or none of the system's.
        match settings_file(db).ok().filter(|f| f.is_file()) {
            Some(file) => cmd.arg(format!("--defaults-file={}", slashed(&file))),
            None => cmd.arg("--no-defaults"),
        };
        if let Some(base) = envs::dir(db.kind, &db.version) {
            cmd.arg(format!("--basedir={}", slashed(&base)));
        }
        cmd.args([format!("--datadir={}", slashed(&data)), format!("--port={port}"), "--bind-address=127.0.0.1".into(), format!("--log-error={}", slashed(&log))]);
        if db.kind == Env::Mysql {
            // Its X protocol takes 33060, WAMP's MySQL's too.
            cmd.arg("--mysqlx=OFF");
        }
        if !cfg!(windows) {
            // PHP's "localhost" goes to `run/mysql.sock`: the main server's.
            let socket = if is_main_db(port) { run.join("mysql.sock") } else { run.join(format!("mysql-{port}.sock")) };
            cmd.arg(format!("--socket={}", slashed(&socket)));
        }
        let child = cmd.spawn().with_context(|| format!("{name} doesn't start"))?;
        remember(child.id());
        let mut children = vec![child];
        if let Err(e) = wait_ready_for(&[port], &mut children, DB_READY_WAIT) {
            for child in &mut children {
                let _ = child.kill();
            }
            bail!("{name}: {e}{}", tail(&log).unwrap_or_default());
        }
        self.dbs.insert(db.clone(), DbServer { child: children.remove(0), port });
        Ok(port)
    }

    /// Stops the database servers no project runs with any more: cleanly (its admin program), or
    /// else killed.
    /// An empty database `name` made on `db` (started for it, stopped after unless something runs on
    /// it); the server's port. Blocking.
    pub fn create_database(&mut self, db: &Db, name: &str) -> Result<u16> {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("invalid database name {name}");
        }
        let run = run_dir()?;
        let logs = logs_dir()?;
        std::fs::create_dir_all(&logs)?;
        let port = self.start_db(db, &run, &logs)?;
        let client = envs::db_client(db.kind, &db.version).with_context(|| format!("{}'s client is missing", db.label("system")));
        let done = client.and_then(|client| run_sql(&client, port, None, &format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")));
        self.stop_unused_dbs();
        done.map(|_| port)
    }

    fn stop_unused_dbs(&mut self) {
        let mut used: BTreeSet<PathBuf> = self.running.values().filter_map(|(_, db)| db.as_ref().and_then(|db| data_dir(db).ok())).collect();
        used.extend(self.manual.iter().cloned());
        let unused: Vec<Db> = self.dbs.keys().filter(|db| !data_dir(db).is_ok_and(|d| used.contains(&d))).cloned().collect();
        for db in unused {
            if let Some(server) = self.dbs.remove(&db) {
                stop_db(&db, server);
            }
        }
    }

    fn start_pool(&mut self, version: &str, mail_port: u16, run: &Path) -> Result<()> {
        if let Some(pool) = self.pools.get_mut(version) {
            if pool.children.iter_mut().all(|c| c.try_wait().ok().flatten().is_none()) {
                return Ok(());
            }
            // One stopped (crashed): the pool started again.
            end_pool(&mut pool.children);
            self.pools.remove(version);
        }
        let program = envs::php_server(version).with_context(|| format!("PHP {version} isn't installed"))?;
        let dir = run.join("php").join(version);
        std::fs::create_dir_all(&dir)?;
        let ini = dir.join("php.ini");
        std::fs::write(&ini, php_ini(version, mail_port)?)?;
        let taken: BTreeSet<u16> = self.pools.values().flat_map(|p| p.ports.iter().copied()).collect();
        let wanted = if cfg!(windows) { CGI_WORKERS } else { 1 };
        let mut ports = Vec::new();
        let mut port = PHP_PORTS;
        while ports.len() < wanted as usize {
            if port > PHP_PORTS + 500 {
                bail!("no free port for PHP");
            }
            if !taken.contains(&port) && free(port) {
                ports.push(port);
            }
            port += 1;
        }
        let mut children = Vec::new();
        for &port in &ports {
            let mut cmd = command(&program);
            if cfg!(windows) {
                // php-cgi exits after 500 requests unless told otherwise.
                cmd.args(["-b", &format!("127.0.0.1:{port}"), "-c"]).arg(&ini).env("PHP_FCGI_MAX_REQUESTS", "0");
            } else {
                let conf = dir.join("php-fpm.conf");
                std::fs::write(&conf, fpm_conf(version, port, &dir)?)?;
                cmd.arg("-y").arg(&conf).arg("-c").arg(&ini).arg("-F");
            }
            let child = cmd.spawn().with_context(|| format!("PHP {version} doesn't start"))?;
            remember(child.id());
            children.push(child);
        }
        let mut pool = Pool { children, ports: ports.clone() };
        let ready = wait_ready_for(&ports, &mut pool.children, PHP_READY_WAIT);
        if let Err(e) = ready {
            end_pool(&mut pool.children);
            let log = php_log(version).ok().and_then(|p| tail(&p)).unwrap_or_default();
            bail!("PHP {version}: {e}{log}");
        }
        self.pools.insert(version.to_owned(), pool);
        Ok(())
    }

    fn start_nginx(&mut self, version: &str, nginx_dir: &Path, port: u16) -> Result<()> {
        if let Some(mut old) = self.nginx.take() {
            let _ = nginx_signal(&old.program, nginx_dir, "stop");
            let _ = old.child.kill();
        }
        let program = envs::nginx(version).context("nginx isn't installed")?;
        if !free(port) {
            bail!("the port {port} is taken by another program: pick another one for the projects");
        }
        let child = command(&program).args(nginx_args(nginx_dir)).spawn().context("nginx doesn't start")?;
        remember(child.id());
        let mut children = vec![child];
        if let Err(e) = wait_ready(&[port], &mut children) {
            for child in &mut children {
                let _ = child.kill();
            }
            let log = tail(&logs_dir()?.join("nginx.error.log")).unwrap_or_default();
            bail!("nginx: {e}{log}");
        }
        self.nginx = Some(Nginx { program, child: children.remove(0), port });
        Ok(())
    }

    /// Deletes the sites of nginx whose project doesn't run.
    fn remove_stale_sites(&self, sites: &Path) {
        for entry in std::fs::read_dir(sites).into_iter().flatten().flatten() {
            let path = entry.path();
            let running = path.file_stem().and_then(|s| s.to_str()).and_then(|s| Uuid::parse_str(s).ok()).is_some_and(|id| self.running.contains_key(&id));
            if !running && path.extension().is_some_and(|e| e == "conf") {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// The pools of the versions running, for nginx.
    fn write_upstreams(&self, nginx_dir: &Path) -> Result<()> {
        let mut text = String::from("# Written by Ronnie: the PHP versions running.\n");
        for (version, pool) in &self.pools {
            text += &format!("upstream {} {{\n", upstream(version));
            for port in &pool.ports {
                text += &format!("    server 127.0.0.1:{port};\n");
            }
            text += "}\n";
        }
        Ok(std::fs::write(nginx_dir.join("upstreams.conf"), text)?)
    }

    /// Stops a project: its site leaves nginx, its pool stops if no other project runs on it, nginx
    /// stops if none runs at all.
    pub fn stop(&mut self, id: Uuid) {
        let Some((version, _)) = self.running.remove(&id) else { return };
        self.stop_unused_dbs();
        let Ok(run) = run_dir() else { return };
        let nginx_dir = run.join("nginx");
        let _ = std::fs::remove_file(nginx_dir.join("sites").join(format!("{id}.conf")));
        if !self.running.values().any(|(v, _)| *v == version)
            && let Some(mut pool) = self.pools.remove(&version)
        {
            end_pool(&mut pool.children);
        }
        let _ = self.write_upstreams(&nginx_dir);
        if self.running.is_empty() {
            self.stop_nginx();
        } else if let Some(n) = &self.nginx {
            let _ = nginx_signal(&n.program, &nginx_dir, "reload");
        }
    }

    fn stop_nginx(&mut self) {
        if let Some(mut n) = self.nginx.take() {
            if let Ok(run) = run_dir() {
                let _ = nginx_signal(&n.program, &run.join("nginx"), "stop");
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline && n.child.try_wait().ok().flatten().is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = n.child.kill();
            let _ = n.child.wait();
        }
    }

    /// Stops everything (Ronnie quits).
    pub fn stop_all(&mut self) {
        let ids: Vec<Uuid> = self.running.keys().copied().collect();
        for id in ids {
            self.stop(id);
        }
        self.stop_nginx();
        for (_, mut pool) in std::mem::take(&mut self.pools) {
            end_pool(&mut pool.children);
        }
        self.manual.clear();
        for (db, server) in std::mem::take(&mut self.dbs) {
            stop_db(&db, server);
        }
        forget_all();
    }
}

/// Stops a database server cleanly (its admin program, as root), killed if it doesn't within a while.
fn stop_db(db: &Db, mut server: DbServer) {
    if let Some(admin) = envs::db_admin(db.kind, &db.version) {
        let _ = command(&admin).args(["--user=root", "--host=127.0.0.1", &format!("--port={}", server.port), "--connect-timeout=3", "shutdown"]).status();
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && server.child.try_wait().ok().flatten().is_none() {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = server.child.kill();
    let _ = server.child.wait();
}

/// Where a database server's data is: `<config dir>/data/<mysql|mariadb>-<line>` (a line's versions
/// share it: 8.4.7's data is 8.4.13's).
pub fn data_dir(db: &Db) -> Result<PathBuf> {
    let line = if db.version == envs::SYSTEM { envs::SYSTEM.to_owned() } else { envs::line(&db.version) };
    let name = format!("{}-{line}", db.kind.name().to_lowercase());
    Ok(crate::config::config_dir().context("no config folder")?.join("data").join(name))
}

/// Ronnie's database servers that have data (one a data folder, run by its newest version
/// installed), each with its port.
pub fn local_servers() -> Vec<(Db, u16)> {
    let mut seen = BTreeSet::new();
    let mut servers = Vec::new();
    for kind in Env::DATABASES {
        for version in envs::installed(kind) {
            let db = Db { kind, version };
            if let Ok(data) = data_dir(&db)
                && has_data(&db)
                && seen.insert(data)
            {
                let port = wanted_port(&db);
                servers.push((db, port));
            }
        }
    }
    servers
}

/// Whether two database servers share their data (a line's versions).
pub fn same_data(a: &Db, b: &Db) -> bool {
    a.kind == b.kind && data_dir(a).ok() == data_dir(b).ok()
}

/// Whether a database server has its data.
pub fn has_data(db: &Db) -> bool {
    data_dir(db).ok().and_then(|d| std::fs::read_dir(d).ok()).is_some_and(|mut entries| entries.next().is_some())
}

/// A database server's first data: its system tables, `root` without a password (as WAMP has it).
fn init_data(db: &Db, data: &Path, log: &Path) -> Result<()> {
    let _ = std::fs::remove_dir_all(data);
    let base = envs::dir(db.kind, &db.version);
    let mut cmd = match db.kind {
        Env::Mariadb => {
            let program = envs::mariadb_install_db(&db.version).context("MariaDB's mariadb-install-db is missing")?;
            std::fs::create_dir_all(data)?;
            let mut cmd = command(&program);
            if !cfg!(windows) {
                cmd.args(["--no-defaults", "--auth-root-authentication-method=normal", "--skip-test-db"]);
                if let Some(base) = &base {
                    cmd.arg(format!("--basedir={}", slashed(base)));
                }
            }
            cmd.arg(format!("--datadir={}", slashed(data)));
            cmd
        }
        _ => {
            let program = envs::db_server(db.kind, &db.version).context("MySQL isn't installed")?;
            std::fs::create_dir_all(data.parent().unwrap_or(data))?;
            let mut cmd = command(&program);
            cmd.arg("--no-defaults");
            if let Some(base) = &base {
                cmd.arg(format!("--basedir={}", slashed(base)));
            }
            cmd.args(["--initialize-insecure".to_owned(), format!("--datadir={}", slashed(data)), format!("--log-error={}", slashed(log))]);
            cmd
        }
    };
    let out = cmd.stderr(Stdio::piped()).stdout(Stdio::piped()).output().context("the database's data can't be made")?;
    if !out.status.success() || !has_data(db) {
        let _ = std::fs::remove_dir_all(data);
        let said = String::from_utf8_lossy(&out.stderr).lines().chain(String::from_utf8_lossy(&out.stdout).lines()).rfind(|l| !l.trim().is_empty()).map(str::to_owned);
        bail!("{}: its data can't be made{}", db.label(""), said.or_else(|| tail(log)).map(|s| format!(": {}", s.trim())).unwrap_or_default());
    }
    Ok(())
}

/// The data of a WAMP's database server, to import.
#[derive(Clone, Debug)]
pub struct WampData {
    pub kind: Env,
    /// "8.4.7".
    pub version: String,
    pub data: PathBuf,
    /// Its port (3306, 3307 for MariaDB beside MySQL).
    pub port: u16,
    /// Its server's settings in its `my.ini` (`sql_mode=""`, InnoDB's...), but for its folders.
    pub settings: Vec<String>,
    /// Its Windows service ("wampmysqld64").
    pub service: Option<String>,
}

/// A WAMP server's `my.ini`: its port and settings, from its server's section (named after its
/// service, "[wampmysqld64]"), the folders left out (Ronnie's are given).
fn wamp_settings(ini: &str) -> (u16, Vec<String>, Option<String>) {
    let mut section = String::new();
    let (mut port, mut settings, mut service) = (None, Vec::new(), None);
    for line in ini.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_lowercase();
            if section.starts_with("wamp") {
                service = Some(section.clone());
            }
            continue;
        }
        if !section.starts_with("wamp") {
            continue;
        }
        let key = line.split('=').next().unwrap_or_default().trim().replace('-', "_").to_lowercase();
        match key.as_str() {
            "port" => port = line.split('=').nth(1).and_then(|p| p.trim().parse().ok()),
            "basedir" | "datadir" | "log_error" | "lc_messages_dir" | "secure_file_priv" | "tmpdir" | "socket" | "general_log_file" | "slow_query_log_file" | "plugin_dir" => {}
            _ => settings.push(line.to_owned()),
        }
    }
    (port.unwrap_or(DB_PORT), settings, service)
}

/// The databases of WAMP (`C:\wamp64` or `C:\wamp`): its MySQL and MariaDB versions with data.
pub fn wamp_data() -> Vec<WampData> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for root in ["C:\\wamp64", "C:\\wamp"] {
        for (kind, folder) in [(Env::Mysql, "mysql"), (Env::Mariadb, "mariadb")] {
            let Ok(entries) = std::fs::read_dir(Path::new(root).join("bin").join(folder)) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(version) = name.strip_prefix(folder).filter(|v| v.starts_with(|c: char| c.is_ascii_digit())) else { continue };
                let data = entry.path().join("data");
                if data.join("mysql").is_dir() {
                    let (port, settings, service) = wamp_settings(&std::fs::read_to_string(entry.path().join("my.ini")).unwrap_or_default());
                    found.push(WampData { kind, version: version.to_owned(), data, port, settings, service });
                }
            }
        }
    }
    found
}

/// Whether WAMP's server of this data runs (its files can't be copied then).
pub fn wamp_runs(wamp: &WampData) -> bool {
    let Some(bin) = wamp.data.parent() else { return false };
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system.processes().values().any(|p| p.exe().is_some_and(|exe| exe.starts_with(bin)))
}

/// Stops WAMP's database services that run (Windows asks the user's leave: they are services), waiting
/// until they did.
pub fn stop_wamp_services(wamps: &[WampData]) -> Result<()> {
    let running: Vec<&WampData> = wamps.iter().filter(|w| wamp_runs(w)).collect();
    if running.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = running.iter().filter_map(|w| w.service.as_deref()).filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric())).collect();
    if names.is_empty() {
        bail!("WAMP's databases run: stop WAMP first (its icon › Stop all services)");
    }
    let stops = names.iter().map(|n| format!("net stop {n}")).collect::<Vec<_>>().join(" & ");
    let script = format!("Start-Process -FilePath cmd.exe -ArgumentList '/c {stops}' -Verb RunAs -Wait -WindowStyle Hidden");
    let _ = command(Path::new("powershell.exe")).args(["-NoProfile", "-NonInteractive", "-Command", &script]).status();
    let deadline = Instant::now() + Duration::from_secs(30);
    while running.iter().any(|w| wamp_runs(w)) {
        if Instant::now() > deadline {
            bail!("WAMP's databases still run: stop WAMP (its icon › Stop all services), then import again");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

/// The settings a database server starts with, WAMP's when imported from it: `<data dir>.cnf`.
fn settings_file(db: &Db) -> Result<PathBuf> {
    let data = data_dir(db)?;
    Ok(data.with_file_name(format!("{}.cnf", data.file_name().unwrap_or_default().to_string_lossy())))
}

/// A new database server's own port, kept in its settings unless it has some: the first from 3306 on
/// that no other server of Ronnie's has (MySQL on 3306, MariaDB beside it on 3307, as WAMP has them),
/// nor another program (a MySQL or MariaDB of the system, running).
fn give_port(db: &Db) -> Result<()> {
    let file = settings_file(db)?;
    if file.is_file() {
        return Ok(());
    }
    let others: BTreeSet<u16> = local_servers().into_iter().filter(|(d, _)| !same_data(d, db)).map(|(_, port)| port).collect();
    let ports = DB_PORT..DB_PORT + 100;
    let port = ports.clone().find(|p| !others.contains(p) && free(*p)).or_else(|| ports.clone().find(|p| !others.contains(p))).unwrap_or(DB_PORT);
    std::fs::write(&file, format!("# {}'s settings, written by Ronnie (its folders are given at start).\n[mysqld]\nport={port}\n", db.label("system")))
        .with_context(|| format!("cannot write {}", file.display()))
}

/// The port a database server would rather have (WAMP's, when imported from it).
/// Whether the server on `port` is Ronnie's main one, the one PHP's "localhost" reaches (its
/// `mysql.sock`): on 3306, or the first of Ronnie's when 3306 is another program's.
fn is_main_db(port: u16) -> bool {
    let ports: BTreeSet<u16> = local_servers().into_iter().map(|(_, p)| p).collect();
    port == DB_PORT || (!ports.contains(&DB_PORT) && ports.first() == Some(&port))
}

fn wanted_port(db: &Db) -> u16 {
    let text = settings_file(db).ok().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    text.lines().find_map(|l| l.trim().strip_prefix("port=").and_then(|p| p.trim().parse().ok())).unwrap_or(DB_PORT)
}

/// The name of a database's folder in a server's data ("chevrin-geli" → "chevrin@002dgeli").
fn db_folder_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '$' { c.to_string() } else { format!("@{:04x}", c as u32) }).collect()
}

/// Whether a server's data holds the database `name`.
pub fn holds_database(db: &Db, name: &str) -> bool {
    data_dir(db).is_ok_and(|d| d.join(db_folder_name(name)).is_dir())
}

/// WAMP's data copied as the data of `db` (of the same line), unless it has data already: its
/// databases, users and passwords; WAMP's settings and port kept. Copied aside, then moved in place.
pub fn import_wamp(wamp: &WampData, db: &Db) -> Result<()> {
    let mut cnf = String::from("# WAMP's settings, imported by Ronnie (its folders are given at start).\n[mysqld]\n");
    for line in &wamp.settings {
        cnf += line;
        cnf.push('\n');
    }
    cnf += &format!("port={}\n", wamp.port);
    let file = settings_file(db)?;
    std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&file, cnf)?;
    if has_data(db) {
        return Ok(());
    }
    if wamp_runs(wamp) {
        bail!("WAMP's {} runs: stop WAMP first (its icon › Stop all services)", wamp.kind.name());
    }
    let target = data_dir(db)?;
    let staging = target.with_file_name(format!("{}.import", target.file_name().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&staging);
    let copied = copy_dir(&wamp.data, &staging);
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).with_context(|| format!("cannot write {}", target.display()))
}

/// A folder copied whole, but for what a server leaves of its last run (its id, its errors).
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_lowercase();
        if lower.ends_with(".pid") || lower.ends_with(".err") {
            continue;
        }
        if entry.file_type()?.is_dir() {
            copy_dir(&path, &to.join(&name))?;
        } else {
            std::fs::copy(&path, to.join(&name)).with_context(|| format!("cannot copy {}", path.display()))?;
        }
    }
    Ok(())
}

/// nginx's arguments: its folder, its configuration, its first log in it (not `/var/log`, out of
/// reach without being root).
fn nginx_args(nginx_dir: &Path) -> Vec<String> {
    let prefix = format!("{}/", slashed(nginx_dir));
    vec!["-p".into(), prefix, "-c".into(), "nginx.conf".into(), "-e".into(), "logs/startup.log".into()]
}

/// Tells nginx to reload its configuration or stop.
fn nginx_signal(program: &Path, nginx_dir: &Path, signal: &str) -> Result<()> {
    let out = command(program).args(nginx_args(nginx_dir)).args(["-s", signal]).stderr(Stdio::piped()).output().context("nginx unreachable")?;
    if !out.status.success() {
        bail!("nginx: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    // Its workers take the new configuration in a moment.
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Waits until every port answers; an error if a program stopped (its configuration refused...) or
/// took too long.
fn wait_ready(ports: &[u16], children: &mut [Child]) -> Result<()> {
    wait_ready_for(ports, children, READY_WAIT)
}

fn wait_ready_for(ports: &[u16], children: &mut [Child], wait: Duration) -> Result<()> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(status) = children.iter_mut().find_map(|c| c.try_wait().ok().flatten()) {
            bail!("stopped at once ({status})");
        }
        if ports.iter().all(|&p| answers(p)) {
            return Ok(());
        }
        if Instant::now() > deadline {
            bail!("no answer after {} s", wait.as_secs());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The end of a log, to say why something didn't start.
fn tail(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let last = lines[lines.len().saturating_sub(3)..].join("\n");
    (!last.is_empty()).then(|| format!("\n{last}"))
}

/// The processes started, by their id (`run/pids`): what a crash leaves running is stopped at the next
/// start.
fn pids_file() -> Option<PathBuf> {
    run_dir().ok().map(|d| d.join("pids"))
}

fn remember(pid: u32) {
    let Some(file) = pids_file() else { return };
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    text += &format!("{pid}\n");
    let _ = std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(file, text);
}

fn forget_all() {
    if let Some(file) = pids_file() {
        let _ = std::fs::remove_file(file);
    }
}

/// Stops what a crash left running: every program of Ronnie's environments folder (nginx's workers
/// too), and the system's nginx it started (remembered by its id, if it still is an nginx: an id may
/// have gone to another program since).
fn kill_leftovers() {
    let Some(env_root) = crate::config::config_dir().map(|d| d.join("env")) else { return };
    let remembered: BTreeSet<u32> = pids_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default().lines().filter_map(|l| l.trim().parse().ok()).collect();
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for (pid, process) in system.processes() {
        let of_env = process.exe().is_some_and(|exe| exe.starts_with(&env_root));
        let name = process.name().to_string_lossy();
        let ours = remembered.contains(&pid.as_u32()) && ["nginx", "mysqld", "mariadbd"].iter().any(|n| name.starts_with(n));
        if of_env || ours {
            process.kill();
        }
    }
    forget_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordpress_settings() {
        let config = wp_config("mon_site", "localhost");
        assert_eq!(quoted_after(&config, "'DB_NAME'").as_deref(), Some("mon_site"));
        assert!(config.contains("define( 'DB_HOST', 'localhost' );") && config.contains("define( 'DB_PASSWORD', '' );"));
        assert_eq!(quoted_after(&config, "$table_prefix").as_deref(), Some("wp_"));
        assert!(valid_site_name("Mon site") && !valid_site_name(".cache") && !valid_site_name("a/b") && !valid_site_name("  ") && !valid_site_name("wamp"));
    }

    #[test]
    fn slugs() {
        let mut p = Project::of_folder(Path::new("/nowhere/Mon Site Été"));
        assert_eq!(p.slug(), "mon-site-ete");
        p.name = "--".into();
        assert_eq!(p.slug(), "site");
        assert_eq!(upstream("8.3.36-1"), "php_8_3_36_1");
    }

    #[test]
    fn kinds_are_found() {
        let base = std::env::temp_dir().join(format!("ronnie-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let wp = base.join("wp");
        std::fs::create_dir_all(wp.join("wp-includes")).unwrap();
        std::fs::write(wp.join("wp-config.php"), "<?php\ndefine( 'DB_NAME', 'chevrin-geli' );\n").unwrap();
        std::fs::write(wp.join("wp-includes/version.php"), "<?php\n$wp_version = '6.9.7';\n").unwrap();
        let found = detect(&wp);
        assert_eq!((found.kind, found.label.as_deref(), found.database.as_deref()), (Kind::WordPress, Some("WordPress 6.9.7"), Some("chevrin-geli")));
        let ps = base.join("ps");
        std::fs::create_dir_all(ps.join("config")).unwrap();
        std::fs::create_dir_all(ps.join("classes")).unwrap();
        std::fs::create_dir_all(ps.join("src/Core")).unwrap();
        std::fs::write(ps.join("config/defines.inc.php"), "<?php").unwrap();
        std::fs::write(ps.join("src/Core/Version.php"), "<?php\nclass Version {\n    const VERSION = '8.2.0';\n}").unwrap();
        let found = detect(&ps);
        assert_eq!((found.kind, found.label.as_deref(), found.php_line.as_deref()), (Kind::PrestaShop, Some("PrestaShop 8.2.0"), Some("8.1")));
        let laravel = base.join("laravel");
        std::fs::create_dir_all(laravel.join("public")).unwrap();
        std::fs::write(laravel.join("artisan"), "").unwrap();
        std::fs::write(laravel.join("public/index.php"), "").unwrap();
        std::fs::write(laravel.join(".env"), "APP_NAME=x\nDB_DATABASE=shop\n").unwrap();
        let found = detect(&laravel);
        assert_eq!((found.kind, found.root.as_str(), found.database.as_deref()), (Kind::Laravel, "public", Some("shop")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_listener_of_a_port_is_found() {
        let netstat = "\r\nConnexions actives\r\n\r\n  Proto  Adresse locale         Adresse distante       État\r\n\
            \x20 TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1180\r\n\
            \x20 TCP    127.0.0.1:3306         127.0.0.1:52011        ESTABLISHED     4242\r\n\
            \x20 TCP    127.0.0.1:3306         0.0.0.0:0              LISTENING       9876\r\n\
            \x20 TCP    [::]:33060             [::]:0                 LISTENING       9876\r\n";
        assert_eq!(netstat_listener(netstat, 3306), Some(9876));
        assert_eq!(netstat_listener(netstat, 33060), Some(9876));
        assert_eq!(netstat_listener(netstat, 3307), None);
    }

    #[test]
    fn projects_are_imported_whole() {
        let base = std::env::temp_dir().join(format!("ronnie-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let site = base.join("www/site");
        std::fs::create_dir_all(site.join("config")).unwrap();
        std::fs::write(site.join("index.php"), "<?php").unwrap();
        std::fs::write(site.join("config/app.php"), "<?php // app").unwrap();
        let dir = base.join("Projets");
        std::fs::create_dir_all(&dir).unwrap();
        let target = import(&site, &dir).unwrap();
        assert_eq!(target, dir.join("site"));
        assert_eq!(std::fs::read_to_string(target.join("config/app.php")).unwrap(), "<?php // app");
        assert!(site.join("index.php").exists());
        assert_eq!(folders_in(&dir), vec![dir.join("site")]);
        assert!(import(&site, &dir).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn wamp_settings_are_kept_but_its_folders() {
        let ini = "[client]\nport=3307\n[wampmariadb64]\nport=3307\nsocket=/tmp/mariadb.sock\nbasedir=\"c:/wamp64/bin/mariadb\"\n\
                   sql_mode=\"\"\nskip_ssl\ninnodb_data_file_path=ibdata1:12M:autoextend:max:500M\nsecure_file_priv=\"c:/wamp64/tmp\"\n[mysqld]\nport=3307\n";
        let (port, settings, service) = wamp_settings(ini);
        assert_eq!((port, service.as_deref()), (3307, Some("wampmariadb64")));
        assert_eq!(settings, ["sql_mode=\"\"", "skip_ssl", "innodb_data_file_path=ibdata1:12M:autoextend:max:500M"]);
        assert_eq!(db_folder_name("chevrin-geli"), "chevrin@002dgeli");
        assert_eq!(db_folder_name("brusson"), "brusson");
    }

    #[test]
    fn prestashop_images_go_to_their_files() {
        let rules = prestashop_images();
        assert!(rules.contains("rewrite ^/(\\d)(-[\\w-]+)?/.+\\.jpg$ /img/p/$1/$1$2.jpg last;"), "{rules}");
        assert!(rules.contains("rewrite ^/(\\d)(\\d)(\\d)(-[\\w-]+)?/.+\\.jpg$ /img/p/$1/$2/$3/$1$2$3$4.jpg last;"), "{rules}");
    }
}
