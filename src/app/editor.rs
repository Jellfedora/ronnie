//! A small text editor for the file manager: a file of either side, opened in place of the panels, with
//! line numbers, syntax colors, search and replace, go to line, indentation helpers, and saving back
//! (⌘ S) — refused, unless confirmed, when the file changed elsewhere since it was opened.

use std::time::Instant;

use egui::text::{ByteIndex, CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::text_edit::TextEditState;

use super::bigtext::{BigText, Pos};
use super::*;

/// Up to this size, the text is one text field (wrapping, live search count); above, the big-file
/// engine that only lays out the lines on screen.
const SMALL_MAX: usize = 1024 * 1024;

/// What the editor asks the file manager to do.
pub(super) enum EditorAction {
    None,
    /// Write these bytes; `force`: even if the file changed since it was read.
    Save { data: Vec<u8>, mtime: Option<i64>, force: bool },
    /// Read the file again (dropping the edits).
    Reload,
    Close,
}

enum State {
    Loading,
    Ready,
    Failed(String),
}

struct Find {
    query: String,
    replace: String,
    case: bool,
    show_replace: bool,
    focus: bool,
    /// The match the cursor is on (index in `Matches::list`; big files: Some(0) when on one).
    current: Option<usize>,
    /// Big files: the last search found nothing.
    missed: bool,
}

/// Matches of the search, for one revision of the text.
#[derive(Default)]
struct Matches {
    key: Option<(u64, String, bool)>,
    /// Char and byte ranges.
    list: Vec<(usize, usize, usize, usize)>,
}

pub(super) struct Editor {
    pub remote: bool,
    pub path: String,
    pub name: String,
    text: String,
    /// The text as last read or saved.
    saved: String,
    dirty: bool,
    /// Bumped whenever the text changes (search and color caches are keyed on it).
    revision: u64,
    crlf: bool,
    bom: bool,
    indent: String,
    lang: &'static Syntax,
    pub mtime: Option<i64>,
    state: State,
    pub saving: bool,
    /// The file changed elsewhere: overwrite or reload?
    conflict: bool,
    confirm_close: bool,
    close_after_save: bool,
    close_requested: bool,
    find: Option<Find>,
    matches: Matches,
    goto: Option<(String, bool)>,
    pub wrap: bool,
    /// A selection to apply (char range), then scroll to.
    select: Option<(usize, usize)>,
    focus: bool,
    cursor: (usize, usize),
    message: Option<(String, Instant)>,
    error: Option<String>,
    /// Colored layout of the text, reused while it doesn't change.
    colors: Option<(String, u64, LayoutJob)>,
    /// Big files: the text is here instead of `text`, and `saved_revision` tells whether it changed.
    big: Option<BigText>,
    saved_revision: u64,
    saving_revision: u64,
    /// Bytes received while opening, of how many.
    pub progress: Option<(u64, u64)>,
}

const MONO: f32 = 13.0;

impl Editor {
    pub fn opening(remote: bool, path: String, name: String) -> Self {
        Self {
            remote,
            lang: Syntax::detect(&name),
            path,
            name,
            text: String::new(),
            saved: String::new(),
            dirty: false,
            revision: 0,
            crlf: false,
            bom: false,
            indent: "    ".into(),
            mtime: None,
            state: State::Loading,
            saving: false,
            conflict: false,
            confirm_close: false,
            close_after_save: false,
            close_requested: false,
            find: None,
            matches: Matches::default(),
            goto: None,
            wrap: false,
            select: None,
            focus: true,
            cursor: (1, 1),
            message: None,
            error: None,
            colors: None,
            big: None,
            saved_revision: 0,
            saving_revision: 0,
            progress: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The file's content arrived (or couldn't be read).
    pub fn loaded(&mut self, result: Result<(Vec<u8>, Option<i64>), String>, t: &Strings) {
        match result.and_then(|(bytes, mtime)| decode(&bytes, t).map(|d| (d, mtime))) {
            Ok(((text, crlf, bom), mtime)) => {
                self.indent = detect_indent(&text, self.lang);
                if text.len() > SMALL_MAX {
                    let big = BigText::new(&text);
                    self.saved_revision = big.revision;
                    self.big = Some(big);
                    self.text.clear();
                    self.saved.clear();
                    self.wrap = false;
                } else {
                    self.big = None;
                    self.saved = text.clone();
                    self.text = text;
                }
                (self.crlf, self.bom, self.mtime) = (crlf, bom, mtime);
                self.dirty = false;
                self.revision += 1;
                self.state = State::Ready;
                self.conflict = false;
                self.error = None;
                self.focus = true;
            }
            // A reload that failed keeps the text being edited.
            Err(e) if matches!(self.state, State::Ready) => self.error = Some(e),
            Err(e) => self.state = State::Failed(e),
        }
    }

    /// The save finished: the file's new modification time, or why it failed.
    pub fn saved(&mut self, result: Result<Option<i64>, String>, conflict: bool, t: &Strings) {
        self.saving = false;
        match result {
            Ok(mtime) => {
                self.mtime = mtime;
                match &self.big {
                    Some(big) => {
                        self.saved_revision = self.saving_revision;
                        self.dirty = big.revision != self.saved_revision;
                    }
                    None => {
                        self.saved = self.text.clone();
                        self.dirty = false;
                    }
                }
                self.conflict = false;
                self.error = None;
                self.message = Some((t.editor_saved.to_owned(), Instant::now()));
                if self.close_after_save {
                    self.close_requested = true;
                    self.confirm_close = false;
                }
            }
            Err(_) if conflict => {
                self.conflict = true;
                self.close_after_save = false;
            }
            Err(e) => {
                self.error = Some(e);
                self.close_after_save = false;
            }
        }
    }

    pub fn save_failed(&mut self, error: String) {
        self.saving = false;
        self.close_after_save = false;
        self.error = Some(error);
    }

    /// ⌘ W from the app's shortcuts (asks first when there are unsaved changes).
    pub fn request_close(&mut self) {
        if self.dirty {
            self.confirm_close = true;
        } else {
            self.close_requested = true;
        }
    }

    fn text_id(&self) -> egui::Id {
        egui::Id::new(("file-editor", &self.path))
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.text.len() + 3);
        if self.bom {
            out.extend_from_slice(b"\xEF\xBB\xBF");
        }
        if let Some(big) = &self.big {
            out.extend_from_slice(&big.to_bytes(if self.crlf { "\r\n" } else { "\n" }));
        } else if self.crlf {
            out.extend_from_slice(self.text.replace('\n', "\r\n").as_bytes());
        } else {
            out.extend_from_slice(self.text.as_bytes());
        }
        out
    }

    fn save(&mut self, force: bool) -> EditorAction {
        if self.saving || !matches!(self.state, State::Ready) {
            return EditorAction::None;
        }
        self.saving = true;
        self.error = None;
        self.saving_revision = self.big.as_ref().map_or(0, |b| b.revision);
        EditorAction::Save { data: self.encode(), mtime: self.mtime, force }
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.dirty = match &self.big {
            Some(big) => big.revision != self.saved_revision,
            None => self.text != self.saved,
        };
    }

    /// The whole editor, filling `rect`.
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) -> EditorAction {
        let mut action = EditorAction::None;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let modal_open = ui.ctx().memory(|m| m.top_modal_layer().is_some());

        // Keys (before the text field sees them).
        let (save, next, prev, goto, escape, find) = ui.input_mut(|i| {
            (
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::G)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::G)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::L)),
                !modal_open && i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::F)),
            )
        });
        if save && !modal_open {
            action = self.save(false);
        }
        if find {
            self.open_find(ui.ctx());
        }
        if goto {
            self.goto = Some((String::new(), true));
        }
        // Escape closes the go-to bar, then the search bar.
        if escape && (self.goto.take().is_some() || self.find.take().is_some()) {
            self.focus = true;
        }

        // Header: name, path, actions.
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 36.0));
        ui.painter().rect_filled(head, 0.0, theme.chrome_bg);
        ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, theme.tab_hover));
        ui.scope_builder(egui::UiBuilder::new().max_rect(head.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new("✎").size(15.0).color(theme.accent));
            ui.label(egui::RichText::new(&self.name).size(14.0).strong());
            if self.dirty {
                ui.label(egui::RichText::new("●").size(11.0).color(theme.accent)).on_hover_text(t.editor_unsaved);
            }
            ui.add(egui::Label::new(egui::RichText::new(&self.path).size(12.0).color(theme.text_muted)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new(format!("✕  {}", t.close)).size(13.0)).clicked() {
                    self.request_close();
                }
                let label = if self.saving { t.editor_saving.to_owned() } else { format!("{}   {}", t.save, shortcut_hint('S')) };
                let fill = if self.dirty { theme.accent } else { theme.tab_hover };
                let color = if self.dirty { theme.bg } else { theme.text };
                let button = egui::Button::new(egui::RichText::new(label).size(13.0).color(color)).fill(fill).corner_radius(5.0);
                if ui.add_enabled(matches!(self.state, State::Ready) && !self.saving, button).clicked() {
                    action = self.save(false);
                }
                ui.add_space(6.0);
                if ui.button("🔍").on_hover_text(format!("{}   {}", t.editor_find, shortcut_hint('F'))).clicked() {
                    self.open_find(ui.ctx());
                }
                if ui.button("↧").on_hover_text(format!("{}   {}", t.editor_goto, shortcut_hint('L'))).clicked() {
                    self.goto = Some((String::new(), true));
                }
                if self.big.is_none() {
                    ui.checkbox(&mut self.wrap, egui::RichText::new(t.editor_wrap).size(12.5));
                }
            });
        });
        let mut top = head.max.y + 1.0;

        // Banners: conflict, error.
        if self.conflict {
            let r = Rect::from_min_size(Pos2::new(rect.min.x, top), Vec2::new(rect.width(), 34.0));
            ui.painter().rect_filled(r, 0.0, theme.ansi[3].gamma_multiply(0.18));
            ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                ui.label(egui::RichText::new(format!("⚠  {}", t.editor_conflict)).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t.cancel).clicked() {
                        self.conflict = false;
                    }
                    if ui.button(t.editor_reload).on_hover_text(t.editor_reload_hint).clicked() {
                        self.conflict = false;
                        action = EditorAction::Reload;
                    }
                    if ui.button(t.editor_overwrite).clicked() {
                        self.conflict = false;
                        action = self.save(true);
                    }
                });
            });
            top = r.max.y;
        }
        if let Some(e) = self.error.clone() {
            let r = Rect::from_min_size(Pos2::new(rect.min.x, top), Vec2::new(rect.width(), 28.0));
            ui.painter().rect_filled(r, 0.0, theme.ansi[1].gamma_multiply(0.15));
            ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(format!("⚠  {e}")).size(12.5).color(theme.ansi[1])).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() {
                        self.error = None;
                    }
                });
            });
            top = r.max.y;
        }

        // Search and go-to bars.
        if self.find.is_some() {
            top = self.find_bar(ui, Rect::from_min_max(Pos2::new(rect.min.x, top), rect.max), theme, t, next, prev);
        }
        if self.goto.is_some() {
            top = self.goto_bar(ui, Rect::from_min_max(Pos2::new(rect.min.x, top), rect.max), theme, t);
        }

        let status_h = 24.0;
        let body = Rect::from_min_max(Pos2::new(rect.min.x, top), Pos2::new(rect.max.x, rect.max.y - status_h));
        match &self.state {
            State::Loading => {
                let text = match self.progress {
                    Some((done, total)) if total > 0 => format!("{}  {} %", t.files_loading, done * 100 / total),
                    _ => t.files_loading.to_owned(),
                };
                ui.painter().text(body.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme.text_muted);
            }
            State::Failed(e) => {
                let e = e.clone();
                ui.scope_builder(egui::UiBuilder::new().max_rect(body.shrink(40.0)).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                    ui.add_space(body.height() * 0.3);
                    ui.label(egui::RichText::new(format!("⚠  {e}")).size(13.5).color(theme.ansi[1]));
                    ui.add_space(10.0);
                    if ui.button(t.close).clicked() {
                        self.close_requested = true;
                    }
                });
            }
            State::Ready => self.text_ui(ui, body, theme),
        }

        // Status bar.
        let bar = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - status_h), rect.max);
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.min.y, Stroke::new(1.0, theme.tab_hover));
        if matches!(self.state, State::Ready) {
            let indent = if self.indent == "\t" { t.editor_tabs.to_owned() } else { t.editor_spaces.replace("{n}", &self.indent.len().to_string()) };
            let right = format!(
                "{}    {}    {}    {}    UTF-8{}",
                t.editor_position.replace("{l}", &self.cursor.0.to_string()).replace("{c}", &self.cursor.1.to_string()),
                if self.lang.name.is_empty() { t.editor_plain } else { self.lang.name },
                indent,
                if self.crlf { "CRLF" } else { "LF" },
                if self.bom { " BOM" } else { "" },
            );
            ui.painter().text(bar.right_center() - Vec2::new(12.0, 0.0), Align2::RIGHT_CENTER, right, FontId::proportional(11.5), theme.text_muted);
        }
        if let Some((msg, at)) = &self.message {
            let age = at.elapsed().as_secs_f32();
            if age < 2.5 {
                ui.painter().text(bar.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, format!("✔  {msg}"), FontId::proportional(11.5), theme.ansi[2]);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            } else {
                self.message = None;
            }
        }

        self.close_dialog(ui.ctx(), theme, t, &mut action);
        if self.close_requested {
            self.close_requested = false;
            action = EditorAction::Close;
        }
        action
    }

    /// ⌘ F: the search bar, with the selection (if any) as the query.
    pub fn open_find(&mut self, ctx: &egui::Context) {
        let selected = match &self.big {
            Some(big) => big.has_selection().then(|| big.selected_text()),
            None => TextEditState::load(ctx, self.text_id()).and_then(|s| s.cursor.char_range()).and_then(|r| {
                let range = r.as_sorted_char_range();
                (range.start.0 != range.end.0).then(|| self.text.chars().skip(range.start.0).take(range.end.0 - range.start.0).collect::<String>())
            }),
        };
        let selected = selected.filter(|s| !s.contains('\n'));
        match &mut self.find {
            Some(find) => {
                if let Some(s) = selected {
                    find.query = s;
                }
                find.focus = true;
            }
            None => self.find = Some(Find { query: selected.unwrap_or_default(), replace: String::new(), case: false, show_replace: false, focus: true, current: None, missed: false }),
        }
    }

    /// Recomputes the matches if the text or the query changed.
    fn update_matches(&mut self) {
        let Some(find) = &self.find else {
            self.matches = Matches::default();
            return;
        };
        let key = (self.revision, find.query.clone(), find.case);
        if self.matches.key.as_ref() == Some(&key) || self.big.is_some() {
            return;
        }
        self.matches.list = find_all(&self.text, &find.query, find.case);
        self.matches.key = Some(key);
    }

    /// Goes to the next (or previous) match from the cursor.
    fn jump(&mut self, ctx: &egui::Context, forward: bool, focus_text: bool) {
        if let (Some(big), Some(f)) = (&mut self.big, &mut self.find) {
            match big.find(&f.query, f.case, forward) {
                Some((a, b)) => {
                    big.select(a, b);
                    (f.current, f.missed) = (Some(0), false);
                }
                None => (f.current, f.missed) = (None, !f.query.is_empty()),
            }
            if focus_text {
                self.focus = true;
            }
            return;
        }
        self.update_matches();
        if self.matches.list.is_empty() {
            if let Some(f) = &mut self.find {
                f.current = None;
            }
            return;
        }
        let range = TextEditState::load(ctx, self.text_id()).and_then(|s| s.cursor.char_range()).map(|r| r.as_sorted_char_range());
        let (start, end) = range.map_or((0, 0), |r| (r.start.0, r.end.0));
        let list = &self.matches.list;
        let i = if forward {
            list.iter().position(|m| m.0 >= end && !(m.0 == start && m.1 == end)).or_else(|| list.iter().position(|m| m.0 > start)).unwrap_or(0)
        } else {
            list.iter().rposition(|m| m.1 <= start && !(m.0 == start && m.1 == end)).unwrap_or(list.len() - 1)
        };
        let (a, b, _, _) = list[i];
        if let Some(f) = &mut self.find {
            f.current = Some(i);
        }
        self.select = Some((a, b));
        if focus_text {
            self.focus = true;
        }
    }

    fn find_bar(&mut self, ui: &mut Ui, area: Rect, theme: &Theme, t: &Strings, next: bool, prev: bool) -> f32 {
        let rows = if self.find.as_ref().is_some_and(|f| f.show_replace) { 2.0 } else { 1.0 };
        let r = Rect::from_min_size(area.min, Vec2::new(area.width(), 8.0 + rows * 30.0));
        ui.painter().rect_filled(r, 0.0, theme.chrome_bg);
        ui.painter().hline(r.x_range(), r.max.y, Stroke::new(1.0, theme.tab_hover));
        let ctx = ui.ctx().clone();
        let (mut go_next, mut go_prev, mut replace_one, mut replace_all, mut close) = (next, prev, false, false, false);
        let before = self.find.as_ref().map(|f| (f.query.clone(), f.case));
        self.update_matches();
        let big = self.big.is_some();
        let count = if big { usize::from(!self.find.as_ref().is_some_and(|f| f.missed)) } else { self.matches.list.len() };
        let current = self.find.as_ref().and_then(|f| f.current).filter(|&c| big || c < count);
        let find = self.find.as_mut().expect("find bar");
        ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            ui.horizontal(|ui| {
                let arrow = if find.show_replace { "▾" } else { "▸" };
                if ui.small_button(arrow).on_hover_text(t.editor_replace).clicked() {
                    find.show_replace = !find.show_replace;
                }
                let edit = ui.add(egui::TextEdit::singleline(&mut find.query).hint_text(t.editor_find).desired_width(260.0).font(FontId::monospace(12.5)));
                if find.focus {
                    edit.request_focus();
                    find.focus = false;
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    if ui.input(|i| i.modifiers.shift) { go_prev = true } else { go_next = true }
                    edit.request_focus();
                }
                let info = match (count, current) {
                    _ if big && find.query.is_empty() => String::new(),
                    (1, _) if big => String::new(),
                    (0, _) if !find.query.is_empty() => t.editor_no_match.to_owned(),
                    (0, _) => String::new(),
                    (n, Some(c)) => format!("{} / {n}", c + 1),
                    (n, None) => n.to_string(),
                };
                ui.add_sized(Vec2::new(90.0, 20.0), egui::Label::new(egui::RichText::new(info).size(12.0).color(if count == 0 && !find.query.is_empty() { theme.ansi[1] } else { theme.text_muted })));
                if ui.small_button("↑").on_hover_text(format!("⇧ {}", shortcut_hint('G'))).clicked() {
                    go_prev = true;
                }
                if ui.small_button("↓").on_hover_text(shortcut_hint('G')).clicked() {
                    go_next = true;
                }
                ui.toggle_value(&mut find.case, egui::RichText::new("Aa").size(12.0)).on_hover_text(t.editor_case);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() {
                        close = true;
                    }
                });
            });
            if find.show_replace {
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.add(egui::TextEdit::singleline(&mut find.replace).hint_text(t.editor_replace).desired_width(260.0).font(FontId::monospace(12.5)));
                    if ui.add_enabled(current.is_some(), egui::Button::new(egui::RichText::new(t.editor_replace).size(12.5))).clicked() {
                        replace_one = true;
                    }
                    if ui.add_enabled(count > 0, egui::Button::new(egui::RichText::new(t.editor_replace_all).size(12.5))).clicked() {
                        replace_all = true;
                    }
                });
            }
        });
        let changed = before != self.find.as_ref().map(|f| (f.query.clone(), f.case));
        if close {
            self.find = None;
            self.focus = true;
            return r.max.y;
        }
        if self.big.is_some() {
            let with = self.find.as_ref().map(|f| f.replace.clone()).unwrap_or_default();
            if changed {
                // From the selection's start, so that the match being typed stays selected.
                if let Some(text) = self.big.as_mut() {
                    let start = text.anchor.min(text.cursor);
                    text.set_cursor(start, false);
                }
                self.jump(&ctx, true, false);
            }
            if replace_one {
                if let Some(text) = self.big.as_mut().filter(|t| t.has_selection()) {
                    text.insert(&with, false);
                    self.changed();
                }
                self.jump(&ctx, true, false);
            }
            if replace_all {
                if let (Some(text), Some(f)) = (self.big.as_mut(), &self.find) {
                    let n = text.replace_all(&f.query, &with, f.case);
                    self.changed();
                    self.message = Some((t.editor_replaced.replace("{n}", &n.to_string()), Instant::now()));
                }
            }
            if go_next || go_prev {
                self.jump(&ctx, go_next, false);
            }
            return r.max.y;
        }
        // Typing in the search field shows the first match from the cursor.
        if changed {
            self.update_matches();
            if let Some(f) = &mut self.find {
                f.current = None;
            }
            if !self.matches.list.is_empty() {
                let from = TextEditState::load(&ctx, self.text_id()).and_then(|s| s.cursor.char_range()).map_or(0, |r| r.as_sorted_char_range().start.0);
                let i = self.matches.list.iter().position(|m| m.0 >= from).unwrap_or(0);
                let (a, b, _, _) = self.matches.list[i];
                self.find.as_mut().unwrap().current = Some(i);
                self.select = Some((a, b));
            }
        }
        if replace_one {
            if let (Some(i), Some(f)) = (current, &self.find) {
                let (a, _, ba, bb) = self.matches.list[i];
                let with = f.replace.clone();
                self.text.replace_range(ba..bb, &with);
                self.changed();
                self.select = Some((a + with.chars().count(), a + with.chars().count()));
                self.apply_selection(&ctx);
                self.jump(&ctx, true, false);
            }
        }
        if replace_all {
            if let Some(f) = &self.find {
                let with = f.replace.clone();
                let n = self.matches.list.len();
                for &(_, _, ba, bb) in self.matches.list.iter().rev() {
                    self.text.replace_range(ba..bb, &with);
                }
                self.changed();
                self.select = Some((0, 0));
                self.message = Some((t.editor_replaced.replace("{n}", &n.to_string()), Instant::now()));
            }
        }
        if go_next || go_prev {
            self.jump(&ctx, go_next, false);
        }
        r.max.y
    }

    fn goto_bar(&mut self, ui: &mut Ui, area: Rect, theme: &Theme, t: &Strings) -> f32 {
        let r = Rect::from_min_size(area.min, Vec2::new(area.width(), 36.0));
        ui.painter().rect_filled(r, 0.0, theme.chrome_bg);
        ui.painter().hline(r.x_range(), r.max.y, Stroke::new(1.0, theme.tab_hover));
        let lines = self.big.as_ref().map_or_else(|| self.text.split('\n').count(), |b| b.lines.len());
        let mut target = None;
        let mut close = false;
        let Some((text, focus)) = &mut self.goto else { return area.min.y };
        ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new(t.editor_goto).size(12.5));
            let edit = ui.add(egui::TextEdit::singleline(text).hint_text(format!("1 – {lines}")).desired_width(90.0).font(FontId::monospace(12.5)));
            if *focus {
                edit.request_focus();
                *focus = false;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                target = text.trim().parse::<usize>().ok();
                close = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    close = true;
                }
            });
        });
        if let (Some(line), Some(big)) = (target, self.big.as_mut()) {
            big.set_cursor(Pos { line: line.clamp(1, lines) - 1, col: 0 }, false);
        } else if let Some(line) = target {
            let line = line.clamp(1, lines);
            let start: usize = self.text.split('\n').take(line - 1).map(|l| l.chars().count() + 1).sum();
            self.select = Some((start, start));
        }
        if close {
            self.goto = None;
            self.focus = true;
        }
        r.max.y
    }

    /// Applies a pending selection to the text field's state.
    fn apply_selection(&mut self, ctx: &egui::Context) {
        let Some((a, b)) = self.select else { return };
        let id = self.text_id();
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
        state.store(ctx, id);
    }

    /// Tab / Shift+Tab (indent the selected lines), Enter (keep the indentation), ⌘ / (comment).
    fn edit_keys(&mut self, ui: &Ui) {
        let id = self.text_id();
        if !ui.memory(|m| m.has_focus(id)) {
            return;
        }
        let (tab, untab, enter, comment) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Slash)),
            )
        });
        if !(tab || untab || enter || comment) {
            return;
        }
        let Some(mut state) = TextEditState::load(ui.ctx(), id) else { return };
        let Some(range) = state.cursor.char_range() else { return };
        let r = range.as_sorted_char_range();
        let (a, b) = (r.start.0, r.end.0);
        let byte = |text: &str, c: usize| text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
        let (ba, bb) = (byte(&self.text, a), byte(&self.text, b));
        let line_start = self.text[..ba].rfind('\n').map_or(0, |i| i + 1);
        let (new_a, new_b);
        if enter {
            let line = &self.text[line_start..ba];
            let mut indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            // After an opening bracket or a colon (Python, YAML), one level more.
            if line.trim_end().ends_with(['{', '[', '(', ':']) {
                indent.push_str(&self.indent);
            }
            let insert = format!("\n{indent}");
            self.text.replace_range(ba..bb, &insert);
            new_a = a + insert.chars().count();
            new_b = new_a;
        } else if tab && a == b {
            // Up to the next indentation stop.
            let col = self.text[line_start..ba].chars().count();
            let insert = if self.indent == "\t" { "\t".to_owned() } else { " ".repeat(self.indent.len() - col % self.indent.len()) };
            self.text.insert_str(ba, &insert);
            new_a = a + insert.chars().count();
            new_b = new_a;
        } else {
            // Whole lines of the selection (not the one the selection ends at the start of).
            let end = if bb > ba && self.text[..bb].ends_with('\n') { bb - 1 } else { bb };
            let block_end = self.text[end..].find('\n').map_or(self.text.len(), |i| end + i);
            let block = self.text[line_start..block_end].to_owned();
            let lines: Vec<&str> = block.split('\n').collect();
            let prefix = self.lang.line_comment.map(|c| format!("{c} "));
            let new: Vec<String> = if comment {
                let Some(prefix) = prefix else { return };
                let mark = prefix.trim_end();
                let all = lines.iter().filter(|l| !l.trim().is_empty()).all(|l| l.trim_start().starts_with(mark));
                let min_indent = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
                lines
                    .iter()
                    .map(|l| {
                        if l.trim().is_empty() {
                            l.to_string()
                        } else if all {
                            let lead = l.len() - l.trim_start().len();
                            let rest = &l[lead..];
                            let rest = rest.strip_prefix(prefix.as_str()).or_else(|| rest.strip_prefix(mark)).unwrap_or(rest);
                            format!("{}{rest}", &l[..lead])
                        } else {
                            format!("{}{prefix}{}", &l[..min_indent], &l[min_indent..])
                        }
                    })
                    .collect()
            } else if untab {
                lines
                    .iter()
                    .map(|l| {
                        let n = if l.starts_with('\t') { 1 } else { l.chars().take(self.indent.len().max(1)).take_while(|c| *c == ' ').count() };
                        l[n..].to_string()
                    })
                    .collect()
            } else {
                lines.iter().map(|l| if l.is_empty() { String::new() } else { format!("{}{l}", self.indent) }).collect()
            };
            let joined = new.join("\n");
            let start_char = self.text[..line_start].chars().count();
            self.text.replace_range(line_start..block_end, &joined);
            new_a = start_char;
            new_b = start_char + joined.chars().count();
        }
        self.changed();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(new_a), CCursor::new(new_b))));
        state.store(ui.ctx(), id);
    }

    /// The text with its line numbers.
    fn text_ui(&mut self, ui: &mut Ui, body: Rect, theme: &Theme) {
        let ctx = ui.ctx().clone();
        let id = self.text_id();
        if let Some(big) = &mut self.big {
            let focus = self.focus && !ctx.memory(|m| m.top_modal_layer().is_some());
            self.focus &= !focus;
            let find = self.find.as_ref().map(|f| (f.query.as_str(), f.case));
            let changed = big.ui(ui, body, id, theme, self.lang, &self.indent, find, focus);
            self.cursor = (big.cursor.line + 1, big.lines[big.cursor.line][..big.cursor.col].chars().count() + 1);
            if changed {
                self.changed();
            }
            return;
        }
        let scroll_to = self.select.is_some();
        self.apply_selection(&ctx);
        let target = self.select.take();
        let id = self.text_id();
        if self.focus && !ctx.memory(|m| m.top_modal_layer().is_some()) {
            ctx.memory_mut(|m| m.request_focus(id));
            self.focus = false;
        }
        self.edit_keys(ui);

        let font = FontId::monospace(MONO);
        let row_h = ui.fonts_mut(|f| f.row_height(&font));
        let lines = self.text.matches('\n').count() + 1;
        let digits = lines.to_string().len().max(3) as f32;
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let gutter = digits * char_w + 22.0;
        let wrap = self.wrap;

        let find_matches = self.find.as_ref().filter(|f| !f.query.is_empty()).map(|f| (f.current, f.query.clone()));
        if find_matches.is_some() {
            self.update_matches();
        }
        // Colors, rebuilt only when the text changes.
        let key = color_key(theme);
        let lang = self.lang;
        let colors = &mut self.colors;
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let text = buf.as_str();
            let fresh = colors.as_ref().is_some_and(|(t, k, _)| *k == key && t == text);
            if !fresh {
                *colors = Some((text.to_owned(), key, highlight(text, lang, theme, &FontId::monospace(MONO))));
            }
            let mut job = colors.as_ref().unwrap().2.clone();
            job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
            ui.painter().layout_job(job)
        };

        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
        let scroll = if wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        let before = self.text.len();
        let mut changed = false;
        let mut cursor = None;
        let matches = &self.matches.list;
        let text = &mut self.text;
        scroll.id_salt(("editor-scroll", &self.path)).auto_shrink(false).show(&mut area, |ui| {
            let visible = ui.clip_rect();
            ui.horizontal_top(|ui| {
                ui.add_space(gutter);
                let rows = ((body.height() - 8.0) / row_h).floor().max(1.0) as usize;
                let width = if wrap { ui.available_width() } else { ui.available_width().max(200.0) };
                let out = egui::TextEdit::multiline(text)
                    .id(id)
                    .font(font.clone())
                    .code_editor()
                    .frame(Frame::NONE)
                    .margin(Vec2::new(4.0, 6.0))
                    .desired_width(width)
                    .desired_rows(rows)
                    .layouter(&mut layouter)
                    .show(ui);
                changed = out.response.changed();
                cursor = out.cursor_range.map(|r| r.primary.index.0);
                let origin = out.galley_pos;

                // Search matches, behind nothing but readable: a light wash, stronger on the current one.
                if let Some((current, _)) = &find_matches {
                    let mut painted = 0;
                    for (i, &(a, b, _, _)) in matches.iter().enumerate() {
                        let ra = out.galley.pos_from_cursor(CCursor::new(a)).translate(origin.to_vec2());
                        if ra.max.y < visible.min.y {
                            continue;
                        }
                        if ra.min.y > visible.max.y || painted > 2000 {
                            break;
                        }
                        let rb = out.galley.pos_from_cursor(CCursor::new(b)).translate(origin.to_vec2());
                        let r = Rect::from_min_max(ra.min, Pos2::new(if rb.min.y == ra.min.y { rb.max.x } else { ra.max.x + char_w }, ra.max.y));
                        let strong = *current == Some(i);
                        ui.painter().rect_filled(r, 2.0, theme.accent.gamma_multiply(if strong { 0.45 } else { 0.2 }));
                        if strong {
                            ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Outside);
                        }
                        painted += 1;
                    }
                }

                // Line numbers (a wrapped line has its number on its first row only).
                let gutter_rect = Rect::from_min_max(Pos2::new(visible.min.x, visible.min.y), Pos2::new(visible.min.x + gutter - 6.0, visible.max.y));
                ui.painter().rect_filled(gutter_rect, 0.0, theme.chrome_bg.gamma_multiply(0.6));
                let current_line = cursor.map(|c| out.galley.pos_from_cursor(CCursor::new(c)).min.y);
                let mut number = 1;
                let mut starts_line = true;
                for row in &out.galley.rows {
                    let y = origin.y + row.pos.y;
                    if starts_line && y + row_h >= visible.min.y && y <= visible.max.y {
                        let here = current_line.is_some_and(|cy| (cy - row.pos.y).abs() < 1.0);
                        let color = if here { theme.text } else { theme.text_muted.gamma_multiply(0.7) };
                        ui.painter().text(Pos2::new(gutter_rect.max.x - 8.0, y + row_h / 2.0), Align2::RIGHT_CENTER, number.to_string(), font.clone(), color);
                    }
                    if row.ends_with_newline {
                        number += 1;
                        starts_line = true;
                    } else {
                        starts_line = false;
                    }
                    if y > visible.max.y {
                        break;
                    }
                }

                if let (true, Some((a, _))) = (scroll_to, target) {
                    let r = out.galley.pos_from_cursor(CCursor::new(a)).translate(origin.to_vec2());
                    ui.scroll_to_rect(r.expand2(Vec2::new(40.0, row_h * 3.0)), Some(egui::Align::Center));
                }
            });
        });
        if changed || self.text.len() != before {
            self.changed();
        }
        if let Some(c) = cursor {
            let before: String = self.text.chars().take(c).collect();
            let line = before.matches('\n').count() + 1;
            let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            self.cursor = (line, col);
        }
    }

    /// "Save the changes?" before closing.
    fn close_dialog(&mut self, ctx: &egui::Context, theme: &Theme, t: &Strings, action: &mut EditorAction) {
        if !self.confirm_close {
            return;
        }
        let mut choice = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("editor-close")).frame(frame).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(egui::RichText::new(t.editor_unsaved_title.replace("{name}", &self.name)).size(16.0).strong());
            ui.add_space(6.0);
            ui.label(egui::RichText::new(t.editor_unsaved_body).size(12.5).color(theme.text_muted));
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = |text: &str, fill: Option<Color32>| {
                    let label = egui::RichText::new(text).size(13.5);
                    let label = if fill.is_some() { label.color(theme.bg) } else { label };
                    let b = egui::Button::new(label).corner_radius(6.0).min_size(Vec2::new(96.0, 30.0));
                    if let Some(fill) = fill { b.fill(fill) } else { b }
                };
                if ui.add(b(t.save, Some(theme.accent))).clicked() {
                    choice = Some(0);
                }
                if ui.add(b(t.editor_discard, None)).clicked() {
                    choice = Some(1);
                }
                if ui.add(b(t.cancel, None)).clicked() {
                    choice = Some(2);
                }
            });
        });
        if modal.should_close() {
            choice.get_or_insert(2);
        }
        match choice {
            Some(0) => {
                self.confirm_close = false;
                self.close_after_save = true;
                *action = self.save(false);
            }
            Some(1) => {
                self.confirm_close = false;
                self.close_requested = true;
            }
            Some(_) => {
                self.confirm_close = false;
                self.focus = true;
            }
            None => {}
        }
    }
}

/// "⌘ S" on macOS, "Ctrl+S" elsewhere.
fn shortcut_hint(key: char) -> String {
    if cfg!(target_os = "macos") { format!("⌘ {key}") } else { format!("Ctrl+{key}") }
}

/// Text from the file's bytes: UTF-8 (a BOM is kept aside), line endings made "\n". Returns whether they
/// were "\r\n", and whether there was a BOM.
fn decode(bytes: &[u8], t: &Strings) -> Result<(String, bool, bool), String> {
    let (bom, bytes) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let text = std::str::from_utf8(bytes).map_err(|_| t.editor_not_text.to_owned())?;
    if text.contains('\0') {
        return Err(t.editor_not_text.to_owned());
    }
    let crlf = text.contains("\r\n");
    Ok((if crlf { text.replace("\r\n", "\n") } else { text.to_owned() }, crlf, bom))
}

/// The file's indentation: tabs, or 2 or 4 spaces (the language's habit when nothing is indented).
fn detect_indent(text: &str, lang: &Syntax) -> String {
    let (mut tabs, mut two, mut four) = (0, 0, 0);
    for line in text.lines().take(2000) {
        if line.starts_with('\t') {
            tabs += 1;
        } else {
            let n = line.len() - line.trim_start_matches(' ').len();
            if n > 0 && !line.trim().is_empty() {
                if n % 4 == 0 { four += 1 } else if n % 2 == 0 { two += 1 }
            }
        }
    }
    if tabs > two + four {
        "\t".into()
    } else if two > 0 || (four == 0 && lang.two_spaces) {
        "  ".into()
    } else {
        "    ".into()
    }
}

/// All the places `query` is in `text`: (char start, char end, byte start, byte end).
fn find_all(text: &str, query: &str, case: bool) -> Vec<(usize, usize, usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    // One char for one char, so that positions stay right (the rare letters whose lower case is longer
    // keep their first char).
    let fold = |c: char| if case { c } else { c.to_lowercase().next().unwrap_or(c) };
    let q: Vec<char> = query.chars().map(fold).collect();
    let chars: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold(c))).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + q.len() <= chars.len() {
        if chars[i..i + q.len()].iter().zip(&q).all(|((_, c), q)| c == q) {
            let end_byte = chars.get(i + q.len()).map_or(text.len(), |(b, _)| *b);
            out.push((i, i + q.len(), chars[i].0, end_byte));
            i += q.len();
            if out.len() >= 100_000 {
                break;
            }
        } else {
            i += 1;
        }
    }
    out
}

// Syntax colors.

/// What the colors know of a language.
pub(super) struct Syntax {
    pub name: &'static str,
    pub line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
    /// Keys before ":" or "=" at the start of lines, [sections] (config files).
    keys: bool,
    /// <tags> (HTML, XML).
    markup: bool,
    /// Indented with 2 spaces by habit.
    two_spaces: bool,
}

const C_LIKE: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "function", "fn", "let", "const", "var", "mut", "static",
    "struct", "enum", "impl", "trait", "class", "interface", "extends", "implements", "new", "delete", "this", "self", "Self", "super", "public", "private",
    "protected", "import", "export", "from", "use", "mod", "pub", "package", "namespace", "try", "catch", "finally", "throw", "throws", "async", "await",
    "match", "where", "type", "typeof", "instanceof", "in", "of", "as", "true", "false", "null", "nil", "None", "undefined", "void", "int", "char", "float",
    "double", "long", "bool", "boolean", "string", "unsigned", "signed", "go", "func", "defer", "chan", "select", "yield", "loop", "echo", "require",
    "include", "foreach", "elseif", "endif", "unsafe", "dyn", "ref", "move", "crate", "extern", "sizeof", "typedef", "struct", "virtual", "override",
];
const PYTHON: &[&str] = &[
    "def", "class", "if", "elif", "else", "for", "while", "in", "not", "and", "or", "is", "return", "import", "from", "as", "with", "try", "except",
    "finally", "raise", "pass", "break", "continue", "lambda", "yield", "global", "nonlocal", "True", "False", "None", "async", "await", "self", "assert", "del",
];
const SHELL: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "return", "local", "export", "readonly",
    "echo", "exit", "source", "alias", "unset", "set", "shift", "true", "false", "sudo", "cd",
];
const SQL: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop", "alter", "add", "index", "join", "left",
    "right", "inner", "outer", "on", "and", "or", "not", "null", "is", "as", "order", "by", "group", "having", "limit", "offset", "primary", "key", "foreign",
    "references", "default", "unique", "distinct", "union", "all", "exists", "in", "like", "between", "case", "when", "then", "else", "end", "SELECT",
    "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "ADD", "INDEX", "JOIN", "LEFT", "RIGHT",
    "INNER", "OUTER", "ON", "AND", "OR", "NOT", "NULL", "IS", "AS", "ORDER", "BY", "GROUP", "HAVING", "LIMIT", "OFFSET", "PRIMARY", "KEY", "FOREIGN",
    "REFERENCES", "DEFAULT", "UNIQUE", "DISTINCT", "UNION", "ALL", "EXISTS", "IN", "LIKE", "BETWEEN", "CASE", "WHEN", "THEN", "ELSE", "END",
];
const NGINX: &[&str] = &["server", "location", "listen", "server_name", "root", "index", "return", "proxy_pass", "include", "upstream", "http", "events", "if", "rewrite", "try_files"];
const DOCKER: &[&str] = &["FROM", "RUN", "CMD", "COPY", "ADD", "ENV", "ARG", "EXPOSE", "WORKDIR", "ENTRYPOINT", "VOLUME", "USER", "LABEL", "AS", "HEALTHCHECK", "SHELL"];

const fn lang(name: &'static str, line_comment: Option<&'static str>, block_comment: Option<(&'static str, &'static str)>, keywords: &'static [&'static str]) -> Syntax {
    Syntax { name, line_comment, block_comment, keywords, keys: false, markup: false, two_spaces: false }
}

static PLAIN: Syntax = lang("", None, None, &[]);
static RUST: Syntax = lang("Rust", Some("//"), Some(("/*", "*/")), C_LIKE);
static JS: Syntax = Syntax { two_spaces: true, ..lang("JavaScript", Some("//"), Some(("/*", "*/")), C_LIKE) };
static TS: Syntax = Syntax { two_spaces: true, ..lang("TypeScript", Some("//"), Some(("/*", "*/")), C_LIKE) };
static C: Syntax = lang("C / C++", Some("//"), Some(("/*", "*/")), C_LIKE);
static JAVA: Syntax = lang("Java", Some("//"), Some(("/*", "*/")), C_LIKE);
static GO: Syntax = lang("Go", Some("//"), Some(("/*", "*/")), C_LIKE);
static PHP: Syntax = lang("PHP", Some("//"), Some(("/*", "*/")), C_LIKE);
static CSS: Syntax = Syntax { two_spaces: true, ..lang("CSS", None, Some(("/*", "*/")), &["important", "media", "import"]) };
static SCSS: Syntax = Syntax { two_spaces: true, ..lang("SCSS", Some("//"), Some(("/*", "*/")), &["import", "mixin", "include", "extend", "media", "if", "else", "each"]) };
static PY: Syntax = lang("Python", Some("#"), None, PYTHON);
static SH: Syntax = lang("Shell", Some("#"), None, SHELL);
static SQLL: Syntax = lang("SQL", Some("--"), Some(("/*", "*/")), SQL);
static JSON: Syntax = Syntax { keys: true, two_spaces: true, ..lang("JSON", None, None, &["true", "false", "null"]) };
static YAML: Syntax = Syntax { keys: true, two_spaces: true, ..lang("YAML", Some("#"), None, &["true", "false", "null", "yes", "no", "on", "off"]) };
static TOML: Syntax = Syntax { keys: true, ..lang("TOML", Some("#"), None, &["true", "false"]) };
static INI: Syntax = Syntax { keys: true, ..lang("INI", Some(";"), None, &["true", "false", "on", "off", "yes", "no"]) };
static CONF: Syntax = Syntax { keys: true, ..lang("Config", Some("#"), None, &["true", "false", "on", "off", "yes", "no"]) };
static ENV: Syntax = Syntax { keys: true, ..lang(".env", Some("#"), None, &["export"]) };
static NGX: Syntax = lang("Nginx", Some("#"), None, NGINX);
static DOCKERFILE: Syntax = lang("Dockerfile", Some("#"), None, DOCKER);
static MAKE: Syntax = lang("Makefile", Some("#"), None, SHELL);
static HTML: Syntax = Syntax { markup: true, two_spaces: true, ..lang("HTML", None, Some(("<!--", "-->")), &[]) };
static XML: Syntax = Syntax { markup: true, two_spaces: true, ..lang("XML", None, Some(("<!--", "-->")), &[]) };
static VUE: Syntax = Syntax { markup: true, two_spaces: true, ..lang("Vue", Some("//"), Some(("<!--", "-->")), C_LIKE) };
static MD: Syntax = lang("Markdown", None, Some(("<!--", "-->")), &[]);

impl Syntax {
    /// From the file name (its extension, or well-known names).
    pub fn detect(name: &str) -> &'static Syntax {
        let lower = name.to_lowercase();
        match lower.as_str() {
            "dockerfile" | "containerfile" => return &DOCKERFILE,
            "makefile" | "gnumakefile" => return &MAKE,
            ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".bash_aliases" | ".zprofile" | ".zshenv" => return &SH,
            ".env" => return &ENV,
            "nginx.conf" => return &NGX,
            _ => {}
        }
        if lower.starts_with(".env.") {
            return &ENV;
        }
        let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
        match ext {
            "rs" => &RUST,
            "js" | "mjs" | "cjs" | "jsx" => &JS,
            "ts" | "tsx" | "mts" => &TS,
            "c" | "h" | "cpp" | "cc" | "hpp" | "cs" => &C,
            "java" | "kt" | "kts" | "scala" | "groovy" | "gradle" | "swift" | "dart" => &JAVA,
            "go" => &GO,
            "php" => &PHP,
            "css" => &CSS,
            "scss" | "sass" | "less" => &SCSS,
            "py" | "pyw" => &PY,
            "sh" | "bash" | "zsh" | "fish" | "ksh" => &SH,
            "sql" => &SQLL,
            "json" | "jsonc" | "json5" => &JSON,
            "yml" | "yaml" | "neon" => &YAML,
            "toml" | "lock" => &TOML,
            "ini" | "cfg" | "properties" => &INI,
            "conf" | "cnf" | "service" | "timer" | "socket" | "htaccess" => {
                if lower.contains("nginx") || lower.ends_with(".vhost.conf") { &NGX } else { &CONF }
            }
            "env" => &ENV,
            "html" | "htm" | "twig" | "blade" | "hbs" | "ejs" => &HTML,
            "xml" | "svg" | "plist" | "xsd" | "xsl" | "csproj" => &XML,
            "vue" | "svelte" => &VUE,
            "md" | "markdown" => &MD,
            _ => &PLAIN,
        }
    }
}

/// Changes when the theme's colors do (the cached colors are then rebuilt).
fn color_key(theme: &Theme) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    theme.text.hash(&mut h);
    theme.ansi.hash(&mut h);
    theme.text_muted.hash(&mut h);
    h.finish()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Comment,
    Str,
    Number,
    Keyword,
    Key,
    Tag,
}

/// The text cut into colored pieces. A simple scanner for every language: comments, strings, numbers,
/// keywords, config keys, markup tags — good enough to read a file, never slow.
pub(super) fn highlight(text: &str, lang: &Syntax, theme: &Theme, font: &FontId) -> LayoutJob {
    let color = |k: Kind| match k {
        Kind::Text => theme.text,
        Kind::Comment => theme.text_muted,
        Kind::Str => theme.ansi[2],
        Kind::Number => theme.ansi[3],
        Kind::Keyword => theme.ansi[5],
        Kind::Key => theme.ansi[4],
        Kind::Tag => theme.ansi[6],
    };
    let mut job = LayoutJob::default();
    let push = |job: &mut LayoutJob, range: std::ops::Range<usize>, kind: Kind| {
        if range.is_empty() {
            return;
        }
        // Consecutive pieces of the same color are one section.
        if let Some(last) = job.sections.last_mut() {
            if last.byte_range.end.0 == range.start && last.format.color == color(kind) {
                last.byte_range.end = ByteIndex(range.end);
                return;
            }
        }
        job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: ByteIndex(range.start)..ByteIndex(range.end), format: TextFormat::simple(font.clone(), color(kind)) });
    };
    job.text = text.to_owned();
    // Plain text, or too big to color quickly: one piece.
    if std::ptr::eq(lang, &PLAIN) || text.len() > 2 * 1024 * 1024 {
        push(&mut job, 0..text.len(), Kind::Text);
        return job;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line_start = true;
    let mut in_tag = false;
    while i < bytes.len() {
        let rest = &text[i..];
        let c = bytes[i];
        if c == b'\n' {
            push(&mut job, i..i + 1, Kind::Text);
            i += 1;
            line_start = true;
            in_tag = false;
            continue;
        }
        if let Some((open, close)) = lang.block_comment {
            if let Some(inner) = rest.strip_prefix(open) {
                let end = inner.find(close).map_or(text.len(), |e| i + open.len() + e + close.len());
                push(&mut job, i..end, Kind::Comment);
                line_start = false;
                i = end;
                continue;
            }
        }
        if let Some(mark) = lang.line_comment {
            // "#" starts a comment at a word boundary only (not in "a#b", nor "#!" config values...).
            let boundary = mark != "#" || i == 0 || bytes[i - 1].is_ascii_whitespace();
            if boundary && rest.starts_with(mark) && !(mark == "//" && i > 0 && bytes[i - 1] == b':') {
                let end = rest.find('\n').map_or(text.len(), |e| i + e);
                push(&mut job, i..end, Kind::Comment);
                i = end;
                continue;
            }
        }
        if lang.markup && !in_tag && c == b'<' {
            let len = rest[1..].find(|ch: char| ch.is_whitespace() || ch == '>').map_or(rest.len(), |e| e + 1);
            push(&mut job, i..i + len, Kind::Tag);
            i += len;
            in_tag = true;
            line_start = false;
            continue;
        }
        if lang.markup && in_tag && (rest.starts_with("/>") || c == b'>') {
            let len = if c == b'>' { 1 } else { 2 };
            push(&mut job, i..i + len, Kind::Tag);
            i += len;
            in_tag = false;
            continue;
        }
        let quote = matches!(c, b'"' | b'\'' | b'`') && (lang.markup == in_tag || !lang.markup);
        // An apostrophe inside a word ("don't") in text-like files isn't a string.
        let word_apostrophe = c == b'\'' && i > 0 && bytes[i - 1].is_ascii_alphanumeric() && (lang.keys || std::ptr::eq(lang, &MD));
        if quote && !word_apostrophe && !std::ptr::eq(lang, &MD) {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != c && !(bytes[j] == b'\n' && c != b'`') {
                j += if bytes[j] == b'\\' && j + 1 < bytes.len() && bytes[j + 1] != b'\n' { 2 } else { 1 };
            }
            let end = (j + 1).min(bytes.len());
            let end = if end <= bytes.len() && text.is_char_boundary(end) { end } else { j.min(bytes.len()) };
            // JSON keys: a string followed by ":".
            let is_key = lang.keys && text[end..].trim_start_matches([' ', '\t']).starts_with(':');
            push(&mut job, i..end, if is_key { Kind::Key } else { Kind::Str });
            i = end;
            line_start = false;
            continue;
        }
        if lang.keys && line_start {
            let lead = rest.len() - rest.trim_start_matches([' ', '\t', '-']).len();
            let body = &rest[lead..];
            if body.starts_with('[') {
                let end = body.find('\n').unwrap_or(body.len());
                push(&mut job, i..i + lead, Kind::Text);
                push(&mut job, i + lead..i + lead + end, Kind::Keyword);
                i += lead + end;
                line_start = false;
                continue;
            }
            let key_len = body.find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '$'))).unwrap_or(body.len());
            let after = body[key_len..].trim_start_matches([' ', '\t']);
            if key_len > 0 && (after.starts_with(':') || after.starts_with('=')) {
                push(&mut job, i..i + lead, Kind::Text);
                push(&mut job, i + lead..i + lead + key_len, Kind::Key);
                i += lead + key_len;
                line_start = false;
                continue;
            }
        }
        if c.is_ascii_digit() && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) {
            let len = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_')).unwrap_or(rest.len());
            push(&mut job, i..i + len, Kind::Number);
            i += len;
            line_start = false;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c == b'@' {
            let len = rest[1..].find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).map_or(rest.len(), |e| e + 1);
            let word = &rest[..len];
            let kind = if lang.markup && in_tag {
                Kind::Key
            } else if lang.keywords.contains(&word) || (c == b'$' && std::ptr::eq(lang, &SH)) {
                Kind::Keyword
            } else {
                Kind::Text
            };
            push(&mut job, i..i + len, kind);
            i += len;
            line_start = false;
            continue;
        }
        let len = rest.chars().next().map_or(1, char::len_utf8);
        push(&mut job, i..i + len, Kind::Text);
        if !c.is_ascii_whitespace() {
            line_start = false;
        }
        i += len;
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_matches_with_positions() {
        let found = find_all("Été, été, ÉTÉ", "été", false);
        assert_eq!(found.len(), 3);
        assert_eq!((found[1].0, found[1].1), (5, 8));
        assert_eq!(&"Été, été, ÉTÉ"[found[1].2..found[1].3], "été");
        assert_eq!(find_all("Été, été", "été", true).len(), 1);
    }

    #[test]
    fn keeps_line_endings_and_bom() {
        let t = crate::i18n::Lang::En.strings();
        let (text, crlf, bom) = decode(b"\xEF\xBB\xBFa\r\nb\r\n", t).unwrap();
        assert_eq!((text.as_str(), crlf, bom), ("a\nb\n", true, true));
        assert!(decode(b"\x00\x01\xff", t).is_err());
    }

    #[test]
    fn detects_indentation() {
        assert_eq!(detect_indent("a:\n  b: 1\n  c: 2\n", &YAML), "  ");
        assert_eq!(detect_indent("fn a() {\n    b();\n}\n", &RUST), "    ");
        assert_eq!(detect_indent("a {\n\tb;\n\tc;\n}\n", &C), "\t");
    }

    #[test]
    fn colors_cover_the_whole_text() {
        let theme = Theme::default();
        for (name, text) in [("a.rs", "fn main() { let s = \"é\\\"x\"; // hé\n}\n"), ("a.yml", "a: 'b'\n# c\n- d: 1\n"), ("a.html", "<a href=\"x\">é</a><!-- c -->"), ("a.json", "{\"a\": [1, true]}")] {
            let job = highlight(text, Syntax::detect(name), &theme, &FontId::monospace(13.0));
            let mut at = 0;
            for s in &job.sections {
                assert_eq!(s.byte_range.start.0, at, "{name}");
                at = s.byte_range.end.0;
            }
            assert_eq!(at, text.len(), "{name}");
        }
    }
}
