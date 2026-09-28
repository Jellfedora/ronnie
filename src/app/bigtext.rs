//! The editor's engine for big files (up to `sftp::MAX_EDIT`): the text is kept line by line, and only
//! the lines on screen are laid out and painted, so that typing and scrolling stay smooth whatever the
//! size. Its own cursor, selection, keyboard, mouse, clipboard and undo.

use std::time::Instant;

use egui::{Event, EventFilter};

use super::editor::{highlight, Syntax};
use super::*;

/// A place in the text: line, and byte offset in it (always on a char boundary).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub(super) struct Pos {
    pub line: usize,
    pub col: usize,
}

/// One change: the text at `start` that was `removed` became `inserted`.
#[derive(Clone, Debug)]
struct Change {
    start: Pos,
    removed: String,
    inserted: String,
}

/// Changes undone together, and the selection before them.
struct Group {
    changes: Vec<Change>,
    before: (Pos, Pos),
    at: Instant,
    /// Typing: the next typed letters join this group.
    typing: bool,
}

pub(super) struct BigText {
    pub lines: Vec<String>,
    /// Selection: from `anchor` to `cursor` (equal: no selection).
    pub anchor: Pos,
    pub cursor: Pos,
    /// First line on screen (fractional while scrolling), and horizontal scroll in points.
    top: f64,
    scroll_x: f32,
    /// The column (in chars) up / down keep going to.
    wanted_col: Option<usize>,
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Bumped by every change.
    pub revision: u64,
    /// The cursor must be scrolled into view (after a key, not after a wheel scroll).
    reveal: bool,
    /// Lines on screen, as drawn last frame (page up / down).
    page: usize,
    dragging: bool,
}

const MONO: f32 = 13.0;

impl BigText {
    /// From text whose line endings are "\n".
    pub fn new(text: &str) -> Self {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        Self { lines, anchor: Pos::default(), cursor: Pos::default(), top: 0.0, scroll_x: 0.0, wanted_col: None, undo: Vec::new(), redo: Vec::new(), revision: 0, reveal: false, page: 30, dragging: false }
    }

    /// The whole text with `eol` between lines.
    pub fn to_bytes(&self, eol: &str) -> Vec<u8> {
        let total: usize = self.lines.iter().map(|l| l.len() + eol.len()).sum();
        let mut out = Vec::with_capacity(total);
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(eol.as_bytes());
            }
            out.extend_from_slice(line.as_bytes());
        }
        out
    }

    fn sorted(&self) -> (Pos, Pos) {
        if self.anchor <= self.cursor { (self.anchor, self.cursor) } else { (self.cursor, self.anchor) }
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.cursor
    }

    pub fn text_range(&self, a: Pos, b: Pos) -> String {
        if a.line == b.line {
            return self.lines[a.line][a.col..b.col].to_owned();
        }
        let mut out = self.lines[a.line][a.col..].to_owned();
        for line in &self.lines[a.line + 1..b.line] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(&self.lines[b.line][..b.col]);
        out
    }

    pub fn selected_text(&self) -> String {
        let (a, b) = self.sorted();
        self.text_range(a, b)
    }

    /// Replaces `a..b` with `text`; returns the end of the inserted text.
    fn raw_replace(&mut self, a: Pos, b: Pos, text: &str) -> Pos {
        let tail = self.lines[b.line][b.col..].to_owned();
        let head = &self.lines[a.line][..a.col];
        let mut parts = text.split('\n');
        let first = parts.next().unwrap_or("");
        let mut new_lines: Vec<String> = vec![format!("{head}{first}")];
        new_lines.extend(parts.map(str::to_owned));
        let last = new_lines.len() - 1;
        let end = Pos { line: a.line + last, col: new_lines[last].len() };
        new_lines[last].push_str(&tail);
        self.lines.splice(a.line..=b.line, new_lines);
        self.revision += 1;
        end
    }

    /// End of `text` inserted at `start`.
    fn end_of(start: Pos, text: &str) -> Pos {
        match text.rfind('\n') {
            Some(i) => Pos { line: start.line + text.matches('\n').count(), col: text.len() - i - 1 },
            None => Pos { line: start.line, col: start.col + text.len() },
        }
    }

    /// Replaces `a..b` with `text`, undoably; `typing` lets consecutive letters undo as one.
    fn edit(&mut self, a: Pos, b: Pos, text: &str, typing: bool) -> Pos {
        let removed = self.text_range(a, b);
        let end = self.raw_replace(a, b, text);
        let change = Change { start: a, removed, inserted: text.to_owned() };
        let joins = typing && self.undo.last().is_some_and(|g| g.typing && g.at.elapsed().as_secs_f32() < 1.5 && !text.contains(char::is_whitespace));
        if joins {
            let group = self.undo.last_mut().unwrap();
            group.changes.push(change);
            group.at = Instant::now();
        } else {
            self.undo.push(Group { changes: vec![change], before: (self.anchor, self.cursor), at: Instant::now(), typing });
            if self.undo.len() > 500 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        end
    }

    /// Replaces the selection (or inserts at the cursor).
    pub fn insert(&mut self, text: &str, typing: bool) {
        let (a, b) = self.sorted();
        let end = self.edit(a, b, text, typing);
        self.set_cursor(end, false);
    }

    fn undo_redo(&mut self, undo: bool) {
        let Some(group) = (if undo { self.undo.pop() } else { self.redo.pop() }) else { return };
        let mut last = group.before.1;
        let order: Vec<&Change> = if undo { group.changes.iter().rev().collect() } else { group.changes.iter().collect() };
        for c in order {
            let (from, to) = if undo { (&c.inserted, &c.removed) } else { (&c.removed, &c.inserted) };
            let end = Self::end_of(c.start, from);
            last = self.raw_replace(c.start, end, to);
        }
        let now = (self.anchor, self.cursor);
        if undo {
            (self.anchor, self.cursor) = group.before;
        } else {
            (self.anchor, self.cursor) = (last, last);
        }
        let back = Group { changes: group.changes, before: now, at: Instant::now(), typing: false };
        if undo { self.redo.push(back) } else { self.undo.push(back) }
        self.reveal = true;
    }

    pub fn set_cursor(&mut self, p: Pos, extend: bool) {
        self.cursor = self.clamp(p);
        if !extend {
            self.anchor = self.cursor;
        }
        self.reveal = true;
    }

    pub fn select(&mut self, a: Pos, b: Pos) {
        self.anchor = self.clamp(a);
        self.cursor = self.clamp(b);
        self.wanted_col = None;
        self.reveal = true;
    }

    fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        let mut col = p.col.min(text.len());
        while !text.is_char_boundary(col) {
            col -= 1;
        }
        Pos { line, col }
    }

    fn char_col(&self, p: Pos) -> usize {
        self.lines[p.line][..p.col].chars().count()
    }

    fn byte_col(&self, line: usize, chars: usize) -> usize {
        self.lines[line].char_indices().nth(chars).map_or(self.lines[line].len(), |(i, _)| i)
    }

    /// Next / previous char boundary, across lines.
    fn step(&self, p: Pos, forward: bool) -> Pos {
        let text = &self.lines[p.line];
        if forward {
            match text[p.col..].chars().next() {
                Some(c) => Pos { line: p.line, col: p.col + c.len_utf8() },
                None if p.line + 1 < self.lines.len() => Pos { line: p.line + 1, col: 0 },
                None => p,
            }
        } else {
            match text[..p.col].chars().next_back() {
                Some(c) => Pos { line: p.line, col: p.col - c.len_utf8() },
                None if p.line > 0 => Pos { line: p.line - 1, col: self.lines[p.line - 1].len() },
                None => p,
            }
        }
    }

    /// Next / previous word boundary on the line (or the line's end / start).
    fn word(&self, p: Pos, forward: bool) -> Pos {
        let text = &self.lines[p.line];
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        if forward {
            if p.col == text.len() {
                return self.step(p, true);
            }
            let rest = &text[p.col..];
            let skip = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
            let after = &rest[skip..];
            let first_word = after.chars().next().is_some_and(is_word);
            let len = after.find(|c: char| is_word(c) != first_word || c.is_whitespace()).unwrap_or(after.len()).max(after.chars().next().map_or(0, char::len_utf8));
            Pos { line: p.line, col: p.col + skip + len }
        } else {
            if p.col == 0 {
                return self.step(p, false);
            }
            let before = &text[..p.col];
            let trimmed = before.trim_end();
            let last_word = trimmed.chars().next_back().is_some_and(is_word);
            let start = trimmed.rfind(|c: char| is_word(c) != last_word || c.is_whitespace()).map_or(0, |i| i + trimmed[i..].chars().next().map_or(1, char::len_utf8));
            let start = if start == trimmed.len() && start > 0 { start - trimmed[..start].chars().next_back().map_or(1, char::len_utf8) } else { start };
            Pos { line: p.line, col: start }
        }
    }

    /// The word around `p` (double click).
    fn word_at(&self, p: Pos) -> (Pos, Pos) {
        let text = &self.lines[p.line];
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let start = text[..p.col].rfind(|c: char| !is_word(c)).map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
        let end = text[p.col..].find(|c: char| !is_word(c)).map_or(text.len(), |i| p.col + i);
        (Pos { line: p.line, col: start }, Pos { line: p.line, col: end })
    }

    /// Searches `query` from the cursor (forward from the selection's end, backward from its start),
    /// wrapping around. Returns the match.
    pub fn find(&self, query: &str, case: bool, forward: bool) -> Option<(Pos, Pos)> {
        if query.is_empty() || query.contains('\n') {
            return None;
        }
        let (a, b) = self.sorted();
        let n = self.lines.len();
        let lower_q = query.to_lowercase();
        let find_in = |line: &str, from: usize, to: usize, last: bool| -> Option<usize> {
            let hay = &line[from..to];
            if case {
                if last { hay.rfind(query) } else { hay.find(query) }.map(|i| i + from)
            } else {
                // Positions in the lowered text match the original only for ASCII; otherwise compare
                // char by char.
                if hay.is_ascii() && query.is_ascii() {
                    let lower = hay.to_ascii_lowercase();
                    if last { lower.rfind(&lower_q) } else { lower.find(&lower_q) }.map(|i| i + from)
                } else {
                    let starts: Vec<usize> = hay.char_indices().map(|(i, _)| i).filter(|&i| hay[i..].chars().zip(query.chars()).filter(|(x, y)| x.to_lowercase().eq(y.to_lowercase())).count() == query.chars().count()).collect();
                    let valid = |i: &usize| hay[*i..].chars().count() >= query.chars().count();
                    if last { starts.into_iter().rev().find(valid) } else { starts.into_iter().find(valid) }.map(|i| i + from)
                }
            }
        };
        let len_at = |line: &str, at: usize| -> usize { line[at..].char_indices().nth(query.chars().count()).map_or(line.len() - at, |(i, _)| i) };
        for k in 0..=n {
            let line_no = if forward { (b.line + k) % n } else { (a.line + n - k % n) % n };
            let line = &self.lines[line_no];
            let (from, to) = match (k, forward) {
                (0, true) => (b.col, line.len()),
                (0, false) => (0, a.col),
                // Back to the starting line after wrapping: the part not searched yet.
                (k, true) if k == n => (0, b.col.min(line.len())),
                (k, false) if k == n => (a.col.min(line.len()), line.len()),
                _ => (0, line.len()),
            };
            if from > to {
                continue;
            }
            if let Some(at) = find_in(line, from, to, !forward) {
                let len = len_at(line, at);
                return Some((Pos { line: line_no, col: at }, Pos { line: line_no, col: at + len }));
            }
        }
        None
    }

    /// Replaces every `query` with `with`, as one undo step. Returns how many.
    pub fn replace_all(&mut self, query: &str, with: &str, case: bool) -> usize {
        if query.is_empty() || query.contains('\n') {
            return 0;
        }
        let before = (self.anchor, self.cursor);
        let lower_q = query.to_lowercase();
        let mut changes = Vec::new();
        for (i, line) in self.lines.iter_mut().enumerate() {
            let found: Vec<usize> = if case {
                line.match_indices(query).map(|(at, _)| at).collect()
            } else if line.is_ascii() && query.is_ascii() {
                line.to_ascii_lowercase().match_indices(&lower_q).map(|(at, _)| at).collect()
            } else {
                // Rare: non-ASCII without case; lowered text may not line up, so skip those lines.
                let lower = line.to_lowercase();
                if lower.len() == line.len() { lower.match_indices(&lower_q).map(|(at, _)| at).collect() } else { Vec::new() }
            };
            if found.is_empty() {
                continue;
            }
            let mut out = String::with_capacity(line.len());
            let mut last = 0;
            let mut shift: isize = 0;
            for at in found {
                if at < last {
                    continue;
                }
                out.push_str(&line[last..at]);
                out.push_str(with);
                let start = Pos { line: i, col: (at as isize + shift) as usize };
                changes.push(Change { start, removed: line[at..at + query.len()].to_owned(), inserted: with.to_owned() });
                shift += with.len() as isize - query.len() as isize;
                last = at + query.len();
            }
            out.push_str(&line[last..]);
            *line = out;
        }
        let n = changes.len();
        if n > 0 {
            self.revision += 1;
            self.undo.push(Group { changes, before, at: Instant::now(), typing: false });
            self.redo.clear();
            let c = self.clamp(self.cursor);
            (self.anchor, self.cursor) = (c, c);
        }
        n
    }

    /// Lines touched by the selection.
    fn selected_lines(&self) -> std::ops::RangeInclusive<usize> {
        let (a, b) = self.sorted();
        let last = if b.col == 0 && b.line > a.line { b.line - 1 } else { b.line };
        a.line..=last
    }

    /// Rewrites whole lines (indent, comment), as one undo step, keeping them selected.
    fn map_lines(&mut self, f: impl Fn(&str) -> String) {
        let range = self.selected_lines();
        let (first, last) = (*range.start(), *range.end());
        let old = self.text_range(Pos { line: first, col: 0 }, Pos { line: last, col: self.lines[last].len() });
        let new: Vec<String> = old.split('\n').map(&f).collect();
        let new = new.join("\n");
        if new == old {
            return;
        }
        let end = self.edit(Pos { line: first, col: 0 }, Pos { line: last, col: self.lines[last].len() }, &new, false);
        self.anchor = Pos { line: first, col: 0 };
        self.cursor = end;
    }

    pub fn indent(&mut self, unit: &str, back: bool) {
        let n = unit.len().max(1);
        if back {
            self.map_lines(|l| {
                let cut = if l.starts_with('\t') { 1 } else { l.chars().take(n).take_while(|c| *c == ' ').count() };
                l[cut..].to_owned()
            });
        } else if !self.has_selection() {
            let col = self.char_col(self.cursor);
            let text = if unit == "\t" { "\t".to_owned() } else { " ".repeat(n - col % n) };
            self.insert(&text, false);
        } else {
            self.map_lines(|l| if l.is_empty() { String::new() } else { format!("{unit}{l}") });
        }
    }

    pub fn toggle_comment(&mut self, mark: &str) {
        let prefix = format!("{mark} ");
        let range = self.selected_lines();
        let lines: Vec<&String> = self.lines[range].iter().filter(|l| !l.trim().is_empty()).collect();
        let all = !lines.is_empty() && lines.iter().all(|l| l.trim_start().starts_with(mark));
        let min_indent = lines.iter().map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
        self.map_lines(|l| {
            if l.trim().is_empty() {
                l.to_owned()
            } else if all {
                let lead = l.len() - l.trim_start().len();
                let rest = &l[lead..];
                let rest = rest.strip_prefix(prefix.as_str()).or_else(|| rest.strip_prefix(mark)).unwrap_or(rest);
                format!("{}{rest}", &l[..lead])
            } else {
                format!("{}{prefix}{}", &l[..min_indent], &l[min_indent..])
            }
        });
    }

    /// Enter: a new line with the current one's indentation (one level more after an opening bracket).
    fn newline(&mut self, unit: &str) {
        let line = &self.lines[self.cursor.line][..self.cursor.col];
        let mut indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if line.trim_end().ends_with(['{', '[', '(', ':']) {
            indent.push_str(unit);
        }
        self.insert(&format!("\n{indent}"), false);
    }

    /// Keyboard, clipboard and text input, while focused.
    fn keys(&mut self, ui: &Ui, unit: &str, comment: Option<&str>) {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        let mac = cfg!(target_os = "macos");
        let word_mod = if mac { mods.alt } else { mods.ctrl };
        for event in events {
            match event {
                Event::Text(text) if !(mods.command && !mods.alt) => {
                    let text: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !text.is_empty() {
                        self.insert(&text, true);
                    }
                }
                Event::Ime(egui::ImeEvent::Commit(text)) => self.insert(&text, true),
                Event::Paste(text) => self.insert(&text.replace("\r\n", "\n").replace('\r', "\n"), false),
                Event::Copy => {
                    if self.has_selection() {
                        ui.ctx().copy_text(self.selected_text());
                    }
                }
                Event::Cut => {
                    if self.has_selection() {
                        ui.ctx().copy_text(self.selected_text());
                        self.insert("", false);
                    }
                }
                Event::Key { key, pressed: true, modifiers: m, .. } => {
                    let shift = m.shift;
                    let move_to = |s: &mut Self, p: Pos| {
                        s.wanted_col = None;
                        s.set_cursor(p, shift);
                    };
                    match key {
                        Key::ArrowLeft | Key::ArrowRight => {
                            let forward = key == Key::ArrowRight;
                            let p = if mac && m.mac_cmd {
                                Pos { line: self.cursor.line, col: if forward { self.lines[self.cursor.line].len() } else { self.home(self.cursor) } }
                            } else if word_mod {
                                self.word(self.cursor, forward)
                            } else if self.has_selection() && !shift {
                                let (a, b) = self.sorted();
                                if forward { b } else { a }
                            } else {
                                self.step(self.cursor, forward)
                            };
                            move_to(self, p);
                        }
                        Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown => {
                            let down = matches!(key, Key::ArrowDown | Key::PageDown);
                            let jump = if matches!(key, Key::PageUp | Key::PageDown) { self.page.max(1) } else { 1 };
                            let p = if m.command && matches!(key, Key::ArrowUp | Key::ArrowDown) {
                                if down { Pos { line: self.lines.len() - 1, col: self.lines.last().map_or(0, String::len) } } else { Pos::default() }
                            } else {
                                let col = *self.wanted_col.get_or_insert(self.char_col(self.cursor));
                                let line = if down { (self.cursor.line + jump).min(self.lines.len() - 1) } else { self.cursor.line.saturating_sub(jump) };
                                Pos { line, col: self.byte_col(line, col) }
                            };
                            let wanted = self.wanted_col;
                            self.set_cursor(p, shift);
                            self.wanted_col = wanted;
                        }
                        Key::Home => {
                            let p = if m.command { Pos::default() } else { Pos { line: self.cursor.line, col: self.home(self.cursor) } };
                            move_to(self, p);
                        }
                        Key::End => {
                            let p = if m.command { Pos { line: self.lines.len() - 1, col: self.lines.last().map_or(0, String::len) } } else { Pos { line: self.cursor.line, col: self.lines[self.cursor.line].len() } };
                            move_to(self, p);
                        }
                        Key::Backspace | Key::Delete => {
                            if !self.has_selection() {
                                let forward = key == Key::Delete;
                                let to = if mac && m.mac_cmd && !forward {
                                    Pos { line: self.cursor.line, col: 0 }
                                } else if word_mod {
                                    self.word(self.cursor, forward)
                                } else {
                                    self.step(self.cursor, forward)
                                };
                                self.anchor = to;
                            }
                            self.wanted_col = None;
                            self.insert("", false);
                        }
                        Key::Enter if !m.command => {
                            self.wanted_col = None;
                            self.newline(unit);
                        }
                        Key::Tab if !m.command && !m.alt => {
                            self.wanted_col = None;
                            self.indent(unit, shift);
                        }
                        Key::A if m.command => {
                            let end = Pos { line: self.lines.len() - 1, col: self.lines.last().map_or(0, String::len) };
                            self.anchor = Pos::default();
                            self.cursor = end;
                        }
                        Key::Z if m.command => self.undo_redo(!shift),
                        Key::Y if m.command && !mac => self.undo_redo(false),
                        Key::Slash if m.command => {
                            if let Some(mark) = comment {
                                self.toggle_comment(mark);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Home: the first non-blank char, or the line's start if already there.
    fn home(&self, p: Pos) -> usize {
        let text = &self.lines[p.line];
        let first = text.len() - text.trim_start().len();
        if p.col == first { 0 } else { first }
    }

    /// The text area: lines on screen, line numbers, selection, matches, cursor. Returns whether the
    /// text changed.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, id: egui::Id, theme: &Theme, syntax: &Syntax, unit: &str, find: Option<(&str, bool)>, focus: bool) -> bool {
        let revision = self.revision;
        let font = FontId::monospace(MONO);
        let row_h = ui.fonts_mut(|f| f.row_height(&font)).round();
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let digits = self.lines.len().to_string().len().max(3) as f32;
        let gutter = digits * char_w + 22.0;
        let bar_w = 10.0;
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + gutter, rect.min.y), Pos2::new(rect.max.x - bar_w, rect.max.y));
        let rows = ((rect.height() - 8.0) / row_h).floor().max(1.0) as usize;
        self.page = rows.saturating_sub(1).max(1);
        let pad = 6.0;

        let response = ui.interact(rect, id, Sense::click_and_drag());
        if focus || response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if response.hovered() && !self.dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false }));
            self.keys(ui, unit, syntax.line_comment);
            // Lets the platform's input method (accents, other scripts) work here.
            let cursor_rect = self.cursor_rect(text_rect, row_h, char_w, pad);
            ui.ctx().output_mut(|o| o.ime = Some(egui::output::IMEOutput { purpose: Default::default(), rect, cursor_rect, should_interrupt_composition: false }));
        }

        // Wheel scrolling (the cursor stays where it is).
        let max_top = self.lines.len().saturating_sub(rows / 2).max(1) as f64 - 1.0;
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta != Vec2::ZERO {
                self.top = (self.top - (delta.y / row_h) as f64).clamp(0.0, max_top.max(0.0));
                self.scroll_x = (self.scroll_x - delta.x).max(0.0);
            }
        }

        // Mouse: click, shift+click, drag, double click (word), triple click (line).
        let to_pos = |s: &Self, p: Pos2| -> Pos {
            let line = (s.top + ((p.y - text_rect.min.y - pad) / row_h) as f64).floor().max(0.0) as usize;
            let line = line.min(s.lines.len() - 1);
            let chars = ((p.x - text_rect.min.x - 4.0 + s.scroll_x) / char_w).round().max(0.0) as usize;
            Pos { line, col: s.byte_col(line, chars) }
        };
        if let Some(pointer) = response.interact_pointer_pos() {
            let p = to_pos(self, pointer);
            if response.triple_clicked() {
                let next = if p.line + 1 < self.lines.len() { Pos { line: p.line + 1, col: 0 } } else { Pos { line: p.line, col: self.lines[p.line].len() } };
                self.select(Pos { line: p.line, col: 0 }, next);
            } else if response.double_clicked() {
                let (a, b) = self.word_at(p);
                self.select(a, b);
            } else if response.drag_started() || response.clicked() {
                let shift = ui.input(|i| i.modifiers.shift);
                self.wanted_col = None;
                self.set_cursor(p, shift);
                self.dragging = response.drag_started();
            } else if response.dragged() && self.dragging {
                self.cursor = p;
                // Past the edges: scroll.
                if pointer.y < text_rect.min.y + pad {
                    self.top = (self.top - 1.0).max(0.0);
                } else if pointer.y > text_rect.max.y - row_h {
                    self.top = (self.top + 1.0).min(max_top.max(0.0));
                }
                ui.ctx().request_repaint();
            }
        }
        if response.drag_stopped() {
            self.dragging = false;
        }

        // Keep the cursor in view after keys and jumps.
        if self.reveal {
            self.reveal = false;
            let line = self.cursor.line as f64;
            let visible = rows as f64 - 1.0;
            if line < self.top {
                self.top = (line - (visible / 3.0).floor()).max(0.0).min(line);
            } else if line > self.top + visible - 1.0 {
                self.top = (line - visible + 1.0).max(0.0);
            }
            let x = self.char_col(self.cursor) as f32 * char_w;
            let width = text_rect.width() - 16.0;
            if x < self.scroll_x {
                self.scroll_x = (x - width / 3.0).max(0.0);
            } else if x > self.scroll_x + width {
                self.scroll_x = x - width * 2.0 / 3.0;
            }
        }
        self.top = self.top.clamp(0.0, max_top.max(0.0));

        // Painting.
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.bg);
        let gutter_rect = Rect::from_min_max(rect.min, Pos2::new(rect.min.x + gutter - 6.0, rect.max.y));
        painter.rect_filled(gutter_rect, 0.0, theme.chrome_bg.gamma_multiply(0.6));
        let text_painter = ui.painter_at(text_rect);
        let first = self.top.floor() as usize;
        let offset = ((self.top - first as f64) as f32) * row_h;
        let (sa, sb) = self.sorted();
        let selection = theme.selection;
        for (k, line_no) in (first..(first + rows + 2).min(self.lines.len())).enumerate() {
            let y = text_rect.min.y + pad + k as f32 * row_h - offset;
            let text = &self.lines[line_no];
            let x0 = text_rect.min.x + 4.0 - self.scroll_x;
            let x_of = |col: usize| x0 + text[..col].chars().count() as f32 * char_w;
            // Selection.
            if sa != sb && line_no >= sa.line && line_no <= sb.line {
                let from = if line_no == sa.line { x_of(sa.col) } else { x0 };
                let to = if line_no == sb.line { x_of(sb.col) } else { x_of(text.len()) + char_w * 0.6 };
                text_painter.rect_filled(Rect::from_min_max(Pos2::new(from, y), Pos2::new(to, y + row_h)), 0.0, selection);
            }
            // Matches on this line.
            if let Some((query, case)) = find.filter(|(q, _)| !q.is_empty()) {
                let found: Vec<usize> = if case {
                    text.match_indices(query).map(|(i, _)| i).collect()
                } else if text.is_ascii() && query.is_ascii() {
                    text.to_ascii_lowercase().match_indices(&query.to_ascii_lowercase()).map(|(i, _)| i).collect()
                } else {
                    let lower = text.to_lowercase();
                    if lower.len() == text.len() { lower.match_indices(&query.to_lowercase()).map(|(i, _)| i).collect() } else { Vec::new() }
                };
                for at in found.into_iter().take(200) {
                    let end = (at + query.len()).min(text.len());
                    if !text.is_char_boundary(end) {
                        continue;
                    }
                    let r = Rect::from_min_max(Pos2::new(x_of(at), y), Pos2::new(x_of(end), y + row_h));
                    let current = sa == (Pos { line: line_no, col: at }) && sb == (Pos { line: line_no, col: end });
                    text_painter.rect_filled(r, 2.0, theme.accent.gamma_multiply(if current { 0.45 } else { 0.2 }));
                    if current {
                        text_painter.rect_stroke(r, 2.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Outside);
                    }
                }
            }
            // The text, colored; very long lines only around what is on screen.
            let visible_chars = ((text_rect.width() / char_w) as usize) + 2;
            let skip = (self.scroll_x / char_w) as usize;
            let (shown, x) = if text.len() > 4000 {
                let start = text.char_indices().nth(skip).map_or(text.len(), |(i, _)| i);
                let end = text[start..].char_indices().nth(visible_chars).map_or(text.len(), |(i, _)| start + i);
                (&text[start..end], x0 + skip as f32 * char_w)
            } else {
                (text.as_str(), x0)
            };
            let galley = ui.painter().layout_job(highlight(shown, syntax, theme, &font));
            text_painter.galley(Pos2::new(x, y + (row_h - galley.size().y) / 2.0), galley, theme.text);
            // Line number.
            let here = line_no == self.cursor.line;
            painter.text(Pos2::new(gutter_rect.max.x - 8.0, y + row_h / 2.0), Align2::RIGHT_CENTER, (line_no + 1).to_string(), font.clone(), if here { theme.text } else { theme.text_muted.gamma_multiply(0.7) });
        }
        // Cursor.
        if focused {
            let r = self.cursor_rect(text_rect, row_h, char_w, pad);
            if text_rect.intersects(r) {
                text_painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(2.0, row_h)), 0.0, theme.cursor);
            }
        }

        // Scroll bar (drag to move through the file).
        let bar = Rect::from_min_max(Pos2::new(rect.max.x - bar_w, rect.min.y), rect.max);
        let total = (self.lines.len() as f64).max(1.0);
        let thumb_h = ((rows as f64 / total) as f32 * bar.height()).clamp(24.0, bar.height());
        let thumb_y = bar.min.y + (self.top / max_top.max(1.0)) as f32 * (bar.height() - thumb_h).max(0.0);
        let thumb = Rect::from_min_size(Pos2::new(bar.min.x + 2.0, thumb_y.min(bar.max.y - thumb_h)), Vec2::new(bar_w - 4.0, thumb_h));
        let bar_resp = ui.interact(bar, id.with("bar"), Sense::click_and_drag());
        if let Some(p) = bar_resp.interact_pointer_pos().filter(|_| bar_resp.dragged() || bar_resp.clicked()) {
            let fraction = ((p.y - bar.min.y - thumb_h / 2.0) / (bar.height() - thumb_h).max(1.0)).clamp(0.0, 1.0) as f64;
            self.top = (fraction * max_top.max(0.0)).round();
        }
        if total > rows as f64 {
            painter.rect_filled(thumb, 4.0, if bar_resp.hovered() || bar_resp.dragged() { theme.text_muted.gamma_multiply(0.8) } else { theme.text_muted.gamma_multiply(0.4) });
        }
        self.revision != revision
    }

    fn cursor_rect(&self, text_rect: Rect, row_h: f32, char_w: f32, pad: f32) -> Rect {
        let y = text_rect.min.y + pad + ((self.cursor.line as f64 - self.top) as f32) * row_h;
        let x = text_rect.min.x + 4.0 - self.scroll_x + self.char_col(self.cursor) as f32 * char_w;
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(char_w, row_h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_undoes_and_redoes() {
        let mut t = BigText::new("hello\nworld");
        t.set_cursor(Pos { line: 0, col: 5 }, false);
        t.insert(",\nbig", false);
        assert_eq!(t.lines, ["hello,", "big", "world"]);
        assert_eq!(t.cursor, Pos { line: 1, col: 3 });
        t.select(Pos { line: 0, col: 3 }, Pos { line: 2, col: 2 });
        t.insert("", false);
        assert_eq!(t.lines, ["helrld"]);
        t.undo_redo(true);
        assert_eq!(t.lines, ["hello,", "big", "world"]);
        t.undo_redo(true);
        assert_eq!(t.lines, ["hello", "world"]);
        t.undo_redo(false);
        t.undo_redo(false);
        assert_eq!(t.lines, ["helrld"]);
        assert_eq!(String::from_utf8(t.to_bytes("\r\n")).unwrap(), "helrld");
    }

    #[test]
    fn finds_and_replaces() {
        let mut t = BigText::new("un Été\nété, ÉTÉ\nrien");
        let (a, b) = t.find("été", false, true).unwrap();
        assert_eq!((a, b), (Pos { line: 0, col: 3 }, Pos { line: 0, col: 8 }));
        t.select(a, b);
        let (a, _) = t.find("été", false, true).unwrap();
        assert_eq!(a, Pos { line: 1, col: 0 });
        // Backward from the start wraps to the last one.
        t.set_cursor(Pos::default(), false);
        let (a, _) = t.find("été", false, false).unwrap();
        assert_eq!(a.line, 1);
        assert_eq!(t.find("été", true, true).unwrap().0, Pos { line: 1, col: 0 });
        let mut t = BigText::new("a.b.c\nxx.yy");
        assert_eq!(t.replace_all(".", "::", true), 3);
        assert_eq!(t.lines, ["a::b::c", "xx::yy"]);
        t.undo_redo(true);
        assert_eq!(t.lines, ["a.b.c", "xx.yy"]);
    }

    #[test]
    fn indents_and_comments_lines() {
        let mut t = BigText::new("a\n  b\nc");
        t.select(Pos { line: 0, col: 0 }, Pos { line: 1, col: 1 });
        t.indent("  ", false);
        assert_eq!(t.lines, ["  a", "    b", "c"]);
        t.indent("  ", true);
        assert_eq!(t.lines, ["a", "  b", "c"]);
        t.toggle_comment("#");
        assert_eq!(t.lines, ["# a", "#   b", "c"]);
        t.toggle_comment("#");
        assert_eq!(t.lines, ["a", "  b", "c"]);
    }

    #[test]
    fn moves_by_words() {
        let t = BigText::new("foo_bar  baz(qux)");
        assert_eq!(t.word(Pos { line: 0, col: 0 }, true).col, 7);
        assert_eq!(t.word(Pos { line: 0, col: 7 }, true).col, 12);
        assert_eq!(t.word(Pos { line: 0, col: 12 }, false).col, 9);
        assert_eq!(t.word(Pos { line: 0, col: 9 }, false).col, 0);
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// cargo test --release big_file_speed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn big_file_speed() {
        let line = "2026-09-28 10:33:12 INFO  request handled path=/api/v1/items id=12345 took=12ms\n";
        let text = line.repeat(150 * 1024 * 1024 / line.len());
        let t0 = Instant::now();
        let mut t = BigText::new(&text);
        let load = t0.elapsed();
        let t0 = Instant::now();
        t.set_cursor(Pos { line: 1_000_000, col: 10 }, false);
        for _ in 0..1000 {
            t.insert("x", true);
        }
        let typing = t0.elapsed();
        let t0 = Instant::now();
        let found = t.find("zzz-not-there", false, true);
        let search = t0.elapsed();
        let t0 = Instant::now();
        let bytes = t.to_bytes("\n");
        let save = t0.elapsed();
        println!("{} lines, load {load:?}, 1000 keys {typing:?}, full search {search:?} ({found:?}), save {save:?} ({} MB)", t.lines.len(), bytes.len() >> 20);
    }
}
