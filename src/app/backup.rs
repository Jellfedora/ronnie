//! Exporting the configuration to a zip, and importing one, on this computer or another one (even
//! under another system): settings, SSH hosts, database connections, profiles, open tabs, SQL and shell
//! histories, terminal contents. Passwords and SSH key files go in too if asked, sealed with a password
//! chosen for the export.
//!
//! An import replaces everything: the current configuration is moved aside (as by a reset), the zip's
//! is written, and Ronnie restarts. From another system, what only makes sense on the computer it came
//! from is left out: the tabs' and profiles' folders, the file manager's local folder, the shortcuts.

use serde::{Deserialize, Serialize};

use super::*;

/// Bumped when a newer Ronnie writes zips an older one can't read.
const FORMAT: u32 = 1;
const MANIFEST: &str = "manifest.json";
const SECRETS: &str = "secrets.bin";
const MAGIC: &[u8] = b"RONNIE-SECRETS-1";
/// Files and folders of the config folder that travel (the rest belongs to this computer).
const FILES: &[&str] = &["config.json", "session.json", "db-history.json", "notes.json"];
const FOLDERS: &[&str] = &["history", "scrollback", "notes-files"];
/// Where the SSH keys of an import go, in the config folder.
const KEYS: &str = "keys";
pub(super) const MIN_PASSWORD: usize = 8;

/// What an export holds, read before importing it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(super) struct Manifest {
    pub format: u32,
    pub version: String,
    /// "macos", "windows" or "linux".
    pub os: String,
    pub created: i64,
    pub secrets: bool,
    pub hosts: usize,
    pub databases: usize,
    pub profiles: usize,
    pub tabs: usize,
}

impl Manifest {
    pub fn other_os(&self) -> bool {
        self.os != os_id()
    }
}

/// Passwords by host or connection, and SSH key files by host (name, content).
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
struct Secrets {
    passwords: Vec<(Uuid, String)>,
    keys: Vec<(Uuid, String, Vec<u8>)>,
}

/// The export or import window.
pub(super) enum BackupDialog {
    Export { secrets: bool, password: String, again: String, error: Option<String> },
    Import { path: PathBuf, manifest: Manifest, password: String, error: Option<String> },
    Exported(PathBuf),
}

pub(super) fn os_id() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

pub(super) fn os_name(id: &str) -> &str {
    match id {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// Writes the export of config folder `dir` to `out`.
fn write_export(dir: &Path, out: &Path, manifest: &Manifest, secrets: Option<(&Secrets, &str)>) -> Result<(), String> {
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;
    let written: Result<(), Box<dyn std::error::Error>> = (|| {
        let file = std::fs::File::create(out)?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(0o600);
        zip.start_file(MANIFEST, options)?;
        zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;
        for name in FILES {
            if let Ok(bytes) = std::fs::read(dir.join(name)) {
                zip.start_file(*name, options)?;
                zip.write_all(&bytes)?;
            }
        }
        for folder in FOLDERS {
            let Ok(entries) = std::fs::read_dir(dir.join(folder)) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|t| t.is_file()) && safe_name(&name) {
                    zip.start_file(format!("{folder}/{name}"), options)?;
                    zip.write_all(&std::fs::read(entry.path())?)?;
                }
            }
        }
        if let Some((secrets, password)) = secrets {
            zip.start_file(SECRETS, options)?;
            zip.write_all(&seal(&serde_json::to_vec(secrets)?, password)?)?;
        }
        zip.finish()?.flush()?;
        Ok(())
    })();
    written.map_err(|e| {
        let _ = std::fs::remove_file(out);
        format!("{} : {e}", out.display())
    })
}

/// A file name that stays in its folder.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

/// `data` sealed with a key derived from `password` (Argon2id): magic, salt, nonce, ciphertext.
fn seal(data: &[u8], password: &str) -> Result<Vec<u8>, String> {
    use chacha20poly1305::aead::rand_core::RngCore;
    use chacha20poly1305::aead::{Aead, AeadCore, OsRng};
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let key = derive(password, &salt)?;
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = ChaCha20Poly1305::new(&key).encrypt(&nonce, data).map_err(|_| "encryption failed".to_owned())?;
    Ok([MAGIC, &salt, &nonce, &sealed].concat())
}

/// The data sealed by `seal`; None if the password is wrong (or the data damaged).
fn open(bytes: &[u8], password: &str) -> Option<Vec<u8>> {
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
    let rest = bytes.strip_prefix(MAGIC)?;
    let (salt, rest) = rest.split_at_checked(16)?;
    let (nonce, sealed) = rest.split_at_checked(12)?;
    let key = derive(password, salt).ok()?;
    ChaCha20Poly1305::new(&key).decrypt(Nonce::from_slice(nonce), sealed).ok()
}

fn derive(password: &str, salt: &[u8]) -> Result<chacha20poly1305::Key, String> {
    let mut key = chacha20poly1305::Key::default();
    argon2::Argon2::default().hash_password_into(password.as_bytes(), salt, &mut key).map_err(|e| e.to_string())?;
    Ok(key)
}

/// The manifest of the export at `path`.
pub(super) fn read_manifest(path: &Path, t: &Strings) -> Result<Manifest, String> {
    let mut zip = open_zip(path)?;
    let manifest: Manifest = zip.by_name(MANIFEST).ok().and_then(|f| serde_json::from_reader(f).ok()).ok_or_else(|| t.backup_bad_file.to_owned())?;
    if manifest.format > FORMAT {
        return Err(t.backup_newer.to_owned());
    }
    Ok(manifest)
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    zip::ZipArchive::new(file).map_err(|e| format!("{} : {e}", path.display()))
}

/// An export read and checked, ready to be written (nothing on disk changed yet).
struct Prepared {
    config: Config,
    session: Option<Session>,
    /// Other files: path in the config folder, content.
    files: Vec<(String, Vec<u8>)>,
    secrets: Option<Secrets>,
}

/// Reads the export at `path`; `password` opens its secrets. From another system (`other_os`), the
/// folders and shortcuts are left out.
fn prepare(path: &Path, manifest: &Manifest, password: &str, t: &Strings) -> Result<Prepared, String> {
    use std::io::Read as _;
    let mut zip = open_zip(path)?;
    let mut read = |name: &str| -> Option<Vec<u8>> {
        let mut file = zip.by_name(name).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    };
    let text = read("config.json").ok_or_else(|| t.backup_bad_file.to_owned())?;
    let mut config = Config::from_json(&String::from_utf8_lossy(&text)).map_err(|e| format!("config.json : {e}"))?;
    let mut session = match read("session.json") {
        Some(bytes) => Some(serde_json::from_slice::<Session>(&bytes).map_err(|e| format!("session.json : {e}"))?),
        None => None,
    };
    let secrets = match (manifest.secrets, read(SECRETS)) {
        (true, Some(bytes)) => Some(open(&bytes, password).and_then(|b| serde_json::from_slice::<Secrets>(&b).ok()).ok_or_else(|| t.backup_wrong_password.to_owned())?),
        _ => None,
    };
    let mut files = Vec::new();
    if let Some(bytes) = read("db-history.json") {
        files.push(("db-history.json".to_owned(), bytes));
    }
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    for name in names {
        let Some((folder, file)) = name.split_once('/') else { continue };
        if FOLDERS.contains(&folder) && safe_name(file) {
            let mut entry = zip.by_name(&name).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            files.push((name, bytes));
        }
    }

    if manifest.other_os() {
        config.settings.shortcuts = Default::default();
        for profile in &mut config.profiles {
            forget_folders(&mut profile.tab.layout);
        }
        for host in &mut config.ssh {
            host.sftp_local = None;
        }
        if let Some(session) = &mut session {
            let windows = session.windows.iter_mut().flat_map(|w| w.tabs.iter_mut());
            for tab in session.tabs.iter_mut().chain(session.closed.iter_mut()).chain(windows) {
                forget_folders(&mut tab.tab.layout);
            }
            for pane in &mut session.closed_panes {
                forget_folders(pane);
            }
        }
    }
    // Without them, nothing is said to be saved.
    if secrets.is_none() {
        for host in &mut config.ssh {
            host.password_saved = false;
        }
        for db in &mut config.databases {
            db.password_saved = false;
        }
    }
    Ok(Prepared { config, session, files, secrets })
}

fn forget_folders(layout: &mut Layout) {
    match layout {
        Layout::Pane { cwd, .. } => *cwd = None,
        Layout::Split { a, b, .. } => {
            forget_folders(a);
            forget_folders(b);
        }
    }
}

/// Writes an import into config folder `dir` (emptied before): its SSH keys in `dir/keys`, the hosts
/// pointing there when their key isn't found where it was. The passwords to save, returned.
fn write_import(dir: &Path, mut prepared: Prepared) -> Result<Vec<(Uuid, String)>, String> {
    let io = |e: std::io::Error| e.to_string();
    config::create_private_dir(dir).map_err(io)?;
    let secrets = prepared.secrets.take().unwrap_or_default();
    for (host, name, bytes) in &secrets.keys {
        let name = if safe_name(name) { name.as_str() } else { "key" };
        let path = dir.join(KEYS).join(format!("{host}-{name}"));
        config::create_private_dir(&dir.join(KEYS)).map_err(io)?;
        write_private(&path, bytes).map_err(io)?;
        if let Some(h) = prepared.config.ssh.iter_mut().find(|h| h.id == *host)
            && h.identity_file.as_ref().is_none_or(|p| !p.is_file())
        {
            h.identity_file = Some(path);
        }
    }
    for (name, bytes) in &prepared.files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            config::create_private_dir(parent).map_err(io)?;
        }
        write_private(&path, bytes).map_err(io)?;
    }
    config::save_config_file(&dir.join("config.json"), &prepared.config).map_err(|e| format!("{e:#}"))?;
    if let Some(session) = &prepared.session {
        config::save(&dir.join("session.json"), session).map_err(|e| format!("{e:#}"))?;
    }
    Ok(secrets.passwords)
}

/// A file readable by the user only.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(bytes)
}

impl App {
    /// Asks where to write the export, and writes it (`password`: to seal the passwords and keys).
    pub(super) fn export_config(&mut self, password: Option<&str>) -> Result<Option<PathBuf>, String> {
        // What is on disk is what goes: the session and the settings as they are now.
        self.sync();
        let dir = config::config_dir().ok_or("no config folder")?;
        let default = format!("ronnie-config-{}.zip", chrono::Local::now().format("%Y%m%d"));
        let Some(out) = rfd::FileDialog::new().set_file_name(default).add_filter("zip", &["zip"]).save_file() else { return Ok(None) };
        let secrets = password.map(|_| {
            let ids = self.config.ssh.iter().filter(|h| h.password_saved).map(|h| h.id).chain(self.config.databases.iter().filter(|d| d.password_saved).map(|d| d.id)).chain(self.config.settings.subsonic.password_saved.then_some(crate::subsonic::PASSWORD_ID));
            let passwords = ids.filter_map(|id| Some((id, ssh::load_password(id)?))).collect();
            let keys = self
                .config
                .ssh
                .iter()
                .filter_map(|h| {
                    let path = h.identity_file.as_ref()?;
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some((h.id, name, std::fs::read(crate::ssh::expand_home(path)).ok()?))
                })
                .collect();
            Secrets { passwords, keys }
        });
        let tabs = self.tabs.len() + self.others.iter().map(|o| o.tabs.len()).sum::<usize>();
        let manifest = Manifest {
            format: FORMAT,
            version: update::VERSION.to_owned(),
            os: os_id().to_owned(),
            created: chrono::Local::now().timestamp(),
            secrets: secrets.is_some(),
            hosts: self.config.ssh.len(),
            databases: self.config.databases.len(),
            profiles: self.config.profiles.len(),
            tabs,
        };
        write_export(&dir, &out, &manifest, secrets.as_ref().zip(password))?;
        crate::log::info(&format!("configuration exported to {}", out.display()));
        Ok(Some(out))
    }

    /// Replaces the whole configuration by the export at `path`, then restarts. An error (a wrong
    /// password, a damaged file) changes nothing.
    pub(super) fn import_config(&mut self, path: &Path, manifest: &Manifest, password: &str) -> Result<(), String> {
        let t = self.t();
        let prepared = prepare(path, manifest, password, t)?;
        let dir = config::config_dir().ok_or("no config folder")?;
        // As a reset: nothing written by this instance any more, the current files moved aside.
        self.sync();
        self.read_only = true;
        self.config_writable = false;
        let backup = config::erase_all().map_err(|e| e.to_string())?;
        let saved = write_import(&dir, prepared).and_then(|passwords| {
            for (id, password) in passwords {
                ssh::save_password(id, &password).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        if let Err(e) = saved {
            let aside = backup.map(|b| format!("\n{}", b.display())).unwrap_or_default();
            return Err(format!("{e}{aside}"));
        }
        crate::log::info(&format!("configuration imported from {} (made on {} by Ronnie {})", path.display(), manifest.os, manifest.version));
        self.relaunch_replaced();
        Ok(())
    }

    /// The export or import window.
    pub(super) fn backup_window(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(dialog) = &mut self.backup else { return };
        let (mut close, mut export, mut import) = (false, None, false);
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let button = |text: &str, fill: Option<Color32>| {
            let label = egui::RichText::new(text).size(13.5);
            let b = egui::Button::new(if fill.is_some() { label.color(theme.bg) } else { label }).corner_radius(6.0).min_size(Vec2::new(96.0, 30.0));
            if let Some(fill) = fill { b.fill(fill) } else { b }
        };
        let password_field = |ui: &mut Ui, text: &mut String, hint: &str| {
            ui.add(egui::TextEdit::singleline(text).password(true).hint_text(hint).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)))
        };
        let modal = egui::Modal::new(egui::Id::new("backup")).frame(frame).show(ctx, |ui| {
            ui.set_width(460.0);
            match dialog {
                BackupDialog::Export { secrets, password, again, error } => {
                    ui.label(egui::RichText::new(t.backup_export_title).size(17.0).strong());
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(t.backup_export_desc).size(12.5).color(theme.text_muted));
                    ui.add_space(12.0);
                    ui.checkbox(secrets, t.backup_secrets);
                    let ok = if *secrets {
                        ui.label(egui::RichText::new(t.backup_secrets_hint).size(12.0).color(theme.text_muted));
                        ui.add_space(4.0);
                        password_field(ui, password, t.backup_password);
                        let last = password_field(ui, again, t.backup_password_again);
                        let problem = if password.chars().count() < MIN_PASSWORD {
                            Some(t.backup_password_short.replace("{n}", &MIN_PASSWORD.to_string()))
                        } else if password != again {
                            (!again.is_empty()).then(|| t.backup_password_mismatch.to_owned()).or(Some(String::new()))
                        } else {
                            None
                        };
                        if let Some(p) = problem.as_ref().filter(|p| !p.is_empty() && (!password.is_empty())) {
                            ui.label(egui::RichText::new(p).size(12.0).color(theme.ansi[3]));
                        }
                        if problem.is_none() && last.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            export = Some(Some(password.clone()));
                        }
                        problem.is_none()
                    } else {
                        true
                    };
                    if let Some(e) = error {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(format!("⚠  {e}")).size(12.5).color(theme.ansi[1]));
                    }
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(ok, button(t.backup_export, Some(theme.accent))).clicked() {
                            export = Some(secrets.then(|| password.clone()));
                        }
                        if ui.add(button(t.cancel, None)).clicked() {
                            close = true;
                        }
                    });
                }
                BackupDialog::Import { manifest, password, error, .. } => {
                    ui.label(egui::RichText::new(t.backup_import_title).size(17.0).strong());
                    ui.add_space(6.0);
                    let date = chrono::DateTime::from_timestamp(manifest.created, 0).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default();
                    ui.label(egui::RichText::new(t.backup_import_from.replace("{date}", &date).replace("{os}", os_name(&manifest.os)).replace("{version}", &manifest.version)).size(13.0));
                    let counts = t
                        .backup_import_counts
                        .replace("{hosts}", &manifest.hosts.to_string())
                        .replace("{dbs}", &manifest.databases.to_string())
                        .replace("{profiles}", &manifest.profiles.to_string())
                        .replace("{tabs}", &manifest.tabs.to_string());
                    ui.label(egui::RichText::new(counts).size(12.5).color(theme.text_muted));
                    ui.add_space(10.0);
                    egui::Frame::NONE.fill(theme.ansi[3].gamma_multiply(0.12)).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new(format!("⚠  {}", t.backup_import_warning)).size(12.5));
                        if manifest.other_os() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(t.backup_import_other_os.replace("{os}", os_name(&manifest.os))).size(12.5));
                        }
                    });
                    ui.add_space(10.0);
                    if manifest.secrets {
                        let field = password_field(ui, password, t.backup_password);
                        if password.is_empty() && error.is_none() {
                            field.request_focus();
                        }
                        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            import = true;
                        }
                    } else {
                        ui.label(egui::RichText::new(t.backup_import_no_secrets).size(12.5).color(theme.text_muted));
                    }
                    if let Some(e) = error {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(format!("⚠  {e}")).size(12.5).color(theme.ansi[1]));
                    }
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready = !manifest.secrets || !password.is_empty();
                        if ui.add_enabled(ready, button(t.backup_import_button, Some(theme.ansi[1]))).clicked() {
                            import = true;
                        }
                        if ui.add(button(t.cancel, None)).clicked() {
                            close = true;
                        }
                    });
                }
                BackupDialog::Exported(path) => {
                    ui.label(egui::RichText::new(format!("✔  {}", t.backup_exported)).size(17.0).strong());
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(egui::RichText::new(path.display().to_string()).monospace().size(12.0)).wrap());
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(button(t.close, Some(theme.accent))).clicked() {
                            close = true;
                        }
                        if ui.add(button(t.open_location, None)).clicked() {
                            config::reveal(path);
                        }
                    });
                }
            }
        });
        if close || modal.should_close() {
            self.backup = None;
            return;
        }
        if let Some(password) = export {
            match self.export_config(password.as_deref()) {
                Ok(Some(path)) => self.backup = Some(BackupDialog::Exported(path)),
                Ok(None) => {}
                Err(e) => {
                    if let Some(BackupDialog::Export { error, .. }) = &mut self.backup {
                        *error = Some(e);
                    }
                }
            }
        }
        if import {
            let Some(BackupDialog::Import { path, manifest, password, .. }) = self.backup.take() else { return };
            if let Err(e) = self.import_config(&path, &manifest, &password) {
                // Still running: a wrong password or a damaged file changed nothing.
                self.backup = Some(BackupDialog::Import { path, manifest, password: String::new(), error: Some(e) });
            }
        }
    }

    /// Asks for an export to import, and shows what it holds.
    pub(super) fn pick_import(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("zip", &["zip"]).pick_file() else { return };
        match read_manifest(&path, self.t()) {
            Ok(manifest) => self.backup = Some(BackupDialog::Import { path, manifest, password: String::new(), error: None }),
            Err(e) => self.error = Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_with_the_password() {
        let sealed = seal(b"secret", "correct horse").unwrap();
        assert_eq!(open(&sealed, "correct horse").as_deref(), Some(&b"secret"[..]));
        assert_eq!(open(&sealed, "wrong"), None);
    }

    #[test]
    fn exports_and_imports_from_another_system() {
        let base = std::env::temp_dir().join(format!("ronnie-backup-{}", std::process::id()));
        let (from, to) = (base.join("from"), base.join("to"));
        std::fs::create_dir_all(from.join("history")).unwrap();
        std::fs::create_dir_all(from.join("shell")).unwrap();
        let host = Uuid::new_v4();
        let profile = Uuid::new_v4();
        let config = format!(
            r#"{{"theme": "ronnie", "shortcuts": {{"new_tab": "Cmd+Y"}},
                "profiles": [{{"id": "{profile}", "name": "web", "layout": {{"type": "pane", "cwd": "/Users/me/web"}}}}],
                "ssh": [{{"id": "{host}", "name": "srv", "host": "srv.example", "identity_file": "/nowhere/id_ed25519", "password_saved": true, "sftp_local": "/Users/me"}}]}}"#
        );
        std::fs::write(from.join("config.json"), config).unwrap();
        std::fs::write(from.join("history/abc"), "ls\n").unwrap();
        std::fs::write(from.join("shell/zshrc"), "not exported").unwrap();
        let secrets = Secrets { passwords: vec![(host, "pw".into())], keys: vec![(host, "id_ed25519".into(), b"KEY".to_vec())] };
        let manifest = Manifest { format: FORMAT, version: "0".into(), os: if os_id() == "windows" { "macos" } else { "windows" }.into(), created: 0, secrets: true, hosts: 1, databases: 0, profiles: 1, tabs: 0 };
        let zip = base.join("export.zip");
        write_export(&from, &zip, &manifest, Some((&secrets, "longpassword"))).unwrap();

        let t = crate::i18n::Lang::En.strings();
        let read = read_manifest(&zip, t).unwrap();
        assert!(read.secrets && read.other_os());
        assert!(prepare(&zip, &read, "nope", t).is_err(), "a wrong password changes nothing");
        let prepared = prepare(&zip, &read, "longpassword", t).unwrap();
        assert_eq!(prepared.config.settings.shortcuts, config::Shortcuts::default());
        assert_eq!(prepared.config.profiles[0].tab.layout.cwds(), vec![None]);
        assert!(prepared.config.ssh[0].sftp_local.is_none());
        let passwords = write_import(&to, prepared).unwrap();
        assert_eq!(passwords, vec![(host, "pw".to_owned())]);
        assert_eq!(std::fs::read_to_string(to.join("history/abc")).unwrap(), "ls\n");
        assert!(!to.join("shell").exists());
        let imported = Config::from_json(&std::fs::read_to_string(to.join("config.json")).unwrap()).unwrap();
        let key = imported.ssh[0].identity_file.clone().unwrap();
        assert!(key.starts_with(to.join(KEYS)));
        assert_eq!(std::fs::read(key).unwrap(), b"KEY");
        let _ = std::fs::remove_dir_all(&base);
    }
}
