//! What is saved to disk: the config (settings and profiles, one JSON file the user can edit or export)
//! and the session (open and recently closed tabs, window), which is state rather than config.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use egui::Color32;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::i18n::Lang;
use crate::pane::Axis;

/// How many closed tabs are remembered.
pub const MAX_CLOSED: usize = 15;

/// A tab's split layout, with what each pane runs.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Layout {
    Pane {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        /// Identifies the pane's command history file (see `shell`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history: Option<Uuid>,
    },
    Split {
        axis: Axis,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Layout {
    /// Number of panes.
    pub fn panes(&self) -> usize {
        match self {
            Layout::Pane { .. } => 1,
            Layout::Split { a, b, .. } => a.panes() + b.panes(),
        }
    }

    /// Directories of its panes, in order.
    pub fn cwds(&self) -> Vec<Option<PathBuf>> {
        match self {
            Layout::Pane { cwd, .. } => vec![cwd.clone()],
            Layout::Split { a, b, .. } => {
                let mut all = a.cwds();
                all.extend(b.cwds());
                all
            }
        }
    }

    /// Sets the directories of its panes, in order (as returned by `cwds`).
    pub fn set_cwds(&mut self, cwds: &mut impl Iterator<Item = Option<PathBuf>>) {
        match self {
            Layout::Pane { cwd, .. } => {
                if let Some(new) = cwds.next() {
                    *cwd = new;
                }
            }
            Layout::Split { a, b, .. } => {
                a.set_cwds(cwds);
                b.set_cwds(cwds);
            }
        }
    }

    /// History ids of its panes.
    pub fn histories(&self, out: &mut Vec<Uuid>) {
        match self {
            Layout::Pane { history, .. } => out.extend(*history),
            Layout::Split { a, b, .. } => {
                a.histories(out);
                b.histories(out);
            }
        }
    }
}

impl Default for Layout {
    fn default() -> Self {
        Layout::Pane { cwd: None, history: None }
    }
}

/// Everything needed to rebuild a tab.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct TabState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_color")]
    pub color: Option<Color32>,
    #[serde(default)]
    pub layout: Layout,
    /// Index of the focused pane, in layout order.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub focused: usize,
}

fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// A saved tab that can be opened again at any time.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Profile {
    pub id: Uuid,
    #[serde(flatten)]
    pub tab: TabState,
    /// Commands saved for this profile, written at the prompt on demand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// Fields written by another version of Ronnie: kept as they are, so that running an older or newer
    /// version never erases them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Profile {
    pub fn name<'a>(&'a self, untitled: &'a str) -> &'a str {
        self.tab.name.as_deref().unwrap_or(untitled)
    }
}

/// Everything the user configures, stored in `config.json`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Config {
    #[serde(flatten)]
    pub settings: Settings,
    /// Saved tabs: name, color, split layout and each pane's directory.
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// SSH connections. Passwords are in the OS keychain, not here.
    #[serde(default)]
    pub ssh: Vec<crate::ssh::SshHost>,
    /// Sidebar arrangement of profiles and SSH hosts: first those outside any group, then the groups.
    #[serde(default)]
    pub ungrouped: Vec<Uuid>,
    #[serde(default)]
    pub groups: Vec<Group>,
    /// Commands saved for every terminal (the ⚡ menu), written at the prompt on demand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// Fields written by another version of Ronnie: kept as they are, so that running an older or newer
    /// version never erases them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A named, collapsible set of profiles and SSH hosts in the sidebar.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
    /// Profile or SSH host ids, in display order.
    #[serde(default)]
    pub items: Vec<Uuid>,
    /// A group of local profiles (in the LOCAL section) rather than of SSH hosts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local: bool,
    /// Fields written by another version of Ronnie: kept as they are, so that running an older or newer
    /// version never erases them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Group {
    pub fn new(name: &str, local: bool) -> Self {
        Self { id: Uuid::new_v4(), name: name.to_owned(), collapsed: false, items: Vec::new(), local, extra: Default::default() }
    }
}

impl Config {
    /// Keeps the sidebar arrangement consistent with the existing profiles and hosts: unknown or
    /// duplicate ids are dropped, new items are placed (in the group named by an imported host, if any).
    pub fn normalize(&mut self) {
        let known: Vec<Uuid> = self.profiles.iter().map(|p| p.id).chain(self.ssh.iter().map(|h| h.id)).collect();
        let mut seen = std::collections::HashSet::new();
        let mut keep = |id: &Uuid| known.contains(id) && seen.insert(*id);
        self.ungrouped.retain(&mut keep);
        for group in &mut self.groups {
            group.items.retain(&mut keep);
        }
        for host in &mut self.ssh {
            let Some(name) = host.group.take() else {
                continue;
            };
            if seen.contains(&host.id) {
                continue;
            }
            let index = match self.groups.iter().position(|g| g.name == name) {
                Some(i) => i,
                None => {
                    self.groups.push(Group::new(&name, false));
                    self.groups.len() - 1
                }
            };
            self.groups[index].items.push(host.id);
            seen.insert(host.id);
        }
        // Profiles live in local groups and hosts in SSH groups (groups made before the LOCAL / SSH split
        // may mix them): a group of profiles only becomes local, misplaced items leave their group.
        let is_profile = |id: &Uuid| self.profiles.iter().any(|p| p.id == *id);
        for group in &mut self.groups {
            if !group.local && !group.items.is_empty() && group.items.iter().all(is_profile) {
                group.local = true;
            }
            let local = group.local;
            let (keep, misplaced): (Vec<Uuid>, Vec<Uuid>) = group.items.iter().partition(|id| is_profile(id) == local);
            group.items = keep;
            self.ungrouped.extend(misplaced);
        }
        for id in known {
            if seen.insert(id) {
                self.ungrouped.push(id);
            }
        }
    }

    /// Removes an item from wherever it is in the sidebar.
    pub fn unplace(&mut self, id: Uuid) {
        self.ungrouped.retain(|i| *i != id);
        for group in &mut self.groups {
            group.items.retain(|i| *i != id);
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("config serializes")
    }

    /// Parses the config; the error names the line and column of the mistake.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

/// Loads `config.json`, or builds it from the files used by earlier versions.
pub fn load_config() -> Result<Config> {
    let path = config_path();
    if path.as_ref().is_some_and(|p| p.exists()) {
        let mut config: Config = load(path)?;
        config.normalize();
        return Ok(config);
    }
    #[derive(Deserialize, Default)]
    struct LegacyProfiles {
        #[serde(default)]
        profiles: Vec<Profile>,
    }
    let dir = config_dir();
    let profiles = load::<LegacyProfiles>(dir.as_ref().map(|d| d.join("profiles.json")))?.profiles;
    let settings = load::<Settings>(dir.as_ref().map(|d| d.join("settings.json")))?;
    let mut config = Config { settings, profiles, ..Default::default() };
    config.normalize();
    Ok(config)
}

/// Last modification time, to notice edits made in another editor.
pub fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Opens a folder in the system file manager.
pub fn open_folder(path: &Path) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program).arg(path).spawn();
}

/// Shows the file in the system file manager.
pub fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct SessionTab {
    /// Profile this tab stays in sync with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Uuid>,
    /// SSH host this tab is connected to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<Uuid>,
    #[serde(flatten)]
    pub tab: TabState,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Session {
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
    #[serde(default)]
    pub active: usize,
    /// Most recently closed last.
    #[serde(default)]
    pub closed: Vec<SessionTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
}

/// Window geometry in points. Position and size are those of the normal (not maximized) window.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

/// User preferences.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Settings {
    #[serde(default)]
    pub language: Lang,
    /// Id of a built-in theme (see `theme::PRESETS`).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Show each local pane's working directory in a strip above it.
    #[serde(default = "default_true")]
    pub show_cwd: bool,
    /// Look for a new release on GitHub at startup and every few hours.
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default)]
    pub shortcuts: Shortcuts,
}

/// Shortcuts the user can change (Settings > Shortcuts).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Shortcuts {
    /// Clears the focused pane's screen and scrollback.
    pub clear_pane: Shortcut,
    pub open_settings: Shortcut,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self { clear_pane: Shortcut::command('K'), open_settings: Shortcut::command('P') }
    }
}

/// A keyboard shortcut, stored as text: "Cmd+K", "Ctrl+Shift+K", "Alt+F2"...
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(transparent)]
pub struct Shortcut(pub String);

impl Shortcut {
    /// `key` with the app's usual modifier: Cmd on macOS, Ctrl+Shift elsewhere (plain Ctrl keys belong
    /// to the shell).
    pub fn command(key: char) -> Self {
        Self(if cfg!(target_os = "macos") { format!("Cmd+{key}") } else { format!("Ctrl+Shift+{key}") })
    }

    /// The shortcut `modifiers` + `key` just typed.
    pub fn typed(modifiers: egui::Modifiers, key: egui::Key) -> Self {
        let mut parts = Vec::new();
        if modifiers.mac_cmd {
            parts.push("Cmd");
        }
        if modifiers.ctrl {
            parts.push("Ctrl");
        }
        if modifiers.alt {
            parts.push("Alt");
        }
        if modifiers.shift {
            parts.push("Shift");
        }
        parts.push(key.name());
        Self(parts.join("+"))
    }

    pub fn parse(&self) -> Option<egui::KeyboardShortcut> {
        let mut modifiers = egui::Modifiers::NONE;
        let mut parts: Vec<&str> = self.0.split('+').map(str::trim).collect();
        // "+" itself as the key: "Cmd++".
        if self.0.ends_with("++") {
            parts.truncate(parts.len().saturating_sub(2));
            parts.push("+");
        }
        let key = egui::Key::from_name(parts.pop()?)?;
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "cmd" | "command" | "super" => modifiers.mac_cmd = true,
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                _ => return None,
            }
        }
        modifiers.command = if cfg!(target_os = "macos") { modifiers.mac_cmd } else { modifiers.ctrl };
        Some(egui::KeyboardShortcut::new(modifiers, key))
    }

    /// As shown in the interface: "⌘ K" on macOS, "Ctrl+Shift+K" elsewhere.
    pub fn label(&self) -> String {
        if !cfg!(target_os = "macos") {
            return self.0.clone();
        }
        let Some(shortcut) = self.parse() else { return self.0.clone() };
        let m = shortcut.modifiers;
        // Keys spaced apart ("⇧ ⌘ K"): stuck together they are hard to read.
        let mut keys: Vec<&str> = [(m.ctrl, "⌃"), (m.alt, "⌥"), (m.shift, "⇧"), (m.mac_cmd, "⌘")].into_iter().filter(|(on, _)| *on).map(|(_, s)| s).collect();
        keys.push(shortcut.logical_key.symbol_or_name());
        keys.join(" ")
    }
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    crate::theme::DEFAULT_THEME.to_owned()
}

impl Default for Settings {
    fn default() -> Self {
        Self { language: Lang::default(), theme: default_theme(), show_cwd: true, auto_update: true, shortcuts: Shortcuts::default() }
    }
}

/// A published build (made by scripts/package.sh). Builds made locally are "dev" builds: they keep
/// their config apart, so that trying a change never touches the installed app's profiles.
pub const OFFICIAL: bool = option_env!("RONNIE_OFFICIAL").is_some();

pub fn config_dir() -> Option<PathBuf> {
    // RONNIE_CONFIG_DIR runs an instance with separate profiles and session (handy for testing).
    if let Some(dir) = std::env::var_os("RONNIE_CONFIG_DIR") {
        return Some(dir.into());
    }
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let official = official_dir()?;
        if OFFICIAL {
            return Some(official);
        }
        let dev = directories::ProjectDirs::from("", "", "ronnie-dev")?.config_dir().to_path_buf();
        // The first dev run starts from a copy of the installed app's data.
        if !dev.exists() && official.is_dir() {
            let _ = copy_dir(&official, &dev);
        }
        Some(dev)
    })
    .clone()
}

/// Deletes everything Ronnie stores in its config directory (the instance lock excepted).
pub fn erase_all() -> std::io::Result<()> {
    let Some(dir) = config_dir() else { return Ok(()) };
    for entry in fs::read_dir(&dir)?.flatten() {
        if entry.file_name() == "instance.lock" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() { fs::remove_dir_all(&path)? } else { fs::remove_file(&path)? }
    }
    Ok(())
}

/// Copies `from` into `to` (files and subdirectories), skipping the instance lock.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_name() == "instance.lock" {
            continue;
        }
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// The published app's config directory.
fn official_dir() -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("", "", "ronnie")?.config_dir().to_path_buf();
    // The app used to be called bipbip: carry its profiles and session over.
    if !dir.exists() {
        if let Some(old) = directories::ProjectDirs::from("", "", "bipbip").map(|d| d.config_dir().to_path_buf()).filter(|d| d.is_dir()) {
            let _ = std::fs::rename(old, &dir);
        }
    }
    Some(dir)
}

/// Locks the config directory for this instance. Returns the lock to keep while running, or None when
/// another Ronnie already holds it: two instances writing the same files would undo each other's
/// changes. When the lock can't be checked at all, the instance behaves as the only one.
pub fn lock_instance() -> Result<Option<fs::File>, ()> {
    let Some(dir) = config_dir() else { return Ok(None) };
    let _ = fs::create_dir_all(&dir);
    let Ok(file) = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("instance.lock")) else { return Ok(None) };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Err(()),
        Err(_) => Ok(None),
    }
}

pub fn session_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("session.json"))
}

/// Reads a JSON file; a missing file gives the default value.
pub fn load<T: for<'de> Deserialize<'de> + Default>(path: Option<PathBuf>) -> Result<T> {
    let Some(path) = path else { return Ok(T::default()) };
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("lecture de {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("lecture de {}", path.display())),
    }
}

/// Writes JSON atomically (temp file + rename) so a crash never leaves a truncated file.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp, path).with_context(|| format!("écriture de {}", path.display()))
}

/// Colors are stored as "#rrggbb".
pub(crate) mod hex_color {
    use egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(color: &Option<Color32>, s: S) -> Result<S::Ok, S::Error> {
        match color {
            Some(c) => s.serialize_str(&format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color32>, D::Error> {
        let Some(s) = Option::<String>::deserialize(d)? else { return Ok(None) };
        let hex = s.trim_start_matches('#');
        let v = u32::from_str_radix(hex, 16).map_err(serde::de::Error::custom)?;
        if hex.len() != 6 {
            return Err(serde::de::Error::custom(format!("couleur invalide : {s}")));
        }
        Ok(Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_profiles_into_local_groups() {
        let (p, h) = (Uuid::new_v4(), Uuid::new_v4());
        let mut config = Config::default();
        config.profiles.push(Profile { id: p, tab: TabState::default(), commands: Vec::new(), extra: Default::default() });
        let mut host = crate::ssh::SshHost::new();
        host.id = h;
        config.ssh.push(host);
        let mut only_profiles = Group::new("Caplaser", false);
        only_profiles.items.push(p);
        let mut ssh_group = Group::new("Serveurs", false);
        ssh_group.items.push(h);
        config.groups = vec![only_profiles, ssh_group];
        config.normalize();
        assert!(config.groups[0].local && config.groups[0].items == [p]);
        assert!(!config.groups[1].local && config.groups[1].items == [h]);
    }

    #[test]
    fn erases_everything_but_the_lock() {
        let dir = std::env::temp_dir().join(format!("ronnie-erase-{}", std::process::id()));
        fs::create_dir_all(dir.join("history")).unwrap();
        for f in ["config.json", "session.json", "instance.lock", "history/abc"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        // SAFETY: no other test reads RONNIE_CONFIG_DIR.
        unsafe { std::env::set_var("RONNIE_CONFIG_DIR", &dir) };
        erase_all().unwrap();
        unsafe { std::env::remove_var("RONNIE_CONFIG_DIR") };
        let left: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["instance.lock"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_unknown_fields() {
        let json = r#"{"theme":"ronnie","future":1,"profiles":[{"id":"11111111-1111-4111-8111-111111111111","name":"A","layout":{"type":"pane"},"later":true}]}"#;
        let config = Config::from_json(json).unwrap();
        assert_eq!(config.settings.theme, "ronnie");
        assert!(!config.extra.contains_key("theme"), "known settings are not duplicated");
        let out = config.to_json();
        assert!(out.contains("\"future\": 1") && out.contains("\"later\": true"), "{out}");
        assert_eq!(out.matches("\"theme\"").count(), 1, "{out}");
    }

    #[test]
    fn parses_shortcuts() {
        let k = Shortcut("Cmd+Shift+K".into()).parse().unwrap();
        assert!(k.modifiers.mac_cmd && k.modifiers.shift && !k.modifiers.ctrl);
        assert_eq!(k.logical_key, egui::Key::K);
        let typed = Shortcut::typed(egui::Modifiers { ctrl: true, alt: true, ..Default::default() }, egui::Key::F2);
        assert_eq!(typed.0, "Ctrl+Alt+F2");
        assert_eq!(typed.parse().unwrap().logical_key, egui::Key::F2);
        assert!(Shortcut("Hyper+K".into()).parse().is_none());
    }

    #[test]
    fn profile_round_trip() {
        let p = Profile {
            id: Uuid::new_v4(),
            tab: TabState {
                name: Some("Projet".into()),
                color: Some(Color32::from_rgb(0xf7, 0x76, 0x8e)),
                layout: Layout::Split {
                    axis: Axis::Horizontal,
                    ratio: 0.3,
                    a: Box::new(Layout::Pane { cwd: Some("/tmp".into()), history: Some(Uuid::new_v4()) }),
                    b: Box::new(Layout::Pane { cwd: None, history: None }),
                },
                focused: 1,
            },
            commands: vec!["npm run dev".into()],
            extra: Default::default(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"color\":\"#f7768e\""), "{json}");
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
    }
}
