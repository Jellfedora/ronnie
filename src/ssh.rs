//! SSH hosts: each connection runs the system `ssh` with the host's options. Passwords are saved
//! encrypted next to the config (see `passwords_path`) and typed for ssh by ronnie itself, acting as
//! its SSH_ASKPASS helper.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use egui::Color32;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::hex_color;

/// Set on ssh's environment so that ronnie, started by ssh as askpass helper, knows which host it answers for.
const ASKPASS_ENV: &str = "RONNIE_ASKPASS";
/// Set for ssh without terminal (SFTP): the helper never asks on a terminal.
const ASKPASS_NO_TTY_ENV: &str = "RONNIE_ASKPASS_NO_TTY";
/// Where the window listens for the askpass helper (a socket; a named pipe on Windows).
const ASKPASS_SOCKET_ENV: &str = "RONNIE_ASKPASS_SOCKET";
pub const CONNECT_TIMEOUT_SECS: u32 = 10;

/// How ssh logs in to a host.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SshAuth {
    /// ssh's defaults: the agent and the keys in ~/.ssh, then a password typed if the server asks.
    Auto,
    /// The password saved by Ronnie.
    Password,
    /// A password typed at each connection.
    Ask,
    /// The server's questions (one-time code, two-factor...), answered at each connection.
    Interactive,
    /// A private key file (and, optionally, a saved password: the key's passphrase, or for servers that
    /// also ask for one).
    Key,
}

impl SshAuth {
    pub const ALL: [SshAuth; 5] = [SshAuth::Auto, SshAuth::Password, SshAuth::Ask, SshAuth::Interactive, SshAuth::Key];
}

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
    /// How to log in; none (hosts saved before the choice existed): the key if one is set, then the
    /// saved password if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<SshAuth>,
    /// Private key to use; none means ssh's defaults and agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<PathBuf>,
    /// Jump host(s), as for `ssh -J`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<String>,
    /// Other ssh options, "Key Value" as in ~/.ssh/config (passed as `-o Key=Value`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Folder on the server where sessions start (a path, `~` allowed); none: the home directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_dir: Option<String>,
    /// Sessions start bash with a colored prompt and colored `ls` / `grep` (the server's own
    /// configuration still loaded first), for servers that don't color anything.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub colors: bool,
    /// Whether a password is saved for this host.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub password_saved: bool,
    /// Came from ~/.ssh/config (they can be removed all at once).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub imported: bool,
    /// Commands saved for this host, written at the prompt on demand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// Folders the file manager was last in: on this computer, and on the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sftp_local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sftp_remote: Option<String>,
    /// Fields written by another version of Ronnie: kept as they are, so that running an older or newer
    /// version never erases them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
            auth: None,
            identity_file: None,
            jump: None,
            options: Vec::new(),
            start_dir: None,
            colors: false,
            password_saved: false,
            imported: false,
            commands: Vec::new(),
            sftp_local: None,
            sftp_remote: None,
            extra: Default::default(),
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

    /// The login method, guessed from the key and password for hosts saved without one.
    pub fn auth_method(&self) -> SshAuth {
        match self.auth {
            Some(auth) => auth,
            None if self.identity_file.is_some() => SshAuth::Key,
            None if self.password_saved => SshAuth::Password,
            None => SshAuth::Auto,
        }
    }

    /// The key passed to ssh.
    fn key(&self) -> Option<&PathBuf> {
        match self.auth {
            Some(SshAuth::Key) | None => self.identity_file.as_ref(),
            Some(_) => None,
        }
    }

    /// Whether ssh gets the saved password.
    pub fn uses_saved_password(&self) -> bool {
        self.password_saved && matches!(self.auth, Some(SshAuth::Password | SshAuth::Key) | None)
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
        if let Some(key) = self.key() {
            args.extend(["-i".to_owned(), expand_home(key).display().to_string()]);
            args.extend(["-o".to_owned(), "IdentitiesOnly=yes".to_owned()]);
        }
        // Straight to the chosen method: agent keys tried first could use up the server's attempts.
        let preferred = match self.auth {
            Some(SshAuth::Password | SshAuth::Ask) => Some("password,keyboard-interactive"),
            Some(SshAuth::Interactive) => Some("keyboard-interactive"),
            _ => None,
        };
        if let Some(methods) = preferred.filter(|_| !self.options.iter().any(|o| o.to_lowercase().starts_with("preferredauthentications"))) {
            args.extend(["-o".to_owned(), format!("PreferredAuthentications={methods}")]);
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
        // "--": a host field starting with "-" can't be taken for an option.
        args.push("--".to_owned());
        args.push(self.host.clone());

        let mut env = Vec::new();
        if self.uses_saved_password() {
            if let Ok(exe) = std::env::current_exe() {
                env.push(("SSH_ASKPASS".to_owned(), exe.display().to_string()));
                env.push(("SSH_ASKPASS_REQUIRE".to_owned(), "force".to_owned()));
                env.push((ASKPASS_ENV.to_owned(), self.id.to_string()));
                // The helper asks the window, which checks who is asking (see askpass.rs).
                if let Some(socket) = crate::askpass::socket_path() {
                    env.push((ASKPASS_SOCKET_ENV.to_owned(), socket.display().to_string()));
                }
            }
        }
        Launch { program: "ssh".to_owned(), args, env }
    }
}

impl SshHost {
    /// ssh running the SFTP subsystem (for the file manager): the terminal's options, no terminal, and
    /// every prompt (password, new host key) answered through the askpass helper, i.e. by the window.
    pub fn sftp_command(&self) -> Launch {
        let mut launch = self.command();
        // The terminal command ends with "--", host.
        launch.args.truncate(launch.args.len().saturating_sub(2));
        // No remote command or port forwards (the terminal session has them), and keep-alives so that a
        // dropped network ends ssh within a minute (the file manager then shows "disconnected").
        for option in ["RemoteCommand=none", "RequestTTY=no", "ClearAllForwardings=yes", "ServerAliveInterval=15", "ServerAliveCountMax=3"] {
            launch.args.extend(["-o".to_owned(), option.to_owned()]);
        }
        launch.args.extend(["-T".to_owned(), "-s".to_owned(), "--".to_owned(), self.host.clone(), "sftp".to_owned()]);
        if let Ok(exe) = std::env::current_exe() {
            launch.env.retain(|(k, _)| !k.starts_with("SSH_ASKPASS") && !k.starts_with("RONNIE_ASKPASS"));
            launch.env.push(("SSH_ASKPASS".to_owned(), exe.display().to_string()));
            launch.env.push(("SSH_ASKPASS_REQUIRE".to_owned(), "force".to_owned()));
            launch.env.push((ASKPASS_ENV.to_owned(), self.id.to_string()));
            // Only the window answers: no terminal to fall back to (or a wrong one, if Ronnie was
            // started from a terminal).
            launch.env.push((ASKPASS_NO_TTY_ENV.to_owned(), "1".to_owned()));
            if let Some(socket) = crate::askpass::socket_path() {
                launch.env.push((ASKPASS_SOCKET_ENV.to_owned(), socket.display().to_string()));
            }
        }
        launch
    }
}

impl SshHost {
    /// ssh only forwarding ports (`-L` options go before its last two arguments, "--" and the host):
    /// no terminal, no command, prompts answered in the window.
    pub fn tunnel_command(&self) -> Launch {
        let mut launch = self.sftp_command();
        // It ends with "-T", "-s", "--", host, "sftp"; its forwards are cleared (not these).
        launch.args.truncate(launch.args.len().saturating_sub(5));
        if let Some(i) = launch.args.iter().position(|a| a == "ClearAllForwardings=yes") {
            launch.args.drain(i.saturating_sub(1)..=i);
        }
        launch.args.extend(["-N".to_owned(), "-o".to_owned(), "ExitOnForwardFailure=yes".to_owned(), "--".to_owned(), self.host.clone()]);
        launch
    }
}

impl SshHost {
    /// ssh running `command` on the server, without terminal, prompts answered in the window (like the
    /// file manager's session).
    pub fn exec_command(&self, command: &str) -> Launch {
        let mut launch = self.sftp_command();
        // It ends with "-T", "-s", "--", host, "sftp".
        launch.args.truncate(launch.args.len().saturating_sub(5));
        launch.args.extend(["-T".to_owned(), "--".to_owned(), self.host.clone(), command.to_owned()]);
        launch
    }
}

impl SshHost {
    /// The terminal command for a new session: in the host's start folder, if it has one.
    pub fn session_command(&self) -> Launch {
        self.command_in(self.start_dir.as_deref())
    }

    /// The terminal command, starting the session in `dir` on the server (a path, `~` allowed) when
    /// it can: not when the host has its own remote command (ssh refuses both).
    pub fn command_in(&self, dir: Option<&str>) -> Launch {
        let mut launch = self.command();
        if (dir.is_none() && !self.colors) || self.has_remote_command(&launch) {
            return launch;
        }
        // The command ends with "--", host: a terminal is requested before them, the command comes after.
        let at = launch.args.len().saturating_sub(2);
        launch.args.insert(at, "-t".to_owned());
        // A folder that is gone lands in the home directory; the shell is the usual login shell.
        launch.args.push(match (dir, self.colors) {
            // Through sh: the user's shell may be fish or anything else.
            (dir, true) => format!("exec sh -c {}", sh_quote(&color_script(dir))),
            (Some(dir), false) => format!("cd {} 2>/dev/null; exec \"$SHELL\" -l", remote_path_arg(dir)),
            (None, false) => unreachable!(),
        });
        launch
    }

    /// Whether ssh would run a RemoteCommand of the host's options or ~/.ssh/config (`ssh -G` resolves
    /// the configuration without connecting).
    fn has_remote_command(&self, launch: &Launch) -> bool {
        if self.options.iter().any(|o| o.trim().to_lowercase().starts_with("remotecommand")) {
            return true;
        }
        let mut cmd = std::process::Command::new(&launch.program);
        cmd.arg("-G").args(&launch.args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing.
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.output() {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).lines().any(|l| l.to_lowercase().starts_with("remotecommand ")),
            // Unknown: keep the plain session.
            _ => true,
        }
    }
}

/// Single quotes for sh (also read right by zsh, bash and fish).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The sh script starting a colored bash session (in `dir` if given): a startup file loads the server's
/// usual configuration, then sets the prompt (which also keeps the "user@host: dir" title Ronnie
/// follows) and the color aliases; it deletes itself first thing. Without bash: the usual shell.
fn color_script(dir: Option<&str>) -> String {
    let cd = dir.map(|d| format!("cd {} 2>/dev/null\n", remote_path_arg(d))).unwrap_or_default();
    format!(
        r#"{cd}if command -v bash >/dev/null 2>&1 && f=$(mktemp 2>/dev/null); then
cat > "$f" <<'RONNIE_RC'
rm -f -- "${{BASH_SOURCE[0]}}"
[ -r /etc/profile ] && . /etc/profile
if [ -r ~/.bash_profile ]; then . ~/.bash_profile; elif [ -r ~/.profile ]; then . ~/.profile; elif [ -r ~/.bashrc ]; then . ~/.bashrc; fi
if [ "$(id -u)" = 0 ]; then _ronnie_c='1;31'; else _ronnie_c='1;32'; fi
PS1='\[\e]0;\u@\h: \w\a\]\[\e['"$_ronnie_c"'m\]\u@\h\[\e[0m\]:\[\e[1;34m\]\w\[\e[0m\]\$ '
unset _ronnie_c
alias ls='ls --color=auto' grep='grep --color=auto' egrep='egrep --color=auto' fgrep='fgrep --color=auto' diff='diff --color=auto'
RONNIE_RC
exec bash --rcfile "$f" -i
fi
exec "${{SHELL:-/bin/sh}}" -l"#
    )
}

/// A remote path for the server's shell: quoted, except a leading `~` which the shell must expand.
fn remote_path_arg(dir: &str) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    match dir.strip_prefix('~') {
        Some("") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~/{}", quote(rest.trim_start_matches('/'))),
        _ => quote(dir),
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

pub fn ssh_dir() -> Option<PathBuf> {
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

/// A PuTTY key (.ppk), which OpenSSH can't read.
pub fn is_putty_key(path: &Path) -> bool {
    let mut head = [0u8; 20];
    let n = std::fs::File::open(expand_home(path)).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    head[..n].starts_with(b"PuTTY-User-Key-File")
}

/// Example key path for the host editor's hint, written the way the platform does.
pub fn example_key_path(name: &str) -> String {
    if cfg!(windows) {
        match ssh_dir() {
            Some(dir) => dir.join(name).display().to_string(),
            None => format!(r"C:\Users\nom\.ssh\{name}"),
        }
    } else {
        format!("~/.ssh/{name}")
    }
}

/// Shown with `~` for the home directory.
pub fn display_path(path: &Path) -> String {
    match directories::BaseDirs::new().and_then(|d| path.strip_prefix(d.home_dir()).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

// Saved passwords.
//
// In passwords.json, each encrypted (ChaCha20-Poly1305, bound to its host's id) with a random key kept in
// another file, secret.key: the passwords file alone (a backup, a synced config folder) reveals nothing.
// Both are readable by the user only; like any password saved without asking for a master password,
// this doesn't stop a program running as the user.

fn passwords_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("passwords.json"))
}

/// The key's file, outside the config folder so that a synced or copied config doesn't carry it: in
/// the local data folder (Linux ~/.local/share, Windows AppData\Local), or a folder of its own where
/// that is the config folder (macOS).
fn secret_key_path() -> Option<PathBuf> {
    if std::env::var_os("RONNIE_CONFIG_DIR").is_some() {
        return crate::config::config_dir().map(|d| d.join("secret.key"));
    }
    let name = if crate::config::OFFICIAL { "ronnie" } else { "ronnie-dev" };
    let dirs = directories::ProjectDirs::from("", "", name)?;
    let dir = if dirs.data_local_dir() == dirs.config_dir() {
        dirs.data_local_dir().with_file_name(format!("{name}-secret"))
    } else {
        dirs.data_local_dir().to_path_buf()
    };
    Some(dir.join("secret.key"))
}

fn secret_key(create: bool) -> std::io::Result<chacha20poly1305::Key> {
    use chacha20poly1305::{aead::OsRng, ChaCha20Poly1305, KeyInit};
    let path = secret_key_path().ok_or(std::io::ErrorKind::NotFound)?;
    match std::fs::read(&path) {
        Ok(bytes) if bytes.len() == 32 => return Ok(*chacha20poly1305::Key::from_slice(&bytes)),
        Ok(_) => return Err(std::io::Error::other(format!("{} is damaged", path.display()))),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound || !create => return Err(e),
        Err(_) => {}
    }
    if let Some(dir) = path.parent() {
        crate::config::create_private_dir(dir)?;
    }
    let key = ChaCha20Poly1305::generate_key(&mut OsRng);
    // Written whole to a temporary file, then linked in place: the key file is never seen half written,
    // and one written meanwhile by another instance (or the askpass helper) is never replaced.
    let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&tmp).and_then(|mut file| {
        file.write_all(&key)?;
        file.sync_all()
    });
    let linked = written.and_then(|()| match std::fs::hard_link(&tmp, &path) {
        // No hard links on this file system: a rename, which could replace a key written meanwhile.
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists && !path.exists() => std::fs::rename(&tmp, &path),
        other => other,
    });
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(key),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => secret_key(false),
        Err(e) => Err(e),
    }
}

type Passwords = std::collections::BTreeMap<Uuid, String>;

fn read_passwords() -> std::io::Result<Passwords> {
    crate::config::load(passwords_path()).map_err(std::io::Error::other)
}

fn write_passwords(passwords: &Passwords) -> std::io::Result<()> {
    let path = passwords_path().ok_or(std::io::ErrorKind::NotFound)?;
    crate::config::save(&path, passwords).map_err(std::io::Error::other)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn encrypt(key: &chacha20poly1305::Key, id: Uuid, password: &str) -> std::io::Result<String> {
    use chacha20poly1305::aead::{Aead, AeadCore, OsRng, Payload};
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = ChaCha20Poly1305::new(key)
        .encrypt(&nonce, Payload { msg: password.as_bytes(), aad: id.as_bytes() })
        .map_err(|_| std::io::Error::other("encryption failed"))?;
    Ok(to_hex(&[nonce.as_slice(), &sealed].concat()))
}

fn decrypt(key: &chacha20poly1305::Key, id: Uuid, sealed: &str) -> Option<String> {
    use chacha20poly1305::aead::{Aead, Payload};
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
    let bytes = from_hex(sealed)?;
    let (nonce, sealed) = bytes.split_at_checked(12)?;
    let plain = ChaCha20Poly1305::new(key).decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: id.as_bytes() }).ok()?;
    String::from_utf8(plain).ok()
}

pub fn save_password(id: Uuid, password: &str) -> std::io::Result<()> {
    let key = secret_key(true)?;
    let mut passwords = read_passwords()?;
    passwords.insert(id, encrypt(&key, id, password)?);
    write_passwords(&passwords)
}

pub fn delete_password(id: Uuid) {
    if let Ok(mut passwords) = read_passwords() {
        if passwords.remove(&id).is_some() {
            let _ = write_passwords(&passwords);
        }
    }
}

pub fn load_password(id: Uuid) -> Option<String> {
    let sealed = read_passwords().ok()?.remove(&id)?;
    decrypt(&secret_key(false).ok()?, id, &sealed)
}

/// The hosts with a saved password; None when the file can't be read.
pub fn saved_password_ids() -> Option<std::collections::HashSet<Uuid>> {
    read_passwords().ok().map(|p| p.into_keys().collect())
}

// Askpass helper.

/// When ssh runs ronnie as its askpass helper, answers the prompt and returns true (the process should
/// then exit). The saved password answers the first password prompt, and with a key file the first
/// passphrase prompt; anything else (host key confirmation, a retry after a wrong password) is asked on
/// the terminal.
pub fn run_askpass() -> bool {
    if std::env::var_os(ASKPASS_ENV).is_none() {
        return false;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    // Only the window may hand out the password, after checking that this helper belongs to an ssh it
    // started. Without it (older window, no socket), the user types the password.
    let saved = std::env::var_os(ASKPASS_SOCKET_ENV).and_then(|socket| crate::askpass::ask_window(std::path::Path::new(&socket), &prompt));
    let terminal = std::env::var_os(ASKPASS_NO_TTY_ENV).is_none();
    let answer = saved.or_else(|| terminal.then(|| ask_on_terminal(&prompt)).flatten());
    match answer {
        Some(answer) => {
            println!("{answer}");
            std::process::exit(0);
        }
        None => std::process::exit(1),
    }
}

/// ssh's own question to unlock a key file ("Enter passphrase for key '…': "), not a server's.
pub fn is_key_passphrase_prompt(prompt: &str) -> bool {
    prompt.trim_start().to_lowercase().starts_with("enter passphrase for key")
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

/// Windows: on the console of the ssh that started this helper (the pane's), echo off for secrets.
#[cfg(windows)]
fn ask_on_terminal(prompt: &str) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode, SetConsoleMode};
    // A release build has no console of its own: use ssh's. (A debug build shares it already.)
    // SAFETY: plain call; failing only means a console is already attached, or there is none.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    let input = std::fs::OpenOptions::new().read(true).write(true).open("CONIN$").ok()?;
    let mut out = std::fs::OpenOptions::new().write(true).open("CONOUT$").ok()?;
    let secret = !prompt.to_lowercase().contains("yes/no");
    let handle = input.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    let mut saved: CONSOLE_MODE = 0;
    // SAFETY: `handle` is the open console input; `saved` is valid for writing.
    let hide = secret && unsafe { GetConsoleMode(handle, &mut saved) } != 0;
    if hide {
        // SAFETY: as above.
        unsafe { SetConsoleMode(handle, saved & !ENABLE_ECHO_INPUT) };
    }
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    let mut line = String::new();
    let read = std::io::BufReader::new(&input).read_line(&mut line);
    if hide {
        // SAFETY: as above.
        unsafe { SetConsoleMode(handle, saved) };
        let _ = writeln!(out);
    }
    read.ok()?;
    Some(line.trim_end_matches(['\r', '\n']).to_owned())
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

    #[test]
    fn quotes_remote_paths() {
        assert_eq!(remote_path_arg("/var/www"), "'/var/www'");
        assert_eq!(remote_path_arg("~"), "~");
        assert_eq!(remote_path_arg("~/it's here"), "~/'it'\\''s here'");
        assert_eq!(remote_path_arg("~bob/x"), "'~bob/x'");
    }

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

    #[test]
    fn passwords_round_trip() {
        let key = chacha20poly1305::Key::from([7u8; 32]);
        let (id, other) = (Uuid::new_v4(), Uuid::new_v4());
        let sealed = encrypt(&key, id, "s3cr3t é").unwrap();
        assert!(!sealed.contains("s3cr3t"));
        assert_eq!(decrypt(&key, id, &sealed).as_deref(), Some("s3cr3t é"));
        // Bound to its host, and to the key.
        assert_eq!(decrypt(&key, other, &sealed), None);
        assert_eq!(decrypt(&chacha20poly1305::Key::from([8u8; 32]), id, &sealed), None);
        assert_eq!(decrypt(&key, id, "zz"), None);
    }

    #[test]
    fn builds_ssh_command() {
        let mut host = parse_ssh_config(CONFIG).remove(0);
        host.password_saved = false;
        let launch = host.command();
        let args = launch.args.join(" ");
        assert!(args.starts_with("-p 2222 -l deploy -i "), "{args}");
        assert!(args.ends_with("-o IdentitiesOnly=yes -o ConnectTimeout=10 -- 10.0.0.1"), "{args}");
        assert!(launch.env.is_empty());
    }

    #[test]
    fn auth_method_picks_ssh_options() {
        let mut host = parse_ssh_config(CONFIG).remove(0);
        host.password_saved = true;
        // Saved before the choice existed: key, then password, as before.
        assert_eq!(host.auth_method(), SshAuth::Key);
        assert!(host.command().args.contains(&"IdentitiesOnly=yes".to_owned()));
        assert!(host.uses_saved_password());

        host.auth = Some(SshAuth::Interactive);
        let args = host.command().args.join(" ");
        assert!(!args.contains(" -i ") && args.contains("PreferredAuthentications=keyboard-interactive"), "{args}");
        assert!(!host.uses_saved_password() && host.command().env.is_empty());

        host.auth = Some(SshAuth::Password);
        assert!(host.command().args.join(" ").contains("PreferredAuthentications=password,keyboard-interactive"));
        assert!(host.uses_saved_password());
    }

    #[test]
    fn builds_sftp_command() {
        let host = parse_ssh_config(CONFIG).remove(0);
        let args = host.sftp_command().args.join(" ");
        assert!(args.ends_with("-T -s -- 10.0.0.1 sftp"), "{args}");
        assert_eq!(args.matches(" -- ").count(), 1);
    }
}

#[cfg(all(test, unix))]
mod color_tests {
    use super::*;
    use std::io::{Read, Write};

    /// The colored session, run as ssh would on the server: the login shell (zsh here) runs the
    /// command, which starts bash with Ronnie's startup file.
    #[test]
    fn starts_a_colored_bash() {
        if !Path::new("/bin/bash").exists() || !Path::new("/bin/zsh").exists() {
            return;
        }
        let home = std::env::temp_dir().join(format!("ronnie-colors-{}", std::process::id()));
        let tmp = home.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(home.join(".bashrc"), "export FROM_BASHRC=yes\n").unwrap();
        let command = format!("exec sh -c {}", sh_quote(&color_script(Some("/"))));
        let pty = portable_pty::native_pty_system().openpty(portable_pty::PtySize { rows: 24, cols: 200, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = portable_pty::CommandBuilder::new("/bin/zsh");
        cmd.args(["-f", "-c", &command]);
        cmd.env("HOME", &home);
        cmd.env("TMPDIR", &tmp);
        cmd.env("SHELL", "/bin/zsh");
        cmd.env("TERM", "xterm-256color");
        let mut child = pty.slave.spawn_command(cmd).unwrap();
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let mut writer = pty.master.take_writer().unwrap();
        writer.write_all(b"echo \"[$FROM_BASHRC]\"; alias ls; pwd; ls \"$TMPDIR\" | wc -l; exit\n").unwrap();
        let mut out = Vec::new();
        let _ = reader.read_to_end(&mut out);
        let _ = child.wait();
        let out = String::from_utf8_lossy(&out);
        assert!(out.contains("[yes]"), "the server's own config is loaded: {out}");
        assert!(out.contains("ls --color=auto"), "{out}");
        assert!(out.contains("\x1b[1;3"), "a colored prompt: {out}");
        assert!(out.contains("\x1b]0;"), "the title Ronnie follows the folder with: {out}");
        assert!(out.lines().any(|l| l.trim() == "/"), "started in the folder asked: {out}");
        assert!(out.lines().any(|l| l.trim() == "0"), "the startup file deleted itself: {out}");
        std::fs::remove_dir_all(&home).unwrap();
    }
}
