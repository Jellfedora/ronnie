//! The VS Code layout of the terminals (Settings > General > Sidebar): the local profiles in a bar on
//! top; in the sidebar, the files of the tab shown from its folder (the tree of the "Files" category) or
//! its git changes; on the right its terminals, a file opened from the tree, or what is picked in git,
//! each behind a tab of a strip.

use super::editor::{Editor, EditorAction};
use super::sidebar::paint_badge;
use super::*;

/// Height of the profiles' bar, and of the strip of the tab's files.
pub(super) const BAR_H: f32 = 40.0;
pub(super) const DOCS_H: f32 = 32.0;
/// Longest name shown on a chip of the bar.
const CHIP_TEXT_W: f32 = 150.0;

/// Something of the profiles' bar.
#[derive(Clone, Copy)]
enum Chip {
    /// A local tab that is no profile.
    Tab(usize),
    Profile(Uuid),
    /// A group's name: a click folds it.
    Group(usize),
}

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
        if !self.config.settings.vscode_layout {
            return None;
        }
        self.shown_tab().filter(|&i| self.tabs[i].ssh.is_none() && self.tabs[i].db.is_none())
    }

    /// The profiles' bar shows: the VS Code layout, on a local tab or the home page (not in a game).
    pub(super) fn ws_bar_shown(&self) -> bool {
        self.config.settings.vscode_layout && !self.notes_page && !self.music_page && !(self.home.is_some() && self.home_game) && (self.ws_tab().is_some() || self.shown_tab().is_none())
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

    /// The local profiles in a bar: the tabs that are none first, then the profiles by group (a folded
    /// group shows only its open ones); "+" at the end opens a terminal.
    pub(super) fn profiles_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, theme.chrome_bg);
        painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, theme.tab_hover));
        // Its empty space drags the window (the native title bar is hidden).
        let bg = ui.interact(rect, ui.id().with("ws-bar-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }

        let is_profile = |id: &Uuid| self.config.profiles.iter().any(|p| p.id == *id);
        let mut chips: Vec<Chip> = (0..self.tabs.len()).filter(|&i| self.tabs[i].ssh.is_none() && self.tabs[i].db.is_none() && self.tabs[i].profile.is_none()).map(Chip::Tab).collect();
        chips.extend(self.config.ungrouped.iter().filter(|id| is_profile(id)).map(|id| Chip::Profile(*id)));
        for (gi, g) in self.config.groups.iter().enumerate().filter(|(_, g)| g.local) {
            chips.push(Chip::Group(gi));
            let open = |id: &Uuid| self.tabs.iter().any(|t| t.profile == Some(*id));
            chips.extend(g.items.iter().filter(|id| is_profile(id) && (!g.collapsed || open(id))).map(|id| Chip::Profile(*id)));
        }

        // Each chip's width, its name laid out.
        let font = FontId::proportional(13.0);
        let shown = self.shown_tab();
        let mut laid: Vec<(Chip, f32, String, bool)> = Vec::with_capacity(chips.len());
        for chip in chips {
            let (name, open) = match chip {
                Chip::Tab(i) => (self.tabs[i].title().to_owned(), true),
                Chip::Profile(id) => match self.item(id) {
                    Some(item) => (item.name, item.open.is_some()),
                    None => continue,
                },
                Chip::Group(gi) => (self.config.groups[gi].name.to_uppercase(), false),
            };
            let w = match chip {
                Chip::Group(_) => painter.layout_no_wrap(name.clone(), FontId::proportional(10.5), theme.text_muted).size().x + 30.0,
                _ => {
                    let renaming = match chip {
                        Chip::Tab(i) => self.rename.as_ref().is_some_and(|r| r.tab == i),
                        Chip::Profile(id) => self.item_rename.as_ref().is_some_and(|r| r.id == id),
                        Chip::Group(_) => false,
                    };
                    let text_w = if renaming { CHIP_TEXT_W } else { painter.layout_no_wrap(name.clone(), font.clone(), theme.text).size().x.min(CHIP_TEXT_W) };
                    10.0 + 20.0 + 8.0 + text_w + if open { 28.0 } else { 12.0 }
                }
            };
            laid.push((chip, w, name, open));
        }

        // The chips scroll sideways (wheel or trackpad) when they don't fit; "+" stays on the right.
        let plus_rect = Rect::from_center_size(Pos2::new(rect.max.x - 22.0, rect.center().y), Vec2::splat(24.0));
        let area = Rect::from_min_max(Pos2::new(rect.min.x + 8.0, rect.min.y), Pos2::new(plus_rect.min.x - 6.0, rect.max.y));
        let total: f32 = laid.iter().map(|(_, w, _, _)| w + 4.0).sum();
        let max_scroll = (total - area.width()).max(0.0);
        let scroll_id = ui.id().with("ws-bar-scroll");
        let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
        if ui.rect_contains_pointer(area) {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            scroll -= delta.x + delta.y;
        }
        // The tab shown, brought into sight when it changes.
        let seen_id = ui.id().with("ws-bar-seen");
        let seen: Option<usize> = ui.data(|d| d.get_temp(seen_id)).flatten();
        if seen != shown {
            let mut x = 0.0;
            for (chip, w, _, _) in &laid {
                let is_shown = match *chip {
                    Chip::Tab(i) => Some(i) == shown,
                    Chip::Profile(id) => shown.is_some_and(|s| self.tabs[s].profile == Some(id)),
                    Chip::Group(_) => false,
                };
                if is_shown {
                    if x < scroll {
                        scroll = x;
                    } else if x + w > scroll + area.width() {
                        scroll = x + w - area.width();
                    }
                }
                x += w + 4.0;
            }
            ui.data_mut(|d| d.insert_temp(seen_id, shown));
        }
        let scroll = scroll.clamp(0.0, max_scroll);
        ui.data_mut(|d| d.insert_temp(scroll_id, scroll));

        let mut action: Option<TabAction> = None;
        let chip_painter = painter.with_clip_rect(area);
        let mut x = area.min.x - scroll;
        for (chip, w, name, open) in laid {
            let slot = Rect::from_min_size(Pos2::new(x, rect.min.y + 6.0), Vec2::new(w, rect.height() - 12.0));
            x += w + 4.0;
            if slot.max.x < area.min.x || slot.min.x > area.max.x {
                continue;
            }
            let hit = slot.intersect(area);
            match chip {
                Chip::Group(gi) => {
                    let gid = self.config.groups[gi].id;
                    let collapsed = self.config.groups[gi].collapsed;
                    let resp = ui.interact(hit, ui.id().with(("ws-group", gid)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let color = if resp.hovered() { theme.text } else { theme.text_muted };
                    chip_painter.vline(slot.min.x + 2.0, slot.y_range().shrink(4.0), Stroke::new(1.0, theme.tab_hover));
                    paint_chevron(&chip_painter, Pos2::new(slot.min.x + 14.0, slot.center().y), !collapsed, color);
                    let mut job = egui::text::LayoutJob::default();
                    job.append(&name, 0.0, egui::TextFormat { font_id: FontId::proportional(10.5), color, extra_letter_spacing: 1.0, ..Default::default() });
                    let galley = chip_painter.layout_job(job);
                    chip_painter.galley(Pos2::new(slot.min.x + 24.0, slot.center().y - galley.size().y / 2.0), galley, color);
                    // A click lists all its profiles (a folded group shows only its open ones in the bar).
                    let items: Vec<Uuid> = self.config.groups[gi].items.iter().copied().filter(|id| self.config.profiles.iter().any(|p| p.id == *id)).collect();
                    egui::Popup::menu(&resp).width(220.0).show(|ui| {
                        for id in items {
                            let Some(item) = self.item(id) else { continue };
                            let label = if item.open.is_some() { format!("●  {}", item.name) } else { format!("    {}", item.name) };
                            menu_item(ui, &label, TabAction::OpenItem(id), &mut action);
                        }
                        ui.separator();
                        menu_item(ui, if collapsed { t.ws_group_all } else { t.ws_group_open_only }, TabAction::ToggleGroup(gid), &mut action);
                    });
                }
                Chip::Tab(_) | Chip::Profile(_) => {
                    let (index, id) = match chip {
                        Chip::Tab(i) => (Some(i), None),
                        Chip::Profile(id) => (self.tabs.iter().position(|t| t.profile == Some(id)), Some(id)),
                        Chip::Group(_) => unreachable!(),
                    };
                    let active = index.is_some() && index == shown;
                    let salt = match (index, id) {
                        (_, Some(id)) => egui::Id::new(("ws-chip", id)),
                        (Some(i), None) => egui::Id::new(("ws-tab", i)),
                        _ => egui::Id::new("ws-none"),
                    };
                    let resp = ui.interact(hit, salt, Sense::click());
                    let hovered = resp.contains_pointer();
                    if active {
                        chip_painter.rect_filled(slot, 7.0, theme.tab_active);
                        chip_painter.hline(slot.x_range().shrink(8.0), slot.max.y - 1.0, Stroke::new(2.0, theme.accent));
                    } else if hovered {
                        chip_painter.rect_filled(slot, 7.0, theme.tab_hover);
                    }
                    let color = index.map(|i| self.tabs[i].color).unwrap_or_else(|| self.config.profiles.iter().find(|p| Some(p.id) == id).and_then(|p| p.tab.color));
                    let dot = Pos2::new(slot.min.x + 20.0, slot.center().y);
                    paint_badge(&chip_painter, dot, &name, color, false, open, active, &theme);
                    if index.and_then(|i| self.live.get(i)).is_some_and(Option::is_some) {
                        paint_live(ui, &chip_painter, dot, false, &theme);
                    }
                    let close_rect = Rect::from_center_size(Pos2::new(slot.max.x - 14.0, slot.center().y), Vec2::splat(16.0));
                    let text_rect = Rect::from_min_max(Pos2::new(slot.min.x + 38.0, slot.min.y), Pos2::new(if open { close_rect.min.x - 4.0 } else { slot.max.x - 8.0 }, slot.max.y));
                    let renamed = self.chip_rename(ui, index, id, text_rect.intersect(area), &mut action);
                    if !renamed {
                        let text_color = if active || open { theme.text } else { theme.text_muted.gamma_multiply(0.85) };
                        let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), font.clone(), text_color);
                        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
                        let galley = chip_painter.layout_job(job);
                        chip_painter.with_clip_rect(text_rect.intersect(area)).galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, text_color);
                    }
                    // Open: ✕ on hover (or shown); a long command ended in it: ✓ or ✗ there meanwhile.
                    if let Some(i) = index {
                        if (hovered || active) && !renamed && area.contains_rect(close_rect) {
                            let close = ui.interact(close_rect, salt.with("close"), Sense::click());
                            if close.hovered() {
                                chip_painter.rect_filled(close_rect, 4.0, theme.tab_hover.gamma_multiply(1.8));
                            }
                            paint_cross(&chip_painter, close_rect.center(), if close.hovered() { theme.text } else { theme.text_muted });
                            if close.clicked() {
                                action = Some(TabAction::Close(i));
                            }
                        } else if let Some(done) = &self.tabs[i].done {
                            chip_painter.circle_filled(close_rect.center(), 3.0, if done.ok { theme.ansi[2] } else { theme.ansi[1] });
                        }
                    }
                    if renamed {
                        continue;
                    }
                    if resp.clicked() {
                        action.get_or_insert(match (index, id) {
                            (_, Some(id)) => TabAction::OpenItem(id),
                            (Some(i), None) => TabAction::Select(i),
                            _ => continue,
                        });
                    }
                    if resp.double_clicked() {
                        action = Some(match (index, id) {
                            (_, Some(id)) => TabAction::StartItemRename(id),
                            (Some(i), None) => TabAction::StartRename(i),
                            _ => continue,
                        });
                    }
                    if let Some(i) = index.filter(|_| resp.middle_clicked()) {
                        action = Some(TabAction::Close(i));
                    }
                    let hint: Vec<String> = [index.and_then(|i| self.live.get(i).cloned().flatten()).map(|p| format!("▶ {p}")), index.and_then(|i| self.tabs[i].done.as_ref()).map(|d| d.summary.clone())].into_iter().flatten().collect();
                    let resp = if hint.is_empty() { resp } else { resp.on_hover_text(hint.join("\n")) };
                    match (index, id) {
                        (_, Some(id)) => {
                            if let Some(item) = self.item(id) {
                                resp.context_menu(|ui| self.item_menu(ui, id, &item, &mut action));
                            }
                        }
                        (Some(i), None) => {
                            resp.context_menu(|ui| self.tab_menu(ui, i, &mut action));
                        }
                        _ => {}
                    }
                }
            }
        }
        // Faded edges where chips are cut.
        if scroll > 0.5 {
            fade_edge(&painter, Rect::from_min_size(area.min, Vec2::new(24.0, area.height())), theme.chrome_bg, true);
        }
        if scroll < max_scroll - 0.5 {
            fade_edge(&painter, Rect::from_min_max(Pos2::new(area.max.x - 24.0, area.min.y), area.max), theme.chrome_bg, false);
        }
        let new_hint = format!("{} ({})", t.new_tab, self.config.settings.shortcuts.new_tab.label());
        if icon_button(ui, &painter, plus_rect, "ws-new-tab", &theme, paint_plus).on_hover_text(new_hint).clicked() {
            action = Some(TabAction::New);
        }
        self.apply_tab_action(ui, action, &[]);
    }

    /// The name of a chip being typed (a tab or a profile renamed): true when it is.
    fn chip_rename(&mut self, ui: &mut Ui, index: Option<usize>, id: Option<Uuid>, rect: Rect, action: &mut Option<TabAction>) -> bool {
        let theme = &self.theme;
        let color = |error: bool| if error { theme.ansi[1] } else { theme.text };
        if let Some(id) = id {
            let Some(r) = self.item_rename.as_mut().filter(|r| r.id == id) else { return false };
            let edit = ui.put(rect.shrink2(Vec2::new(0.0, 5.0)), rename_field(&mut r.text, color(r.error.is_some())));
            if r.fresh {
                edit.request_focus();
                select_all(ui, edit.id, &r.text);
                r.fresh = false;
            }
            if edit.changed() {
                r.error = None;
            }
            if let Some(err) = r.error {
                edit.request_focus();
                edit.show_tooltip_text(egui::RichText::new(err).color(theme.ansi[1]));
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.item_rename = None;
            } else if enter {
                *action = Some(TabAction::RenameItem(id, r.text.clone(), true));
            } else if edit.lost_focus() {
                *action = Some(TabAction::RenameItem(id, r.text.clone(), false));
            }
            return true;
        }
        let Some(i) = index else { return false };
        let Some(r) = self.rename.as_mut().filter(|r| r.tab == i) else { return false };
        let edit = ui.put(rect.shrink2(Vec2::new(0.0, 5.0)), rename_field(&mut r.text, color(r.error.is_some())));
        if edit.changed() {
            r.error = None;
        }
        if let Some(err) = r.error {
            edit.request_focus();
            edit.show_tooltip_text(egui::RichText::new(err).color(theme.ansi[1]));
        }
        if r.select_all > 0 {
            edit.request_focus();
            select_all(ui, edit.id, &r.text);
            if !ui.input(|inp| inp.pointer.any_down()) {
                r.select_all -= 1;
            }
            ui.ctx().request_repaint();
        }
        let (enter, escape) = ui.input(|inp| (inp.key_pressed(Key::Enter), inp.key_pressed(Key::Escape)));
        if escape {
            self.rename = None;
            self.focus_terminal = true;
        } else if enter {
            *action = Some(TabAction::Rename(i, true));
        } else if edit.lost_focus() {
            *action = Some(TabAction::Rename(i, false));
        }
        true
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
                painter.hline(r.x_range(), r.min.y + 1.0, Stroke::new(2.0, theme.accent));
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
                let mut editor = Box::new(Editor::opening(false, key.clone(), name));
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

/// The field of a name being typed on a chip.
fn rename_field(text: &mut String, color: Color32) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).font(FontId::proportional(13.0)).frame(Frame::NONE).text_color(color)
}

/// A band fading from `color` to nothing, over the cut edge of a row of chips (`left`: on the left).
fn fade_edge(painter: &egui::Painter, rect: Rect, color: Color32, left: bool) {
    let (a, b) = if left { (color, Color32::TRANSPARENT) } else { (Color32::TRANSPARENT, color) };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), b);
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.colored_vertex(rect.left_bottom(), a);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}
