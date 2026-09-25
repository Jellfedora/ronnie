//! SSH hosts: each connection runs the system `ssh` with the host's options. Passwords stay in the OS
//! keychain and are typed for ssh by ronnie itself, acting as its SSH_ASKPASS helper.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use egui::Color32;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::hex_color;

/// Set on ssh's environment so that ronnie, started by ssh as askpass helper, knows which host it answers for.
const ASKPASS_ENV: &str = "RONNIE_ASKPASS";
const KEYCHAIN_SERVICE: &str = "ronnie-ssh";
/// Where passwords were saved when the app was called bipbip.
const LEGACY_KEYCHAIN_SERVICE: &str = "bipbip-ssh";
pub const CONNECT_TIMEOUT_SECS: u32 = 10;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SshHost {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_color")]
    pub color: Option<Color32>,
    /// Group read from ~/.ssh/config or an older config; placed in the sidebar groups, then cleared.
    #[serde(default, skip_serializing)]
    pub group: Option<String>,
    /// Address or name to connect to.
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Private key to use; none means ssh's defaults and agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<PathBuf>,
    /// Jump host(s), as for `ssh -J`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<String>,
    /// Other ssh options, "Key Value" as in ~/.ssh/config (passed as `-o Key=Value`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Whether a password is stored in the keychain for this host.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub password_saved: bool,
    /// Came from ~/.ssh/config (they can be removed all at once).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub imported: bool,
    /// Commands saved for this host, written at the prompt on demand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

impl SshHost {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            color: None,
            group: None,
            host: String::new(),
            port: None,
            user: None,
            identity_file: None,
            jump: None,
            options: Vec::new(),
            password_saved: false,
            imported: false,
            commands: Vec::new(),
        }
    }

    /// `user@host:port`, for display.
    pub fn address(&self) -> String {
        let mut s = String::new();
        if let Some(user) = &self.user {
            s.push_str(user);
            s.push('@');
        }
        s.push_str(&self.host);
        if let Some(port) = self.port.filter(|p| *p != 22) {
            s.push_str(&format!(":{port}"));
        }
        s
    }

    /// The ssh command line and extra environment for connecting.
    pub fn command(&self) -> Launch {
        let mut args = Vec::new();
        if let Some(port) = self.port {
            args.extend(["-p".to_owned(), port.to_string()]);
        }
        if let Some(user) = &self.user {
            args.extend(["-l".to_owned(), user.clone()]);
        }
        if let Some(key) = &self.identity_file {
            args.extend(["-i".to_owned(), expand_home(key).display().to_string()]);
            args.extend(["-o".to_owned(), "IdentitiesOnly=yes".to_owned()]);
        }
        if let Some(jump) = &self.jump {
            args.extend(["-J".to_owned(), jump.clone()]);
        }
        for option in &self.options {
            if let Some((key, value)) = option.trim().split_once(char::is_whitespace) {
                args.extend(["-o".to_owned(), format!("{key}={}", value.trim())]);
            }
        }
        // An unreachable host (VPN off...) fails after a few seconds instead of the system's minute or more.
        if !self.options.iter().any(|o| o.to_lowercase().starts_with("connecttimeout")) {
            args.extend(["-o".to_owned(), format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}")]);
        }
        args.push(self.host.clone());

        let mut env = Vec::new();
        if self.password_saved {
            if let Ok(exe) = std::env::current_exe() {
                env.push(("SSH_ASKPASS".to_owned(), exe.display().to_string()));
                env.push(("SSH_ASKPASS_REQUIRE".to_owned(), "force".to_owned()));
                env.push((ASKPASS_ENV.to_owned(), self.id.to_string()));
            }
        }
        Launch { program: "ssh".to_owned(), args, env }
    }
}

/// A program to run in a pane instead of the user's shell.
#[derive(Clone, Debug)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), directories::BaseDirs::new()) {
        (Ok(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ => path.to_path_buf(),
    }
}

fn ssh_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".ssh"))
}

/// Private keys found in ~/.ssh, shown as choices in the host editor.
pub fn find_keys() -> Vec<PathBuf> {
    let Some(dir) = ssh_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut keys: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let skip = name.ends_with(".pub") || name.starts_with("known_hosts") || name == "config" || name == "authorized_keys";
            // Keys have a public half next to them, or start with a PEM/OpenSSH header.
            !skip && (p.with_file_name(format!("{name}.pub")).exists() || starts_like_key(p))
        })
        .collect();
    keys.sort();
    keys
}

fn starts_like_key(path: &Path) -> bool {
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    head.starts_with("-----BEGIN") || head.starts_with("PuTTY-User-Key-File")
}

/// Shown with `~` for the home directory.
pub fn display_path(path: &Path) -> String {
    match directories::BaseDirs::new().and_then(|d| path.strip_prefix(d.home_dir()).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

// Keychain.

fn keychain_entry(id: Uuid) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &id.to_string())
}

fn legacy_keychain_entry(id: Uuid) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(LEGACY_KEYCHAIN_SERVICE, &id.to_string())
}

pub fn save_password(id: Uuid, password: &str) -> keyring::Result<()> {
    keychain_entry(id)?.set_password(password)
}

pub fn delete_password(id: Uuid) {
    for entry in [keychain_entry(id), legacy_keychain_entry(id)].into_iter().flatten() {
        let _ = entry.delete_credential();
    }
}

pub fn load_password(id: Uuid) -> Option<String> {
    if let Some(password) = keychain_entry(id).ok()?.get_password().ok() {
        return Some(password);
    }
    // Saved under the old name: move it to the new one.
    let legacy = legacy_keychain_entry(id).ok()?;
    let password = legacy.get_password().ok()?;
    if save_password(id, &password).is_ok() {
        let _ = legacy.delete_credential();
    }
    Some(password)
}

// Askpass helper.

/// When ssh runs ronnie as its askpass helper, answers the prompt and returns true (the process should
/// then exit). The saved password answers the first password prompt; anything else (host key
/// confirmation, a retry after a wrong password, a key passphrase) is asked on the terminal.
pub fn run_askpass() -> bool {
    let Ok(id) = std::env::var(ASKPASS_ENV) else { return false };
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let answer = saved_answer(&id, &prompt).or_else(|| ask_on_terminal(&prompt));
    match answer {
        Some(answer) => {
            println!("{answer}");
            std::process::exit(0);
        }
        None => std::process::exit(1),
    }
}

fn saved_answer(id: &str, prompt: &str) -> Option<String> {
    let id: Uuid = id.parse().ok()?;
    let lower = prompt.to_lowercase();
    if !lower.contains("password") || lower.contains("passphrase") {
        return None;
    }
    // One automatic try per ssh process: if it was wrong, ssh asks again and the user types it.
    #[cfg(unix)]
    let ssh_pid = unsafe { libc::getppid() };
    #[cfg(not(unix))]
    let ssh_pid = 0;
    let marker = std::env::temp_dir().join(format!("ronnie-askpass-{ssh_pid}-{id}"));
    if marker.exists() {
        return None;
    }
    let password = load_password(id)?;
    let _ = std::fs::write(&marker, b"");
    Some(password)
}

#[cfg(unix)]
fn ask_on_terminal(prompt: &str) -> Option<String> {
    use std::os::fd::AsRawFd;
    let tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
    let secret = !prompt.to_lowercase().contains("yes/no");
    let fd = tty.as_raw_fd();
    // Hide what is typed for passwords and passphrases.
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    let hide = secret && unsafe { libc::tcgetattr(fd, &mut saved) } == 0;
    if hide {
        let mut silent = saved;
        silent.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &silent) };
    }
    let mut out = &tty;
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    let mut line = String::new();
    let read = std::io::BufReader::new(&tty).read_line(&mut line);
    if hide {
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
        let _ = writeln!(out);
    }
    read.ok()?;
    Some(line.trim_end_matches(['\r', '\n']).to_owned())
}

#[cfg(not(unix))]
fn ask_on_terminal(_prompt: &str) -> Option<String> {
    None
}

// ~/.ssh/config import.

/// Hosts defined in ~/.ssh/config (wildcard patterns skipped). Groups come from "# ===== Name =====" comments.
pub fn import_ssh_config() -> std::io::Result<Vec<SshHost>> {
    let path = ssh_dir().map(|d| d.join("config")).ok_or(std::io::ErrorKind::NotFound)?;
    Ok(parse_ssh_config(&std::fs::read_to_string(path)?))
}

fn parse_ssh_config(text: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut group: Option<String> = None;
    // Index of the host whose block we are in; None inside `Host *` or `Match` blocks.
    let mut current: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(comment) = line.strip_prefix('#') {
            let name = comment.trim().trim_matches('=').trim();
            if comment.trim_start().starts_with("==") && !name.is_empty() {
                group = Some(name.to_owned());
            }
            continue;
        }
        let Some((key, value)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else { continue };
        let value = value.trim().trim_start_matches('=').trim().trim_matches('"');
        match key.to_lowercase().as_str() {
            "host" => {
                let alias = value.split_whitespace().next().unwrap_or("");
                current = None;
                if alias.contains(['*', '?', '!']) || alias.is_empty() {
                    continue;
                }
                let mut host = SshHost::new();
                host.name = alias.to_owned();
                host.host = alias.to_owned();
                host.group = group.clone();
                host.imported = true;
                hosts.push(host);
                current = Some(hosts.len() - 1);
            }
            "match" => current = None,
            _ => {
                let Some(host) = current.map(|i| &mut hosts[i]) else { continue };
                match key.to_lowercase().as_str() {
                    "hostname" => host.host = value.to_owned(),
                    "user" => host.user = Some(value.to_owned()),
                    "port" => host.port = value.parse().ok(),
                    "identityfile" => host.identity_file = Some(expand_home(Path::new(value))),
                    "proxyjump" => host.jump = Some(value.to_owned()),
                    // Implied by choosing a key in ronnie.
                    "identitiesonly" => {}
                    _ => host.options.push(format!("{key} {value}")),
                }
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "
Host *
  ServerAliveInterval 60

# ===== Apaches =====
Host web1 web1-alias
  HostName 10.0.0.1
  User deploy
  Port 2222
  IdentityFile ~/.ssh/id_ed25519_prod
  IdentitiesOnly yes

# ===== Dédiés =====
Host db
  HostName db.example.com
  User root
  ProxyJump web1
  ForwardAgent yes
";

    #[test]
    fn imports_hosts_with_groups() {
        let hosts = parse_ssh_config(CONFIG);
        assert_eq!(hosts.len(), 2);
        let web = &hosts[0];
        assert_eq!((web.name.as_str(), web.host.as_str()), ("web1", "10.0.0.1"));
        assert_eq!(web.group.as_deref(), Some("Apaches"));
        assert_eq!((web.user.as_deref(), web.port), (Some("deploy"), Some(2222)));
        assert!(web.identity_file.as_ref().unwrap().ends_with(".ssh/id_ed25519_prod"));
        assert!(web.options.is_empty());
        let db = &hosts[1];
        assert_eq!(db.group.as_deref(), Some("Dédiés"));
        assert_eq!(db.jump.as_deref(), Some("web1"));
        assert_eq!(db.options, ["ForwardAgent yes"]);
    }

    /// Touches the real keychain, so it only runs on demand: `cargo test -- --ignored keychain`.
    #[test]
    #[ignore]
    fn keychain_password_answers_once() {
        let id = Uuid::new_v4();
        save_password(id, "s3cret").unwrap();
        let prompt = "demo@host's password: ";
        let first = saved_answer(&id.to_string(), prompt);
        let second = saved_answer(&id.to_string(), prompt);
        let passphrase = saved_answer(&id.to_string(), "Enter passphrase for key: ");
        delete_password(id);
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("ronnie-askpass-{}-{id}", unsafe { libc::getppid() })));
        assert_eq!(first.as_deref(), Some("s3cret"));
        assert_eq!(second, None, "a wrong password must not be retried automatically");
        assert_eq!(passphrase, None);
        assert!(load_password(id).is_none());
    }

    #[test]
    fn builds_ssh_command() {
        let mut host = parse_ssh_config(CONFIG).remove(0);
        host.password_saved = false;
        let launch = host.command();
        let args = launch.args.join(" ");
        assert!(args.starts_with("-p 2222 -l deploy -i "), "{args}");
        assert!(args.ends_with("-o IdentitiesOnly=yes -o ConnectTimeout=10 10.0.0.1"), "{args}");
        assert!(launch.env.is_empty());
    }
}
