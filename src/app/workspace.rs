//! The terminals laid out as in VS Code: the local profiles in a bar on top (see profilebar.rs); in the
//! sidebar, the files of the tab shown as a tree from its folder, or its git changes; on the right its
//! terminals, a file opened from the tree, or what is picked in git, each behind a tab of a strip.

use super::editor::{Editor, EditorAction};
use super::*;

/// Height of the strip of the tab's files.
pub(super) const DOCS_H: f32 = 32.0;

/// What was clicked in the strip of the tab's files.
enum DocAction {
    Terminals,
    Show(usize),
    Close(usize),
    Diff,
    CloseDiff,
}

impl App {
    /// The local tab shown, in the VS Code layout.
    pub(super) fn ws_tab(&self) -> Option<usize> {
        self.shown_tab().filter(|&i| self.tabs[i].ssh.is_none() && self.tabs[i].db.is_none())
    }

    /// The profiles' bar shows: the VS Code layout, on a local tab, or on the home page while the
    /// sidebar shows the terminals (not the SSH, the databases… nor in a game).
    pub(super) fn ws_bar_shown(&self) -> bool {
        let home = self.shown_tab().is_none() && self.category() == Category::Local;
        !self.notes_page && !self.music_page && !self.dev_page && !self.modules_page && !(self.home.is_some() && self.home_game) && (self.ws_tab().is_some() || home)
    }

    /// The folder tab `i`'s tree starts from: the one picked, else its first pane's.
    pub(super) fn ws_root(&self, i: usize) -> Option<PathBuf> {
        let tab = self.tabs.get(i)?;
        if let Some(root) = tab.root.as_ref().filter(|r| r.is_dir()) {
            return Some(root.clone());
        }
        let first = tab.layout.first_leaf();
        tab.cwds.get(&first).cloned().or_else(|| tab.panes.get(&first).and_then(Terminal::cwd)).filter(|d| d.is_dir())
    }

    /// The sidebar's panel for the terminals: the files of the tab shown, or its git changes.
    pub(super) fn ws_panel(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let tab = self.ws_tab();
        // Its folder is fixed the first time it shows (then changed from the tree's menu).
        if let Some(i) = tab
            && self.tabs[i].root.is_none()
        {
            self.tabs[i].root = self.ws_root(i);
        }

        // Files or git, side by side.
        let seg = Rect::from_min_size(Pos2::new(left + 4.0, *y + 2.0), Vec2::new(row_w - 8.0, 28.0));
        ui.painter().rect_filled(seg, 7.0, theme.bg.gamma_multiply(0.6));
        for (k, label) in [t.cat_files, t.ws_git].into_iter().enumerate() {
            let git = k == 1;
            let cell = Rect::from_min_size(Pos2::new(seg.min.x + seg.width() / 2.0 * k as f32, seg.min.y), Vec2::new(seg.width() / 2.0, seg.height())).shrink(2.0);
            let resp = ui.interact(cell, ui.id().with(("ws-seg", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let on = self.ws_git == git;
            if on || resp.hovered() {
                ui.painter().rect_filled(cell, 6.0, if on { theme.tab_active } else { theme.tab_hover });
            }
            let color = if on { theme.text } else { theme.text_muted };
            let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(12.5), color);
            let icon_w = 18.0;
            let start = cell.center().x - (galley.size().x + icon_w) / 2.0;
            let icon = Rect::from_center_size(Pos2::new(start + 6.0, cell.center().y), Vec2::splat(13.0));
            if git {
                git::paint_branch_icon(ui.painter(), icon, if on { theme.accent } else { color });
            } else {
                files::paint_file_icon(ui.painter(), icon, "", true, false, None, &theme);
            }
            ui.painter().galley(Pos2::new(start + icon_w, cell.center().y - galley.size().y / 2.0), galley, color);
            if resp.clicked() {
                self.ws_git = git;
            }
        }
        *y = seg.max.y + 8.0;

        if !self.ws_git {
            self.files_section(ui, left, row_w, y);
            return;
        }
        let hint = |ui: &Ui, y: &mut f32, text: &str| {
            let galley = ui.painter().layout(text.to_owned(), FontId::proportional(12.5), theme.text_muted, row_w - 16.0);
            let h = galley.size().y;
            ui.painter().galley(Pos2::new(left + 8.0, *y + 6.0), galley, theme.text_muted);
            *y += h + 12.0;
        };
        let Some(i) = tab else { return hint(ui, y, t.ws_no_tab) };
        let Some(root) = self.ws_root(i).and_then(|r| git::git_root(&r)) else { return hint(ui, y, t.ws_no_repo) };
        let tab = &mut self.tabs[i];
        if tab.git_side.as_ref().is_none_or(|g| g.root() != root) {
            tab.git_side = Some(Box::new(git::GitView::open(root)));
            tab.show_diff = false;
        }
        let Some(view) = tab.git_side.as_mut() else { return };
        // Down to the bottom of what the panel shows: its lists scroll by themselves.
        let rect = Rect::from_min_max(Pos2::new(left, *y), Pos2::new(left + row_w, ui.clip_rect().max.y.max(*y + 200.0) - 6.0));
        let before = view.picked();
        view.side_ui(ui, rect, &theme, t);
        let after = view.picked();
        // Something picked: shown on the right, in place of the terminals.
        if after.is_none() {
            tab.show_diff = false;
        } else if after != before {
            tab.show_diff = true;
            tab.doc = None;
        }
        *y = rect.max.y;
    }

    /// The strip above the right side shows: there is more than the terminals.
    pub(super) fn docs_shown(&self, i: usize) -> bool {
        let tab = &self.tabs[i];
        !tab.docs.is_empty() || tab.git_side.as_ref().is_some_and(|g| g.picked().is_some())
    }

    /// The strip above the right side: the terminals, the files opened, what git shows.
    pub(super) fn docs_strip(&mut self, ui: &mut Ui, rect: Rect, i: usize) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, theme.chrome_bg);
        painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, theme.tab_hover));
        let tab = &self.tabs[i];
        let font = FontId::proportional(12.5);
        let mut action = None;
        let mut x = rect.min.x;
        // One cell: its label, whether it is shown, a ✕ (and whether the file has unsaved changes).
        let cell = |ui: &mut Ui, x: &mut f32, key: egui::Id, label: &str, icon: u8, on: bool, close: Option<bool>, tip: Option<&str>| -> (bool, bool) {
            let galley = painter.layout_no_wrap(label.to_owned(), font.clone(), theme.text);
            let w = 12.0 + 18.0 + galley.size().x.min(220.0) + if close.is_some() { 30.0 } else { 14.0 };
            let r = Rect::from_min_size(Pos2::new(*x, rect.min.y), Vec2::new(w, rect.height() - 1.0));
            *x += w;
            if r.min.x > rect.max.x {
                return (false, false);
            }
            let resp = ui.interact(r.intersect(rect), key, Sense::click());
            let resp = match tip {
                Some(tip) => resp.on_hover_text(tip),
                None => resp,
            };
            let hovered = resp.contains_pointer();
            if on {
                painter.rect_filled(r, 0.0, theme.bg);
                if theme.jurassic {
                    super::jurassic::hazard(&painter, Rect::from_min_size(r.min, Vec2::new(r.width(), 3.0)));
                } else {
                    painter.hline(r.x_range(), r.min.y + 1.0, Stroke::new(2.0, theme.accent));
                }
            } else if hovered {
                painter.rect_filled(r, 0.0, theme.tab_hover.gamma_multiply(0.6));
            }
            painter.vline(r.max.x - 0.5, r.y_range(), Stroke::new(1.0, theme.tab_hover));
            let color = if on { theme.text } else { theme.text_muted };
            let icon_rect = Rect::from_center_size(Pos2::new(r.min.x + 18.0, r.center().y), Vec2::splat(13.0));
            match icon {
                0 => {
                    painter.text(icon_rect.center(), Align2::CENTER_CENTER, ">_", FontId::monospace(11.0), if on { theme.accent } else { color });
                }
                1 => files::paint_file_icon(&painter, icon_rect, label, false, false, None, &theme),
                _ => git::paint_branch_icon(&painter, icon_rect, if on { theme.accent } else { color }),
            }
            let text_rect = Rect::from_min_max(Pos2::new(r.min.x + 30.0, r.min.y), Pos2::new(r.max.x - if close.is_some() { 26.0 } else { 10.0 }, r.max.y));
            let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), font.clone(), color);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(text_rect.min.x, r.center().y - galley.size().y / 2.0), galley, color);
            let mut closed = false;
            if let Some(dirty) = close {
                let close_rect = Rect::from_center_size(Pos2::new(r.max.x - 15.0, r.center().y), Vec2::splat(16.0));
                // Unsaved: a dot, turning into ✕ under the pointer.
                if dirty && !hovered {
                    painter.circle_filled(close_rect.center(), 4.0, theme.text);
                } else if hovered || on {
                    let c = ui.interact(close_rect, key.with("close"), Sense::click());
                    if c.hovered() {
                        painter.rect_filled(close_rect, 4.0, theme.tab_hover.gamma_multiply(1.8));
                    }
                    paint_cross(&painter, close_rect.center(), if c.hovered() { theme.text } else { theme.text_muted });
                    closed = c.clicked();
                }
                closed |= resp.middle_clicked();
            }
            (resp.clicked() && !closed, closed)
        };
        let terminals = tab.doc.is_none() && !tab.show_diff;
        if cell(ui, &mut x, ui.id().with("ws-doc-terms"), t.ws_terminals, 0, terminals, None, None).0 {
            action = Some(DocAction::Terminals);
        }
        for (k, doc) in tab.docs.iter().enumerate() {
            let (clicked, closed) = cell(ui, &mut x, ui.id().with(("ws-doc", &doc.path)), &doc.name, 1, tab.doc == Some(k), Some(doc.is_dirty()), Some(&doc.path));
            if closed {
                action = Some(DocAction::Close(k));
            } else if clicked {
                action = Some(DocAction::Show(k));
            }
        }
        if let Some(picked) = tab.git_side.as_ref().and_then(|g| g.picked()) {
            let (clicked, closed) = cell(ui, &mut x, ui.id().with("ws-doc-diff"), &picked, 2, tab.show_diff, Some(false), None);
            if closed {
                action = Some(DocAction::CloseDiff);
            } else if clicked {
                action = Some(DocAction::Diff);
            }
        }
        let tab = &mut self.tabs[i];
        match action {
            None => {}
            Some(DocAction::Terminals) => {
                tab.doc = None;
                tab.show_diff = false;
                tab.show_files = false;
                self.focus_terminal = true;
            }
            Some(DocAction::Show(k)) => {
                tab.doc = Some(k);
                tab.show_diff = false;
            }
            Some(DocAction::Close(k)) => {
                // Unsaved: the editor asks first (shown for that).
                if tab.docs[k].is_dirty() {
                    tab.doc = Some(k);
                    tab.show_diff = false;
                    tab.docs[k].request_close();
                } else {
                    self.close_doc(i, k);
                }
            }
            Some(DocAction::Diff) => {
                tab.show_diff = true;
                tab.doc = None;
            }
            Some(DocAction::CloseDiff) => {
                if let Some(g) = &mut tab.git_side {
                    g.unpick();
                }
                tab.show_diff = false;
                self.focus_terminal = tab.doc.is_none();
            }
        }
    }

    /// Draws what shows in place of tab `i`'s terminals (a file, what git shows); false when they show.
    pub(super) fn ws_content(&mut self, ui: &mut Ui, rect: Rect, i: usize) -> bool {
        let t = self.t();
        let theme = self.theme.clone();
        let tab = &mut self.tabs[i];
        if let Some(k) = tab.doc {
            let Some(editor) = tab.docs.get_mut(k) else {
                tab.doc = None;
                return false;
            };
            let action = editor.ui(ui, rect, &theme, t);
            self.doc_action(i, k, action);
            return true;
        }
        if tab.show_diff {
            match &mut tab.git_side {
                Some(g) if g.picked().is_some() => {
                    g.main_ui(ui, rect, &theme, t);
                    return true;
                }
                _ => tab.show_diff = false,
            }
        }
        false
    }

    /// Opens a file of this computer in tab `i`, beside its terminals (documents of other programs open
    /// with them); already open, it is shown.
    pub(super) fn ws_open(&mut self, i: usize, path: &Path) {
        if crate::openwith::is_binary(path) {
            crate::openwith::open(path);
            return;
        }
        let t = self.t();
        let Some(tab) = self.tabs.get_mut(i) else { return };
        let key = path.display().to_string();
        let k = match tab.docs.iter().position(|d| d.path == key) {
            Some(k) => k,
            None => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| key.clone());
                let mut editor = Editor::opening(false, key.clone(), name);
                editor.loaded(files::read_local_file(&key, t), t);
                tab.docs.push(editor);
                tab.docs.len() - 1
            }
        };
        tab.doc = Some(k);
        tab.show_diff = false;
        tab.show_files = false;
    }

    /// Does what file `k` of tab `i`'s editor asked.
    fn doc_action(&mut self, i: usize, k: usize, action: EditorAction) {
        let t = self.t();
        let Some(editor) = self.tabs.get_mut(i).and_then(|tab| tab.docs.get_mut(k)) else { return };
        match action {
            EditorAction::None => {}
            EditorAction::Close => self.close_doc(i, k),
            EditorAction::Reload => {
                let result = files::read_local_file(&editor.path, t);
                editor.loaded(result, t);
            }
            EditorAction::Save { data, mtime, force } => files::save_local(editor, &data, mtime, force, t),
        }
    }

    /// Closes file `k` of tab `i`: the one beside it shows if it was shown (the terminals after the last).
    fn close_doc(&mut self, i: usize, k: usize) {
        let tab = &mut self.tabs[i];
        if k >= tab.docs.len() {
            return;
        }
        tab.docs.remove(k);
        tab.doc = match tab.doc {
            Some(d) if d == k => (!tab.docs.is_empty()).then(|| k.saturating_sub(1).min(tab.docs.len() - 1)),
            Some(d) if d > k => Some(d - 1),
            d => d,
        };
        if tab.doc.is_none() && !tab.show_diff {
            self.focus_terminal = true;
        }
    }

    /// ⌘ E in the VS Code layout: from a file (or git) back to the terminals, and from the terminals to
    /// the last file shown. False when there is none: the file manager then, as usual.
    pub(super) fn ws_toggle(&mut self, i: usize) -> bool {
        let tab = &mut self.tabs[i];
        if tab.doc.is_some() || tab.show_diff {
            tab.doc = None;
            tab.show_diff = false;
            self.focus_terminal = true;
            return true;
        }
        if tab.docs.is_empty() {
            return false;
        }
        tab.doc = Some(tab.docs.len() - 1);
        true
    }
}
