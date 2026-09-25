use egui::{
    Align2, Color32, FontId, Frame, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke, Ui,
    Vec2, ViewportCommand,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use uuid::Uuid;

use crate::config::{self, Config, Layout, Profile, Session, SessionTab, TabState, WindowState};
use crate::i18n::{Lang, Strings};
use crate::pane::{self, Direction, Node, PaneId};
use crate::terminal::{FontSet, LocalUrl, Terminal};
use crate::ssh::{self, SshHost};
use crate::theme::{Preset, Theme, PRESETS, TAB_COLORS};
use crate::update::{self, Updater};

const SIDEBAR_WIDTH: f32 = 220.0;
/// Window title: dev builds are told apart from the installed app.
const APP_TITLE: &str = if config::OFFICIAL { "Ronnie" } else { "Ronnie (dev)" };
const SIDEBAR_PAD: f32 = 8.0;
/// Space above the first section: room for the macOS traffic lights (the title bar is merged into the sidebar).
const SIDEBAR_TOP: f32 = if cfg!(target_os = "macos") { 40.0 } else { 10.0 };
/// Band under the traffic lights holding the app name.
const LOGO_H: f32 = 46.0;
const SECTION_HEADER_H: f32 = 26.0;
const SECTION_GAP: f32 = 12.0;
const ROW_H: f32 = 30.0;
const ROW_GAP: f32 = 2.0;
/// Height of a group title in the profiles section.
const GROUP_H: f32 = 24.0;
/// Bottom strip of the sidebar holding the settings button.
const FOOTER_H: f32 = 40.0;
/// How often open tabs are compared with what is on disk.
/// Height of the strip above each local pane showing its working directory.
const PANE_HEADER_H: f32 = 22.0;
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
    /// SSH panes whose connection ended: kept on screen with their last output until reconnected or closed.
    dead: std::collections::HashSet<PaneId>,
    /// Panes whose shell starts the first time the tab is shown: restoring many tabs at once
    /// would otherwise start all their shells together and saturate the CPU.
    pending: Vec<PaneId>,
}

impl Tab {
    fn new(layout: Node, panes: HashMap<PaneId, Terminal>) -> Self {
        let focused = layout.first_leaf();
        Self { name: None, color: None, panes, layout, focused, rects: Vec::new(), profile: None, ssh: None, cwds: HashMap::new(), histories: HashMap::new(), dead: Default::default(), pending: Vec::new() }
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
            Node::Leaf(id) => Layout::Pane { cwd: self.cwds.get(id).cloned(), history: self.histories.get(id).copied() },
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

pub struct App {
    tabs: Vec<Tab>,
    active: usize,
    theme: Theme,
    fonts: FontSet,
    rename: Option<Rename>,
    focus_terminal: bool,
    error: Option<String>,
    window_title: String,
    next_pane: PaneId,
    /// While a tab is dragged: pointer x minus the tab's left edge.
    tab_grab: Option<f32>,

    config: Config,
    /// Recently closed tabs, most recent last.
    closed: Vec<SessionTab>,
    /// What was last written to disk, to skip identical writes.
    saved_config: Config,
    saved_session: Session,
    /// False while config.json can't be read (invalid JSON...): never overwrite the user's file then.
    config_writable: bool,
    /// Modification time of config.json when last read or written, to notice outside edits.
    config_mtime: Option<std::time::SystemTime>,
    last_sync: f64,
    window: Option<WindowState>,
    settings_dialog: bool,
    settings_tab: SettingsTab,
    /// Text of the config file editor while it is shown.
    editor: Option<ConfigEditor>,
    /// SSH host being created or edited.
    host_editor: Option<HostEditor>,
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
    /// A clicked link waiting for "Open this link?" confirmation.
    link_confirm: Option<String>,
    /// "Reset everything?" dialog shown.
    confirm_reset: bool,
    /// Close waiting for the user to confirm, because programs are running.
    confirm_close: Option<ConfirmClose>,
    /// The window may close without asking again (confirmed, or restarting).
    close_confirmed: bool,
    /// App time when the startup splash began; None once it is over.
    splash: Option<f64>,
    ctx: egui::Context,
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
}

impl CommandsMenu {
    fn new(tab: usize, pane: PaneId) -> Self {
        Self { tab, pane, new_command: String::new(), for_tab: true }
    }
}

/// Search box over a pane's command history.
struct HistorySearch {
    tab: usize,
    pane: PaneId,
    query: String,
    /// The pane's commands, most recent first.
    entries: Vec<String>,
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
            .filter(|e| {
                let e = e.to_lowercase();
                words.iter().all(|w| e.contains(w))
            })
            .map(String::as_str)
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
    password: String,
    /// The password field was edited: save (or forget, if emptied) it on save.
    password_changed: bool,
    /// Password shown in clear (the saved one is loaded from the keychain for that).
    reveal: bool,
    /// Sidebar group to put the host in (None: outside groups).
    group: Option<Uuid>,
    is_new: bool,
    error: Option<String>,
}

impl HostEditor {
    fn new(draft: SshHost, is_new: bool, group: Option<Uuid>) -> Self {
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
    Profiles,
    Ssh,
    ConfigFile,
    Shortcuts,
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
}

impl ShortcutAction {
    const ALL: [ShortcutAction; 9] = [
        Self::NewTab,
        Self::ClosePane,
        Self::SplitRight,
        Self::SplitDown,
        Self::FindText,
        Self::FindCommands,
        Self::ReopenTab,
        Self::ClearPane,
        Self::OpenSettings,
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
    pub fn new(cc: &eframe::CreationContext<'_>, session: Session, config: anyhow::Result<Config>, theme: Theme) -> Self {
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            theme,
            fonts: FontSet { size: 14.0, line_height: 1.2 },
            rename: None,
            focus_terminal: true,
            error: None,
            window_title: String::new(),
            next_pane: 1,
            tab_grab: None,
            config: Config::default(),
            closed: Vec::new(),
            saved_config: Config::default(),
            saved_session: Session::default(),
            config_writable: true,
            config_mtime: config::config_path().and_then(|p| config::modified(&p)),
            last_sync: 0.0,
            window: None,
            settings_dialog: false,
            settings_tab: SettingsTab::General,
            editor: None,
            host_editor: None,
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
            read_only: false,
            confirm_reset: false,
            link_confirm: None,
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
            }
            Err(e) => {
                app.config_writable = false;
                app.error = Some(format!("{} : {e:#}", app.t().config_not_loaded));
            }
        }
        let known = |c: &&SessionTab| c.profile.is_some_and(|id| app.config.profiles.iter().any(|p| p.id == id));
        app.closed = session.closed.iter().filter(known).cloned().collect();
        app.window = session.window;
        for tab in &session.tabs {
            // A profile tab starts from its profile: it may have been edited in config.json since.
            let profile = tab.profile.and_then(|id| app.config.profiles.iter().find(|p| p.id == id));
            let state = profile.map_or_else(|| tab.tab.clone(), |p| p.tab.clone());
            app.open_tab(&state, tab.profile);
            // SSH tabs reconnect when first shown, like other restored tabs start their shell.
            if let (Some(id), Some(last)) = (tab.ssh, app.tabs.last_mut()) {
                last.ssh = app.config.ssh.iter().any(|h| h.id == id).then_some(id);
            }
        }
        app.active = session.active.min(app.tabs.len().saturating_sub(1));
        app.saved_session = session;
        if app.tabs.is_empty() {
            app.new_tab(&cc.egui_ctx);
        }
        if app.read_only {
            app.config_writable = false;
        } else {
            app.forget_unused_histories();
        }
        #[cfg(target_os = "macos")]
        {
            app.menu = crate::menu::MenuBar::install(&cc.egui_ctx, app.t(), &app.config.settings.shortcuts.open_settings);
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
        let layout = self.build(&state.layout, &mut cwds, &mut histories);
        let mut tab = Tab::new(layout, HashMap::new());
        tab.pending = tab.layout.leaves();
        tab.cwds = cwds;
        tab.histories = histories;
        if let Some(id) = tab.layout.leaves().get(state.focused) {
            tab.focused = *id;
        }
        tab.name = state.name.clone();
        tab.color = state.color;
        tab.profile = profile.filter(|id| self.config.profiles.iter().any(|p| p.id == *id));
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.focus_terminal = true;
    }

    /// Turns a saved layout into a tree with fresh pane ids, collecting each pane's directory.
    fn build(&mut self, layout: &Layout, cwds: &mut HashMap<PaneId, PathBuf>, histories: &mut HashMap<PaneId, Uuid>) -> Node {
        match layout {
            Layout::Pane { cwd, history } => {
                let id = self.next_pane;
                self.next_pane += 1;
                if let Some(cwd) = cwd {
                    cwds.insert(id, cwd.clone());
                }
                if let Some(history) = history {
                    histories.insert(id, *history);
                }
                Node::Leaf(id)
            }
            Layout::Split { axis, ratio, a, b } => Node::Split {
                axis: *axis,
                ratio: ratio.clamp(0.1, 0.9),
                a: Box::new(self.build(a, cwds, histories)),
                b: Box::new(self.build(b, cwds, histories)),
            },
        }
    }

    /// Scope of tab `index`'s own commands: its profile or SSH host, if any.
    fn tab_scope(&self, index: usize) -> Option<(CommandScope, String)> {
        let tab = self.tabs.get(index)?;
        if let Some(host) = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)) {
            return Some((CommandScope::Host(host.id), host.name.clone()));
        }
        let profile = tab.profile.and_then(|id| self.config.profiles.iter().find(|p| p.id == id))?;
        Some((CommandScope::Profile(profile.id), profile.name(self.t().untitled).to_owned()))
    }

    fn commands_mut(&mut self, scope: CommandScope) -> Option<&mut Vec<String>> {
        match scope {
            CommandScope::General => Some(&mut self.config.commands),
            CommandScope::Profile(id) => self.config.profiles.iter_mut().find(|p| p.id == id).map(|p| &mut p.commands),
            CommandScope::Host(id) => self.config.ssh.iter_mut().find(|h| h.id == id).map(|h| &mut h.commands),
        }
    }

    fn commands_of(&self, scope: CommandScope) -> Vec<String> {
        match scope {
            CommandScope::General => self.config.commands.clone(),
            CommandScope::Profile(id) => self.config.profiles.iter().find(|p| p.id == id).map(|p| p.commands.clone()).unwrap_or_default(),
            CommandScope::Host(id) => self.config.ssh.iter().find(|h| h.id == id).map(|h| h.commands.clone()).unwrap_or_default(),
        }
    }

    /// Commands offered in tab `index`: its own first, then the general ones.
    fn saved_commands(&self, index: usize) -> Vec<String> {
        let mut all = self.tab_scope(index).map(|(scope, _)| self.commands_of(scope)).unwrap_or_default();
        for c in &self.config.commands {
            if !all.contains(c) {
                all.push(c.clone());
            }
        }
        all
    }

    /// The ⚡ menu, under the pane's header on the right: click a command to write it at the prompt
    /// (it isn't run); ✕ removes it; the field at the bottom adds one.
    fn commands_menu_ui(&mut self, ctx: &egui::Context) {
        let Some(menu) = &self.commands_menu else { return };
        let (index, pane) = (menu.tab, menu.pane);
        let Some(pane_rect) = self.tabs.get(index).filter(|_| index == self.active).and_then(|t| t.rects.iter().find(|(id, _)| *id == pane)).map(|(_, r)| *r) else {
            self.commands_menu = None;
            return;
        };
        let t = self.t();
        let tab_scope = self.tab_scope(index);
        let sections: Vec<(CommandScope, String, Vec<String>)> = tab_scope
            .iter()
            .map(|(scope, name)| (*scope, name.clone(), self.commands_of(*scope)))
            .chain(std::iter::once((CommandScope::General, t.commands_general.to_owned(), self.config.commands.clone())))
            .collect();
        let width = 360.0_f32.min(pane_rect.width() - 16.0);
        let pos = Pos2::new(pane_rect.max.x - width - 8.0, pane_rect.min.y + PANE_HEADER_H + 6.0);
        let theme = self.theme.clone();

        let mut insert = None;
        let mut remove = None;
        let mut add = None;
        let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let menu = self.commands_menu.as_mut().unwrap();
        let area = egui::Area::new(egui::Id::new("commands-menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.accent.gamma_multiply(0.6))).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(width - 20.0);
                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (scope, name, commands) in &sections {
                        ui.label(egui::RichText::new(name.to_uppercase()).size(11.0).strong().color(theme.text_muted));
                        if commands.is_empty() {
                            ui.label(egui::RichText::new(t.commands_empty).size(12.5).color(theme.text_muted.gamma_multiply(0.7)));
                        }
                        for (i, command) in commands.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let x = ui.add(egui::Button::new(egui::RichText::new("✕").size(11.0).color(theme.text_muted)).frame(false));
                                if x.clicked() {
                                    remove = Some((*scope, i));
                                }
                                let label = egui::RichText::new(command.replace('\n', " ⏎ ")).monospace().size(12.5).color(theme.text);
                                let row = ui.add(egui::Button::new(label).frame_when_inactive(false).truncate());
                                if row.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    insert = Some(command.clone());
                                }
                            });
                        }
                        ui.add_space(6.0);
                    }
                });
                ui.separator();
                let edit = ui.add(egui::TextEdit::singleline(&mut menu.new_command).hint_text(t.commands_new).font(FontId::monospace(12.5)).desired_width(f32::INFINITY));
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    if let Some((_, name)) = &tab_scope {
                        ui.selectable_value(&mut menu.for_tab, true, egui::RichText::new(name).size(12.5));
                        ui.selectable_value(&mut menu.for_tab, false, egui::RichText::new(t.commands_general).size(12.5));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button = egui::Button::new(egui::RichText::new(t.commands_add).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(5.0);
                        if (ui.add_enabled(!menu.new_command.trim().is_empty(), button).clicked() || enter) && !menu.new_command.trim().is_empty() {
                            let scope = match (&tab_scope, menu.for_tab) {
                                (Some((scope, _)), true) => *scope,
                                _ => CommandScope::General,
                            };
                            add = Some((scope, menu.new_command.trim().to_owned()));
                            menu.new_command.clear();
                        }
                    });
                });
                ui.label(egui::RichText::new(t.commands_hint).size(11.0).color(theme.text_muted));
            });
        });
        // A click elsewhere closes it (except on the pane header, where the ⚡ button toggles it). Tested
        // on the rect: `contains_pointer` is false over the menu's own widgets.
        let inside = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p));
        let clicked_outside = ctx.input(|i| i.pointer.any_click()) && !inside;

        if let Some((scope, i)) = remove {
            if let Some(list) = self.commands_mut(scope) {
                if i < list.len() {
                    list.remove(i);
                }
            }
        }
        if let Some((scope, command)) = add {
            if let Some(list) = self.commands_mut(scope) {
                if !list.contains(&command) {
                    list.push(command);
                }
            }
        }
        if let Some(command) = insert {
            if let Some(term) = self.tabs.get_mut(index).and_then(|t| t.panes.get_mut(&pane)) {
                term.paste_text(&command);
            }
            self.commands_menu = None;
            self.focus_terminal = true;
        } else if escape {
            self.commands_menu = None;
            self.focus_terminal = true;
        } else if clicked_outside && remove.is_none() {
            let over_header = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.y < pane_rect.min.y + PANE_HEADER_H && pane_rect.contains(p));
            if !over_header {
                self.commands_menu = None;
            }
        }
    }

    /// Opens the search box over a pane, on the displayed text (`text`) or on the commands typed there
    /// (local panes only). The same shortcut again closes it; the other one switches mode.
    fn open_search(&mut self, index: usize, pane: PaneId, text: bool) {
        // One popup at a time: the search replaces the ⚡ menu.
        self.commands_menu = None;
        if let Some(search) = self.history_search.as_mut().filter(|s| s.tab == index && s.pane == pane) {
            if search.text == text {
                self.close_search();
            } else {
                search.text = text;
                search.fresh = true;
            }
            return;
        }
        self.close_search();
        let Some(tab) = self.tabs.get_mut(index) else { return };
        let local = tab.ssh.is_none();
        let entries = if local { tab.history_path(pane).map(|p| crate::shell::read_history(&p)).unwrap_or_default() } else { Vec::new() };
        self.history_search = Some(HistorySearch { tab: index, pane, query: String::new(), entries, selected: 0, fresh: true, text: text || !local, local, status: (0, 0) });
    }

    fn close_search(&mut self) {
        if let Some(search) = self.history_search.take() {
            if let Some(term) = self.tabs.get_mut(search.tab).and_then(|t| t.panes.get_mut(&search.pane)) {
                term.clear_find();
            }
            self.focus_terminal = true;
        }
    }

    /// The search box, at the top of its pane. Text mode highlights the occurrences in the output
    /// (Enter: older one, Shift+Enter: newer one). Commands mode lists the commands typed in the pane:
    /// Enter pastes one at the prompt, Cmd+Enter runs it.
    fn history_search_ui(&mut self, ctx: &egui::Context) {
        let Some(search) = &mut self.history_search else { return };
        let Some(pane_rect) = self.tabs.get(search.tab).filter(|_| search.tab == self.active).and_then(|t| t.rects.iter().find(|(id, _)| *id == search.pane)).map(|(_, r)| *r) else {
            self.history_search = None;
            return;
        };
        let (index, pane) = (search.tab, search.pane);
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        let shortcuts = self.config.settings.shortcuts.clone();
        let width = (pane_rect.width() - 32.0).clamp(200.0, 620.0);
        let pos = Pos2::new(pane_rect.center().x - width / 2.0, pane_rect.min.y + PANE_HEADER_H + 10.0);

        let (up, down, newer, enter, run, escape) = ctx.input_mut(|i| {
            let run = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter));
            let newer = i.consume_key(Modifiers::SHIFT, Key::Enter);
            (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), newer, i.consume_key(Modifiers::NONE, Key::Enter), run, i.consume_key(Modifiers::NONE, Key::Escape))
        });
        let count = search.matches().len();
        if !search.text {
            if up {
                search.selected = search.selected.saturating_sub(1);
            }
            if down && count > 0 {
                search.selected = (search.selected + 1).min(count - 1);
            }
        }
        let mut chosen: Option<(String, bool)> = None;
        let mut find: Option<String> = None;
        let mut step: Option<bool> = None; // Some(true): older occurrence
        let mut close = escape;
        let was_text = search.text;

        egui::Area::new(egui::Id::new("history-search")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.accent.gamma_multiply(0.6))).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.horizontal(|ui| {
                    let text_label = format!("{}  {}", t.search_text, shortcuts.find_text.label());
                    if ui.selectable_label(search.text, egui::RichText::new(text_label).size(12.5)).clicked() {
                        search.text = true;
                    }
                    if search.local {
                        let commands_label = format!("{}  {}", t.search_commands, shortcuts.find_commands.label());
                        if ui.selectable_label(!search.text, egui::RichText::new(commands_label).size(12.5)).clicked() {
                            search.text = false;
                        }
                    }
                });
                if search.text != was_text {
                    search.fresh = true;
                    // Switching to text mode searches what was typed; leaving it clears the highlights.
                    find = Some(if search.text { search.query.clone() } else { String::new() });
                }
                ui.add_space(4.0);
                let hint = if search.text { t.search_text_hint } else { t.history_search };
                let edit = ui.add(egui::TextEdit::singleline(&mut search.query).hint_text(format!("🔍  {hint}")).font(FontId::monospace(13.0)).desired_width(f32::INFINITY));
                if search.fresh || !edit.has_focus() {
                    edit.request_focus();
                    search.fresh = false;
                }
                if edit.changed() {
                    search.selected = 0;
                    if search.text {
                        find = Some(search.query.clone());
                    }
                }
                ui.add_space(6.0);

                if search.text {
                    ui.horizontal(|ui| {
                        let (current, total) = search.status;
                        let status = if search.query.is_empty() { String::new() } else if total == 0 { t.search_none.to_owned() } else { format!("{current} / {total}") };
                        ui.label(egui::RichText::new(status).size(12.5).color(theme.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(total > 0, egui::Button::new("↓")).on_hover_text("⇧ ↩").clicked() {
                                step = Some(false);
                            }
                            if ui.add_enabled(total > 0, egui::Button::new("↑")).on_hover_text("↩").clicked() {
                                step = Some(true);
                            }
                        });
                    });
                    if enter || up {
                        step = Some(true);
                    }
                    if newer || down {
                        step = Some(false);
                    }
                    ui.label(egui::RichText::new(t.search_text_keys).size(11.0).color(theme.text_muted));
                    return;
                }

                let matches = search.matches();
                if matches.is_empty() {
                    ui.label(egui::RichText::new(t.history_empty).size(12.5).color(theme.text_muted));
                }
                egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, command) in matches.iter().enumerate() {
                        let selected = i == search.selected;
                        let one_line = command.replace('\n', " ⏎ ");
                        let text = egui::RichText::new(one_line).monospace().size(12.5).color(if selected { theme.text } else { theme.text_muted });
                        // Left-aligned in a row of its own height (a full-width button would center the command).
                        let row = ui.horizontal(|ui| ui.add(egui::Button::selectable(selected, text).truncate())).inner;
                        if selected && (up || down) {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            chosen = Some((command.to_string(), false));
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.history_search_hint).size(11.0).color(theme.text_muted));
                if (enter || run) && chosen.is_none() {
                    if let Some(command) = matches.get(search.selected) {
                        chosen = Some((command.to_string(), run));
                    }
                }
            });
        });

        if let Some(term) = self.tabs.get_mut(index).and_then(|t| t.panes.get_mut(&pane)) {
            let mut status = None;
            if let Some(query) = find {
                status = Some(if query.is_empty() {
                    term.clear_find();
                    (0, 0)
                } else {
                    term.find(&query)
                });
            }
            if let Some(older) = step {
                term.find_step(older);
                status = Some(term.find_status());
            }
            if let Some((command, run)) = &chosen {
                term.paste_text(command);
                if *run {
                    term.type_text("\r");
                }
                close = true;
            }
            if let (Some(status), Some(search)) = (status, self.history_search.as_mut()) {
                search.status = status;
            }
        }
        if close {
            self.close_search();
        }
    }

    /// Deletes the command histories of terminals that were closed for good: neither open, nor in a
    /// profile or the recently closed tabs.
    fn forget_unused_histories(&self) {
        let mut keep: Vec<Uuid> = self.tabs.iter().flat_map(|t| t.histories.values().copied()).collect();
        for p in &self.config.profiles {
            p.tab.layout.histories(&mut keep);
        }
        for c in &self.closed {
            c.tab.layout.histories(&mut keep);
        }
        crate::shell::forget_others(&keep.into_iter().collect());
    }

    /// What a new pane of this tab runs: an ssh session for SSH tabs, the user's shell otherwise.
    fn launch_for(&self, index: usize) -> Option<crate::ssh::Launch> {
        let id = self.tabs.get(index)?.ssh?;
        Some(self.config.ssh.iter().find(|h| h.id == id)?.command())
    }

    /// Starts the shells of a tab's panes that have not run yet.
    fn start_pending(&mut self, ctx: &egui::Context, index: usize) {
        let launch = self.launch_for(index);
        let Some(tab) = self.tabs.get_mut(index) else { return };
        for id in std::mem::take(&mut tab.pending) {
            let history = tab.history_path(id);
            match Terminal::local(ctx, tab.cwds.get(&id).map(PathBuf::as_path), launch.as_ref(), history.as_deref()) {
                Ok(term) => {
                    tab.panes.insert(id, term);
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
    }

    /// Restarts panes of a tab (a new ssh session replaces the old or ended one).
    fn reconnect(&mut self, ctx: &egui::Context, index: usize, panes: &[PaneId]) {
        let launch = self.launch_for(index);
        let Some(tab) = self.tabs.get_mut(index) else { return };
        for &id in panes {
            let history = tab.history_path(id);
            match Terminal::local(ctx, None, launch.as_ref(), history.as_deref()) {
                Ok(term) => {
                    tab.panes.insert(id, term);
                    tab.dead.remove(&id);
                    tab.pending.retain(|p| *p != id);
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        self.focus_terminal = true;
    }

    /// Connects to an SSH host, or switches to its tab if it is already open.
    fn open_ssh(&mut self, id: Uuid) {
        if let Some(index) = self.tabs.iter().position(|t| t.ssh == Some(id)) {
            self.select(index);
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
        } else if let Some(p) = self.config.profiles.iter().find(|p| p.id == id) {
            let state = p.tab.clone();
            self.open_tab(&state, Some(id));
        }
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

    /// Pushes open tabs into their profiles and writes what changed to disk.
    fn sync(&mut self) {
        let mut tabs = Vec::with_capacity(self.tabs.len());
        for tab in &mut self.tabs {
            // An SSH tab shows its host's name and color.
            if let Some(host) = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)) {
                tab.name = Some(host.name.clone());
                tab.color = host.color;
            }
            let state = tab.state();
            if let Some(id) = tab.profile {
                match self.config.profiles.iter_mut().find(|p| p.id == id) {
                    Some(p) => p.tab = state.clone(),
                    None => tab.profile = None,
                }
            }
            tabs.push(SessionTab { profile: tab.profile, ssh: tab.ssh, tab: state });
        }
        let session = Session { tabs, active: self.active, closed: self.closed.clone(), window: self.window };

        if self.read_only {
            return;
        }
        if session != self.saved_session {
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

    fn save_config(&mut self) {
        if !self.config_writable || self.config == self.saved_config {
            return;
        }
        let Some(path) = config::config_path() else { return };
        match config::save(&path, &self.config) {
            Ok(()) => {
                self.saved_config = self.config.clone();
                self.config_mtime = config::modified(&path);
            }
            Err(e) => self.error = Some(format!("{} : {e:#}", self.t().config_save_failed)),
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
            Ok(config) => {
                self.config_writable = true;
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

    fn spawn(&mut self, ctx: &egui::Context, cwd: Option<&Path>, launch: Option<&crate::ssh::Launch>, history: Uuid) -> Option<(PaneId, Terminal)> {
        match Terminal::local(ctx, cwd, launch, crate::shell::history_path(history).as_deref()) {
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
        if let Some((id, term)) = self.spawn(ctx, None, None, history) {
            let mut tab = Tab::new(Node::Leaf(id), HashMap::from([(id, term)]));
            tab.histories.insert(id, history);
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
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
        let launch = self.launch_for(index);
        let history = Uuid::new_v4();
        if let Some((id, term)) = self.spawn(ctx, cwd.as_deref(), launch.as_ref(), history) {
            let tab = &mut self.tabs[index];
            tab.layout.split(tab.focused, id, side);
            tab.panes.insert(id, term);
            tab.histories.insert(id, history);
            tab.focused = id;
            self.focus_terminal = true;
        }
    }

    fn close_pane(&mut self, index: usize, pane: PaneId) {
        let Some(tab) = self.tabs.get_mut(index) else { return };
        if !tab.remove_pane(pane) {
            self.close_tab(index);
        } else if index == self.active {
            self.focus_terminal = true;
        }
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
        let mut tab = self.tabs.remove(index);
        let state = tab.state();
        if let Some(p) = self.config.profiles.iter_mut().find(|p| Some(p.id) == tab.profile) {
            p.tab = state.clone();
        }
        // Only profiles are worth reopening; a plain terminal is just a new tab.
        if tab.profile.is_some() {
            self.closed.retain(|c| c.profile != tab.profile);
            self.closed.push(SessionTab { profile: tab.profile, ssh: None, tab: state });
            if self.closed.len() > config::MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        if self.rename.as_ref().is_some_and(|r| r.tab == index) {
            self.rename = None;
        }
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.focus_terminal = true;
    }

    fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
            self.focus_terminal = true;
        }
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
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (i, key) in digits.into_iter().enumerate() {
            if ui.input_mut(|inp| inp.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key))) {
                // Cmd+9 always goes to the last tab, like browsers.
                self.select(if i == 8 { self.tabs.len().saturating_sub(1) } else { i });
            }
        }
    }

    /// Shown when every tab is closed: the app stays open.
    fn empty_state(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let mut open = None;
        let mut new = false;
        let width = 260.0;
        let area = Rect::from_center_size(rect.center(), Vec2::new(width, rect.height().min(420.0)));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new(t.no_tabs).size(18.0).color(self.theme.text));
            ui.add_space(12.0);
            let hint = self.config.settings.shortcuts.new_tab.label();
            if ui.add(egui::Button::new(t.new_terminal).shortcut_text(hint).min_size(Vec2::new(width, 32.0))).clicked() {
                new = true;
            }
            if !self.config.profiles.is_empty() {
                ui.add_space(18.0);
                ui.label(egui::RichText::new(t.profiles).size(11.0).color(self.theme.text_muted));
                ui.add_space(4.0);
                for p in &self.config.profiles {
                    let (dot, name) = (p.tab.color.unwrap_or(self.theme.text_muted), p.name(t.untitled));
                    let resp = ui.add(egui::Button::new(format!("     {name}")).min_size(Vec2::new(width, 30.0)).truncate());
                    ui.painter().circle_filled(Pos2::new(resp.rect.min.x + 14.0, resp.rect.center().y), 4.0, dot);
                    if resp.clicked() {
                        open = Some(p.id);
                    }
                }
            }
        });
        if new {
            self.new_tab(ui.ctx());
        }
        if let Some(id) = open {
            self.open_profile(id);
        }
    }

    /// A profile or an SSH host, as shown in the sidebar.
    fn item(&self, id: Uuid) -> Option<Item> {
        let t = self.t();
        if let Some(p) = self.config.profiles.iter().find(|p| p.id == id) {
            let panes = p.tab.layout.panes();
            return Some(Item {
                name: p.name(t.untitled).to_owned(),
                color: p.tab.color,
                ssh: false,
                open: self.tabs.iter().position(|t| t.profile == Some(id)),
                hint: format!("{panes} {}", t.layout_panes),
            });
        }
        let host = self.config.ssh.iter().find(|h| h.id == id)?;
        Some(Item {
            name: host.name.clone(),
            color: host.color,
            ssh: true,
            open: self.tabs.iter().position(|t| t.ssh == Some(id)),
            hint: host.address(),
        })
    }

    /// Profiles (`local`, listed under the LOCAL terminals) or SSH hosts (with their own header), in
    /// user-defined, collapsible groups.
    fn profiles_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>, local: bool) {
        let t = self.t();
        let painter = ui.painter().clone();
        // The dragged row is painted above the others.
        let drag_painter = painter.clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("item-drag")));

        // SSH header: title and a "+" menu.
        if !local {
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        painter.text(
            Pos2::new(header.min.x + 6.0, header.center().y),
            Align2::LEFT_CENTER,
            t.profiles,
            FontId::proportional(11.0),
            self.theme.text_muted.gamma_multiply(0.8),
        );
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        let plus = icon_button(ui, &painter, plus_rect, "profiles-plus", &self.theme, paint_plus);
        egui::Popup::menu(&plus).width(210.0).show(|ui| {
            menu_item(ui, t.new_host, TabAction::NewHost, action);
            menu_item(ui, t.new_group, TabAction::NewGroup(false), action);
        });
        *y += SECTION_HEADER_H;
        }

        if !local && self.config.ssh.is_empty() && !self.config.groups.iter().any(|g| !g.local) {
            let hint = painter.layout(t.no_profiles.to_owned(), FontId::proportional(12.0), self.theme.text_muted.gamma_multiply(0.7), row_w - 12.0);
            let h = hint.size().y;
            painter.galley(Pos2::new(left + 6.0, *y + 4.0), hint, self.theme.text_muted);
            *y += h + 12.0;
            return;
        }

        let of_kind = |id: &&Uuid| if local { self.config.profiles.iter().any(|p| p.id == **id) } else { self.config.ssh.iter().any(|h| h.id == **id) };
        let mut rows: Vec<Row> = self.config.ungrouped.iter().filter(of_kind).map(|id| Row::Item(None, *id)).collect();
        for (gi, g) in self.config.groups.iter().enumerate().filter(|(_, g)| g.local == local) {
            rows.push(Row::Group(gi));
            if !g.collapsed {
                rows.extend(g.items.iter().filter(of_kind).map(|id| Row::Item(Some(gi), *id)));
            }
        }
        let (pointer, pointer_down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));
        let mut rects: Vec<(Row, Rect)> = Vec::with_capacity(rows.len());

        for row in rows {
            let h = if matches!(row, Row::Group(_)) { GROUP_H } else { ROW_H };
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, h));
            *y += h + ROW_GAP;
            rects.push((row, slot));
            let dragged_here = self.item_drag.as_ref().is_some_and(|d| pointer_down && d.is(row, &self.config));
            // A row being dragged follows the pointer; its slot stays empty.
            let (rect, painter) = match (dragged_here, pointer, &self.item_drag) {
                (true, Some(p), Some(d)) => (slot.translate(Vec2::new(0.0, p.y - d.grab - slot.min.y)), &drag_painter),
                _ => (slot, &painter),
            };
            match row {
                Row::Group(gi) => self.group_row(ui, painter, gi, slot, rect, dragged_here, action),
                Row::Item(_, id) => self.item_row(ui, painter, id, slot, rect, dragged_here, action),
            }
        }

        // Drop target of a drag in progress, shown as a line (or a highlighted group); applied on release.
        if let (Some(drag), Some(p)) = (&self.item_drag, pointer) {
            let (target, mark) = drop_target(drag, p, &rects, &self.config);
            if pointer_down {
                match mark {
                    Mark::Line(y) => {
                        painter.hline(left + 4.0..=left + row_w - 4.0, y, Stroke::new(2.0, self.theme.accent));
                    }
                    Mark::Into(r) => {
                        painter.rect_stroke(r, 6.0, Stroke::new(1.5, self.theme.accent), egui::StrokeKind::Inside);
                    }
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else {
                *action = Some(target);
                self.item_drag = None;
            }
        }
    }

    fn group_row(&mut self, ui: &mut Ui, painter: &egui::Painter, gi: usize, slot: Rect, rect: Rect, dragged: bool, action: &mut Option<TabAction>) {
        let t = self.t();
        let group = &self.config.groups[gi];
        let (gid, collapsed, group_local) = (group.id, group.collapsed, group.local);
        let resp = ui.interact(slot, ui.id().with(("group", gid)), Sense::click_and_drag());
        if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                self.item_drag = Some(ItemDrag { what: Dragged::Group(gid), grab: p.y - slot.min.y });
            }
        }
        let hovered = resp.contains_pointer() || dragged;
        if hovered {
            painter.rect_filled(rect, 6.0, self.theme.tab_hover);
        }
        let color = if hovered { self.theme.text } else { self.theme.text_muted };
        paint_chevron(painter, Pos2::new(rect.min.x + 12.0, rect.center().y), !collapsed, color);
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y), Pos2::new(rect.max.x - 30.0, rect.max.y));

        if let Some(rename) = self.group_rename.as_mut().filter(|r| r.0 == gid) {
            let edit = ui.put(text_rect.shrink2(Vec2::new(0.0, 3.0)), egui::TextEdit::singleline(&mut rename.1).font(FontId::proportional(12.5)).frame(Frame::NONE));
            if rename.2 {
                edit.request_focus();
                select_all(ui, edit.id, &rename.1);
                rename.2 = false;
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.group_rename = None;
            } else if enter || edit.lost_focus() {
                *action = Some(TabAction::RenameGroup(gid, rename.1.trim().to_owned()));
            }
            return;
        }

        let name = egui::RichText::new(&group.name).size(12.5).strong().color(color);
        let mut job = egui::text::LayoutJob::simple_singleline(name.text().to_owned(), FontId::proportional(12.5), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
        let count = group.items.iter().filter(|id| if group_local { self.config.profiles.iter().any(|p| p.id == **id) } else { self.config.ssh.iter().any(|h| h.id == **id) }).count();
        if collapsed || hovered {
            painter.text(
                Pos2::new(rect.max.x - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                count.to_string(),
                FontId::proportional(11.0),
                self.theme.text_muted.gamma_multiply(0.8),
            );
        }
        if resp.double_clicked() {
            *action = Some(TabAction::StartGroupRename(gid));
        } else if resp.clicked() {
            *action = Some(TabAction::ToggleGroup(gid));
        }
        resp.context_menu(|ui| {
            ui.set_min_width(170.0);
            menu_item(ui, t.rename, TabAction::StartGroupRename(gid), action);
            menu_item(ui, t.new_group, TabAction::NewGroup(group_local), action);
            ui.separator();
            menu_item(ui, t.delete_group, TabAction::DeleteGroup(gid), action);
        });
    }

    fn item_row(&mut self, ui: &mut Ui, painter: &egui::Painter, id: Uuid, slot: Rect, rect: Rect, dragged: bool, action: &mut Option<TabAction>) {
        let t = self.t();
        let Some(item) = self.item(id) else { return };
        let resp = ui.interact(slot, ui.id().with(("item", id)), Sense::click_and_drag());
        if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                self.item_drag = Some(ItemDrag { what: Dragged::Item(id), grab: p.y - slot.min.y });
            }
        }
        let hovered = resp.contains_pointer() && self.item_drag.is_none();
        let active = item.open == Some(self.active);
        let fill = if active || dragged {
            self.theme.tab_active
        } else if hovered {
            self.theme.tab_hover
        } else {
            self.theme.tab_bg
        };
        painter.rect_filled(rect, 6.0, fill);
        if active {
            paint_active_bar(painter, rect, self.theme.accent);
        }
        let dot = Pos2::new(rect.min.x + 14.0, rect.center().y);
        let live = item.open.and_then(|i| self.live.get(i).cloned().flatten());
        if live.is_some() {
            paint_live(ui, painter, dot, &self.theme);
        }
        match item.color {
            Some(color) => painter.circle_filled(dot, 4.0, color),
            None => painter.circle_stroke(dot, 3.5, Stroke::new(1.0, self.theme.text_muted.gamma_multiply(0.6))),
        };

        let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
        let show_close = item.open.is_some() && (active || hovered);
        let edit_rect = if show_close { close_rect.translate(Vec2::new(-20.0, 0.0)) } else { close_rect };
        let show_edit = hovered && item.ssh;
        let text_right = if show_edit {
            edit_rect.min.x - 4.0
        } else if show_close {
            close_rect.min.x - 4.0
        } else {
            rect.max.x - 34.0
        };
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + 28.0, rect.min.y), Pos2::new(text_right, rect.max.y));

        if let Some(rename) = self.item_rename.as_mut().filter(|r| r.id == id) {
            let color = if rename.error.is_some() { self.theme.ansi[1] } else { self.theme.text };
            let edit = ui.put(text_rect.shrink2(Vec2::new(0.0, 5.0)), egui::TextEdit::singleline(&mut rename.text).font(FontId::proportional(13.0)).frame(Frame::NONE).text_color(color));
            if rename.fresh {
                edit.request_focus();
                select_all(ui, edit.id, &rename.text);
                rename.fresh = false;
            }
            if edit.changed() {
                rename.error = None;
            }
            if let Some(err) = rename.error {
                edit.request_focus();
                painter.rect_stroke(rect, 6.0, Stroke::new(1.0, self.theme.ansi[1]), egui::StrokeKind::Inside);
                egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), edit.id.with("err"), egui::PopupAnchor::Position(rect.left_bottom() + Vec2::new(0.0, 4.0)))
                    .show(|ui| ui.label(egui::RichText::new(err).color(self.theme.ansi[1])));
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.item_rename = None;
            } else if enter {
                *action = Some(TabAction::RenameItem(id, rename.text.clone(), true));
            } else if edit.lost_focus() {
                *action = Some(TabAction::RenameItem(id, rename.text.clone(), false));
            }
            return;
        }

        let color = if active || (item.open.is_some() && hovered) {
            self.theme.text
        } else if item.open.is_some() || hovered {
            self.theme.text_muted
        } else {
            self.theme.text_muted.gamma_multiply(0.75)
        };
        let mut job = egui::text::LayoutJob::simple_singleline(item.name.clone(), FontId::proportional(13.0), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
        if show_close {
            let close = ui.interact(close_rect, ui.id().with(("item-close", id)), Sense::click());
            if close.hovered() {
                painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
            }
            paint_cross(painter, close_rect.center(), if close.hovered() { self.theme.text } else { self.theme.text_muted });
            if close.clicked() {
                *action = item.open.map(TabAction::Close);
            }
        }
        if show_edit {
            let edit = ui.interact(edit_rect, ui.id().with(("item-edit", id)), Sense::click());
            if edit.hovered() {
                painter.rect_filled(edit_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
            }
            paint_pencil(painter, edit_rect.center(), if edit.hovered() { self.theme.text } else { self.theme.text_muted });
            if edit.on_hover_text(t.edit).clicked() {
                *action = Some(TabAction::EditHost(id));
            }
        }

        let resp = match &live {
            Some(program) => resp.on_hover_text(format!("{}\n▶ {program}", item.hint)),
            None => resp.on_hover_text(&item.hint),
        };
        if resp.double_clicked() {
            *action = Some(if item.ssh { TabAction::EditHost(id) } else { TabAction::StartItemRename(id) });
        } else if resp.clicked() {
            action.get_or_insert(TabAction::OpenItem(id));
        }
        if resp.middle_clicked() {
            if let Some(i) = item.open {
                *action = Some(TabAction::Close(i));
            }
        }
        resp.context_menu(|ui| self.item_menu(ui, id, &item, action));
    }

    /// Right-click menu of a profile or an SSH host.
    fn item_menu(&self, ui: &mut Ui, id: Uuid, item: &Item, action: &mut Option<TabAction>) {
        let t = self.t();
        ui.set_min_width(180.0);
        menu_item(ui, if item.ssh { t.connect } else { t.open }, TabAction::OpenItem(id), action);
        if item.ssh {
            menu_item(ui, t.edit, TabAction::EditHost(id), action);
        } else {
            menu_item(ui, t.edit, TabAction::EditProfile(id), action);
            menu_item(ui, t.rename, TabAction::StartItemRename(id), action);
        }
        if let (Some(i), true) = (item.open, item.ssh) {
            menu_item(ui, t.reconnect, TabAction::Reconnect(i), action);
        }
        {
            ui.menu_button(t.move_to, |ui| {
                let current = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
                if ui.add_enabled(current.is_some(), egui::Button::new(t.no_group)).clicked() {
                    *action = Some(TabAction::DropItem(id, None, None));
                    ui.close();
                }
                // Profiles go into local groups, hosts into SSH groups.
                for g in self.config.groups.iter().filter(|g| g.local != item.ssh) {
                    if ui.add_enabled(current != Some(g.id), egui::Button::new(&g.name)).clicked() {
                        *action = Some(TabAction::DropItem(id, Some(g.id), None));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t.new_group).clicked() {
                    *action = Some(TabAction::NewGroupWith(id));
                    ui.close();
                }
            });
        }
        ui.separator();
        ui.label(egui::RichText::new(t.color).size(12.0).strong().color(self.theme.text_muted));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for color in TAB_COLORS {
                let (r, s) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                ui.painter().circle_filled(r.center(), 7.0, color);
                if item.color == Some(color) || s.hovered() {
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, self.theme.text));
                }
                if s.clicked() {
                    *action = Some(TabAction::ItemColor(id, Some(color)));
                    ui.close();
                }
            }
        });
        if item.color.is_some() && ui.button(t.remove_color).clicked() {
            *action = Some(TabAction::ItemColor(id, None));
            ui.close();
        }
        ui.separator();
        if let Some(i) = item.open {
            menu_item(ui, t.close, TabAction::Close(i), action);
        }
        menu_item(ui, t.delete, if item.ssh { TabAction::DeleteHost(id) } else { TabAction::DeleteProfile(id) }, action);
    }

    fn delete_profile(&mut self, id: Uuid) {
        self.config.profiles.retain(|p| p.id != id);
        self.closed.retain(|c| c.profile != Some(id));
        for tab in self.tabs.iter_mut().filter(|t| t.profile == Some(id)) {
            tab.profile = None;
        }
    }

    /// Renames a profile (and its open tab). Refused when empty or used by another profile.
    fn rename_profile(&mut self, id: Uuid, name: &str) -> Result<(), &'static str> {
        let name = name.trim();
        if name.is_empty() {
            return Err(self.t().host_required);
        }
        if self.config.profiles.iter().any(|p| p.id != id && p.tab.name.as_deref() == Some(name)) {
            return Err(self.t().name_taken);
        }
        if let Some(p) = self.config.profiles.iter_mut().find(|p| p.id == id) {
            p.tab.name = Some(name.to_owned());
        }
        for tab in self.tabs.iter_mut().filter(|t| t.profile == Some(id)) {
            tab.name = Some(name.to_owned());
        }
        Ok(())
    }

    fn set_profile_color(&mut self, id: Uuid, color: Option<Color32>) {
        if let Some(p) = self.config.profiles.iter_mut().find(|p| p.id == id) {
            p.tab.color = color;
        }
        for tab in self.tabs.iter_mut().filter(|t| t.profile == Some(id)) {
            tab.color = color;
        }
    }

    /// Settings page listing the profiles: rename, recolor, open or delete them.
    /// Profiles (`local`) or SSH hosts as in the sidebar: each group in order, then those without a
    /// group. The section name is None when there are no groups at all (a plain list).
    fn grouped_ids(&self, local: bool) -> Vec<(Option<String>, Vec<Uuid>)> {
        let of_kind = |id: &Uuid| if local { self.config.profiles.iter().any(|p| p.id == *id) } else { self.config.ssh.iter().any(|h| h.id == *id) };
        let mut sections: Vec<(Option<String>, Vec<Uuid>)> = self
            .config
            .groups
            .iter()
            .filter(|g| g.local == local)
            .map(|g| (Some(g.name.clone()), g.items.iter().copied().filter(of_kind).collect::<Vec<_>>()))
            .filter(|(_, ids)| !ids.is_empty())
            .collect();
        let loose: Vec<Uuid> = self.config.ungrouped.iter().copied().filter(of_kind).collect();
        if !loose.is_empty() {
            let name = (!sections.is_empty()).then(|| self.t().no_group.to_owned());
            sections.push((name, loose));
        }
        sections
    }

    fn profiles_ui(&mut self, ui: &mut Ui, t: &Strings) {
        if self.config.profiles.is_empty() {
            ui.label(egui::RichText::new(t.no_profiles).size(13.0).color(self.theme.text_muted));
            return;
        }
        let (mut rename, mut recolor, mut open, mut delete) = (None, None, None, None);
        let sections = self.grouped_ids(true);
        egui::ScrollArea::vertical().max_height(ui.available_height()).show(ui, |ui| {
            for (name, ids) in &sections {
                if let Some(name) = name {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(name.to_uppercase()).size(12.0).strong().color(self.theme.text_muted));
                }
            for p in ids.iter().filter_map(|id| self.config.profiles.iter().find(|p| p.id == *id)) {
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    let dot = egui::RichText::new("●").size(18.0).color(p.tab.color.unwrap_or(self.theme.text_muted));
                    ui.menu_button(dot, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for color in TAB_COLORS {
                                let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                                ui.painter().circle_filled(r.center(), 7.5, color);
                                if p.tab.color == Some(color) || s.hovered() {
                                    ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, self.theme.text));
                                }
                                if s.clicked() {
                                    recolor = Some((p.id, Some(color)));
                                    ui.close();
                                }
                            }
                        });
                        if p.tab.color.is_some() && ui.button(t.remove_color).clicked() {
                            recolor = Some((p.id, None));
                            ui.close();
                        }
                    });
                    let current = p.name(t.untitled).to_owned();
                    let buffer = self.profile_names.entry(p.id).or_insert_with(|| current.clone());
                    let edit = ui.add(egui::TextEdit::singleline(buffer).desired_width(250.0).margin(Vec2::new(6.0, 5.0)).font(FontId::proportional(14.0)));
                    if edit.lost_focus() && *buffer != current {
                        rename = Some((p.id, buffer.clone()));
                    }
                    let panes = p.tab.layout.panes();
                    ui.label(egui::RichText::new(format!("{panes} {}", t.layout_panes)).size(12.0).color(self.theme.text_muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                            delete = Some(p.id);
                        }
                        if ui.button(t.open).clicked() {
                            open = Some(p.id);
                        }
                    });
                });
            }
            }
        });
        if let Some(err) = self.profile_error {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(err).size(13.0).color(self.theme.ansi[1]));
        }
        if let Some((id, name)) = rename {
            match self.rename_profile(id, &name) {
                Ok(()) => self.profile_error = None,
                Err(err) => {
                    // Put the current name back in the field.
                    self.profile_names.remove(&id);
                    self.profile_error = Some(err);
                }
            }
        }
        if let Some((id, color)) = recolor {
            self.set_profile_color(id, color);
        }
        if let Some(id) = delete {
            self.delete_profile(id);
            self.profile_names.remove(&id);
        }
        if let Some(id) = open {
            self.open_profile(id);
            self.settings_dialog = false;
        }
    }

    fn delete_host(&mut self, id: Uuid) {
        self.config.ssh.retain(|h| h.id != id);
        ssh::delete_password(id);
        // Open sessions keep running as plain tabs.
        for tab in self.tabs.iter_mut().filter(|t| t.ssh == Some(id)) {
            tab.ssh = None;
        }
    }

    /// Hosts of ~/.ssh/config that Ronnie doesn't have yet.
    fn new_ssh_config_hosts(&self) -> std::io::Result<(usize, Vec<SshHost>)> {
        let hosts = ssh::import_ssh_config()?;
        let total = hosts.len();
        let new = hosts.into_iter().filter(|host| !self.config.ssh.iter().any(|h| h.name == host.name && h.host == host.host)).collect();
        Ok((total, new))
    }

    /// Adds the hosts of ~/.ssh/config that Ronnie doesn't have yet (in their groups).
    fn import_ssh(&mut self) {
        match self.new_ssh_config_hosts() {
            Ok((_, hosts)) => {
                self.config.ssh.extend(hosts);
                self.config.normalize();
            }
            Err(e) => self.error = Some(format!("{} : {e}", self.t().ssh_import_failed)),
        }
    }

    /// Settings page for SSH: import from ~/.ssh/config (explained first) and the list of hosts.
    fn ssh_settings_ui(&mut self, ui: &mut Ui, t: &Strings) {
        let muted = self.theme.text_muted;
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).size(12.0).strong().color(muted));
            ui.add_space(4.0);
        };
        heading(ui, t.import_title);
        Frame::new().fill(self.theme.bg).corner_radius(6.0).inner_margin(12.0).stroke(Stroke::new(1.0, self.theme.tab_hover)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(t.import_explain).size(13.0));
            ui.add_space(8.0);
            match self.new_ssh_config_hosts() {
                Err(_) => {
                    ui.label(egui::RichText::new(t.no_ssh_config).size(13.0).color(muted));
                }
                Ok((total, new)) => {
                    ui.horizontal(|ui| {
                        let summary = t.import_found.replace("{total}", &total.to_string()).replace("{new}", &new.len().to_string());
                        ui.label(egui::RichText::new(summary).size(13.0).color(muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = t.import_button.replace("{new}", &new.len().to_string());
                            let button = egui::Button::new(egui::RichText::new(label).size(14.0)).min_size(Vec2::new(0.0, 30.0));
                            if ui.add_enabled(!new.is_empty(), button).clicked() {
                                self.import_ssh();
                            }
                        });
                    });
                }
            }
        });
        ui.add_space(16.0);

        heading(ui, t.ssh_hosts);
        if self.config.ssh.is_empty() {
            ui.label(egui::RichText::new(t.no_ssh_hosts).size(13.0).color(muted));
            return;
        }
        let (mut edit, mut delete) = (None, None);
        let sections = self.grouped_ids(false);
        egui::ScrollArea::vertical().max_height(ui.available_height()).show(ui, |ui| {
            for (name, ids) in &sections {
                if let Some(name) = name {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(name.to_uppercase()).size(12.0).strong().color(muted));
                }
            for host in ids.iter().filter_map(|id| self.config.ssh.iter().find(|h| h.id == *id)) {
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    ui.label(egui::RichText::new("●").size(16.0).color(host.color.unwrap_or(muted)));
                    ui.label(egui::RichText::new(&host.name).size(14.0));
                    ui.label(egui::RichText::new(host.address()).size(12.0).color(muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                            delete = Some(host.id);
                        }
                        if ui.button(t.edit).clicked() {
                            edit = Some(host.id);
                        }
                    });
                });
            }
            }
        });
        let imported = self.config.ssh.iter().filter(|h| h.imported).count();
        if imported > 0 {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = format!("{} ({imported})", t.delete_imported);
                if self.confirm_delete_imported {
                    ui.label(egui::RichText::new(t.delete_imported_confirm).size(13.0).color(self.theme.ansi[1]));
                    if ui.button(egui::RichText::new(t.confirm).color(self.theme.ansi[1])).clicked() {
                        let ids: Vec<Uuid> = self.config.ssh.iter().filter(|h| h.imported).map(|h| h.id).collect();
                        for id in ids {
                            self.delete_host(id);
                        }
                        self.confirm_delete_imported = false;
                    }
                    if ui.button(t.cancel).clicked() {
                        self.confirm_delete_imported = false;
                    }
                } else if ui.button(egui::RichText::new(label).color(self.theme.ansi[1])).clicked() {
                    self.confirm_delete_imported = true;
                }
            });
        }
        if let Some(id) = delete {
            self.delete_host(id);
        }
        if let Some(id) = edit {
            if let Some(host) = self.config.ssh.iter().find(|h| h.id == id) {
                let group = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
                self.host_editor = Some(HostEditor::new(host.clone(), false, group));
            }
        }
    }

    /// Dialog to create or edit an SSH host.
    fn profile_editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.profile_editor else { return };
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        let mut result: Option<bool> = None; // Some(true) = save, Some(false) = cancel
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("profile-editor")).frame(frame).backdrop_color(Color32::from_black_alpha(170)).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(egui::RichText::new(t.edit_profile).size(18.0).strong());
            ui.add_space(14.0);
            let label = |ui: &mut Ui, text: &str| ui.label(egui::RichText::new(text).size(12.0).strong().color(theme.text_muted));

            label(ui, &t.host_name.to_uppercase());
            let name = ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)));
            if name.changed() {
                editor.error = None;
            }
            if let Some(err) = editor.error {
                ui.label(egui::RichText::new(err).size(12.5).color(theme.ansi[1]));
            }
            ui.add_space(12.0);

            label(ui, t.color);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for color in TAB_COLORS {
                    let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 8.0, color);
                    if editor.color == Some(color) || s.hovered() {
                        ui.painter().circle_stroke(r.center(), 9.5, Stroke::new(1.5, theme.text));
                    }
                    if s.clicked() {
                        editor.color = Some(color);
                    }
                }
                if editor.color.is_some() && ui.small_button(t.remove_color).clicked() {
                    editor.color = None;
                }
            });
            ui.add_space(12.0);

            label(ui, &t.profile_panes.to_uppercase());
            egui::Grid::new("profile-panes").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (i, cwd) in editor.cwds.iter_mut().enumerate() {
                    ui.label(egui::RichText::new(t.pane_n.replace("{n}", &(i + 1).to_string())).size(13.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(cwd).hint_text("~").font(FontId::monospace(12.5)).desired_width(370.0).margin(Vec2::new(6.0, 4.0)));
                    ui.end_row();
                }
            });
            ui.add_space(12.0);

            label(ui, &t.commands.to_uppercase());
            let mut remove = None;
            for (i, command) in editor.commands.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(command).font(FontId::monospace(12.5)).desired_width(420.0).margin(Vec2::new(6.0, 4.0)));
                    if ui.add(egui::Button::new(egui::RichText::new("✕").color(theme.text_muted)).frame(false)).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                editor.commands.remove(i);
            }
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut editor.new_command).hint_text(t.commands_new).font(FontId::monospace(12.5)).desired_width(360.0).margin(Vec2::new(6.0, 4.0)));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (ui.button(t.commands_add).clicked() || enter) && !editor.new_command.trim().is_empty() {
                    editor.commands.push(editor.new_command.trim().to_owned());
                    editor.new_command.clear();
                }
            });

            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = egui::Button::new(egui::RichText::new(t.save).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0));
                if ui.add(save).clicked() {
                    result = Some(true);
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                    result = Some(false);
                }
            });
        });
        if modal.should_close() {
            result = Some(false);
        }
        match result {
            Some(true) => self.save_profile_editor(),
            Some(false) => self.profile_editor = None,
            None => {}
        }
    }

    /// Applies the profile editor: to the profile, and to its tab when open.
    fn save_profile_editor(&mut self) {
        let Some(editor) = self.profile_editor.take() else { return };
        if let Err(err) = self.rename_profile(editor.id, &editor.name) {
            self.profile_editor = Some(ProfileEditor { error: Some(err), ..editor });
            return;
        }
        self.set_profile_color(editor.id, editor.color);
        let cwds: Vec<Option<PathBuf>> = editor.cwds.iter().map(|c| c.trim()).map(|c| (!c.is_empty()).then(|| ssh::expand_home(Path::new(c)))).collect();
        if let Some(p) = self.config.profiles.iter_mut().find(|p| p.id == editor.id) {
            p.tab.layout.set_cwds(&mut cwds.clone().into_iter());
            p.commands = editor.commands.iter().map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()).collect();
        }
        // An open tab would write its own directories back into the profile: update them too (they apply
        // to shells started from now on).
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.profile == Some(editor.id)) {
            for (leaf, cwd) in tab.layout.leaves().into_iter().zip(cwds) {
                match cwd {
                    Some(cwd) => tab.cwds.insert(leaf, cwd),
                    None => tab.cwds.remove(&leaf),
                };
            }
        }
        self.focus_terminal = true;
    }

    fn host_editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.host_editor else { return };
        let t = self.config.settings.language.strings();
        let theme = &self.theme;
        let groups: Vec<(Uuid, String)> = self.config.groups.iter().map(|g| (g.id, g.name.clone())).collect();
        let mut result: Option<bool> = None; // Some(true) = save, Some(false) = cancel
        let mut delete = false;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("host-editor")).frame(frame).show(ctx, |ui| {
            ui.set_width(440.0);
            let title = if editor.is_new { t.new_host } else { t.edit_host };
            ui.label(egui::RichText::new(title).size(18.0).strong());
            ui.add_space(14.0);
            let field_w = 300.0;
            let d = &mut editor.draft;
            egui::Grid::new("host-fields").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                let label = |ui: &mut Ui, text: &str| ui.label(egui::RichText::new(text).size(13.0).color(theme.text_muted));
                let text_field = |ui: &mut Ui, value: &mut String, hint: &str| {
                    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(field_w).margin(Vec2::new(6.0, 5.0)))
                };

                label(ui, t.host_address);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut d.host).hint_text("exemple.com").desired_width(field_w - 80.0).margin(Vec2::new(6.0, 5.0)));
                    ui.add(egui::TextEdit::singleline(&mut editor.port).hint_text("22").desired_width(66.0).margin(Vec2::new(6.0, 5.0)));
                });
                ui.end_row();

                label(ui, t.host_name);
                text_field(ui, &mut d.name, &d.host.clone());
                ui.end_row();

                label(ui, t.user);
                let mut user = d.user.clone().unwrap_or_default();
                text_field(ui, &mut user, "root");
                d.user = Some(user.trim().to_owned()).filter(|u| !u.is_empty());
                ui.end_row();

                label(ui, t.key);
                ui.vertical(|ui| {
                    let current = match &d.identity_file {
                        None => t.key_default.to_owned(),
                        Some(k) if editor.keys.contains(k) => ssh::display_path(k),
                        Some(_) => t.key_other.to_owned(),
                    };
                    egui::ComboBox::from_id_salt("host-key").selected_text(current).width(field_w).show_ui(ui, |ui| {
                        if ui.selectable_label(d.identity_file.is_none(), t.key_default).clicked() {
                            d.identity_file = None;
                        }
                        for key in &editor.keys {
                            if ui.selectable_label(d.identity_file.as_ref() == Some(key), ssh::display_path(key)).clicked() {
                                d.identity_file = Some(key.clone());
                            }
                        }
                        let custom = d.identity_file.as_ref().is_some_and(|k| !editor.keys.contains(k));
                        if ui.selectable_label(custom, t.key_other).clicked() {
                            d.identity_file = Some(PathBuf::from(editor.custom_key.trim()));
                        }
                    });
                    if d.identity_file.as_ref().is_some_and(|k| !editor.keys.contains(k)) {
                        if ui.add(egui::TextEdit::singleline(&mut editor.custom_key).hint_text("~/.ssh/ma_cle").desired_width(field_w).margin(Vec2::new(6.0, 5.0))).changed() {
                            d.identity_file = Some(PathBuf::from(editor.custom_key.trim()));
                        }
                    }
                });
                ui.end_row();

                label(ui, t.password);
                ui.vertical(|ui| {
                    let hint = if d.password_saved && !editor.password_changed { "••••••••" } else { t.optional };
                    ui.horizontal(|ui| {
                        let field = egui::TextEdit::singleline(&mut editor.password)
                            .password(!editor.reveal)
                            .hint_text(hint)
                            .desired_width(field_w - 80.0)
                            .margin(Vec2::new(6.0, 5.0));
                        if ui.add(field).changed() {
                            editor.password_changed = true;
                        }
                        let toggle = if editor.reveal { t.hide_password } else { t.show_password };
                        if ui.add(egui::Button::new(toggle).min_size(Vec2::new(72.0, 24.0))).clicked() {
                            editor.reveal = !editor.reveal;
                            // Show the saved password so it can be checked or corrected.
                            if editor.reveal && d.password_saved && !editor.password_changed && editor.password.is_empty() {
                                editor.password = ssh::load_password(d.id).unwrap_or_default();
                            }
                        }
                    });
                    if d.password_saved && !editor.password_changed {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(t.password_saved).size(12.0).color(theme.text_muted));
                            if ui.small_button(t.forget_password).clicked() {
                                editor.password.clear();
                                editor.password_changed = true;
                            }
                        });
                    }
                });
                ui.end_row();

                label(ui, t.jump);
                let mut jump = d.jump.clone().unwrap_or_default();
                text_field(ui, &mut jump, t.optional);
                d.jump = Some(jump.trim().to_owned()).filter(|j| !j.is_empty());
                ui.end_row();

                label(ui, t.host_group);
                let current = groups.iter().find(|(id, _)| Some(*id) == editor.group).map_or(t.no_group, |(_, name)| name.as_str());
                egui::ComboBox::from_id_salt("host-group").selected_text(current).width(field_w).show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor.group, None, t.no_group);
                    for (id, name) in &groups {
                        ui.selectable_value(&mut editor.group, Some(*id), name);
                    }
                });
                ui.end_row();

                // Menu headings are in capitals; here it's a field label like the others.
                label(ui, &format!("{}{}", &t.color[..1], t.color[1..].to_lowercase()));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for color in TAB_COLORS {
                        let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 7.5, color);
                        if d.color == Some(color) || s.hovered() {
                            ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, theme.text));
                        }
                        if s.clicked() {
                            d.color = if d.color == Some(color) { None } else { Some(color) };
                        }
                    }
                });
                ui.end_row();
            });

            ui.add_space(16.0);
            if let Some(err) = &editor.error {
                ui.label(egui::RichText::new(err).size(13.0).color(theme.ansi[1]));
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                if !editor.is_new && ui.button(egui::RichText::new(t.delete).size(14.0).color(theme.ansi[1])).clicked() {
                    delete = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.save).size(14.0)).min_size(Vec2::new(110.0, 30.0))).clicked() {
                        result = Some(true);
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(14.0)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                        result = Some(false);
                    }
                });
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }

        if delete {
            let id = editor.draft.id;
            self.host_editor = None;
            self.delete_host(id);
            return;
        }
        match result {
            Some(false) => self.host_editor = None,
            Some(true) => self.save_host(),
            None => {}
        }
    }

    /// Validates and stores the host being edited, with its password in the keychain.
    fn save_host(&mut self) {
        let t = self.t();
        let Some(editor) = &mut self.host_editor else { return };
        let mut host = editor.draft.clone();
        host.host = host.host.trim().to_owned();
        if host.host.is_empty() {
            editor.error = Some(t.host_required.to_owned());
            return;
        }
        host.port = match editor.port.trim() {
            "" => None,
            p => match p.parse::<u16>() {
                Ok(p) if p > 0 => Some(p),
                _ => {
                    editor.error = Some(t.invalid_port.to_owned());
                    return;
                }
            },
        };
        host.name = host.name.trim().to_owned();
        if host.name.is_empty() {
            host.name = host.host.clone();
        }
        if host.identity_file.as_ref().is_some_and(|k| k.as_os_str().is_empty()) {
            host.identity_file = None;
        }
        if editor.password_changed {
            if editor.password.is_empty() {
                ssh::delete_password(host.id);
                host.password_saved = false;
            } else {
                match ssh::save_password(host.id, &editor.password) {
                    Ok(()) => host.password_saved = true,
                    Err(e) => {
                        editor.error = Some(format!("{} : {e}", t.keychain_failed));
                        return;
                    }
                }
            }
        }
        let (id, group) = (host.id, editor.group);
        match self.config.ssh.iter_mut().find(|h| h.id == id) {
            Some(existing) => *existing = host,
            None => self.config.ssh.push(host),
        }
        // Move it only if its group changed, so it keeps its place otherwise.
        let current = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
        let placed = current.is_some() || self.config.ungrouped.contains(&id);
        if !placed || current != group {
            self.config.unplace(id);
            match group.and_then(|g| self.config.groups.iter_mut().find(|x| x.id == g)) {
                Some(g) => g.items.push(id),
                None => self.config.ungrouped.push(id),
            }
        }
        self.host_editor = None;
    }

    /// The settings dialog: language and color theme, applied as soon as they are picked.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_dialog {
            return;
        }
        let t = self.t();
        let mut picked = self.config.settings.clone();
        let mut close = false;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("settings")).frame(frame).backdrop_color(Color32::from_black_alpha(190)).show(ctx, |ui| {
            // Same size on every tab: pages scroll inside, the window never jumps around.
            ui.set_width(SETTINGS_WIDTH);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.settings).size(18.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("✕").size(14.0)).frame(false)).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (tab, label) in [(SettingsTab::About, t.about), (SettingsTab::General, t.general), (SettingsTab::Profiles, t.manage_profiles), (SettingsTab::Ssh, t.ssh_tab), (SettingsTab::Shortcuts, t.shortcuts), (SettingsTab::ConfigFile, t.config_file)] {
                    let text = egui::RichText::new(label).size(14.0);
                    if ui.add(egui::Button::selectable(self.settings_tab == tab, text).min_size(Vec2::new(0.0, 28.0))).clicked() {
                        self.settings_tab = tab;
                    }
                }
            });
            ui.separator();
            ui.add_space(8.0);
            let body = Vec2::new(SETTINGS_WIDTH, SETTINGS_BODY_HEIGHT.min(ctx.content_rect().height() - 200.0).max(200.0));
            ui.allocate_ui_with_layout(body, egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(body);
            ui.set_max_height(body.y);
            match self.settings_tab {
                SettingsTab::ConfigFile => return self.config_editor_ui(ui, t),
                SettingsTab::Profiles => return self.profiles_ui(ui, t),
                SettingsTab::Ssh => return self.ssh_settings_ui(ui, t),
                SettingsTab::About => return self.about_ui(ui, ctx, t, &mut picked),
                SettingsTab::Shortcuts => return self.shortcuts_ui(ui, t, &mut picked),
                SettingsTab::General => {}
            }

            let heading_color = self.theme.text_muted;
            let heading = move |ui: &mut Ui, text: &str| {
                ui.label(egui::RichText::new(text).size(12.0).strong().color(heading_color));
                ui.add_space(4.0);
            };
            let max_height = ui.available_height();
            egui::ScrollArea::vertical().max_height(max_height).min_scrolled_height(max_height).show(ui, |ui| {
            heading(ui, &t.language.to_uppercase());
            ui.horizontal(|ui| {
                for lang in Lang::ALL {
                    let label = egui::RichText::new(lang.label()).size(14.0);
                    if ui.add(egui::Button::selectable(picked.language == lang, label).min_size(Vec2::new(110.0, 30.0))).clicked() {
                        picked.language = lang;
                    }
                }
            });
            ui.add_space(18.0);

            heading(ui, &t.display.to_uppercase());
            ui.checkbox(&mut picked.show_cwd, egui::RichText::new(t.show_cwd).size(14.0));
            ui.add_space(18.0);

            heading(ui, &t.theme.to_uppercase());
            for (dark, label) in [(true, t.dark), (false, t.light)] {
                ui.label(egui::RichText::new(label).size(13.0).color(self.theme.text_muted));
                let presets: Vec<&Preset> = PRESETS.iter().filter(|p| p.dark == dark).collect();
                for pair in presets.chunks(3) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for preset in pair {
                            if theme_card(ui, preset, picked.theme == preset.id, self.theme.text).clicked() {
                                picked.theme = preset.id.to_owned();
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
            }
            ui.add_space(12.0);

            heading(ui, &t.reset_section.to_uppercase());
            let reset = egui::Button::new(egui::RichText::new(t.reset_button).size(13.5).color(self.theme.ansi[1]))
                .stroke(Stroke::new(1.0, self.theme.ansi[1].gamma_multiply(0.7)))
                .fill(Color32::TRANSPARENT)
                .corner_radius(6.0)
                .min_size(Vec2::new(0.0, 30.0));
            if ui.add(reset).clicked() {
                self.confirm_reset = true;
            }
            ui.add_space(8.0);
            });
            });
        });
        if self.settings_tab != SettingsTab::Shortcuts {
            self.shortcut_capture = None;
        }
        if close || modal.should_close() {
            self.settings_dialog = false;
            self.shortcut_capture = None;
            self.editor = None;
            self.profile_names.clear();
            self.profile_error = None;
            self.focus_terminal = true;
        }
        if picked != self.config.settings {
            let mut config = self.config.clone();
            config.settings = picked;
            self.apply_config(config);
            self.save_config();
        }
    }

    /// "Shortcuts" settings page: each action's shortcut can be recorded (click, then type the
    /// combination); a few fixed ones are listed below.
    fn shortcuts_ui(&mut self, ui: &mut Ui, t: &Strings, picked: &mut config::Settings) {
        let muted = self.theme.text_muted;
        // Recording: the next key pressed with a modifier becomes the shortcut; Escape cancels.
        if let Some(action) = self.shortcut_capture {
            let typed = ui.input_mut(|i| {
                let found = i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                });
                if found.is_some() {
                    i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
                }
                found
            });
            match typed {
                Some((Key::Escape, _)) => self.shortcut_capture = None,
                Some((key, m)) if m.command || m.ctrl || m.alt || m.mac_cmd => {
                    *action.get_mut(&mut picked.shortcuts) = config::Shortcut::typed(m, key);
                    self.shortcut_capture = None;
                }
                _ => {}
            }
        }

        let defaults = config::Shortcuts::default();
        let parsed: Vec<Option<KeyboardShortcut>> = ShortcutAction::ALL.iter().map(|a| a.get(&picked.shortcuts).parse()).collect();
        egui::ScrollArea::vertical().max_height(ui.available_height()).show(ui, |ui| {
            egui::Grid::new("shortcuts-grid").num_columns(3).spacing([16.0, 8.0]).show(ui, |ui| {
                for (i, action) in ShortcutAction::ALL.into_iter().enumerate() {
                    let current = action.get(&picked.shortcuts).clone();
                    let default = action.get(&defaults).clone();
                    ui.label(egui::RichText::new(action.label(t)).size(14.0));
                    let recording = self.shortcut_capture == Some(action);
                    // The same combination on two actions: only one would work.
                    let conflict = parsed[i].is_some() && parsed.iter().enumerate().any(|(j, p)| j != i && *p == parsed[i]);
                    let text = if recording { t.shortcut_press.to_owned() } else { current.label() };
                    let color = if recording { self.theme.accent } else if conflict { self.theme.ansi[1] } else { self.theme.text };
                    let button = egui::Button::new(egui::RichText::new(text).size(13.5).monospace().color(color)).corner_radius(6.0).min_size(Vec2::new(150.0, 26.0));
                    let resp = ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let resp = if conflict { resp.on_hover_text(t.shortcut_conflict) } else { resp };
                    if resp.clicked() {
                        self.shortcut_capture = if recording { None } else { Some(action) };
                    }
                    if current != default {
                        if ui.button(t.shortcut_reset).clicked() {
                            *action.get_mut(&mut picked.shortcuts) = default;
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });

            ui.add_space(22.0);
            ui.label(egui::RichText::new(t.shortcut_fixed.to_uppercase()).size(12.0).strong().color(muted));
            ui.add_space(6.0);
            let mac = cfg!(target_os = "macos");
            let fixed = [
                (t.copy, if mac { "⌘ C" } else { "Ctrl+Shift+C" }),
                (t.paste, if mac { "⌘ V" } else { "Ctrl+Shift+V" }),
                (t.shortcut_move_pane, if mac { "⌘ ← ↑ → ↓" } else { "Ctrl+Alt+← ↑ → ↓" }),
                (t.shortcut_clear_line, if mac { "⌘ ⌫" } else { "Ctrl+U" }),
            ];
            egui::Grid::new("fixed-shortcuts").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                for (label, keys) in fixed {
                    ui.label(egui::RichText::new(label).size(13.5).color(muted));
                    ui.label(egui::RichText::new(keys).size(13.5).monospace());
                    ui.end_row();
                }
            });
        });
    }

    /// "Informations" settings page: logo, version and updates, link to the project.
    fn about_ui(&mut self, ui: &mut Ui, ctx: &egui::Context, t: &Strings, picked: &mut config::Settings) {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            let (logo, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::hover());
            paint_metal(ui.painter(), logo.center(), Align2::CENTER_CENTER, "Ronnie", 64.0, self.theme.accent, 1.0);
            ui.add_space(4.0);
            // Tagline and the sign of the horns (an image: egui's emoji font lacks it).
            let hand = self.metal_hand.get_or_insert_with(|| load_png(ctx, "metal-hand", include_bytes!("../assets/icon/metal-hand.png")));
            let galley = ui.painter().layout_no_wrap(t.tagline.to_owned(), FontId::proportional(14.0), self.theme.text_muted);
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            let left = row.center().x - (galley.size().x + 26.0) / 2.0;
            let text_pos = Pos2::new(left, row.center().y - galley.size().y / 2.0);
            let hand_rect = Rect::from_center_size(Pos2::new(left + galley.size().x + 15.0, row.center().y), Vec2::splat(20.0));
            ui.painter().galley(text_pos, galley, self.theme.text_muted);
            egui::Image::new(&*hand).paint_at(ui, hand_rect);
            ui.add_space(22.0);
        });
        ui.separator();
        ui.add_space(14.0);
        ui.label(egui::RichText::new(t.updates.to_uppercase()).size(12.0).strong().color(self.theme.text_muted));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.version.replace("{v}", update::VERSION)).size(14.0));
            ui.add_space(12.0);
            if ui.add_enabled(!self.updater.busy(), egui::Button::new(egui::RichText::new(t.check_now).size(13.0))).clicked() {
                self.update_dismissed = false;
                self.updater.check(ctx);
            }
            let muted_color = self.theme.text_muted;
            let muted = move |s: &str| egui::RichText::new(s.to_owned()).size(13.0).color(muted_color);
            match self.updater.state() {
                update::State::Checking => {
                    ui.spinner();
                    ui.label(muted(t.checking));
                }
                update::State::UpToDate => {
                    ui.label(muted(t.up_to_date));
                }
                update::State::Available(a) => {
                    ui.label(egui::RichText::new(t.update_available.replace("{v}", &a.version)).size(13.0).color(self.theme.accent));
                    if ui.button(t.update_now).clicked() {
                        self.update_attempted = true;
                        self.update_dismissed = false;
                        self.updater.install(ctx, a.clone());
                    }
                    ui.hyperlink_to(t.release_notes, &a.url);
                }
                update::State::Installing(v) => {
                    ui.spinner();
                    ui.label(muted(&t.installing.replace("{v}", &v)));
                }
                update::State::Installed(v) => {
                    ui.label(muted(&t.update_installed.replace("{v}", &v)));
                    if ui.button(t.restart).clicked() {
                        self.request_close(CloseRequest::Restart);
                    }
                }
                update::State::Failed(e) => {
                    ui.label(egui::RichText::new(t.update_failed).size(13.0).color(self.theme.ansi[1])).on_hover_text(e);
                }
                update::State::Idle => {}
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut picked.auto_update, egui::RichText::new(t.auto_update).size(14.0));
        ui.add_space(22.0);

        ui.label(egui::RichText::new(t.project.to_uppercase()).size(12.0).strong().color(self.theme.text_muted));
        ui.add_space(4.0);
        let icon = self.github_icon.get_or_insert_with(|| load_png(ctx, "github-mark", include_bytes!("../assets/icon/github-mark.png")));
        let logo = egui::Image::new(&*icon).fit_to_exact_size(Vec2::splat(16.0)).tint(self.theme.text);
        ui.horizontal(|ui| {
            let github = egui::Button::image_and_text(logo, egui::RichText::new(update::REPO).size(13.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
            if ui.add(github).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                crate::terminal::open_url(&format!("https://github.com/{}", update::REPO));
            }
            let releases = egui::Button::new(egui::RichText::new(t.all_releases).size(13.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
            if ui.add(releases).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                crate::terminal::open_url(&format!("https://github.com/{}/releases", update::REPO));
            }
        });
    }

    /// The config file, editable as JSON. Saving validates it first: a mistake is reported, never written.
    fn config_editor_ui(&mut self, ui: &mut Ui, t: &Strings) {
        let current = self.config.to_json();
        let editor = self.editor.get_or_insert_with(|| ConfigEditor::new(current.clone()));
        if editor.text == editor.base && editor.base != current {
            // Nothing typed: follow changes made elsewhere (renamed tab, outside edit...).
            *editor = ConfigEditor::new(current.clone());
        }
        let dirty = editor.text != editor.base;

        ui.label(egui::RichText::new(t.config_hint).size(13.0).color(self.theme.text_muted));
        ui.add_space(6.0);
        let path = config::config_path();
        ui.horizontal(|ui| {
            if let Some(path) = &path {
                let label = egui::RichText::new(path.display().to_string()).monospace().size(12.0).color(self.theme.text_muted);
                ui.add_sized(Vec2::new(EDITOR_WIDTH - 200.0, 20.0), egui::Label::new(label).truncate())
                    .on_hover_text(path.display().to_string());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The file only exists once something was saved.
                let exists = path.as_ref().is_some_and(|p| p.exists());
                if ui.add_enabled(exists, egui::Button::new(t.reveal_file)).clicked() {
                    if let Some(path) = &path {
                        config::reveal(path);
                    }
                }
            });
        });
        ui.add_space(6.0);

        let frame = Frame::new().fill(self.theme.bg).corner_radius(6.0).inner_margin(8.0).stroke(Stroke::new(1.0, self.theme.tab_hover));
        frame.show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(ui.available_height() - 50.0).auto_shrink(false).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut editor.text)
                        .font(egui::FontId::new(13.0, egui::FontFamily::Name("mono".into())))
                        .code_editor()
                        .frame(Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .desired_rows(24)
                        .text_color(self.theme.fg),
                );
            });
        });
        ui.add_space(8.0);

        let mut apply = None;
        ui.horizontal(|ui| {
            if let Some(err) = &editor.error {
                ui.label(egui::RichText::new(format!("{} : {err}", t.invalid_json)).size(13.0).color(self.theme.ansi[1]));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(dirty, egui::Button::new(egui::RichText::new(t.save).size(14.0)).min_size(Vec2::new(110.0, 30.0))).clicked() {
                    match Config::from_json(&editor.text) {
                        Ok(config) => apply = Some(config),
                        Err(e) => editor.error = Some(e.to_string()),
                    }
                }
                if ui.add_enabled(dirty, egui::Button::new(egui::RichText::new(t.revert).size(14.0)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    *editor = ConfigEditor::new(current.clone());
                }
            });
        });
        if let Some(config) = apply {
            self.apply_config(config);
            self.config_writable = !self.read_only;
            self.save_config();
            // Show the file as written (normalized formatting, defaults filled in).
            self.editor = Some(ConfigEditor::new(self.config.to_json()));
        }
    }

    /// Small caps section title with an optional "+" button on the right. Returns true when "+" is clicked.
    fn section_header(&self, ui: &Ui, title: &str, pos: Pos2, width: f32, plus_hint: Option<&str>) -> bool {
        let rect = Rect::from_min_size(pos, Vec2::new(width, SECTION_HEADER_H));
        let painter = ui.painter();
        painter.text(
            Pos2::new(rect.min.x + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(11.0),
            self.theme.text_muted.gamma_multiply(0.8),
        );
        let Some(hint) = plus_hint else { return false };
        let plus_rect = Rect::from_center_size(Pos2::new(rect.max.x - 12.0, rect.center().y), Vec2::splat(20.0));
        let plus = ui.interact(plus_rect, ui.id().with(("section-plus", title)), Sense::click());
        if plus.hovered() {
            painter.rect_filled(plus_rect, 4.0, self.theme.tab_hover);
        }
        let stroke = Stroke::new(1.6, if plus.hovered() { self.theme.text } else { self.theme.text_muted });
        let (c, d) = (plus_rect.center(), 5.5);
        painter.line_segment([c - Vec2::new(d, 0.0), c + Vec2::new(d, 0.0)], stroke);
        painter.line_segment([c - Vec2::new(0.0, d), c + Vec2::new(0.0, d)], stroke);
        plus.on_hover_text(hint).clicked()
    }

    /// Right-click menu of a tab.
    fn tab_menu(&self, ui: &mut Ui, i: usize, action: &mut Option<TabAction>) {
        let t = self.t();
        ui.set_min_width(170.0);
        let mut item = |ui: &mut Ui, label: &str, a: TabAction| {
            if ui.button(label).clicked() {
                *action = Some(a);
                ui.close();
            }
        };
        item(ui, t.rename, TabAction::StartRename(i));
        item(ui, t.split_right, TabAction::Split(i, Direction::Right));
        item(ui, t.split_down, TabAction::Split(i, Direction::Down));
        ui.separator();
        ui.label(egui::RichText::new(t.color).size(12.0).strong().color(self.theme.text_muted));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for color in TAB_COLORS {
                let (r, s) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                let selected = self.tabs[i].color == Some(color);
                ui.painter().circle_filled(r.center(), 7.0, color);
                if selected || s.hovered() {
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, self.theme.text));
                }
                if s.clicked() {
                    *action = Some(TabAction::SetColor(i, Some(color)));
                    ui.close();
                }
            }
        });
        if self.tabs[i].color.is_some() && ui.button(t.remove_color).clicked() {
            *action = Some(TabAction::SetColor(i, None));
            ui.close();
        }
        ui.separator();
        if ui.button(t.close).clicked() {
            *action = Some(TabAction::Close(i));
            ui.close();
        }
    }

    /// Invitation to install a new release (then to restart), drawn at the bottom of `area`. Returns
    /// the top of what it drew (`area.max.y` when nothing is shown).
    fn update_card(&mut self, ui: &mut Ui, area: Rect) -> f32 {
        let t = self.t();
        let (title, detail) = match self.updater.state() {
            update::State::Available(a) if !self.update_dismissed => (t.update_available.replace("{v}", &a.version), None),
            update::State::Installing(v) => (t.installing.replace("{v}", &v), None),
            update::State::Installed(v) => (t.update_installed.replace("{v}", &v), None),
            update::State::Failed(e) if self.update_attempted => (t.update_failed.to_owned(), Some(e)),
            _ => return area.max.y,
        };
        let state = self.updater.state();
        let buttons = !matches!(state, update::State::Installing(_));
        let h = if buttons { 78.0 } else { 44.0 };
        let card = Rect::from_min_max(Pos2::new(area.min.x, area.max.y - h - 6.0), Pos2::new(area.max.x, area.max.y - 6.0));
        let painter = ui.painter();
        painter.rect_filled(card, 8.0, self.theme.tab_active);
        painter.rect_stroke(card, 8.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        let mut job = egui::text::LayoutJob::simple_singleline(title, FontId::proportional(13.0), self.theme.text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(card.width() - 24.0);
        painter.galley(card.min + Vec2::new(12.0, 11.0), painter.layout_job(job), self.theme.text);
        let hover = ui.interact(card, ui.id().with("update-card"), Sense::hover());
        if let Some(e) = &detail {
            hover.on_hover_text(e);
        }
        if matches!(state, update::State::Installing(_)) {
            ui.put(Rect::from_center_size(Pos2::new(card.max.x - 20.0, card.min.y + 20.0), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
            return card.min.y - 6.0;
        }

        let row = Rect::from_min_max(Pos2::new(card.min.x + 10.0, card.max.y - 38.0), Pos2::new(card.max.x - 10.0, card.max.y - 10.0));
        let half = (row.width() - 8.0) / 2.0;
        let (left_rect, right_rect) = (Rect::from_min_size(row.min, Vec2::new(half, row.height())), Rect::from_min_size(row.min + Vec2::new(half + 8.0, 0.0), Vec2::new(half, row.height())));
        let primary = |ui: &mut Ui, rect: Rect, label: &str| {
            let text = egui::RichText::new(label).size(12.5).color(self.theme.bg);
            ui.put(rect, egui::Button::new(text).fill(self.theme.accent).corner_radius(6.0)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        };
        let secondary = |ui: &mut Ui, rect: Rect, label: &str| ui.put(rect, egui::Button::new(egui::RichText::new(label).size(12.5)).corner_radius(6.0)).clicked();
        match state {
            update::State::Available(a) => {
                if primary(ui, left_rect, t.update_now) {
                    self.update_attempted = true;
                    self.updater.install(ui.ctx(), a.clone());
                }
                if secondary(ui, right_rect, t.later) {
                    self.update_dismissed = true;
                }
            }
            update::State::Installed(_) => {
                if primary(ui, left_rect, t.restart) {
                    self.request_close(CloseRequest::Restart);
                }
                if secondary(ui, right_rect, t.later) {
                    self.update_dismissed = true;
                }
            }
            update::State::Failed(_) => {
                if primary(ui, left_rect, t.check_now) {
                    self.updater.check(ui.ctx());
                }
                if secondary(ui, right_rect, t.close) {
                    self.update_attempted = false;
                }
            }
            _ => {}
        }
        card.min.y - 6.0
    }

    /// Saves everything, starts the updated app and closes this one.
    fn restart(&mut self) {
        self.sync();
        match self.updater.relaunch() {
            Ok(()) => {
                self.close_confirmed = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(e) => self.error = Some(format!("{} : {e:#}", self.t().update_failed)),
        }
    }

    /// What closing these panes of tab `index` (all of them if None) would interrupt: programs running
    /// instead of the shell, and open SSH connections.
    fn busy(&self, index: usize, panes: Option<&[PaneId]>) -> Vec<String> {
        let Some(tab) = self.tabs.get(index) else { return Vec::new() };
        let mut busy = Vec::new();
        for (id, term) in &tab.panes {
            if panes.is_some_and(|p| !p.contains(id)) || tab.dead.contains(id) || term.has_exited() {
                continue;
            }
            if tab.ssh.is_some() {
                busy.push(format!("{}  ·  ssh", tab.title()));
            } else if let Some(program) = term.foreground() {
                // The folder tells apart several panes running the same program.
                let dir = term.cwd().and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()));
                busy.push(match dir {
                    Some(dir) => format!("{}  ·  {program}  ({dir})", tab.title()),
                    None => format!("{}  ·  {program}", tab.title()),
                });
            }
        }
        // Panes are in a map: give the list a stable order.
        busy.sort();
        busy
    }

    fn busy_for(&self, request: CloseRequest) -> Vec<String> {
        match request {
            CloseRequest::Pane(index, id) => self.busy(index, Some(&[id])),
            CloseRequest::Tab(index) => self.busy(index, None),
            CloseRequest::Window | CloseRequest::Restart => (0..self.tabs.len()).flat_map(|i| self.busy(i, None)).collect(),
        }
    }

    /// Closes right away, or asks first when that would interrupt running programs.
    fn request_close(&mut self, request: CloseRequest) {
        let busy = self.busy_for(request);
        if busy.is_empty() {
            self.do_close(request);
        } else {
            self.confirm_close = Some(ConfirmClose { request, busy });
        }
    }

    fn do_close(&mut self, request: CloseRequest) {
        match request {
            CloseRequest::Pane(index, id) => self.close_pane(index, id),
            CloseRequest::Tab(index) => self.close_tab(index),
            CloseRequest::Window => {
                self.close_confirmed = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            CloseRequest::Restart => self.restart(),
        }
    }

    /// "Open this link?" for links that aren't web addresses, showing the exact target.
    fn link_confirm_window(&mut self, ctx: &egui::Context) {
        // Links clicked in any pane of the visible tab.
        if self.link_confirm.is_none() {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                self.link_confirm = tab.panes.values_mut().find_map(Terminal::take_link_request);
            }
        }
        let Some(url) = self.link_confirm.clone() else { return };
        let t = self.t();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-link")).frame(frame).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(egui::RichText::new(t.open_link_title).size(17.0).strong());
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.open_link_body).size(13.5).color(self.theme.text_muted));
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(&url).monospace().size(12.5)).wrap());
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(t.open_link).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                cancel.request_focus();
                if cancel.clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        if let Some(open) = answer {
            if open {
                crate::terminal::open_url(&url);
            }
            self.link_confirm = None;
            self.focus_terminal = true;
        }
    }

    /// "Reset everything?" dialog: erases the whole configuration, then restarts.
    fn confirm_reset_window(&mut self, ctx: &egui::Context) {
        if !self.confirm_reset {
            return;
        }
        let t = self.t();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-reset")).frame(frame).backdrop_color(Color32::from_black_alpha(190)).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(egui::RichText::new(format!("⚠  {}", t.reset_title)).size(17.0).strong().color(self.theme.ansi[1]));
            ui.add_space(10.0);
            ui.label(egui::RichText::new(t.reset_body).size(13.5));
            if let Some(dir) = config::config_dir() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(dir.display().to_string()).size(11.5).monospace().color(self.theme.text_muted));
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let erase = egui::Button::new(egui::RichText::new(t.reset_confirm).size(13.5).color(Color32::WHITE)).fill(self.theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(110.0, 30.0));
                if ui.add(erase).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                // Cancel is the default: Enter or Escape erase nothing.
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
            Some(true) => {
                self.confirm_reset = false;
                self.reset_everything();
            }
            Some(false) => self.confirm_reset = false,
            None => {}
        }
    }

    /// Erases every profile, host, setting, tab and history (and the saved passwords), then restarts.
    fn reset_everything(&mut self) {
        for host in self.config.ssh.iter().filter(|h| h.password_saved) {
            ssh::delete_password(host.id);
        }
        // Nothing may be written again before quitting (not even the session on exit).
        self.read_only = true;
        self.config_writable = false;
        if let Err(e) = config::erase_all() {
            self.error = Some(format!("{e:#}"));
            return;
        }
        match self.updater.relaunch() {
            Ok(()) => {
                self.close_confirmed = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    /// "Close anyway?" dialog listing the programs that would be stopped.
    fn confirm_close_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm_close else { return };
        let t = self.t();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-close")).frame(frame).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new(t.close_anyway_title).size(17.0).strong());
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.close_anyway_body).size(13.5).color(self.theme.text_muted));
            ui.add_space(6.0);
            for item in confirm.busy.iter().take(8) {
                ui.label(egui::RichText::new(format!("•  {item}")).size(13.0).monospace());
            }
            if confirm.busy.len() > 8 {
                ui.label(egui::RichText::new(format!("…  +{}", confirm.busy.len() - 8)).size(13.0).color(self.theme.text_muted));
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = egui::Button::new(egui::RichText::new(t.close).size(13.5).color(Color32::WHITE)).fill(self.theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0));
                if ui.add(close).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                // Cancel is the default: Enter or Escape keep everything running.
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
            Some(true) => {
                let request = confirm.request;
                self.confirm_close = None;
                self.do_close(request);
            }
            Some(false) => {
                self.confirm_close = None;
                self.focus_terminal = true;
            }
            None => {}
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        self.live = self.tabs.iter_mut().map(|tab| tab.panes.values_mut().find_map(|term| term.live_program(&ctx).map(str::to_owned))).collect();
        // New profiles and hosts show up outside groups; deleted ones disappear.
        self.config.normalize();
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, self.theme.chrome_bg);
        ui.painter().vline(bar.max.x - 0.5, bar.y_range(), Stroke::new(1.0, self.theme.tab_hover));

        // Empty space in the sidebar drags the window (the native title bar is hidden).
        let bg = ui.interact(bar, ui.id().with("sidebar-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }

        if !ui.input(|i| i.pointer.any_down()) {
            self.tab_grab = None;
        }
        let mut action: Option<TabAction> = None;
        let left = bar.min.x + SIDEBAR_PAD;
        let row_w = bar.width() - 2.0 * SIDEBAR_PAD;

        // Everything below the traffic lights scrolls when there are many tabs.
        let footer_top = bar.max.y - FOOTER_H;
        let logo_rect = Rect::from_min_size(Pos2::new(bar.min.x, bar.min.y + SIDEBAR_TOP), Vec2::new(bar.width() - 1.0, LOGO_H));
        paint_logo(ui.painter(), logo_rect, &self.theme);
        // Dev builds say so, next to the logo: they don't share the installed app's profiles.
        if !config::OFFICIAL {
            let at = Pos2::new(logo_rect.center().x + 58.0, logo_rect.center().y - 12.0);
            let galley = ui.painter().layout_no_wrap("DEV".to_owned(), FontId::monospace(9.5), self.theme.bg);
            let badge = Rect::from_min_size(at, galley.size() + Vec2::new(8.0, 2.0));
            ui.painter().rect_filled(badge, 3.0, self.theme.ansi[3]);
            ui.painter().galley(badge.min + Vec2::new(4.0, 1.0), galley, self.theme.bg);
        }
        let card_top = self.update_card(ui, Rect::from_min_max(Pos2::new(left, bar.min.y), Pos2::new(left + row_w, footer_top)));
        let scroll_rect = Rect::from_min_max(Pos2::new(bar.min.x, logo_rect.max.y), Pos2::new(bar.max.x - 1.0, card_top));

        // Footer: settings button, always visible.
        let button = Rect::from_min_max(Pos2::new(left, footer_top + 5.0), Pos2::new(left + row_w, bar.max.y - 7.0));
        let settings = ui.interact(button, ui.id().with("settings-btn"), Sense::click());
        let hot = settings.hovered() || self.settings_dialog;
        ui.painter().rect_filled(button, 6.0, if hot { self.theme.tab_active } else { self.theme.tab_hover.gamma_multiply(0.6) });
        ui.painter().rect_stroke(button, 6.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(if hot { 0.8 } else { 0.35 })), egui::StrokeKind::Inside);
        paint_gear(ui.painter(), Pos2::new(button.min.x + 16.0, button.center().y), self.theme.accent);
        ui.painter().text(Pos2::new(button.min.x + 32.0, button.center().y), Align2::LEFT_CENTER, t.settings, FontId::proportional(13.0), self.theme.text);
        let shortcut = self.config.settings.shortcuts.open_settings.label();
        ui.painter().text(Pos2::new(button.max.x - 10.0, button.center().y), Align2::RIGHT_CENTER, &shortcut, FontId::proportional(11.5), self.theme.text_muted);
        if settings.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.settings_dialog = true;
        }

        let mut scroll_ui = ui.new_child(egui::UiBuilder::new().max_rect(scroll_rect));
        // A slim handle that stays visible whenever the list overflows (egui's default only shows on hover).
        scroll_ui.spacing_mut().scroll = egui::style::ScrollStyle {
            bar_width: 6.0,
            floating_allocated_width: 4.0,
            dormant_background_opacity: 0.0,
            active_background_opacity: 0.0,
            dormant_handle_opacity: 0.6,
            ..egui::style::ScrollStyle::thin()
        };
        // Mouse drags never scroll it (egui's default): they move tabs, or the window from empty space.
        let scroll = egui::ScrollArea::vertical()
            .id_salt("sidebar-scroll")
            .auto_shrink(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
        scroll.show(&mut scroll_ui, |ui| {
        let base_painter = ui.painter().clone();
        // The dragged tab is painted above its neighbours.
        let drag_painter = base_painter.clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("tab-drag")));
        let origin = ui.max_rect().min;
        let mut y = origin.y;

        // Local section: open tabs that are neither a profile nor an SSH host, then local profiles.
        let new_hint = format!("{} ({})", t.new_tab, self.config.settings.shortcuts.new_tab.label());
        if self.section_header(ui, t.terminals, Pos2::new(left, y), row_w, Some(&new_hint)) {
            action = Some(TabAction::New);
        }
        y += SECTION_HEADER_H;

        let is_plain = |tab: &Tab| tab.ssh.is_none() && tab.profile.is_none();
        let first_y = y;
        let local_count = self.tabs.iter().filter(|t| is_plain(t)).count();
        let mut tab_rects: Vec<(usize, Rect)> = Vec::with_capacity(local_count);
        for i in 0..self.tabs.len() {
            if !is_plain(&self.tabs[i]) {
                continue;
            }
            let slot = Rect::from_min_size(Pos2::new(left, y), Vec2::new(row_w, ROW_H));
            tab_rects.push((i, slot));
            y += ROW_H + ROW_GAP;

            let id = ui.id().with(("tab", i));
            let resp = ui.interact(slot, id, Sense::click_and_drag());
            let pointer_y = ui.input(|inp| inp.pointer.interact_pos()).map(|p| p.y);
            // Only on the first frame: a drag moved to a new slot after a swap may report started again.
            if resp.drag_started() && self.tab_grab.is_none() {
                self.tab_grab = pointer_y.map(|py| py - slot.min.y);
            }
            let dragging = resp.dragged() && self.tab_grab.is_some();
            let (rect, painter) = match (dragging, self.tab_grab, pointer_y) {
                (true, Some(grab), Some(py)) => {
                    let max_y = first_y + (ROW_H + ROW_GAP) * (local_count as f32 - 1.0);
                    let top = (py - grab).clamp(first_y, max_y.max(first_y));
                    (slot.translate(Vec2::new(0.0, top - slot.min.y)), drag_painter.clone())
                }
                _ => (slot, base_painter.clone()),
            };
            let active = i == self.active;
            let tab = &self.tabs[i];
            // Not `hovered()`: that turns false when the pointer is over the ✕ (another widget), which
            // would hide the ✕ as soon as the pointer reaches it.
            let row_hovered = resp.contains_pointer();

            let fill = if active || dragging {
                self.theme.tab_active
            } else if row_hovered {
                self.theme.tab_hover
            } else {
                self.theme.tab_bg
            };
            painter.rect_filled(rect, 6.0, fill);
            if active {
                paint_active_bar(&painter, rect, self.theme.accent);
            }
            let dot = Pos2::new(rect.min.x + 14.0, rect.center().y);
            let live = self.live.get(i).cloned().flatten();
            if live.is_some() {
                paint_live(ui, &painter, dot, &self.theme);
            }
            match tab.color {
                Some(color) => painter.circle_filled(dot, 4.0, color),
                None => painter.circle_stroke(dot, 3.5, Stroke::new(1.0, self.theme.text_muted.gamma_multiply(0.6))),
            };

            let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
            let show_close = active || row_hovered;
            let text_left = rect.min.x + 28.0;

            let renaming = self.rename.as_ref().is_some_and(|r| r.tab == i);
            if renaming {
                let r = self.rename.as_mut().unwrap();
                let edit_rect = Rect::from_min_max(Pos2::new(text_left, rect.min.y + 5.0), Pos2::new(rect.max.x - 8.0, rect.max.y - 5.0));
                let edit = ui.put(
                    edit_rect,
                    egui::TextEdit::singleline(&mut r.text)
                        .font(FontId::proportional(13.0))
                        .frame(Frame::NONE)
                        .text_color(if r.error.is_some() { self.theme.ansi[1] } else { self.theme.text }),
                );
                if edit.changed() {
                    r.error = None;
                }
                if let Some(err) = r.error {
                    // Keep editing until the name is changed or the rename cancelled.
                    edit.request_focus();
                    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, self.theme.ansi[1]), egui::StrokeKind::Inside);
                    egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), edit.id.with("err"), egui::PopupAnchor::Position(rect.left_bottom() + Vec2::new(0.0, 4.0)))
                        .show(|ui| ui.label(egui::RichText::new(err).color(self.theme.ansi[1])));
                }
                if r.select_all > 0 {
                    edit.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
                        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(r.text.chars().count()));
                        state.cursor.set_char_range(Some(all));
                        state.store(ui.ctx(), edit.id);
                    }
                    if !ui.input(|inp| inp.pointer.any_down()) {
                        r.select_all -= 1;
                    }
                    ui.ctx().request_repaint();
                }
                let enter = ui.input(|inp| inp.key_pressed(Key::Enter));
                let escape = ui.input(|inp| inp.key_pressed(Key::Escape));
                if escape {
                    self.rename = None;
                    self.focus_terminal = true;
                } else if enter {
                    action = Some(TabAction::Rename(i, true));
                } else if edit.lost_focus() {
                    action = Some(TabAction::Rename(i, false));
                }
            } else {
                let text_color = if active { self.theme.text } else { self.theme.text_muted };
                let text_right = if show_close { close_rect.min.x - 4.0 } else { rect.max.x - 10.0 };
                let text_rect = Rect::from_min_max(Pos2::new(text_left, rect.min.y), Pos2::new(text_right, rect.max.y));
                let mut job = egui::text::LayoutJob::simple_singleline(tab.title().to_owned(), FontId::proportional(13.0), text_color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
                let galley = painter.layout_job(job);
                let pos = Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0);
                painter.with_clip_rect(text_rect).galley(pos, galley, text_color);
            }

            if show_close && !renaming {
                let close = ui.interact(close_rect, id.with("close"), Sense::click());
                if close.hovered() {
                    painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
                }
                let c = close_rect.center();
                let d = 3.5;
                let stroke = Stroke::new(1.4, if close.hovered() { self.theme.text } else { self.theme.text_muted });
                painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
                painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
                if close.clicked() {
                    action = Some(TabAction::Close(i));
                }
            }

            if resp.clicked() {
                action.get_or_insert(TabAction::Select(i));
            }
            if resp.double_clicked() {
                action = Some(TabAction::StartRename(i));
            }
            if resp.middle_clicked() {
                action = Some(TabAction::Close(i));
            }
            if dragging {
                action = Some(TabAction::DragOver(i, rect.center().y));
            }
            if resp.drag_stopped() {
                self.tab_grab = None;
            }
            let resp = match &live {
                Some(program) => resp.on_hover_text(format!("▶ {program}")),
                None => resp,
            };
            resp.context_menu(|ui| self.tab_menu(ui, i, &mut action));
        }

        // Local profiles sit with the terminals, in their own groups; the SSH section below lists hosts.
        self.profiles_section(ui, left, row_w, &mut y, &mut action, true);

        y += SECTION_GAP;
        self.profiles_section(ui, left, row_w, &mut y, &mut action, false);

        // Content height, so the scroll area knows how far it can go.
        ui.allocate_rect(Rect::from_min_max(origin, Pos2::new(origin.x + 1.0, y + ROW_H + SIDEBAR_PAD)), Sense::hover());

        match action {
            Some(TabAction::Select(i)) => self.select(i),
            Some(TabAction::Close(i)) => self.request_close(CloseRequest::Tab(i)),
            Some(TabAction::New) => self.new_tab(ui.ctx()),
            Some(TabAction::Split(i, side)) => self.split(ui.ctx(), i, side),
            Some(TabAction::DeleteProfile(id)) => self.delete_profile(id),
            Some(TabAction::NewHost) => self.host_editor = Some(HostEditor::new(SshHost::new(), true, None)),
            Some(TabAction::EditHost(id)) => {
                if let Some(host) = self.config.ssh.iter().find(|h| h.id == id) {
                    let group = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
                    self.host_editor = Some(HostEditor::new(host.clone(), false, group));
                }
            }
            Some(TabAction::DeleteHost(id)) => self.delete_host(id),
            Some(TabAction::OpenItem(id)) => {
                if self.config.profiles.iter().any(|p| p.id == id) {
                    self.open_profile(id);
                } else {
                    self.open_ssh(id);
                }
            }
            Some(TabAction::ItemColor(id, color)) => {
                self.set_profile_color(id, color);
                if let Some(host) = self.config.ssh.iter_mut().find(|h| h.id == id) {
                    host.color = color;
                }
                for tab in self.tabs.iter_mut().filter(|t| t.ssh == Some(id)) {
                    tab.color = color;
                }
            }
            Some(TabAction::StartItemRename(id)) => {
                if let Some(item) = self.item(id) {
                    self.item_rename = Some(ItemRename { id, text: item.name, error: None, fresh: true });
                }
            }
            Some(TabAction::RenameItem(id, text, confirmed)) => match self.rename_profile(id, &text) {
                Ok(()) => self.item_rename = None,
                Err(err) if confirmed => {
                    if let Some(r) = self.item_rename.as_mut() {
                        r.error = Some(err);
                    }
                }
                Err(_) => self.item_rename = None,
            },
            Some(TabAction::ToggleGroup(id)) => {
                if let Some(g) = self.config.groups.iter_mut().find(|g| g.id == id) {
                    g.collapsed = !g.collapsed;
                }
            }
            Some(TabAction::StartGroupRename(id)) => {
                if let Some(g) = self.config.groups.iter().find(|g| g.id == id) {
                    self.group_rename = Some((id, g.name.clone(), true));
                }
            }
            Some(TabAction::RenameGroup(id, name)) => {
                if let Some(g) = self.config.groups.iter_mut().find(|g| g.id == id).filter(|_| !name.is_empty()) {
                    g.name = name;
                }
                self.group_rename = None;
            }
            Some(TabAction::NewGroup(local)) => {
                let group = config::Group::new(t.new_group_name, local);
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::EditProfile(id)) => {
                // An open profile tab has the latest layout: save it first.
                self.sync();
                if let Some(p) = self.config.profiles.iter().find(|p| p.id == id) {
                    let cwds = p.tab.layout.cwds().into_iter().map(|c| c.map(|c| c.display().to_string()).unwrap_or_default()).collect();
                    self.profile_editor = Some(ProfileEditor {
                        id,
                        name: p.tab.name.clone().unwrap_or_default(),
                        color: p.tab.color,
                        cwds,
                        commands: p.commands.clone(),
                        new_command: String::new(),
                        error: None,
                    });
                }
            }
            Some(TabAction::NewGroupWith(item)) => {
                let local = self.config.profiles.iter().any(|p| p.id == item);
                let mut group = config::Group::new(t.new_group_name, local);
                self.config.unplace(item);
                group.items.push(item);
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::DeleteGroup(id)) => {
                // Its items stay, outside groups.
                if let Some(index) = self.config.groups.iter().position(|g| g.id == id) {
                    let group = self.config.groups.remove(index);
                    self.config.ungrouped.extend(group.items);
                }
            }
            Some(TabAction::DropItem(item, group, before)) => {
                self.config.unplace(item);
                let list = match group.and_then(|g| self.config.groups.iter_mut().find(|x| x.id == g)) {
                    Some(g) => &mut g.items,
                    None => &mut self.config.ungrouped,
                };
                let at = before.and_then(|b| list.iter().position(|x| *x == b)).unwrap_or(list.len());
                list.insert(at, item);
            }
            Some(TabAction::Reconnect(i)) => {
                if let Some(tab) = self.tabs.get(i) {
                    let panes = tab.layout.leaves();
                    self.reconnect(ui.ctx(), i, &panes);
                    self.select(i);
                }
            }
            Some(TabAction::DropGroup(id, before)) => {
                if let Some(index) = self.config.groups.iter().position(|g| g.id == id) {
                    let group = self.config.groups.remove(index);
                    let at = before.and_then(|b| self.config.groups.iter().position(|g| g.id == b)).unwrap_or(self.config.groups.len());
                    self.config.groups.insert(at, group);
                }
            }
            Some(TabAction::SetColor(i, c)) => {
                self.tabs[i].color = c;
                if let Some(host) = self.tabs[i].ssh.and_then(|id| self.config.ssh.iter_mut().find(|h| h.id == id)) {
                    host.color = c;
                }
            }
            Some(TabAction::StartRename(i)) => {
                self.rename = Some(Rename { tab: i, text: self.tabs[i].title().to_owned(), select_all: 3, error: None });
            }
            Some(TabAction::Rename(i, confirmed)) => {
                if let Some(mut r) = self.rename.take() {
                    let name = r.text.trim();
                    let tab = &self.tabs[i];
                    let taken = tab.ssh.is_none()
                        && !name.is_empty()
                        && self.config.profiles.iter().any(|p| Some(p.id) != tab.profile && p.tab.name.as_deref() == Some(name));
                    if taken {
                        // Clicking away cancels; Enter keeps the editor open with the reason.
                        if confirmed {
                            r.error = Some(self.t().name_taken);
                            self.rename = Some(r);
                        }
                        return;
                    }
                    self.tabs[i].name = (!name.is_empty()).then(|| name.to_owned());
                    if let Some(host) = self.tabs[i].ssh.and_then(|id| self.config.ssh.iter_mut().find(|h| h.id == id)) {
                        if !name.is_empty() {
                            host.name = name.to_owned();
                        }
                    } else if self.tabs[i].name.is_some() && self.tabs[i].profile.is_none() {
                        // Naming a tab means it's worth keeping: it becomes a profile right away.
                        self.save_as_profile(i);
                    }
                }
                self.focus_terminal = true;
            }
            Some(TabAction::DragOver(i, py)) => {
                // Swap with the neighbour as soon as the dragged tab's middle enters its slot.
                let target = tab_rects.iter().find(|(_, r)| py >= r.min.y && py <= r.max.y + ROW_GAP).map(|(t, _)| *t);
                if let Some(t) = target {
                    if t != i {
                        let tab = self.tabs.remove(i);
                        self.tabs.insert(t, tab);
                        if self.active == i {
                            self.active = t;
                        } else if i < self.active && self.active <= t {
                            self.active -= 1;
                        } else if t <= self.active && self.active < i {
                            self.active += 1;
                        }
                        self.rename = None;
                        // Keep the drag going on the tab's new slot.
                        ui.ctx().set_dragged_id(ui.id().with(("tab", t)));
                    }
                }
            }
            None => {}
        }
        });
    }
}


const THEME_CARD: Vec2 = Vec2::new(230.0, 84.0);
const EDITOR_WIDTH: f32 = 720.0;
const SETTINGS_WIDTH: f32 = EDITOR_WIDTH;
/// Height of the settings pages (below the tabs), shrunk on small windows.
const SETTINGS_BODY_HEIGHT: f32 = 540.0;

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
    painter.galley(Pos2::new(x, rect.min.y + 34.0), painter.layout_job(job), theme.fg);
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
    let painter = ui.painter();
    let dots = ".".repeat(1 + (ui.input(|i| i.time) * 2.5) as usize % 3);
    let center = pane.center() - Vec2::new(0.0, 20.0);
    painter.text(center, Align2::CENTER_CENTER, format!("{text}{dots}"), FontId::proportional(15.0), theme.text);
    painter.text(center + Vec2::new(0.0, 26.0), Align2::CENTER_CENTER, t.connecting_hint, FontId::proportional(13.0), theme.text_muted);
}

/// A texture from embedded PNG bytes.
fn load_png(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let png = eframe::icon_data::from_png_bytes(bytes).unwrap_or_default();
    let image = egui::ColorImage::from_rgba_unmultiplied([png.width as usize, png.height as usize], &png.rgba);
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

/// Breathing green halo around a tab's dot while a program runs in it.
fn paint_live(ui: &Ui, painter: &egui::Painter, dot: Pos2, theme: &Theme) {
    let phase = (ui.input(|i| i.time) * std::f64::consts::TAU / 2.0).sin() as f32 * 0.5 + 0.5;
    painter.circle_filled(dot, 7.5, theme.ansi[2].gamma_multiply(0.10 + 0.18 * phase));
    painter.circle_stroke(dot, 6.5, Stroke::new(1.2, theme.ansi[2].gamma_multiply(0.45 + 0.5 * phase)));
    // A slow breath: a few frames per second are enough, and keep the CPU quiet.
    ui.ctx().request_repaint_after(Duration::from_millis(100));
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
fn paint_splash(painter: &egui::Painter, screen: Rect, theme: &Theme, t: f32) -> bool {
    const LETTERS: f32 = 0.13; // delay between two letters
    const DROP: f32 = 0.55; // duration of a letter's fall
    const FADE_START: f32 = 2.4;
    const END: f32 = 3.0;
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
    }
    true
}

/// What was clicked in a pane's header strip.
#[derive(Default)]
struct HeaderClicks {
    /// The strip itself: focus the pane.
    focus: bool,
    reconnect: bool,
    search: bool,
    commands: bool,
}

/// What the strip above a pane shows.
enum Header<'a> {
    /// A local pane: its working directory (when known and shown), and the local servers announced by
    /// its program.
    Local(Option<&'a Path>, &'a [LocalUrl]),
    /// An SSH pane: the host, with a reconnect button.
    Ssh(&'a str),
}

/// Strip above a pane: its working directory (home as `~`, leading folders elided to fit) or its SSH
/// host with a reconnect button, plus the saved commands (⚡) and, for local panes, history search.
fn pane_header(ui: &mut Ui, rect: Rect, id: PaneId, header: Header, focused: bool, theme: &Theme, t: &Strings, shortcuts: &config::Shortcuts) -> HeaderClicks {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme.chrome_bg);
    let resp = ui.interact(rect, egui::Id::new(("pane-header", id)), Sense::click());
    let font = FontId::monospace(12.0);
    let color = if focused { theme.text } else { theme.text_muted };
    let mut max_w = rect.width() - 20.0;

    let mut clicks = HeaderClicks::default();
    let icon = |ui: &mut Ui, right: f32, text: &str, tip: String| {
        let at = Rect::from_min_size(Pos2::new(right - 22.0, rect.min.y + 2.0), Vec2::new(22.0, rect.height() - 4.0));
        let button = egui::Button::new(egui::RichText::new(text).size(11.0)).frame(false);
        ui.put(at, button).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    };
    let mut right = rect.max.x - 4.0;
    // Local panes: history search, then the local servers (newest on the right), opened in the browser on click.
    if let Header::Local(_, urls) = header {
        clicks.search = icon(ui, right, "🔍", format!("{}  ({})", t.search_text_hint, shortcuts.find_text.label()));
        right -= 26.0;
        clicks.commands = icon(ui, right, "⚡", t.commands.to_owned());
        right -= 26.0;
        max_w -= 56.0;
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
        let text = egui::RichText::new(format!("↻  {}", t.reconnect)).size(12.0);
        let button = egui::Button::new(text).corner_radius(4.0).min_size(Vec2::new(0.0, rect.height() - 4.0));
        let w = 120.0_f32.min(rect.width() / 2.0);
        let at = Rect::from_min_max(Pos2::new(right - w, rect.min.y + 2.0), Pos2::new(right, rect.max.y - 2.0));
        clicks.reconnect = ui.put(at, button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
        clicks.commands = icon(ui, at.min.x - 4.0, "⚡", t.commands.to_owned());
        max_w -= w + 34.0;
    }

    let (text, tooltip) = match header {
        Header::Ssh(host) => (host.to_owned(), None),
        Header::Local(None, _) => {
            clicks.focus = resp.clicked();
            return clicks;
        }
        Header::Local(Some(cwd), _) => {
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
    let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
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

/// Accent mark on the left edge of the active tab.
fn paint_active_bar(painter: &egui::Painter, row: Rect, accent: Color32) {
    let bar = Rect::from_min_size(Pos2::new(row.min.x, row.min.y + 7.0), Vec2::new(3.0, row.height() - 14.0));
    painter.rect_filled(bar, 1.5, accent);
}

/// A menu button that records `a` as the action and closes the menu.
fn menu_item<A>(ui: &mut Ui, label: &str, a: A, action: &mut Option<A>) {
    if ui.button(label).clicked() {
        *action = Some(a);
        ui.close();
    }
}



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
    DeleteGroup(Uuid),
    /// Move an item into a group (None: outside groups), before another item or at the end.
    DropItem(Uuid, Option<Uuid>, Option<Uuid>),
    /// Move a group before another one, or last.
    DropGroup(Uuid, Option<Uuid>),
    /// Restart every session of an SSH tab.
    Reconnect(usize),
}

enum PaneAction {
    Copy(PaneId),
    Paste(PaneId),
    Split(PaneId, Direction),
    Close(PaneId),
    Reveal(PaneId),
    Reconnect(PaneId),
    /// Write a saved command at the prompt, without running it.
    Insert(PaneId, String),
    /// Open the saved commands menu.
    Commands(PaneId),
}

/// Right-click menu of a terminal pane.
fn pane_menu(ui: &mut Ui, t: &Strings, shortcuts: &config::Shortcuts, id: PaneId, can_copy: bool, local: bool, commands: &[String], action: &mut Option<PaneAction>) {
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
    item(ui, true, t.split_up, String::new(), PaneAction::Split(id, Direction::Up));
    item(ui, true, t.split_right, shortcuts.split_right.label(), PaneAction::Split(id, Direction::Right));
    item(ui, true, t.split_down, shortcuts.split_down.label(), PaneAction::Split(id, Direction::Down));
    item(ui, true, t.split_left, String::new(), PaneAction::Split(id, Direction::Left));
    ui.separator();
    if local {
        // An SSH pane's directory is on the server.
        item(ui, true, t.open_location, String::new(), PaneAction::Reveal(id));
    } else {
        item(ui, true, t.reconnect, String::new(), PaneAction::Reconnect(id));
    }
    ui.separator();
    let mut picked = None;
    ui.menu_button(format!("⚡  {}", t.commands), |ui| {
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
    ui.separator();
    item(ui, true, t.close_pane, shortcuts.close_pane.label(), PaneAction::Close(id));
    if picked.is_some() {
        *action = picked;
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
        // Background tabs must keep answering their programs (cursor position reports, etc).
        for tab in &mut self.tabs {
            for term in tab.panes.values_mut() {
                term.process_events(ui.ctx(), &self.theme);
            }
        }

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

        self.track_window(ui.ctx());
        let now = ui.input(|i| i.time);
        if now - self.last_sync >= SYNC_INTERVAL {
            self.last_sync = now;
            self.sync();
        }
        ui.ctx().request_repaint_after(Duration::from_secs_f64(SYNC_INTERVAL));

        egui::Panel::left("sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(Frame::NONE)
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(self.theme.bg))
            .show(ui, |ui| {
                if let Some(err) = &self.error {
                    ui.colored_label(Color32::from_rgb(0xf7, 0x76, 0x8e), err);
                }
                let rect = ui.available_rect_before_wrap();
                self.start_pending(ui.ctx(), self.active);
                // SSH host whose saved password can answer prompts in this tab (sudo...).
                let password_host = self.tabs.get(self.active).and_then(|t| t.ssh).filter(|id| self.config.ssh.iter().any(|h| h.id == *id && h.password_saved));
                let saved_commands = self.saved_commands(self.active);
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    self.empty_state(ui, rect);
                    return;
                };
                if self.focus_terminal && self.rename.is_none() {
                    if let Some(term) = tab.panes.get(&tab.focused) {
                        term.request_focus(ui);
                    }
                    self.focus_terminal = false;
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
                let mut close_dead = None;
                let mut open_search = None;
                let mut open_commands = None;
                // Checked before the panes handle keys, so Cmd+Enter doesn't reach the terminal.
                let prompt = password_host.filter(|_| tab.panes.get(&tab.focused).is_some_and(Terminal::awaits_password));
                let mut fill_password = prompt.is_some()
                    && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter)));
                for &(id, r) in &rects {
                    let Some(term) = tab.panes.get_mut(&id) else { continue };
                    // Local panes show their directory (optional); SSH panes their host and a reconnect button.
                    // The strip also appears, even with directories hidden, when the program announced a local server.
                    let urls = if local { term.local_urls(ui.ctx()).to_vec() } else { Vec::new() };
                    let body = if !local || self.config.settings.show_cwd || !urls.is_empty() {
                        let (head, body) = r.split_top_bottom_at_y(r.min.y + PANE_HEADER_H);
                        let cwd = if local && self.config.settings.show_cwd { term.cached_cwd(ui.ctx()).map(Path::to_path_buf) } else { None };
                        let header = if local { Header::Local(cwd.as_deref(), &urls) } else { Header::Ssh(&ssh_label) };
                        let clicks = pane_header(ui, head, id, header, id == tab.focused, &self.theme, strings, &self.config.settings.shortcuts);
                        if clicks.search {
                            open_search = Some(id);
                        }
                        if clicks.commands {
                            open_commands = Some(id);
                        }
                        if clicks.focus {
                            term.request_focus(ui);
                        }
                        if clicks.reconnect {
                            reconnect = Some(vec![id]);
                        }
                        body
                    } else {
                        r
                    };
                    let resp = term.ui(ui, body, &self.theme, &self.fonts);
                    if resp.secondary_clicked() {
                        resp.request_focus();
                    }
                    if resp.has_focus() {
                        tab.focused = id;
                    }
                    let can_copy = term.selection_text().is_some();
                    resp.context_menu(|ui| pane_menu(ui, strings, &self.config.settings.shortcuts, id, can_copy, local, &saved_commands, &mut pane_action));
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
                        // Keep the animated dots moving while nothing else repaints.
                        ui.ctx().request_repaint_after(Duration::from_millis(400));
                    }
                    // Dim inactive panes and frame the focused one so it stands out.
                    if split && id != tab.focused {
                        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(if self.theme.dark { 50 } else { 18 }));
                    } else if split {
                        ui.painter().rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                    }
                }
                tab.rects = rects;

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
                    Some(PaneAction::Insert(id, command)) => {
                        if let Some(term) = tab.panes.get_mut(&id) {
                            term.paste_text(&command);
                        }
                        self.focus_terminal = true;
                    }
                    Some(PaneAction::Commands(id)) => open_commands = Some(id),
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
                if let Some(id) = open_commands {
                    // One popup at a time: the ⚡ menu replaces the search.
                    self.close_search();
                    self.commands_menu = if self.commands_menu.as_ref().is_some_and(|m| m.pane == id) { None } else { Some(CommandsMenu::new(self.active, id)) };
                }
                if let Some(panes) = reconnect {
                    self.reconnect(ui.ctx(), self.active, &panes);
                }
            });

        self.settings_window(ui.ctx());
        self.host_editor_window(ui.ctx());
        self.profile_editor_window(ui.ctx());

        // macOS menu bar.
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.menu {
            menu.sync(self.config.settings.language.strings(), &self.config.settings.shortcuts.open_settings);
            for action in menu.actions() {
                match action {
                    crate::menu::MenuAction::Settings => self.settings_dialog = true,
                    crate::menu::MenuAction::Reload => self.request_close(CloseRequest::Restart),
                    crate::menu::MenuAction::Quit => self.request_close(CloseRequest::Window),
                }
            }
        }

        // Closing the window (red button, Cmd+Q...) asks first when programs are still running.
        if ui.input(|i| i.viewport().close_requested()) && !self.close_confirmed {
            let busy = self.busy_for(CloseRequest::Window);
            if !busy.is_empty() {
                ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                self.confirm_close = Some(ConfirmClose { request: CloseRequest::Window, busy });
            }
        }
        self.history_search_ui(ui.ctx());
        self.commands_menu_ui(ui.ctx());
        self.confirm_close_window(ui.ctx());
        self.confirm_reset_window(ui.ctx());
        self.link_confirm_window(ui.ctx());

        // Startup splash, over everything; a click or a key skips it.
        if let Some(start) = self.splash {
            let now = ui.input(|i| i.time);
            let start = if start.is_nan() { now } else { start };
            self.splash = Some(start);
            let skip = ui.input(|i| i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("splash")));
            if skip || !paint_splash(&painter, ui.ctx().content_rect(), &self.theme, (now - start) as f32) {
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

    fn on_exit(&mut self) {
        self.sync();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }
}
