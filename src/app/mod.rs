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

mod files;
mod popups;
mod settings;
mod sidebar;

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
    /// SSH tabs: the file manager (created when first shown), and whether it is shown instead of the
    /// terminals.
    files: Option<Box<files::FileManager>>,
    show_files: bool,
}

impl Tab {
    fn new(layout: Node, panes: HashMap<PaneId, Terminal>) -> Self {
        let focused = layout.first_leaf();
        Self { name: None, color: None, panes, layout, focused, rects: Vec::new(), profile: None, ssh: None, cwds: HashMap::new(), histories: HashMap::new(), dead: Default::default(), pending: Vec::new(), files: None, show_files: false }
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
    /// App time of the last user input, to stop the periodic sync once idle.
    last_input: f64,
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
    #[cfg(unix)]
    askpass: crate::askpass::Server,
    /// Questions from ssh processes without a terminal (SFTP): password, new host key.
    #[cfg(unix)]
    ssh_prompts: std::sync::mpsc::Receiver<crate::askpass::Prompt>,
    /// The one being answered, and what is typed.
    #[cfg(unix)]
    ssh_prompt: Option<(crate::askpass::Prompt, String)>,
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
    ToggleFiles,
}

impl ShortcutAction {
    const ALL: [ShortcutAction; 10] = [
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
            last_input: 0.0,
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
            #[cfg(unix)]
            askpass: crate::askpass::Server::start(),
            #[cfg(unix)]
            ssh_prompts: std::sync::mpsc::channel().1,
            #[cfg(unix)]
            ssh_prompt: None,
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
        watch_config(&cc.egui_ctx);
        // Ronnie handles Cmd +/- itself (the zoom is saved in the settings).
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        #[cfg(unix)]
        {
            let (prompts, receiver) = std::sync::mpsc::channel();
            app.askpass.set_prompter(prompts, &cc.egui_ctx);
            app.ssh_prompts = receiver;
        }
        app.saved_session = session;
        if app.tabs.is_empty() {
            app.new_tab(&cc.egui_ctx);
        }
        if app.read_only {
            app.config_writable = false;
        } else if !app.session_frozen {
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
        self.allow_ssh_panes(index);
    }

    /// Lets the ssh processes of tab `index` get their host's saved password from the askpass helper.
    fn allow_ssh_panes(&self, index: usize) {
        #[cfg(unix)]
        if let Some((tab, host)) = self.tabs.get(index).and_then(|t| Some((t, t.ssh?))) {
            for term in tab.panes.values() {
                if let Some(pid) = term.pid() {
                    self.askpass.allow(pid, host);
                }
            }
        }
        #[cfg(not(unix))]
        let _ = index;
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
        self.allow_ssh_panes(index);
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

    fn save_config(&mut self) {
        if !self.config_writable || self.config == self.saved_config {
            return;
        }
        let Some(path) = config::config_path() else { return };
        match config::save_config_file(&path, &self.config) {
            Ok(()) => {
                self.saved_config = self.config.clone();
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
            self.allow_ssh_panes(index);
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
                ShortcutAction::ToggleFiles => {
                    if let Some(show) = self.tabs.get(self.active).filter(|t| t.ssh.is_some()).map(|t| !t.show_files) {
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
    files: bool,
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
        // The server's files, FileZilla style.
        let files = egui::Button::new(egui::RichText::new(format!("📁  {}", t.files_button)).size(12.0)).corner_radius(4.0).min_size(Vec2::new(0.0, rect.height() - 4.0));
        let fw = 90.0_f32.min(rect.width() / 4.0);
        let files_at = Rect::from_min_max(Pos2::new(at.min.x - 30.0 - fw, rect.min.y + 2.0), Pos2::new(at.min.x - 30.0, rect.max.y - 2.0));
        clicks.files = ui.put(files_at, files).on_hover_text(format!("{} ({})", t.files_open, shortcuts.toggle_files.label())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
        max_w -= w + 34.0 + fw + 4.0;
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
    /// Opens an SSH host's tab on its file manager.
    OpenFiles(Uuid),
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
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            for term in tab.panes.values_mut() {
                term.set_allow_clipboard(self.config.settings.clipboard_from_programs);
                term.process_events(ui.ctx(), &self.theme);
                term.set_visible(i == self.active);
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
        self.poll_files();
        // Errors shown to the user also go to ronnie.log, to diagnose them later.
        if self.error != self.logged_error {
            if let Some(e) = &self.error {
                crate::log::error(e);
            }
            self.logged_error = self.error.clone();
        }

        self.track_window(ui.ctx());
        let now = ui.input(|i| i.time);
        if now - self.last_sync >= SYNC_INTERVAL {
            self.last_sync = now;
            self.sync();
        }
        // Changes come with input: keep syncing shortly after it, then let an idle app sleep. Outside
        // edits of config.json wake it up through the config watcher.
        if ui.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.last_input = now;
        }
        if now - self.last_input < 3.0 * SYNC_INTERVAL {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(SYNC_INTERVAL));
        }

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
                // An SSH tab showing its file manager.
                if self.tabs.get(self.active).is_some_and(|t| t.show_files) {
                    self.files_view(ui, rect);
                    return;
                }
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
                let mut open_files = false;
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
                        if clicks.files {
                            open_files = true;
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
                    let can_copy = term.has_selection();
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
                if open_files {
                    self.toggle_files(self.active, true);
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
        self.paste_confirm_window(ui.ctx());
        self.ssh_prompt_window(ui.ctx());

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
        #[cfg(unix)]
        crate::askpass::cleanup();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }
}
