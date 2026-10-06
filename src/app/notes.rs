//! Notes: folders of Markdown notes. The folders are a section of the sidebar; the page that takes the
//! terminals' place shows a folder's notes by date and the note, either as Markdown to edit or rendered.
//! Kept in notes.json; every note and folder has an id and the date of its last change, and
//! deletions are kept as marks, for a sync to come.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;
use std::path::{Path, PathBuf};

use chrono::{Datelike as _, TimeZone as _};
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Align2, Color32, FontFamily, FontId, Frame, Id, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config;
use crate::i18n::Strings;
use crate::theme::Theme;

const TOOLBAR_H: f32 = 42.0;
const LIST_W: f32 = 300.0;
const NOTE_H: f32 = 62.0;
/// Text size in the note.
const BODY: f32 = 15.0;
/// Width of the note's text column at most, centered in a wider page.
const COLUMN_W: f32 = 720.0;
/// Notes in the bin are deleted for good after this long.
const TRASH_DAYS: i64 = 30;
/// The folder (in the config folder) holding the images and files put in notes; links to them start with it.
pub(super) const FILES_DIR: &str = "notes-files";
/// Size of the Markdown marks hidden by the rendering: they stay in the text, out of sight.
const HIDDEN: f32 = 1.0;
const TABLE_ROW_H: f32 = 32.0;
const IMAGE_MAX_H: f32 = 420.0;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Store {
    pub folders: Vec<Folder>,
    pub notes: Vec<Note>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    /// The folder it is in (None: at the top, in the sidebar).
    #[serde(default)]
    pub parent: Option<Uuid>,
    /// Last change, in milliseconds since 1970.
    pub updated: i64,
    /// Deleted: kept as a mark so that a sync deletes it elsewhere too.
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Note {
    pub id: Uuid,
    /// None: the "Notes" folder, where notes go by default.
    #[serde(default)]
    pub folder: Option<Uuid>,
    /// Markdown; its first line is its title.
    pub text: String,
    pub created: i64,
    pub updated: i64,
    #[serde(default)]
    pub pinned: bool,
    /// When it went to the bin.
    #[serde(default)]
    pub trashed: Option<i64>,
}

/// A folder of the sidebar's section, or the bin.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Place {
    Folder(Option<Uuid>),
    Trash,
}

/// A row of the sidebar's section: a folder at the top, a note in no folder, or the bin.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Entry {
    Place(Place),
    Note(Uuid),
}

/// A note dragged from the list (onto a folder of the sidebar).
pub(super) struct DraggedNote(pub Uuid);

/// A change asked from a menu or a click.
#[derive(Clone, Debug)]
pub(super) enum Action {
    Select(Uuid),
    Place(Place),
    Pin(Uuid, bool),
    Move(Uuid, Option<Uuid>),
    Trash(Uuid),
    Restore(Uuid),
    Purge(Uuid),
    EmptyTrash,
    NewFolder,
    /// A folder inside this one (named in the list).
    NewSubfolder(Option<Uuid>),
    RenameFolder(Uuid),
    DeleteFolder(Uuid),
    NewNote,
    /// A new note in this folder.
    NewNoteIn(Option<Uuid>),
    /// A note shown where it is.
    Open(Uuid),
    /// The page closes.
    Close,
}

impl Action {
    /// Whether the page opens for it (to show what it did).
    pub(super) fn shows(&self) -> bool {
        matches!(self, Action::Select(_) | Action::Place(_) | Action::NewFolder | Action::NewSubfolder(_) | Action::NewNote | Action::NewNoteIn(_) | Action::Open(_))
    }
}

/// A line prefix the toolbar's buttons put or take away.
#[derive(Clone, Copy, PartialEq)]
enum Prefix {
    Heading,
    Bullet,
    Numbered,
    Check,
    Quote,
}

/// What a button of the toolbar (or ⌘ B, ⌘ I) does to the note.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Format {
    Heading,
    Bold,
    Italic,
    Strike,
    Code,
    Link,
    Bullet,
    Numbered,
    Check,
    Quote,
    Rule,
    Table,
    Image,
    Attach,
}

pub(super) struct Notes {
    store: Store,
    /// Where they are saved; None when the file couldn't be read (it isn't overwritten then).
    path: Option<PathBuf>,
    /// Changed since this time and not saved yet.
    dirty: Option<f64>,
    error: Option<String>,
    place: Place,
    selected: Option<Uuid>,
    search: String,
    /// The folder being renamed, its name as typed, and whether its field still has to take the focus.
    rename: Option<(Uuid, String, bool)>,
    /// The note shown rendered rather than as its Markdown.
    rendered: bool,
    focus_search: bool,
    focus_editor: bool,
    /// The note as laid out: its text, what it was laid out for, and the layout.
    layout: Option<(String, u64, Live)>,
    /// The images shown in notes, by their address.
    images: HashMap<String, Img>,
    /// A format asked by a shortcut, done at the next frame.
    pending: Option<Format>,
    /// A file that couldn't be added, and when.
    notice: Option<(String, f64)>,
    /// The table's cell being edited.
    cell_edit: Option<CellEdit>,
}

/// A table's cell edited in its own field (rows count without the |---| line, the headings 0).
struct CellEdit {
    note: Uuid,
    table: usize,
    row: usize,
    col: usize,
    text: String,
    /// Its text to select, the field to focus.
    select: bool,
    focus: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum CellMove {
    Next,
    Prev,
    /// Return: the cell below, a row more after the last.
    Below,
    Down,
    Up,
    Out,
}

/// What a table's cell menu does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum TableOp {
    RowBelow(usize),
    ColRight(usize),
    DeleteRow(usize),
    DeleteCol(usize),
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn local(ms: i64) -> chrono::DateTime<chrono::Local> {
    chrono::Local.timestamp_millis_opt(ms).single().unwrap_or_else(chrono::Local::now)
}

const EDITOR: &str = "notes-editor";
const SEARCH: &str = "notes-search";
const RENAME: &str = "notes-rename";
const CELL: &str = "notes-cell";

impl Notes {
    fn with(store: Store, path: Option<PathBuf>, error: Option<String>) -> Self {
        Self { store, path, dirty: None, error, place: Place::Folder(None), selected: None, search: String::new(), rename: None, rendered: true, focus_search: false, focus_editor: false, layout: None, images: HashMap::new(), pending: None, notice: None, cell_edit: None }
    }

    pub(super) fn load() -> Self {
        let path = config::config_dir().map(|d| d.join("notes.json"));
        let (store, path, error) = match config::load::<Store>(path.clone()) {
            Ok(store) => (store, path, None),
            Err(e) => (Store::default(), None, Some(format!("{e:#}"))),
        };
        let mut notes = Self::with(store, path, error);
        let limit = now_ms() - TRASH_DAYS * 24 * 3600 * 1000;
        let before = notes.store.notes.len();
        // Notes left empty (Ronnie closed while one was being started) and old ones in the bin.
        notes.store.notes.retain(|n| n.trashed.is_none_or(|at| at > limit) && (n.trashed.is_some() || !n.text.trim().is_empty()));
        if notes.store.notes.len() != before {
            notes.dirty = Some(0.0);
        }
        notes.selected = notes.listed().first().map(|n| n.id);
        notes.sweep_files();
        notes
    }

    /// Writes the notes now if they changed.
    pub(super) fn flush(&mut self) {
        if self.dirty.take().is_none() {
            return;
        }
        let Some(path) = &self.path else { return };
        self.error = config::save(path, &self.store).err().map(|e| format!("{e:#}"));
    }

    pub(super) fn focus_search(&mut self) {
        self.focus_search = true;
    }

    /// The note shown takes the keyboard (a new one when there is none).
    pub(super) fn focus_editor(&mut self) {
        self.focus_editor = true;
    }

    /// Switches the note between its Markdown and its rendering.
    pub(super) fn toggle_rendered(&mut self) {
        self.rendered = !self.rendered;
        self.focus_editor = true;
    }

    /// The note's text has the keyboard.
    pub(super) fn editing(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.has_focus(Id::new(EDITOR)) || m.has_focus(Id::new(CELL)))
    }

    /// ⌘ B, ⌘ I typed in the note.
    pub(super) fn format_key(&mut self, format: Format) {
        self.pending = Some(format);
    }

    fn changed(&mut self, ui: &Ui) {
        if self.dirty.is_none() {
            self.dirty = Some(ui.input(|i| i.time));
        }
    }

    fn note(&self, id: Uuid) -> Option<&Note> {
        self.store.notes.iter().find(|n| n.id == id)
    }

    fn note_mut(&mut self, id: Uuid) -> Option<&mut Note> {
        self.store.notes.iter_mut().find(|n| n.id == id)
    }

    fn folders(&self) -> Vec<&Folder> {
        let mut folders: Vec<&Folder> = self.store.folders.iter().filter(|f| !f.deleted).collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        folders
    }

    pub(super) fn place_name(&self, place: Place, t: &Strings) -> String {
        match place {
            Place::Trash => t.notes_trash.to_owned(),
            Place::Folder(None) => t.notes_default.to_owned(),
            Place::Folder(Some(id)) => self.folder_paths().into_iter().find(|(f, _)| *f == id).map(|(_, p)| p).unwrap_or_default(),
        }
    }

    /// The folders in `parent`, by name.
    fn children(&self, parent: Option<Uuid>) -> Vec<&Folder> {
        self.folders().into_iter().filter(|f| f.parent == parent).collect()
    }

    /// The folders in `id`, and theirs, and so on.
    fn descendants(&self, id: Uuid) -> Vec<Uuid> {
        let mut out = vec![id];
        let mut k = 0;
        while k < out.len() {
            let parent = out[k];
            out.extend(self.store.folders.iter().filter(|f| !f.deleted && f.parent == Some(parent)).map(|f| f.id));
            k += 1;
        }
        out
    }

    /// The notes in folder `id` and in its folders.
    fn count_in(&self, id: Uuid) -> usize {
        let inside = self.descendants(id);
        self.store.notes.iter().filter(|n| n.trashed.is_none() && n.folder.is_some_and(|f| inside.contains(&f))).count()
    }

    /// Every folder with its path ("Work › Clients"), in order.
    fn folder_paths(&self) -> Vec<(Uuid, String)> {
        let mut out = Vec::new();
        fn walk(notes: &Notes, parent: Option<Uuid>, prefix: &str, out: &mut Vec<(Uuid, String)>) {
            for f in notes.children(parent) {
                let path = if prefix.is_empty() { f.name.clone() } else { format!("{prefix} › {}", f.name) };
                out.push((f.id, path.clone()));
                walk(notes, Some(f.id), &path, out);
            }
        }
        walk(self, None, "", &mut out);
        out
    }

    /// The parent of folder `id`.
    fn parent_of(&self, id: Uuid) -> Option<Uuid> {
        self.store.folders.iter().find(|f| f.id == id).and_then(|f| f.parent)
    }

    /// The sidebar's rows: the folders at the top, the notes in no folder, and the bin when it holds
    /// something; with their counts.
    pub(super) fn entries(&self, t: &Strings) -> Vec<(Entry, String, usize)> {
        let mut rows: Vec<(Entry, String, usize)> = self.children(None).into_iter().map(|f| (Entry::Place(Place::Folder(Some(f.id))), f.name.clone(), self.count_in(f.id))).collect();
        let mut loose: Vec<&Note> = self.store.notes.iter().filter(|n| n.trashed.is_none() && n.folder.is_none() && !n.text.trim().is_empty()).collect();
        loose.sort_by_key(|n| (std::cmp::Reverse(n.pinned), std::cmp::Reverse(n.updated)));
        rows.extend(loose.into_iter().map(|n| (Entry::Note(n.id), title_of(&n.text).unwrap_or(t.notes_new).to_owned(), 0)));
        let trashed = self.store.notes.iter().filter(|n| n.trashed.is_some()).count();
        if trashed > 0 || self.place == Place::Trash {
            rows.push((Entry::Place(Place::Trash), t.notes_trash.to_owned(), trashed));
        }
        rows
    }

    /// The name of a row of the sidebar.
    pub(super) fn entry_name(&self, entry: Entry, t: &Strings) -> String {
        match entry {
            Entry::Place(place) => self.place_name(place, t),
            Entry::Note(id) => self.note(id).and_then(|n| title_of(&n.text)).unwrap_or(t.notes_new).to_owned(),
        }
    }

    /// Whether a row of the sidebar is the one shown.
    pub(super) fn entry_shown(&self, entry: Entry) -> bool {
        match entry {
            Entry::Place(place) => self.place == place,
            Entry::Note(id) => self.place == Place::Folder(None) && self.selected == Some(id),
        }
    }

    /// The folder being renamed.
    pub(super) fn renaming(&self) -> Option<Uuid> {
        self.rename.as_ref().map(|(id, ..)| *id)
    }

    /// The field renaming the folder, in `field`.
    pub(super) fn rename_field(&mut self, ui: &mut Ui, field: Rect, theme: &Theme) {
        let Some((id, text, fresh)) = self.rename.as_mut() else { return };
        ui.painter().rect_filled(field, 5.0, theme.bg);
        ui.painter().rect_stroke(field, 5.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Inside);
        let edit = ui.put(field.shrink2(Vec2::new(6.0, 2.0)), egui::TextEdit::singleline(text).id(Id::new(RENAME)).frame(Frame::NONE).font(FontId::proportional(13.0)));
        if std::mem::take(fresh) {
            edit.request_focus();
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit.id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()))));
            state.store(ui.ctx(), edit.id);
        }
        let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if escape {
            self.rename = None;
        } else if enter || edit.lost_focus() {
            let (id, name) = (*id, text.trim().to_owned());
            self.rename = None;
            if let Some(f) = self.store.folders.iter_mut().find(|f| f.id == id).filter(|f| !name.is_empty() && f.name != name) {
                f.name = name;
                f.updated = now_ms();
                self.changed(ui);
            }
        }
    }

    /// The notes the list shows: pinned first, then the latest changed. A search looks in every folder.
    fn listed(&self) -> Vec<&Note> {
        let query = self.search.trim().to_lowercase();
        let mut notes: Vec<&Note> = self
            .store
            .notes
            .iter()
            .filter(|n| match self.place {
                Place::Trash => n.trashed.is_some(),
                _ if !query.is_empty() => n.trashed.is_none(),
                Place::Folder(f) => n.trashed.is_none() && n.folder == f,
            })
            .filter(|n| query.is_empty() || n.text.to_lowercase().contains(&query))
            .collect();
        notes.sort_by_key(|n| (std::cmp::Reverse(n.pinned && self.place != Place::Trash), std::cmp::Reverse(n.updated)));
        notes
    }

    /// Selects `id`, dropping the note left if it was left empty.
    fn select(&mut self, id: Option<Uuid>) {
        if let Some(old) = self.selected.filter(|old| Some(*old) != id) {
            let before = self.store.notes.len();
            self.store.notes.retain(|n| n.id != old || n.trashed.is_some() || !n.text.trim().is_empty());
            if self.store.notes.len() != before {
                self.dirty.get_or_insert(0.0);
            }
        }
        self.selected = id;
        self.layout = None;
        self.cell_edit = None;
    }

    fn new_note(&mut self, ui: &Ui) {
        let folder = match self.place {
            Place::Folder(f) => f,
            Place::Trash => None,
        };
        self.place = Place::Folder(folder);
        self.search.clear();
        let now = now_ms();
        let id = Uuid::new_v4();
        self.store.notes.push(Note { id, folder, text: String::new(), created: now, updated: now, pinned: false, trashed: None });
        self.select(Some(id));
        self.focus_editor = true;
        self.changed(ui);
    }

    pub(super) fn apply(&mut self, action: Action, ui: &Ui, t: &Strings) {
        let now = now_ms();
        match action {
            Action::Close => return self.flush(),
            Action::Select(id) => {
                self.select(Some(id));
                return;
            }
            Action::Place(place) => {
                self.place = place;
                self.search.clear();
                let first = self.listed().first().map(|n| n.id);
                self.select(first);
                return;
            }
            Action::NewNote => return self.new_note(ui),
            Action::NewNoteIn(folder) => {
                self.place = Place::Folder(folder);
                return self.new_note(ui);
            }
            Action::Open(id) => {
                let folder = self.note(id).and_then(|n| n.folder);
                self.place = Place::Folder(folder);
                self.search.clear();
                self.select(Some(id));
                return;
            }
            Action::Pin(id, on) => {
                if let Some(n) = self.note_mut(id) {
                    n.pinned = on;
                    n.updated = now;
                }
            }
            Action::Move(id, folder) => {
                if let Some(n) = self.note_mut(id) {
                    n.folder = folder;
                    n.trashed = None;
                    n.updated = now;
                }
            }
            Action::Trash(id) => {
                if self.selected == Some(id) {
                    let next = self.listed().into_iter().map(|n| n.id).find(|n| *n != id);
                    self.selected = next;
                    self.layout = None;
                }
                if let Some(n) = self.note_mut(id) {
                    n.trashed = Some(now);
                    n.pinned = false;
                    n.updated = now;
                }
            }
            Action::Restore(id) => {
                let alive: Vec<Uuid> = self.folders().iter().map(|f| f.id).collect();
                if let Some(n) = self.note_mut(id) {
                    n.trashed = None;
                    n.folder = n.folder.filter(|f| alive.contains(f));
                    n.updated = now;
                }
            }
            Action::Purge(id) => {
                self.store.notes.retain(|n| n.id != id);
                if self.selected == Some(id) {
                    self.selected = None;
                }
                self.sweep_files();
            }
            Action::EmptyTrash => {
                self.store.notes.retain(|n| n.trashed.is_none());
                if self.place == Place::Trash {
                    self.place = Place::Folder(None);
                    self.selected = self.listed().first().map(|n| n.id);
                }
                self.sweep_files();
            }
            Action::NewFolder | Action::NewSubfolder(_) => {
                let parent = if let Action::NewSubfolder(p) = action { p } else { None };
                let names: Vec<String> = self.children(parent).iter().map(|f| f.name.to_lowercase()).collect();
                let name = (1..).map(|k| if k == 1 { t.notes_new_folder.to_owned() } else { format!("{} {k}", t.notes_new_folder) }).find(|n| !names.contains(&n.to_lowercase())).unwrap_or_default();
                let id = Uuid::new_v4();
                self.store.folders.push(Folder { id, name: name.clone(), parent, updated: now, deleted: false });
                self.rename = Some((id, name, true));
                // At the top: named in the sidebar, and shown. Inside a folder: named in its list.
                if parent.is_none() {
                    self.apply(Action::Place(Place::Folder(Some(id))), ui, t);
                } else {
                    self.place = Place::Folder(parent);
                    self.search.clear();
                }
            }
            Action::RenameFolder(id) => {
                if let Some(f) = self.store.folders.iter().find(|f| f.id == id) {
                    self.rename = Some((id, f.name.clone(), true));
                }
                return;
            }
            Action::DeleteFolder(id) => {
                // Its notes (and those of its folders) go to the bin, as in macOS Notes.
                let gone = self.descendants(id);
                let parent = self.parent_of(id);
                for n in self.store.notes.iter_mut().filter(|n| n.folder.is_some_and(|f| gone.contains(&f)) && n.trashed.is_none()) {
                    n.trashed = Some(now);
                    n.pinned = false;
                    n.updated = now;
                }
                for f in self.store.folders.iter_mut().filter(|f| gone.contains(&f.id)) {
                    f.deleted = true;
                    f.updated = now;
                }
                if matches!(self.place, Place::Folder(Some(f)) if gone.contains(&f)) {
                    self.apply(Action::Place(Place::Folder(parent)), ui, t);
                }
            }
        }
        self.changed(ui);
    }

    /// Draws the page in `rect`. Returns what the app has to do (the page closing).
    pub(super) fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings, mode_key: &str) -> Option<Action> {
        let time = ui.input(|i| i.time);
        if self.dirty.is_some_and(|since| time - since > 1.0) {
            self.flush();
        } else if self.dirty.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(1100));
        }
        if self.focus_editor && self.selected.is_none() {
            self.new_note(ui);
        }
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0.0, theme.bg);
        let (bar, body) = rect.split_top_bottom_at_y(rect.min.y + TOOLBAR_H);
        let list_w = LIST_W.min(rect.width() * 0.4);
        let list_rect = Rect::from_min_max(body.min, Pos2::new(body.min.x + list_w, body.max.y));
        let editor_rect = Rect::from_min_max(Pos2::new(list_rect.max.x, body.min.y), body.max);
        painter.rect_filled(list_rect, 0.0, theme.chrome_bg);

        let mut action = None;
        let mut format = None;
        self.toolbar(ui, bar, list_rect.max.x, theme, t, mode_key, &mut action, &mut format);
        painter.hline(rect.x_range(), bar.max.y - 0.5, Stroke::new(1.0, theme.tab_hover));
        painter.vline(list_rect.max.x - 0.5, body.y_range(), Stroke::new(1.0, theme.tab_hover));
        self.list_ui(ui, list_rect, theme, t, &mut action);
        self.editor_ui(ui, editor_rect, theme, t, format);
        match action {
            Some(Action::Close) => {
                self.flush();
                Some(Action::Close)
            }
            Some(action) => {
                self.apply(action, ui, t);
                None
            }
            None => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn toolbar(&mut self, ui: &mut Ui, bar: Rect, editor_x: f32, theme: &Theme, t: &Strings, mode_key: &str, action: &mut Option<Action>, format: &mut Option<Format>) {
        let y = bar.center().y;
        // Over the list: the folder shown, and a new note.
        let searching = !self.search.trim().is_empty();
        let title = if searching { t.notes_search.to_owned() } else { self.place_name(self.place, t) };
        let new_rect = Rect::from_center_size(Pos2::new(editor_x - 22.0, y), Vec2::splat(28.0));
        let folder_rect = new_rect.translate(Vec2::new(-30.0, 0.0));
        // Inside a folder: back to the one holding it.
        let mut title_x = bar.min.x + 16.0;
        if let Place::Folder(Some(id)) = self.place
            && !searching
            && let Some(parent) = self.parent_of(id)
        {
            let back = Rect::from_center_size(Pos2::new(bar.min.x + 22.0, y), Vec2::splat(28.0));
            if tool_button(ui, back, "notes-up", theme, paint_back_icon, false).on_hover_text(t.notes_up).clicked() {
                *action = Some(Action::Place(Place::Folder(Some(parent))));
            }
            title_x = back.max.x + 6.0;
        }
        let title_w = folder_rect.min.x - title_x - 8.0;
        let galley = ui.painter().layout_job(single_line(&title, FontId::new(14.0, FontFamily::Name("note-bold".into())), theme.text, title_w));
        ui.painter().galley(Pos2::new(title_x, y - galley.size().y / 2.0), galley, theme.text);
        if tool_button(ui, new_rect, "notes-new", theme, paint_compose_icon, false).on_hover_text(t.notes_new).clicked() {
            *action = Some(Action::NewNote);
        }
        if let Place::Folder(folder) = self.place
            && tool_button(ui, folder_rect, "notes-new-folder", theme, paint_new_folder_icon, false).on_hover_text(if folder.is_some() { t.notes_new_subfolder } else { t.notes_new_folder }).clicked()
        {
            *action = Some(Action::NewSubfolder(folder));
        }

        // Right side: close, search, and the switch between the Markdown and its rendering.
        let close_rect = Rect::from_center_size(Pos2::new(bar.max.x - 22.0, y), Vec2::splat(28.0));
        if tool_button(ui, close_rect, "notes-close", theme, paint_close_icon, false).on_hover_text(t.notes_close).clicked() {
            *action = Some(Action::Close);
        }
        let room = close_rect.min.x - editor_x;
        let search_w = (room * 0.35).clamp(90.0, 220.0);
        let search_rect = Rect::from_min_max(Pos2::new(close_rect.min.x - 8.0 - search_w, y - 13.0), Pos2::new(close_rect.min.x - 8.0, y + 13.0));
        ui.painter().rect_filled(search_rect, 7.0, theme.chrome_bg);
        let search_id = Id::new(SEARCH);
        let had = self.search.clone();
        let edit = ui.put(
            search_rect.shrink2(Vec2::new(9.0, 5.0)),
            egui::TextEdit::singleline(&mut self.search).id(search_id).hint_text(format!("🔍 {}", t.notes_search)).frame(Frame::NONE).font(FontId::proportional(12.5)),
        );
        if ui.memory(|m| m.has_focus(search_id)) {
            ui.painter().rect_stroke(search_rect, 7.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.search.clear();
                edit.surrender_focus();
            }
        }
        if std::mem::take(&mut self.focus_search) {
            edit.request_focus();
        }
        if self.search != had && self.selected.is_none_or(|s| !self.listed().iter().any(|n| n.id == s)) {
            let first = self.listed().first().map(|n| n.id);
            self.select(first);
        }

        let note = self.selected.and_then(|id| self.note(id));
        let Some(trashed) = note.map(|n| n.trashed.is_some()) else { return };
        let mut right = search_rect.min.x - 12.0;
        if !trashed {
            // Two segments: the rendering, the Markdown.
            let labels = [(true, t.notes_rendered), (false, t.notes_markdown)];
            let font = FontId::proportional(12.0);
            let widths: Vec<f32> = labels.iter().map(|(_, l)| ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), theme.text).size().x + 20.0).collect();
            let total: f32 = widths.iter().sum::<f32>() + 4.0;
            let whole = Rect::from_min_max(Pos2::new(right - total, y - 13.0), Pos2::new(right, y + 13.0));
            if whole.min.x > editor_x + 8.0 {
                ui.painter().rect_filled(whole, 7.0, theme.chrome_bg);
                let mut x = whole.min.x + 2.0;
                for ((rendered, label), w) in labels.into_iter().zip(widths) {
                    let r = Rect::from_min_size(Pos2::new(x, whole.min.y + 2.0), Vec2::new(w, whole.height() - 4.0));
                    x += w;
                    let resp = ui.interact(r, Id::new(("notes-mode", rendered)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.notes_mode_tip.replace("{key}", mode_key));
                    let on = self.rendered == rendered;
                    if on {
                        ui.painter().rect_filled(r, 5.0, theme.tab_active);
                        ui.painter().rect_stroke(r, 5.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.45)), egui::StrokeKind::Inside);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 5.0, theme.tab_hover);
                    }
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font.clone(), if on { theme.text } else { theme.text_muted });
                    if resp.clicked() && !on {
                        self.toggle_rendered();
                    }
                }
                right = whole.min.x - 10.0;
            }
        }
        // The formatting buttons; those without room go in a menu.
        if trashed {
            return;
        }
        let buttons = format_buttons(t);
        let room = right - 4.0;
        let mut x = editor_x + 14.0;
        let mut overflow: Vec<(Format, String)> = Vec::new();
        for (k, button) in buttons.iter().enumerate() {
            match button {
                None if overflow.is_empty() => {
                    ui.painter().vline(x + 4.0, (y - 9.0)..=(y + 9.0), Stroke::new(1.0, theme.tab_hover.gamma_multiply(1.5)));
                    x += 10.0;
                }
                None => {}
                Some((f, tip, paint)) => {
                    let last = buttons[k + 1..].iter().all(Option::is_none);
                    let needed = if last { 28.0 } else { 60.0 };
                    if !overflow.is_empty() || x + needed > room {
                        overflow.push((*f, tip.clone()));
                        continue;
                    }
                    let r = Rect::from_min_size(Pos2::new(x, y - 14.0), Vec2::splat(28.0));
                    if tool_button(ui, r, &format!("notes-format-{f:?}"), theme, *paint, false).on_hover_text(tip).clicked() {
                        *format = Some(*f);
                    }
                    x += 30.0;
                }
            }
        }
        if !overflow.is_empty() && x + 28.0 <= right {
            let r = Rect::from_min_size(Pos2::new(x, y - 14.0), Vec2::splat(28.0));
            let more = tool_button(ui, r, "notes-format-more", theme, paint_more_icon, false).on_hover_text(t.notes_more);
            egui::Popup::menu(&more).width(220.0).show(|ui| {
                for (f, tip) in &overflow {
                    if ui.button(tip).clicked() {
                        *format = Some(*f);
                    }
                }
            });
        }
    }

    fn list_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings, action: &mut Option<Action>) {
        let now = chrono::Local::now();
        let searching = !self.search.trim().is_empty();
        let show_folder = searching || self.place == Place::Trash;
        let notes: Vec<Note> = self.listed().into_iter().cloned().collect();
        let folders: Vec<(Uuid, String)> = self.folder_paths();
        // The folders inside the one shown, above its notes.
        let subfolders: Vec<(Uuid, String, usize)> = match self.place {
            Place::Folder(Some(id)) if !searching => self.children(Some(id)).iter().map(|f| (f.id, f.name.clone(), self.count_in(f.id))).collect(),
            _ => Vec::new(),
        };
        let folder_name = |id: Option<Uuid>| id.and_then(|id| folders.iter().find(|(f, _)| *f == id)).map_or(t.notes_default.to_owned(), |(_, n)| n.clone());
        let error_h = if self.error.is_some() { 44.0 } else { 0.0 };
        let list = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.max.y - error_h));
        if let Some(error) = &self.error {
            let r = Rect::from_min_max(Pos2::new(rect.min.x + 8.0, list.max.y), rect.max - Vec2::new(8.0, 4.0));
            let galley = ui.painter().layout(t.notes_save_error.replace("{e}", error), FontId::proportional(11.0), Color32::from_rgb(0xf7, 0x76, 0x8e), r.width());
            ui.painter().with_clip_rect(r).galley(r.min, galley, Color32::WHITE);
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink2(Vec2::new(8.0, 0.0))));
        if notes.is_empty() && subfolders.is_empty() {
            let text = if searching { t.notes_no_result } else { t.notes_empty };
            child.painter().text(Pos2::new(list.center().x, list.min.y + 60.0), Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme.text_muted);
            return;
        }
        let width = list.width() - 16.0;
        let bold = |size: f32| FontId::new(size, FontFamily::Name("note-bold".into()));
        let regular = |size: f32| FontId::new(size, FontFamily::Name("note".into()));
        egui::ScrollArea::vertical().id_salt(("notes-list", self.place)).auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if !subfolders.is_empty() {
                ui.add_space(8.0);
            }
            for (id, name, count) in &subfolders {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
                let dropping = resp.dnd_hover_payload::<DraggedNote>().is_some();
                if let Some(note) = resp.dnd_release_payload::<DraggedNote>() {
                    *action = Some(Action::Move(note.0, Some(*id)));
                }
                if resp.hovered() || dropping {
                    ui.painter().rect_filled(row, 8.0, theme.tab_hover);
                }
                if dropping {
                    ui.painter().rect_stroke(row, 8.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Inside);
                }
                paint_folder_icon(ui.painter(), Pos2::new(row.min.x + 18.0, row.center().y), theme.accent);
                let text = Rect::from_min_max(Pos2::new(row.min.x + 34.0, row.min.y + 5.0), Pos2::new(row.max.x - 8.0, row.max.y - 5.0));
                if self.rename.as_ref().is_some_and(|(r, ..)| r == id) {
                    self.rename_field(ui, text, theme);
                    continue;
                }
                let galley = ui.painter().layout_job(single_line(name, bold(13.5), theme.text, text.width() - 30.0));
                ui.painter().galley(Pos2::new(text.min.x, row.center().y - galley.size().y / 2.0), galley, theme.text);
                if *count > 0 {
                    ui.painter().text(Pos2::new(row.max.x - 12.0, row.center().y), Align2::RIGHT_CENTER, count.to_string(), regular(12.0), theme.text_muted);
                }
                if resp.double_clicked() {
                    *action = Some(Action::RenameFolder(*id));
                } else if resp.clicked() {
                    *action = Some(Action::Place(Place::Folder(Some(*id))));
                }
                let id = *id;
                resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| {
                    ui.set_min_width(180.0);
                    for (label, a) in [(t.notes_new, Action::NewNoteIn(Some(id))), (t.notes_new_subfolder, Action::NewSubfolder(Some(id))), (t.notes_rename, Action::RenameFolder(id))] {
                        if ui.button(label).clicked() {
                            *action = Some(a);
                        }
                    }
                    ui.separator();
                    if ui.button(egui::RichText::new(t.notes_delete_folder).color(theme.ansi[1])).clicked() {
                        *action = Some(Action::DeleteFolder(id));
                    }
                });
            }
            let mut section = None;
            for note in &notes {
                let here = if note.pinned && self.place != Place::Trash { t.notes_pinned.to_owned() } else { date_section(note.updated, now, t) };
                if section.as_ref() != Some(&here) {
                    ui.add_space(if section.is_some() { 14.0 } else { 10.0 });
                    let (r, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
                    ui.painter().text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, &here, bold(12.5), theme.text_muted);
                    section = Some(here);
                }
                let (row, resp) = ui.allocate_exact_size(Vec2::new(width, NOTE_H), Sense::click_and_drag());
                let selected = self.selected == Some(note.id);
                let fill = if selected { theme.accent.gamma_multiply(0.25) } else if resp.hovered() { theme.tab_hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(row, 8.0, fill);
                let inner = row.shrink2(Vec2::new(11.0, 9.0));
                let title = title_of(&note.text).unwrap_or(t.notes_new);
                let galley = ui.painter().layout_job(single_line(title, bold(13.5), theme.text, inner.width()));
                ui.painter().galley(inner.min, galley, theme.text);
                let date = short_date(note.updated, now, t);
                let date_galley = ui.painter().layout_no_wrap(date, regular(12.0), theme.text);
                let line2 = inner.min.y + 20.0;
                let date_w = date_galley.size().x;
                ui.painter().galley(Pos2::new(inner.min.x, line2), date_galley, theme.text);
                let preview = preview_of(&note.text).unwrap_or_else(|| t.notes_no_text.to_owned());
                let galley = ui.painter().layout_job(single_line(&preview, regular(12.0), theme.text_muted, inner.width() - date_w - 8.0));
                ui.painter().galley(Pos2::new(inner.min.x + date_w + 8.0, line2), galley, theme.text_muted);
                if show_folder {
                    let at = Pos2::new(inner.min.x + 5.0, inner.max.y - 4.0);
                    paint_folder_icon(&ui.painter().clone(), at, theme.text_muted.gamma_multiply(0.8));
                    let galley = ui.painter().layout_job(single_line(&folder_name(note.folder), regular(11.0), theme.text_muted, inner.width() - 16.0));
                    ui.painter().galley(Pos2::new(inner.min.x + 14.0, at.y - galley.size().y / 2.0), galley, theme.text_muted);
                } else if !selected {
                    ui.painter().hline(inner.min.x..=row.max.x - 11.0, row.max.y - 0.5, Stroke::new(1.0, theme.tab_hover));
                }
                if resp.clicked() || resp.drag_started() {
                    *action = Some(Action::Select(note.id));
                }
                // Dragged onto a folder of the sidebar: moved there.
                if resp.dragged() && note.trashed.is_none() {
                    resp.dnd_set_drag_payload(DraggedNote(note.id));
                    if let Some(pointer) = ui.ctx().pointer_interact_pos() {
                        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("notes-drag")));
                        let galley = painter.layout_no_wrap(title.to_owned(), regular(13.0), theme.text);
                        let r = Rect::from_min_size(pointer + Vec2::new(12.0, 6.0), galley.size() + Vec2::new(18.0, 10.0));
                        painter.rect_filled(r, 7.0, theme.tab_active);
                        painter.rect_stroke(r, 7.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                        painter.galley(r.min + Vec2::new(9.0, 5.0), galley, theme.text);
                    }
                }
                let id = note.id;
                resp.context_menu(|ui| {
                    ui.set_min_width(170.0);
                    if note.trashed.is_some() {
                        if ui.button(t.notes_restore).clicked() {
                            *action = Some(Action::Restore(id));
                        }
                        if ui.button(egui::RichText::new(t.notes_delete_forever).color(theme.ansi[1])).clicked() {
                            *action = Some(Action::Purge(id));
                        }
                        return;
                    }
                    if ui.button(if note.pinned { t.notes_unpin } else { t.notes_pin }).clicked() {
                        *action = Some(Action::Pin(id, !note.pinned));
                    }
                    ui.menu_button(t.notes_move_to, |ui| {
                        for (folder, name) in std::iter::once((None, t.notes_default.to_owned())).chain(folders.iter().map(|(f, n)| (Some(*f), n.clone()))) {
                            if ui.add_enabled(folder != note.folder, egui::Button::new(name)).clicked() {
                                *action = Some(Action::Move(id, folder));
                            }
                        }
                    });
                    ui.separator();
                    if ui.button(egui::RichText::new(t.notes_delete).color(theme.ansi[1])).clicked() {
                        *action = Some(Action::Trash(id));
                    }
                });
            }
            ui.add_space(12.0);
        });
    }

    fn editor_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings, format: Option<Format>) {
        let Some(id) = self.selected.filter(|id| self.note(*id).is_some()) else {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, t.notes_none, FontId::proportional(14.0), theme.text_muted);
            return;
        };
        let (updated, trashed) = self.note(id).map(|n| (n.updated, n.trashed.is_some())).unwrap_or_default();
        let column = COLUMN_W.min(rect.width() - 56.0).max(120.0);
        let side = ((rect.width() - column) / 2.0).max(16.0);
        let mut top = rect.min.y + 16.0;
        ui.painter().text(Pos2::new(rect.center().x, top), Align2::CENTER_TOP, long_date(updated, t), FontId::new(12.0, FontFamily::Name("note".into())), theme.text_muted);
        top += 28.0;
        let time = ui.input(|i| i.time);
        if let Some((notice, since)) = &self.notice {
            if time - since < 8.0 {
                let galley = ui.painter().layout(notice.clone(), FontId::proportional(12.0), Color32::from_rgb(0xf7, 0x76, 0x8e), column);
                let h = galley.size().y;
                ui.painter().galley(Pos2::new(rect.min.x + side, top), galley, Color32::WHITE);
                top += h + 10.0;
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            } else {
                self.notice = None;
            }
        }
        if trashed {
            let r = Rect::from_min_size(Pos2::new(rect.min.x + side, top), Vec2::new(column, 30.0));
            ui.painter().rect_filled(r, 7.0, theme.chrome_bg);
            let galley = ui.painter().layout_job(single_line(t.notes_in_trash, FontId::proportional(12.0), theme.text_muted, r.width() - 16.0));
            ui.painter().galley(r.center() - galley.size() / 2.0, galley, theme.text_muted);
            top += 40.0;
        }
        let area = Rect::from_min_max(Pos2::new(rect.min.x, top), rect.max);
        self.note_ui(ui, area, side, column, id, trashed, theme, t, format);
    }

    /// The note's text, to edit: rendered (its Markdown marks hidden, but on the lines being edited) or
    /// as its Markdown. Images, tables, boxes and the like are drawn over it.
    #[allow(clippy::too_many_arguments)]
    fn note_ui(&mut self, ui: &mut Ui, area: Rect, side: f32, column: f32, id: Uuid, trashed: bool, theme: &Theme, t: &Strings, format: Option<Format>) {
        let editor_id = Id::new(EDITOR);
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(editor_id));
        let pending = self.pending.take();
        if !trashed {
            if let Some(f) = format.or(pending) {
                self.format(&ctx, id, f, t);
            }
            self.take_files(ui, area, id, focused, theme, t);
        }
        let rendered = self.rendered || trashed;
        let cursor = egui::TextEdit::load_state(&ctx, editor_id).and_then(|s| s.cursor.char_range());
        // Rendered, a table is edited cell by cell: the cursor coming into one opens its cell.
        if rendered
            && focused
            && !trashed
            && let Some(c) = cursor
            && let Some((table, row, col)) = self.note(id).and_then(|n| cell_at(&n.text, byte_of(&n.text, c.primary.index.0)))
        {
            self.open_cell(id, table, row, col);
        }
        // Tab in a table's Markdown: to the next cell (Shift: the one before).
        if focused
            && !trashed
            && !rendered
            && let Some(c) = cursor
            && self.note(id).is_some_and(|n| table_at(&n.text, byte_of(&n.text, c.primary.index.0)).is_some())
        {
            let back = ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab));
            let next = back || ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
            if next
                && let Some(note) = self.note_mut(id)
                && let Some(cell) = next_cell(&mut note.text, c.primary.index.0, back)
            {
                note.updated = now_ms();
                select(&ctx, editor_id, cell);
                self.changed(ui);
            }
        }
        if let Some(note) = self.store.notes.iter().find(|n| n.id == id) {
            load_images(&mut self.images, &ctx, &note.text);
        }
        // The lines being edited show their Markdown.
        let selection = |ctx: &egui::Context| {
            egui::TextEdit::load_state(ctx, editor_id).and_then(|s| s.cursor.char_range()).map(|r| {
                let (a, b) = (r.primary.index.0, r.secondary.index.0);
                a.min(b)..a.max(b)
            })
        };
        let active = if focused && rendered { selection(&ctx) } else { None };
        let key = {
            let mut h = std::hash::DefaultHasher::new();
            (color_key(theme), &active, rendered, column.to_bits(), self.images.len()).hash(&mut h);
            h.finish()
        };
        let enter = ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.is_none());
        let editing = self.cell_edit.as_ref().filter(|e| e.note == id).map(|e| (e.table, e.row, e.col));
        let Notes { store, layout, images, .. } = self;
        let Some(note) = store.notes.iter_mut().find(|n| n.id == id) else { return };
        let sizes = |src: &str| images.get(src).and_then(|i| i.size);
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let text = buf.as_str();
            if !layout.as_ref().is_some_and(|(t, k, _)| *k == key && t == text) {
                let active = active.as_ref().map(|r| byte_of(text, r.start)..byte_of(text, r.end));
                *layout = Some((text.to_owned(), key, live(text, active, !rendered, theme, column, &sizes)));
            }
            let mut job = layout.as_ref().map(|(_, _, l)| l.job.clone()).unwrap_or_default();
            job.wrap.max_width = wrap_width;
            ui.painter().layout_job(job)
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let mut changed = false;
        let mut new_cursor = None;
        let mut shown = None;
        egui::ScrollArea::vertical().id_salt(("notes-text", id)).auto_shrink(false).show(&mut child, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                // Kept for what goes under the text (code blocks, table headings...), known once it is laid out.
                let back = ui.painter().add(Shape::Noop);
                let rows = (area.height() / (BODY * 1.5)).floor().max(4.0) as usize;
                let out = egui::TextEdit::multiline(&mut note.text)
                    .id(editor_id)
                    .font(FontId::new(BODY, FontFamily::Name("note".into())))
                    .frame(Frame::NONE)
                    .margin(Vec2::new(0.0, 2.0))
                    .desired_width(column)
                    .desired_rows(rows)
                    .lock_focus(true)
                    .interactive(!trashed)
                    .hint_text(t.notes_hint)
                    .layouter(&mut layouter)
                    .show(ui);
                changed = out.response.changed();
                let cursor = out.cursor_range.map(|r| r.primary.index.0);
                if changed
                    && enter
                    && let Some(c) = cursor.and_then(|c| continue_list(&mut note.text, c))
                {
                    new_cursor = Some(c);
                }
                shown = Some((out.galley.clone(), out.galley_pos, back, out.response.clone(), ui.clip_rect()));
            });
            ui.add_space(40.0);
        });
        let mut hits = Hits::default();
        let mut over = ui.new_child(egui::UiBuilder::new().max_rect(area));
        if let Some((galley, origin, back, response, clip)) = shown
            && let Some((_, _, live)) = layout.as_ref()
        {
            over.set_clip_rect(clip);
            hits = paint_live(&over, live, &galley, origin, column, back, &response, images, theme, t, !trashed, editing);
        }
        if changed {
            note.updated = now_ms();
        }
        if let Some(c) = new_cursor {
            set_cursor(&ctx, editor_id, c);
        }
        if changed {
            self.changed(ui);
        }
        if let Some(byte) = hits.toggle
            && let Some(note) = self.note_mut(id)
        {
            let mark = if note.text.as_bytes().get(byte) == Some(&b' ') { "x" } else { " " };
            note.text.replace_range(byte..byte + 1, mark);
            note.updated = now_ms();
            self.changed(ui);
        }
        if let Some(c) = hits.cursor {
            set_cursor(&ctx, editor_id, c);
            self.focus_editor = true;
        }
        // Other files (archives, programs...) are saved where the user picks rather than opened.
        if let Some(target) = hits.open {
            if is_file(&target) && !resolve(&target).is_some_and(|p| is_document(&p)) {
                self.save_file(ui, &target, t);
            } else {
                open_target(&target);
            }
        }
        if let Some(target) = hits.save {
            self.save_file(ui, &target, t);
        }
        if let Some(path) = hits.reveal.as_deref().and_then(resolve) {
            config::reveal(&path);
        }
        if let Some((table, op)) = hits.table_op {
            self.table_op(&ctx, id, table, op);
        }
        if let Some((table, row, col)) = hits.cell {
            self.open_cell(id, table, row, col);
            ctx.request_repaint();
        } else if let Some(rect) = hits.cell_rect {
            self.cell_ui(&mut over, rect, id, theme);
        } else if self.cell_edit.as_ref().is_some_and(|e| e.note == id && !e.focus) {
            self.cell_edit = None;
        }
        if std::mem::take(&mut self.focus_editor) {
            ctx.memory_mut(|m| m.request_focus(editor_id));
        }
        // The cursor moved to other lines: they show their Markdown at the next frame.
        let now_focused = ctx.memory(|m| m.has_focus(editor_id));
        if rendered && (now_focused != focused || (now_focused && selection(&ctx) != active)) {
            ctx.request_repaint();
        }
    }

    /// Files dropped on the page, or an image pasted in the note: kept with the notes, put in it.
    fn take_files(&mut self, ui: &Ui, area: Rect, id: Uuid, focused: bool, theme: &Theme, t: &Strings) {
        let (hovering, dropped) = ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>()));
        if hovering {
            let r = area.shrink(8.0);
            ui.painter().rect_filled(r, 12.0, theme.accent.gamma_multiply(0.06));
            ui.painter().rect_stroke(r, 12.0, Stroke::new(1.5, theme.accent.gamma_multiply(0.8)), egui::StrokeKind::Inside);
            ui.painter().text(Pos2::new(r.center().x, r.max.y - 28.0), Align2::CENTER_CENTER, t.notes_drop, FontId::proportional(13.0), theme.accent);
        }
        let mut added = Vec::new();
        for path in dropped {
            match attach(&path) {
                Ok(md) => added.push(md),
                Err(e) => self.notify(ui, t.notes_attach_error.replace("{file}", &path.display().to_string()).replace("{e}", &e)),
            }
        }
        // ⌘ V with an image and no text on the clipboard (egui only pastes text).
        let pasted = focused && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command)));
        if pasted
            && let Ok(mut clipboard) = arboard::Clipboard::new()
            && clipboard.get_text().map_or(true, |s| s.is_empty())
            && let Ok(image) = clipboard.get_image()
        {
            match save_image(image) {
                Ok(md) => added.push(md),
                Err(e) => self.notify(ui, t.notes_attach_error.replace("{file}", "image").replace("{e}", &e)),
            }
        }
        if !added.is_empty() {
            self.put_files(ui, id, added);
        }
    }

    /// A file of a note copied where the user picks, under its name.
    fn save_file(&mut self, ui: &Ui, target: &str, t: &Strings) {
        let Some(path) = resolve(target) else { return };
        let stored = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Its name without what made it unique.
        let name = stored.split_once('-').map_or(stored.as_str(), |(_, n)| n).to_owned();
        let Some(out) = rfd::FileDialog::new().set_file_name(&name).save_file() else { return };
        if let Err(e) = std::fs::copy(&path, &out) {
            self.notify(ui, t.notes_save_file_error.replace("{file}", &out.display().to_string()).replace("{e}", &e.to_string()));
        }
    }

    fn notify(&mut self, ui: &Ui, message: String) {
        self.notice = Some((message, ui.input(|i| i.time)));
    }

    /// Puts files (their Markdown) in the note at the cursor: images on lines of their own, the other
    /// files as links in the text.
    fn put_files(&mut self, ui: &Ui, id: Uuid, items: Vec<String>) {
        let editor_id = Id::new(EDITOR);
        let cursor = egui::TextEdit::load_state(ui.ctx(), editor_id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0.max(r.secondary.index.0));
        let Some(note) = self.note_mut(id) else { return };
        let mut cursor = cursor.unwrap_or(usize::MAX).min(note.text.chars().count());
        for md in items {
            cursor = if md.starts_with('!') { insert_block(&mut note.text, cursor, &md).1 } else { insert_inline(&mut note.text, cursor, &md) };
        }
        note.updated = now_ms();
        set_cursor(ui.ctx(), editor_id, cursor);
        self.focus_editor = true;
        self.changed(ui);
    }

    /// A button of the toolbar (or ⌘ B, ⌘ I) on the selection.
    fn format(&mut self, ctx: &egui::Context, id: Uuid, format: Format, t: &Strings) {
        let editor_id = Id::new(EDITOR);
        // A table's cell being edited: the text's formats go to it, the table's button adds a row.
        if let Some(edit) = self.cell_edit.as_mut().filter(|e| e.note == id) {
            let cell_id = Id::new(CELL);
            let len = edit.text.chars().count();
            let sel = egui::TextEdit::load_state(ctx, cell_id).and_then(|s| s.cursor.char_range()).map_or(len..len, |r| {
                let (a, b) = (r.primary.index.0.min(len), r.secondary.index.0.min(len));
                a.min(b)..a.max(b)
            });
            let new = match format {
                Format::Bold => wrap(&mut edit.text, sel, "**"),
                Format::Italic => wrap(&mut edit.text, sel, "*"),
                Format::Strike => wrap(&mut edit.text, sel, "~~"),
                Format::Code => wrap(&mut edit.text, sel, "`"),
                Format::Link => link(&mut edit.text, sel, t.notes_link_text),
                Format::Table => {
                    let (table, row) = (edit.table, edit.row);
                    return self.table_op(ctx, id, table, TableOp::RowBelow(row));
                }
                _ => return,
            };
            select(ctx, cell_id, new);
            edit.focus = true;
            let (table, row, col, value) = (edit.table, edit.row, edit.col, edit.text.clone());
            return self.set_cell(ctx, id, table, row, col, &value);
        }
        let sel = egui::TextEdit::load_state(ctx, editor_id).and_then(|s| s.cursor.char_range()).map_or(0..0, |r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            a.min(b)..a.max(b)
        });
        if matches!(format, Format::Image | Format::Attach) {
            let dialog = rfd::FileDialog::new();
            let dialog = if format == Format::Image { dialog.add_filter(t.notes_images, &["png", "jpg", "jpeg", "webp"]) } else { dialog };
            let Some(paths) = dialog.pick_files() else { return };
            let mut items = Vec::new();
            for path in paths {
                match attach(&path) {
                    Ok(md) => items.push(md),
                    Err(e) => self.notice = Some((t.notes_attach_error.replace("{file}", &path.display().to_string()).replace("{e}", &e), ctx.input(|i| i.time))),
                }
            }
            if !items.is_empty() {
                let ui_time = ctx.input(|i| i.time);
                let Some(note) = self.note_mut(id) else { return };
                let mut cursor = sel.end.min(note.text.chars().count());
                for md in items {
                    cursor = if md.starts_with('!') { insert_block(&mut note.text, cursor, &md).1 } else { insert_inline(&mut note.text, cursor, &md) };
                }
                note.updated = now_ms();
                set_cursor(ctx, editor_id, cursor);
                self.focus_editor = true;
                self.dirty.get_or_insert(ui_time);
            }
            return;
        }
        let Some(note) = self.note_mut(id) else { return };
        let len = note.text.chars().count();
        let sel = sel.start.min(len)..sel.end.min(len);
        let text = &mut note.text;
        let new = match format {
            Format::Heading => {
                let c = toggle_prefix(text, sel.start, Prefix::Heading);
                c..c
            }
            Format::Bullet => prefix_lines(text, sel, Prefix::Bullet),
            Format::Numbered => prefix_lines(text, sel, Prefix::Numbered),
            Format::Check => prefix_lines(text, sel, Prefix::Check),
            Format::Quote => prefix_lines(text, sel, Prefix::Quote),
            Format::Bold => wrap(text, sel, "**"),
            Format::Italic => wrap(text, sel, "*"),
            Format::Strike => wrap(text, sel, "~~"),
            Format::Code => wrap(text, sel, "`"),
            Format::Link => link(text, sel, t.notes_link_text),
            Format::Rule => {
                let (_, after) = insert_block(text, sel.end, "---");
                after..after
            }
            Format::Table => table(text, sel.end, t.notes_column),
            Format::Image | Format::Attach => return,
        };
        note.updated = now_ms();
        select(ctx, editor_id, new);
        self.focus_editor = true;
        self.dirty.get_or_insert(ctx.input(|i| i.time));
    }

    /// Edits a table's cell, in place.
    fn open_cell(&mut self, id: Uuid, table: usize, row: usize, col: usize) {
        let text = self.note(id).and_then(|n| table_cells(&n.text, table)).and_then(|(_, lines)| lines.get(line_of(row))?.get(col).cloned()).unwrap_or_default();
        self.cell_edit = Some(CellEdit { note: id, table, row, col, text, select: true, focus: true });
    }

    /// The field of the cell being edited, in `rect`: ⇥ goes to the next cell (a row more after the
    /// last), Return to the one below, ↑ ↓ through the rows and out of the table, Esc out of it.
    fn cell_ui(&mut self, ui: &mut Ui, rect: Rect, id: Uuid, theme: &Theme) {
        let cell_id = Id::new(CELL);
        let has = ui.memory(|m| m.has_focus(cell_id));
        let moved = if has {
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::SHIFT, Key::Tab) {
                    Some(CellMove::Prev)
                } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                    Some(CellMove::Next)
                } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                    Some(CellMove::Below)
                } else if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    Some(CellMove::Down)
                } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    Some(CellMove::Up)
                } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                    Some(CellMove::Out)
                } else {
                    None
                }
            })
        } else {
            None
        };
        let Some(edit) = self.cell_edit.as_mut() else { return };
        let r = rect.shrink(1.0);
        ui.painter().rect_filled(r, 4.0, theme.accent.gamma_multiply(0.08));
        ui.painter().rect_stroke(r, 4.0, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
        let font = note_font(14.0, if edit.row == 0 { "note-bold" } else { "note" });
        let filter = egui::EventFilter { tab: true, escape: true, horizontal_arrows: true, vertical_arrows: true };
        let field = egui::TextEdit::singleline(&mut edit.text)
            .id(cell_id)
            .frame(Frame::NONE)
            .font(font)
            .margin(Vec2::ZERO)
            .vertical_align(egui::Align::Center)
            .desired_width(rect.width() - 16.0)
            .event_filter(filter);
        let resp = ui.put(rect.shrink2(Vec2::new(8.0, 2.0)), field);
        if std::mem::take(&mut edit.focus) {
            resp.request_focus();
        }
        if std::mem::take(&mut edit.select) {
            select(ui.ctx(), cell_id, 0..edit.text.chars().count());
        }
        let (table, row, col, value) = (edit.table, edit.row, edit.col, edit.text.clone());
        if resp.changed() {
            self.set_cell(ui.ctx(), id, table, row, col, &value);
        }
        if let Some(moved) = moved {
            self.move_cell(ui.ctx(), id, moved);
        } else if !ui.memory(|m| m.has_focus(cell_id)) {
            self.cell_edit = None;
        }
    }

    /// Writes the cell's text in the note's table.
    fn set_cell(&mut self, ctx: &egui::Context, id: Uuid, table: usize, row: usize, col: usize, value: &str) {
        let value = value.replace(['\n', '\r'], " ").replace("\\|", "|").replace('|', "\\|");
        self.rewrite(ctx, id, table, |lines| {
            if let Some(line) = lines.get_mut(line_of(row)) {
                if line.len() <= col {
                    line.resize(col + 1, String::new());
                }
                line[col] = value.trim().to_owned();
            }
        });
    }

    /// Changes the note's `table`-th table, its lines' cells given to `edit` (the |---| line second).
    fn rewrite(&mut self, ctx: &egui::Context, id: Uuid, table: usize, edit: impl FnOnce(&mut Vec<Vec<String>>)) {
        let time = ctx.input(|i| i.time);
        let Some(note) = self.note_mut(id) else { return };
        let Some((range, mut lines)) = table_cells(&note.text, table) else { return };
        edit(&mut lines);
        let cols = lines.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let text: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(k, cells)| {
                let cells: Vec<String> = (0..cols).map(|j| cells.get(j).filter(|c| k != 1 || !c.is_empty()).cloned().unwrap_or_else(|| if k == 1 { "---".into() } else { String::new() })).collect();
                format!("| {} |", cells.join(" | "))
            })
            .collect();
        let text = text.join("\n");
        if note.text[range.clone()] != text {
            note.text.replace_range(range, &text);
            note.updated = now_ms();
            self.dirty.get_or_insert(time);
        }
    }

    /// A row or a column added or deleted from a cell's menu (or the toolbar).
    fn table_op(&mut self, ctx: &egui::Context, id: Uuid, table: usize, op: TableOp) {
        let Some((_, lines)) = self.note(id).and_then(|n| table_cells(&n.text, table)) else { return };
        let cols = lines.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let rows = lines.len().saturating_sub(1).max(1);
        // The cell edited after (none when it was deleted).
        let mut next = self.cell_edit.as_ref().filter(|e| e.note == id && e.table == table).map(|e| (e.row, e.col));
        match op {
            TableOp::RowBelow(row) => {
                self.rewrite(ctx, id, table, |lines| lines.insert(line_of(row) + if row == 0 { 2 } else { 1 }, vec![String::new(); cols]));
                next = Some((row + 1, next.map_or(0, |(_, c)| c)));
            }
            TableOp::ColRight(col) => {
                self.rewrite(ctx, id, table, |lines| {
                    for (k, line) in lines.iter_mut().enumerate() {
                        line.resize(line.len().max(col + 1), String::new());
                        line.insert(col + 1, if k == 1 { "---".into() } else { String::new() });
                    }
                });
                next = Some((next.map_or(0, |(r, _)| r), col + 1));
            }
            TableOp::DeleteRow(row) if row > 0 && row <= rows => {
                self.rewrite(ctx, id, table, |lines| {
                    lines.remove(line_of(row));
                });
                next = None;
            }
            TableOp::DeleteCol(col) if cols > 1 => {
                self.rewrite(ctx, id, table, |lines| {
                    for line in lines.iter_mut().filter(|l| l.len() > col) {
                        line.remove(col);
                    }
                });
                next = None;
            }
            _ => return,
        }
        match next {
            Some((row, col)) => self.open_cell(id, table, row, col),
            None => self.cell_edit = None,
        }
    }

    /// Moves the cell edited, or out of the table (into the note's text, above or below it).
    fn move_cell(&mut self, ctx: &egui::Context, id: Uuid, moved: CellMove) {
        let Some(edit) = self.cell_edit.as_ref() else { return };
        let (table, row, col) = (edit.table, edit.row, edit.col);
        let Some((range, lines)) = self.note(id).and_then(|n| table_cells(&n.text, table)) else { return };
        let cols = lines.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let rows = lines.len().saturating_sub(1).max(1);
        let target = match moved {
            CellMove::Next if col + 1 < cols => Some((row, col + 1)),
            CellMove::Next | CellMove::Below if row + 1 >= rows => {
                self.table_op(ctx, id, table, TableOp::RowBelow(row));
                Some((row + 1, if moved == CellMove::Next { 0 } else { col }))
            }
            CellMove::Next => Some((row + 1, 0)),
            CellMove::Below => Some((row + 1, col)),
            CellMove::Prev if col > 0 => Some((row, col - 1)),
            CellMove::Prev if row > 0 => Some((row - 1, cols - 1)),
            CellMove::Prev => Some((row, col)),
            CellMove::Down if row + 1 < rows => Some((row + 1, col)),
            CellMove::Up if row > 0 => Some((row - 1, col)),
            CellMove::Up => {
                self.leave_table(ctx, id, range, true);
                None
            }
            CellMove::Down | CellMove::Out => {
                self.leave_table(ctx, id, range, false);
                None
            }
        };
        if let Some((row, col)) = target {
            self.open_cell(id, table, row, col);
        }
    }

    /// Out of a table: the cursor in the note's text on the line above it or below it (one is made
    /// when there is none).
    fn leave_table(&mut self, ctx: &egui::Context, id: Uuid, table: Range<usize>, above: bool) {
        self.cell_edit = None;
        let time = ctx.input(|i| i.time);
        let Some(note) = self.note_mut(id) else { return };
        let cursor = if above {
            if table.start == 0 {
                note.text.insert(0, '\n');
                0
            } else {
                note.text[..table.start - 1].chars().count()
            }
        } else {
            if table.end >= note.text.len() {
                note.text.push('\n');
            }
            note.text[..table.end + 1].chars().count()
        };
        note.updated = now_ms();
        self.dirty.get_or_insert(time);
        set_cursor(ctx, Id::new(EDITOR), cursor);
        self.focus_editor = true;
    }

    /// Deletes the files kept with the notes that no note links to any more (their notes deleted for good).
    fn sweep_files(&self) {
        if self.path.is_none() {
            return;
        }
        let Some(dir) = config::config_dir().map(|d| d.join(FILES_DIR)) else { return };
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().is_ok_and(|t| t.is_file()) && !self.store.notes.iter().any(|n| n.text.contains(&name)) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

impl Drop for Notes {
    fn drop(&mut self) {
        self.flush();
    }
}

/// An image shown in notes, and its size in points (None: it couldn't be read).
pub(super) struct Img {
    texture: Option<egui::TextureHandle>,
    size: Option<Vec2>,
}

/// A line of the note.
#[derive(Clone)]
struct Line {
    start: usize,
    /// Before its line break.
    end: usize,
    /// After its line break.
    next: usize,
    role: Role,
    /// The lines of the block it belongs to (a table, a block of code), by index; itself otherwise.
    block: Range<usize>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Text,
    /// A ``` line, opening or closing a block of code.
    Fence,
    Code,
    Table,
    /// The |---|---| line under a table's headings.
    Separator,
}

/// The note's lines, with the blocks of code and the tables found.
fn structure(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut at = 0;
    for l in text.split_inclusive('\n') {
        let next = at + l.len();
        lines.push(Line { start: at, end: at + l.trim_end_matches(['\n', '\r']).len(), next, role: Role::Text, block: 0..0 });
        at = next;
    }
    // The empty line after the last line break (where the cursor can be).
    if text.is_empty() || text.ends_with('\n') {
        lines.push(Line { start: at, end: at, next: at, role: Role::Text, block: 0..0 });
    }
    let content = |l: &Line| text[l.start..l.end].trim_start();
    let n = lines.len();
    let mut i = 0;
    while i < n {
        if content(&lines[i]).starts_with("```") {
            let close = (i + 1..n).find(|k| content(&lines[*k]).starts_with("```"));
            let last = close.unwrap_or(n - 1);
            for (k, line) in lines.iter_mut().enumerate().take(last + 1).skip(i) {
                line.role = if k == i || Some(k) == close { Role::Fence } else { Role::Code };
                line.block = i..last + 1;
            }
            i = last + 1;
            continue;
        }
        if content(&lines[i]).starts_with('|') && i + 1 < n && is_separator(content(&lines[i + 1])) {
            let mut last = i + 1;
            while last + 1 < n && content(&lines[last + 1]).starts_with('|') {
                last += 1;
            }
            for (k, line) in lines.iter_mut().enumerate().take(last + 1).skip(i) {
                line.role = if k == i + 1 { Role::Separator } else { Role::Table };
                line.block = i..last + 1;
            }
            i = last + 1;
            continue;
        }
        lines[i].block = i..i + 1;
        i += 1;
    }
    lines
}

/// "|---|:---:|", under a table's headings.
fn is_separator(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('|') && line.contains('-') && line.split('|').map(str::trim).filter(|c| !c.is_empty()).all(|c| c.chars().all(|ch| matches!(ch, '-' | ':')))
}

/// A table's row `line` (bytes): each cell's place between its bars, and its text (an empty place in
/// the middle of an empty cell).
fn cells_of(text: &str, line: Range<usize>) -> Vec<(Range<usize>, Range<usize>)> {
    let s = &text[line.clone()];
    let bars: Vec<usize> = s.char_indices().filter(|(i, c)| *c == '|' && (*i == 0 || s.as_bytes()[i - 1] != b'\\')).map(|(i, _)| line.start + i).collect();
    let mut cells = Vec::new();
    for (k, bar) in bars.iter().enumerate() {
        let from = bar + 1;
        let to = bars.get(k + 1).copied().unwrap_or(line.end);
        let inner = &text[from..to];
        if k + 1 == bars.len() && inner.trim().is_empty() {
            break;
        }
        let lead = inner.len() - inner.trim_start().len();
        let content = if inner.trim().is_empty() {
            let mid = from + inner.len().min(1);
            mid..mid
        } else {
            from + lead..from + lead + inner.trim().len()
        };
        cells.push((from..to, content));
    }
    cells
}

/// The table holding byte `at`: its lines.
fn table_at(text: &str, at: usize) -> Option<Vec<Line>> {
    let lines = structure(text);
    let i = lines.iter().position(|l| l.start <= at && at <= l.end)?;
    matches!(lines[i].role, Role::Table | Role::Separator).then(|| lines[lines[i].block.clone()].to_vec())
}

/// The note's `index`-th table: where it is (bytes), and its lines' cells, the |---| line second.
fn table_cells(text: &str, index: usize) -> Option<(Range<usize>, Vec<Vec<String>>)> {
    let lines = structure(text);
    let first = (0..lines.len()).filter(|k| lines[*k].role == Role::Table && lines[*k].block.start == *k).nth(index)?;
    let block = lines[first].block.clone();
    let range = lines[block.start].start..lines[block.end - 1].end;
    let cells = block.map(|k| cells_of(text, lines[k].start..lines[k].end).into_iter().map(|(_, c)| text[c].to_owned()).collect()).collect();
    Some((range, cells))
}

/// The line of a table holding row `row` (the headings 0, the |---| line skipped).
fn line_of(row: usize) -> usize {
    if row == 0 { 0 } else { row + 1 }
}

/// The table's cell at byte `at`: the table (its index in the note), the row and the column.
fn cell_at(text: &str, at: usize) -> Option<(usize, usize, usize)> {
    let lines = structure(text);
    let i = lines.iter().position(|l| l.start <= at && at <= l.end)?;
    let line = &lines[i];
    if !matches!(line.role, Role::Table | Role::Separator) {
        return None;
    }
    let table = (0..line.block.start).filter(|k| lines[*k].role == Role::Table && lines[*k].block.start == *k).count();
    let k = i - line.block.start;
    let row = match k {
        0 => 0,
        1 => usize::from(line.block.len() > 2),
        _ => k - 1,
    };
    let row_line = &lines[line.block.start + line_of(row)];
    let col = cells_of(text, row_line.start..row_line.end).iter().rposition(|(slot, _)| slot.start <= at).filter(|_| k != 1).unwrap_or(0);
    Some((table, row, col))
}

fn char_range(text: &str, bytes: Range<usize>) -> Range<usize> {
    let start = text[..bytes.start].chars().count();
    start..start + text[bytes].chars().count()
}

/// Tab in a table (at character `cursor`): the next cell's text, or the one before. Past the last
/// cell, a row is added.
fn next_cell(text: &mut String, cursor: usize, back: bool) -> Option<Range<usize>> {
    let byte = byte_of(text, cursor);
    let rows = table_at(text, byte)?;
    let cells: Vec<(Range<usize>, Range<usize>)> = rows.iter().filter(|l| l.role == Role::Table).flat_map(|l| cells_of(text, l.start..l.end)).collect();
    let here = cells.iter().rposition(|(slot, _)| slot.start <= byte).unwrap_or(0);
    if back {
        return cells.get(here.saturating_sub(1)).map(|(_, c)| char_range(text, c.clone()));
    }
    if let Some((_, c)) = cells.get(here + 1) {
        return Some(char_range(text, c.clone()));
    }
    let last = rows.last()?;
    let count = cells_of(text, last.start..last.end).len().max(1);
    text.insert_str(last.end, &format!("\n|{}", "  |".repeat(count)));
    let c = text[..last.end].chars().count() + 3;
    Some(c..c)
}

/// The toolbar's table: a new one with its first heading selected; in a table, a row more under the
/// cursor's.
fn table(text: &mut String, cursor: usize, column: &str) -> Range<usize> {
    let byte = byte_of(text, cursor);
    if let Some(rows) = table_at(text, byte) {
        let here = rows.iter().position(|l| l.start <= byte && byte <= l.end).unwrap_or(0);
        // On the headings: under the line below them.
        let line = &rows[if here == 0 { 1 } else { here }];
        let count = rows.iter().filter(|l| l.role == Role::Table).map(|l| cells_of(text, l.start..l.end).len()).max().unwrap_or(1).max(1);
        let at = line.end;
        text.insert_str(at, &format!("\n|{}", "  |".repeat(count)));
        let c = text[..at].chars().count() + 3;
        return c..c;
    }
    let names: Vec<String> = (1..=3).map(|k| column.replace("{n}", &k.to_string())).collect();
    let block = format!("| {} |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |", names.join(" | "));
    let (start, _) = insert_block(text, cursor, &block);
    start + 2..start + 2 + names[0].chars().count()
}

/// Puts `block` on lines of its own at the cursor (character `cursor`): on its line when that is
/// empty, under it otherwise, a line left after it. Returns the characters where it starts and where
/// the cursor goes (the line after it).
fn insert_block(text: &mut String, cursor: usize, block: &str) -> (usize, usize) {
    let byte = byte_of(text, cursor);
    let line = line_at(text, byte);
    let (at, before) = if text[line.clone()].trim().is_empty() {
        text.replace_range(line.clone(), "");
        (line.start, "")
    } else {
        (line.end, "\n")
    };
    let after = if text[at..].starts_with('\n') { "" } else { "\n" };
    text.insert_str(at, &format!("{before}{block}{after}"));
    let start = text[..at].chars().count() + before.len();
    (start, start + block.chars().count() + 1)
}

/// Puts `md` in the text at character `cursor`, spaced from the words around. Returns where the
/// cursor goes.
fn insert_inline(text: &mut String, cursor: usize, md: &str) -> usize {
    let byte = byte_of(text, cursor);
    let before = if text[..byte].chars().last().is_some_and(|c| !c.is_whitespace()) { " " } else { "" };
    let s = format!("{before}{md} ");
    text.insert_str(byte, &s);
    cursor + s.chars().count()
}

/// `kind` put on (or taken off) each line of the selection (characters). Returns the selection after.
fn prefix_lines(text: &mut String, sel: Range<usize>, kind: Prefix) -> Range<usize> {
    let (a, b) = (byte_of(text, sel.start), byte_of(text, sel.end));
    let first = line_at(text, a).start;
    let starts: Vec<usize> = std::iter::once(first).chain(text[first..b].match_indices('\n').map(|(i, _)| first + i + 1).filter(|s| *s < b)).collect();
    if starts.len() <= 1 {
        let c = toggle_prefix(text, sel.start, kind);
        return c..c;
    }
    for start in starts.iter().rev() {
        let c = text[..*start].chars().count();
        toggle_prefix(text, c, kind);
    }
    let from = text[..first].chars().count();
    let len: usize = text[first..].split_inclusive('\n').take(starts.len()).map(str::len).sum();
    let end = first + text[first..first + len].trim_end_matches('\n').len();
    from..text[..end].chars().count()
}

/// Puts `mark` on both sides of the selection (characters), or takes it away when it is there.
/// Returns the selection after.
fn wrap(text: &mut String, sel: Range<usize>, mark: &str) -> Range<usize> {
    let (mut a, mut b) = (byte_of(text, sel.start), byte_of(text, sel.end));
    // Not around the spaces selected with the words.
    while a < b && text[a..b].starts_with(char::is_whitespace) {
        a += text[a..].chars().next().map_or(1, char::len_utf8);
    }
    while a < b && text[a..b].ends_with(char::is_whitespace) {
        b -= text[..b].chars().last().map_or(1, char::len_utf8);
    }
    let m = mark.len();
    let start = text[..a].chars().count();
    let inner = text[a..b].chars().count();
    if text[..a].ends_with(mark) && text[b..].starts_with(mark) {
        text.replace_range(b..b + m, "");
        text.replace_range(a - m..a, "");
        return start - m..start - m + inner;
    }
    if b - a >= 2 * m && text[a..b].starts_with(mark) && text[a..b].ends_with(mark) {
        let kept = text[a + m..b - m].to_owned();
        text.replace_range(a..b, &kept);
        return start..start + inner - 2 * m;
    }
    text.insert_str(b, mark);
    text.insert_str(a, mark);
    start + m..start + m + inner
}

/// The selection (characters) made a link's label, or a placeholder label; its address selected, to
/// type. A web address selected becomes the address.
fn link(text: &mut String, sel: Range<usize>, label: &str) -> Range<usize> {
    let (a, b) = (byte_of(text, sel.start), byte_of(text, sel.end));
    let selected = text[a..b].trim().to_owned();
    if selected.starts_with("http://") || selected.starts_with("https://") {
        text.replace_range(a..b, &format!("[{label}]({selected})"));
        let start = sel.start + 1;
        return start..start + label.chars().count();
    }
    let label = if selected.is_empty() || selected.contains('\n') { label.to_owned() } else { selected };
    text.replace_range(a..b, &format!("[{label}](https://)"));
    let url = sel.start + label.chars().count() + 3;
    url..url + "https://".len()
}

/// A line that is only an image: its address.
fn image_src(line: &str) -> Option<&str> {
    let line = line.trim();
    let (front, label, back, url) = md_link(line)?;
    (front == 2 && front + label + back == line.len()).then_some(url)
}

/// "[label](address)" or "![label](address)" starting `s`: the marks before, the label's length, the
/// marks after, and the address.
fn md_link(s: &str) -> Option<(usize, usize, usize, &str)> {
    let front = if s.starts_with("![") {
        2
    } else if s.starts_with('[') {
        1
    } else {
        return None;
    };
    let close = s[front..].find("](")? + front;
    let label = &s[front..close];
    if (label.is_empty() && front == 1) || label.contains(['[', '\n']) {
        return None;
    }
    let from = close + 2;
    let end = s[from..].find(')')? + from;
    let url = &s[from..end];
    if url.is_empty() || url.contains(char::is_whitespace) {
        return None;
    }
    Some((front, close - front, end + 1 - close, url))
}

/// A link to a file kept with the notes.
fn is_file(url: &str) -> bool {
    url.strip_prefix(FILES_DIR).is_some_and(|rest| rest.starts_with('/'))
}

fn is_image(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    name.contains('.') && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp")
}

/// Where an image or a file of a note is: one kept with the notes, or a path on this computer.
fn resolve(src: &str) -> Option<PathBuf> {
    if let Some(name) = src.strip_prefix(FILES_DIR).and_then(|s| s.strip_prefix('/')) {
        // Only a file of that folder, never a way out of it.
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return None;
        }
        return config::config_dir().map(|d| d.join(FILES_DIR).join(name));
    }
    let path = PathBuf::from(src.strip_prefix("file://").unwrap_or(src));
    path.is_absolute().then_some(path)
}

/// Copies `path` among the notes' files. Returns its Markdown: an image, or a link to the file.
fn attach(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).to_string());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stored = store_name(&name);
    let dir = config::config_dir().ok_or("?")?.join(FILES_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::copy(path, dir.join(&stored)).map_err(|e| e.to_string())?;
    Ok(markdown_for(&name, &stored))
}

/// An image pasted: saved as a PNG among the notes' files. Returns its Markdown.
fn save_image(image: arboard::ImageData) -> Result<String, String> {
    let rgba = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned()).ok_or("?")?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let stored = store_name("image.png");
    let dir = config::config_dir().ok_or("?")?.join(FILES_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&stored), png.into_inner()).map_err(|e| e.to_string())?;
    Ok(markdown_for("image", &stored))
}

/// A file's name among the notes' files: unique, and without what would end a Markdown link.
fn store_name(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' }).collect();
    format!("{}-{}", &Uuid::new_v4().simple().to_string()[..8], if safe.is_empty() { "file".into() } else { safe })
}

fn markdown_for(name: &str, stored: &str) -> String {
    let label: String = name.chars().filter(|c| !matches!(c, '[' | ']' | '\n' | '\r')).collect();
    if is_image(stored) { format!("![{label}]({FILES_DIR}/{stored})") } else { format!("[{label}]({FILES_DIR}/{stored})") }
}

/// A file that opens as a document (and runs nothing, nor unpacks next to the notes' files).
fn is_document(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let documents = ["png", "jpg", "jpeg", "webp", "gif", "pdf", "txt", "md", "csv", "json", "xml", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "pages", "numbers", "key", "rtf", "mp3", "m4a", "wav", "mp4", "mov", "svg", "heic", "log", "sql"];
    documents.contains(&ext.as_str())
}

/// Opens a link of a note: a web address, or a document kept with the notes.
fn open_target(target: &str) {
    if target.starts_with("http://") || target.starts_with("https://") {
        return crate::terminal::links::open(target);
    }
    if let Some(path) = resolve(target).filter(|p| is_document(p)) {
        crate::terminal::links::open(&path.to_string_lossy());
    }
}

/// Reads the images the note shows that aren't loaded yet.
fn load_images(images: &mut HashMap<String, Img>, ctx: &egui::Context, text: &str) {
    for line in text.lines() {
        let Some(src) = image_src(line) else { continue };
        if images.contains_key(src) {
            continue;
        }
        let read = resolve(src).and_then(|p| std::fs::read(p).ok()).and_then(|b| image::load_from_memory(&b).ok());
        let img = match read {
            Some(image) => {
                let size = Vec2::new(image.width() as f32, image.height() as f32) / ctx.pixels_per_point();
                let image = if image.width().max(image.height()) > 2400 { image.thumbnail(2400, 2400) } else { image };
                let rgba = image.to_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                Img { texture: Some(ctx.load_texture(format!("note-image:{src}"), color, egui::TextureOptions::LINEAR)), size: Some(size) }
            }
            None => Img { texture: None, size: None },
        };
        images.insert(src.to_owned(), img);
    }
}

/// An image's size in a column `width` wide.
fn fit(size: Vec2, width: f32) -> Vec2 {
    size * (width / size.x.max(1.0)).min(IMAGE_MAX_H / size.y.max(1.0)).min(1.0)
}

/// A table's cell: its text's place (characters), and its text.
type Cell = (Range<usize>, String);

/// What is drawn over (or under) the text where the marks are hidden. Places are in characters.
#[derive(Clone, Debug)]
enum Deco {
    Bullet { at: usize, level: usize },
    /// `mark`: the byte between the brackets.
    Task { at: usize, mark: usize, done: bool },
    Quote { from: usize, to: usize },
    Rule { at: usize },
    Code { from: usize, to: usize },
    /// `top`: where the image starts below its line's top (under its Markdown when that shows).
    Image { at: usize, end: usize, src: String, top: f32 },
    /// Each row: where its line starts, and its cells (their places and text).
    Table { index: usize, rows: Vec<(usize, Vec<Cell>)> },
}

/// The note laid out for its TextEdit, every character kept (the hidden marks very small and
/// transparent), and what to draw with it.
#[derive(Clone, Default)]
pub(super) struct Live {
    job: LayoutJob,
    decos: Vec<Deco>,
    /// The links of the rendered lines: their places (characters) and addresses.
    links: Vec<(Range<usize>, String)>,
}

/// Lays out the note: the lines touched by `active` (bytes), or all of them when `raw`, show their
/// Markdown, its marks dimmed; the others are rendered, their marks hidden.
fn live(text: &str, active: Option<Range<usize>>, raw: bool, theme: &Theme, column: f32, image: &dyn Fn(&str) -> Option<Vec2>) -> Live {
    let lines = structure(text);
    let mut starts = Vec::with_capacity(lines.len());
    let (mut count, mut prev) = (0, 0);
    for l in &lines {
        count += text[prev..l.start].chars().count();
        starts.push(count);
        prev = l.start;
    }
    let ch = |k: usize, byte: usize| starts[k] + text[lines[k].start..byte].chars().count();
    let touched = |l: &Line| active.as_ref().is_some_and(|a| l.start <= a.end && a.start <= l.end);
    // Tables stay tables: their cells are edited in place.
    let shown: Vec<bool> = lines.iter().map(|l| raw || (!matches!(l.role, Role::Table | Role::Separator) && lines[l.block.clone()].iter().any(touched))).collect();
    let mut tables = 0;
    let title = lines.iter().position(|l| l.role == Role::Text && !text[l.start..l.end].trim().is_empty());

    let lh = BODY * 1.5;
    let body = TextFormat { line_height: Some(lh), ..format(note_font(BODY, "note"), theme.text) };
    let dim_color = theme.text_muted.gamma_multiply(0.8);
    let dim = TextFormat { color: dim_color, ..body.clone() };
    let mono = |color: Color32| TextFormat { line_height: Some(lh), ..format(FontId::monospace(BODY - 2.0), color) };
    let hidden = |h: f32| TextFormat { font_id: note_font(HIDDEN, "note"), color: Color32::TRANSPARENT, line_height: Some(h), ..Default::default() };
    let marker = TextFormat { font_id: note_font(BODY, "note-bold"), color: theme.accent, ..body.clone() };

    let mut b = Builder::default();
    let mut decos = Vec::new();
    let mut links = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let hide = !shown[i];
        let content = &text[l.start..l.end];
        let indent = content.len() - content.trim_start().len();
        let item = &content[indent..];
        let at = l.start + indent;
        let nl = l.end..l.next;
        let mut scratch = Vec::new();
        let sink = if hide { &mut links } else { &mut scratch };
        match l.role {
            Role::Code if hide => {
                b.lead = 14.0;
                b.put(text, l.start..l.next, TextFormat { line_height: Some(BODY * 1.45), ..mono(theme.text) });
            }
            Role::Code => b.put(text, l.start..l.next, TextFormat { background: theme.chrome_bg, ..mono(theme.text) }),
            Role::Fence if hide => {
                if i == l.block.start {
                    let last = l.block.end - 1;
                    decos.push(Deco::Code { from: ch(i, l.start), to: ch(last, lines[last].end) });
                }
                b.put(text, l.start..l.next, hidden(10.0));
            }
            Role::Fence => b.put(text, l.start..l.next, mono(dim_color)),
            Role::Table | Role::Separator if hide => {
                if i == l.block.start {
                    let rows = l
                        .block
                        .clone()
                        .filter(|k| lines[*k].role == Role::Table)
                        .map(|k| (ch(k, lines[k].start), cells_of(text, lines[k].start..lines[k].end).into_iter().map(|(_, c)| (ch(k, c.start)..ch(k, c.end), text[c].to_owned())).collect()))
                        .collect();
                    decos.push(Deco::Table { index: tables, rows });
                    tables += 1;
                }
                b.put(text, l.start..l.next, hidden(if l.role == Role::Separator { 3.0 } else { TABLE_ROW_H }));
            }
            Role::Table | Role::Separator => b.put(text, l.start..l.next, mono(theme.text)),
            Role::Text if item.is_empty() => b.put(text, l.start..l.next, body.clone()),
            Role::Text if title == Some(i) => {
                let marks = at + (item.len() - item.trim_start_matches('#').trim_start().len());
                let f = |c| TextFormat { line_height: Some(34.0), ..format(note_font(27.0, "note-bold"), c) };
                let m = if hide { hidden(34.0) } else { f(dim_color) };
                b.put(text, l.start..marks, m.clone());
                inline(&mut b, text, marks..l.end, &f(theme.text), theme, Some(&m), sink);
                b.put(text, nl, f(theme.text));
            }
            Role::Text => {
                if let Some(src) = image_src(item) {
                    let size = image(src).map_or(Vec2::new(column.min(320.0), 44.0), |s| fit(s, column));
                    let top = if hide { 0.0 } else { lh };
                    let h = top + size.y + 16.0;
                    decos.push(Deco::Image { at: ch(i, l.start), end: ch(i, l.end), src: src.to_owned(), top });
                    b.put(text, l.start..l.next, if hide { hidden(h) } else { TextFormat { line_height: Some(h), valign: egui::Align::TOP, ..dim.clone() } });
                } else if let Some(level) = heading(item) {
                    let size = [22.0, 19.0, 16.5, BODY, BODY, BODY][level - 1];
                    let f = |c| TextFormat { line_height: Some(size * 1.5), ..format(note_font(size, "note-bold"), c) };
                    let m = if hide { hidden(size * 1.5) } else { f(dim_color) };
                    b.put(text, l.start..at + level + 1, m.clone());
                    inline(&mut b, text, at + level + 1..l.end, &f(theme.text), theme, Some(&m), sink);
                    b.put(text, nl, f(theme.text));
                } else if item.len() >= 3 && b"-*_".iter().any(|c| item.bytes().all(|x| x == *c)) {
                    if hide {
                        decos.push(Deco::Rule { at: ch(i, l.start) });
                        b.put(text, l.start..l.next, hidden(22.0));
                    } else {
                        b.put(text, l.start..l.next, dim.clone());
                    }
                } else if item.starts_with('>') {
                    let quote = TextFormat { font_id: note_font(BODY, "note-italic"), color: theme.text_muted, ..body.clone() };
                    if hide {
                        let marks = at + 1 + usize::from(item.as_bytes().get(1) == Some(&b' '));
                        decos.push(Deco::Quote { from: ch(i, l.start), to: ch(i, l.end) });
                        b.lead = 18.0;
                        b.put(text, l.start..marks, hidden(lh));
                        inline(&mut b, text, marks..l.end, &quote, theme, Some(&hidden(lh)), sink);
                    } else {
                        b.put(text, l.start..at + 1, TextFormat { color: theme.accent, ..body.clone() });
                        inline(&mut b, text, at + 1..l.end, &quote, theme, Some(&dim), sink);
                    }
                    b.put(text, nl, body.clone());
                } else {
                    let (len, checked) = list_marker(item);
                    let level = content[..indent].chars().map(|c| if c == '\t' { 2 } else { 1 }).sum::<usize>() / 2;
                    let base = if checked == Some(true) { TextFormat { strikethrough: Stroke::new(1.0, dim_color), color: theme.text_muted, ..body.clone() } } else { body.clone() };
                    let m = if hide { hidden(lh) } else { dim.clone() };
                    let after = at + len;
                    if len == 0 {
                        inline(&mut b, text, l.start..l.end, &body, theme, Some(&m), sink);
                    } else if !hide {
                        b.put(text, l.start..after, marker.clone());
                        inline(&mut b, text, after..l.end, &base, theme, Some(&m), sink);
                    } else {
                        let indent_w = level as f32 * 22.0;
                        if let Some(done) = checked {
                            decos.push(Deco::Task { at: ch(i, after), mark: at + 3, done });
                            b.lead = indent_w + 28.0;
                            b.put(text, l.start..after, hidden(lh));
                        } else if item.as_bytes()[0].is_ascii_digit() {
                            b.lead = indent_w + 2.0;
                            b.put(text, l.start..at, hidden(lh));
                            b.put(text, at..after, marker.clone());
                        } else {
                            decos.push(Deco::Bullet { at: ch(i, after), level });
                            b.lead = indent_w + 22.0;
                            b.put(text, l.start..after, hidden(lh));
                        }
                        inline(&mut b, text, after..l.end, &base, theme, Some(&m), sink);
                    }
                    b.put(text, nl, body.clone());
                }
            }
        }
    }
    Live { job: b.job, decos, links }
}

/// What was clicked among the drawings over the note.
#[derive(Default)]
struct Hits {
    /// A task's box: the byte of its mark.
    toggle: Option<usize>,
    /// Where the cursor goes (characters).
    cursor: Option<usize>,
    /// A link, or an image, to open.
    open: Option<String>,
    /// A table's cell clicked: the table, its row and column.
    cell: Option<(usize, usize, usize)>,
    /// Where the cell being edited is.
    cell_rect: Option<Rect>,
    table_op: Option<(usize, TableOp)>,
    /// A file to save elsewhere, or to show in its folder.
    save: Option<String>,
    reveal: Option<String>,
}

/// Draws what the rendered lines show instead of their marks: bullets, boxes, quotes, rules, blocks of
/// code, images, tables, files; `back` takes what goes under the text. Links get the hand.
#[allow(clippy::too_many_arguments)]
fn paint_live(ui: &Ui, live: &Live, galley: &egui::Galley, origin: Pos2, column: f32, back: egui::layers::ShapeIdx, text: &egui::Response, images: &HashMap<String, Img>, theme: &Theme, t: &Strings, editable: bool, editing: Option<(usize, usize, usize)>) -> Hits {
    let mut hits = Hits::default();
    let painter = ui.painter();
    let row = |c: usize| galley.pos_from_cursor(CCursor::new(c)).translate(origin.to_vec2());
    let right = origin.x + column;
    let mut under = Vec::new();
    let line = Stroke::new(1.0, theme.tab_hover.gamma_multiply(1.5));
    for deco in &live.decos {
        match deco {
            Deco::Bullet { at, level } => {
                let r = row(*at);
                let c = Pos2::new(r.min.x - 11.0, r.center().y);
                if level % 2 == 0 {
                    painter.circle_filled(c, 2.8, theme.accent);
                } else {
                    painter.circle_stroke(c, 2.6, Stroke::new(1.2, theme.accent));
                }
            }
            Deco::Task { at, mark, done } => {
                let r = row(*at);
                let c = Pos2::new(r.min.x - 15.0, r.center().y);
                let sense = if editable { Sense::click() } else { Sense::hover() };
                let mut boxed = ui.interact(Rect::from_center_size(c, Vec2::splat(22.0)), Id::new(("notes-task", *mark)), sense);
                if editable {
                    boxed = boxed.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                if *done {
                    painter.circle_filled(c, 8.5, theme.accent);
                    painter.add(Shape::line(vec![c + Vec2::new(-3.8, 0.2), c + Vec2::new(-1.0, 3.0), c + Vec2::new(4.0, -3.0)], Stroke::new(1.8, theme.bg)));
                } else {
                    painter.circle_stroke(c, 8.0, Stroke::new(1.4, if boxed.hovered() { theme.accent } else { theme.text_muted }));
                }
                if boxed.clicked() {
                    hits.toggle = Some(*mark);
                }
            }
            Deco::Quote { from, to } => {
                let (a, b) = (row(*from), row(*to));
                painter.rect_filled(Rect::from_min_max(Pos2::new(origin.x + 2.0, a.min.y + 3.0), Pos2::new(origin.x + 5.0, b.max.y - 3.0)), 1.5, theme.accent.gamma_multiply(0.6));
            }
            Deco::Rule { at } => {
                painter.hline(origin.x..=right, row(*at).center().y, line);
            }
            Deco::Code { from, to } => {
                let r = Rect::from_min_max(Pos2::new(origin.x, row(*from).min.y), Pos2::new(right, row(*to).max.y));
                under.push(Shape::rect_filled(r, 8.0, theme.chrome_bg));
                under.push(Shape::rect_stroke(r, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside));
            }
            Deco::Image { at, end, src, top } => {
                let img = images.get(src);
                let size = img.and_then(|i| i.size).map_or(Vec2::new(column.min(320.0), 44.0), |s| fit(s, column));
                let rect = Rect::from_min_size(Pos2::new(origin.x, row(*at).min.y + top + 8.0), size);
                match img.and_then(|i| i.texture.as_ref()) {
                    Some(texture) => egui::Image::from_texture(egui::load::SizedTexture::new(texture.id(), size)).corner_radius(8).paint_at(ui, rect),
                    None => {
                        under.push(Shape::rect_filled(rect, 8.0, theme.chrome_bg));
                        painter.text(rect.center(), Align2::CENTER_CENTER, format!("{}  ·  {src}", t.notes_image_missing), FontId::proportional(12.0), theme.text_muted);
                    }
                }
                let resp = ui.interact(rect, Id::new(("notes-image", *at)), Sense::click()).on_hover_text(t.notes_image_open);
                if resp.double_clicked() {
                    hits.open = Some(src.clone());
                } else if resp.clicked() && editable {
                    hits.cursor = Some(*end);
                }
            }
            Deco::Table { index, rows } => {
                let Some(((first, _), (last, _))) = rows.first().zip(rows.last()) else { continue };
                let cols = rows.iter().map(|(_, c)| c.len()).max().unwrap_or(0).max(1);
                let regular = format(note_font(14.0, "note"), theme.text);
                let bold = format(note_font(14.0, "note-bold"), theme.text);
                let job = |s: &str, head: bool| rich(s, if head { &bold } else { &regular }, theme).0;
                let mut widths = vec![70.0_f32; cols];
                for (k, (_, cells)) in rows.iter().enumerate() {
                    for (j, (_, s)) in cells.iter().enumerate() {
                        let w = painter.layout_job(job(s, k == 0)).size().x + 24.0;
                        widths[j] = widths[j].max(w.min(column * 0.6));
                    }
                }
                let total: f32 = widths.iter().sum();
                if total > column {
                    widths.iter_mut().for_each(|w| *w *= column / total);
                }
                let total: f32 = widths.iter().sum();
                let outer = Rect::from_min_max(Pos2::new(origin.x, row(*first).min.y), Pos2::new(origin.x + total, row(*last).max.y));
                let head_bottom = rows.get(1).map_or(outer.max.y, |(at, _)| row(*at).min.y);
                let corners = egui::CornerRadius { nw: 6, ne: 6, sw: if rows.len() == 1 { 6 } else { 0 }, se: if rows.len() == 1 { 6 } else { 0 } };
                under.push(Shape::rect_filled(Rect::from_min_max(outer.min, Pos2::new(outer.max.x, head_bottom)), corners, theme.chrome_bg));
                for (k, (at, cells)) in rows.iter().enumerate() {
                    let r = row(*at);
                    if k > 0 {
                        painter.hline(outer.x_range(), r.min.y, line);
                    }
                    let mut x = origin.x;
                    for (j, w) in widths.iter().enumerate() {
                        let cell = Rect::from_min_max(Pos2::new(x, r.min.y), Pos2::new(x + w, r.max.y));
                        let here = (*index, k, j);
                        if editing == Some(here) {
                            hits.cell_rect = Some(cell);
                        } else {
                            if let Some((_, s)) = cells.get(j) {
                                let mut job = job(s, k == 0);
                                job.wrap = egui::text::TextWrapping::truncate_at_width((w - 16.0).max(8.0));
                                let g = painter.layout_job(job);
                                painter.galley(Pos2::new(cell.min.x + 8.0, cell.center().y - g.size().y / 2.0), g, theme.text);
                            }
                            if editable {
                                let resp = ui.interact(cell, Id::new(("notes-cell", *index, k, j)), Sense::click()).on_hover_cursor(egui::CursorIcon::Text);
                                if resp.clicked() {
                                    hits.cell = Some(here);
                                }
                                resp.context_menu(|ui| {
                                    ui.set_min_width(200.0);
                                    if ui.button(t.notes_row_below).clicked() {
                                        hits.table_op = Some((*index, TableOp::RowBelow(k)));
                                    }
                                    if ui.button(t.notes_col_right).clicked() {
                                        hits.table_op = Some((*index, TableOp::ColRight(j)));
                                    }
                                    ui.separator();
                                    if ui.add_enabled(k > 0, egui::Button::new(t.notes_delete_row)).clicked() {
                                        hits.table_op = Some((*index, TableOp::DeleteRow(k)));
                                    }
                                    if ui.add_enabled(cols > 1, egui::Button::new(t.notes_delete_col)).clicked() {
                                        hits.table_op = Some((*index, TableOp::DeleteCol(j)));
                                    }
                                });
                            }
                        }
                        x += w;
                        if j + 1 < cols {
                            painter.vline(x, outer.y_range(), line);
                        }
                    }
                }
                painter.rect_stroke(outer, 6.0, line, egui::StrokeKind::Inside);
            }
        }
    }
    // Files: a chip with a clip.
    for (range, url) in &live.links {
        let (a, b) = (row(range.start), row(range.end));
        if is_file(url) && (a.min.y - b.min.y).abs() < 1.0 {
            let chip = Rect::from_min_max(Pos2::new(a.min.x - 21.0, a.min.y + 2.0), Pos2::new(b.max.x + 7.0, a.max.y - 2.0));
            let resp = ui.interact(chip, Id::new(("notes-file", range.start)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            under.push(Shape::rect_filled(chip, 6.0, if resp.hovered() { theme.tab_hover.gamma_multiply(1.6) } else { theme.tab_hover }));
            paint_clip_icon(painter, Pos2::new(chip.min.x + 10.0, chip.center().y), theme.accent);
            if resp.clicked() {
                hits.open = Some(url.clone());
            }
            resp.context_menu(|ui| {
                ui.set_min_width(200.0);
                if ui.button(t.notes_file_open).clicked() {
                    hits.open = Some(url.clone());
                }
                if ui.button(t.notes_file_save).clicked() {
                    hits.save = Some(url.clone());
                }
                if ui.button(t.reveal_file).clicked() {
                    hits.reveal = Some(url.clone());
                }
            });
        }
    }
    painter.set(back, Shape::Vec(under));
    // Links: the hand over them, opened by a click.
    if let Some(pos) = text.hover_pos() {
        let c = galley.cursor_from_pos(pos - origin).index.0;
        let over = live.links.iter().find(|(range, url)| {
            let (a, b) = (row(range.start), row(range.end));
            let lead = if is_file(url) { 21.0 } else { 0.0 };
            if (a.min.y - b.min.y).abs() < 1.0 { a.y_range().contains(pos.y) && (a.min.x - lead..=b.max.x + 4.0).contains(&pos.x) } else { range.start <= c && c <= range.end }
        });
        if let Some((_, url)) = over {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if text.clicked() {
                hits.open = Some(url.clone());
            }
        }
    }
    hits
}


fn set_cursor(ctx: &egui::Context, id: Id, chars: usize) {
    select(ctx, id, chars..chars);
}

/// Selects characters `range` in the TextEdit `id`.
fn select(ctx: &egui::Context, id: Id, range: Range<usize>) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(range.start), CCursor::new(range.end))));
    state.store(ctx, id);
}

fn color_key(theme: &Theme) -> u64 {
    let [a, b, c, d] = theme.text.to_array();
    let [e, f, g, h] = theme.accent.to_array();
    u64::from_le_bytes([a, b, c, d, e, f, g, h]) ^ u64::from(u32::from_le_bytes(theme.text_muted.to_array())).rotate_left(17)
}

fn single_line(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
    job
}

/// A note's title: its first line with text, without the "#" of a heading.
fn title_of(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.trim_start_matches('#').trim()).filter(|l| !l.is_empty())
}

/// The line after the title, without its Markdown marks.
fn preview_of(text: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("```"));
    lines.next()?;
    let line = lines.next()?;
    let line = line.trim_start_matches('#').trim_start_matches('>').trim_start();
    let (marker, _) = list_marker(line);
    Some(line[marker..].replace("**", "").replace('`', ""))
}

fn date_section(ms: i64, now: chrono::DateTime<chrono::Local>, t: &Strings) -> String {
    let day = local(ms).date_naive();
    let today = now.date_naive();
    let days = (today - day).num_days();
    if days <= 0 {
        t.notes_today.to_owned()
    } else if days == 1 {
        t.notes_yesterday.to_owned()
    } else if days < 7 {
        t.notes_week.to_owned()
    } else if days < 30 {
        t.notes_month.to_owned()
    } else if day.year() == today.year() {
        let month = t.months[day.month0() as usize];
        let mut chars = month.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
    } else {
        day.year().to_string()
    }
}

/// The time today, the date otherwise.
fn short_date(ms: i64, now: chrono::DateTime<chrono::Local>, t: &Strings) -> String {
    let d = local(ms);
    if d.date_naive() == now.date_naive() { d.format("%H:%M").to_string() } else { d.format(t.notes_date_format).to_string() }
}

fn long_date(ms: i64, t: &Strings) -> String {
    let d = local(ms);
    t.notes_long_date
        .replace("{day}", &d.day().to_string())
        .replace("{month}", t.months[d.month0() as usize])
        .replace("{year}", &d.year().to_string())
        .replace("{time}", &d.format("%H:%M").to_string())
}

/// The list mark starting `line` (already without its indent): its length, and for a task whether it
/// is ticked. "- ", "* ", "+ ", "1. ", "1) ", "- [ ] ", "- [x] ".
fn list_marker(line: &str) -> (usize, Option<bool>) {
    let b = line.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        if b.len() >= 5 && b[2] == b'[' && matches!(b[3], b' ' | b'x' | b'X') && b[4] == b']' && (b.len() == 5 || b[5] == b' ') {
            return (b.len().min(6), Some(b[3] != b' '));
        }
        return (2, None);
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=3).contains(&digits) && b.len() >= digits + 2 && matches!(b[digits], b'.' | b')') && b[digits + 1] == b' ' {
        return (digits + 2, None);
    }
    (0, None)
}

/// "## " starting a line: the heading's level.
fn heading(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|c| *c == b'#').count();
    ((1..=6).contains(&level) && line.as_bytes().get(level) == Some(&b' ')).then_some(level)
}

fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(b, _)| b)
}

/// The line holding byte `at`: where it starts and ends (before its newline).
fn line_at(text: &str, at: usize) -> Range<usize> {
    let start = text[..at].rfind('\n').map_or(0, |p| p + 1);
    let end = text[at..].find('\n').map_or(text.len(), |p| at + p);
    start..end
}

/// Return pressed in a list: the next line starts the next item; pressed on an empty item, the list
/// ends there instead (as in macOS Notes). `cursor` is just after the new line. Returns where the
/// cursor goes when the text changed.
fn continue_list(text: &mut String, cursor: usize) -> Option<usize> {
    let byte = byte_of(text, cursor);
    if byte == 0 || text.as_bytes()[byte - 1] != b'\n' {
        return None;
    }
    let prev = line_at(text, byte - 1);
    let content = &text[prev.clone()];
    let indent = content.len() - content.trim_start().len();
    let item = &content[indent..];
    let (len, checked) = list_marker(item);
    if len == 0 {
        return None;
    }
    if item[len..].trim().is_empty() {
        text.replace_range(prev.start..byte, "");
        return Some(text[..prev.start].chars().count());
    }
    let marker = if checked.is_some() {
        format!("{} [ ] ", &item[..1])
    } else if let Ok(n) = item[..len - 2].parse::<u32>() {
        format!("{}{} ", n + 1, &item[len - 2..len - 1])
    } else {
        item[..len].to_owned()
    };
    let insert = format!("{}{marker}", &content[..indent]);
    let added = insert.chars().count();
    text.insert_str(byte, &insert);
    Some(cursor + added)
}

/// Puts `kind` at the start of the line of character `cursor`, or takes it away when it is there
/// (a heading goes #, ##, ###, then plain). Returns where the cursor goes.
fn toggle_prefix(text: &mut String, cursor: usize, kind: Prefix) -> usize {
    let byte = byte_of(text, cursor);
    let line = line_at(text, byte);
    let content = &text[line.clone()];
    let indent = content.len() - content.trim_start().len();
    let item = &content[indent..];
    let level = heading(item);
    let (list_len, checked) = list_marker(item);
    let numbered = list_len > 0 && item.as_bytes()[0].is_ascii_digit();
    let quote_len = if item.starts_with('>') { 1 + usize::from(item.as_bytes().get(1) == Some(&b' ')) } else { 0 };
    // The mark the line has, replaced by the new one.
    let any = level.map_or(if quote_len > 0 { quote_len } else { list_len }, |l| l + 1);
    let (old_len, new) = match kind {
        Prefix::Heading => match level {
            Some(l) if l >= 3 => (l + 1, String::new()),
            Some(l) => (l + 1, format!("{} ", "#".repeat(l + 1))),
            None => (any, "# ".to_owned()),
        },
        Prefix::Bullet if list_len > 0 && checked.is_none() && !numbered => (list_len, String::new()),
        Prefix::Bullet => (any, "- ".to_owned()),
        Prefix::Numbered if numbered => (list_len, String::new()),
        Prefix::Numbered => (any, "1. ".to_owned()),
        Prefix::Check if checked.is_some() => (list_len, String::new()),
        Prefix::Check => (any, "- [ ] ".to_owned()),
        Prefix::Quote if quote_len > 0 => (quote_len, String::new()),
        Prefix::Quote => (any, "> ".to_owned()),
    };
    let start = line.start + indent;
    let old_chars = text[start..start + old_len].chars().count();
    text.replace_range(start..start + old_len, &new);
    let line_chars = text[..start].chars().count();
    let new_chars = new.chars().count();
    if cursor >= line_chars + old_chars { cursor + new_chars - old_chars } else { line_chars + new_chars }
}

fn note_font(size: f32, face: &str) -> FontId {
    FontId::new(size, FontFamily::Name(face.into()))
}

fn format(font: FontId, color: Color32) -> TextFormat {
    TextFormat { font_id: font, color, ..Default::default() }
}

/// A layout being built, with the room to leave before the next text.
#[derive(Default)]
struct Builder {
    job: LayoutJob,
    lead: f32,
}

impl Builder {
    fn put(&mut self, text: &str, range: Range<usize>, format: TextFormat) {
        if !range.is_empty() {
            self.job.append(&text[range], std::mem::take(&mut self.lead), format);
        }
    }
}

/// One line rendered (a table's cell): the marks gone, and the links' places (in characters of the
/// laid out text).
fn rich(text: &str, base: &TextFormat, theme: &Theme) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let mut b = Builder::default();
    let mut links = Vec::new();
    inline(&mut b, text, 0..text.len(), base, theme, None, &mut links);
    if b.job.sections.is_empty() {
        // An empty item still takes a line.
        b.job.append(" ", 0.0, base.clone());
    }
    (b.job, links)
}

/// The text of one line: **bold**, *italics*, `code`, ~~struck~~ and links, their marks in `marks`
/// (dimmed, or hidden), or dropped when None. The links are listed in `links`.
fn inline(b: &mut Builder, text: &str, range: Range<usize>, base: &TextFormat, theme: &Theme, marks: Option<&TextFormat>, links: &mut Vec<(Range<usize>, String)>) {
    let s = &text[range.clone()];
    let off = range.start;
    let bytes = s.as_bytes();
    let face = |name: &str| TextFormat { font_id: note_font(base.font_id.size, name), ..base.clone() };
    let italic_face = if base.font_id.family == FontFamily::Name("note-bold".into()) { "note-bold-italic" } else { "note-italic" };
    let bold_face = if base.font_id.family == FontFamily::Name("note-italic".into()) { "note-bold-italic" } else { "note-bold" };
    let hidden = marks.is_some_and(|m| m.color == Color32::TRANSPARENT);
    let link_format = TextFormat { color: theme.accent, underline: Stroke::new(1.0, theme.accent.gamma_multiply(0.6)), ..base.clone() };
    let (mut i, mut plain) = (0, 0);
    while i < bytes.len() {
        let rest = &s[i..];
        // (marks before, the inside's length, marks after, the inside's format, a link's address)
        let span: Option<(usize, usize, usize, TextFormat, Option<String>)> = if rest.starts_with("**") || rest.starts_with("__") {
            rest[2..].find(&rest[..2]).filter(|n| *n > 0).map(|n| (2, n, 2, face(bold_face), None))
        } else if let Some(after) = rest.strip_prefix("~~") {
            after.find("~~").filter(|n| *n > 0).map(|n| (2, n, 2, TextFormat { strikethrough: Stroke::new(1.0, base.color), ..base.clone() }, None))
        } else if bytes[i] == b'`' {
            rest[1..].find('`').filter(|n| *n > 0).map(|n| (1, n, 1, TextFormat { font_id: FontId::monospace(base.font_id.size - 1.5), background: theme.tab_hover, color: theme.ansi[3], ..base.clone() }, None))
        } else if let Some((front, label, back, url)) = md_link(rest) {
            let format = if is_file(url) { TextFormat { color: theme.accent, ..base.clone() } } else { link_format.clone() };
            Some((front, label, back, format, Some(url.to_owned())))
        } else if (bytes[i] == b'*' || (bytes[i] == b'_' && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()))) && bytes.get(i + 1).is_some_and(|c| !c.is_ascii_whitespace() && *c != bytes[i]) {
            let mark = bytes[i] as char;
            rest[1..]
                .find(mark)
                .filter(|n| *n > 0 && !rest[1..1 + n].ends_with(' '))
                .filter(|n| mark == '*' || !rest.as_bytes().get(n + 2).is_some_and(|c| c.is_ascii_alphanumeric()))
                .map(|n| (1, n, 1, face(italic_face), None))
        } else if (rest.starts_with("http://") || rest.starts_with("https://")) && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let n = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let n = n - (rest[..n].len() - rest[..n].trim_end_matches([')', '.', ',', ';']).len());
            Some((0, n, 0, link_format.clone(), Some(rest[..n].to_owned())))
        } else {
            None
        };
        match span {
            Some((front, inner, back, format, link)) => {
                b.put(text, off + plain..off + i, base.clone());
                // A file: room for its clip.
                let chip = hidden && link.as_deref().is_some_and(is_file);
                if let Some(m) = marks {
                    if chip {
                        b.lead += 21.0;
                    }
                    b.put(text, off + i..off + i + front, m.clone());
                }
                let from = b.job.text.chars().count();
                b.put(text, off + i + front..off + i + front + inner, format);
                if let Some(url) = link {
                    links.push((from..b.job.text.chars().count(), url));
                }
                if let Some(m) = marks {
                    b.put(text, off + i + front + inner..off + i + front + inner + back, m.clone());
                }
                if chip {
                    b.lead = 9.0;
                }
                i += front + inner + back;
                plain = i;
            }
            None => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    b.put(text, off + plain..range.end, base.clone());
}


/// A toolbar button: an icon, lit when hovered or `on`.
fn tool_button(ui: &Ui, rect: Rect, salt: &str, theme: &Theme, paint: fn(&egui::Painter, Pos2, Color32), on: bool) -> egui::Response {
    let resp = ui.interact(rect, Id::new(salt), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() || on {
        ui.painter().rect_filled(rect, 7.0, if resp.hovered() { theme.tab_hover } else { theme.tab_active });
    }
    paint(ui.painter(), rect.center(), if resp.hovered() || on { theme.text } else { theme.text_muted });
    resp
}

/// A folder with a plus: a new folder.
fn paint_new_folder_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    paint_folder_icon(painter, c + Vec2::new(-1.5, 0.5), color);
    let p = c + Vec2::new(6.0, -5.0);
    painter.circle_filled(p, 4.2, Color32::TRANSPARENT);
    painter.line_segment([p + Vec2::new(-2.5, 0.0), p + Vec2::new(2.5, 0.0)], Stroke::new(1.4, color));
    painter.line_segment([p + Vec2::new(0.0, -2.5), p + Vec2::new(0.0, 2.5)], Stroke::new(1.4, color));
}

/// A chevron to the left: back to the folder above.
fn paint_back_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.add(Shape::line(vec![c + Vec2::new(2.5, -5.0), c + Vec2::new(-2.5, 0.0), c + Vec2::new(2.5, 5.0)], Stroke::new(1.6, color)));
}

/// A page with lines: the notes' icon.
pub(super) fn paint_notes_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let page = Rect::from_center_size(c, Vec2::new(11.0, 13.0));
    painter.rect_stroke(page, 2.0, stroke, egui::StrokeKind::Middle);
    for dy in [-2.5, 0.5, 3.5] {
        let w = if dy > 3.0 { 3.0 } else { 6.0 };
        painter.line_segment([Pos2::new(page.min.x + 2.5, c.y + dy), Pos2::new(page.min.x + 2.5 + w, c.y + dy)], Stroke::new(1.1, color));
    }
}

pub(super) fn paint_folder_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let body = Rect::from_center_size(c + Vec2::new(0.0, 1.0), Vec2::new(12.0, 8.5));
    painter.rect_stroke(body, 1.5, stroke, egui::StrokeKind::Middle);
    painter.line_segment([Pos2::new(body.min.x + 0.5, body.min.y - 1.5), Pos2::new(body.min.x + 4.5, body.min.y - 1.5)], Stroke::new(1.6, color));
}

pub(super) fn paint_trash_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    painter.line_segment([c + Vec2::new(-5.5, -4.0), c + Vec2::new(5.5, -4.0)], stroke);
    painter.line_segment([c + Vec2::new(-1.5, -5.5), c + Vec2::new(1.5, -5.5)], stroke);
    let can = [c + Vec2::new(-4.0, -4.0), c + Vec2::new(-3.2, 5.5), c + Vec2::new(3.2, 5.5), c + Vec2::new(4.0, -4.0)];
    painter.add(egui::Shape::line(can.to_vec(), stroke));
}

fn paint_close_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let (stroke, d) = (Stroke::new(1.4, color), 4.0);
    painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
    painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
}

/// A square and a pencil: a new note.
fn paint_compose_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let points = [c + Vec2::new(1.0, -6.0), c + Vec2::new(-6.0, -6.0), c + Vec2::new(-6.0, 6.0), c + Vec2::new(6.0, 6.0), c + Vec2::new(6.0, -1.0)];
    painter.add(egui::Shape::line(points.to_vec(), stroke));
    painter.line_segment([c + Vec2::new(-1.5, 1.5), c + Vec2::new(6.0, -6.0)], Stroke::new(1.8, color));
}

fn paint_heading_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.text(c, Align2::CENTER_CENTER, "Aa", note_font(14.0, "note-bold"), color);
}

fn paint_bullets_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    for dy in [-4.0, 0.0, 4.0] {
        painter.circle_filled(c + Vec2::new(-5.0, dy), 1.3, color);
        painter.line_segment([c + Vec2::new(-2.0, dy), c + Vec2::new(6.0, dy)], Stroke::new(1.2, color));
    }
}

fn paint_check_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.circle_stroke(c, 6.5, stroke);
    painter.add(egui::Shape::line(vec![c + Vec2::new(-3.0, 0.2), c + Vec2::new(-0.8, 2.6), c + Vec2::new(3.2, -2.4)], Stroke::new(1.5, color)));
}

type Paint = fn(&egui::Painter, Pos2, Color32);

/// The toolbar's formatting buttons, None between their groups.
fn format_buttons(t: &Strings) -> Vec<Option<(Format, String, Paint)>> {
    let key = |k: char| if cfg!(target_os = "macos") { format!("⌘ {k}") } else { format!("Ctrl+{k}") };
    vec![
        Some((Format::Heading, t.notes_heading.to_owned(), paint_heading_icon as Paint)),
        Some((Format::Bold, format!("{} ({})", t.notes_bold, key('B')), paint_bold_icon)),
        Some((Format::Italic, format!("{} ({})", t.notes_italic, key('I')), paint_italic_icon)),
        Some((Format::Strike, t.notes_strike.to_owned(), paint_strike_icon)),
        Some((Format::Code, t.notes_code.to_owned(), paint_code_icon)),
        Some((Format::Link, t.notes_link.to_owned(), paint_link_icon)),
        None,
        Some((Format::Bullet, t.notes_bullets.to_owned(), paint_bullets_icon)),
        Some((Format::Numbered, t.notes_numbered.to_owned(), paint_numbered_icon)),
        Some((Format::Check, t.notes_checklist.to_owned(), paint_check_icon)),
        Some((Format::Quote, t.notes_quote.to_owned(), paint_quote_icon)),
        Some((Format::Rule, t.notes_rule.to_owned(), paint_rule_icon)),
        None,
        Some((Format::Table, t.notes_table.to_owned(), paint_table_icon)),
        Some((Format::Image, t.notes_image.to_owned(), paint_image_icon)),
        Some((Format::Attach, t.notes_attach.to_owned(), paint_clip_icon)),
    ]
}

fn paint_bold_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.text(c, Align2::CENTER_CENTER, "B", note_font(15.0, "note-bold"), color);
}

fn paint_italic_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.text(c, Align2::CENTER_CENTER, "I", note_font(15.0, "note-italic"), color);
}

fn paint_strike_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.text(c, Align2::CENTER_CENTER, "S", note_font(15.0, "note"), color);
    painter.hline((c.x - 6.0)..=(c.x + 6.0), c.y + 0.5, Stroke::new(1.3, color));
}

fn paint_code_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.add(Shape::line(vec![c + Vec2::new(-3.5, -4.0), c + Vec2::new(-7.0, 0.0), c + Vec2::new(-3.5, 4.0)], stroke));
    painter.add(Shape::line(vec![c + Vec2::new(3.5, -4.0), c + Vec2::new(7.0, 0.0), c + Vec2::new(3.5, 4.0)], stroke));
    painter.line_segment([c + Vec2::new(1.3, -5.0), c + Vec2::new(-1.3, 5.0)], stroke);
}

/// Two links of a chain.
fn paint_link_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    for dx in [-3.2, 3.2] {
        painter.rect_stroke(Rect::from_center_size(c + Vec2::new(dx, 0.0), Vec2::new(9.0, 6.0)), 3.0, stroke, egui::StrokeKind::Middle);
    }
}

fn paint_numbered_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    for (k, dy) in [-3.5_f32, 3.5].into_iter().enumerate() {
        painter.text(c + Vec2::new(-4.5, dy), Align2::CENTER_CENTER, (k + 1).to_string(), FontId::proportional(7.5), color);
        painter.line_segment([c + Vec2::new(-1.0, dy), c + Vec2::new(7.0, dy)], Stroke::new(1.2, color));
    }
}

fn paint_quote_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.rect_filled(Rect::from_center_size(c + Vec2::new(-5.5, 0.0), Vec2::new(2.2, 12.0)), 1.0, color);
    for dy in [-3.5, 0.0, 3.5] {
        let w = if dy > 3.0 { 5.0 } else { 9.0 };
        painter.line_segment([c + Vec2::new(-2.0, dy), c + Vec2::new(-2.0 + w, dy)], Stroke::new(1.2, color));
    }
}

fn paint_rule_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.hline((c.x - 7.0)..=(c.x + 7.0), c.y, Stroke::new(1.5, color));
    for dy in [-4.5, 4.5] {
        painter.hline((c.x - 7.0)..=(c.x + 7.0), c.y + dy, Stroke::new(1.0, color.gamma_multiply(0.4)));
    }
}

fn paint_table_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let r = Rect::from_center_size(c, Vec2::new(14.0, 11.0));
    painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Middle);
    painter.hline(r.x_range(), r.min.y + 3.7, stroke);
    painter.hline(r.x_range(), r.min.y + 7.3, stroke);
    painter.vline(c.x, r.y_range(), stroke);
}

fn paint_image_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let r = Rect::from_center_size(c, Vec2::new(14.0, 11.0));
    painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Middle);
    painter.circle_filled(r.min + Vec2::new(10.0, 3.3), 1.4, color);
    painter.add(Shape::line(vec![Pos2::new(r.min.x + 1.0, r.max.y - 1.5), Pos2::new(r.min.x + 5.0, r.min.y + 5.0), Pos2::new(r.min.x + 8.5, r.max.y - 3.0), Pos2::new(r.min.x + 10.5, r.max.y - 4.8), Pos2::new(r.max.x - 1.0, r.max.y - 1.5)], stroke));
}

/// A paperclip, leaning.
fn paint_clip_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let arc = |center: Vec2, radius: f32, from: f32, to: f32| (0..=6).map(move |k| center + radius * Vec2::angled(from + (to - from) * k as f32 / 6.0));
    let mut points: Vec<Vec2> = Vec::new();
    points.push(Vec2::new(1.6, -1.5));
    points.push(Vec2::new(1.6, 3.2));
    points.extend(arc(Vec2::new(0.0, 3.2), 1.6, 0.0, std::f32::consts::PI));
    points.push(Vec2::new(-1.6, -3.8));
    points.extend(arc(Vec2::new(0.6, -3.8), 2.2, std::f32::consts::PI, 2.0 * std::f32::consts::PI));
    points.push(Vec2::new(2.8, 4.0));
    points.extend(arc(Vec2::new(0.0, 4.0), 2.8, 0.0, std::f32::consts::PI));
    points.push(Vec2::new(-2.8, -1.0));
    let rot = egui::emath::Rot2::from_angle(0.7);
    painter.add(Shape::line(points.into_iter().map(|p| c + rot * p).collect(), Stroke::new(1.2, color)));
}

fn paint_more_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    for dx in [-5.0, 0.0, 5.0] {
        painter.circle_filled(c + Vec2::new(dx, 0.0), 1.5, color);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_previews() {
        assert_eq!(title_of("\n  # Courses \n- [ ] pain\n- lait"), Some("Courses"));
        assert_eq!(preview_of("# Courses\n\n- [ ] **pain** frais"), Some("pain frais".to_owned()));
        assert_eq!(title_of("  \n "), None);
        assert_eq!(preview_of("Seule ligne"), None);
    }

    #[test]
    fn list_marks() {
        assert_eq!(list_marker("- [ ] a"), (6, Some(false)));
        assert_eq!(list_marker("* [x] a"), (6, Some(true)));
        assert_eq!(list_marker("- [ ]"), (5, Some(false)));
        assert_eq!(list_marker("- a"), (2, None));
        assert_eq!(list_marker("12. a"), (4, None));
        assert_eq!(list_marker("-a"), (0, None));
        assert_eq!(heading("## Titre"), Some(2));
        assert_eq!(heading("#hashtag"), None);
    }

    #[test]
    fn return_continues_lists() {
        // "- [x] pain\n" with the cursor after the new line: the next task starts unticked.
        let mut text = "- [x] pain\n".to_owned();
        assert_eq!(continue_list(&mut text, 11), Some(17));
        assert_eq!(text, "- [x] pain\n- [ ] ");
        let mut text = "  3. trois\nsuite".to_owned();
        assert_eq!(continue_list(&mut text, 11), Some(16));
        assert_eq!(text, "  3. trois\n  4. suite");
        // An empty item ends the list.
        let mut text = "- a\n- \n".to_owned();
        assert_eq!(continue_list(&mut text, 7), Some(4));
        assert_eq!(text, "- a\n");
        let mut text = "plain\n".to_owned();
        assert_eq!(continue_list(&mut text, 6), None);
    }

    #[test]
    fn toolbar_prefixes() {
        let mut text = "> cité".to_owned();
        toggle_prefix(&mut text, 2, Prefix::Numbered);
        assert_eq!(text, "1. cité");
        toggle_prefix(&mut text, 2, Prefix::Quote);
        assert_eq!(text, "> cité");
        toggle_prefix(&mut text, 2, Prefix::Quote);
        assert_eq!(text, "cité");
        let mut text = "Titre\nligne".to_owned();
        assert_eq!(toggle_prefix(&mut text, 8, Prefix::Check), 14);
        assert_eq!(text, "Titre\n- [ ] ligne");
        assert_eq!(toggle_prefix(&mut text, 14, Prefix::Bullet), 10);
        assert_eq!(text, "Titre\n- ligne");
        toggle_prefix(&mut text, 10, Prefix::Bullet);
        assert_eq!(text, "Titre\nligne");
        for expected in ["# ligne", "## ligne", "### ligne", "ligne"] {
            toggle_prefix(&mut text, 7, Prefix::Heading);
            assert_eq!(&text[6..], expected);
        }
    }

    #[test]
    fn tables() {
        let text = "Titre\n| a | b |\n|---|:-:|\n| 1 |  |\nfin";
        let roles: Vec<Role> = structure(text).iter().map(|l| l.role).collect();
        assert_eq!(roles, [Role::Text, Role::Table, Role::Separator, Role::Table, Role::Text]);
        let row = structure(text)[3].clone();
        let cells: Vec<&str> = cells_of(text, row.start..row.end).into_iter().map(|(_, c)| &text[c]).collect();
        assert_eq!(cells, ["1", ""]);
        // Tab: from "a" to "b", then to the next row; past the last cell, a new row.
        let mut text = text.to_owned();
        assert_eq!(next_cell(&mut text, 8, false), Some(12..13));
        assert_eq!(next_cell(&mut text, 12, false), Some(28..29));
        let end = text.find("|  |").unwrap() + 2;
        let c = text[..end].chars().count();
        let new = next_cell(&mut text, c, false).unwrap();
        assert!(text.ends_with("|  |\n|  |  |\nfin"), "{text}");
        assert_eq!(new.start, text.find("|  |  |\nfin").unwrap() + 2);
        assert_eq!(next_cell(&mut text, 12, true), Some(8..9));
        // The toolbar's table, then a row more from its headings.
        let mut text = "Note\n".to_owned();
        let sel = table(&mut text, 5, "Col {n}");
        assert_eq!(&text[sel.clone()], "Col 1");
        assert!(text.starts_with("Note\n| Col 1 | Col 2 | Col 3 |\n| --- | --- | --- |\n|  |  |  |\n"), "{text}");
        table(&mut text, sel.start, "Col {n}");
        assert_eq!(text.matches("|  |  |  |").count(), 3);
    }

    #[test]
    fn table_cells_in_place() {
        let text = "x\n| a | b |\n|---|---|\n| 1 | 2 |\n\n| c |\n|---|\n";
        assert_eq!(cell_at(text, text.find('b').unwrap()), Some((0, 0, 1)));
        assert_eq!(cell_at(text, text.find('2').unwrap()), Some((0, 1, 1)));
        assert_eq!(cell_at(text, text.find('c').unwrap()), Some((1, 0, 0)));
        assert_eq!(cell_at(text, 0), None);
        let (range, cells) = table_cells(text, 0).unwrap();
        assert_eq!(&text[range], "| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(cells, [vec!["a", "b"], vec!["---", "---"], vec!["1", "2"]]);
    }

    #[test]
    fn wrapping_and_links() {
        let mut text = "un mot ici".to_owned();
        assert_eq!(wrap(&mut text, 3..7, "**"), 5..8);
        assert_eq!(text, "un **mot** ici");
        assert_eq!(wrap(&mut text, 5..8, "**"), 3..6);
        assert_eq!(text, "un mot ici");
        assert_eq!(wrap(&mut text, 6..6, "*"), 7..7);
        assert_eq!(text, "un mot** ici");
        let mut text = "voir ici".to_owned();
        let sel = link(&mut text, 5..8, "lien");
        assert_eq!(text, "voir [ici](https://)");
        assert_eq!(&text[sel], "https://");
        let mut text = "a\nb".to_owned();
        assert_eq!(insert_block(&mut text, 0, "---"), (2, 6));
        assert_eq!(text, "a\n---\nb");
        let mut text = "a\nb\nc".to_owned();
        assert_eq!(prefix_lines(&mut text, 0..3, Prefix::Check), 0..15);
        assert_eq!(text, "- [ ] a\n- [ ] b\nc");
    }

    #[test]
    fn links_and_images() {
        assert_eq!(md_link("[doc](notes-files/ab-doc.pdf) suite"), Some((1, 3, 25, "notes-files/ab-doc.pdf")));
        assert_eq!(image_src(" ![chat](notes-files/12-chat.png) "), Some("notes-files/12-chat.png"));
        assert_eq!(image_src("![chat](a.png) et texte"), None);
        assert_eq!(md_link("[](x)"), None);
        assert!(resolve("notes-files/../config.json").is_none());
        assert!(resolve("notes-files/a/b").is_none());
        assert_eq!(markdown_for("mon [fichier].pdf", "12-mon--fichier-.pdf"), "[mon fichier.pdf](notes-files/12-mon--fichier-.pdf)");
        assert!(store_name("le chat (2).png").ends_with("-le-chat--2-.png"));
    }

    #[test]
    fn rendering_drops_the_marks() {
        let theme = crate::theme::Theme::default();
        let base = format(note_font(BODY, "note"), theme.text);
        let (job, links) = rich("un **gras** et `code` https://x.y/a).", &base, &theme);
        assert_eq!(job.text, "un gras et code https://x.y/a).");
        assert_eq!(links, vec![(16..29, "https://x.y/a".to_owned())]);
    }

    #[test]
    fn markdown_keeps_every_byte() {
        let text = "# Titre\nun **gras** et *ital* `code` ~~non~~ https://x.y\n- [x] fait\n```\nlet a = 1;\n```\n> cité\n## Sous-titre\nsnake_case_name é";
        let theme = crate::theme::Theme::default();
        let text = &format!("{text}\n| a | b |\n|---|---|\n| 1 | 2 |\n![i](notes-files/x.png)\n- [ ] [doc](notes-files/d.pdf)\n> q\n---\n1. un\n");
        // Every byte kept, in its place: shown as Markdown, rendered, or with a line being edited.
        for (active, raw) in [(None, true), (None, false), (Some(10..12), false)] {
            let job = live(text, active, raw, &theme, 600.0, &|_| None).job;
            let joined: String = job.sections.iter().map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0]).collect();
            assert_eq!(&joined, text);
        }
        let rendered = live(text, Some(10..12), false, &theme, 600.0, &|_| None);
        let kinds: Vec<String> = rendered.decos.iter().map(|d| format!("{d:?}").split([' ', '{']).next().unwrap_or_default().to_owned()).collect();
        assert_eq!(kinds, ["Task", "Code", "Quote", "Table", "Image", "Task", "Quote", "Rule"]);
        assert_eq!(rendered.links.iter().map(|(_, u)| u.as_str()).collect::<Vec<_>>(), ["notes-files/d.pdf"]);
        // The line being edited (the second) shows its marks; the rendered ones hide them.
        let color = |job: &LayoutJob, at: usize| job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).map(|s| s.format.color);
        assert_ne!(color(&rendered.job, text.find("**").unwrap()), Some(Color32::TRANSPARENT));
        assert_eq!(color(&rendered.job, text.find("- [x]").unwrap()), Some(Color32::TRANSPARENT));
    }
    #[test]
    fn folders_in_folders() {
        let folder = |name: &str, parent: Option<Uuid>| Folder { id: Uuid::new_v4(), name: name.into(), parent, updated: 0, deleted: false };
        let work = folder("Work", None);
        let clients = folder("Clients", Some(work.id));
        let acme = folder("Acme", Some(clients.id));
        let home = folder("Home", None);
        let note = |text: &str, folder: Option<Uuid>| Note { id: Uuid::new_v4(), folder, text: text.into(), created: 0, updated: 0, pinned: false, trashed: None };
        let store = Store { folders: vec![work.clone(), clients.clone(), acme.clone(), home.clone()], notes: vec![note("a", Some(acme.id)), note("b", Some(work.id)), note("Loose", None)] };
        let notes = Notes::with(store, None, None);
        assert_eq!(notes.folder_paths().into_iter().map(|(_, p)| p).collect::<Vec<_>>(), ["Home", "Work", "Work › Clients", "Work › Clients › Acme"]);
        assert_eq!(notes.count_in(work.id), 2);
        assert_eq!(notes.descendants(clients.id), [clients.id, acme.id]);
        let t = &crate::i18n::FR;
        let rows: Vec<String> = notes.entries(t).into_iter().map(|(_, n, _)| n).collect();
        assert_eq!(rows, ["Home", "Work", "Loose"]);
    }
}
