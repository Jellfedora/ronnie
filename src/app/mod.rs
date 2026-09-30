use egui::{
    Align2, Color32, FontId, Frame, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke, Ui,
    Vec2, ViewportCommand,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::config::{self, Config, Layout, Profile, Session, SessionTab, SessionWindow, TabState, WindowState};
use crate::i18n::{Lang, Strings};
use crate::pane::{self, Direction, Node, PaneId};
use crate::terminal::{Finished, FontSet, LocalUrl, Terminal};
use crate::ssh::{self, SshAuth, SshHost};
use crate::theme::{Preset, Theme, PRESETS, TAB_COLORS};
use crate::update::{self, Updater};

mod bigtext;
mod complete;
mod dbview;
mod easter;
mod editor;
mod files;
mod game;
mod git;
mod home;
mod guard;
mod issues;
mod loading;
mod motion;
mod perms;
mod sql;
mod sqlcomplete;
mod popups;
mod viewer;
mod settings;
mod sidebar;

const SIDEBAR_WIDTH: f32 = 220.0;
/// The folded sidebar: room for the macOS window buttons, then badges.
const RAIL_WIDTH: f32 = 72.0;
/// Window title: dev builds are told apart from the installed app.
const APP_TITLE: &str = if config::OFFICIAL { "Ronnie" } else { "Ronnie (dev)" };
const SIDEBAR_PAD: f32 = 8.0;
/// Space above the first section: room for the macOS traffic lights (the title bar is merged into the sidebar).
const SIDEBAR_TOP: f32 = if cfg!(target_os = "macos") { 40.0 } else { 10.0 };
/// Band under the traffic lights holding the app name.
const LOGO_H: f32 = 46.0;
const SECTION_HEADER_H: f32 = 30.0;
const SECTION_GAP: f32 = 12.0;
const ROW_H: f32 = 32.0;
const ROW_GAP: f32 = 3.0;
/// Height of a group title in the profiles section.
const GROUP_H: f32 = 26.0;
/// Bottom strip of the sidebar holding the settings button.
const FOOTER_H: f32 = 40.0;
/// How often open tabs are compared with what is on disk.
/// Height of the strip above each local pane showing its working directory.
const PANE_HEADER_H: f32 = 26.0;
/// Lines of a terminal kept to show again next time.
const SCROLLBACK_SAVED: usize = 5000;
const SYNC_INTERVAL: f64 = 1.0;
/// Seconds between two automatic update checks.
const UPDATE_INTERVAL: f64 = 6.0 * 3600.0;

pub struct Tab {
    /// Name given by the user; falls back to the focused program's title.
    pub name: Option<String>,
    pub color: Option<Color32>,
    panes: HashMap<PaneId, Terminal>,
    layout: Node,
    focused: PaneId,
    /// Pane rects from the last frame, for keyboard navigation.
    rects: Vec<(PaneId, Rect)>,
    /// Profile this tab keeps up to date.
    profile: Option<Uuid>,
    /// SSH host the tab's panes connect to; its name and color are the host's.
    ssh: Option<Uuid>,
    /// Last known directory of each pane, kept once its shell has exited.
    cwds: HashMap<PaneId, PathBuf>,
    /// Command history id of each pane (see `shell`).
    histories: HashMap<PaneId, Uuid>,
    /// Names given to panes.
    names: HashMap<PaneId, String>,
    /// Commands typed in panes when they start (see `Layout::Pane::startup`), and the panes waiting
    /// for their shell's prompt to get them (since when).
    startup: HashMap<PaneId, String>,
    startup_due: HashMap<PaneId, Instant>,
    /// SSH panes whose connection ended: kept on screen with their last output until reconnected or closed.
    dead: std::collections::HashSet<PaneId>,
    /// Panes whose shell starts the first time the tab is shown: restoring many tabs at once
    /// would otherwise start all their shells together and saturate the CPU.
    pending: Vec<PaneId>,
    /// SSH tabs: the file manager (created when first shown), and whether it is shown instead of the
    /// terminals.
    files: Option<Box<files::FileManager>>,
    show_files: bool,
    /// Local panes showing their repository's changes instead of their terminal.
    git: HashMap<PaneId, Box<git::GitView>>,
    /// Database server this tab browses: its view is shown instead of the terminals (⌘ E switches).
    db: Option<Uuid>,
    db_view: Option<Box<dbview::DbView>>,
    show_db: bool,
    /// A long command ended here while the tab wasn't shown: a ✓ or ✗ on the tab until it is.
    done: Option<Done>,
}

/// The end of a long command, marked on its tab.
pub struct Done {
    pub ok: bool,
    /// Command and duration, shown on hover.
    pub summary: String,
}

impl Tab {
    fn new(layout: Node, panes: HashMap<PaneId, Terminal>) -> Self {
        let focused = layout.first_leaf();
        Self { name: None, color: None, panes, layout, focused, rects: Vec::new(), profile: None, ssh: None, cwds: HashMap::new(), histories: HashMap::new(), names: HashMap::new(), startup: HashMap::new(), startup_due: HashMap::new(), dead: Default::default(), pending: Vec::new(), files: None, show_files: false, git: HashMap::new(), done: None, db: None, db_view: None, show_db: false }
    }

    /// Snapshot of what should be saved for this tab.
    fn state(&mut self) -> TabState {
        for (id, term) in &self.panes {
            if let Some(cwd) = term.cwd() {
                self.cwds.insert(*id, cwd);
            }
        }
        let focused = self.layout.leaves().iter().position(|id| *id == self.focused).unwrap_or(0);
        TabState { name: self.name.clone(), color: self.color, layout: self.layout_of(&self.layout), focused }
    }

    fn layout_of(&self, node: &Node) -> Layout {
        match node {
            Node::Leaf(id) => Layout::Pane {
                cwd: self.cwds.get(id).cloned(),
                history: self.histories.get(id).copied(),
                name: self.names.get(id).cloned(),
                startup: self.startup.get(id).cloned(),
            },
            Node::Split { axis, ratio, a, b } => Layout::Split {
                axis: *axis,
                ratio: *ratio,
                a: Box::new(self.layout_of(a)),
                b: Box::new(self.layout_of(b)),
            },
        }
    }

    /// History file of pane `id`, given an id on first use.
    fn history_path(&mut self, id: PaneId) -> Option<PathBuf> {
        crate::shell::history_path(*self.histories.entry(id).or_insert_with(Uuid::new_v4))
    }

    /// Pane `id` as saved, when worth reopening once closed: named or with startup commands, in a
    /// local tab (an SSH or database tab's panes belong to it).
    fn closed_pane(&mut self, id: PaneId) -> Option<Layout> {
        if self.ssh.is_some() || self.db.is_some() || !(self.names.contains_key(&id) || self.startup.contains_key(&id)) {
            return None;
        }
        if let Some(cwd) = self.panes.get(&id).and_then(Terminal::cwd) {
            self.cwds.insert(id, cwd);
        }
        Some(self.layout_of(&Node::Leaf(id)))
    }

    fn title(&self) -> &str {
        self.name.as_deref().or_else(|| self.panes.get(&self.focused)?.title()).unwrap_or("Terminal")
    }

    /// Removes a pane. Returns false when it was the last one (the tab should close).
    fn remove_pane(&mut self, id: PaneId) -> bool {
        if self.layout.leaves().len() <= 1 {
            return false;
        }
        self.pending.retain(|p| *p != id);
        self.dead.remove(&id);
        self.startup.remove(&id);
        self.startup_due.remove(&id);
        // Hand focus to the pane that visually takes its place.
        let next = [Direction::Left, Direction::Up, Direction::Right, Direction::Down]
            .into_iter()
            .find_map(|d| pane::neighbor(&self.rects, id, d));
        self.layout.remove(id);
        self.panes.remove(&id);
        if self.focused == id {
            self.focused = next.filter(|n| self.panes.contains_key(n)).unwrap_or_else(|| self.layout.first_leaf());
        }
        true
    }
}

struct Rename {
    tab: usize,
    text: String,
    /// Frames left during which the whole name stays selected, so typing replaces it. Counted only once
    /// the mouse is released, otherwise the double-click that opened the editor would move the cursor.
    select_all: u8,
    /// Why the typed name was refused (already used by another profile).
    error: Option<&'static str>,
}

/// What belongs to one window: its tabs and what is going on in them. The window being drawn has its
/// own in `App` itself; the other windows' wait in `App::others`, and are swapped in to be drawn.
/// A git view or a file manager in a window of its own.
struct ToolWindow {
    viewport: egui::ViewportId,
    title: String,
    tool: Tool,
    /// Closing asked while it would interrupt these (transfers, a file being edited): confirm first.
    confirm: Option<Vec<String>>,
}

enum Tool {
    Git(Box<git::GitView>),
    /// With the SSH host it browses (None: this computer).
    Files(Box<files::FileManager>, Option<Uuid>),
}

impl ToolWindow {
    fn new(title: String, tool: Tool) -> Self {
        Self { viewport: egui::ViewportId::from_hash_of(("tool-window", Uuid::new_v4())), title: format!("{title} — {APP_TITLE}"), tool, confirm: None }
    }

    /// What closing it would interrupt.
    fn busy(&self, t: &Strings) -> Vec<String> {
        let Tool::Files(fm, _) = &self.tool else { return Vec::new() };
        let mut busy = Vec::new();
        if fm.busy() {
            busy.push(t.files_transfers.to_lowercase());
        }
        if fm.editor.as_ref().is_some_and(|e| e.is_dirty()) {
            busy.push(t.editor_open_tab.to_owned());
        }
        busy
    }

    fn git(view: Box<git::GitView>) -> Self {
        let repo = view.root().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self::new(format!("{repo} · git"), Tool::Git(view))
    }
}

#[derive(Default)]
struct WindowSlot {
    viewport: egui::ViewportId,
    tabs: Vec<Tab>,
    active: usize,
    rename: Option<Rename>,
    focus_terminal: bool,
    tab_grab: Option<f32>,
    window_title: String,
    window: Option<WindowState>,
    /// Where and how big the window opened (given once, not to fight the user moving it).
    opened_at: Option<WindowState>,
    live: Vec<Option<String>>,
    commands_menu: Option<CommandsMenu>,
    history_search: Option<HistorySearch>,
    paste_confirm: Option<(PaneId, String)>,
    confirm_close: Option<ConfirmClose>,
    window_closing: bool,
    guard_confirm: Option<(PaneId, String, guard::Danger)>,
    pane_drag: Option<PaneId>,
    pane_rename: Option<(PaneId, String, bool)>,
    toasts: Vec<Toast>,
    toast_rects: Vec<Rect>,
    home: Option<String>,
    home_game: bool,
    game: game::Game,
    ronnie_show: Option<(PaneId, f64)>,
    game_folded: bool,
}

pub struct App {
    tabs: Vec<Tab>,
    active: usize,
    theme: Theme,
    fonts: FontSet,
    rename: Option<Rename>,
    focus_terminal: bool,
    /// Terminal contents saved: the output they had then, and when last saved.
    scrollback_seq: HashMap<Uuid, u64>,
    last_scrollback_save: f64,
    error: Option<String>,
    window_title: String,
    next_pane: PaneId,
    /// While a tab is dragged: pointer x minus the tab's left edge.
    tab_grab: Option<f32>,

    config: Config,
    /// Recently closed tabs, most recent last.
    closed: Vec<SessionTab>,
    /// Named panes closed (see `Session::closed_panes`), most recent last.
    closed_panes: Vec<Layout>,
    /// What was last written to disk, to skip identical writes.
    saved_config: Config,
    saved_session: Session,
    /// False while config.json can't be read (invalid JSON...): never overwrite the user's file then.
    config_writable: bool,
    /// Modification time of config.json when last read or written, to notice outside edits.
    config_mtime: Option<std::time::SystemTime>,
    last_sync: f64,
    /// App time of the last user input, to stop the periodic sync once idle.
    last_input: f64,
    window: Option<WindowState>,
    settings_dialog: bool,
    settings_tab: SettingsTab,
    /// Text of the config file editor while it is shown.
    editor: Option<ConfigEditor>,
    /// The log's lines while its page is shown (read when it opens, or on "Refresh").
    logs: Option<Vec<crate::log::Entry>>,
    /// SSH host being created or edited.
    host_editor: Option<HostEditor>,
    /// Database connection being created or edited.
    db_editor: Option<DbEditor>,
    /// A MariaDB / MySQL server runs on this machine (offered in the sidebar while none is set up).
    local_db: bool,
    /// Profile or group being dragged in the sidebar.
    item_drag: Option<ItemDrag>,
    /// Group being renamed: id, typed name, whether the editor was just opened.
    group_rename: Option<(Uuid, String, bool)>,
    /// Profile being renamed in the sidebar.
    item_rename: Option<ItemRename>,
    /// Names being typed in the profiles settings page.
    profile_names: HashMap<Uuid, String>,
    profile_error: Option<&'static str>,
    /// "Delete imported hosts" was clicked once and waits for confirmation.
    confirm_delete_imported: bool,
    updater: Updater,
    /// When the last automatic update check started (app time, seconds).
    last_update_check: Option<f64>,
    /// "Later" was clicked on the update invitation: don't show it again this session.
    update_dismissed: bool,
    /// The user asked for an install: its failure is worth showing in the sidebar.
    update_attempted: bool,
    /// Per tab, the program running in it (if any), for the sidebar's live badge. Refreshed each frame.
    live: Vec<Option<String>>,
    /// macOS menu bar (settings, reload, quit).
    #[cfg(target_os = "macos")]
    menu: Option<crate::menu::MenuBar>,
    /// Shortcut being recorded in the settings: the next key combination replaces it.
    shortcut_capture: Option<ShortcutAction>,
    /// GitHub logo and sign of the horns for the About page, loaded on first use.
    github_icon: Option<egui::TextureHandle>,
    metal_hand: Option<egui::TextureHandle>,
    /// Profile being edited.
    profile_editor: Option<ProfileEditor>,
    /// Saved commands menu open over a pane.
    commands_menu: Option<CommandsMenu>,
    /// History search open over a pane.
    history_search: Option<HistorySearch>,
    /// Lock on the config directory, held while running (see `config::lock_instance`).
    _instance_lock: Option<std::fs::File>,
    /// Another Ronnie runs on the same config: this one writes nothing.
    read_only: bool,
    /// config.json or session.json couldn't be read at startup: the session isn't saved this run.
    session_frozen: bool,
    /// Last error written to the log, to write each one once.
    logged_error: Option<String>,
    /// Multi-line paste waiting for confirmation: pane and text.
    paste_confirm: Option<(PaneId, String)>,
    /// A clicked link waiting for "Open this link?" confirmation.
    link_confirm: Option<String>,
    /// "Reset everything?" dialog shown.
    confirm_reset: bool,
    /// Hands saved SSH passwords to the ssh processes of this window's panes (Unix).
    askpass: crate::askpass::Server,
    /// Questions from ssh processes without a terminal (SFTP): password, new host key.
    ssh_prompts: std::sync::mpsc::Receiver<crate::askpass::Prompt>,
    /// The one being answered, and what is typed.
    ssh_prompt: Option<(crate::askpass::Prompt, String)>,
    /// Path suggestions in terminals: folders listed lately, and the line and suggestion picked.
    path_cache: HashMap<PathBuf, (std::time::Instant, Vec<(String, bool)>)>,
    /// The line, the suggestion picked, and whether it was picked with the arrows (then Enter takes it).
    path_pick: (String, usize, bool),
    /// The line whose suggestions Escape put away.
    path_dismissed: Option<String>,
    /// Close waiting for the user to confirm, because programs are running.
    confirm_close: Option<ConfirmClose>,
    /// The window may close without asking again (confirmed, or restarting).
    close_confirmed: bool,
    /// App time when the startup splash began; None once it is over.
    splash: Option<f64>,
    ctx: egui::Context,
    /// The window being drawn (the main one is ROOT), and the other windows (see `WindowSlot`).
    viewport: egui::ViewportId,
    others: Vec<WindowSlot>,
    /// Git views and file managers opened in windows of their own (shared by all the windows).
    tool_windows: Vec<ToolWindow>,
    /// Windows opened this frame, shown from the next one.
    new_windows: Vec<WindowSlot>,
    /// Where the app-wide dialogs (settings...) show: the window last in front.
    dialog_viewport: egui::ViewportId,
    /// Where this window opened (other windows only).
    opened_at: Option<WindowState>,
    /// This (other) window closes: its tabs close with it.
    window_closing: bool,
    /// A destructive command held back at Enter, waiting for "run it anyway" (see `guard`).
    guard_confirm: Option<(PaneId, String, guard::Danger)>,
    /// Pane being dragged by its strip, to swap it with another.
    pane_drag: Option<PaneId>,
    /// Pane being renamed: its id, the name typed, whether the field was just opened.
    pane_rename: Option<(PaneId, String, bool)>,
    /// The home page is shown instead of the active tab: at launch, after a tab closed (with a line
    /// about it), or from the logo.
    home: Option<String>,
    /// The home page shows the typing game (three clicks on the logo).
    home_game: bool,
    game: game::Game,
    /// "ronnie" was typed in this pane: its show, since when (see `easter`).
    ronnie_show: Option<(PaneId, f64)>,
    /// The sidebar was folded for a round of the game: unfolded after it.
    game_folded: bool,
    /// A pane's startup commands being edited: in which window and tab, and the text typed.
    startup_edit: Option<(egui::ViewportId, usize, PaneId, String)>,
    /// Notices in the window's corner, and where they were drawn (for the hover).
    toasts: Vec<Toast>,
    toast_rects: Vec<Rect>,
}

/// Pushes a window's tabs into their profiles, and returns what the session keeps of them.
fn sync_tabs(tabs: &mut [Tab], config: &mut Config) -> Vec<SessionTab> {
    let mut out = Vec::with_capacity(tabs.len());
    for tab in tabs {
        // An SSH tab shows its host's name and color.
        if let Some(host) = tab.ssh.and_then(|id| config.ssh.iter().find(|h| h.id == id)) {
            tab.name = Some(host.name.clone());
            tab.color = host.color;
        }
        if let Some(c) = tab.db.and_then(|id| config.databases.iter().find(|c| c.id == id)) {
            tab.name = Some(c.name.clone());
            tab.color = c.color;
        }
        let state = tab.state();
        if let Some(id) = tab.profile {
            match config.profiles.iter_mut().find(|p| p.id == id) {
                Some(p) => p.tab = state.clone(),
                None => tab.profile = None,
            }
        }
        out.push(SessionTab { profile: tab.profile, ssh: tab.ssh, db: tab.db, tab: state });
    }
    out
}

/// The database connection editor's state.
struct DbEditor {
    draft: config::DbConnection,
    port: String,
    password: String,
    password_changed: bool,
    reveal: bool,
    is_new: bool,
    error: Option<String>,
    /// "Test the connection": waiting, then its outcome.
    testing: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    tested: Option<Result<String, String>>,
    /// The test's SSH forward, kept until the test ends.
    test_pid: Option<u32>,
}

impl DbEditor {
    fn new(draft: config::DbConnection, is_new: bool) -> Self {
        Self { port: draft.port.to_string(), draft, password: String::new(), password_changed: false, reveal: false, is_new, error: None, testing: None, tested: None, test_pid: None }
    }
}

/// A profile or an SSH host, as shown in the sidebar.
struct Item {
    name: String,
    color: Option<Color32>,
    ssh: bool,
    /// Index of its open tab.
    open: Option<usize>,
    hint: String,
}

/// A line of the profiles section: an item (with its group index) or a group title.
#[derive(Clone, Copy)]
enum Row {
    Item(Option<usize>, Uuid),
    Group(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum Dragged {
    Item(Uuid),
    Group(Uuid),
}

struct ItemDrag {
    what: Dragged,
    /// Pointer y minus the row's top, so the row doesn't jump under the pointer.
    grab: f32,
}

impl ItemDrag {
    fn is(&self, row: Row, config: &Config) -> bool {
        match (self.what, row) {
            (Dragged::Item(id), Row::Item(_, other)) => id == other,
            (Dragged::Group(id), Row::Group(gi)) => config.groups.get(gi).is_some_and(|g| g.id == id),
            _ => false,
        }
    }
}

/// How a pending drop is shown: an insertion line, or a group to drop into.
enum Mark {
    Line(f32),
    Into(Rect),
}

/// Where a dragged item or group would land at pointer position `p`.
fn drop_target(drag: &ItemDrag, p: Pos2, rects: &[(Row, Rect)], config: &Config) -> (TabAction, Mark) {
    let bottom = rects.last().map_or(p.y, |(_, r)| r.max.y + 1.0);
    let group_id = |gi: usize| config.groups[gi].id;
    match drag.what {
        Dragged::Item(id) => {
            // Only the groups shown in this section count (local and SSH groups are listed apart).
            let mut previous_group = None;
            for &(row, rect) in rects {
                match row {
                    Row::Item(_, other) if other == id => {}
                    Row::Group(gi) if rect.contains(p) => return (TabAction::DropItem(id, Some(group_id(gi)), None), Mark::Into(rect)),
                    Row::Item(g, other) if p.y < rect.center().y => {
                        return (TabAction::DropItem(id, g.map(group_id), Some(other)), Mark::Line(rect.min.y - 1.0));
                    }
                    // Above a group title: at the end of whatever comes before it.
                    Row::Group(_) if p.y < rect.min.y => {
                        return (TabAction::DropItem(id, previous_group.map(group_id), None), Mark::Line(rect.min.y - 1.0));
                    }
                    _ => {}
                }
                if let Row::Group(gi) = row {
                    previous_group = Some(gi);
                }
            }
            (TabAction::DropItem(id, previous_group.map(group_id), None), Mark::Line(bottom))
        }
        Dragged::Group(id) => {
            for &(row, rect) in rects {
                if let Row::Group(gi) = row {
                    if group_id(gi) != id && p.y < rect.center().y {
                        return (TabAction::DropGroup(id, Some(group_id(gi))), Mark::Line(rect.min.y - 1.0));
                    }
                }
            }
            (TabAction::DropGroup(id, None), Mark::Line(bottom))
        }
    }
}

/// Where a saved command lives.
#[derive(Clone, Copy, PartialEq)]
enum CommandScope {
    General,
    Profile(Uuid),
    Host(Uuid),
}

/// The ⚡ menu of a pane: saved commands to write at its prompt, and adding / removing them.
struct CommandsMenu {
    tab: usize,
    pane: PaneId,
    new_command: String,
    /// Add to the tab's profile or host rather than to the general commands.
    for_tab: bool,
    /// Opened this frame: the click that opened it (a menu entry...) must not close it.
    fresh: bool,
}

impl CommandsMenu {
    fn new(tab: usize, pane: PaneId) -> Self {
        Self { tab, pane, new_command: String::new(), for_tab: true, fresh: true }
    }
}

/// Search box over a pane's command history.
struct HistorySearch {
    tab: usize,
    pane: PaneId,
    query: String,
    /// The pane's commands, most recent first.
    entries: Vec<String>,
    /// The same, lowercased once for filtering.
    lower: Vec<String>,
    /// Index in the filtered list.
    selected: usize,
    /// Just opened: take the keyboard focus.
    fresh: bool,
    /// Searching the displayed text rather than the typed commands.
    text: bool,
    /// Local pane: its typed commands can be searched too.
    local: bool,
    /// Text mode: (current occurrence, total).
    status: (usize, usize),
}

impl HistorySearch {
    /// Commands containing every word typed (case-insensitive), most recent first.
    fn matches(&self) -> Vec<&str> {
        let words: Vec<String> = self.query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .zip(&self.lower)
            .filter(|(_, lower)| words.iter().all(|w| lower.contains(w)))
            .map(|(e, _)| e.as_str())
            .take(200)
            .collect()
    }
}

/// Something the user asked to close, which may interrupt running programs.
#[derive(Clone, Copy)]
enum CloseRequest {
    Pane(usize, PaneId),
    Tab(usize),
    Window,
    /// Close to start the updated app.
    Restart,
}

/// A close waiting for confirmation, with what it would interrupt.
struct ConfirmClose {
    request: CloseRequest,
    busy: Vec<String>,
    /// "Don't ask again" ticked (offered for a tab or a pane only).
    dont_ask: bool,
}

struct ItemRename {
    id: Uuid,
    text: String,
    error: Option<&'static str>,
    /// Just opened: take focus and select everything.
    fresh: bool,
}

/// "Edit profile" dialog: name, color, each pane's folder and the saved commands.
struct ProfileEditor {
    id: Uuid,
    name: String,
    color: Option<Color32>,
    /// Folder of each pane, as typed.
    cwds: Vec<String>,
    commands: Vec<String>,
    new_command: String,
    error: Option<&'static str>,
}

struct HostEditor {
    draft: SshHost,
    port: String,
    /// Typed path when the key is not one of those found in ~/.ssh.
    custom_key: String,
    keys: Vec<PathBuf>,
    /// The chosen key file, and whether it is a PuTTY key (checked when the choice changes).
    putty_check: Option<(PathBuf, bool)>,
    password: String,
    /// The password field was edited: save (or forget, if emptied) it on save.
    password_changed: bool,
    /// Password shown in clear (the saved one is loaded for that).
    reveal: bool,
    /// Sidebar group to put the host in (None: outside groups).
    group: Option<Uuid>,
    is_new: bool,
    error: Option<String>,
}

impl HostEditor {
    fn new(mut draft: SshHost, is_new: bool, group: Option<Uuid>) -> Self {
        draft.auth = Some(draft.auth_method());
        let keys = ssh::find_keys();
        let custom_key = draft
            .identity_file
            .as_ref()
            .filter(|k| !keys.contains(k))
            .map(|k| ssh::display_path(k))
            .unwrap_or_default();
        Self {
            port: draft.port.map(|p| p.to_string()).unwrap_or_default(),
            draft,
            custom_key,
            keys,
            putty_check: None,
            password: String::new(),
            password_changed: false,
            reveal: false,
            group,
            is_new,
            error: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SettingsTab {
    General,
    Appearance,
    Profiles,
    Ssh,
    ConfigFile,
    Logs,
    Shortcuts,
    Features,
    About,
}

/// A shortcut that can be changed in the settings.
#[derive(Clone, Copy, PartialEq)]
enum ShortcutAction {
    NewTab,
    ClosePane,
    SplitRight,
    SplitDown,
    FindText,
    FindCommands,
    ReopenTab,
    ClearPane,
    OpenSettings,
    ToggleFiles,
    NewWindow,
    ToggleSidebar,
}

impl ShortcutAction {
    const ALL: [ShortcutAction; 12] = [
        Self::NewTab,
        Self::ClosePane,
        Self::SplitRight,
        Self::SplitDown,
        Self::FindText,
        Self::FindCommands,
        Self::ReopenTab,
        Self::ClearPane,
        Self::OpenSettings,
        Self::ToggleFiles,
        Self::NewWindow,
        Self::ToggleSidebar,
    ];

    fn label(self, t: &Strings) -> &'static str {
        match self {
            Self::NewTab => t.shortcut_new_tab,
            Self::ClosePane => t.shortcut_close_pane,
            Self::SplitRight => t.shortcut_split_right,
            Self::SplitDown => t.shortcut_split_down,
            Self::FindText => t.shortcut_find_text,
            Self::FindCommands => t.history_search,
            Self::ReopenTab => t.shortcut_reopen,
            Self::ClearPane => t.shortcut_clear_pane,
            Self::OpenSettings => t.shortcut_open_settings,
            Self::ToggleFiles => t.shortcut_toggle_files,
            Self::NewWindow => t.new_window,
            Self::ToggleSidebar => t.toggle_sidebar,
        }
    }

    fn get(self, s: &config::Shortcuts) -> &config::Shortcut {
        match self {
            Self::NewTab => &s.new_tab,
            Self::ClosePane => &s.close_pane,
            Self::SplitRight => &s.split_right,
            Self::SplitDown => &s.split_down,
            Self::FindText => &s.find_text,
            Self::FindCommands => &s.find_commands,
            Self::ReopenTab => &s.reopen_tab,
            Self::ClearPane => &s.clear_pane,
            Self::OpenSettings => &s.open_settings,
            Self::ToggleFiles => &s.toggle_files,
            Self::NewWindow => &s.new_window,
            Self::ToggleSidebar => &s.toggle_sidebar,
        }
    }

    fn get_mut(self, s: &mut config::Shortcuts) -> &mut config::Shortcut {
        match self {
            Self::NewTab => &mut s.new_tab,
            Self::ClosePane => &mut s.close_pane,
            Self::SplitRight => &mut s.split_right,
            Self::SplitDown => &mut s.split_down,
            Self::FindText => &mut s.find_text,
            Self::FindCommands => &mut s.find_commands,
            Self::ReopenTab => &mut s.reopen_tab,
            Self::ClearPane => &mut s.clear_pane,
            Self::OpenSettings => &mut s.open_settings,
            Self::ToggleFiles => &mut s.toggle_files,
            Self::NewWindow => &mut s.new_window,
            Self::ToggleSidebar => &mut s.toggle_sidebar,
        }
    }
}

struct ConfigEditor {
    text: String,
    /// The config as JSON when the text was loaded: the text is modified when it differs from it.
    base: String,
    error: Option<String>,
}

impl ConfigEditor {
    fn new(json: String) -> Self {
        Self { text: json.clone(), base: json, error: None }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, session: Session, session_error: Option<String>, config: anyhow::Result<Config>, theme: Theme) -> Self {
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            theme,
            fonts: FontSet { size: 14.0, line_height: 1.2 },
            rename: None,
            focus_terminal: true,
            scrollback_seq: HashMap::new(),
            last_scrollback_save: 0.0,
            error: None,
            window_title: String::new(),
            next_pane: 1,
            tab_grab: None,
            config: Config::default(),
            closed: Vec::new(),
            closed_panes: Vec::new(),
            saved_config: Config::default(),
            saved_session: Session::default(),
            config_writable: true,
            config_mtime: config::config_path().and_then(|p| config::modified(&p)),
            last_sync: 0.0,
            last_input: 0.0,
            window: None,
            settings_dialog: false,
            settings_tab: SettingsTab::General,
            editor: None,
            logs: None,
            host_editor: None,
            db_editor: None,
            local_db: crate::db::local_server(),
            item_drag: None,
            group_rename: None,
            item_rename: None,
            profile_names: HashMap::new(),
            profile_error: None,
            confirm_delete_imported: false,
            updater: Updater::new(),
            last_update_check: None,
            update_dismissed: false,
            update_attempted: false,
            live: Vec::new(),
            _instance_lock: None,
            askpass: crate::askpass::Server::start(),
            ssh_prompts: std::sync::mpsc::channel().1,
            ssh_prompt: None,
            viewport: egui::ViewportId::ROOT,
            others: Vec::new(),
            tool_windows: Vec::new(),
            new_windows: Vec::new(),
            dialog_viewport: egui::ViewportId::ROOT,
            opened_at: None,
            window_closing: false,
            guard_confirm: None,
            pane_drag: None,
            pane_rename: None,
            startup_edit: None,
            home: None,
            home_game: false,
            game: game::Game::default(),
            ronnie_show: None,
            game_folded: false,
            toasts: Vec::new(),
            toast_rects: Vec::new(),
            path_cache: HashMap::new(),
            path_pick: (String::new(), 0, false),
            path_dismissed: None,
            read_only: false,
            session_frozen: false,
            confirm_reset: false,
            link_confirm: None,
            paste_confirm: None,
            logged_error: None,
            history_search: None,
            commands_menu: None,
            profile_editor: None,
            github_icon: None,
            metal_hand: None,
            shortcut_capture: None,
            #[cfg(target_os = "macos")]
            menu: None,
            confirm_close: None,
            close_confirmed: false,
            splash: Some(f64::NAN),
            ctx: cc.egui_ctx.clone(),
        };

        match config::lock_instance() {
            Ok(lock) => app._instance_lock = lock,
            Err(()) => {
                app.read_only = true;
                app.error = Some(app.t().already_running.to_owned());
            }
        }
        match config {
            Ok(c) => {
                // A config migrated from older files is written out as config.json on the first sync.
                let exists = config::config_path().is_some_and(|p| p.exists());
                app.saved_config = if exists { c.clone() } else { Config::default() };
                app.config = c;
                // Passwords saved in the system keychain by earlier versions aren't read any more: to be
                // typed again, rather than offered by a "type password" button that can't work.
                if let Some(saved) = ssh::saved_password_ids() {
                    for host in app.config.ssh.iter_mut().filter(|h| h.password_saved && !saved.contains(&h.id)) {
                        host.password_saved = false;
                    }
                }
                app.fonts.size = app.config.settings.font_size.clamp(*config::FONT_SIZES.start(), *config::FONT_SIZES.end());
                crate::terminal::set_default_scrollback(app.config.settings.scrollback);
                cc.egui_ctx.set_zoom_factor(app.config.settings.ui_zoom.clamp(*config::UI_ZOOMS.start(), *config::UI_ZOOMS.end()));
            }
            Err(e) => {
                app.config_writable = false;
                // Profiles are unknown this run: the restored tabs lost their links to them, so the
                // session isn't saved and no history is cleaned up until a restart with a valid file.
                app.session_frozen = true;
                app.error = Some(format!("{} : {e:#}", app.t().config_not_loaded));
            }
        }
        if let Some(e) = session_error {
            app.session_frozen = true;
            app.error = Some(format!("{} : {e}", app.t().session_not_loaded));
        }
        let known = |c: &&SessionTab| c.profile.is_some_and(|id| app.config.profiles.iter().any(|p| p.id == id));
        app.closed = session.closed.iter().filter(known).cloned().collect();
        app.closed_panes = session.closed_panes.clone();
        app.window = session.window;
        app.restore_tabs(&session.tabs, session.active);
        // The other windows, where they were.
        for w in &session.windows {
            let mut slot = WindowSlot { viewport: egui::ViewportId::from_hash_of(("window", Uuid::new_v4())), opened_at: w.window, window: w.window, ..Default::default() };
            app.swap_window(&mut slot);
            app.restore_tabs(&w.tabs, w.active);
            app.swap_window(&mut slot);
            if !slot.tabs.is_empty() {
                app.others.push(slot);
            }
        }
        watch_config(&cc.egui_ctx);
        // Ronnie handles Cmd +/- itself (the zoom is saved in the settings).
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        {
            let (prompts, receiver) = std::sync::mpsc::channel();
            app.askpass.set_prompter(prompts, &cc.egui_ctx);
            app.ssh_prompts = receiver;
        }
        app.saved_session = session;
        // No tab left open last time (or a first launch): the home page, not a new terminal.
        if app.tabs.is_empty() {
            app.home = Some(String::new());
        }
        if app.read_only {
            app.config_writable = false;
        } else if !app.session_frozen {
            app.forget_unused_histories();
        }
        #[cfg(target_os = "macos")]
        {
            app.menu = crate::menu::MenuBar::install(&cc.egui_ctx, app.t(), &app.config.settings.shortcuts.open_settings, &app.config.settings.shortcuts.new_window);
        }
        if !app.config.settings.splash {
            app.splash = None;
        }
        // Development: open the settings on a page right away (to look at them).
        if cfg!(debug_assertions) {
            if std::env::var_os("RONNIE_DEMO_TOAST").is_some() {
                app.splash = None;
                app.toasts.push(Toast { ok: true, title: app.t().command_done.to_owned(), body: "sleep 15 && echo \"Terminé\"\nAcqpa  ·  15 s".into(), tab: 0, at: std::time::Instant::now() });
            }
            if let Ok(page) = std::env::var("RONNIE_OPEN_SETTINGS") {
                app.settings_dialog = true;
                app.splash = None;
                app.settings_tab = match page.as_str() {
                    "appearance" => SettingsTab::Appearance,
                    "shortcuts" => SettingsTab::Shortcuts,
                    "profiles" => SettingsTab::Profiles,
                    "ssh" => SettingsTab::Ssh,
                    "config" => SettingsTab::ConfigFile,
                    "about" => SettingsTab::About,
                    _ => SettingsTab::General,
                };
            }
        }
        app
    }

    /// Interface texts in the chosen language.
    fn t(&self) -> &'static Strings {
        self.config.settings.language.strings()
    }

    /// Opens a tab rebuilt from a saved state (profile, closed tab or previous session).
    /// Shells start lazily: see `Tab::pending`.
    fn open_tab(&mut self, state: &TabState, profile: Option<Uuid>) {
        let (mut cwds, mut histories) = (HashMap::new(), HashMap::new());
        let (mut names, mut startup) = (HashMap::new(), HashMap::new());
        let layout = self.build(&state.layout, &mut cwds, &mut histories, &mut names, &mut startup);
        let mut tab = Tab::new(layout, HashMap::new());
        tab.pending = tab.layout.leaves();
        tab.cwds = cwds;
        tab.histories = histories;
        tab.names = names;
        tab.startup = startup;
        if let Some(id) = tab.layout.leaves().get(state.focused) {
            tab.focused = *id;
        }
        tab.name = state.name.clone();
        tab.color = state.color;
        tab.profile = profile.filter(|id| self.config.profiles.iter().any(|p| p.id == *id));
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.home = None;
        self.focus_terminal = true;
    }

    /// Turns a saved layout into a tree with fresh pane ids, collecting each pane's directory.
    fn build(&mut self, layout: &Layout, cwds: &mut HashMap<PaneId, PathBuf>, histories: &mut HashMap<PaneId, Uuid>, names: &mut HashMap<PaneId, String>, startup: &mut HashMap<PaneId, String>) -> Node {
        match layout {
            Layout::Pane { cwd, history, name, startup: commands } => {
                let id = self.next_pane;
                self.next_pane += 1;
                if let Some(cwd) = cwd {
                    cwds.insert(id, cwd.clone());
                }
                if let Some(history) = history {
                    histories.insert(id, *history);
                }
                if let Some(name) = name {
                    names.insert(id, name.clone());
                }
                if let Some(commands) = commands {
                    startup.insert(id, commands.clone());
                }
                Node::Leaf(id)
            }
            Layout::Split { axis, ratio, a, b } => Node::Split {
                axis: *axis,
                ratio: ratio.clamp(0.1, 0.9),
                a: Box::new(self.build(a, cwds, histories, names, startup)),
                b: Box::new(self.build(b, cwds, histories, names, startup)),
            },
        }
    }

    /// Deletes the command histories of terminals that were closed for good: neither open, nor in a
    /// profile or the recently closed tabs.
    fn forget_unused_histories(&self) {
        let all_tabs = self.tabs.iter().chain(self.others.iter().chain(&self.new_windows).flat_map(|w| &w.tabs));
        let mut keep: Vec<Uuid> = all_tabs.flat_map(|t| t.histories.values().copied()).collect();
        for p in &self.config.profiles {
            p.tab.layout.histories(&mut keep);
        }
        for c in &self.closed {
            c.tab.layout.histories(&mut keep);
        }
        for p in &self.closed_panes {
            p.histories(&mut keep);
        }
        crate::shell::forget_others(&keep.into_iter().collect());
    }

    /// What a new pane of this tab runs: an ssh session for SSH tabs, the user's shell otherwise.
    fn launch_for(&self, index: usize) -> Option<crate::ssh::Launch> {
        let id = self.tabs.get(index)?.ssh?;
        Some(self.config.ssh.iter().find(|h| h.id == id)?.session_command())
    }

    /// Starts the shells of a tab's panes that have not run yet.
    fn start_pending(&mut self, ctx: &egui::Context, index: usize) {
        let launch = self.launch_for(index);
        let restore = self.config.settings.restore_scrollback;
        let restored = self.t().scrollback_restored;
        let Some(tab) = self.tabs.get_mut(index) else { return };
        for id in std::mem::take(&mut tab.pending) {
            let history = tab.history_path(id);
            // What the pane showed last time, then a line saying so.
            let restore = restore.then(|| tab.histories.get(&id).and_then(|h| crate::shell::scrollback_path(*h)).and_then(|p| std::fs::read(p).ok())).flatten().filter(|b| crate::terminal::has_text(b)).map(|mut bytes| {
                bytes.extend_from_slice(format!("\x1b[0;2m── {} ──\x1b[0m\r\n", restored).as_bytes());
                bytes
            });
            match Terminal::local(ctx, tab.cwds.get(&id).map(PathBuf::as_path), launch.as_ref(), history.as_deref(), restore.as_deref()) {
                Ok(term) => {
                    tab.panes.insert(id, term);
                    if tab.startup.contains_key(&id) {
                        tab.startup_due.insert(id, Instant::now());
                    }
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        self.allow_ssh_panes(index);
    }

    /// Lets the ssh processes of tab `index` get their host's saved password from the askpass helper.
    fn allow_ssh_panes(&self, index: usize) {
        if let Some((tab, host)) = self.tabs.get(index).and_then(|t| Some((t, self.config.ssh.iter().find(|h| Some(h.id) == t.ssh)?))) {
            for term in tab.panes.values() {
                if let Some(pid) = term.pid() {
                    self.askpass.allow(pid, host);
                }
            }
        }
    }

    /// Restarts panes of a tab (a new ssh session replaces the old or ended one).
    fn reconnect(&mut self, ctx: &egui::Context, index: usize, panes: &[PaneId]) {
        let launch = self.launch_for(index);
        let Some(tab) = self.tabs.get_mut(index) else { return };
        for &id in panes {
            let history = tab.history_path(id);
            match Terminal::local(ctx, None, launch.as_ref(), history.as_deref(), None) {
                Ok(term) => {
                    tab.panes.insert(id, term);
                    tab.dead.remove(&id);
                    tab.pending.retain(|p| *p != id);
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        self.allow_ssh_panes(index);
        self.focus_terminal = true;
    }

    /// Starts pane `id` again (a fresh shell, where it was) and types its startup commands once ready.
    fn relaunch(&mut self, ctx: &egui::Context, index: usize, id: PaneId) {
        let launch = self.launch_for(index);
        let Some(tab) = self.tabs.get_mut(index) else { return };
        let cwd = tab.panes.get(&id).and_then(Terminal::cwd).or_else(|| tab.cwds.get(&id).cloned());
        let history = tab.history_path(id);
        match Terminal::local(ctx, cwd.as_deref(), launch.as_ref(), history.as_deref(), None) {
            Ok(term) => {
                tab.panes.insert(id, term);
                tab.dead.remove(&id);
                tab.pending.retain(|p| *p != id);
                tab.git.remove(&id);
                tab.startup_due.insert(id, Instant::now());
                tab.focused = id;
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        self.allow_ssh_panes(index);
        self.focus_terminal = true;
    }

    /// Connects to an SSH host, or switches to its tab if it is already open.
    /// Opens (or shows) the tab of database connection `id`.
    fn open_db(&mut self, id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.db == Some(id)) {
            if let Some(tab) = self.tabs.get_mut(index) {
                tab.show_db = true;
            }
            self.select(index);
            return;
        }
        if self.show_elsewhere(|t| t.db == Some(id)) {
            return;
        }
        let Some(conn) = self.config.databases.iter().find(|c| c.id == id) else { return };
        let (name, color) = (conn.name.clone(), conn.color);
        let pane = self.next_pane;
        self.next_pane += 1;
        // A terminal behind the view (⌘ E), started only when shown.
        let mut tab = Tab::new(Node::Leaf(pane), HashMap::new());
        tab.pending = vec![pane];
        tab.db = Some(id);
        tab.show_db = true;
        tab.name = Some(name);
        tab.color = color;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.home = None;
    }

    /// The active tab's database view, filling `rect` (connecting it the first time).
    fn db_view_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let index = self.active;
        let Some(id) = self.tabs.get(index).and_then(|t| t.db) else { return };
        let Some(conn) = self.config.databases.iter().find(|c| c.id == id).cloned() else { return };
        let host = conn.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)).cloned();
        let Some(tab) = self.tabs.get_mut(index) else { return };
        let view = tab.db_view.get_or_insert_with(|| {
            let password = conn.password_saved.then(|| ssh::load_password(conn.id)).flatten();
            let tunnel = host.as_ref().map(|h| (h.tunnel_command(), h.name.clone()));
            Box::new(dbview::DbView::new(ui.ctx(), &conn, password, tunnel))
        });
        view.confirm_changes = self.config.settings.db_confirm_changes;
        let dbview::DbAction::None = view.ui(ui, rect, &self.theme, t);
        // A new SSH forward (opened, or opened again): its prompts are answered in the window.
        if let (Some(pid), Some(h)) = (view.take_tunnel_pid(), &host) {
            self.askpass.allow_interactive(pid, h);
        }
    }

    fn open_ssh(&mut self, id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.ssh == Some(id)) {
            self.select(index);
            return;
        }
        if self.show_elsewhere(|t| t.ssh == Some(id)) {
            return;
        }
        let Some(host) = self.config.ssh.iter().find(|h| h.id == id) else { return };
        let (name, color) = (host.name.clone(), host.color);
        let pane = self.next_pane;
        self.next_pane += 1;
        let mut tab = Tab::new(Node::Leaf(pane), HashMap::new());
        tab.pending = vec![pane];
        tab.ssh = Some(id);
        tab.name = Some(name);
        tab.color = color;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.home = None;
        self.focus_terminal = true;
    }

    fn save_as_profile(&mut self, index: usize) {
        let Some(tab) = self.tabs.get_mut(index) else { return };
        if tab.name.is_none() {
            tab.name = Some(tab.title().to_owned());
        }
        let id = Uuid::new_v4();
        tab.profile = Some(id);
        let state = tab.state();
        self.config.profiles.push(Profile { id, tab: state, commands: Vec::new(), extra: Default::default() });
    }

    /// Opens a profile, or switches to its tab if it is already open (a profile is open at most once).
    fn open_profile(&mut self, id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.profile == Some(id)) {
            self.select(index);
        } else if self.show_elsewhere(|t| t.profile == Some(id)) {
        } else if let Some(p) = self.config.profiles.iter().find(|p| p.id == id) {
            let state = p.tab.clone();
            self.open_tab(&state, Some(id));
        }
    }

    /// A tab open in another window: brings that window to the front, on the tab.
    fn show_elsewhere(&mut self, is: impl Fn(&Tab) -> bool) -> bool {
        for w in &mut self.others {
            if let Some(i) = w.tabs.iter().position(&is) {
                w.active = i;
                w.focus_terminal = true;
                self.ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::Focus);
                return true;
            }
        }
        false
    }

    /// A new window, a little offset from this one, where `fill` opens its tabs.
    fn new_window(&mut self, fill: impl FnOnce(&mut Self)) {
        let opened_at = self.window.map(|w| WindowState { x: w.x + 36.0, y: w.y + 36.0, maximized: false, fullscreen: false, ..w });
        let mut slot = WindowSlot { viewport: egui::ViewportId::from_hash_of(("window", Uuid::new_v4())), opened_at, window: opened_at, focus_terminal: true, ..Default::default() };
        self.swap_window(&mut slot);
        fill(self);
        self.focus_terminal = true;
        self.swap_window(&mut slot);
        self.new_windows.push(slot);
    }

    /// Takes tab `index` out of this window (to move it to another), keeping its programs running.
    fn take_tab(&mut self, index: usize) -> Option<Tab> {
        if index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        // Popups tied to this window's tabs by index.
        self.rename = None;
        self.commands_menu = None;
        self.history_search = None;
        self.confirm_close = None;
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.focus_terminal = true;
        Some(tab)
    }

    /// Moves tab `index` to a new window.
    fn tab_to_new_window(&mut self, index: usize) {
        let Some(tab) = self.take_tab(index) else { return };
        self.new_window(|app| {
            app.tabs.push(tab);
            app.active = 0;
        });
        // The main window keeps at least one tab.
        if self.tabs.is_empty() && self.viewport == egui::ViewportId::ROOT {
            self.new_tab(&self.ctx.clone());
        }
    }

    /// Opens a profile or an SSH host in a new window; already open here, its tab moves there.
    fn item_to_new_window(&mut self, id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.profile == Some(id) || t.ssh == Some(id)) {
            self.tab_to_new_window(index);
            return;
        }
        if self.show_elsewhere(|t| t.profile == Some(id) || t.ssh == Some(id)) {
            return;
        }
        let profile = self.config.profiles.iter().any(|p| p.id == id);
        self.new_window(|app| if profile { app.open_profile(id) } else { app.open_ssh(id) });
    }

    /// Opens the tabs of a saved window (session) in this one.
    fn restore_tabs(&mut self, tabs: &[SessionTab], active: usize) {
        for tab in tabs {
            // A profile tab starts from its profile: it may have been edited in config.json since.
            let profile = tab.profile.and_then(|id| self.config.profiles.iter().find(|p| p.id == id));
            let state = profile.map_or_else(|| tab.tab.clone(), |p| p.tab.clone());
            self.open_tab(&state, tab.profile);
            // SSH tabs reconnect when first shown, like other restored tabs start their shell.
            if let (Some(id), Some(last)) = (tab.ssh, self.tabs.last_mut()) {
                last.ssh = self.config.ssh.iter().any(|h| h.id == id).then_some(id);
            }
            // Database tabs connect when first shown.
            if let (Some(id), Some(last)) = (tab.db, self.tabs.last_mut()) {
                last.db = self.config.databases.iter().any(|c| c.id == id).then_some(id);
                last.show_db = true;
            }
        }
        self.active = active.min(self.tabs.len().saturating_sub(1));
    }

    fn reopen_closed(&mut self, index: usize) {
        if index >= self.closed.len() {
            return;
        }
        let tab = self.closed.remove(index);
        match tab.profile.filter(|id| self.config.profiles.iter().any(|p| p.id == *id)) {
            // Its profile holds the latest state, and may already be open.
            Some(id) => self.open_profile(id),
            None => self.open_tab(&tab.tab, None),
        }
    }

    /// Remembers the window geometry; the normal size is kept while maximized or fullscreen.
    fn track_window(&mut self, ctx: &egui::Context) {
        let (outer, inner, maximized, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.maximized.unwrap_or(false), v.fullscreen.unwrap_or(false))
        });
        let (Some(outer), Some(inner)) = (outer, inner) else { return };
        let mut w = self.window.unwrap_or(WindowState { x: 0.0, y: 0.0, width: 0.0, height: 0.0, maximized, fullscreen });
        if !maximized && !fullscreen {
            (w.x, w.y, w.width, w.height) = (outer.min.x, outer.min.y, inner.width(), inner.height());
        }
        w.maximized = maximized;
        w.fullscreen = fullscreen;
        self.window = Some(w);
    }

    /// Saves what the terminals of tab `index` (all tabs if None) show, those whose screen changed since
    /// (or all, with `force`).
    fn save_scrollbacks(&mut self, index: Option<usize>, force: bool) {
        if !self.config.settings.restore_scrollback || self.read_only {
            return;
        }
        // This window's tabs (only tab `index` if given), and the other windows' unless `index` is given.
        let others = self.others.iter().flat_map(|w| &w.tabs).filter(|_| index.is_none());
        let tabs = self.tabs.iter().enumerate().filter(|(i, _)| index.is_none_or(|x| x == *i)).map(|(_, t)| t).chain(others);
        for tab in tabs {
            for (pane, term) in &tab.panes {
                let Some(&id) = tab.histories.get(pane) else { continue };
                let seq = term.output_seq();
                if !force && self.scrollback_seq.get(&id) == Some(&seq) {
                    continue;
                }
                // A full-screen program's screen is not worth keeping: the last save stays.
                if let Some(bytes) = term.dump(SCROLLBACK_SAVED) {
                    self.scrollback_seq.insert(id, seq);
                    crate::shell::save_scrollback(id, &bytes);
                }
            }
        }
    }

    /// Pushes open tabs into their profiles and writes what changed to disk.
    fn sync(&mut self) {
        // Every window, the main one (ROOT) apart; the window being drawn is in `self`.
        let mut main = None;
        let mut windows = Vec::new();
        let here = SessionWindow { tabs: sync_tabs(&mut self.tabs, &mut self.config), active: self.active, window: self.window };
        if self.viewport == egui::ViewportId::ROOT { main = Some(here) } else { windows.push(here) }
        for slot in &mut self.others {
            let w = SessionWindow { tabs: sync_tabs(&mut slot.tabs, &mut self.config), active: slot.active, window: slot.window };
            if slot.viewport == egui::ViewportId::ROOT { main = Some(w) } else { windows.push(w) }
        }
        let main = main.unwrap_or_default();
        let session = Session { tabs: main.tabs, active: main.active, closed: self.closed.clone(), closed_panes: self.closed_panes.clone(), window: main.window, windows };

        if self.read_only {
            return;
        }
        if session != self.saved_session && !self.session_frozen {
            if let Some(path) = config::session_path() {
                match config::save(&path, &session) {
                    Ok(()) => self.saved_session = session,
                    Err(e) => self.error = Some(format!("{} : {e:#}", self.t().session_save_failed)),
                }
            }
        }
        self.reload_config_if_edited();
        self.save_config();
    }

    /// Moves the state of window `slot` into `self` and this one's into `slot` (see `WindowSlot`).
    fn swap_window(&mut self, slot: &mut WindowSlot) {
        use std::mem::swap;
        swap(&mut self.viewport, &mut slot.viewport);
        swap(&mut self.tabs, &mut slot.tabs);
        swap(&mut self.active, &mut slot.active);
        swap(&mut self.rename, &mut slot.rename);
        swap(&mut self.focus_terminal, &mut slot.focus_terminal);
        swap(&mut self.tab_grab, &mut slot.tab_grab);
        swap(&mut self.window_title, &mut slot.window_title);
        swap(&mut self.window, &mut slot.window);
        swap(&mut self.opened_at, &mut slot.opened_at);
        swap(&mut self.live, &mut slot.live);
        swap(&mut self.commands_menu, &mut slot.commands_menu);
        swap(&mut self.history_search, &mut slot.history_search);
        swap(&mut self.paste_confirm, &mut slot.paste_confirm);
        swap(&mut self.confirm_close, &mut slot.confirm_close);
        swap(&mut self.window_closing, &mut slot.window_closing);
        swap(&mut self.guard_confirm, &mut slot.guard_confirm);
        swap(&mut self.pane_drag, &mut slot.pane_drag);
        swap(&mut self.pane_rename, &mut slot.pane_rename);
        swap(&mut self.toasts, &mut slot.toasts);
        swap(&mut self.toast_rects, &mut slot.toast_rects);
        swap(&mut self.home, &mut slot.home);
        swap(&mut self.home_game, &mut slot.home_game);
        swap(&mut self.game, &mut slot.game);
        swap(&mut self.ronnie_show, &mut slot.ronnie_show);
        swap(&mut self.game_folded, &mut slot.game_folded);
    }

    fn save_config(&mut self) {
        // The sidebar folded for a round of the game is saved as it was before.
        let mut config = self.config.clone();
        if self.game_folded {
            config.settings.sidebar_folded = false;
        }
        if !self.config_writable || config == self.saved_config {
            return;
        }
        let Some(path) = config::config_path() else { return };
        match config::save_config_file(&path, &config) {
            Ok(()) => {
                self.saved_config = config;
                self.config_mtime = config::modified(&path);
            }
            Err(e) => {
                self.error = Some(format!("{} : {e:#}", self.t().config_save_failed));
                // Not again every second: the next outside edit of the file retries.
                self.config_writable = false;
            }
        }
    }

    /// Picks up changes made to config.json in another editor.
    fn reload_config_if_edited(&mut self) {
        let Some(path) = config::config_path() else { return };
        let mtime = config::modified(&path);
        if mtime.is_none() || mtime == self.config_mtime {
            return;
        }
        self.config_mtime = mtime;
        let result = std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|text| Ok(Config::from_json(&text)?));
        match result {
            Ok(mut config) => {
                config.normalize();
                self.config_writable = !self.read_only;
                self.saved_config = config.clone();
                self.apply_config(config);
            }
            Err(e) => {
                // Keep running with the current config, but don't overwrite the file being edited.
                self.config_writable = false;
                self.error = Some(format!("{} : {e:#}", self.t().config_not_loaded));
            }
        }
    }

    /// Switches to a new config (from the editor or the file), keeping open tabs consistent with it.
    fn apply_config(&mut self, config: Config) {
        if config.settings.theme != self.config.settings.theme {
            self.theme = Preset::find(&config.settings.theme).theme();
            self.ctx.set_visuals(self.theme.visuals());
        }
        self.fonts.size = config.settings.font_size.clamp(*config::FONT_SIZES.start(), *config::FONT_SIZES.end());
        if config.settings.ui_zoom != self.config.settings.ui_zoom {
            self.ctx.set_zoom_factor(config.settings.ui_zoom.clamp(*config::UI_ZOOMS.start(), *config::UI_ZOOMS.end()));
        }
        if config.settings.scrollback != self.config.settings.scrollback {
            let lines = config.settings.scrollback.clamp(*config::SCROLLBACK_LINES.start(), *config::SCROLLBACK_LINES.end());
            crate::terminal::set_default_scrollback(lines);
            for term in self.tabs.iter().flat_map(|t| t.panes.values()) {
                term.set_scrollback(lines);
            }
        }
        let exists = |id: Uuid| config.profiles.iter().any(|p| p.id == id);
        for tab in &mut self.tabs {
            if let Some(p) = tab.profile.and_then(|id| config.profiles.iter().find(|p| p.id == id)) {
                // Edited name and color show up on the open tab right away.
                tab.name = p.tab.name.clone();
                tab.color = p.tab.color;
            } else {
                tab.profile = None;
            }
        }
        self.closed.retain(|c| c.profile.is_some_and(exists));
        self.config = config;
        self.error = None;
    }

    /// Long commands that ended out of sight: a ✓ or ✗ on their tab when it isn't shown, and a system
    /// notification when Ronnie isn't the window in front.
    fn finished_commands(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let settings = &self.config.settings;
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let mut notified = false;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let shown = i == self.active && self.home.is_none() && !tab.show_files;
            // Taken even when disabled, not to report old commands once enabled.
            let finished: Vec<Finished> = tab.panes.values_mut().flat_map(|term| term.take_finished(ctx)).collect();
            let long = finished.into_iter().filter(|f| settings.notify_commands && f.duration.as_secs() >= settings.notify_after).last();
            let Some(done) = long.filter(|_| !(shown && focused)) else { continue };
            let ok = done.code.is_none_or(|c| c == 0);
            let command = done.command.as_deref().unwrap_or(t.command_generic);
            let duration = format_duration(done.duration);
            if !shown {
                tab.done = Some(Done { ok, summary: format!("{} {command}  ·  {duration}", if ok { "✓" } else { "✗" }) });
            }
            let title = match done.code.filter(|c| *c != 0) {
                Some(code) => t.command_failed.replace("{code}", &code.to_string()),
                None => t.command_done.to_owned(),
            };
            // Ronnie in front, another tab shown: a notice in the window (which leads to the tab), the
            // system's, or both, as chosen.
            let body = format!("{command}\n{}  ·  {duration}", tab.title());
            if focused && !shown && settings.notify_style.in_app() {
                self.toasts.push(Toast { ok, title: title.clone(), body: body.clone(), tab: i, at: std::time::Instant::now() });
            }
            if focused && !shown && settings.notify_style.system() {
                crate::notify::send(&title, &body);
            }
            if !focused {
                let title = match done.code.filter(|c| *c != 0) {
                    Some(code) => t.command_failed.replace("{code}", &code.to_string()),
                    None => t.command_done.to_owned(),
                };
                crate::notify::send(&title, &format!("{command}\n{}  ·  {duration}", tab.title()));
                notified = true;
            }
        }
        if notified {
            ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        if let Some(tab) = self.tabs.get_mut(self.active).filter(|t| !t.show_files) {
            tab.done = None;
        }
    }

    /// A sample notice, as the settings' "Test" button shows it (in the window, from the system, or both).
    fn test_notification(&mut self) {
        let t = self.t();
        let body = format!("{}\n{}  ·  12 s", t.notify_test_command, self.tabs.get(self.active).map_or("Ronnie", |t| t.title()));
        let style = self.config.settings.notify_style;
        if style.in_app() {
            self.toasts.push(Toast { ok: true, title: t.command_done.to_owned(), body: body.clone(), tab: self.active, at: std::time::Instant::now() });
        }
        if style.system() {
            crate::notify::send(t.command_done, &body);
        }
    }

    /// The notices in the bottom right corner: a few seconds each (longer under the pointer); a click
    /// goes to their tab.
    fn toasts_ui(&mut self, ctx: &egui::Context) {
        const LIFE: f32 = 7.0;
        self.toasts.retain(|t| t.at.elapsed().as_secs_f32() < LIFE);
        if self.toasts.is_empty() {
            return;
        }
        // The newest four.
        let extra = self.toasts.len().saturating_sub(4);
        self.toasts.drain(..extra);
        let screen = ctx.content_rect();
        let place = self.config.settings.toast_position;
        // On the left: next to the sidebar, not over it.
        let sidebar_w = if self.config.settings.sidebar_folded { RAIL_WIDTH } else { SIDEBAR_WIDTH };
        let (left, right) = (screen.min.x + sidebar_w + 16.0, screen.max.x - 16.0);
        let mut edge = if place.top() { screen.min.y + 16.0 } else { screen.max.y - 16.0 };
        let mut go = None;
        let mut hovered_any = false;
        let t_go_to_tab = self.t().go_to_tab;
        let mut rects = vec![Rect::NOTHING; self.toasts.len()];
        for (k, toast) in self.toasts.iter().enumerate().rev() {
            let width = 320.0;
            let x = match place.side() {
                -1 => left,
                0 => (left + right) / 2.0,
                _ => right,
            };
            let align = match (place.top(), place.side()) {
                (true, -1) => Align2::LEFT_TOP,
                (true, 0) => Align2::CENTER_TOP,
                (true, _) => Align2::RIGHT_TOP,
                (false, -1) => Align2::LEFT_BOTTOM,
                (false, 0) => Align2::CENTER_BOTTOM,
                (false, _) => Align2::RIGHT_BOTTOM,
            };
            let age = toast.at.elapsed().as_secs_f32();
            // Fades in, then out at the end.
            let alpha = (age / 0.25).min(1.0).min(((LIFE - age) / 0.6).clamp(0.0, 1.0));
            // In the theme's colors: its accent, its red for a failure.
            let accent = self.theme.accent;
            let mark = if toast.ok { accent } else { self.theme.ansi[1] };
            let hovered = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| self.toast_rects.get(k).is_some_and(|r| r.contains(p)));
            let area = egui::Area::new(egui::Id::new(("toast", k))).order(egui::Order::Foreground).pivot(align).fixed_pos(Pos2::new(x, edge)).show(ctx, |ui| {
                ui.set_opacity(alpha);
                let frame = Frame::popup(ui.style()).fill(self.theme.chrome_bg).stroke(Stroke::new(1.0, accent.gamma_multiply(if hovered { 0.9 } else { 0.45 }))).corner_radius(12.0).inner_margin(egui::Margin { left: 18, right: 14, top: 12, bottom: 14 });
                let shown = frame.show(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 11.0, mark.gamma_multiply(0.18));
                        ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(1.0, mark.gamma_multiply(0.5)));
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, if toast.ok { "✓" } else { "✗" }, FontId::proportional(12.5), mark);
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(&toast.title).size(13.5).strong().color(self.theme.text));
                    });
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(egui::RichText::new(&toast.body).size(12.5).color(self.theme.text_muted)).wrap());
                    if hovered {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(format!("{}  →", t_go_to_tab)).size(11.5).color(accent));
                    }
                });
                // A strip of the accent on the left, and the time left at the bottom.
                let r = shown.response.rect;
                ui.painter().rect_filled(Rect::from_min_size(r.min + Vec2::new(6.0, 12.0), Vec2::new(3.0, r.height() - 24.0)), 1.5, mark.gamma_multiply(0.9));
                let left_time = (1.0 - age / LIFE).clamp(0.0, 1.0);
                let bar = Rect::from_min_size(Pos2::new(r.min.x + 14.0, r.max.y - 5.0), Vec2::new((r.width() - 28.0) * left_time, 2.0));
                ui.painter().rect_filled(bar, 1.0, accent.gamma_multiply(0.6));
            });
            rects[k] = area.response.rect;
            let resp = area.response.interact(Sense::click());
            hovered_any |= resp.hovered();
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                go = Some((k, toast.tab));
            }
            // The next one stacks away from the edge.
            let step = area.response.rect.height() + 10.0;
            edge += if place.top() { step } else { -step };
        }
        // Kept while the pointer is over one.
        if hovered_any {
            for t in &mut self.toasts {
                t.at = t.at.max(std::time::Instant::now() - std::time::Duration::from_secs_f32(1.0));
            }
        }
        self.toast_rects = rects;
        if let Some((k, tab)) = go {
            self.toasts.remove(k);
            self.select(tab);
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn spawn(&mut self, ctx: &egui::Context, cwd: Option<&Path>, launch: Option<&crate::ssh::Launch>, history: Uuid) -> Option<(PaneId, Terminal)> {
        match Terminal::local(ctx, cwd, launch, crate::shell::history_path(history).as_deref(), None) {
            Ok(term) => {
                let id = self.next_pane;
                self.next_pane += 1;
                Some((id, term))
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        let history = Uuid::new_v4();
        // Starts in the directory of the terminal in front, when it is a local one.
        let cwd = self.tabs.get(self.active).filter(|t| t.ssh.is_none()).and_then(|t| t.panes.get(&t.focused)).and_then(Terminal::cwd);
        if let Some((id, term)) = self.spawn(ctx, cwd.as_deref(), None, history) {
            let mut tab = Tab::new(Node::Leaf(id), HashMap::from([(id, term)]));
            tab.histories.insert(id, history);
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
            self.home = None;
            self.focus_terminal = true;
        }
    }

    fn split(&mut self, ctx: &egui::Context, index: usize, side: Direction) {
        if index >= self.tabs.len() {
            return;
        }
        // The new pane starts in the same directory as the one being split (or another ssh session to the same host).
        let tab = &self.tabs[index];
        let cwd = tab.panes.get(&tab.focused).and_then(Terminal::cwd);
        // SSH: in the same folder on the server, when the shell there tells which.
        let remote_dir = tab.panes.get(&tab.focused).and_then(Terminal::reported_cwd);
        let launch = match (tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)), remote_dir) {
            (Some(host), Some(dir)) => Some(host.command_in(Some(&dir))),
            _ => self.launch_for(index),
        };
        let history = Uuid::new_v4();
        if let Some((id, term)) = self.spawn(ctx, cwd.as_deref(), launch.as_ref(), history) {
            let tab = &mut self.tabs[index];
            tab.layout.split(tab.focused, id, side);
            tab.panes.insert(id, term);
            tab.histories.insert(id, history);
            tab.focused = id;
            self.focus_terminal = true;
            self.allow_ssh_panes(index);
        }
    }

    fn close_pane(&mut self, index: usize, pane: PaneId) {
        // Named: kept to be reopened, with what it showed (the last pane: see `close_tab`).
        if self.tabs.get(index).is_some_and(|t| t.layout.leaves().len() > 1) {
            self.save_scrollbacks(Some(index), false);
            if let Some(state) = self.tabs.get_mut(index).and_then(|t| t.closed_pane(pane)) {
                self.remember_closed_pane(state);
            }
        }
        let Some(tab) = self.tabs.get_mut(index) else { return };
        if !tab.remove_pane(pane) {
            self.close_tab(index);
        } else if index == self.active {
            self.focus_terminal = true;
        }
    }

    /// Keeps a closed pane to be reopened (one of that name replaces an older one).
    fn remember_closed_pane(&mut self, state: Layout) {
        let name = |l: &Layout| match l {
            Layout::Pane { name, .. } => name.clone(),
            Layout::Split { .. } => None,
        };
        let new = name(&state);
        self.closed_panes.retain(|p| new.is_none() || name(p) != new);
        self.closed_panes.push(state);
        if self.closed_panes.len() > config::MAX_CLOSED_PANES {
            self.closed_panes.remove(0);
        }
    }

    /// Opens closed pane `k` again, right of pane `at` of tab `index`.
    fn reopen_pane(&mut self, ctx: &egui::Context, index: usize, at: PaneId, k: usize) {
        if k >= self.closed_panes.len() || index >= self.tabs.len() {
            return;
        }
        let state = self.closed_panes.remove(k);
        let (mut cwds, mut histories, mut names, mut startup) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let Node::Leaf(id) = self.build(&state, &mut cwds, &mut histories, &mut names, &mut startup) else { return };
        let tab = &mut self.tabs[index];
        tab.layout.split(at, id, Direction::Right);
        tab.cwds.extend(cwds);
        tab.histories.extend(histories);
        tab.names.extend(names);
        tab.startup.extend(startup);
        tab.pending.push(id);
        tab.focused = id;
        self.start_pending(ctx, index);
        self.focus_terminal = true;
    }

    fn focus_neighbor(&mut self, dir: Direction) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if let Some(id) = pane::neighbor(&tab.rects, tab.focused, dir) {
            tab.focused = id;
            self.focus_terminal = true;
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // Its profile shows it again when reopened.
        self.save_scrollbacks(Some(index), false);
        let shown = self.shown_tab() == Some(index);
        let mut tab = self.tabs.remove(index);
        let state = tab.state();
        if let Some(p) = self.config.profiles.iter_mut().find(|p| Some(p.id) == tab.profile) {
            p.tab = state.clone();
        }
        // Only profiles are worth reopening; a plain terminal is just a new tab (its named panes can be
        // reopened one by one).
        if tab.profile.is_none() {
            for id in tab.layout.leaves() {
                if let Some(state) = tab.closed_pane(id) {
                    self.remember_closed_pane(state);
                }
            }
        }
        if tab.profile.is_some() {
            self.closed.retain(|c| c.profile != tab.profile);
            self.closed.push(SessionTab { profile: tab.profile, ssh: None, db: None, tab: state });
            if self.closed.len() > config::MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        // Whatever points at a tab by its index follows the shift, or is dropped with the closed tab:
        // otherwise a pending rename, popup or "close anyway?" would act on its neighbour.
        let shift = |i: &mut usize| -> bool {
            match (*i).cmp(&index) {
                std::cmp::Ordering::Equal => false,
                std::cmp::Ordering::Greater => {
                    *i -= 1;
                    true
                }
                std::cmp::Ordering::Less => true,
            }
        };
        if self.rename.as_mut().is_some_and(|r| !shift(&mut r.tab)) {
            self.rename = None;
        }
        if self.history_search.as_mut().is_some_and(|s| !shift(&mut s.tab)) {
            self.history_search = None;
        }
        if self.commands_menu.as_mut().is_some_and(|m| !shift(&mut m.tab)) {
            self.commands_menu = None;
        }
        let pending = self.confirm_close.as_mut().map(|c| match &mut c.request {
            CloseRequest::Pane(i, _) | CloseRequest::Tab(i) => shift(i),
            CloseRequest::Window | CloseRequest::Restart => true,
        });
        if pending == Some(false) {
            self.confirm_close = None;
        }
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        // Not a neighbour picked at random: the home page, saying what closed. A tab closed in the
        // background leaves the one shown in place.
        if shown || (self.home.is_none() && self.tabs.is_empty()) {
            self.go_home(Some(tab.title()));
            self.focus_terminal = true;
        }
    }

    fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
            self.home = None;
            self.focus_terminal = true;
        }
    }

    /// The tab shown: none on the home page.
    fn shown_tab(&self) -> Option<usize> {
        (self.home.is_none() && self.active < self.tabs.len()).then_some(self.active)
    }

    /// The home page, with a line about what just closed (none: empty).
    fn go_home(&mut self, closed: Option<&str>) {
        let t = self.t();
        let line = closed.filter(|n| !n.is_empty()).map(|name| {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            t.home_quips[nanos as usize % t.home_quips.len()].replace("{name}", name)
        });
        self.home = Some(line.unwrap_or_default());
        self.home_game = false;
        // The keys go to the game, not to a terminal no longer shown.
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    fn handle_shortcuts(&mut self, ui: &Ui) {
        // A shortcut is being recorded in the settings: keys are for it.
        if self.shortcut_capture.is_some() {
            return;
        }
        // The configurable shortcuts (Settings > Shortcuts), the most specific first: consume_shortcut
        // ignores extra Shift / Alt, so Cmd+T would otherwise also take Cmd+Shift+T.
        let shortcuts = self.config.settings.shortcuts.clone();
        let mut bound: Vec<(KeyboardShortcut, ShortcutAction)> = ShortcutAction::ALL.iter().filter_map(|a| Some((a.get(&shortcuts).parse()?, *a))).collect();
        let weight = |k: &KeyboardShortcut| [k.modifiers.shift, k.modifiers.alt, k.modifiers.ctrl, k.modifiers.mac_cmd].iter().filter(|m| **m).count();
        bound.sort_by_key(|(k, _)| std::cmp::Reverse(weight(k)));
        let mut fired = Vec::new();
        for (shortcut, action) in bound {
            if ui.input_mut(|i| i.consume_shortcut(&shortcut)) {
                fired.push(action);
            }
        }
        // The usual Cmd+, opens the settings too.
        if ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))) {
            fired.push(ShortcutAction::OpenSettings);
        }
        let focused = self.tabs.get(self.active).map(|t| t.focused);
        // The file manager hides the terminals: their actions would act on panes the user can't see.
        let files_shown = self.tabs.get(self.active).is_some_and(|t| t.show_files || (t.db.is_some() && t.show_db));
        let pane_action = |a: ShortcutAction| matches!(a, ShortcutAction::ClosePane | ShortcutAction::SplitRight | ShortcutAction::SplitDown | ShortcutAction::FindText | ShortcutAction::FindCommands | ShortcutAction::ClearPane);
        // A file open in the file manager's editor: ⌘ F searches it, ⌘ W closes it.
        // The home page: the tab behind it isn't shown, nothing may act on it.
        if self.home.is_some() {
            fired.retain(|a| matches!(a, ShortcutAction::NewTab | ShortcutAction::ReopenTab | ShortcutAction::OpenSettings | ShortcutAction::NewWindow | ShortcutAction::ToggleSidebar));
        }
        if let Some(viewer) = self.tabs.get_mut(self.active).filter(|t| t.show_files).and_then(|t| t.files.as_mut()).and_then(|f| f.viewer.as_mut()) {
            fired.retain(|a| match a {
                ShortcutAction::FindText => {
                    viewer.open_find();
                    false
                }
                ShortcutAction::ClosePane => {
                    viewer.request_close();
                    false
                }
                _ => true,
            });
        }
        if let Some(editor) = self.tabs.get_mut(self.active).filter(|t| t.show_files).and_then(|t| t.files.as_mut()).and_then(|f| f.editor.as_mut()) {
            fired.retain(|a| match a {
                ShortcutAction::FindText => {
                    editor.open_find(ui.ctx());
                    false
                }
                ShortcutAction::ClosePane => {
                    editor.request_close();
                    false
                }
                _ => true,
            });
        }
        fired.retain(|a| !(files_shown && pane_action(*a)));
        for action in fired {
            match action {
                ShortcutAction::NewTab => self.new_tab(ui.ctx()),
                ShortcutAction::ClosePane => {
                    if let Some(pane) = focused {
                        self.request_close(CloseRequest::Pane(self.active, pane));
                    }
                }
                ShortcutAction::SplitRight => self.split(ui.ctx(), self.active, Direction::Right),
                ShortcutAction::SplitDown => self.split(ui.ctx(), self.active, Direction::Down),
                ShortcutAction::FindText | ShortcutAction::FindCommands => {
                    if let Some(pane) = focused {
                        self.open_search(self.active, pane, action == ShortcutAction::FindText);
                    }
                }
                ShortcutAction::ReopenTab => {
                    if let Some(last) = self.closed.len().checked_sub(1) {
                        self.reopen_closed(last);
                    }
                }
                ShortcutAction::ClearPane => {
                    if let Some(term) = self.tabs.get_mut(self.active).and_then(|t| t.panes.get_mut(&t.focused)) {
                        term.clear();
                    }
                }
                ShortcutAction::OpenSettings => self.settings_dialog = !self.settings_dialog,
                ShortcutAction::ToggleSidebar => self.config.settings.sidebar_folded = !self.config.settings.sidebar_folded,
                ShortcutAction::NewWindow => {
                    let ctx = ui.ctx().clone();
                    self.new_window(|app| app.new_tab(&ctx));
                }
                ShortcutAction::ToggleFiles => {
                    // A database tab has no terminal to switch to.
                    if let Some(show) = self.tabs.get(self.active).filter(|t| t.db.is_none()).map(|t| !t.show_files) {
                        self.toggle_files(self.active, show);
                    }
                }
            }
        }
        // Move between panes: Cmd+Alt+arrows (Ctrl+Alt+arrows outside macOS).
        let nav = Modifiers::COMMAND | Modifiers::ALT;
        for (key, dir) in [
            (Key::ArrowLeft, Direction::Left),
            (Key::ArrowRight, Direction::Right),
            (Key::ArrowUp, Direction::Up),
            (Key::ArrowDown, Direction::Down),
        ] {
            if ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(nav, key))) {
                self.focus_neighbor(dir);
            }
            // macOS: Cmd+arrow too, but only toward an existing pane; otherwise the terminal gets it
            // (start / end of line).
            let neighbor = self.tabs.get(self.active).and_then(|t| pane::neighbor(&t.rects, t.focused, dir)).is_some();
            if cfg!(target_os = "macos") && neighbor && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::MAC_CMD, key))) {
                self.focus_neighbor(dir);
            }
        }
        if ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::Tab))) && !self.tabs.is_empty() {
            self.select((self.active + 1) % self.tabs.len());
        }
        if ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab)))
            && !self.tabs.is_empty()
        {
            self.select((self.active + self.tabs.len() - 1) % self.tabs.len());
        }
        // Size of the whole interface: Cmd + / Cmd - / Cmd 0 (Ctrl elsewhere), by steps of 10 %.
        let zoom = [(Key::Plus, 0.1), (Key::Equals, 0.1), (Key::Minus, -0.1), (Key::Num0, 0.0)];
        for (key, step) in zoom {
            if ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key))) {
                let zoom = if step == 0.0 { 1.0 } else { ((self.config.settings.ui_zoom + step) * 10.0).round() / 10.0 };
                self.config.settings.ui_zoom = zoom.clamp(*config::UI_ZOOMS.start(), *config::UI_ZOOMS.end());
                ui.ctx().set_zoom_factor(self.config.settings.ui_zoom);
            }
        }
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (i, key) in digits.into_iter().enumerate() {
            if ui.input_mut(|inp| inp.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key))) {
                // Cmd+9 always goes to the last tab, like browsers.
                self.select(if i == 8 { self.tabs.len().saturating_sub(1) } else { i });
            }
        }
    }

}

const THEME_CARD: Vec2 = Vec2::new(204.0, 84.0);
const EDITOR_WIDTH: f32 = 720.0;
const SETTINGS_WIDTH: f32 = 940.0;
/// The settings' side navigation.
const SETTINGS_NAV_WIDTH: f32 = 210.0;
const SETTINGS_HEIGHT: f32 = 680.0;

/// A clickable preview of a theme: its sidebar, a sample terminal line and its palette.
fn theme_card(ui: &mut Ui, preset: &Preset, selected: bool, _highlight: Color32) -> egui::Response {
    let theme = preset.theme();
    let (rect, resp) = ui.allocate_exact_size(THEME_CARD, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, theme.bg);
    // Sidebar strip with an active tab marked by the accent.
    let side = Rect::from_min_max(rect.min, Pos2::new(rect.min.x + 46.0, rect.max.y));
    painter.rect_filled(side, egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, theme.chrome_bg);
    let active = Rect::from_min_size(side.min + Vec2::new(6.0, 30.0), Vec2::new(34.0, 12.0));
    painter.rect_filled(active, 3.0, theme.tab_active);
    painter.rect_filled(Rect::from_min_size(active.min, Vec2::new(2.5, 12.0)), 1.0, theme.accent);
    for k in 0..2 {
        let line = Rect::from_min_size(side.min + Vec2::new(10.0, 50.0 + k as f32 * 12.0), Vec2::new(22.0, 4.0));
        painter.rect_filled(line, 2.0, theme.text_muted.gamma_multiply(0.6));
    }

    let x = side.max.x + 12.0;
    painter.text(Pos2::new(x, rect.min.y + 10.0), Align2::LEFT_TOP, preset.name, FontId::proportional(14.0), theme.fg);
    // A prompt and a colored `ls`, as the terminal would show it.
    let mono = FontId::new(11.0, egui::FontFamily::Name("mono".into()));
    let mut job = egui::text::LayoutJob::default();
    for (text, color) in [("~ ", theme.ansi[4]), ("$ ", theme.ansi[5]), ("ls ", theme.fg), ("src ", theme.ansi[6]), ("main.rs ", theme.ansi[2]), ("err", theme.ansi[1])] {
        job.append(text, 0.0, egui::TextFormat { font_id: mono.clone(), color, ..Default::default() });
    }
    // Clipped: a narrow card cuts the end of the sample.
    painter.with_clip_rect(rect.shrink(6.0)).galley(Pos2::new(x, rect.min.y + 34.0), painter.layout_job(job), theme.fg);
    for (k, color) in theme.ansi[1..7].iter().chain(std::iter::once(&theme.accent)).enumerate() {
        let r = Rect::from_min_size(Pos2::new(x + k as f32 * 20.0, rect.max.y - 20.0), Vec2::new(16.0, 8.0));
        painter.rect_filled(r, 2.0, *color);
    }

    let border = if selected {
        Stroke::new(2.0, theme.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, theme.accent.gamma_multiply(0.6))
    } else {
        Stroke::new(1.0, theme.tab_active)
    };
    painter.rect_stroke(rect, 8.0, border, egui::StrokeKind::Inside);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Shown in an SSH pane until the server answers.
fn paint_connecting(ui: &Ui, pane: Rect, theme: &Theme, t: &Strings, text: &str) {
    // Over what the pane showed last time (restored), so that it reads.
    ui.painter().rect_filled(pane, 0.0, theme.bg.gamma_multiply(0.94));
    loading::screen(ui, pane, theme, text, None, Some(t.connecting_hint));
}

/// Wakes the (otherwise idle) UI when config.json is edited in another program, so that the change is
/// picked up: a stat every two seconds, in the background.
fn watch_config(ctx: &egui::Context) {
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new().name("config-watch".into()).spawn(move || {
        let path = config::config_path();
        let mut seen = path.as_deref().and_then(config::modified);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = path.as_deref().and_then(config::modified);
            if now != seen {
                seen = now;
                ctx.request_repaint();
            }
        }
    });
}

/// "COULEUR" → "Couleur" (by chars: slicing bytes would panic on a first letter such as "É").
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars.as_str().to_lowercase().chars()).collect()).unwrap_or_default()
}

/// A texture from embedded PNG bytes.
fn load_png(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let png = eframe::icon_data::from_png_bytes(bytes).unwrap_or_default();
    let image = egui::ColorImage::from_rgba_unmultiplied([png.width as usize, png.height as usize], &png.rgba);
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

/// Green halo around a tab's dot while a program runs in it. It breathes slowly while the window is
/// focused, and holds still otherwise (animating an unseen badge would keep the app redrawing).
fn paint_live(ui: &Ui, painter: &egui::Painter, dot: Pos2, theme: &Theme) {
    let focused = ui.input(|i| i.focused);
    let phase = if focused { (ui.input(|i| i.time) * std::f64::consts::TAU / 2.0).sin() as f32 * 0.5 + 0.5 } else { 0.6 };
    painter.circle_filled(dot, 7.5, theme.ansi[2].gamma_multiply(0.10 + 0.18 * phase));
    painter.circle_stroke(dot, 6.5, Stroke::new(1.2, theme.ansi[2].gamma_multiply(0.45 + 0.5 * phase)));
    if focused {
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
}

/// A notice in the corner of the window (a long command ended in another tab).
struct Toast {
    ok: bool,
    title: String,
    body: String,
    /// The tab it leads to.
    tab: usize,
    at: std::time::Instant,
}

/// "45 s", "2 min 14 s", "1 h 03 min".
fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {:02} s", s / 60, s % 60),
        _ => format!("{} h {:02} min", s / 3600, s / 60 % 60),
    }
}

/// Text in the logo's metal font: drop shadow, dark outline, `color` fill with a lighter top edge.
/// `alpha` fades it all (0 to 1).
fn paint_metal(painter: &egui::Painter, at: Pos2, align: Align2, text: &str, size: f32, color: Color32, alpha: f32) {
    let font = FontId::new(size, egui::FontFamily::Name("metal".into()));
    let s = size / 32.0;
    let draw = |offset: Vec2, c: Color32| {
        painter.text(at + offset, align, text, font.clone(), c.gamma_multiply(alpha));
    };
    draw(Vec2::new(0.0, 3.0 * s), Color32::from_black_alpha(140));
    for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
        draw(Vec2::new(dx, dy) * 1.2 * s, Color32::from_black_alpha(200));
    }
    draw(Vec2::new(0.0, -0.8 * s), color.lerp_to_gamma(Color32::WHITE, 0.55));
    draw(Vec2::ZERO, color);
}

/// The sidebar logo, in the theme's accent color.
fn paint_logo(painter: &egui::Painter, rect: Rect, theme: &Theme) {
    paint_metal(painter, rect.center() - Vec2::new(0.0, 2.0), Align2::CENTER_CENTER, "Ronnie", 32.0, theme.accent, 1.0);
}

/// Startup splash, `t` seconds in: the letters of "Ronnie" drop in one by one, an accent line and the
/// version appear, then everything fades out. Returns false once it is over.
fn paint_splash(painter: &egui::Painter, screen: Rect, theme: &Theme, t: f32, strings: &Strings) -> bool {
    let warning = strings.alpha_warning;
    const LETTERS: f32 = 0.13; // delay between two letters
    const DROP: f32 = 0.55; // duration of a letter's fall
    const FADE_START: f32 = 3.0;
    const END: f32 = 3.6;
    if t >= END {
        return false;
    }
    let fade = 1.0 - ((t - FADE_START) / (END - FADE_START)).clamp(0.0, 1.0);
    let ease_out_back = |x: f32| {
        let (c1, c3) = (1.70158, 2.70158);
        1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
    };
    painter.rect_filled(screen, 0.0, theme.chrome_bg.gamma_multiply(fade));

    let size = (screen.width() / 7.0).clamp(56.0, 110.0);
    let center = screen.center() - Vec2::new(0.0, size * 0.25);
    // Soft glow behind the name, swelling once the letters have landed.
    let landed = ((t - 0.9) / 0.6).clamp(0.0, 1.0);
    for k in 0..12 {
        let r = size * (0.6 + k as f32 * 0.22) * (0.8 + 0.2 * landed);
        painter.circle_filled(center, r, theme.accent.gamma_multiply(0.012 * landed * fade));
    }

    let font = FontId::new(size, egui::FontFamily::Name("metal".into()));
    let widths: Vec<f32> = "Ronnie".chars().map(|c| painter.layout_no_wrap(c.to_string(), font.clone(), Color32::WHITE).size().x).collect();
    let mut x = center.x - widths.iter().sum::<f32>() / 2.0;
    for (i, c) in "Ronnie".chars().enumerate() {
        let p = ((t - i as f32 * LETTERS) / DROP).clamp(0.0, 1.0);
        if p > 0.0 {
            let y = center.y - (1.0 - ease_out_back(p)) * size * 0.9;
            paint_metal(painter, Pos2::new(x, y), Align2::LEFT_CENTER, &c.to_string(), size, theme.accent, p.min(1.0) * fade);
        }
        x += widths[i];
    }

    // Accent line growing from the middle, then the version under it.
    let line = ((t - 1.1) / 0.45).clamp(0.0, 1.0);
    if line > 0.0 {
        let half = size * 1.6 * (1.0 - (1.0 - line).powi(3));
        let y = center.y + size * 0.62;
        painter.hline(center.x - half..=center.x + half, y, Stroke::new(2.0, theme.accent.gamma_multiply(fade)));
        let version = ((t - 1.4) / 0.4).clamp(0.0, 1.0) * fade;
        painter.text(Pos2::new(center.x, y + 22.0), Align2::CENTER_CENTER, format!("v{}", update::VERSION), FontId::monospace(13.0), theme.text_muted.gamma_multiply(version));
        // Alpha: not for production work.
        let alpha = ((t - 1.6) / 0.4).clamp(0.0, 1.0) * fade;
        let galley = painter.layout(warning.to_owned(), FontId::proportional(12.5), theme.ansi[3].gamma_multiply(alpha), (screen.width() - 48.0).min(460.0));
        let box_rect = Rect::from_center_size(Pos2::new(center.x, y + 62.0), galley.size() + Vec2::new(24.0, 14.0));
        painter.rect_filled(box_rect, 8.0, theme.ansi[3].gamma_multiply(0.10 * alpha));
        painter.rect_stroke(box_rect, 8.0, Stroke::new(1.0, theme.ansi[3].gamma_multiply(0.45 * alpha)), egui::StrokeKind::Inside);
        painter.galley(box_rect.center() - galley.size() / 2.0, galley, theme.ansi[3].gamma_multiply(alpha));
    }

    // At the bottom: made with love, the heart beating.
    let credit = ((t - 1.8) / 0.5).clamp(0.0, 1.0) * fade;
    if credit > 0.0 {
        let font = FontId::proportional(11.5);
        let color = theme.text_muted.gamma_multiply(credit);
        // Spaced out, like a credit.
        let spaced = |text: &str| {
            let mut format = egui::TextFormat::simple(font.clone(), color);
            format.extra_letter_spacing = 2.0;
            painter.layout_job(egui::text::LayoutJob::single_section(text.to_owned(), format))
        };
        let (before, after) = (spaced(strings.splash_made_with), spaced(strings.splash_by));
        let heart_w = 14.0;
        let gap = 7.0;
        let total = before.size().x + gap + heart_w + gap + after.size().x;
        let y = screen.max.y - 34.0;
        let mut x = screen.center().x - total / 2.0;
        painter.galley(Pos2::new(x, y - before.size().y / 2.0), before.clone(), color);
        x += before.size().x + gap;
        let beat = 1.0 + 0.18 * ((t * 7.0).sin().max(0.0)).powi(2);
        paint_heart(painter, Pos2::new(x + heart_w / 2.0, y), 6.0 * beat, theme.ansi[1].gamma_multiply(credit));
        x += heart_w + gap;
        painter.galley(Pos2::new(x, y - after.size().y / 2.0), after, color);
    }
    true
}

/// ❤, drawn (the fonts may lack it): two round lobes on a point.
fn paint_heart(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let lobe = r * 0.55;
    painter.circle_filled(c + Vec2::new(-lobe * 0.9, -r * 0.25), lobe, color);
    painter.circle_filled(c + Vec2::new(lobe * 0.9, -r * 0.25), lobe, color);
    let points = vec![c + Vec2::new(-lobe * 1.85, -r * 0.05), c + Vec2::new(lobe * 1.85, -r * 0.05), c + Vec2::new(0.0, r * 1.05)];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// What was clicked in a pane's header strip.
#[derive(Default)]
struct HeaderClicks {
    /// The ⚙ button: the pane's menu (the right-click one) opens under it.
    gear: Option<egui::Response>,
    /// The strip itself: focus the pane.
    focus: bool,
    reconnect: bool,
    /// Start the pane again with its startup commands.
    relaunch: bool,
    search: bool,
    commands: bool,
    files: bool,
    git: bool,
    /// The git view or the file manager, in a window of its own (right click on their button).
    git_window: bool,
    files_window: bool,
    /// The strip is being dragged (to move the pane onto another).
    drag_started: bool,
    close: bool,
    /// Double click: rename the pane.
    rename: bool,
}

/// What the strip above a pane shows.
enum Header<'a> {
    /// A local pane: its working directory (when known and shown), the local servers announced by its
    /// program, and whether it is in a git repository (Some: whether its changes are shown).
    Local(Option<&'a Path>, &'a [LocalUrl], Option<bool>),
    /// An SSH pane: the host, with a reconnect button.
    Ssh(&'a str),
}

/// Strip above a pane: its working directory (home as `~`, leading folders elided to fit) or its SSH
/// host with reconnect (↻) and file manager (📁) icons, plus the saved commands (⚡) and, for local panes,
/// history search.
#[allow(clippy::too_many_arguments)]
fn pane_header(ui: &mut Ui, rect: Rect, id: PaneId, header: Header, name: Option<&str>, startup: bool, focused: bool, theme: &Theme, t: &Strings, shortcuts: &config::Shortcuts) -> HeaderClicks {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme.chrome_bg);
    let resp = ui.interact(rect, egui::Id::new(("pane-header", id)), Sense::click_and_drag());
    let font = FontId::monospace(12.0);
    let color = if focused { theme.text } else { theme.text_muted };
    let mut max_w = rect.width() - 20.0;

    let mut clicks = HeaderClicks { drag_started: resp.drag_started(), rename: resp.double_clicked(), ..Default::default() };
    let icon = |ui: &mut Ui, right: f32, text: &str, tip: String| {
        let at = Rect::from_min_size(Pos2::new(right - 26.0, rect.min.y + 2.0), Vec2::new(26.0, rect.height() - 4.0));
        let button = egui::Button::new(egui::RichText::new(text).size(15.0)).frame_when_inactive(false).corner_radius(5.0);
        ui.put(at, button).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    // Right click on the files button: in a window of its own.
    let files_menu = |resp: &egui::Response, clicks: &mut HeaderClicks| {
        resp.context_menu(|ui| {
            if ui.button(t.new_window_open).clicked() {
                clicks.files_window = true;
                ui.close();
            }
        });
    };
    let mut right = rect.max.x - 4.0;
    // Close, rightmost (it asks first when a program runs in the pane).
    // Drawn: the font's ✕ is smaller than the other icons.
    {
        let at = Rect::from_min_size(Pos2::new(right - 26.0, rect.min.y + 2.0), Vec2::new(26.0, rect.height() - 4.0));
        let resp = ui.interact(at, egui::Id::new(("pane-close", id)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(at, 5.0, theme.tab_hover);
        }
        let (c, d) = (at.center(), 5.0);
        let stroke = Stroke::new(1.6, if resp.hovered() { theme.text } else { color });
        ui.painter().line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
        ui.painter().line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
        clicks.close = resp.on_hover_text(format!("{}  ({})", t.close_pane, shortcuts.close_pane.label())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
    }
    right -= 30.0;
    max_w -= 30.0;
    // The pane's options, for those who don't think of right-clicking.
    {
        let at = Rect::from_min_size(Pos2::new(right - 26.0, rect.min.y + 2.0), Vec2::new(26.0, rect.height() - 4.0));
        let button = egui::Button::new(egui::RichText::new("⚙").size(14.0).color(color)).frame_when_inactive(false).corner_radius(5.0);
        clicks.gear = Some(ui.put(at, button).on_hover_text(t.pane_options).on_hover_cursor(egui::CursorIcon::PointingHand));
    }
    right -= 30.0;
    max_w -= 30.0;
    // Startup commands: run again (in a fresh shell).
    if startup {
        clicks.relaunch = icon(ui, right, "▶", t.startup_relaunch.to_owned()).clicked();
        right -= 30.0;
        max_w -= 30.0;
    }
    // Local panes: history search, then the local servers (newest on the right), opened in the browser on click.
    if let Header::Local(_, urls, git) = header {
        clicks.search = icon(ui, right, "🔍", format!("{}  ({})", t.search_text_hint, shortcuts.find_text.label())).clicked();
        right -= 30.0;
        clicks.commands = icon(ui, right, "⚡", t.commands.to_owned()).clicked();
        right -= 30.0;
        // This folder's files, with the editor.
        let resp = icon(ui, right, "📁", format!("{}  ({})", t.files_open_here, shortcuts.toggle_files.label()));
        clicks.files = resp.clicked();
        files_menu(&resp, &mut clicks);
        right -= 30.0;
        max_w -= 96.0;
        // In a repository: its changes, in place of the terminal (drawn: no font has a branch).
        if let Some(open) = git {
            let at = Rect::from_min_size(Pos2::new(right - 26.0, rect.min.y + 2.0), Vec2::new(26.0, rect.height() - 4.0));
            let resp = ui.interact(at, egui::Id::new(("pane-git", id)), Sense::click());
            if open || resp.hovered() {
                ui.painter().rect_filled(at, 5.0, if open { theme.tab_active } else { theme.tab_hover });
            }
            git::paint_branch_icon(ui.painter(), Rect::from_center_size(at.center(), Vec2::splat(15.0)), if open { theme.accent } else { color });
            let resp = resp.on_hover_text(if open { t.git_back } else { t.git_open }).on_hover_cursor(egui::CursorIcon::PointingHand);
            clicks.git = resp.clicked();
            resp.context_menu(|ui| {
                if ui.button(t.new_window_open).clicked() {
                    clicks.git_window = true;
                    ui.close();
                }
                if open && ui.button(t.git_back).clicked() {
                    clicks.git = true;
                    ui.close();
                }
            });
            right -= 30.0;
            max_w -= 30.0;
        }
        right -= 4.0;
        for url in urls.iter().rev() {
            let text = egui::RichText::new(format!("↗ :{}", url.port)).size(12.0).monospace().color(theme.bg);
            let button = egui::Button::new(text).fill(theme.ansi[2]).corner_radius(4.0);
            let w = 16.0 + 7.5 * (3 + url.port.to_string().len()) as f32;
            if right - w < rect.min.x + rect.width() / 3.0 {
                break;
            }
            let at = Rect::from_min_max(Pos2::new(right - w, rect.min.y + 3.0), Pos2::new(right, rect.max.y - 3.0));
            if ui.put(at, button).on_hover_text(&url.url).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                crate::terminal::open_url(&url.url);
            }
            right -= w + 6.0;
            max_w = right - rect.min.x - 16.0;
        }
    }
    if let Header::Ssh(_) = header {
        clicks.reconnect = icon(ui, right, "↻", t.reconnect.to_owned()).clicked();
        right -= 30.0;
        clicks.commands = icon(ui, right, "⚡", t.commands.to_owned()).clicked();
        right -= 30.0;
        // The server's files, FileZilla style.
        let resp = icon(ui, right, "📁", format!("{}  ({})", t.files_open, shortcuts.toggle_files.label()));
        clicks.files = resp.clicked();
        files_menu(&resp, &mut clicks);
        max_w -= 96.0;
    }

    let (text, tooltip) = match header {
        Header::Ssh(host) => (host.to_owned(), None),
        Header::Local(None, _, _) if name.is_none() => {
            clicks.focus = resp.clicked();
            return clicks;
        }
        Header::Local(None, _, _) => (String::new(), None),
        Header::Local(Some(cwd), _, _) => {
            let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
            let (prefix, rest) = match home.as_deref().and_then(|h| cwd.strip_prefix(h).ok()) {
                Some(rest) => ("~", rest),
                None => ("", cwd),
            };
            let parts: Vec<String> = rest.iter().map(|p| p.to_string_lossy().into_owned()).filter(|p| p != "/").collect();
            let label = |skip: usize| {
                let head = if skip > 0 { "…" } else { prefix };
                let tail = parts[skip..].join("/");
                if tail.is_empty() { if head.is_empty() { "/".to_owned() } else { head.to_owned() } } else { format!("{head}/{tail}") }
            };
            let mut skip = 0;
            while painter.layout_no_wrap(label(skip), font.clone(), color).size().x > max_w && skip + 1 < parts.len() {
                skip += 1;
            }
            (label(skip), Some(cwd.display().to_string()))
        }
    };
    // The pane's name first, in the accent color.
    let mut job = egui::text::LayoutJob::default();
    if let Some(name) = name {
        job.append(name, 0.0, egui::TextFormat { font_id: font.clone(), color: if focused { theme.accent } else { theme.accent.gamma_multiply(0.7) }, ..Default::default() });
        if !text.is_empty() {
            job.append("  ·  ", 0.0, egui::TextFormat { font_id: font.clone(), color: theme.text_muted, ..Default::default() });
        }
    }
    job.append(&text, 0.0, egui::TextFormat { font_id: font.clone(), color, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(0.0));
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(rect.min.x + 10.0, rect.center().y - galley.size().y / 2.0), galley, color);
    let resp = match tooltip {
        Some(tip) => resp.on_hover_text(tip),
        None => resp,
    };
    clicks.focus = resp.clicked();
    clicks
}

/// Bar at the bottom of an SSH pane whose connection ended. Returns (reconnect, close) clicks.
fn closed_banner(ui: &mut Ui, pane: Rect, theme: &Theme, t: &Strings) -> (bool, bool) {
    let bar = Rect::from_min_max(Pos2::new(pane.min.x, pane.max.y - 40.0), pane.max);
    ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
    ui.painter().hline(bar.x_range(), bar.min.y, Stroke::new(1.0, theme.accent.gamma_multiply(0.6)));
    ui.painter().text(bar.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, t.ssh_closed, FontId::proportional(13.0), theme.text);
    let size = Vec2::new(150.0, 26.0);
    let reconnect_rect = Rect::from_min_size(Pos2::new(bar.max.x - 12.0 - 2.0 * size.x - 8.0, bar.center().y - size.y / 2.0), size);
    let close_rect = reconnect_rect.translate(Vec2::new(size.x + 8.0, 0.0));
    let reconnect = egui::Button::new(egui::RichText::new(format!("{}  ↩", t.reconnect)).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0);
    let close = egui::Button::new(egui::RichText::new(format!("{}  Esc", t.close)).size(13.0)).corner_radius(6.0);
    (ui.put(reconnect_rect, reconnect).clicked(), ui.put(close_rect, close).clicked())
}

/// Pencil: the edit icon.
fn paint_pencil(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let (a, b) = (c + Vec2::new(-4.0, 4.0), c + Vec2::new(3.5, -3.5));
    painter.line_segment([a, b], Stroke::new(2.6, color));
    painter.line_segment([a, a + Vec2::new(-1.5, 1.5)], stroke);
    painter.line_segment([b + Vec2::new(0.8, -0.8), b + Vec2::new(1.8, -1.8)], Stroke::new(2.6, color));
}

/// Gear outline: the settings icon.
fn paint_gear(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.circle_stroke(c, 4.2, stroke);
    painter.circle_stroke(c, 1.6, stroke);
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::TAU / 8.0;
        let dir = Vec2::angled(a);
        painter.line_segment([c + dir * 4.8, c + dir * 7.0], Stroke::new(2.0, color));
    }
}

/// A small clickable icon with a hover background.
fn icon_button(ui: &Ui, painter: &egui::Painter, rect: Rect, salt: &str, theme: &Theme, paint: fn(&egui::Painter, Pos2, Color32)) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(salt), Sense::click());
    if resp.hovered() {
        painter.rect_filled(rect, 4.0, theme.tab_hover);
    }
    paint(painter, rect.center(), if resp.hovered() { theme.text } else { theme.text_muted });
    resp
}

fn paint_plus(painter: &egui::Painter, c: Pos2, color: Color32) {
    let (stroke, d) = (Stroke::new(1.6, color), 5.5);
    painter.line_segment([c - Vec2::new(d, 0.0), c + Vec2::new(d, 0.0)], stroke);
    painter.line_segment([c - Vec2::new(0.0, d), c + Vec2::new(0.0, d)], stroke);
}

fn paint_cross(painter: &egui::Painter, c: Pos2, color: Color32) {
    let (stroke, d) = (Stroke::new(1.4, color), 3.5);
    painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
    painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
}

/// Triangle pointing down (open) or right (collapsed).
fn paint_chevron(painter: &egui::Painter, c: Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(4.0, -2.0), c + Vec2::new(0.0, 2.5)]
    } else {
        vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(2.5, 0.0), c + Vec2::new(-2.0, 4.0)]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Selects the whole text of a text field, so typing replaces it.
fn select_all(ui: &Ui, id: egui::Id, text: &str) {
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), id);
    }
}


/// A menu button that records `a` as the action and closes the menu.
fn menu_item<A>(ui: &mut Ui, label: &str, a: A, action: &mut Option<A>) {
    if ui.button(label).clicked() {
        *action = Some(a);
        ui.close();
    }
}

#[derive(Clone)]
enum TabAction {
    Select(usize),
    Close(usize),
    New,
    StartRename(usize),
    /// Commit the rename; true when confirmed with Enter (a refused name then keeps the editor open).
    Rename(usize, bool),
    SetColor(usize, Option<Color32>),
    DragOver(usize, f32),
    Split(usize, Direction),
    DeleteProfile(Uuid),
    NewHost,
    EditHost(Uuid),
    DeleteHost(Uuid),
    OpenItem(Uuid),
    ItemColor(Uuid, Option<Color32>),
    StartItemRename(Uuid),
    /// New name; true when confirmed with Enter.
    RenameItem(Uuid, String, bool),
    ToggleGroup(Uuid),
    StartGroupRename(Uuid),
    RenameGroup(Uuid, String),
    /// New group of local profiles (true) or of SSH hosts.
    NewGroup(bool),
    /// New group holding this item.
    NewGroupWith(Uuid),
    /// Opens the profile editor.
    EditProfile(Uuid),
    /// Opens an SSH host's tab on its file manager.
    OpenFiles(Uuid),
    DeleteGroup(Uuid),
    /// Move an item into a group (None: outside groups), before another item or at the end.
    DropItem(Uuid, Option<Uuid>, Option<Uuid>),
    /// Move a group before another one, or last.
    DropGroup(Uuid, Option<Uuid>),
    /// Restart every session of an SSH tab.
    Reconnect(usize),
    /// Copy a profile (right after it), or open the host editor on a copy of a host.
    Duplicate(Uuid),
    /// Open a profile or an SSH host in a new window (moving its tab there if it is open here).
    ItemNewWindow(Uuid),
    NewDb,
    /// New database connection, filled for the server found on this machine.
    AddLocalDb,
    OpenDb(Uuid),
    EditDb(Uuid),
    DeleteDb(Uuid),
    /// Move an open tab to a new window.
    TabNewWindow(usize),
}

enum PaneAction {
    Copy(PaneId),
    Paste(PaneId),
    Split(PaneId, Direction),
    Close(PaneId),
    Reveal(PaneId),
    Reconnect(PaneId),
    Rename(PaneId),
    /// The file manager (this folder, or the server's).
    Files(PaneId),
    /// Write a saved command at the prompt, without running it.
    Insert(PaneId, String),
    /// Open the saved commands menu.
    Commands(PaneId),
    /// Edit the commands typed when the pane starts.
    Startup(PaneId),
    /// Start the pane again, with its startup commands.
    Relaunch(PaneId),
    /// Open closed pane k (see `App::closed_panes`) next to this one.
    Reopen(PaneId, usize),
}

/// Right-click menu of a terminal pane.
#[allow(clippy::too_many_arguments)]
fn pane_menu(ui: &mut Ui, t: &Strings, shortcuts: &config::Shortcuts, id: PaneId, can_copy: bool, local: bool, startup: bool, closed: &[String], commands: &[String], action: &mut Option<PaneAction>) {
    ui.set_min_width(180.0);
    let shortcut = |mac: &str, other: &str| if cfg!(target_os = "macos") { mac.to_owned() } else { other.to_owned() };
    let mut item = |ui: &mut Ui, enabled: bool, label: &str, hint: String, a: PaneAction| {
        let button = egui::Button::new(label).shortcut_text(hint);
        if ui.add_enabled(enabled, button).clicked() {
            *action = Some(a);
            ui.close();
        }
    };
    item(ui, can_copy, t.copy, shortcut("⌘ C", "Ctrl+Shift+C"), PaneAction::Copy(id));
    item(ui, true, t.paste, shortcut("⌘ V", "Ctrl+Shift+V"), PaneAction::Paste(id));
    ui.separator();
    // The splits with a shortcut first.
    item(ui, true, t.split_right, shortcuts.split_right.label(), PaneAction::Split(id, Direction::Right));
    item(ui, true, t.split_down, shortcuts.split_down.label(), PaneAction::Split(id, Direction::Down));
    item(ui, true, t.split_left, String::new(), PaneAction::Split(id, Direction::Left));
    item(ui, true, t.split_up, String::new(), PaneAction::Split(id, Direction::Up));
    ui.separator();
    if local {
        item(ui, true, t.files_open_here, shortcuts.toggle_files.label(), PaneAction::Files(id));
        // An SSH pane's directory is on the server.
        item(ui, true, t.open_location, String::new(), PaneAction::Reveal(id));
    } else {
        item(ui, true, t.files_open, shortcuts.toggle_files.label(), PaneAction::Files(id));
        item(ui, true, t.reconnect, String::new(), PaneAction::Reconnect(id));
    }
    ui.separator();
    let mut picked = None;
    ui.menu_button(t.commands, |ui| {
        ui.set_min_width(220.0);
        if commands.is_empty() {
            ui.label(egui::RichText::new(t.commands_empty).color(ui.visuals().weak_text_color()));
        }
        for command in commands {
            let label = egui::RichText::new(command.replace('\n', " ⏎ ")).monospace();
            if ui.add(egui::Button::new(label).truncate()).clicked() {
                picked = Some(PaneAction::Insert(id, command.clone()));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(t.commands_manage).clicked() {
            picked = Some(PaneAction::Commands(id));
            ui.close();
        }
    });
    item(ui, true, t.startup_menu, String::new(), PaneAction::Startup(id));
    if startup {
        item(ui, true, t.startup_relaunch, String::new(), PaneAction::Relaunch(id));
    }
    ui.separator();
    item(ui, true, t.rename_pane, String::new(), PaneAction::Rename(id));
    // Named panes closed, the latest first.
    let mut reopen = None;
    if !closed.is_empty() {
        ui.menu_button(t.reopen_pane, |ui| {
            ui.set_min_width(200.0);
            for (k, name) in closed.iter().enumerate().rev() {
                if ui.button(name).clicked() {
                    reopen = Some(PaneAction::Reopen(id, k));
                    ui.close();
                }
            }
        });
    }
    item(ui, true, t.close_pane, shortcuts.close_pane.label(), PaneAction::Close(id));
    if picked.is_some() || reopen.is_some() {
        *action = picked.or(reopen);
    }
}

impl App {
    /// Draws the other windows, each with its state swapped in; closes those that were closed, and adds
    /// those opened meanwhile.
    /// Draws the git views and file managers opened in windows of their own; closes those closed.
    fn tool_windows_ui(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let theme = self.theme.clone();
        let mut k = 0;
        while k < self.tool_windows.len() {
            let w = &mut self.tool_windows[k];
            let builder = egui::ViewportBuilder::default().with_title(w.title.clone()).with_app_id(crate::update::APP_ID).with_min_inner_size([520.0, 320.0]).with_inner_size([1100.0, 720.0]);
            let (mut close, mut reconnect) = (false, None);
            ctx.show_viewport_immediate(w.viewport, builder, |ui, _| {
                let rect = ui.max_rect();
                ui.painter().rect_filled(rect, 0.0, theme.bg);
                match &mut w.tool {
                    Tool::Git(view) => close = matches!(view.ui(ui, rect, &theme, t, true), Some(git::Exit::Back)),
                    Tool::Files(fm, host) => match fm.ui(ui, rect, &theme, t, &w.title) {
                        files::FilesAction::ShowTerminal => close = true,
                        files::FilesAction::Reconnect => reconnect = *host,
                        files::FilesAction::None => {}
                    },
                }
                if ui.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                // Transfers or unsaved changes: "Close anyway?" first.
                if close && w.confirm.is_none() {
                    let busy = w.busy(t);
                    if !busy.is_empty() {
                        ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                        w.confirm = Some(busy);
                        close = false;
                    }
                } else if close {
                    // Asked again while the question shows: it stays open until answered.
                    ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                    close = false;
                }
                if let Some(busy) = &w.confirm {
                    let mut answer = None;
                    let frame = Frame::popup(&ui.ctx().global_style()).inner_margin(20.0).fill(theme.chrome_bg);
                    let modal = egui::Modal::new(egui::Id::new("tool-close")).frame(frame).show(ui.ctx(), |ui| {
                        ui.set_width(380.0);
                        ui.label(egui::RichText::new(t.close_anyway_title).size(17.0).strong());
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(t.close_anyway_body).size(13.5).color(theme.text_muted));
                        ui.add_space(6.0);
                        for item in busy {
                            ui.label(egui::RichText::new(format!("•  {item}")).size(13.0).monospace());
                        }
                        ui.add_space(14.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close = egui::Button::new(egui::RichText::new(t.close).size(13.5).color(Color32::WHITE)).fill(theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0));
                            if ui.add(close).clicked() {
                                answer = Some(true);
                            }
                            let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                            cancel.request_focus();
                            if cancel.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                                answer = Some(false);
                            }
                        });
                    });
                    if modal.should_close() {
                        answer = Some(false);
                    }
                    match answer {
                        Some(true) => close = true,
                        Some(false) => w.confirm = None,
                        None => {}
                    }
                }
            });
            if let Some(host) = reconnect.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)).cloned() {
                if let Tool::Files(fm, _) = &mut self.tool_windows[k].tool {
                    fm.connect(&self.ctx, &host, &self.askpass);
                }
            }
            if close {
                self.tool_windows.remove(k);
            } else {
                k += 1;
            }
        }
    }

    fn other_windows(&mut self, ctx: &egui::Context) {
        let mut k = 0;
        while k < self.others.len() {
            let mut slot = std::mem::take(&mut self.others[k]);
            self.swap_window(&mut slot);
            // While it is drawn, `others` holds every other window (the main one included).
            self.others[k] = slot;
            let mut builder = egui::ViewportBuilder::default().with_title(APP_TITLE).with_app_id(crate::update::APP_ID).with_min_inner_size([520.0, 320.0]);
            match self.opened_at {
                Some(w) => builder = builder.with_position([w.x, w.y]).with_inner_size([w.width, w.height]),
                None => builder = builder.with_inner_size([1100.0, 720.0]),
            }
            let viewport = self.viewport;
            ctx.show_viewport_immediate(viewport, builder, |ui, _| self.window_ui(ui));
            // Its last tab closed: the window goes too.
            let closing = self.window_closing || self.tabs.is_empty();
            if closing {
                while !self.tabs.is_empty() {
                    self.close_tab(0);
                }
            }
            let mut slot = std::mem::take(&mut self.others[k]);
            self.swap_window(&mut slot);
            if closing {
                // Its dialogs go back to the main window.
                if self.dialog_viewport == slot.viewport {
                    self.dialog_viewport = egui::ViewportId::ROOT;
                }
                self.others.remove(k);
            } else {
                self.others[k] = slot;
                k += 1;
            }
        }
        self.others.append(&mut self.new_windows);
    }

    /// One window: its sidebar, tabs and dialogs. Called for the main window, then for each other one
    /// with its state swapped in (see `WindowSlot`).
    fn window_ui(&mut self, ui: &mut Ui) {
        let main_window = self.viewport == egui::ViewportId::ROOT;
        // App-wide dialogs show in the window last in front.
        if ui.input(|i| i.viewport().focused) == Some(true) {
            self.dialog_viewport = self.viewport;
        }
        let dialogs_here = self.dialog_viewport == self.viewport;
        // Background tabs must keep answering their programs (cursor position reports, etc).
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            for term in tab.panes.values_mut() {
                term.set_allow_clipboard(self.config.settings.clipboard_from_programs);
                term.process_events(ui.ctx(), &self.theme);
                // Panes behind the file manager aren't on screen either.
                term.set_visible(i == self.active && self.home.is_none() && !tab.show_files);
            }
        }
        self.finished_commands(ui.ctx());

        // Panes whose shell has exited close by themselves. SSH panes stay, to show why the connection ended.
        let exited: Vec<(PaneId, usize)> = self
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                t.panes.iter().filter(|(id, term)| term.has_exited() && !t.dead.contains(id)).map(move |(id, _)| (*id, i))
            })
            .collect();
        // Highest tab index first so earlier indices stay valid if a tab closes.
        for (pane, index) in exited.into_iter().rev() {
            if self.tabs[index].ssh.is_some() {
                self.tabs[index].dead.insert(pane);
            } else {
                self.close_pane(index, pane);
            }
        }
        self.handle_shortcuts(ui);
        self.poll_files();
        for tab in &mut self.tabs {
            if let Some(view) = &mut tab.db_view {
                view.poll();
            }
        }

        self.track_window(ui.ctx());
        let now = ui.input(|i| i.time);
        // Changes come with input: keep syncing shortly after it, then let an idle app sleep. Outside
        // edits of config.json wake it up through the config watcher.
        if ui.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.last_input = now;
        }
        if now - self.last_input < 3.0 * SYNC_INTERVAL {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(SYNC_INTERVAL));
        }

        // Folding and unfolding glide: the width eases between the rail and the sidebar, the sidebar
        // sliding under the edge, a light running along it.
        let folded = self.config.settings.sidebar_folded;
        let open = ui.ctx().animate_bool_with_time_and_easing(egui::Id::new("sidebar-open"), !folded, 0.38, motion::in_out_cubic);
        let width = RAIL_WIDTH + (SIDEBAR_WIDTH - RAIL_WIDTH) * open;
        let theme = self.theme.clone();
        egui::Panel::left("sidebar")
            .exact_size(width)
            .resizable(false)
            .frame(Frame::NONE)
            .show(ui, |ui| {
                if open <= 0.001 {
                    return self.sidebar_rail(ui);
                }
                if open >= 0.999 {
                    return self.sidebar(ui);
                }
                let panel = ui.max_rect();
                let full = Rect::from_min_size(Pos2::new(panel.min.x - (SIDEBAR_WIDTH - width) * 0.6, panel.min.y), Vec2::new(SIDEBAR_WIDTH, panel.height()));
                let mut moving = ui.new_child(egui::UiBuilder::new().max_rect(full).layout(egui::Layout::top_down(egui::Align::Min)));
                moving.set_clip_rect(panel);
                self.sidebar(&mut moving);
                // Darker as it folds, and a streak of light on its edge, brightest mid-way.
                ui.painter().rect_filled(panel, 0.0, theme.bg.gamma_multiply(0.55 * (1.0 - open)));
                let glow = 1.0 - (2.0 * open - 1.0).abs();
                for (w, a) in [(8.0, 0.08), (4.0, 0.18), (1.5, 0.9)] {
                    ui.painter().vline(panel.max.x - 1.0, panel.y_range(), Stroke::new(w, theme.accent.gamma_multiply(a * glow)));
                }
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(self.theme.bg))
            .show(ui, |ui| {
                if let Some(err) = &self.error {
                    ui.colored_label(Color32::from_rgb(0xf7, 0x76, 0x8e), err);
                }
                let rect = ui.available_rect_before_wrap();
                if self.shown_tab().is_none() {
                    self.home_page(ui, rect);
                    return;
                }
                // Away from the home page: its game stops, music included, and the sidebar comes back.
                self.game.leave();
                self.fold_for_game(false);
                // An SSH tab showing its file manager.
                if self.tabs.get(self.active).is_some_and(|t| t.show_files) {
                    self.files_view(ui, rect);
                    return;
                }
                // A database tab showing its view.
                if self.tabs.get(self.active).is_some_and(|t| t.db.is_some() && t.show_db) {
                    self.db_view_ui(ui, rect);
                    return;
                }
                self.start_pending(ui.ctx(), self.active);
                // SSH host whose saved password can answer prompts in this tab (sudo...).
                let password_host = self.tabs.get(self.active).and_then(|t| t.ssh).filter(|id| self.config.ssh.iter().any(|h| h.id == *id && h.uses_saved_password()));
                let saved_commands = self.saved_commands(self.active);
                // Named panes closed, to reopen in this tab (a local one).
                let closed_panes: Vec<String> = if self.tabs.get(self.active).is_some_and(|t| t.ssh.is_none() && t.db.is_none()) {
                    self.closed_panes.iter().map(|p| match p {
                        Layout::Pane { name: Some(n), .. } => n.clone(),
                        Layout::Pane { cwd, .. } => cwd.as_ref().and_then(|c| c.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Terminal".into()),
                        Layout::Split { .. } => String::new(),
                    }).collect()
                } else {
                    Vec::new()
                };
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    self.home_page(ui, rect);
                    return;
                };
                // Given once the click that asked for it (in the sidebar...) is over: during that frame, egui
                // takes the focus away from every widget the pointer isn't on, the terminal included.
                if self.focus_terminal && self.rename.is_none() && self.pane_rename.is_none() {
                    if ui.input(|i| i.pointer.any_pressed() || i.pointer.any_click() || i.pointer.any_down()) {
                        ui.ctx().request_repaint();
                    } else {
                        if let Some(term) = tab.panes.get(&tab.focused) {
                            term.request_focus(ui);
                        }
                        self.focus_terminal = false;
                    }
                }

                let mut rects = Vec::with_capacity(tab.panes.len());
                let line = Stroke::new(1.0, self.theme.tab_hover);
                tab.layout.show(ui, rect, ui.id().with("layout"), line, &mut rects);
                let split = rects.len() > 1;
                let strings = self.config.settings.language.strings();
                let local = tab.ssh.is_none();
                let host = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id));
                let connecting = host.map(|h| strings.connecting.replace("{host}", &h.name).replace("{address}", &h.address())).unwrap_or_default();
                let ssh_label = host.map(|h| format!("{}  ·  {}", h.name, h.address())).unwrap_or_default();
                let mut pane_action = None;
                let mut reconnect: Option<Vec<PaneId>> = None;
                let mut relaunch: Option<PaneId> = None;
                let mut reopen: Option<(PaneId, usize)> = None;
                let mut close_dead = None;
                let mut open_search = None;
                let mut open_commands = None;
                let mut open_files = false;
                let mut open_files_window = None;
                // Path suggestions (→ takes one, Alt+↑ ↓ picks): at a local prompt (zsh tells what is typed,
                // bash's prompt is read from the screen), or at a server's usual prompt (read from the screen; the server's files
                // come through a background SFTP session). Checked before the panes handle keys, so these
                // don't reach the terminal.
                let mut suggest: Option<(String, String, Vec<(String, bool)>)> = None;
                let mut remote_query = None;
                let modal = ui.ctx().memory(|m| m.top_modal_layer().is_some());
                if let Some(term) = tab.panes.get(&tab.focused).filter(|t| self.config.settings.path_suggestions && !modal && !t.scrolled_back() && t.has_focus(ui)) {
                    if local {
                        suggest = (|| {
                            let (line, end) = term.typed_input()?;
                            if !end {
                                return None;
                            }
                            let cwd = term.cwd()?;
                            let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
                            let wanted = if cfg!(windows) { complete::parse_windows(&line, &cwd, &home) } else { complete::parse(&line, &cwd, &home) }?;
                            let listed = self.path_cache.get(&wanted.dir).is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(2));
                            if !listed {
                                if self.path_cache.len() > 64 {
                                    self.path_cache.clear();
                                }
                                self.path_cache.insert(wanted.dir.clone(), (std::time::Instant::now(), complete::list(&wanted.dir)));
                            }
                            let items: Vec<(String, bool)> = complete::matching(&self.path_cache[&wanted.dir].1, &wanted.prefix, 6, cfg!(windows)).into_iter().cloned().collect();
                            (!items.is_empty()).then_some((line, wanted.prefix, items))
                        })();
                    } else if let Some((line, true)) = term.guess_prompt_input() {
                        // Only when a path is being typed (no SFTP session for nothing).
                        if complete::parse_remote(&line, "/", "/").is_some() {
                            remote_query = Some((line, term.reported_cwd()));
                        }
                    }
                }
                // Hosts that log in without asking (agent, key, saved password): a password window must
                // never pop up for a suggestion.
                let quiet_host = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)).filter(|h| matches!(h.auth_method(), SshAuth::Auto | SshAuth::Key) || h.uses_saved_password()).cloned();
                if let (Some((line, cwd)), Some(host)) = (remote_query, quiet_host) {
                    if tab.files.is_none() {
                        let mut fm = Box::new(files::FileManager::new(&host));
                        fm.connect(&self.ctx, &host, &self.askpass);
                        tab.files = Some(fm);
                    }
                    let mut waiting;
                    if let Some(fm) = tab.files.as_mut() {
                        waiting = fm.connecting();
                        suggest = (|| {
                            let home = fm.remote_home()?.to_owned();
                            let cwd = match cwd {
                                Some(c) if c == "~" || c.starts_with("~/") => format!("{home}{}", &c[1..]),
                                Some(c) => c,
                                None => home.clone(),
                            };
                            let (dir, prefix) = complete::parse_remote(&line, &cwd, &home)?;
                            let entries = fm.remote_entries(&dir);
                            waiting = entries.is_none();
                            let items: Vec<(String, bool)> = complete::matching(&entries?, &prefix, 6, false).into_iter().cloned().collect();
                            (!items.is_empty()).then_some((line, prefix, items))
                        })();
                    } else {
                        waiting = true;
                    }
                    // The session or the listing arrives in the background.
                    if waiting {
                        ui.ctx().request_repaint_after(Duration::from_millis(150));
                    }
                }
                // "ronnie" + Enter: not for the shell. The line is erased, the pane puts on a show and the
                // video opens in the browser.
                if self.guard_confirm.is_none() && !modal {
                    if let Some(term) = tab.panes.get_mut(&tab.focused).filter(|t| !t.scrolled_back() && t.has_focus(ui)) {
                        let line = if local { term.typed_input() } else { term.guess_prompt_input() }.map(|(l, _)| l);
                        if line.is_some_and(|l| easter::called(&l)) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                            // Ctrl+U: the line typed, gone.
                            term.type_text("\x15");
                            self.ronnie_show = Some((tab.focused, ui.input(|i| i.time)));
                            crate::terminal::open_url(easter::VIDEO);
                        }
                    }
                }
                // The metal guard: Enter on a destructive command waits for a confirmation.
                if self.config.settings.metal_guard && self.guard_confirm.is_none() && !modal {
                    if let Some(term) = tab.panes.get(&tab.focused).filter(|t| !t.scrolled_back() && t.has_focus(ui)) {
                        let line = if local { term.typed_input() } else { term.guess_prompt_input() }.map(|(l, _)| l);
                        if let Some((line, danger)) = line.and_then(|l| Some((l.clone(), guard::check(&l, !local)?))) {
                            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                                self.guard_confirm = Some((tab.focused, line, danger));
                            }
                        }
                    }
                }
                // Escape put the list away for this line.
                if suggest.as_ref().is_some_and(|(line, _, _)| self.path_dismissed.as_ref() == Some(line)) {
                    suggest = None;
                }
                let mut take_suggestion = None;
                if let Some((line, _, items)) = &suggest {
                    if self.path_pick.0 != *line {
                        self.path_pick = (line.clone(), 0, false);
                    }
                    let (_, pick, chosen) = &mut self.path_pick;
                    *pick = (*pick).min(items.len() - 1);
                    let mut dismiss = false;
                    ui.input_mut(|i| {
                        // While the list shows, ↑ ↓ move in it (the shell's history is one Escape away).
                        if i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::ALT, Key::ArrowDown) {
                            (*pick, *chosen) = ((*pick + 1) % items.len(), true);
                        }
                        if i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::ALT, Key::ArrowUp) {
                            (*pick, *chosen) = ((*pick + items.len() - 1) % items.len(), true);
                        }
                        // → takes the one shown; Enter only one picked with the arrows (else it runs the line).
                        if i.consume_key(Modifiers::NONE, Key::ArrowRight) || (*chosen && i.consume_key(Modifiers::NONE, Key::Enter)) {
                            take_suggestion = Some(*pick);
                        }
                        dismiss = i.consume_key(Modifiers::NONE, Key::Escape);
                    });
                    if dismiss {
                        self.path_dismissed = Some(line.clone());
                    }
                }
                // Checked before the panes handle keys, so Cmd+Enter doesn't reach the terminal.
                let prompt = password_host.filter(|_| tab.panes.get(&tab.focused).is_some_and(Terminal::awaits_password));
                let mut fill_password = prompt.is_some()
                    && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter)));
                for &(id, r) in &rects {
                    let Some(term) = tab.panes.get_mut(&id) else { continue };
                    // Startup commands, typed once the shell shows its prompt.
                    if let Some(since) = tab.startup_due.get(&id).copied() {
                        if term.ready_for_commands(since.elapsed(), local) {
                            let commands: String = tab.startup.get(&id).map(|c| c.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| format!("{l}\r")).collect()).unwrap_or_default();
                            term.type_text(&commands);
                            tab.startup_due.remove(&id);
                        } else if since.elapsed() > Duration::from_secs(60) {
                            tab.startup_due.remove(&id);
                        } else {
                            ui.ctx().request_repaint_after(Duration::from_millis(100));
                        }
                    }
                    // Local panes show their directory (optional); SSH panes their host and a reconnect button.
                    // The strip also appears, even with directories hidden, when the program announced a local server.
                    let urls = if local { term.local_urls(ui.ctx()).to_vec() } else { Vec::new() };
                    // The repository of a local pane's folder, if any.
                    let repo = if local { term.cached_cwd(ui.ctx()).and_then(git::repo_root) } else { None };
                    let git_open = tab.git.contains_key(&id);
                    // Split: always a strip, to drag the pane by.
                    let body = if !local || self.config.settings.show_cwd || !urls.is_empty() || split || repo.is_some() || git_open {
                        let (head, body) = r.split_top_bottom_at_y(r.min.y + PANE_HEADER_H);
                        let cwd = if local && self.config.settings.show_cwd { term.cached_cwd(ui.ctx()).map(Path::to_path_buf) } else { None };
                        // SSH: the folder on the server, when its shell tells.
                        let remote_label = (!local && self.config.settings.show_cwd).then(|| term.reported_cwd()).flatten().map(|dir| format!("{ssh_label}  ·  {dir}"));
                        let git_state = (repo.is_some() || git_open).then_some(git_open);
                        let header = if local { Header::Local(cwd.as_deref(), &urls, git_state) } else { Header::Ssh(remote_label.as_deref().unwrap_or(&ssh_label)) };
                        let clicks = pane_header(ui, head, id, header, tab.names.get(&id).map(String::as_str), tab.startup.contains_key(&id), id == tab.focused, &self.theme, strings, &self.config.settings.shortcuts);
                        if clicks.rename {
                            self.pane_rename = Some((id, tab.names.get(&id).cloned().unwrap_or_default(), true));
                        }
                        if let Some(gear) = &clicks.gear {
                            let (can_copy, startup) = (term.has_selection(), tab.startup.contains_key(&id));
                            egui::Popup::menu(gear).show(|ui| pane_menu(ui, strings, &self.config.settings.shortcuts, id, can_copy, local, startup, &closed_panes, &saved_commands, &mut pane_action));
                        }
                        // Renaming: the name is typed in the strip itself.
                        if let Some((_, text, fresh)) = self.pane_rename.as_mut().filter(|(p, _, _)| *p == id) {
                            let field = Rect::from_min_max(head.min + Vec2::new(6.0, 3.0), Pos2::new(head.min.x + (head.width() * 0.5).max(160.0), head.max.y - 3.0));
                            ui.painter().rect_filled(field, 5.0, self.theme.bg);
                            let edit = ui.put(field.shrink2(Vec2::new(6.0, 1.0)), egui::TextEdit::singleline(text).font(FontId::monospace(12.0)).frame(Frame::NONE).hint_text(strings.pane_name_hint));
                            ui.painter().rect_stroke(field, 5.0, Stroke::new(1.0, self.theme.accent), egui::StrokeKind::Inside);
                            if *fresh {
                                edit.request_focus();
                                *fresh = false;
                            }
                            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                            if escape {
                                self.pane_rename = None;
                                self.focus_terminal = true;
                            } else if enter || edit.lost_focus() {
                                let name = text.trim().to_owned();
                                if name.is_empty() {
                                    tab.names.remove(&id);
                                } else {
                                    tab.names.insert(id, name);
                                }
                                self.pane_rename = None;
                                self.focus_terminal = true;
                            }
                        }
                        if clicks.close {
                            pane_action = Some(PaneAction::Close(id));
                        }
                        if clicks.drag_started && split {
                            self.pane_drag = Some(id);
                        }
                        if clicks.search {
                            open_search = Some(id);
                        }
                        if clicks.commands {
                            open_commands = Some(id);
                        }
                        if clicks.files {
                            open_files = true;
                        }
                        if clicks.git_window {
                            // The view shown in the pane moves there, as it was.
                            if let Some(view) = tab.git.remove(&id).or_else(|| repo.clone().map(|root| Box::new(git::GitView::new(root)))) {
                                self.tool_windows.push(ToolWindow::git(view));
                            }
                        }
                        if clicks.files_window {
                            open_files_window = Some(id);
                        }
                        if clicks.git {
                            if git_open {
                                tab.git.remove(&id);
                                self.focus_terminal = true;
                            } else if let Some(root) = repo.clone() {
                                tab.git.insert(id, Box::new(git::GitView::new(root)));
                            }
                        }
                        if clicks.focus {
                            term.request_focus(ui);
                        }
                        if clicks.reconnect {
                            reconnect = Some(vec![id]);
                        }
                        if clicks.relaunch {
                            relaunch = Some(id);
                        }
                        body
                    } else {
                        r
                    };
                    if let Some(view) = tab.git.get_mut(&id) {
                        // The repository's changes, in place of the terminal.
                        match view.ui(ui, body, &self.theme, strings, false) {
                            Some(git::Exit::Back) => {
                                tab.git.remove(&id);
                                self.focus_terminal = true;
                                tab.focused = id;
                            }
                            Some(git::Exit::Window) => {
                                if let Some(view) = tab.git.remove(&id) {
                                    self.tool_windows.push(ToolWindow::git(view));
                                }
                            }
                            None => {}
                        }
                    } else {
                        let resp = term.ui(ui, body, &self.theme, &self.fonts);
                        if resp.secondary_clicked() {
                            resp.request_focus();
                        }
                        if resp.has_focus() {
                            tab.focused = id;
                        }
                        let can_copy = term.has_selection();
                        let startup = tab.startup.contains_key(&id);
                        resp.context_menu(|ui| pane_menu(ui, strings, &self.config.settings.shortcuts, id, can_copy, local, startup, &closed_panes, &saved_commands, &mut pane_action));
                    }
                    if let Some((_, since)) = self.ronnie_show.filter(|(p, _)| *p == id) {
                        let hand = self.metal_hand.get_or_insert_with(|| load_png(ui.ctx(), "metal-hand", include_bytes!("../../assets/icon/metal-hand.png"))).clone();
                        let time = (ui.input(|i| i.time) - since) as f32;
                        if easter::show(ui, r, &self.theme, strings, time, &hand) {
                            self.ronnie_show = None;
                            self.focus_terminal = true;
                        }
                    }
                    if tab.dead.contains(&id) {
                        let (again, close) = closed_banner(ui, r, &self.theme, strings);
                        if again {
                            reconnect = Some(vec![id]);
                        }
                        if close {
                            close_dead = Some(id);
                        }
                    } else if !local && !term.has_output() {
                        paint_connecting(ui, r, &self.theme, strings, &connecting);
                        // Keep the spinner turning while nothing else repaints (only while connecting).
                        ui.ctx().request_repaint_after(Duration::from_millis(30));
                    }
                    // Dim inactive panes and frame the focused one so it stands out.
                    if split && id != tab.focused {
                        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(if self.theme.dark { 50 } else { 18 }));
                    } else if split {
                        ui.painter().rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                    }
                }
                tab.rects = rects;
                // Views of panes closed meanwhile.
                let panes = &tab.panes;
                tab.git.retain(|id, _| panes.contains_key(id));

                // A pane dragged by its strip: the one under the pointer lights up, and they swap places
                // on release.
                if let Some(dragged) = self.pane_drag {
                    let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
                    let target = pointer.and_then(|p| tab.rects.iter().find(|(id, r)| *id != dragged && r.contains(p)).map(|(id, r)| (*id, *r)));
                    let layer = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("pane-drag")));
                    if let Some((_, r)) = tab.rects.iter().find(|(id, _)| *id == dragged) {
                        layer.rect_filled(*r, 0.0, Color32::from_black_alpha(60));
                    }
                    if let Some((_, r)) = target {
                        layer.rect_filled(r.shrink(2.0), 6.0, self.theme.accent.gamma_multiply(0.22));
                        layer.rect_stroke(r.shrink(2.0), 6.0, Stroke::new(2.0, self.theme.accent), egui::StrokeKind::Inside);
                        layer.text(r.center(), Align2::CENTER_CENTER, "⇄", FontId::proportional(42.0), self.theme.accent);
                    }
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    if ui.input(|i| i.pointer.any_released() || !i.pointer.any_down()) {
                        if let Some((target, _)) = target {
                            tab.layout.swap(dragged, target);
                            tab.focused = dragged;
                            self.focus_terminal = true;
                        }
                        self.pane_drag = None;
                    }
                }

                if let (Some((_, prefix, items)), Some(term)) = (suggest.as_ref().filter(|(l, _, _)| self.path_dismissed.as_ref() != Some(l)), tab.panes.get_mut(&tab.focused)) {
                    let pos = term.cursor_pos() + Vec2::new(-(prefix.chars().count() as f32) * term.cell_size().x, term.cell_size().y + 2.0);
                    let pick = self.path_pick.1;
                    let theme = &self.theme;
                    egui::Area::new(egui::Id::new("path-suggestions")).order(egui::Order::Foreground).fixed_pos(pos).interactable(true).show(ui.ctx(), |ui| {
                        Frame::popup(ui.style()).fill(theme.chrome_bg).inner_margin(6.0).show(ui, |ui| {
                            for (i, (name, is_dir)) in items.iter().enumerate() {
                                let row = ui.horizontal(|ui| {
                                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                                    files::paint_file_icon(ui.painter(), icon, name, *is_dir, false, None, theme);
                                    let text = egui::RichText::new(if *is_dir { format!("{name}/") } else { name.clone() }).monospace().size(12.5);
                                    ui.add(egui::Button::selectable(i == pick, text))
                                });
                                if row.inner.clicked() {
                                    take_suggestion = Some(i);
                                }
                            }
                            ui.label(egui::RichText::new(strings.path_suggest_hint).size(11.0).color(theme.text_muted));
                        });
                    });
                    if let Some(i) = take_suggestion {
                        let (name, is_dir) = &items[i];
                        // PowerShell's escapes on Windows (a server's shell has the usual ones).
                        let text = if local && cfg!(windows) { complete::completion_windows(name, prefix, *is_dir) } else { complete::completion(name, prefix, *is_dir) };
                        term.type_text(&text);
                        self.focus_terminal = true;
                    }
                }
                if let (Some(_), Some(term)) = (prompt, tab.panes.get(&tab.focused)) {
                    let pos = term.cursor_pos() + Vec2::new(10.0, -3.0);
                    let hint = if cfg!(target_os = "macos") { "⌘ ↩" } else { "Ctrl+↩" };
                    egui::Area::new(egui::Id::new("fill-password")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
                        let text = egui::RichText::new(format!("🔑  {}   {hint}", strings.fill_password)).size(13.0).color(self.theme.bg);
                        let button = egui::Button::new(text).fill(self.theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 24.0));
                        if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            fill_password = true;
                        }
                    });
                }
                if let (true, Some(id)) = (fill_password, prompt) {
                    match (ssh::load_password(id), tab.panes.get_mut(&tab.focused)) {
                        (Some(password), Some(term)) => {
                            term.type_text(&format!("{password}\r"));
                            self.focus_terminal = true;
                        }
                        (None, _) => self.error = Some(format!("{} : {}", strings.keychain_failed, strings.password_missing)),
                        _ => {}
                    }
                }

                // A closed SSH connection: Enter reconnects, Escape closes the pane. Escape also gives up
                // on a connection that is still being established.
                let waiting = !local && tab.panes.get(&tab.focused).is_some_and(|t| !t.has_output() && !t.has_exited());
                if waiting && ui.input(|i| i.key_pressed(Key::Escape)) {
                    close_dead = Some(tab.focused);
                } else if tab.dead.contains(&tab.focused) {
                    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                    if enter {
                        reconnect = Some(vec![tab.focused]);
                    } else if escape {
                        close_dead = Some(tab.focused);
                    }
                }

                match pane_action {
                    Some(PaneAction::Copy(id)) => {
                        if let Some(text) = tab.panes.get(&id).and_then(|t| t.selection_text()) {
                            ui.ctx().copy_text(text);
                        }
                        self.focus_terminal = true;
                    }
                    Some(PaneAction::Paste(id)) => {
                        let text = arboard::Clipboard::new().and_then(|mut c| c.get_text());
                        match (text, tab.panes.get_mut(&id)) {
                            (Ok(text), Some(term)) => term.paste_text(&text),
                            (Err(e), _) => self.error = Some(format!("{} : {e}", self.t().clipboard)),
                            _ => {}
                        }
                        self.focus_terminal = true;
                    }
                    Some(PaneAction::Split(id, side)) => {
                        tab.focused = id;
                        self.split(ui.ctx(), self.active, side);
                    }
                    Some(PaneAction::Close(id)) => self.request_close(CloseRequest::Pane(self.active, id)),
                    Some(PaneAction::Reconnect(id)) => reconnect = Some(vec![id]),
                    Some(PaneAction::Rename(id)) => {
                        tab.focused = id;
                        self.pane_rename = Some((id, tab.names.get(&id).cloned().unwrap_or_default(), true));
                    }
                    Some(PaneAction::Files(id)) => {
                        tab.focused = id;
                        open_files = true;
                    }
                    Some(PaneAction::Insert(id, command)) => {
                        if let Some(term) = tab.panes.get_mut(&id) {
                            term.paste_text(&command);
                        }
                        self.focus_terminal = true;
                    }
                    Some(PaneAction::Commands(id)) => open_commands = Some(id),
                    Some(PaneAction::Startup(id)) => {
                        self.startup_edit = Some((self.viewport, self.active, id, tab.startup.get(&id).cloned().unwrap_or_default()));
                    }
                    Some(PaneAction::Relaunch(id)) => relaunch = Some(id),
                    Some(PaneAction::Reopen(id, k)) => reopen = Some((id, k)),
                    Some(PaneAction::Reveal(id)) => {
                        if let Some(dir) = tab.panes.get(&id).and_then(Terminal::cwd) {
                            config::open_folder(&dir);
                        }
                    }
                    None => {}
                }
                if let Some(id) = close_dead {
                    self.close_pane(self.active, id);
                }
                if let Some(id) = open_search {
                    self.open_search(self.active, id, true);
                }
                if open_files {
                    self.toggle_files(self.active, true);
                }
                if let Some(id) = open_files_window {
                    self.files_window(self.active, id);
                }
                if let Some(id) = open_commands {
                    // One popup at a time: the ⚡ menu replaces the search.
                    self.close_search();
                    self.commands_menu = if self.commands_menu.as_ref().is_some_and(|m| m.pane == id) { None } else { Some(CommandsMenu::new(self.active, id)) };
                }
                if let Some(panes) = reconnect {
                    self.reconnect(ui.ctx(), self.active, &panes);
                }
                if let Some(id) = relaunch {
                    self.relaunch(ui.ctx(), self.active, id);
                }
                if let Some((id, k)) = reopen {
                    self.reopen_pane(ui.ctx(), self.active, id, k);
                }
            });

        if dialogs_here {
            self.settings_window(ui.ctx());
            self.host_editor_window(ui.ctx());
            self.db_editor_window(ui.ctx());
            self.profile_editor_window(ui.ctx());
        }

        // macOS menu bar.
        #[cfg(target_os = "macos")]
        let mut new_window = false;
        #[cfg(target_os = "macos")]
        if let Some(menu) = self.menu.as_mut().filter(|_| main_window) {
            menu.sync(self.config.settings.language.strings(), &self.config.settings.shortcuts.open_settings, &self.config.settings.shortcuts.new_window);
            for action in menu.actions() {
                match action {
                    crate::menu::MenuAction::Settings => self.settings_dialog = true,
                    crate::menu::MenuAction::Reload => self.request_close(CloseRequest::Restart),
                    crate::menu::MenuAction::Quit => self.request_close(CloseRequest::Window),
                    crate::menu::MenuAction::NewWindow => new_window = true,
                }
            }
        }
        #[cfg(target_os = "macos")]
        if new_window {
            let ctx = ui.ctx().clone();
            self.new_window(|app| app.new_tab(&ctx));
        }

        // Closing the window (red button, Cmd+Q...) asks first when programs are still running. Another
        // window than the main one closes by itself, with its tabs.
        if ui.input(|i| i.viewport().close_requested()) && !self.close_confirmed {
            let busy = self.busy_for(CloseRequest::Window);
            if !busy.is_empty() {
                ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                self.confirm_close = Some(ConfirmClose { request: CloseRequest::Window, busy, dont_ask: false });
            } else if !main_window {
                self.window_closing = true;
            }
        }
        self.history_search_ui(ui.ctx());
        self.commands_menu_ui(ui.ctx());
        self.confirm_close_window(ui.ctx());
        self.paste_confirm_window(ui.ctx());
        self.guard_window(ui.ctx());
        self.startup_window(ui.ctx());
        if dialogs_here {
            self.confirm_reset_window(ui.ctx());
            self.link_confirm_window(ui.ctx());
            self.ssh_prompt_window(ui.ctx());
        }

        self.toasts_ui(ui.ctx());

        // Startup splash, over everything; a click or a key skips it.
        if let Some(start) = self.splash.filter(|_| main_window) {
            let now = ui.input(|i| i.time);
            let start = if start.is_nan() { now } else { start };
            self.splash = Some(start);
            let skip = ui.input(|i| i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("splash")));
            if skip || !paint_splash(&painter, ui.ctx().content_rect(), &self.theme, (now - start) as f32, self.t()) {
                self.splash = None;
            }
            ui.ctx().request_repaint();
        }

        // Keep the window title in sync (visible in the OS task switcher).
        {
            let title = self.tabs.get(self.active).map_or_else(|| APP_TITLE.to_owned(), |t| format!("{} — {APP_TITLE}", t.title()));
            if title != self.window_title {
                ui.ctx().send_viewport_cmd(ViewportCommand::Title(title.clone()));
                self.window_title = title;
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Debug builds don't look for updates (they would replace themselves with a release), unless asked.
        // Only published builds update themselves: a local build would replace itself with the release.
        let updates_on = config::OFFICIAL || std::env::var_os("RONNIE_UPDATE_CHECK").is_some();
        if updates_on && self.config.settings.auto_update {
            let now = ui.input(|i| i.time);
            if self.last_update_check.is_none_or(|last| now - last >= UPDATE_INTERVAL) {
                self.last_update_check = Some(now);
                self.updater.check(ui.ctx());
            }
            ui.ctx().request_repaint_after(Duration::from_secs_f64(UPDATE_INTERVAL));
        }
        self.window_ui(ui);
        self.other_windows(ui.ctx());
        self.tool_windows_ui(ui.ctx());

        // Errors shown to the user also go to ronnie.log, to diagnose them later.
        if self.error != self.logged_error {
            if let Some(e) = &self.error {
                crate::log::error(e);
            }
            self.logged_error = self.error.clone();
        }
        let now = ui.input(|i| i.time);
        if now - self.last_sync >= SYNC_INTERVAL {
            self.last_sync = now;
            self.sync();
        }
        // Also saved now and then, so that a crash or a power cut loses little.
        if now - self.last_scrollback_save >= 60.0 {
            self.last_scrollback_save = now;
            self.save_scrollbacks(None, false);
        }
    }

    fn on_exit(&mut self) {
        self.save_scrollbacks(None, false);
        self.sync();
        crate::askpass::cleanup();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }
}
