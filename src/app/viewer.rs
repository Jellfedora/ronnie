//! Viewer of files too big to edit (logs, dumps: any size). Only a window of a few MB around what is
//! on screen is kept; moving through the file reads the part needed. Search goes through the file part
//! by part, and "follow" shows what is added at the end as it comes (like `tail -f`).

use std::time::{Duration, Instant};

use egui::EventFilter;

use super::editor::{highlight, Syntax};
use super::*;
use crate::sftp::MAX_RANGE;

/// Bytes kept around the place shown.
const WINDOW: u64 = 4 * 1024 * 1024;
/// Longest part of a line shown (minified files can have a single line of many MB).
const LINE_MAX: usize = 20_000;
const MONO: f32 = 13.0;

/// What the viewer asks the file manager for.
pub(super) struct RangeRequest {
    pub offset: u64,
    pub len: u64,
    pub tail: bool,
}

pub(super) enum ViewerAction {
    None,
    Close,
    /// Small enough: open it in the editor instead.
    Edit,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Want {
    /// The window around this offset.
    At(u64),
    /// The end of the file.
    Tail,
    /// A part to search, from this offset (forward) or up to it (backward).
    Search(u64, bool),
}

/// After a jump, `top` is moved to a line's start: the next one, or the one it is in.
#[derive(Clone, Copy, PartialEq)]
enum Align {
    Next,
    Containing,
}

struct Find {
    query: String,
    case: bool,
    focus: bool,
    /// A search going through the file: where it is.
    searching: Option<(u64, bool)>,
    /// The match shown (byte range).
    found: Option<(u64, u64)>,
    missed: bool,
}

pub(super) struct Viewer {
    pub remote: bool,
    pub path: String,
    name: String,
    size: u64,
    win_start: u64,
    win: Vec<u8>,
    loaded: bool,
    /// Byte offset of the first line on screen (a line's start once aligned).
    top: u64,
    align: Option<Align>,
    /// The end: after it is read, show its last lines.
    at_end: bool,
    want: Option<Want>,
    in_flight: Option<Want>,
    follow: bool,
    last_tail: Instant,
    scroll_x: f32,
    wheel: f32,
    rows: usize,
    find: Option<Find>,
    error: Option<String>,
    syntax: &'static Syntax,
    focus: bool,
    menu_line: Option<String>,
    close_requested: bool,
    /// Editable (not bigger than the editor's limit).
    editable: bool,
}

impl Viewer {
    pub fn new(remote: bool, path: String, name: String, size: u64) -> Self {
        Self {
            remote,
            syntax: Syntax::detect(&name),
            path,
            name,
            size,
            win_start: 0,
            win: Vec::new(),
            loaded: false,
            top: 0,
            align: None,
            at_end: false,
            want: Some(Want::At(0)),
            in_flight: None,
            follow: false,
            last_tail: Instant::now(),
            scroll_x: 0.0,
            wheel: 0.0,
            rows: 40,
            find: None,
            error: None,
            focus: true,
            menu_line: None,
            close_requested: false,
            editable: size <= crate::sftp::MAX_EDIT,
        }
    }

    fn win_end(&self) -> u64 {
        self.win_start + self.win.len() as u64
    }

    /// The next part to read, if any (one at a time).
    pub fn take_request(&mut self) -> Option<RangeRequest> {
        if self.in_flight.is_some() {
            return None;
        }
        if self.follow && self.want.is_none() && self.last_tail.elapsed() > Duration::from_secs(1) {
            self.want = Some(Want::Tail);
        }
        let want = self.want.take()?;
        self.in_flight = Some(want);
        Some(match want {
            Want::At(o) => RangeRequest { offset: o.saturating_sub(WINDOW / 2), len: WINDOW, tail: false },
            Want::Tail => RangeRequest { offset: 0, len: WINDOW, tail: true },
            Want::Search(o, true) => RangeRequest { offset: o, len: MAX_RANGE, tail: false },
            Want::Search(o, false) => RangeRequest { offset: o.saturating_sub(MAX_RANGE), len: MAX_RANGE.min(o), tail: false },
        })
    }

    /// A part arrived: (offset, bytes, file size now).
    pub fn range_arrived(&mut self, result: Result<(u64, Vec<u8>, u64), String>) {
        let Some(what) = self.in_flight.take() else { return };
        let (offset, data, size) = match result {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e);
                self.follow = false;
                if let Some(f) = &mut self.find {
                    f.searching = None;
                }
                return;
            }
        };
        self.size = size;
        match what {
            Want::Search(from, forward) => {
                self.search_part(offset, &data, from, forward);
            }
            Want::Tail => {
                self.last_tail = Instant::now();
                // Following: only when something was added (or the first time), keeping the view still otherwise.
                let grew = offset + data.len() as u64 != self.win_end() || !self.loaded;
                (self.win_start, self.win, self.loaded) = (offset, data, true);
                if grew || self.at_end {
                    self.show_end();
                }
            }
            Want::At(_) => {
                (self.win_start, self.win, self.loaded) = (offset, data, true);
                self.top = self.top.clamp(self.win_start, self.win_end());
                match self.align.take() {
                    Some(Align::Next) if self.top > 0 && self.byte(self.top - 1) != Some(b'\n') => {
                        self.top = self.next_line(self.top).unwrap_or(self.top);
                    }
                    Some(Align::Containing) => self.top = self.line_start(self.top),
                    _ => {}
                }
                if self.at_end {
                    self.show_end();
                }
            }
        }
    }

    fn byte(&self, at: u64) -> Option<u8> {
        at.checked_sub(self.win_start).and_then(|i| self.win.get(i as usize)).copied()
    }

    /// Start of the line after the one at `at` (None: not in the window).
    fn next_line(&self, at: u64) -> Option<u64> {
        let i = at.checked_sub(self.win_start)? as usize;
        self.win.get(i..)?.iter().position(|b| *b == b'\n').map(|p| at + p as u64 + 1)
    }

    /// Start of the line `at` is in (the window's start if it doesn't say).
    fn line_start(&self, at: u64) -> u64 {
        let i = (at.saturating_sub(self.win_start) as usize).min(self.win.len());
        match self.win[..i].iter().rposition(|b| *b == b'\n') {
            Some(p) => self.win_start + p as u64 + 1,
            None => self.win_start,
        }
    }

    /// The last lines of the file on screen.
    fn show_end(&mut self) {
        self.at_end = false;
        let mut at = self.win_end();
        // The file's last "\n" doesn't start an empty line on screen.
        if at > self.win_start && self.byte(at - 1) == Some(b'\n') {
            at -= 1;
        }
        for _ in 0..self.rows.saturating_sub(2) {
            let start = self.line_start(at);
            if start == self.win_start {
                at = start;
                break;
            }
            at = start - 1;
        }
        self.top = self.line_start(at);
    }

    /// Scrolls by `n` lines (negative: up).
    fn scroll_lines(&mut self, n: i64) {
        for _ in 0..n.unsigned_abs() {
            if n > 0 {
                match self.next_line(self.top) {
                    Some(next) if next < self.size => self.top = next,
                    Some(_) => break,
                    None => {
                        if self.win_end() < self.size {
                            self.want_at(self.top, None);
                        }
                        break;
                    }
                }
            } else {
                if self.top == 0 {
                    break;
                }
                let start = self.line_start(self.top - 1);
                if start == self.win_start && self.win_start > 0 {
                    self.want_at(self.top, None);
                    break;
                }
                self.top = start;
            }
        }
        // Near the window's edges: read around the new place ahead of time.
        let margin = WINDOW / 8;
        if (self.top.saturating_sub(self.win_start) < margin && self.win_start > 0) || (self.win_end().saturating_sub(self.top) < margin && self.win_end() < self.size) {
            self.want_at(self.top, None);
        }
    }

    fn want_at(&mut self, offset: u64, align: Option<Align>) {
        if self.in_flight.is_none() || align.is_some() {
            self.want = Some(Want::At(offset));
        }
        if align.is_some() {
            self.align = align;
        }
    }

    /// Jumps to a byte offset (scroll bar, search), showing its line.
    fn jump(&mut self, offset: u64, align: Align) {
        self.top = offset.min(self.size);
        if offset >= self.win_start && offset <= self.win_end() && (offset + WINDOW / 8 < self.win_end() || self.win_end() == self.size) && (offset >= self.win_start + WINDOW / 8 || self.win_start == 0) {
            self.top = match align {
                Align::Next if self.top > 0 && self.byte(self.top - 1) != Some(b'\n') => self.next_line(self.top).unwrap_or(self.top),
                Align::Containing => self.line_start(self.top),
                _ => self.top,
            };
        } else {
            self.want_at(offset, Some(align));
        }
    }

    pub fn open_find(&mut self) {
        match &mut self.find {
            Some(f) => f.focus = true,
            None => self.find = Some(Find { query: String::new(), case: false, focus: true, searching: None, found: None, missed: false }),
        }
    }

    pub fn request_close(&mut self) {
        self.close_requested = true;
    }

    /// Starts a search from the match shown (or the top of the screen).
    fn search(&mut self, forward: bool) {
        let top = self.top;
        let Some(f) = &mut self.find else { return };
        if f.query.is_empty() {
            return;
        }
        f.missed = false;
        let from = match f.found {
            Some((a, b)) => if forward { b } else { a },
            None => top,
        };
        f.searching = Some((from, forward));
        // The window first; the rest part by part.
        let (start, data) = (self.win_start, std::mem::take(&mut self.win));
        let done = self.search_part(start, &data, from, forward);
        self.win = data;
        if !done {
            let f = self.find.as_mut().unwrap();
            let next = if forward { self.win_start + self.win.len() as u64 } else { self.win_start };
            let next = if forward { next.max(from) } else { next.min(from) };
            f.searching = Some((next, forward));
            self.want = Some(Want::Search(next, forward));
        }
    }

    /// Searches `data` (at `offset`) for the query after `from` (forward) or before it. Returns whether
    /// the search is over (found, or the file's end reached).
    fn search_part(&mut self, offset: u64, data: &[u8], from: u64, forward: bool) -> bool {
        let Some(f) = &mut self.find else { return true };
        if f.searching.is_none() {
            return true;
        }
        let query = f.query.as_bytes().to_vec();
        let fold = |b: &[u8]| if f.case { b.to_vec() } else { b.to_ascii_lowercase() };
        let (hay, q) = (fold(data), fold(&query));
        let end = offset + data.len() as u64;
        let found = if forward {
            let skip = from.saturating_sub(offset) as usize;
            hay.get(skip..).and_then(|h| h.windows(q.len()).position(|w| w == q.as_slice())).map(|p| offset + (skip + p) as u64)
        } else {
            let upto = (from.saturating_sub(offset) as usize).min(hay.len());
            hay[..upto].windows(q.len()).rposition(|w| w == q.as_slice()).map(|p| offset + p as u64)
        };
        match found {
            Some(at) => {
                f.found = Some((at, at + query.len() as u64));
                f.searching = None;
                self.jump(at, Align::Containing);
                true
            }
            None if (forward && end >= self.size) || (!forward && offset == 0) => {
                f.searching = None;
                f.missed = true;
                true
            }
            None => {
                // Overlap, for a match across two parts.
                let overlap = query.len().saturating_sub(1) as u64;
                let next = if forward { end.saturating_sub(overlap) } else { offset + overlap };
                f.searching = Some((next, forward));
                self.want = Some(Want::Search(next, forward));
                false
            }
        }
    }

    /// Lines on screen: (start offset, bytes).
    fn visible(&self, rows: usize) -> Vec<(u64, &[u8])> {
        let mut out = Vec::with_capacity(rows);
        let mut at = self.top;
        while out.len() < rows && at <= self.win_end() {
            let i = (at - self.win_start) as usize;
            let rest = &self.win[i..];
            match rest.iter().position(|b| *b == b'\n') {
                Some(p) => {
                    out.push((at, &rest[..p]));
                    at += p as u64 + 1;
                }
                None => {
                    // The window's end: the file's last line, or more to read.
                    if self.win_end() >= self.size && !rest.is_empty() {
                        out.push((at, rest));
                    }
                    break;
                }
            }
        }
        out
    }

    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) -> ViewerAction {
        let mut action = ViewerAction::None;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let modal_open = ui.ctx().memory(|m| m.top_modal_layer().is_some());
        let (find_key, next, prev, escape) = ui.input_mut(|i| {
            (
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::F)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::G)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::G)),
                !modal_open && i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if find_key {
            self.open_find();
        }
        if escape {
            match self.find.as_mut() {
                Some(f) if f.searching.is_some() => f.searching = None,
                Some(_) => {
                    self.find = None;
                    self.focus = true;
                }
                None => {}
            }
        }

        // Header.
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 36.0));
        ui.painter().rect_filled(head, 0.0, theme.chrome_bg);
        ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, theme.tab_hover));
        ui.scope_builder(egui::UiBuilder::new().max_rect(head.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new("👁").size(14.0).color(theme.accent));
            ui.label(egui::RichText::new(&self.name).size(14.0).strong());
            ui.label(egui::RichText::new(t.viewer_read_only).size(11.0).color(theme.bg).background_color(theme.text_muted));
            ui.add(egui::Label::new(egui::RichText::new(&self.path).size(12.0).color(theme.text_muted)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new(format!("✕  {}", t.close)).size(13.0)).clicked() {
                    self.close_requested = true;
                }
                if self.editable && ui.button(egui::RichText::new(format!("✎  {}", t.files_edit)).size(13.0)).clicked() {
                    action = ViewerAction::Edit;
                }
                ui.add_space(6.0);
                if ui.button("🔍").on_hover_text(t.editor_find).clicked() {
                    self.open_find();
                }
                if ui.button("⤓").on_hover_text(t.viewer_end).clicked() {
                    self.at_end = true;
                    self.want = Some(Want::Tail);
                }
                if ui.toggle_value(&mut self.follow, egui::RichText::new(t.viewer_follow).size(12.5)).on_hover_text(t.viewer_follow_hint).changed() && self.follow {
                    self.at_end = true;
                    self.want = Some(Want::Tail);
                }
            });
        });
        let mut top = head.max.y + 1.0;

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
        if self.find.is_some() {
            top = self.find_bar(ui, Rect::from_min_max(Pos2::new(rect.min.x, top), rect.max), theme, t, next, prev);
        }

        let status_h = 24.0;
        let body = Rect::from_min_max(Pos2::new(rect.min.x, top), Pos2::new(rect.max.x, rect.max.y - status_h));
        if self.loaded {
            self.text_ui(ui, body, theme, t);
        } else {
            ui.painter().text(body.center(), Align2::CENTER_CENTER, t.files_loading, FontId::proportional(13.0), theme.text_muted);
        }

        let bar = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - status_h), rect.max);
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.min.y, Stroke::new(1.0, theme.tab_hover));
        let percent = (self.top * 100).checked_div(self.size).unwrap_or(0);
        let right = format!("{} %    {}    {}", percent, format_bytes(self.size, t), if self.syntax_name().is_empty() { t.editor_plain } else { self.syntax_name() });
        ui.painter().text(bar.right_center() - Vec2::new(12.0, 0.0), Align2::RIGHT_CENTER, right, FontId::proportional(11.5), theme.text_muted);
        if self.follow {
            ui.painter().text(bar.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, format!("●  {}", t.viewer_following), FontId::proportional(11.5), theme.ansi[2]);
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        if self.in_flight.is_some() || self.want.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if self.close_requested {
            self.close_requested = false;
            action = ViewerAction::Close;
        }
        action
    }

    fn syntax_name(&self) -> &'static str {
        self.syntax.name
    }

    fn find_bar(&mut self, ui: &mut Ui, area: Rect, theme: &Theme, t: &Strings, next: bool, prev: bool) -> f32 {
        let r = Rect::from_min_size(area.min, Vec2::new(area.width(), 38.0));
        ui.painter().rect_filled(r, 0.0, theme.chrome_bg);
        ui.painter().hline(r.x_range(), r.max.y, Stroke::new(1.0, theme.tab_hover));
        let (mut go_next, mut go_prev, mut close) = (next, prev, false);
        let size = self.size.max(1);
        let f = self.find.as_mut().expect("find bar");
        let before = (f.query.clone(), f.case);
        ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut f.query).hint_text(t.editor_find).desired_width(260.0).font(FontId::monospace(12.5)));
            if f.focus {
                edit.request_focus();
                f.focus = false;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                if ui.input(|i| i.modifiers.shift) { go_prev = true } else { go_next = true }
                edit.request_focus();
            }
            if ui.small_button("↑").clicked() {
                go_prev = true;
            }
            if ui.small_button("↓").clicked() {
                go_next = true;
            }
            ui.toggle_value(&mut f.case, egui::RichText::new("Aa").size(12.0)).on_hover_text(t.editor_case);
            let (info, color) = match (f.searching, f.missed) {
                (Some((at, _)), _) => (t.viewer_searching.replace("{p}", &(at * 100 / size).to_string()), theme.text_muted),
                (None, true) => (t.editor_no_match.to_owned(), theme.ansi[1]),
                _ => (String::new(), theme.text_muted),
            };
            ui.label(egui::RichText::new(info).size(12.0).color(color));
            if f.searching.is_some() {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    close = true;
                }
            });
        });
        if before != (f.query.clone(), f.case) {
            (f.found, f.missed, f.searching) = (None, false, None);
        }
        if close {
            self.find = None;
            self.focus = true;
        } else if go_next || go_prev {
            self.search(go_next);
        }
        r.max.y
    }

    fn text_ui(&mut self, ui: &mut Ui, body: Rect, theme: &Theme, t: &Strings) {
        let id = egui::Id::new(("file-viewer", &self.path));
        let font = FontId::monospace(MONO);
        let row_h = ui.fonts_mut(|f| f.row_height(&font)).round();
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let bar_w = 10.0;
        let text_rect = Rect::from_min_max(body.min + Vec2::new(10.0, 6.0), Pos2::new(body.max.x - bar_w, body.max.y));
        let rows = ((text_rect.height()) / row_h).floor().max(1.0) as usize;
        self.rows = rows;

        let response = ui.interact(body, id, Sense::click());
        if self.focus || response.clicked() {
            response.request_focus();
            self.focus = false;
        }
        if response.has_focus() {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false }));
            let (up, down, pg_up, pg_down, home, end, left, right) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::PageUp),
                    i.consume_key(Modifiers::NONE, Key::PageDown),
                    i.consume_key(Modifiers::NONE, Key::Home) || i.consume_key(Modifiers::COMMAND, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::End) || i.consume_key(Modifiers::COMMAND, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                    i.consume_key(Modifiers::NONE, Key::ArrowRight),
                )
            });
            let page = rows.saturating_sub(1).max(1) as i64;
            if up {
                self.scroll_lines(-1);
            }
            if down {
                self.scroll_lines(1);
            }
            if pg_up {
                self.scroll_lines(-page);
            }
            if pg_down {
                self.scroll_lines(page);
            }
            if home {
                self.jump(0, Align::Next);
            }
            if end {
                self.at_end = true;
                self.want = Some(Want::Tail);
            }
            if left {
                self.scroll_x = (self.scroll_x - char_w * 8.0).max(0.0);
            }
            if right {
                self.scroll_x += char_w * 8.0;
            }
        }
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta.y != 0.0 {
                // Whole lines; the rest is kept for the next frame.
                self.wheel -= delta.y / row_h;
                let lines = self.wheel.trunc();
                if lines != 0.0 {
                    self.wheel -= lines;
                    self.scroll_lines(lines as i64);
                    self.follow &= lines > 0.0;
                }
            }
            self.scroll_x = (self.scroll_x - delta.x).max(0.0);
        }

        let painter = ui.painter_at(Rect::from_min_max(body.min, Pos2::new(body.max.x - bar_w, body.max.y)));
        let found = self.find.as_ref().and_then(|f| f.found);
        let visible: Vec<(u64, String)> = self.visible(rows).into_iter().map(|(at, bytes)| (at, String::from_utf8_lossy(&bytes[..bytes.len().min(LINE_MAX)]).into_owned())).collect();
        let pointer = ui.input(|i| i.pointer.interact_pos());
        for (k, (at, text)) in visible.iter().enumerate() {
            let y = text_rect.min.y + k as f32 * row_h;
            let x = text_rect.min.x - self.scroll_x;
            // The match shown.
            if let Some((a, b)) = found.filter(|(a, _)| *a >= *at && *a <= at + text.len() as u64) {
                let from = (a - at) as usize;
                let to = ((b - at) as usize).min(text.len());
                if text.is_char_boundary(from) && text.is_char_boundary(to) {
                    let x0 = x + text[..from].chars().count() as f32 * char_w;
                    let x1 = x0 + text[from..to].chars().count() as f32 * char_w;
                    let r = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + row_h));
                    painter.rect_filled(r, 2.0, theme.accent.gamma_multiply(0.45));
                    painter.rect_stroke(r, 2.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Outside);
                }
            }
            let galley = ui.painter().layout_job(highlight(text, self.syntax, theme, &font));
            painter.galley(Pos2::new(x, y + (row_h - galley.size().y) / 2.0), galley, theme.text);
            if response.secondary_clicked() && pointer.is_some_and(|p| p.y >= y && p.y < y + row_h) {
                self.menu_line = Some(text.clone());
            }
        }
        response.context_menu(|ui| {
            if let Some(line) = &self.menu_line {
                if ui.button(t.viewer_copy_line).clicked() {
                    ui.ctx().copy_text(line.clone());
                    ui.close();
                }
            }
        });

        // Scroll bar: the place in the file (drag to go anywhere).
        let bar = Rect::from_min_max(Pos2::new(body.max.x - bar_w, body.min.y), body.max);
        let thumb_h = 28.0_f32.min(bar.height());
        let fraction = if self.size > 0 { self.top as f64 / self.size as f64 } else { 0.0 };
        let thumb = Rect::from_min_size(Pos2::new(bar.min.x + 2.0, bar.min.y + fraction as f32 * (bar.height() - thumb_h)), Vec2::new(bar_w - 4.0, thumb_h));
        let bar_resp = ui.interact(bar, id.with("bar"), Sense::click_and_drag());
        if let Some(p) = bar_resp.interact_pointer_pos().filter(|_| bar_resp.dragged() || bar_resp.clicked()) {
            let fraction = ((p.y - bar.min.y - thumb_h / 2.0) / (bar.height() - thumb_h).max(1.0)).clamp(0.0, 1.0) as f64;
            self.follow = false;
            self.jump((fraction * self.size as f64) as u64, Align::Next);
        }
        ui.painter().rect_filled(thumb, 4.0, if bar_resp.hovered() || bar_resp.dragged() { theme.text_muted.gamma_multiply(0.8) } else { theme.text_muted.gamma_multiply(0.4) });
    }
}

/// "1,2 Go" / "1.2 GB".
fn format_bytes(bytes: u64, t: &Strings) -> String {
    let units = [t.unit_b, t.unit_kb, t.unit_mb, t.unit_gb, t.unit_tb];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if unit == 0 { bytes.to_string() } else { format!("{value:.1}") };
    let text = if t.decimal_comma { text.replace('.', ",") } else { text };
    format!("{text} {}", units[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds the viewer from a file in memory, as the file manager does.
    fn serve(v: &mut Viewer, file: &[u8]) {
        while let Some(r) = v.take_request() {
            let size = file.len() as u64;
            let offset = if r.tail { size.saturating_sub(r.len) } else { r.offset.min(size) };
            let end = (offset + r.len.min(MAX_RANGE)).min(size);
            v.range_arrived(Ok((offset, file[offset as usize..end as usize].to_vec(), size)));
        }
    }

    fn big_file() -> Vec<u8> {
        (0..400_000).map(|i| format!("line {i:06}\n")).collect::<String>().into_bytes()
    }

    #[test]
    fn scrolls_through_a_big_file() {
        let file = big_file();
        let mut v = Viewer::new(false, "x".into(), "x.log".into(), file.len() as u64);
        v.rows = 10;
        serve(&mut v, &file);
        assert_eq!(v.visible(1)[0].1, b"line 000000");
        v.scroll_lines(3);
        assert_eq!(v.visible(1)[0].1, b"line 000003");
        v.scroll_lines(-5);
        assert_eq!(v.visible(1)[0].1, b"line 000000");
        // Far away (past the window): read there, then aligned on a line.
        v.jump(file.len() as u64 / 2 + 5, Align::Next);
        serve(&mut v, &file);
        let first = String::from_utf8(v.visible(1)[0].1.to_vec()).unwrap();
        assert!(first.starts_with("line 2000"), "{first}");
        // Scrolling on and on reads ahead.
        for _ in 0..1000 {
            v.scroll_lines(500);
            serve(&mut v, &file);
        }
        assert!(v.visible(10).last().unwrap().1.starts_with(b"line 3999"));
        // The end.
        v.at_end = true;
        v.want = Some(Want::Tail);
        serve(&mut v, &file);
        let lines = v.visible(10);
        assert_eq!(lines.last().unwrap().1, b"line 399999");
    }

    #[test]
    fn searches_the_whole_file() {
        let file = big_file();
        let mut v = Viewer::new(false, "x".into(), "x.log".into(), file.len() as u64);
        serve(&mut v, &file);
        v.open_find();
        v.find.as_mut().unwrap().query = "LINE 3500".into();
        v.search(true);
        serve(&mut v, &file);
        let found = v.find.as_ref().unwrap().found.unwrap();
        assert_eq!(&file[found.0 as usize..found.1 as usize], b"line 3500");
        assert!(v.visible(1)[0].1.starts_with(b"line 3500"));
        // Backward from there: the one before.
        v.find.as_mut().unwrap().query = "line 0000".into();
        v.find.as_mut().unwrap().found = Some(found);
        v.search(false);
        serve(&mut v, &file);
        assert!(v.visible(1)[0].1.starts_with(b"line 0000"));
        v.find.as_mut().unwrap().query = "absent".into();
        v.find.as_mut().unwrap().found = None;
        v.search(true);
        serve(&mut v, &file);
        assert!(v.find.as_ref().unwrap().missed);
    }
}
