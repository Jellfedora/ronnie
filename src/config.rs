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
/// Named panes closed, kept to be reopened.
pub const MAX_CLOSED_PANES: usize = 20;

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
        /// Name given to the pane (shown in its strip).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Commands typed in the pane when it starts (one a line), and again when relaunched.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        startup: Option<String>,
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
        Layout::Pane { cwd: None, history: None, name: None, startup: None }
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
    /// SSH connections. Passwords are in passwords.json, encrypted (see `ssh`).
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
    /// MariaDB / MySQL servers. Passwords are in passwords.json, encrypted, like the SSH ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<DbConnection>,
    /// Fields written by another version of Ronnie: kept as they are, so that running an older or newer
    /// version never erases them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// What kind of server a database connection talks to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// MariaDB or MySQL.
    #[default]
    Mysql,
    /// Microsoft SQL Server.
    Sqlserver,
}

impl Engine {
    pub fn is_mysql(&self) -> bool {
        *self == Engine::Mysql
    }

    pub fn default_port(self) -> u16 {
        match self {
            Engine::Mysql => 3306,
            Engine::Sqlserver => 1433,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::Mysql => "MariaDB / MySQL",
            Engine::Sqlserver => "SQL Server",
        }
    }
}

/// A MariaDB / MySQL or SQL Server server to browse.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct DbConnection {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Engine::is_mysql")]
    pub engine: Engine,
    /// Address or name; "localhost" goes through the local socket, as for the mysql client. SQL Server:
    /// `server\instance` for a named instance (found through the SQL Server Browser unless a port is given).
    pub host: String,
    #[serde(default = "default_db_port")]
    pub port: u16,
    pub user: String,
    /// Database opened first (none: the list of all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_color")]
    pub color: Option<egui::Color32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub password_saved: bool,
    /// Reached through this SSH host (a port forward): `host` and `port` are then as seen from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<Uuid>,
    /// SQL Server: accept the server's certificate without checking it (often self-signed).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trust_cert: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_db_port() -> u16 {
    3306
}

impl DbConnection {
    pub fn new() -> Self {
        Self { id: Uuid::new_v4(), name: String::new(), engine: Engine::Mysql, host: "localhost".into(), port: 3306, user: String::new(), database: None, color: None, password_saved: false, ssh: None, trust_cert: false, extra: Default::default() }
    }

    /// `user@host:port`, for display.
    pub fn address(&self) -> String {
        if self.port == self.engine.default_port() { format!("{}@{}", self.user, self.host) } else { format!("{}@{}:{}", self.user, self.host, self.port) }
    }
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

    /// Parses the config; the error names the line and column of the mistake. Entries this version
    /// can't read (a profile, host or group written by a newer version, say) don't make the whole file
    /// fail: they are kept aside under "unreadable_<list>" and written back as they were. An unknown
    /// language falls back to the default one.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let mut value: serde_json::Value = serde_json::from_str(text)?;
        if let Some(map) = value.as_object_mut() {
            if map.get("language").is_some_and(|l| serde_json::from_value::<Lang>(l.clone()).is_err()) {
                map.remove("language");
            }
            set_aside::<Profile>(map, "profiles");
            set_aside::<crate::ssh::SshHost>(map, "ssh");
            set_aside::<Group>(map, "groups");
        }
        serde_json::from_value(value)
    }
}

/// Moves the entries of the array `key` that don't parse as `T` to "unreadable_<key>".
fn set_aside<T: for<'de> Deserialize<'de>>(map: &mut serde_json::Map<String, serde_json::Value>, key: &str) {
    let Some(serde_json::Value::Array(items)) = map.get_mut(key) else { return };
    let (good, bad): (Vec<_>, Vec<_>) = std::mem::take(items).into_iter().partition(|v| serde_json::from_value::<T>(v.clone()).is_ok());
    *items = good;
    if !bad.is_empty() {
        let aside = map.entry(format!("unreadable_{key}")).or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if let serde_json::Value::Array(list) = aside {
            list.extend(bad);
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

/// Loads `config.json`, or builds it from the files used by earlier versions.
pub fn load_config() -> Result<Config> {
    let path = config_path();
    if let Some(path) = path.as_ref().filter(|p| p.exists()) {
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut config = Config::from_json(&text).with_context(|| format!("reading {}", path.display()))?;
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
    /// Database server this tab browses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<Uuid>,
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
    /// Named panes closed (or with startup commands), most recent last: each a `Layout::Pane`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed_panes: Vec<Layout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
    /// The other windows, with their own tabs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<SessionWindow>,
}

/// A window besides the main one.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct SessionWindow {
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
    #[serde(default)]
    pub active: usize,
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
    /// Programs may write to the clipboard (OSC 52: vim, tmux, remote hosts...).
    #[serde(default = "default_true")]
    pub clipboard_from_programs: bool,
    /// Terminal text size, in points (Cmd +/- changes it).
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Lines kept above the screen in each terminal.
    #[serde(default = "default_scrollback")]
    pub scrollback: usize,
    /// Size of the whole interface, terminal included (Cmd +/- changes it): 1.0 is 100 %.
    #[serde(default = "default_zoom")]
    pub ui_zoom: f32,
    /// Tell when a command that ran at least `notify_after` seconds ends out of sight.
    #[serde(default = "default_true")]
    pub notify_commands: bool,
    #[serde(default = "default_notify_after")]
    pub notify_after: u64,
    /// Where the notices of the window show (a long command ended in another tab).
    #[serde(default)]
    pub toast_position: ToastPosition,
    /// How to tell, Ronnie being in front (in the background, it is always the system's).
    #[serde(default)]
    pub notify_style: NotifyStyle,
    /// The sidebar folded into a narrow rail of badges.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sidebar_folded: bool,
    /// Sections of the sidebar folded to their title: local terminals, SSH hosts, databases.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_collapsed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ssh_collapsed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub db_collapsed: bool,
    /// Suggest files and folders for the path being typed in local terminals.
    #[serde(default = "default_true")]
    pub path_suggestions: bool,
    /// Show again, when Ronnie reopens, what each terminal showed.
    #[serde(default = "default_true")]
    pub restore_scrollback: bool,
    /// Ask before running a well-known destructive command (see `app::guard`).
    #[serde(default = "default_true")]
    pub metal_guard: bool,
    /// Show the SQL of a change made through the database view (a cell, a row, a column...) before it runs.
    #[serde(default = "default_true")]
    pub db_confirm_changes: bool,
    /// Ask before closing a tab or a pane whose programs are still running.
    #[serde(default = "default_true")]
    pub confirm_close_busy: bool,
    /// Show the animated "Ronnie" logo when the app starts.
    #[serde(default = "default_true")]
    pub splash: bool,
    /// Tips about Ronnie scrolling by at the bottom of the home page.
    #[serde(default = "default_true")]
    pub home_tips: bool,
    /// The home page's cards of SSH hosts and of database connections.
    #[serde(default = "default_true")]
    pub home_hosts: bool,
    #[serde(default = "default_true")]
    pub home_databases: bool,
    /// Claude's plan usage in the strip of the panes where Claude Code runs.
    #[serde(default = "default_true")]
    pub claude_usage: bool,
    /// Ronnie sets itself as Claude Code's status line command at launch (to get the usage); off once
    /// the user removed it.
    #[serde(default = "default_true")]
    pub claude_statusline: bool,
    /// The home page's typing game: the best rounds, the best first.
    #[serde(default)]
    pub typing_scores: Vec<TypingScore>,
    /// Its music: on or off (None: asked the first time).
    #[serde(default)]
    pub game_sound: Option<bool>,
}

/// A round of the home page's typing game.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct TypingScore {
    /// Letters typed right in the minute: the score.
    pub letters: u32,
    pub words: u32,
    pub wpm: u32,
    /// Percent of the keys typed that were right.
    pub accuracy: u32,
    pub combo: u32,
    /// When, in seconds since 1970.
    pub at: i64,
}

/// Places kept on the leaderboard.
pub const TYPING_SCORES: usize = 10;

pub const NOTIFY_AFTER_SECS: std::ops::RangeInclusive<u64> = 3..=600;

fn default_notify_after() -> u64 {
    10
}

pub const UI_ZOOMS: std::ops::RangeInclusive<f32> = 0.6..=2.0;

fn default_zoom() -> f32 {
    1.0
}

pub const FONT_SIZES: std::ops::RangeInclusive<f32> = 9.0..=32.0;
pub const SCROLLBACK_LINES: std::ops::RangeInclusive<usize> = 1_000..=100_000;

fn default_font_size() -> f32 {
    14.0
}

fn default_scrollback() -> usize {
    10_000
}

/// Where a notice shows while Ronnie is in front.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NotifyStyle {
    /// In the window's corner.
    #[default]
    InApp,
    /// The system's notification.
    System,
    Both,
}

impl NotifyStyle {
    pub fn in_app(self) -> bool {
        matches!(self, Self::InApp | Self::Both)
    }

    pub fn system(self) -> bool {
        matches!(self, Self::System | Self::Both)
    }
}

/// A corner or the middle of an edge of the window.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToastPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    #[default]
    BottomRight,
}

impl ToastPosition {
    pub const ALL: [ToastPosition; 6] = [Self::TopLeft, Self::TopCenter, Self::TopRight, Self::BottomLeft, Self::BottomCenter, Self::BottomRight];

    pub fn top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }

    /// -1 left, 0 middle, 1 right.
    pub fn side(self) -> i8 {
        match self {
            Self::TopLeft | Self::BottomLeft => -1,
            Self::TopCenter | Self::BottomCenter => 0,
            Self::TopRight | Self::BottomRight => 1,
        }
    }
}

/// Shortcuts the user can change (Settings > Shortcuts).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Shortcuts {
    pub new_tab: Shortcut,
    pub close_pane: Shortcut,
    pub split_right: Shortcut,
    pub split_down: Shortcut,
    /// Searches the focused pane's displayed text.
    pub find_text: Shortcut,
    /// Searches the commands typed in the focused pane.
    pub find_commands: Shortcut,
    pub reopen_tab: Shortcut,
    /// Clears the focused pane's screen and scrollback.
    pub clear_pane: Shortcut,
    pub open_settings: Shortcut,
    /// Switches an SSH tab between its terminal and its file manager.
    pub toggle_files: Shortcut,
    #[serde(default = "default_new_window")]
    pub new_window: Shortcut,
    #[serde(default = "default_toggle_sidebar")]
    pub toggle_sidebar: Shortcut,
}

fn default_toggle_sidebar() -> Shortcut {
    Shortcut::command('B')
}

fn default_new_window() -> Shortcut {
    Shortcut::command('N')
}

impl Default for Shortcuts {
    fn default() -> Self {
        let mac = cfg!(target_os = "macos");
        Self {
            new_tab: Shortcut::command('T'),
            close_pane: Shortcut::command('W'),
            split_right: Shortcut::command('D'),
            split_down: Shortcut(if mac { "Cmd+Shift+D" } else { "Ctrl+Shift+E" }.into()),
            find_text: Shortcut::command('F'),
            find_commands: Shortcut::command('R'),
            reopen_tab: Shortcut(if mac { "Cmd+Shift+T" } else { "Ctrl+Alt+Shift+T" }.into()),
            clear_pane: Shortcut::command('K'),
            open_settings: Shortcut::command('P'),
            toggle_files: Shortcut::command('E'),
            new_window: default_new_window(),
            toggle_sidebar: default_toggle_sidebar(),
        }
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
        Self { language: Lang::default(), theme: default_theme(), show_cwd: true, auto_update: true, shortcuts: Shortcuts::default(), clipboard_from_programs: true, font_size: default_font_size(), scrollback: default_scrollback(), ui_zoom: default_zoom(), notify_commands: true, notify_after: default_notify_after(), toast_position: ToastPosition::default(), notify_style: NotifyStyle::default(), sidebar_folded: false, local_collapsed: false, ssh_collapsed: false, db_collapsed: false, path_suggestions: true, restore_scrollback: true, metal_guard: true, db_confirm_changes: true, confirm_close_busy: true, splash: true, home_tips: true, home_hosts: true, home_databases: true, claude_usage: true, claude_statusline: true, typing_scores: Vec::new(), game_sound: None }
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

/// Empties Ronnie's config directory (the instance lock excepted). Nothing is deleted: the files move
/// to a sibling "<dir>-backup-<time>" directory, returned, in case the reset was a mistake.
pub fn erase_all() -> std::io::Result<Option<PathBuf>> {
    let Some(dir) = config_dir() else { return Ok(None) };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let name = format!("{}-backup-{stamp}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "ronnie".into()));
    let backup = dir.with_file_name(name);
    create_private_dir(&backup)?;
    for entry in fs::read_dir(&dir)?.flatten() {
        if entry.file_name() == "instance.lock" {
            continue;
        }
        fs::rename(entry.path(), backup.join(entry.file_name()))?;
    }
    Ok(Some(backup))
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
    let _ = create_private_dir(&dir);
    let Ok(file) = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("instance.lock")) else { return Ok(None) };
    // An instance that just relaunched this one (update, reset) may still be quitting: wait for it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2500);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(fs::TryLockError::WouldBlock) => return Err(()),
            Err(_) => return Ok(None),
        }
    }
}

pub fn session_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("session.json"))
}

/// Reads a JSON file; a missing file gives the default value.
pub fn load<T: for<'de> Deserialize<'de> + Default>(path: Option<PathBuf>) -> Result<T> {
    let Some(path) = path else { return Ok(T::default()) };
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("reading {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Writes JSON atomically (temp file flushed to disk, then renamed), readable by the user only: a crash
/// or a power cut never leaves a truncated file, and other accounts can't read hosts or commands.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    // The rename itself survives a power cut once the directory is flushed too.
    #[cfg(unix)]
    if let Some(dir) = path.parent().and_then(|d| fs::File::open(d).ok()) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Saves the config, first keeping the previous file as config.json.bak (at most once a day, so the
/// backup is a known good state rather than the last few seconds).
pub fn save_config_file(path: &Path, config: &Config) -> Result<()> {
    let backup = path.with_extension("json.bak");
    let stale = modified(&backup).is_none_or(|t| t.elapsed().is_ok_and(|age| age.as_secs() > 24 * 3600));
    if stale && path.exists() {
        let _ = fs::copy(path, &backup);
    }
    save(path, config)
}

/// Creates `dir` (and its parents) accessible by the user only.
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    if dir.is_dir() {
        // Also tightens folders made by older versions (or copies) with default permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o077 != 0) {
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
            }
        }
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Reads session.json. A file that can't be read is kept aside as session.json.bad-<time> (rather than
/// overwritten by the next save) and the reason returned, to be shown.
pub fn load_session() -> (Session, Option<String>) {
    let Some(path) = session_path() else { return (Session::default(), None) };
    match load::<Session>(Some(path.clone())) {
        Ok(session) => (session, None),
        Err(e) => {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let aside = path.with_extension(format!("json.bad-{stamp}"));
            let _ = fs::rename(&path, &aside);
            (Session::default(), Some(format!("{e:#} → {}", aside.display())))
        }
    }
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
            return Err(serde::de::Error::custom(format!("invalid color: {s}")));
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
        let backup = erase_all().unwrap().unwrap();
        unsafe { std::env::remove_var("RONNIE_CONFIG_DIR") };
        let left: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["instance.lock"]);
        assert!(backup.join("config.json").exists() && backup.join("history/abc").exists(), "moved aside, not deleted");
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&backup).unwrap();
    }

    #[test]
    fn sets_aside_unreadable_entries() {
        let json = r#"{"language":"klingon","profiles":[{"id":"11111111-1111-4111-8111-111111111111","name":"A","layout":{"type":"pane"}},{"id":"nope","layout":{"type":"hexagon"}}]}"#;
        let config = Config::from_json(json).unwrap();
        assert_eq!(config.settings.language, Lang::default());
        assert_eq!(config.profiles.len(), 1);
        assert!(config.to_json().contains("\"unreadable_profiles\""), "the bad entry is kept");
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
    fn session_keeps_other_windows() {
        // A session from before windows existed still reads.
        let old: Session = serde_json::from_str(r#"{"tabs":[],"active":0}"#).unwrap();
        assert!(old.windows.is_empty());
        let w = WindowState { x: 10.0, y: 20.0, width: 800.0, height: 600.0, maximized: false, fullscreen: false };
        let session = Session { windows: vec![SessionWindow { tabs: Vec::new(), active: 0, window: Some(w) }], ..Default::default() };
        let back: Session = serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
        assert_eq!(back, session);
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
                    a: Box::new(Layout::Pane { cwd: Some("/tmp".into()), history: Some(Uuid::new_v4()), name: Some("api".into()), startup: Some("cd /tmp\nnpm run dev".into()) }),
                    b: Box::new(Layout::Pane { cwd: None, history: None, name: None, startup: None }),
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
