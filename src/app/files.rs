//! The file manager of an SSH tab, FileZilla style: this computer on the left, the server on the right,
//! transfers by drag and drop or buttons, a queue with progress at the bottom, and rename / delete /
//! new folder / permissions on either side.

use std::collections::HashSet;
use std::time::Instant;

use super::*;
use crate::sftp::{self, Entry, Event, Request};

/// Where a panel's files are.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Name,
    Size,
    Modified,
    Mode,
}

/// Rows being dragged from a panel.
#[derive(Clone)]
struct FilesDrag {
    from: Side,
    names: Vec<String>,
}

struct Panel {
    /// Current directory (a local path, or an absolute remote path).
    path: String,
    /// The path field, as typed.
    path_text: String,
    entries: Vec<Entry>,
    selected: HashSet<String>,
    /// Last clicked row, for Shift+click ranges.
    anchor: Option<String>,
    sort: (SortBy, bool),
    loading: bool,
}

impl Panel {
    fn new(path: String) -> Self {
        Self { path_text: path.clone(), path, entries: Vec::new(), selected: HashSet::new(), anchor: None, sort: (SortBy::Name, true), loading: false }
    }

    /// Entries to show, sorted (directories first), hidden files filtered.
    fn visible(&self, show_hidden: bool) -> Vec<&Entry> {
        let mut rows: Vec<&Entry> = self.entries.iter().filter(|e| show_hidden || !e.name.starts_with('.')).collect();
        let (by, asc) = self.sort;
        rows.sort_by(|a, b| {
            let order = match by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.mtime.cmp(&b.mtime),
                SortBy::Mode => a.mode.cmp(&b.mode),
            };
            b.is_dir.cmp(&a.is_dir).then(if asc { order } else { order.reverse() })
        });
        rows
    }

    fn selection(&self) -> Vec<String> {
        let mut names: Vec<String> = self.selected.iter().cloned().collect();
        names.sort();
        names
    }
}

#[derive(Clone, PartialEq)]
enum TransferState {
    Queued,
    Running,
    Done,
    Failed(String),
}

struct Transfer {
    id: u64,
    upload: bool,
    label: String,
    done: u64,
    total: u64,
    current: String,
    state: TransferState,
    started: Instant,
}

/// A dialog of the file manager.
enum Dialog {
    Rename { side: Side, from: String, text: String, fresh: bool },
    Mkdir { side: Side, text: String, fresh: bool },
    Delete { side: Side, names: Vec<String> },
    Chmod { names: Vec<String>, mode: u32, octal: String, recursive: bool, any_dir: bool },
    /// Some of the files to transfer already exist on the other side.
    Conflict { from: Side, names: Vec<String>, target: String, existing: Vec<String> },
}

enum Status {
    Connecting,
    Ready,
    Closed(String),
}

/// What the file manager asks its tab to do.
pub(super) enum FilesAction {
    None,
    ShowTerminal,
    Reconnect,
}

pub(super) struct FileManager {
    pub host: Uuid,
    conn: Option<sftp::Connection>,
    status: Status,
    local: Panel,
    remote: Panel,
    transfers: Vec<Transfer>,
    next_id: u64,
    dialog: Option<Dialog>,
    error: Option<String>,
    show_hidden: bool,
    /// The panel the keyboard acts on (last clicked).
    active: Side,
}

impl FileManager {
    /// Starts in the host's last folders (or the home directories).
    pub fn new(host: &SshHost) -> Self {
        let home = directories::BaseDirs::new().map(|d| d.home_dir().display().to_string()).unwrap_or_else(|| "/".into());
        let local = host.sftp_local.as_ref().filter(|p| std::path::Path::new(p).is_dir()).cloned().unwrap_or(home);
        let mut fm = Self {
            host: host.id,
            conn: None,
            status: Status::Connecting,
            local: Panel::new(local),
            remote: Panel::new(host.sftp_remote.clone().unwrap_or_default()),
            transfers: Vec::new(),
            next_id: 1,
            dialog: None,
            error: None,
            show_hidden: false,
            active: Side::Remote,
        };
        fm.read_local();
        fm
    }

    /// Opens the SFTP session; ssh's questions (password, host key) go to the askpass prompter.
    pub fn connect(&mut self, ctx: &egui::Context, host: &SshHost, #[cfg(unix)] askpass: &crate::askpass::Server) {
        let launch = host.sftp_command();
        match sftp::Connection::open(ctx, &launch.program, &launch.args, &launch.env) {
            Ok(conn) => {
                #[cfg(unix)]
                if let Some(pid) = conn.pid {
                    askpass.allow_interactive(pid, host.id);
                }
                self.conn = Some(conn);
                self.status = Status::Connecting;
                self.error = None;
            }
            Err(e) => self.status = Status::Closed(format!("{e:#}")),
        }
    }

    /// Transfers not finished yet (to warn before closing).
    pub fn busy(&self) -> bool {
        self.transfers.iter().any(|t| matches!(t.state, TransferState::Queued | TransferState::Running))
    }

    /// The folders to remember for the host: (local, remote).
    pub fn folders(&self) -> (String, Option<String>) {
        (self.local.path.clone(), (!self.remote.path.is_empty()).then(|| self.remote.path.clone()))
    }

    fn send(&self, request: Request) {
        if let Some(conn) = &self.conn {
            conn.send(request);
        }
    }

    fn list_remote(&mut self) {
        if !self.remote.path.is_empty() {
            self.remote.loading = true;
            self.send(Request::List(self.remote.path.clone()));
        }
    }

    fn read_local(&mut self) {
        let path = std::path::PathBuf::from(&self.local.path);
        match std::fs::read_dir(&path) {
            Ok(dir) => {
                self.local.entries = dir.flatten().filter_map(|e| local_entry(&e)).collect();
                self.local.path_text = self.local.path.clone();
                let names: HashSet<String> = self.local.entries.iter().map(|e| e.name.clone()).collect();
                self.local.selected.retain(|n| names.contains(n));
            }
            Err(e) => self.error = Some(format!("{} : {e}", path.display())),
        }
    }

    /// Handles what the session sent. Called every frame, for every tab (transfers go on in the background).
    pub fn poll(&mut self) {
        let Some(conn) = &self.conn else { return };
        for event in conn.poll() {
            match event {
                Event::Connected { home } => {
                    self.status = Status::Ready;
                    if self.remote.path.is_empty() {
                        self.remote.path = home;
                    }
                    self.list_remote();
                }
                Event::Listing { path, entries } => {
                    if path == self.remote.path {
                        self.remote.entries = entries;
                        self.remote.loading = false;
                        self.remote.path_text = path;
                        let names: HashSet<String> = self.remote.entries.iter().map(|e| e.name.clone()).collect();
                        self.remote.selected.retain(|n| names.contains(n));
                    }
                }
                Event::Changed => self.list_remote(),
                Event::Error(e) => {
                    self.remote.loading = false;
                    // A folder that can't be listed: stay where we were.
                    if self.remote.path != self.remote.path_text && !self.remote.path_text.is_empty() {
                        self.remote.path = self.remote.path_text.clone();
                    }
                    self.error = Some(e);
                }
                Event::Progress { id, done, total, current } => {
                    if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                        (t.done, t.total, t.current, t.state) = (done, total, current, TransferState::Running);
                    }
                }
                Event::Finished { id, result } => {
                    if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                        t.state = match result {
                            Ok(()) => TransferState::Done,
                            Err(e) => TransferState::Failed(e),
                        };
                    }
                    self.read_local();
                }
                Event::Closed(reason) => {
                    self.status = Status::Closed(reason);
                    for t in &mut self.transfers {
                        if matches!(t.state, TransferState::Queued | TransferState::Running) {
                            t.state = TransferState::Failed(String::new());
                        }
                    }
                }
            }
        }
        if matches!(self.status, Status::Closed(_)) {
            self.conn = None;
        }
    }

    fn panel(&mut self, side: Side) -> &mut Panel {
        match side {
            Side::Local => &mut self.local,
            Side::Remote => &mut self.remote,
        }
    }

    fn path_of(&self, side: Side, name: &str) -> String {
        match side {
            Side::Local => std::path::Path::new(&self.local.path).join(name).display().to_string(),
            Side::Remote => sftp::join(&self.remote.path, name),
        }
    }

    fn open_dir(&mut self, side: Side, path: String) {
        match side {
            Side::Local => {
                let previous = std::mem::replace(&mut self.local.path, path);
                self.local.selected.clear();
                self.read_local();
                if self.error.is_some() && !std::path::Path::new(&self.local.path).is_dir() {
                    self.local.path = previous;
                }
            }
            Side::Remote => {
                self.remote.path_text = std::mem::replace(&mut self.remote.path, path);
                self.remote.selected.clear();
                self.list_remote();
            }
        }
    }

    fn parent_of(side: Side, path: &str) -> String {
        match side {
            Side::Local => std::path::Path::new(path).parent().map(|p| p.display().to_string()).unwrap_or_else(|| path.to_owned()),
            Side::Remote => sftp::parent(path),
        }
    }

    /// Transfers `names` of `from` into the other side's `target` directory, asking first when some
    /// already exist there.
    fn transfer(&mut self, from: Side, names: Vec<String>, target: Option<String>) {
        if names.is_empty() || self.conn.is_none() {
            return;
        }
        let to = if from == Side::Local { Side::Remote } else { Side::Local };
        let target = target.unwrap_or_else(|| self.panel(to).path.clone());
        let existing: Vec<String> = if target == self.panel(to).path {
            let there: HashSet<&str> = self.panel(to).entries.iter().map(|e| e.name.as_str()).collect();
            names.iter().filter(|n| there.contains(n.as_str())).cloned().collect()
        } else {
            Vec::new()
        };
        if existing.is_empty() {
            self.start_transfer(from, names, target, false);
        } else {
            self.dialog = Some(Dialog::Conflict { from, names, target, existing });
        }
    }

    fn start_transfer(&mut self, from: Side, names: Vec<String>, target: String, overwrite: bool) {
        let id = self.next_id;
        self.next_id += 1;
        let label = match names.len() {
            1 => names[0].clone(),
            n => format!("{}  +{}", names[0], n - 1),
        };
        let paths: Vec<String> = names.iter().map(|n| self.path_of(from, n)).collect();
        let request = match from {
            Side::Local => Request::Upload { id, local: paths.into_iter().map(Into::into).collect(), remote_dir: target, overwrite },
            Side::Remote => Request::Download { id, remote: paths, local_dir: target.into(), overwrite },
        };
        self.send(request);
        self.transfers.push(Transfer { id, upload: from == Side::Local, label, done: 0, total: 0, current: String::new(), state: TransferState::Queued, started: Instant::now() });
    }

    fn mkdir(&mut self, side: Side, name: &str) {
        let path = self.path_of(side, name);
        match side {
            Side::Local => {
                if let Err(e) = std::fs::create_dir(&path) {
                    self.error = Some(format!("{path} : {e}"));
                }
                self.read_local();
            }
            Side::Remote => self.send(Request::Mkdir(path)),
        }
    }

    fn rename(&mut self, side: Side, from: &str, to: &str) {
        let (a, b) = (self.path_of(side, from), self.path_of(side, to));
        match side {
            Side::Local => {
                if let Err(e) = std::fs::rename(&a, &b) {
                    self.error = Some(format!("{a} : {e}"));
                }
                self.read_local();
            }
            Side::Remote => self.send(Request::Rename(a, b)),
        }
    }

    fn delete(&mut self, side: Side, names: &[String]) {
        let paths: Vec<String> = names.iter().map(|n| self.path_of(side, n)).collect();
        match side {
            Side::Local => {
                for path in paths {
                    let p = std::path::Path::new(&path);
                    let result = if p.is_dir() && !p.is_symlink() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
                    if let Err(e) = result {
                        self.error = Some(format!("{path} : {e}"));
                    }
                }
                self.read_local();
            }
            Side::Remote => self.send(Request::Remove(paths)),
        }
    }

    /// The whole view. Returns what the tab should do (show the terminal, reconnect).
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings, host_name: &str) -> FilesAction {
        let mut action = FilesAction::None;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        ui.painter().rect_filled(rect, 0.0, theme.bg);

        // Toolbar.
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new(format!("📁  {host_name}")).size(14.0).strong());
            ui.add_space(8.0);
            let (text, color) = match &self.status {
                Status::Connecting => (t.files_connecting.to_owned(), theme.text_muted),
                Status::Ready => (t.files_connected.to_owned(), theme.ansi[2]),
                Status::Closed(_) => (t.files_disconnected.to_owned(), theme.ansi[1]),
            };
            ui.label(egui::RichText::new(text).size(12.5).color(color));
            if let Status::Closed(reason) = &self.status {
                if !reason.is_empty() {
                    ui.label(egui::RichText::new("ⓘ").color(theme.ansi[1])).on_hover_text(reason);
                }
                if ui.button(format!("↻  {}", t.reconnect)).clicked() {
                    action = FilesAction::Reconnect;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new(format!("⌨  {}", t.files_terminal)).size(13.0)).clicked() {
                    action = FilesAction::ShowTerminal;
                }
                ui.add_space(6.0);
                if ui.button("↻").on_hover_text(t.files_refresh).clicked() {
                    self.read_local();
                    self.list_remote();
                }
                ui.checkbox(&mut self.show_hidden, egui::RichText::new(t.files_hidden).size(12.5));
            });
        });

        // Panels, with the transfer buttons between them, and the queue below.
        let queue_h = 150.0_f32.min(rect.height() * 0.35);
        let body = Rect::from_min_max(Pos2::new(rect.min.x, bar.max.y), Pos2::new(rect.max.x, rect.max.y - queue_h));
        let middle_w = 110.0;
        let panel_w = (body.width() - middle_w) / 2.0;
        let left = Rect::from_min_size(body.min, Vec2::new(panel_w, body.height()));
        let middle = Rect::from_min_size(Pos2::new(left.max.x, body.min.y), Vec2::new(middle_w, body.height()));
        let right = Rect::from_min_max(Pos2::new(middle.max.x, body.min.y), body.max);

        let connected = matches!(self.status, Status::Ready);
        let local_out = self.panel_ui(ui, left.shrink(6.0), Side::Local, theme, t, connected);
        let remote_out = self.panel_ui(ui, right.shrink(6.0), Side::Remote, theme, t, connected);
        for out in [local_out, remote_out] {
            self.apply(out);
        }

        ui.scope_builder(egui::UiBuilder::new().max_rect(middle.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
            ui.add_space((middle.height() / 2.0 - 50.0).max(0.0));
            let upload = egui::Button::new(egui::RichText::new(format!("{}  →", t.files_upload)).size(13.0)).min_size(Vec2::new(94.0, 30.0));
            if ui.add_enabled(connected && !self.local.selected.is_empty(), upload).on_hover_text(t.files_upload_hint).clicked() {
                self.transfer(Side::Local, self.local.selection(), None);
            }
            ui.add_space(8.0);
            let download = egui::Button::new(egui::RichText::new(format!("←  {}", t.files_download)).size(13.0)).min_size(Vec2::new(94.0, 30.0));
            if ui.add_enabled(connected && !self.remote.selected.is_empty(), download).on_hover_text(t.files_download_hint).clicked() {
                self.transfer(Side::Remote, self.remote.selection(), None);
            }
        });

        let queue = Rect::from_min_max(Pos2::new(rect.min.x, body.max.y), rect.max);
        self.queue_ui(ui, queue, theme, t);
        self.keyboard(ui);
        self.dialog_ui(ui.ctx(), theme, t);
        action
    }

    /// One panel: path bar, column headers, rows. Returns what the user did.
    fn panel_ui(&mut self, ui: &mut Ui, rect: Rect, side: Side, theme: &Theme, t: &Strings, connected: bool) -> PanelOut {
        let mut out = PanelOut::default();
        let show_hidden = self.show_hidden;
        let active = self.active == side;
        let panel = self.panel(side);
        let frame_stroke = Stroke::new(1.0, if active { theme.accent.gamma_multiply(0.5) } else { theme.tab_hover });
        ui.painter().rect_stroke(rect, 6.0, frame_stroke, egui::StrokeKind::Inside);
        let inner = rect.shrink(6.0);

        // Title and path.
        let head = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 52.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            let title = if side == Side::Local { t.files_local } else { t.files_remote };
            ui.label(egui::RichText::new(title.to_uppercase()).size(11.0).strong().color(theme.text_muted));
            ui.horizontal(|ui| {
                if ui.button("↑").on_hover_text(t.files_parent).clicked() {
                    out.go = Some(FileManager::parent_of(side, &panel.path));
                }
                if ui.button("🏠").on_hover_text(t.files_home).clicked() {
                    out.home = true;
                }
                let edit = ui.add(egui::TextEdit::singleline(&mut panel.path_text).font(FontId::monospace(12.5)).desired_width(f32::INFINITY));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    out.go = Some(panel.path_text.trim().to_owned());
                }
            });
        });

        // Column headers.
        let cols = Columns::new(inner.width(), side == Side::Remote);
        let header = Rect::from_min_size(Pos2::new(inner.min.x, head.max.y + 2.0), Vec2::new(inner.width(), 20.0));
        for (by, label, x, w) in [(SortBy::Name, t.files_name, 0.0, cols.name), (SortBy::Size, t.files_size, cols.size_x, cols.size), (SortBy::Modified, t.files_modified, cols.date_x, cols.date), (SortBy::Mode, t.files_mode, cols.mode_x, cols.mode)] {
            if w <= 0.0 {
                continue;
            }
            let r = Rect::from_min_size(header.min + Vec2::new(x, 0.0), Vec2::new(w, header.height()));
            let resp = ui.interact(r, ui.id().with(("files-col", side as u8, by as u8)), Sense::click());
            let arrow = if panel.sort.0 == by { if panel.sort.1 { " ▲" } else { " ▼" } } else { "" };
            ui.painter().text(r.left_center() + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER, format!("{label}{arrow}"), FontId::proportional(11.5), if resp.hovered() { theme.text } else { theme.text_muted });
            if resp.clicked() {
                panel.sort = if panel.sort.0 == by { (by, !panel.sort.1) } else { (by, true) };
            }
        }

        // Rows.
        let list = Rect::from_min_max(Pos2::new(inner.min.x, header.max.y + 2.0), inner.max);
        let rows: Vec<Entry> = panel.visible(show_hidden).into_iter().cloned().collect();
        let loading = panel.loading;
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
        let drag_payload = egui::DragAndDrop::payload::<FilesDrag>(ui.ctx());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| i.pointer.any_released());
        let mut drop_into: Option<String> = None;
        egui::ScrollArea::vertical().id_salt(("files-list", side as u8)).auto_shrink(false).show(&mut list_ui, |ui| {
            if side == Side::Remote && !connected {
                ui.label(egui::RichText::new(t.files_not_connected).size(12.5).color(theme.text_muted));
                return;
            }
            if rows.is_empty() {
                ui.label(egui::RichText::new(if loading { t.files_loading } else { t.files_empty }).size(12.5).color(theme.text_muted));
            }
            let panel = self.panel(side);
            let names: Vec<String> = rows.iter().map(|e| e.name.clone()).collect();
            for entry in &rows {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click_and_drag());
                let selected = panel.selected.contains(&entry.name);
                // A folder of the other panel under a drag: drop into it.
                let drop_target = entry.is_dir && drag_payload.as_ref().is_some_and(|p| p.from != side) && pointer.is_some_and(|p| row.contains(p));
                if drop_target {
                    drop_into = Some(entry.name.clone());
                }
                let fill = if drop_target {
                    theme.accent.gamma_multiply(0.35)
                } else if selected {
                    theme.accent.gamma_multiply(if active { 0.30 } else { 0.18 })
                } else if resp.hovered() {
                    theme.tab_hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(row, 3.0, fill);
                let y = row.center().y;
                let icon = if entry.is_dir { "📁" } else if entry.is_link { "🔗" } else { "📄" };
                let name_rect = Rect::from_min_size(row.min, Vec2::new(cols.name - 6.0, row.height()));
                let mut job = egui::text::LayoutJob::simple_singleline(format!("{icon}  {}", entry.name), FontId::proportional(13.0), theme.text);
                job.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width() - 6.0);
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(Pos2::new(row.min.x + 4.0, y - galley.size().y / 2.0), galley, theme.text);
                let muted = FontId::proportional(12.0);
                if cols.size > 0.0 && !entry.is_dir {
                    ui.painter().text(Pos2::new(row.min.x + cols.size_x + cols.size - 6.0, y), Align2::RIGHT_CENTER, format_size(entry.size, t), muted.clone(), theme.text_muted);
                }
                if cols.date > 0.0 {
                    ui.painter().text(Pos2::new(row.min.x + cols.date_x + 4.0, y), Align2::LEFT_CENTER, format_time(entry.mtime), muted.clone(), theme.text_muted);
                }
                if cols.mode > 0.0 {
                    if let Some(mode) = entry.mode {
                        ui.painter().text(Pos2::new(row.min.x + cols.mode_x + 4.0, y), Align2::LEFT_CENTER, mode_string(mode, entry.is_dir), FontId::monospace(11.5), theme.text_muted);
                    }
                }

                // Selection: click, Cmd+click (toggle), Shift+click (range).
                if resp.clicked() || resp.secondary_clicked() || resp.drag_started() {
                    out.focus = true;
                    let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
                    if shift {
                        let anchor = panel.anchor.as_ref().and_then(|a| names.iter().position(|n| n == a)).unwrap_or(0);
                        let here = names.iter().position(|n| *n == entry.name).unwrap_or(0);
                        let (a, b) = (anchor.min(here), anchor.max(here));
                        panel.selected = names[a..=b].iter().cloned().collect();
                    } else if cmd && resp.clicked() {
                        if !panel.selected.remove(&entry.name) {
                            panel.selected.insert(entry.name.clone());
                        }
                        panel.anchor = Some(entry.name.clone());
                    } else if !selected || resp.clicked() {
                        panel.selected = HashSet::from([entry.name.clone()]);
                        panel.anchor = Some(entry.name.clone());
                    }
                }
                let resp = match &entry.owner {
                    Some(owner) if side == Side::Remote => resp.on_hover_text(format!("{}  ·  {owner}", entry.name)),
                    _ => resp,
                };
                if resp.double_clicked() {
                    if entry.is_dir {
                        out.go = Some(match side {
                            Side::Local => std::path::Path::new(&panel.path).join(&entry.name).display().to_string(),
                            Side::Remote => sftp::join(&panel.path, &entry.name),
                        });
                    } else {
                        out.transfer = Some(vec![entry.name.clone()]);
                    }
                }
                if resp.drag_started() {
                    egui::DragAndDrop::set_payload(ui.ctx(), FilesDrag { from: side, names: panel.selection() });
                }
                let selection = panel.selection();
                resp.context_menu(|ui| {
                    ui.set_min_width(200.0);
                    let other = if side == Side::Local { t.files_upload } else { t.files_download };
                    if ui.add_enabled(connected, egui::Button::new(other)).clicked() {
                        out.transfer = Some(selection.clone());
                        ui.close();
                    }
                    if entry.is_dir && ui.button(t.open).clicked() {
                        out.go = Some(match side {
                            Side::Local => std::path::Path::new(&panel.path).join(&entry.name).display().to_string(),
                            Side::Remote => sftp::join(&panel.path, &entry.name),
                        });
                        ui.close();
                    }
                    ui.separator();
                    if selection.len() == 1 && ui.button(t.rename).clicked() {
                        out.dialog = Some(Dialog::Rename { side, from: entry.name.clone(), text: entry.name.clone(), fresh: true });
                        ui.close();
                    }
                    if side == Side::Remote && ui.button(t.files_permissions).clicked() {
                        let mode = entry.mode.unwrap_or(0o644);
                        let any_dir = rows.iter().any(|e| e.is_dir && selection.contains(&e.name));
                        out.dialog = Some(Dialog::Chmod { names: selection.clone(), mode, octal: format!("{mode:03o}"), recursive: false, any_dir });
                        ui.close();
                    }
                    if ui.button(t.files_new_folder).clicked() {
                        out.dialog = Some(Dialog::Mkdir { side, text: String::new(), fresh: true });
                        ui.close();
                    }
                    if ui.button(t.files_copy_path).clicked() {
                        ui.ctx().copy_text(match side {
                            Side::Local => std::path::Path::new(&panel.path).join(&entry.name).display().to_string(),
                            Side::Remote => sftp::join(&panel.path, &entry.name),
                        });
                        ui.close();
                    }
                    if side == Side::Local && ui.button(t.open_location).clicked() {
                        config::reveal(&std::path::Path::new(&panel.path).join(&entry.name));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(egui::RichText::new(t.delete).color(theme.ansi[1])).clicked() {
                        out.dialog = Some(Dialog::Delete { side, names: selection.clone() });
                        ui.close();
                    }
                });
            }
            // Empty space: clears the selection; right-click for a new folder.
            let rest = ui.allocate_response(Vec2::new(ui.available_width(), ui.available_height().max(40.0)), Sense::click());
            if rest.clicked() {
                self.panel(side).selected.clear();
                out.focus = true;
            }
            rest.context_menu(|ui| {
                if ui.button(t.files_new_folder).clicked() {
                    out.dialog = Some(Dialog::Mkdir { side, text: String::new(), fresh: true });
                    ui.close();
                }
                if ui.button(t.files_refresh).clicked() {
                    out.refresh = true;
                    ui.close();
                }
            });
        });

        // Drop from the other panel: into the folder under the pointer, or this panel's folder.
        let over = pointer.is_some_and(|p| rect.contains(p));
        if let Some(payload) = drag_payload.as_ref().filter(|p| p.from != side && over) {
            if drop_into.is_none() {
                ui.painter().rect_stroke(rect, 6.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
            }
            if released {
                egui::DragAndDrop::clear_payload(ui.ctx());
                let target = drop_into.map(|dir| match side {
                    Side::Local => std::path::Path::new(&self.panel(side).path).join(dir).display().to_string(),
                    Side::Remote => sftp::join(&self.panel(side).path, &dir),
                });
                out.dropped = Some((payload.names.clone(), target));
            }
        }
        out.side = Some(side);
        out
    }

    fn apply(&mut self, out: PanelOut) {
        let Some(side) = out.side else { return };
        if out.focus {
            self.active = side;
        }
        if let Some(path) = out.go.filter(|p| !p.is_empty()) {
            self.open_dir(side, path);
        }
        if out.home {
            let home = match side {
                Side::Local => directories::BaseDirs::new().map(|d| d.home_dir().display().to_string()),
                Side::Remote => None,
            };
            match home {
                Some(home) => self.open_dir(side, home),
                // The remote home is what "." resolves to.
                None => self.open_dir(side, ".".into()),
            }
        }
        if out.refresh {
            match side {
                Side::Local => self.read_local(),
                Side::Remote => self.list_remote(),
            }
        }
        if let Some(names) = out.transfer {
            self.transfer(side, names, None);
        }
        if let Some((names, target)) = out.dropped {
            let from = if side == Side::Local { Side::Remote } else { Side::Local };
            self.transfer(from, names, target);
        }
        if let Some(dialog) = out.dialog {
            self.dialog = Some(dialog);
        }
    }

    /// Delete / F2 / Enter / Cmd+A on the active panel, when no text field has the keyboard.
    fn keyboard(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.dialog.is_some() {
            return;
        }
        let side = self.active;
        let (delete, rename, enter, all) = ui.input_mut(|i| {
            let delete = i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::COMMAND, Key::Backspace);
            (delete, i.consume_key(Modifiers::NONE, Key::F2), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::COMMAND, Key::A))
        });
        let selection = self.panel(side).selection();
        if delete && !selection.is_empty() {
            self.dialog = Some(Dialog::Delete { side, names: selection.clone() });
        }
        if rename && selection.len() == 1 {
            self.dialog = Some(Dialog::Rename { side, from: selection[0].clone(), text: selection[0].clone(), fresh: true });
        }
        if enter && selection.len() == 1 {
            let is_dir = self.panel(side).entries.iter().any(|e| e.name == selection[0] && e.is_dir);
            if is_dir {
                let path = self.path_of(side, &selection[0]);
                self.open_dir(side, path);
            }
        }
        if all {
            let show_hidden = self.show_hidden;
            let panel = self.panel(side);
            panel.selected = panel.visible(show_hidden).into_iter().map(|e| e.name.clone()).collect();
        }
    }

    fn queue_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) {
        ui.painter().rect_filled(rect, 0.0, theme.chrome_bg);
        ui.painter().hline(rect.x_range(), rect.min.y, Stroke::new(1.0, theme.tab_hover));
        let mut cancel = None;
        let mut clear = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(10.0, 6.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.files_transfers.to_uppercase()).size(11.0).strong().color(theme.text_muted));
                if let Some(e) = &self.error {
                    ui.add_space(12.0);
                    ui.add(egui::Label::new(egui::RichText::new(format!("⚠ {e}")).size(12.0).color(theme.ansi[1])).truncate());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let finished = self.transfers.iter().any(|t| matches!(t.state, TransferState::Done | TransferState::Failed(_)));
                    if ui.add_enabled(finished || self.error.is_some(), egui::Button::new(egui::RichText::new(t.files_clear).size(12.0))).clicked() {
                        clear = true;
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt("files-queue").auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                if self.transfers.is_empty() {
                    ui.label(egui::RichText::new(t.files_no_transfer).size(12.0).color(theme.text_muted));
                }
                for tr in &self.transfers {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(if tr.upload { "↑" } else { "↓" }).size(14.0).color(theme.accent));
                        ui.add_sized(Vec2::new(200.0, 18.0), egui::Label::new(egui::RichText::new(&tr.label).size(12.5)).truncate());
                        let fraction = if tr.total > 0 { tr.done as f32 / tr.total as f32 } else { 0.0 };
                        let secs = tr.started.elapsed().as_secs_f64().max(0.1);
                        let text = match &tr.state {
                            TransferState::Queued => t.files_queued.to_owned(),
                            TransferState::Running => format!("{} / {}  ·  {}/s", format_size(tr.done, t), format_size(tr.total, t), format_size((tr.done as f64 / secs) as u64, t)),
                            TransferState::Done => format!("✔  {}", format_size(tr.total, t)),
                            TransferState::Failed(e) if e == "cancelled" => t.files_cancelled.to_owned(),
                            TransferState::Failed(e) => format!("✖  {e}"),
                        };
                        let color = match tr.state {
                            TransferState::Failed(_) => theme.ansi[1],
                            TransferState::Done => theme.ansi[2],
                            _ => theme.accent,
                        };
                        let bar_w = (ui.available_width() - 40.0).max(80.0);
                        let bar = egui::ProgressBar::new(if tr.state == TransferState::Done { 1.0 } else { fraction }).text(egui::RichText::new(text).size(11.5)).fill(color.gamma_multiply(0.6)).desired_width(bar_w);
                        ui.add(bar).on_hover_text(&tr.current);
                        if matches!(tr.state, TransferState::Queued | TransferState::Running) && ui.small_button("✕").on_hover_text(t.cancel).clicked() {
                            cancel = Some(tr.id);
                        }
                    });
                }
            });
        });
        if let Some(id) = cancel {
            self.send(Request::Cancel(id));
        }
        if clear {
            self.transfers.retain(|t| matches!(t.state, TransferState::Queued | TransferState::Running));
            self.error = None;
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, theme: &Theme, t: &Strings) {
        let Some(dialog) = &mut self.dialog else { return };
        let mut outcome: Option<Outcome> = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("files-dialog")).frame(frame).show(ctx, |ui| {
            ui.set_width(420.0);
            let title = |ui: &mut Ui, text: &str| {
                ui.label(egui::RichText::new(text).size(16.0).strong());
                ui.add_space(6.0);
            };
            let list = |ui: &mut Ui, names: &[String]| {
                for name in names.iter().take(8) {
                    ui.label(egui::RichText::new(format!("•  {name}")).monospace().size(12.5));
                }
                if names.len() > 8 {
                    ui.label(format!("…  +{}", names.len() - 8));
                }
            };
            if let Some(heading) = match dialog {
                Dialog::Rename { .. } => Some(t.rename),
                Dialog::Mkdir { .. } => Some(t.files_new_folder),
                _ => None,
            } {
                title(ui, heading);
            }
            match dialog {
                Dialog::Rename { text, fresh, .. } | Dialog::Mkdir { text, fresh, .. } => {
                    let edit = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)));
                    if *fresh {
                        edit.request_focus();
                        *fresh = false;
                    }
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        outcome = Some(Outcome::Confirm);
                    }
                }
                Dialog::Delete { names, .. } => {
                    title(ui, &t.files_delete_title.replace("{n}", &names.len().to_string()));
                    list(ui, names);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t.files_delete_body).size(12.5).color(theme.text_muted));
                }
                Dialog::Chmod { names, mode, octal, recursive, any_dir } => {
                    title(ui, t.files_permissions);
                    let what = if names.len() == 1 { names[0].clone() } else { t.files_n_items.replace("{n}", &names.len().to_string()) };
                    ui.label(egui::RichText::new(what).monospace().size(12.5).color(theme.text_muted));
                    ui.add_space(8.0);
                    egui::Grid::new("chmod-grid").num_columns(4).spacing([18.0, 8.0]).show(ui, |ui| {
                        ui.label("");
                        for h in [t.files_read, t.files_write, t.files_exec] {
                            ui.label(egui::RichText::new(h).size(12.5).color(theme.text_muted));
                        }
                        ui.end_row();
                        for (who, shift) in [(t.files_owner, 6), (t.files_group, 3), (t.files_others, 0)] {
                            ui.label(egui::RichText::new(who).size(13.0));
                            for bit in [4u32, 2, 1] {
                                let flag = bit << shift;
                                let mut on = *mode & flag != 0;
                                if ui.checkbox(&mut on, "").changed() {
                                    *mode = if on { *mode | flag } else { *mode & !flag };
                                    *octal = format!("{:03o}", *mode & 0o777);
                                }
                            }
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(t.files_octal).size(13.0));
                        if ui.add(egui::TextEdit::singleline(octal).desired_width(60.0).font(FontId::monospace(13.0))).changed() {
                            if let Some(value) = u32::from_str_radix(octal.trim(), 8).ok().filter(|v| *v <= 0o7777) {
                                *mode = value;
                            }
                        }
                        ui.label(egui::RichText::new(mode_string(*mode, false)).monospace().size(12.5).color(theme.text_muted));
                    });
                    if *any_dir {
                        ui.checkbox(recursive, egui::RichText::new(t.files_recursive).size(13.0));
                    }
                }
                Dialog::Conflict { existing, .. } => {
                    title(ui, &t.files_conflict_title.replace("{n}", &existing.len().to_string()));
                    list(ui, existing);
                }
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = |text: &str, fill: Option<Color32>| {
                    let label = egui::RichText::new(text).size(13.5);
                    let label = if fill.is_some() { label.color(theme.bg) } else { label };
                    let b = egui::Button::new(label).corner_radius(6.0).min_size(Vec2::new(96.0, 30.0));
                    match fill {
                        Some(fill) => b.fill(fill),
                        None => b,
                    }
                };
                match dialog {
                    Dialog::Conflict { .. } => {
                        if ui.add(button(t.files_overwrite, Some(theme.accent))).clicked() {
                            outcome = Some(Outcome::Confirm);
                        }
                        if ui.add(button(t.files_skip, None)).on_hover_text(t.files_skip_hint).clicked() {
                            outcome = Some(Outcome::Skip);
                        }
                    }
                    Dialog::Delete { .. } => {
                        if ui.add(button(t.delete, Some(theme.ansi[1]))).clicked() {
                            outcome = Some(Outcome::Confirm);
                        }
                    }
                    _ => {
                        if ui.add(button(t.confirm, Some(theme.accent))).clicked() {
                            outcome = Some(Outcome::Confirm);
                        }
                    }
                }
                if ui.add(button(t.cancel, None)).clicked() {
                    outcome = Some(Outcome::Cancel);
                }
            });
        });
        if modal.should_close() {
            outcome.get_or_insert(Outcome::Cancel);
        }
        let Some(outcome) = outcome else { return };
        let Some(dialog) = self.dialog.take() else { return };
        match (dialog, outcome) {
            (_, Outcome::Cancel) => {}
            (Dialog::Rename { side, from, text, .. }, _) => {
                let to = text.trim();
                if !to.is_empty() && to != from && !to.contains('/') {
                    self.rename(side, &from, to);
                }
            }
            (Dialog::Mkdir { side, text, .. }, _) => {
                let name = text.trim();
                if !name.is_empty() && !name.contains('/') {
                    self.mkdir(side, name);
                }
            }
            (Dialog::Delete { side, names }, _) => self.delete(side, &names),
            (Dialog::Chmod { names, mode, recursive, .. }, _) => {
                let paths = names.iter().map(|n| sftp::join(&self.remote.path, n)).collect();
                self.send(Request::Chmod { paths, mode, recursive });
            }
            (Dialog::Conflict { from, names, target, .. }, outcome) => self.start_transfer(from, names, target, outcome == Outcome::Confirm),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Confirm,
    /// Conflict: transfer, but keep the existing files.
    Skip,
    Cancel,
}

impl App {
    /// Shows the file manager of SSH tab `index` (connecting it the first time), or its terminal again.
    pub(super) fn toggle_files(&mut self, index: usize, show: bool) {
        let Some(host) = self.tabs.get(index).and_then(|t| t.ssh).and_then(|id| self.config.ssh.iter().find(|h| h.id == id)).cloned() else { return };
        let ctx = self.ctx.clone();
        #[cfg(unix)]
        let askpass = self.askpass.clone();
        let tab = &mut self.tabs[index];
        tab.show_files = show;
        if show && tab.files.is_none() {
            let mut fm = Box::new(FileManager::new(&host));
            fm.connect(&ctx, &host, #[cfg(unix)] &askpass);
            tab.files = Some(fm);
        }
        if !show {
            self.focus_terminal = true;
        }
    }

    /// Lets every file manager handle its events (transfers go on in background tabs), and remembers
    /// each host's folders.
    pub(super) fn poll_files(&mut self) {
        for tab in &mut self.tabs {
            let Some(fm) = &mut tab.files else { continue };
            fm.poll();
            let (local, remote) = fm.folders();
            if let Some(host) = self.config.ssh.iter_mut().find(|h| h.id == fm.host) {
                if host.sftp_local.as_deref() != Some(&local) {
                    host.sftp_local = Some(local);
                }
                if remote.is_some() && host.sftp_remote != remote {
                    host.sftp_remote = remote;
                }
            }
        }
    }

    /// The active tab's file manager, filling `rect`.
    pub(super) fn files_view(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let index = self.active;
        let Some(tab) = self.tabs.get_mut(index) else { return };
        let host_name = tab.title().to_owned();
        let Some(fm) = &mut tab.files else { return };
        match fm.ui(ui, rect, &self.theme, t, &host_name) {
            FilesAction::None => {}
            FilesAction::ShowTerminal => self.toggle_files(index, false),
            FilesAction::Reconnect => {
                if let Some(host) = self.config.ssh.iter().find(|h| Some(h.id) == self.tabs[index].ssh).cloned() {
                    #[cfg(unix)]
                    let askpass = self.askpass.clone();
                    if let Some(fm) = &mut self.tabs[index].files {
                        fm.connect(&self.ctx, &host, #[cfg(unix)] &askpass);
                    }
                }
            }
        }
    }

    /// A question from an ssh without terminal (SFTP): password or new host key, answered here.
    pub(super) fn ssh_prompt_window(&mut self, ctx: &egui::Context) {
        #[cfg(unix)]
        {
            if self.ssh_prompt.is_none() {
                self.ssh_prompt = self.ssh_prompts.try_recv().ok().map(|p| (p, String::new()));
            }
            let Some((prompt, answer)) = &mut self.ssh_prompt else { return };
            let t = self.config.settings.language.strings();
            let mut done: Option<Option<String>> = None;
            let yes_no = !prompt.secret;
            let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
            let modal = egui::Modal::new(egui::Id::new("ssh-prompt")).frame(frame).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(egui::RichText::new(format!("🔑  {}", t.ssh_prompt_title)).size(16.0).strong());
                ui.add_space(8.0);
                ui.add(egui::Label::new(egui::RichText::new(&prompt.text).monospace().size(12.5)).wrap());
                ui.add_space(10.0);
                if yes_no {
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("yes").monospace()).clicked() {
                            done = Some(Some("yes".into()));
                        }
                        if ui.button(egui::RichText::new("no").monospace()).clicked() {
                            done = Some(Some("no".into()));
                        }
                    });
                } else {
                    let edit = ui.add(egui::TextEdit::singleline(answer).password(true).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)));
                    edit.request_focus();
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(Some(answer.clone()));
                    }
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !yes_no && ui.button(t.confirm).clicked() {
                        done = Some(Some(answer.clone()));
                    }
                    if ui.button(t.cancel).clicked() {
                        done = Some(None);
                    }
                });
            });
            if modal.should_close() {
                done.get_or_insert(None);
            }
            if let Some(reply) = done {
                if let Some((prompt, _)) = self.ssh_prompt.take() {
                    let _ = prompt.reply.send(reply);
                }
            }
        }
        #[cfg(not(unix))]
        let _ = ctx;
    }
}

/// What a panel reported this frame.
#[derive(Default)]
struct PanelOut {
    side: Option<Side>,
    focus: bool,
    go: Option<String>,
    home: bool,
    refresh: bool,
    transfer: Option<Vec<String>>,
    /// Names dragged from the other panel, and the folder dropped onto (None: this panel's folder).
    dropped: Option<(Vec<String>, Option<String>)>,
    dialog: Option<Dialog>,
}

/// Column positions of a panel of `width`: narrow panels drop the date, then the permissions.
struct Columns {
    name: f32,
    size_x: f32,
    size: f32,
    date_x: f32,
    date: f32,
    mode_x: f32,
    mode: f32,
}

impl Columns {
    fn new(width: f32, with_mode: bool) -> Self {
        let size = 80.0;
        let date = if width > 460.0 { 125.0 } else { 0.0 };
        let mode = if with_mode && width > 560.0 { 95.0 } else { 0.0 };
        let name = (width - size - date - mode).max(80.0);
        Self { name, size_x: name, size, date_x: name + size, date, mode_x: name + size + date, mode }
    }
}

fn local_entry(entry: &std::fs::DirEntry) -> Option<Entry> {
    let meta = std::fs::symlink_metadata(entry.path()).ok()?;
    let is_link = meta.file_type().is_symlink();
    let target = if is_link { std::fs::metadata(entry.path()).ok() } else { None };
    let is_dir = target.as_ref().map_or(meta.is_dir(), |m| m.is_dir());
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let mode = None;
    Some(Entry {
        name: entry.file_name().to_string_lossy().into_owned(),
        is_dir,
        is_link,
        size: meta.len(),
        mtime: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
        mode,
        owner: None,
    })
}

/// "12,3 Mo" / "12.3 MB".
fn format_size(bytes: u64, t: &Strings) -> String {
    let units = [t.unit_b, t.unit_kb, t.unit_mb, t.unit_gb, t.unit_tb];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if unit == 0 { format!("{bytes}") } else { format!("{value:.1}") };
    let text = if t.decimal_comma { text.replace('.', ",") } else { text };
    format!("{text} {}", units[unit])
}

fn format_time(mtime: Option<i64>) -> String {
    use chrono::TimeZone as _;
    mtime.and_then(|s| chrono::Local.timestamp_opt(s, 0).single()).map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

/// "drwxr-xr-x" style.
fn mode_string(mode: u32, is_dir: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_modes_and_sizes() {
        assert_eq!(mode_string(0o755, true), "drwxr-xr-x");
        assert_eq!(mode_string(0o640, false), "-rw-r-----");
        let fr = crate::i18n::Lang::Fr.strings();
        assert_eq!(format_size(512, fr), "512 o");
        assert_eq!(format_size(1536, fr), "1,5 Ko");
        assert_eq!(format_size(5 * 1024 * 1024, crate::i18n::Lang::En.strings()), "5.0 MB");
    }
}
