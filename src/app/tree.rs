//! The sidebar's "Files" category: this computer's folders as a tree from a root folder (as VS Code's
//! explorer), with a search through it. Text files open in Ronnie's editor, documents with their program.

use super::files::{FileManager, OpenWith, TreeOp};
use super::sidebar::paint_row_bg;
use super::*;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const TREE_ROW_H: f32 = 24.0;
const INDENT: f32 = 12.0;
/// A search stops there.
const MAX_RESULTS: usize = 500;
/// Folders listed again after that long, to show what changed on disk.
const RELIST: Duration = Duration::from_secs(3);
/// The search starts once typing pauses that long.
const TYPING_PAUSE: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct Item {
    name: String,
    is_dir: bool,
    is_link: bool,
}

#[derive(Default)]
pub(super) struct FileTree {
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, (Instant, Vec<Item>)>,
    /// The item last clicked (where a Shift+click range starts).
    selected: Option<PathBuf>,
    /// The items selected (⌘ click, Shift+click): what a right click or a drag acts on.
    picked: HashSet<PathBuf>,
    /// The rows in the order drawn: this frame's, and the last one's (for Shift+click).
    order: Vec<PathBuf>,
    last_order: Vec<PathBuf>,
    /// The folder under what is dragged (from the tree or the Finder), this frame.
    drop_dir: Option<PathBuf>,
    /// A file manager of this computer, out of sight, doing what the menus ask (moving, archives,
    /// permissions...) with its dialogs.
    ops: Option<Box<FileManager>>,
    /// Its readings of folders when last looked: the tree lists them again when they change.
    ops_readings: u64,
    /// The selected row brought into view on the next frame.
    reveal: bool,
    query: String,
    /// When the query last changed: searched once typing pauses.
    typed: Option<Instant>,
    search: Option<Search>,
    /// A name typed in place.
    naming: Option<Naming>,
    error: Option<String>,
    /// The folder searched, picked by a right click (None: the root).
    scope: Option<PathBuf>,
    /// The search field takes the keyboard on the next frame.
    focus_search: bool,
}

/// A search through the folders under the root, on its own thread.
struct Search {
    query: String,
    root: PathBuf,
    found: Arc<Mutex<Vec<(PathBuf, bool)>>>,
    done: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NamingKind {
    NewFile,
    NewFolder,
    Rename,
}

/// A new file or folder in folder `path`, or item `path` renamed.
struct Naming {
    kind: NamingKind,
    path: PathBuf,
    text: String,
    fresh: bool,
}

/// What was clicked in the tree.
/// Items of the tree dragged.
struct TreeDrag(Vec<PathBuf>);

#[derive(Clone, Copy)]
enum Pick {
    Only,
    /// ⌘ click.
    Toggle,
    /// Shift+click.
    Range,
}

enum TreeAction {
    Select(PathBuf, Pick),
    Op(TreeOp),
    Toggle(PathBuf),
    /// Folders unfold; text files open in the editor, the others with their program.
    Open(PathBuf),
    Launch(PathBuf, OpenWith),
    Edit(PathBuf),
    Terminal(PathBuf),
    SearchIn(Option<PathBuf>),
    SetRoot(Option<PathBuf>),
    PickRoot,
    RootFromTerminal,
    New(PathBuf, NamingKind),
    Rename(PathBuf),
    ShowInTree(PathBuf),
    Refresh,
    CollapseAll,
    ToggleHidden,
}

fn home_dir() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("/"))
}

/// The items of `dir`: folders first, then by name (case aside).
fn read_items(dir: &Path, hidden: bool) -> Vec<Item> {
    let mut items: Vec<Item> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let is_link = entry.file_type().is_ok_and(|t| t.is_symlink());
            // A link to a folder unfolds as one.
            let is_dir = entry.path().is_dir();
            Some(Item { name, is_dir, is_link })
        })
        .collect();
    items.sort_by_cached_key(|i| (!i.is_dir, i.name.to_lowercase()));
    items
}

/// Folders a search doesn't go into: too big, and never what is looked for.
fn skipped(name: &str, hidden: bool) -> bool {
    matches!(name, ".git" | "node_modules") || (!hidden && name.starts_with('.'))
}

fn start_search(root: PathBuf, query: String, hidden: bool, ctx: egui::Context) -> Search {
    let found = Arc::new(Mutex::new(Vec::new()));
    let (done, stop) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_owned).collect();
    let (out, finished, stopped, from) = (found.clone(), done.clone(), stop.clone(), root.clone());
    std::thread::spawn(move || {
        // Breadth first: the nearest first.
        let mut queue = std::collections::VecDeque::from([from]);
        let mut last_wake = Instant::now();
        'walk: while let Some(dir) = queue.pop_front() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if skipped(&name, hidden) {
                    continue;
                }
                let lower = name.to_lowercase();
                if words.iter().all(|w| lower.contains(w.as_str())) {
                    let mut found = out.lock().unwrap_or_else(|e| e.into_inner());
                    found.push((entry.path(), is_dir));
                    if found.len() >= MAX_RESULTS {
                        break 'walk;
                    }
                }
                if is_dir {
                    queue.push_back(entry.path());
                }
            }
            if last_wake.elapsed() > Duration::from_millis(150) {
                last_wake = Instant::now();
                ctx.request_repaint();
            }
        }
        finished.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    });
    Search { query, root, found, done, stop }
}

impl FileTree {
    fn items(&mut self, dir: &Path, hidden: bool) -> Vec<Item> {
        match self.listings.get(dir) {
            Some((at, items)) if at.elapsed() < RELIST => items.clone(),
            _ => {
                let items = read_items(dir, hidden);
                self.listings.insert(dir.to_path_buf(), (Instant::now(), items.clone()));
                items
            }
        }
    }

    /// Unfolds the folders down to `path`, selects it and brings it into view.
    fn show(&mut self, root: &Path, path: &Path) {
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(root) && *d != root) {
            self.expanded.insert(d.to_path_buf());
            dir = d.parent();
        }
        self.selected = Some(path.to_path_buf());
        self.reveal = true;
    }
}

/// Two arrows around a circle: refresh.
fn paint_refresh(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let r = 5.0;
    let points: Vec<Pos2> = (0..=20).map(|k| {
        let a = 0.6 + k as f32 / 20.0 * 5.0;
        c + Vec2::new(a.cos(), a.sin()) * r
    }).collect();
    let end = *points.last().unwrap_or(&c);
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment([end, end + Vec2::new(3.0, -0.5)], stroke);
    painter.line_segment([end, end + Vec2::new(0.5, 3.0)], stroke);
}

/// A box with a dash: fold every folder.
fn paint_collapse(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.rect_stroke(Rect::from_center_size(c, Vec2::splat(11.0)), 2.0, stroke, egui::StrokeKind::Middle);
    painter.line_segment([c - Vec2::new(3.0, 0.0), c + Vec2::new(3.0, 0.0)], stroke);
}

/// Three dots: more.
fn paint_dots(painter: &egui::Painter, c: Pos2, color: Color32) {
    for dx in [-4.5, 0.0, 4.5] {
        painter.circle_filled(c + Vec2::new(dx, 0.0), 1.3, color);
    }
}

impl App {
    /// The folder the tree starts from.
    fn tree_root(&self) -> PathBuf {
        self.config.settings.files_root.as_ref().map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(home_dir)
    }

    /// The local terminal in front: its folder.
    fn terminal_dir(&self) -> Option<PathBuf> {
        let tab = self.tabs.get(self.shown_tab()?).filter(|t| t.ssh.is_none())?;
        tab.panes.get(&tab.focused).and_then(Terminal::cwd)
    }

    /// The "Files" category: the root folder's name and its buttons, the search field, then the tree
    /// (or what the search found).
    pub(super) fn files_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let root = self.tree_root();
        let hidden = self.config.settings.files_hidden;
        let painter = ui.painter().clone();
        let mut action = None;
        self.tree.last_order = std::mem::take(&mut self.tree.order);
        self.tree.drop_dir = None;
        // Files from the Finder: the system doesn't move the pointer meanwhile, asked each frame.
        let (os_hover, os_dropped) = ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>()));
        if os_hover {
            ui.ctx().request_repaint();
        }
        let list_top = *y;

        // The buttons, on the right.
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        let button = |k: f32| Rect::from_center_size(Pos2::new(header.max.x - 12.0 - 22.0 * k, header.center().y), Vec2::splat(20.0));
        let more = icon_button(ui, &painter, button(0.0), "tree-more", &theme, paint_dots);
        egui::Popup::menu(&more).width(220.0).show(|ui| {
            menu_item(ui, &format!("📁  {}", t.tree_choose_root), TreeAction::PickRoot, &mut action);
            if self.terminal_dir().is_some() {
                menu_item(ui, &format!(">_  {}", t.tree_terminal_root), TreeAction::RootFromTerminal, &mut action);
            }
            menu_item(ui, &format!("⌂  {}", t.tree_home_root), TreeAction::SetRoot(None), &mut action);
            ui.separator();
            let mut on = hidden;
            if ui.checkbox(&mut on, t.files_hidden).clicked() {
                action = Some(TreeAction::ToggleHidden);
                ui.close();
            }
        });
        if icon_button(ui, &painter, button(1.0), "tree-collapse", &theme, paint_collapse).on_hover_text(t.tree_collapse).clicked() {
            action = Some(TreeAction::CollapseAll);
        }
        if icon_button(ui, &painter, button(2.0), "tree-refresh", &theme, paint_refresh).on_hover_text(t.files_refresh).clicked() {
            action = Some(TreeAction::Refresh);
        }
        let plus = icon_button(ui, &painter, button(3.0), "tree-plus", &theme, paint_plus);
        // In the folder selected (or the one of the file selected), else the root.
        let target = match &self.tree.selected {
            Some(p) if p.is_dir() => p.clone(),
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or_else(|| root.clone()),
            None => root.clone(),
        };
        egui::Popup::menu(&plus).width(180.0).show(|ui| {
            menu_item(ui, t.files_new_file, TreeAction::New(target.clone(), NamingKind::NewFile), &mut action);
            menu_item(ui, t.files_new_folder, TreeAction::New(target.clone(), NamingKind::NewFolder), &mut action);
        });
        *y += SECTION_HEADER_H;

        // The search field.
        let field = Rect::from_min_size(Pos2::new(left + 4.0, *y), Vec2::new(row_w - 8.0, 26.0));
        let search_id = ui.id().with("tree-search");
        // Searched in a folder picked, while it is there.
        let scope = self.tree.scope.clone().filter(|d| d.is_dir() && d.starts_with(&root));
        let scope_name = scope.as_ref().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned());
        let hint = match &scope_name {
            Some(name) => t.tree_search_in.replace("{name}", name),
            None => t.tree_search.to_owned(),
        };
        let edit = ui.put(
            field,
            egui::TextEdit::singleline(&mut self.tree.query).id(search_id).hint_text(format!("🔍  {hint}")).font(FontId::proportional(13.0)).margin(Vec2::new(8.0, 4.0)),
        );
        if std::mem::take(&mut self.tree.focus_search) {
            edit.request_focus();
        }
        if edit.changed() {
            self.tree.typed = Some(Instant::now());
        }
        if edit.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            // Once empty, the root again.
            if self.tree.query.is_empty() {
                action = Some(TreeAction::SearchIn(None));
            }
            self.tree.query.clear();
            self.tree.search = None;
        }
        *y += 26.0 + 6.0;
        // The folder searched, and a cross back to the whole tree.
        if let (Some(dir), Some(name)) = (&scope, &scope_name) {
            let chip = Rect::from_min_size(Pos2::new(left + 4.0, *y), Vec2::new(row_w - 8.0, 20.0));
            let text_rect = Rect::from_min_max(chip.min, Pos2::new(chip.max.x - 24.0, chip.max.y));
            let mut job = egui::text::LayoutJob::simple_singleline(format!("📁  {}", t.tree_searching_in.replace("{name}", name)), FontId::proportional(11.5), theme.accent);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width() - 8.0);
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(chip.min.x + 4.0, chip.center().y - galley.size().y / 2.0), galley, theme.accent);
            ui.interact(text_rect, ui.id().with("tree-scope"), Sense::hover()).on_hover_text(dir.display().to_string());
            let close = Rect::from_center_size(Pos2::new(chip.max.x - 10.0, chip.center().y), Vec2::splat(18.0));
            if icon_button(ui, &painter, close, "tree-scope-close", &theme, paint_cross).on_hover_text(t.tree_search_everywhere).clicked() {
                action = Some(TreeAction::SearchIn(None));
            }
            *y += 20.0 + 6.0;
        }
        // Archives and copies going on in the background.
        if self.tree.ops.as_ref().is_some_and(|fm| fm.working()) {
            painter.text(Pos2::new(left + 6.0, *y + 8.0), Align2::LEFT_CENTER, format!("⏳  {}", t.tree_working), FontId::proportional(11.5), theme.text_muted);
            *y += 16.0 + 6.0;
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if let Some(e) = &self.tree.error {
            let galley = painter.layout(format!("⚠ {e}"), FontId::proportional(11.5), theme.ansi[1], row_w - 8.0);
            painter.galley(Pos2::new(left + 4.0, *y), galley.clone(), theme.ansi[1]);
            *y += galley.size().y + 6.0;
        }

        let query = self.tree.query.trim().to_owned();
        if query.is_empty() {
            self.tree.search = None;
            self.tree_rows(ui, &root, left, row_w, y, &mut action);
        } else {
            let base = scope.unwrap_or_else(|| root.clone());
            self.search_rows(ui, &base, &query, left, row_w, y, &mut action);
        }
        // Dropped from the Finder onto a folder (or anywhere else in the list: the root): copied there.
        let list = Rect::from_min_max(Pos2::new(left, list_top), Pos2::new(left + row_w, (*y).max(ui.clip_rect().max.y)));
        if !os_dropped.is_empty() && super::files::drag_pointer(ui.ctx()).is_some_and(|p| list.contains(p)) {
            let dir = self.tree.drop_dir.clone().unwrap_or_else(|| root.clone());
            action = Some(TreeAction::Op(TreeOp::CopyInto(os_dropped, dir)));
        }
        // Items of the tree dragged out of the window: to the Finder or another app.
        if let Some(payload) = egui::DragAndDrop::payload::<TreeDrag>(ui.ctx()) {
            let window = ui.ctx().content_rect();
            let outside = ui.input(|i| i.pointer.latest_pos()).is_none_or(|p| !window.shrink(2.0).contains(p));
            if outside && crate::dragout::button_down() {
                egui::DragAndDrop::clear_payload(ui.ctx());
                ui.ctx().stop_dragging();
                crate::dragout::start_files(&payload.0);
            }
        }
        if let Some(action) = action {
            self.tree_action(ui.ctx(), action, &root);
        }
    }

    /// The tree under `root`: unfolded folders with their items, row after row.
    fn tree_rows(&mut self, ui: &mut Ui, root: &Path, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TreeAction>) {
        let hidden = self.config.settings.files_hidden;
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let visible = ui.clip_rect();
        // A new item typed at the top of the root.
        if self.tree.naming.as_ref().is_some_and(|n| n.kind != NamingKind::Rename && n.path == root) {
            self.naming_row(ui, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, TREE_ROW_H)), 0);
            *y += TREE_ROW_H;
        }
        let mut stack: Vec<(PathBuf, Item, usize)> = self.tree.items(root, hidden).into_iter().rev().map(|i| (root.join(&i.name), i, 0)).collect();
        if stack.is_empty() {
            painter.text(Pos2::new(left + 12.0, *y + TREE_ROW_H / 2.0), Align2::LEFT_CENTER, self.t().tree_empty, FontId::proportional(12.5), theme.text_muted);
            *y += TREE_ROW_H;
        }
        while let Some((path, item, depth)) = stack.pop() {
            let row = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, TREE_ROW_H));
            *y += TREE_ROW_H;
            let open = item.is_dir && self.tree.expanded.contains(&path);
            if open {
                let children = self.tree.items(&path, hidden);
                stack.extend(children.into_iter().rev().map(|i| (path.join(&i.name), i, depth + 1)));
            }
            // Off screen: only its place.
            if row.intersects(visible) {
                self.tree_row(ui, &painter, row, &path, &item, depth, open, None, action);
            }
            // A new item typed in this folder: first in it.
            if open && self.tree.naming.as_ref().is_some_and(|n| n.kind != NamingKind::Rename && n.path == path) {
                self.naming_row(ui, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, TREE_ROW_H)), depth + 1);
                *y += TREE_ROW_H;
            }
        }
    }

    /// What the search found: names, with their folder (from the root) beside them.
    #[allow(clippy::too_many_arguments)]
    fn search_rows(&mut self, ui: &mut Ui, root: &Path, query: &str, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TreeAction>) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let stale = self.tree.search.as_ref().is_none_or(|s| s.query != query || s.root != root);
        if stale {
            // Once typing pauses.
            match self.tree.typed {
                Some(at) if at.elapsed() < TYPING_PAUSE => ui.ctx().request_repaint_after(TYPING_PAUSE - at.elapsed()),
                _ => self.tree.search = Some(start_search(root.to_path_buf(), query.to_owned(), self.config.settings.files_hidden, ui.ctx().clone())),
            }
        }
        let Some(search) = &self.tree.search else { return };
        let found = search.found.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let done = search.done.load(Ordering::Relaxed);
        let note = if !done {
            Some(t.tree_searching.to_owned())
        } else if found.is_empty() {
            Some(t.tree_no_result.to_owned())
        } else if found.len() >= MAX_RESULTS {
            Some(t.tree_first_results.replace("{n}", &MAX_RESULTS.to_string()))
        } else {
            None
        };
        let visible = ui.clip_rect();
        for (path, is_dir) in found {
            let row = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, TREE_ROW_H));
            *y += TREE_ROW_H;
            if !row.intersects(visible) {
                continue;
            }
            let item = Item { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), is_dir, is_link: false };
            let folder = path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.display().to_string()).unwrap_or_default();
            self.tree_row(ui, &painter, row, &path, &item, 0, false, Some(&folder), action);
        }
        if let Some(note) = note {
            painter.text(Pos2::new(left + 12.0, *y + TREE_ROW_H / 2.0), Align2::LEFT_CENTER, note, FontId::proportional(12.0), theme.text_muted);
            *y += TREE_ROW_H;
        }
    }

    /// A row of the tree (or of the search, with the item's folder).
    #[allow(clippy::too_many_arguments)]
    fn tree_row(&mut self, ui: &mut Ui, painter: &egui::Painter, row: Rect, path: &Path, item: &Item, depth: usize, open: bool, folder: Option<&str>, action: &mut Option<TreeAction>) {
        let t = self.t();
        let theme = self.theme.clone();
        let resp = ui.interact(row, ui.id().with(("tree-row", path)), Sense::click_and_drag());
        self.tree.order.push(path.to_path_buf());
        let selected = self.tree.picked.contains(path) || (self.tree.picked.is_empty() && self.tree.selected.as_deref() == Some(path));
        // Where what is dragged would go: this folder, or the one this file is in.
        let into = if item.is_dir { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
        let dragged = egui::DragAndDrop::payload::<TreeDrag>(ui.ctx());
        let os_hover = ui.input(|i| !i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty());
        let pointer = if os_hover { super::files::drag_pointer(ui.ctx()) } else { ui.input(|i| i.pointer.interact_pos()) };
        // Not into itself, nor one of its own folders.
        let fits = dragged.as_ref().is_none_or(|d| !d.0.iter().any(|p| into.starts_with(p) || p.parent() == Some(into.as_path())));
        let target = (dragged.is_some() || os_hover) && fits && pointer.is_some_and(|p| row.contains(p));
        if target {
            self.tree.drop_dir = Some(into.clone());
        }
        if selected && self.tree.reveal {
            self.tree.reveal = false;
            ui.scroll_to_rect(row, Some(egui::Align::Center));
        }
        if selected {
            painter.rect_filled(row, 5.0, theme.accent.gamma_multiply(0.18));
        } else {
            paint_row_bg(painter, row, false, resp.hovered(), &theme);
        }
        if target {
            painter.rect_stroke(row, 5.0, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
        }
        // Dropped here: moved into the folder (copied with ⌥).
        if let Some(payload) = resp.dnd_release_payload::<TreeDrag>()
            && fits
        {
            let copy = ui.input(|i| i.modifiers.alt);
            let paths = payload.0.clone();
            *action = Some(TreeAction::Op(if copy { TreeOp::CopyInto(paths, into.clone()) } else { TreeOp::MoveInto(paths, into.clone()) }));
        }
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), TreeDrag(self.tree_targets(path)));
        }
        // The guides of the folders it is in.
        for level in 0..depth {
            let x = row.min.x + 12.0 + level as f32 * INDENT;
            painter.vline(x, row.y_range(), Stroke::new(1.0, theme.tab_hover));
        }
        let x = row.min.x + 6.0 + depth as f32 * INDENT;
        if item.is_dir && folder.is_none() {
            paint_chevron(painter, Pos2::new(x + 6.0, row.center().y), open, theme.text_muted);
        }
        let icon = Rect::from_center_size(Pos2::new(x + 20.0, row.center().y), Vec2::splat(14.0));
        super::files::paint_file_icon(painter, icon, &item.name, item.is_dir, item.is_link, None, &theme);
        let text_left = x + 32.0;
        let renaming = self.tree.naming.as_ref().is_some_and(|n| n.kind == NamingKind::Rename && n.path == path);
        if renaming {
            let field = Rect::from_min_max(Pos2::new(text_left - 4.0, row.min.y + 1.0), Pos2::new(row.max.x - 2.0, row.max.y - 1.0));
            self.naming_field(ui, field);
        } else {
            let hidden = item.name.starts_with('.');
            let color = if selected { theme.text } else if hidden { theme.text_muted.gamma_multiply(0.8) } else { theme.text_muted.gamma_multiply(1.15) };
            let mut job = egui::text::LayoutJob::default();
            job.append(&item.name, 0.0, egui::TextFormat { font_id: FontId::proportional(13.0), color, ..Default::default() });
            if let Some(folder) = folder.filter(|f| !f.is_empty()) {
                job.append(folder, 8.0, egui::TextFormat { font_id: FontId::proportional(11.0), color: theme.text_muted.gamma_multiply(0.7), ..Default::default() });
            }
            // A folder's size, once counted.
            let size = self.tree.ops.as_ref().and_then(|fm| fm.local_size(path)).map(|s| match s {
                None => "…".to_owned(),
                Some(Ok(n)) => super::files::format_size(*n, t),
                Some(Err(_)) => "?".to_owned(),
            });
            let mut right = row.max.x - 6.0;
            if let Some(size) = size {
                let galley = painter.layout_no_wrap(size, FontId::proportional(11.0), theme.accent.gamma_multiply(0.85));
                right -= galley.size().x + 6.0;
                painter.galley(Pos2::new(row.max.x - 6.0 - galley.size().x, row.center().y - galley.size().y / 2.0), galley, theme.accent);
            }
            job.wrap = egui::text::TextWrapping::truncate_at_width(right - text_left);
            let galley = painter.layout_job(job);
            let cut = galley.elided;
            painter.galley(Pos2::new(text_left, row.center().y - galley.size().y / 2.0), galley, color);
            if cut {
                // Cut short: whole on hover.
                resp.clone().on_hover_text(path.display().to_string());
            }
        }

        if resp.clicked() {
            let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            *action = Some(if shift {
                TreeAction::Select(path.to_path_buf(), Pick::Range)
            } else if cmd {
                TreeAction::Select(path.to_path_buf(), Pick::Toggle)
            } else if item.is_dir && folder.is_none() {
                TreeAction::Toggle(path.to_path_buf())
            } else {
                TreeAction::Select(path.to_path_buf(), Pick::Only)
            });
        }
        // A folder of the tree unfolds on the first click; one found by the search is shown in the tree.
        if resp.double_clicked() && !item.is_dir {
            *action = Some(TreeAction::Open(path.to_path_buf()));
        } else if resp.double_clicked() && folder.is_some() {
            *action = Some(TreeAction::ShowInTree(path.to_path_buf()));
        }
        // A right click outside the selection: on this item alone.
        if resp.secondary_clicked() && !self.tree.picked.contains(path) {
            self.tree.picked = HashSet::from([path.to_path_buf()]);
            self.tree.selected = Some(path.to_path_buf());
        }
        let root = self.tree_root();
        let targets = self.tree_targets(path);
        let several = targets.len() > 1;
        let dirs: Vec<PathBuf> = targets.iter().filter(|p| p.is_dir()).cloned().collect();
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| {
            ui.set_min_width(210.0);
            let p = path.to_path_buf();
            if several {
                ui.label(egui::RichText::new(t.tree_items.replace("{n}", &targets.len().to_string())).size(11.5).color(theme.text_muted));
            } else if item.is_dir {
                menu_item(ui, t.files_new_file, TreeAction::New(p.clone(), NamingKind::NewFile), action);
                menu_item(ui, t.files_new_folder, TreeAction::New(p.clone(), NamingKind::NewFolder), action);
                ui.separator();
                menu_item(ui, &format!("🔍  {}", t.tree_search_here), TreeAction::SearchIn(Some(p.clone())), action);
                menu_item(ui, &format!(">_  {}", t.tree_open_terminal), TreeAction::Terminal(p.clone()), action);
                menu_item(ui, t.tree_set_root, TreeAction::SetRoot(Some(p.clone())), action);
            } else {
                menu_item(ui, t.open, TreeAction::Open(p.clone()), action);
                ui.menu_button(t.files_open_with, |ui| {
                    ui.set_min_width(180.0);
                    for app in crate::openwith::apps_for(path) {
                        let label = if app.default { format!("{}  ({})", app.name, t.files_open_default) } else { app.name.clone() };
                        menu_item(ui, &label, TreeAction::Launch(p.clone(), OpenWith::App(app)), action);
                    }
                    ui.separator();
                    menu_item(ui, t.files_open_other, TreeAction::Launch(p.clone(), OpenWith::Choose), action);
                });
                menu_item(ui, t.files_edit, TreeAction::Edit(p.clone()), action);
            }
            if folder.is_some() && !several {
                menu_item(ui, t.tree_show_in_tree, TreeAction::ShowInTree(p.clone()), action);
            }
            ui.separator();
            // As in the file manager of a terminal.
            menu_item(ui, t.files_move, TreeAction::Op(TreeOp::Move(targets.clone())), action);
            menu_item(ui, t.duplicate, TreeAction::Op(TreeOp::Duplicate(targets.clone())), action);
            menu_item(ui, &format!("📦  {}", t.files_compress), TreeAction::Op(TreeOp::Compress(targets.clone())), action);
            // Windows has no such permissions.
            if cfg!(unix) {
                menu_item(ui, t.files_permissions, TreeAction::Op(TreeOp::Permissions(targets.clone())), action);
            }
            if !dirs.is_empty() {
                menu_item(ui, &format!("Σ  {}", t.files_dir_size), TreeAction::Op(TreeOp::Sizes(dirs.clone())), action);
            }
            ui.separator();
            if ui.button(t.files_copy_path).clicked() {
                ui.ctx().copy_text(targets.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"));
                ui.close();
            }
            if ui.button(t.tree_copy_relative).clicked() {
                let relative: Vec<String> = targets.iter().map(|p| p.strip_prefix(&root).unwrap_or(p).display().to_string()).collect();
                ui.ctx().copy_text(relative.join("\n"));
                ui.close();
            }
            if ui.button(t.open_location).clicked() {
                config::reveal(path);
                ui.close();
            }
            ui.separator();
            if !several {
                menu_item(ui, t.rename, TreeAction::Rename(p.clone()), action);
            }
            if ui.button(egui::RichText::new(t.delete).color(theme.ansi[1])).clicked() {
                *action = Some(TreeAction::Op(TreeOp::Delete(targets.clone())));
                ui.close();
            }
        });
    }

    /// The row of a new file or folder being named.
    fn naming_row(&mut self, ui: &mut Ui, row: Rect, depth: usize) {
        let theme = self.theme.clone();
        let x = row.min.x + 6.0 + depth as f32 * INDENT;
        let folder = self.tree.naming.as_ref().is_some_and(|n| n.kind == NamingKind::NewFolder);
        super::files::paint_file_icon(ui.painter(), Rect::from_center_size(Pos2::new(x + 20.0, row.center().y), Vec2::splat(14.0)), "", folder, false, None, &theme);
        self.naming_field(ui, Rect::from_min_max(Pos2::new(x + 28.0, row.min.y + 1.0), Pos2::new(row.max.x - 2.0, row.max.y - 1.0)));
    }

    /// The field of the name being typed: Enter (or a click elsewhere) makes it, Escape gives up.
    fn naming_field(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(naming) = &mut self.tree.naming else { return };
        let id = ui.id().with("tree-naming");
        let edit = ui.put(rect, egui::TextEdit::singleline(&mut naming.text).id(id).font(FontId::proportional(13.0)).margin(Vec2::new(4.0, 2.0)));
        if naming.fresh {
            naming.fresh = false;
            edit.request_focus();
            // The name without its extension selected, as in the Finder.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let stem = match naming.text.rfind('.') {
                    Some(dot) if dot > 0 && naming.kind == NamingKind::Rename => naming.text[..dot].chars().count(),
                    _ => naming.text.chars().count(),
                };
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(stem))));
                state.store(ui.ctx(), id);
            }
            return;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.tree.naming = None;
        } else if edit.lost_focus() {
            self.finish_naming();
        }
    }

    /// Makes the file or folder named, or renames the item.
    fn finish_naming(&mut self) {
        let Some(naming) = self.tree.naming.take() else { return };
        let name = naming.text.trim();
        if name.is_empty() || name.contains('/') || (cfg!(windows) && name.contains('\\')) {
            return;
        }
        let t = self.t();
        let target = match naming.kind {
            NamingKind::Rename => naming.path.parent().map(|p| p.join(name)).unwrap_or_else(|| PathBuf::from(name)),
            _ => naming.path.join(name),
        };
        if target == naming.path {
            return;
        }
        if target.symlink_metadata().is_ok() {
            self.tree.error = Some(t.tree_exists.replace("{name}", name));
            return;
        }
        let result = match naming.kind {
            NamingKind::NewFile => std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map(drop),
            NamingKind::NewFolder => std::fs::create_dir(&target),
            NamingKind::Rename => std::fs::rename(&naming.path, &target),
        };
        if let Err(e) = result {
            self.tree.error = Some(format!("{} : {e}", target.display()));
            return;
        }
        self.tree.listings.clear();
        let root = self.tree_root();
        if naming.kind == NamingKind::Rename && self.tree.expanded.remove(&naming.path) {
            self.tree.expanded.insert(target.clone());
        }
        self.tree.show(&root, &target);
        if naming.kind == NamingKind::NewFile {
            self.edit_local_file(&target);
        }
    }

    /// What a right click or a drag on `path` acts on: the selection when it is in it and its items
    /// share a folder, else `path` alone.
    fn tree_targets(&self, path: &Path) -> Vec<PathBuf> {
        let picked = &self.tree.picked;
        if picked.len() > 1 && picked.contains(path) && picked.iter().all(|p| p.parent() == path.parent()) {
            // In the tree's order.
            let mut items: Vec<PathBuf> = picked.iter().cloned().collect();
            items.sort_by_key(|p| self.tree.last_order.iter().position(|o| o == p).unwrap_or(usize::MAX));
            items
        } else {
            vec![path.to_path_buf()]
        }
    }

    /// The tree's file manager: its dialogs, its work in the background, the folders it changed listed
    /// again. Every frame, wherever the sidebar is.
    pub(super) fn tree_windows(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(fm) = self.tree.ops.as_deref_mut() else { return };
        fm.poll(t);
        fm.tree_dialogs(ctx, &theme, t);
        if let Some(e) = fm.take_error() {
            self.tree.error = Some(e);
        }
        if fm.readings != self.tree.ops_readings {
            self.tree.ops_readings = fm.readings;
            self.tree.listings.clear();
            self.tree.search = None;
        }
    }

    fn tree_action(&mut self, ctx: &egui::Context, action: TreeAction, root: &Path) {
        self.tree.error = None;
        match action {
            TreeAction::Select(p, pick) => {
                match pick {
                    Pick::Only => self.tree.picked = HashSet::from([p.clone()]),
                    Pick::Toggle => {
                        // The one picked alone so far joins the selection.
                        if self.tree.picked.is_empty()
                            && let Some(s) = &self.tree.selected
                        {
                            self.tree.picked.insert(s.clone());
                        }
                        if !self.tree.picked.remove(&p) {
                            self.tree.picked.insert(p.clone());
                        }
                    }
                    Pick::Range => {
                        let order = &self.tree.last_order;
                        let at = |q: &Path| order.iter().position(|o| o == q);
                        if let (Some(a), Some(b)) = (self.tree.selected.as_deref().and_then(at), at(&p)) {
                            self.tree.picked = order[a.min(b)..=a.max(b)].iter().cloned().collect();
                            return;
                        }
                        self.tree.picked = HashSet::from([p.clone()]);
                    }
                }
                self.tree.selected = Some(p);
            }
            TreeAction::Op(op) => {
                let t = self.t();
                let fm = self.tree.ops.get_or_insert_with(|| Box::new(FileManager::for_tree(&root.display().to_string(), ctx)));
                fm.tree_op(op, t);
                self.tree.picked.clear();
            }
            TreeAction::Toggle(p) => {
                if !self.tree.expanded.remove(&p) {
                    self.tree.expanded.insert(p.clone());
                    // Listed afresh on the way in.
                    self.tree.listings.remove(&p);
                }
                self.tree.picked = HashSet::from([p.clone()]);
                self.tree.selected = Some(p);
            }
            TreeAction::Open(p) if p.is_dir() => {
                self.tree.expanded.insert(p.clone());
                self.tree.selected = Some(p);
            }
            TreeAction::Open(p) => {
                self.tree.selected = Some(p.clone());
                if crate::openwith::is_binary(&p) {
                    crate::openwith::open(&p);
                } else {
                    self.edit_local_file(&p);
                }
            }
            TreeAction::Launch(p, how) => how.launch(&p),
            TreeAction::Edit(p) => self.edit_local_file(&p),
            TreeAction::Terminal(p) => self.new_tab_in(ctx, Some(&p)),
            TreeAction::SearchIn(dir) => {
                self.tree.scope = dir.filter(|d| d != root);
                self.tree.search = None;
                self.tree.focus_search = self.tree.scope.is_some();
            }
            TreeAction::SetRoot(p) => self.set_tree_root(p),
            TreeAction::PickRoot => {
                if let Some(dir) = rfd::FileDialog::new().set_directory(root).pick_folder() {
                    self.set_tree_root(Some(dir));
                }
            }
            TreeAction::RootFromTerminal => {
                let dir = self.terminal_dir();
                if dir.is_some() {
                    self.set_tree_root(dir);
                }
            }
            TreeAction::New(dir, kind) => {
                if dir != root {
                    self.tree.expanded.insert(dir.clone());
                }
                self.tree.query.clear();
                self.tree.naming = Some(Naming { kind, path: dir, text: String::new(), fresh: true });
            }
            TreeAction::Rename(p) => {
                let text = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.tree.query.clear();
                self.tree.show(root, &p);
                self.tree.naming = Some(Naming { kind: NamingKind::Rename, path: p, text, fresh: true });
            }
            TreeAction::ShowInTree(p) => {
                self.tree.query.clear();
                self.tree.search = None;
                if p.is_dir() {
                    self.tree.expanded.insert(p.clone());
                }
                self.tree.show(root, &p);
            }
            TreeAction::Refresh => {
                self.tree.listings.clear();
                self.tree.search = None;
            }
            TreeAction::CollapseAll => self.tree.expanded.clear(),
            TreeAction::ToggleHidden => {
                self.config.settings.files_hidden = !self.config.settings.files_hidden;
                self.tree.listings.clear();
                self.tree.search = None;
            }
        }
    }

    fn set_tree_root(&mut self, dir: Option<PathBuf>) {
        self.config.settings.files_root = dir.filter(|d| *d != home_dir()).map(|d| d.display().to_string());
        self.tree.scope = None;
        self.tree.expanded.clear();
        self.tree.listings.clear();
        self.tree.search = None;
        self.tree.selected = None;
    }

    /// Opens a text file of this computer in the editor: in the local tab shown (its file manager), or
    /// in a new one.
    pub(super) fn edit_local_file(&mut self, path: &Path) {
        let shown = self.shown_tab().filter(|&i| self.tabs[i].ssh.is_none() && self.tabs[i].db.is_none());
        let index = match shown {
            Some(i) => i,
            None => {
                let before = self.tabs.len();
                self.new_tab_in(&self.ctx.clone(), path.parent());
                if self.tabs.len() == before {
                    return;
                }
                self.active
            }
        };
        let t = self.t();
        self.toggle_files(index, true);
        if let Some(fm) = self.tabs[index].files.as_deref_mut() {
            if fm.editor.as_ref().is_some_and(|e| e.is_dirty()) {
                // Unsaved changes there: the file waits for them to be dealt with.
                self.tree.error = Some(t.tree_unsaved.to_owned());
                return;
            }
            fm.edit_local(path, t);
        }
    }
}
