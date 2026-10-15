//! The bar of local profiles above the terminals: the tabs that are no profile first, then the profiles
//! outside groups, then each group in a capsule of its own, its name first. A profile is a pill tinted
//! with its color: filled when open, underlined when shown. Profiles are dragged to another place or
//! another group, groups by their name to another place.

use super::sidebar::paint_badge;
use super::*;

/// Height of the bar.
pub(super) const BAR_H: f32 = 46.0;
const CHIP_H: f32 = 30.0;
/// Longest name shown on a chip.
const CHIP_TEXT_W: f32 = 150.0;
/// Room inside a group's capsule around its chips, between two capsules, between two chips.
const SEG_PAD: f32 = 4.0;
const SEG_GAP: f32 = 10.0;
const CHIP_GAP: f32 = 4.0;
/// Width of a group's name being typed.
const GROUP_EDIT_W: f32 = 130.0;

#[derive(Clone, Copy, PartialEq)]
enum Chip {
    /// A local tab that is no profile.
    Tab(usize),
    Profile(Uuid),
}

/// A run of chips: the tabs that are no profile (`plain`), the profiles outside groups (no `group`), or
/// a group's (index in `config.groups`), with where it was laid out.
struct Segment {
    group: Option<usize>,
    plain: bool,
    label_w: f32,
    chips: Vec<(Chip, f32, Rect)>,
    rect: Rect,
}

/// What of the bar is dragged: a profile or a group, with the pointer's x minus its left edge, and its
/// width.
#[derive(Clone, Copy)]
pub(super) enum BarDrag {
    Profile { id: Uuid, grab: f32, w: f32 },
    Group { id: Uuid, grab: f32, w: f32 },
}

impl App {
    /// The profiles' bar, filling `rect`; "+" on its right opens a terminal.
    pub(super) fn profiles_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, theme.chrome_bg);
        painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, theme.tab_hover));
        let mut action: Option<TabAction> = None;
        // Its empty space drags the window (the native title bar is hidden); a right click there makes
        // a group.
        let bg = ui.interact(rect, ui.id().with("ws-bar-bg"), Sense::click_and_drag());
        if bg.drag_started() && self.bar_drag.is_none() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        bg.context_menu(|ui| {
            ui.set_min_width(170.0);
            menu_item(ui, t.new_tab, TabAction::New, &mut action);
            menu_item(ui, t.new_group, TabAction::NewGroup(true), &mut action);
        });

        let (pointer, pointer_down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));
        let dragging_profile = matches!(self.bar_drag, Some(BarDrag::Profile { .. }));
        let mut segments = self.bar_segments(&painter, dragging_profile);

        // Laid out from 0, then shifted by the scroll.
        let plus_rect = Rect::from_center_size(Pos2::new(rect.max.x - 22.0, rect.center().y), Vec2::splat(26.0));
        let area = Rect::from_min_max(Pos2::new(rect.min.x + 8.0, rect.min.y), Pos2::new(plus_rect.min.x - 6.0, rect.max.y));
        let cy = rect.center().y;
        let mut x = 0.0;
        for seg in &mut segments {
            let x0 = x;
            if !seg.plain {
                x += SEG_PAD;
            }
            x += seg.label_w;
            for (_, w, r) in &mut seg.chips {
                *r = Rect::from_min_size(Pos2::new(x, cy - CHIP_H / 2.0), Vec2::new(*w, CHIP_H));
                x += *w + CHIP_GAP;
            }
            if !seg.chips.is_empty() {
                x -= CHIP_GAP;
            }
            if !seg.plain {
                x += SEG_PAD;
            }
            seg.rect = Rect::from_min_max(Pos2::new(x0, cy - CHIP_H / 2.0 - SEG_PAD), Pos2::new(x.max(x0 + 24.0), cy + CHIP_H / 2.0 + SEG_PAD));
            x = seg.rect.max.x + SEG_GAP;
        }
        let total = (x - SEG_GAP).max(0.0);

        // Sideways scroll (wheel, trackpad; near an edge while dragging), keeping the tab shown in sight
        // when it changes.
        let max_scroll = (total - area.width()).max(0.0);
        let scroll_id = ui.id().with("ws-bar-scroll");
        let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
        if ui.rect_contains_pointer(area) {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            scroll -= delta.x + delta.y;
        }
        if let (Some(_), Some(p)) = (self.bar_drag, pointer) {
            if p.x < area.min.x + 36.0 {
                scroll -= 8.0;
            } else if p.x > area.max.x - 36.0 {
                scroll += 8.0;
            }
            ui.ctx().request_repaint();
        }
        let shown = self.shown_tab();
        let seen_id = ui.id().with("ws-bar-seen");
        let seen: Option<usize> = ui.data(|d| d.get_temp(seen_id)).flatten();
        if seen != shown {
            let shown_rect = segments.iter().flat_map(|s| &s.chips).find(|(c, _, _)| self.chip_tab(*c) == shown && shown.is_some()).map(|(_, _, r)| *r);
            if let Some(r) = shown_rect {
                if r.min.x < scroll {
                    scroll = r.min.x;
                } else if r.max.x > scroll + area.width() {
                    scroll = r.max.x - area.width();
                }
            }
            ui.data_mut(|d| d.insert_temp(seen_id, shown));
        }
        let scroll = scroll.clamp(0.0, max_scroll);
        ui.data_mut(|d| d.insert_temp(scroll_id, scroll));
        let shift = Vec2::new(area.min.x - scroll, 0.0);
        for seg in &mut segments {
            seg.rect = seg.rect.translate(shift);
            for (_, _, r) in &mut seg.chips {
                *r = r.translate(shift);
            }
        }

        // Where what is dragged would go: the place it is shown at, and the action doing it.
        let drop = match (self.bar_drag, pointer) {
            // Taken far from the bar: let go there, it stays where it was.
            (Some(_), Some(p)) if !rect.expand(60.0).contains(p) => None,
            (Some(drag), Some(p)) => self.bar_drop(drag, p.x, &segments),
            _ => None,
        };

        let clip = painter.with_clip_rect(area);
        for seg in &segments {
            if seg.rect.max.x < area.min.x || seg.rect.min.x > area.max.x {
                continue;
            }
            let group = seg.group.map(|gi| self.config.groups[gi].id);
            let dragged_group = matches!(self.bar_drag, Some(BarDrag::Group { id, .. }) if Some(id) == group);
            if !seg.plain {
                // The capsule; the one a profile would be dropped into, outlined.
                let target = dragging_profile && drop.as_ref().is_some_and(|(_, a)| matches!(a, TabAction::DropItem(_, g, _) if *g == group));
                clip.rect_filled(seg.rect, 11.0, theme.bg.gamma_multiply(if dragged_group { 0.25 } else { 0.55 }));
                clip.rect_stroke(seg.rect, 11.0, Stroke::new(1.0, if target { theme.accent.gamma_multiply(0.8) } else { theme.tab_hover.gamma_multiply(0.8) }), egui::StrokeKind::Inside);
            }
            if let Some(gi) = seg.group {
                let label = Rect::from_min_size(Pos2::new(seg.rect.min.x + SEG_PAD, cy - CHIP_H / 2.0), Vec2::new(seg.label_w, CHIP_H));
                self.group_label(ui, &clip, gi, label.intersect(area), label, &mut action);
            }
            for &(chip, w, r) in &seg.chips {
                if r.max.x < area.min.x || r.min.x > area.max.x {
                    continue;
                }
                self.bar_chip(ui, &clip, chip, w, r, area, &mut action);
            }
        }

        // What is dragged, under the pointer above the others, and the mark of where it would go.
        if let (Some(drag), Some(p)) = (self.bar_drag, pointer) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("ws-bar-drag"))).with_clip_rect(rect);
            if let Some((mark, _)) = &drop {
                top.vline(*mark, egui::Rangef::new(cy - CHIP_H / 2.0 - 2.0, cy + CHIP_H / 2.0 + 2.0), Stroke::new(2.5, theme.accent));
                top.circle_filled(Pos2::new(*mark, cy - CHIP_H / 2.0 - 3.0), 3.0, theme.accent);
            }
            match drag {
                BarDrag::Profile { id, grab, w } => {
                    let r = Rect::from_min_size(Pos2::new(p.x - grab, cy - CHIP_H / 2.0 - 2.0), Vec2::new(w, CHIP_H));
                    top.rect_filled(r.translate(Vec2::new(0.0, 3.0)).expand(1.0), 9.0, Color32::from_black_alpha(90));
                    top.rect_filled(r, 8.0, theme.tab_active);
                    let index = self.tabs.iter().position(|t| t.profile == Some(id));
                    let item = self.item(id);
                    let (name, color) = item.map(|i| (i.name, i.color)).unwrap_or_default();
                    self.paint_chip(ui, &top, r, &name, color, index, true, true);
                }
                BarDrag::Group { id, grab, w } => {
                    let r = Rect::from_min_size(Pos2::new(p.x - grab, cy - CHIP_H / 2.0 - 2.0), Vec2::new(w, CHIP_H));
                    top.rect_filled(r.translate(Vec2::new(0.0, 3.0)).expand(1.0), 9.0, Color32::from_black_alpha(90));
                    top.rect_filled(r, 8.0, theme.tab_active);
                    top.rect_stroke(r, 8.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                    let name = self.config.groups.iter().find(|g| g.id == id).map(|g| g.name.to_uppercase()).unwrap_or_default();
                    paint_group_name(&top, r, &name, theme.text);
                }
            }
            // Let go: there; Escape: as it was.
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.bar_drag = None;
            } else if !pointer_down {
                self.bar_drag = None;
                if let Some((_, a)) = drop {
                    action = Some(a);
                }
            }
        } else if !pointer_down {
            self.bar_drag = None;
        }

        // Faded edges where chips are cut.
        if scroll > 0.5 {
            fade_edge(&painter, Rect::from_min_size(area.min, Vec2::new(24.0, area.height())), theme.chrome_bg, true);
        }
        if scroll < max_scroll - 0.5 {
            fade_edge(&painter, Rect::from_min_max(Pos2::new(area.max.x - 24.0, area.min.y), area.max), theme.chrome_bg, false);
        }
        painter.vline(plus_rect.min.x - 3.0, egui::Rangef::new(cy - 10.0, cy + 10.0), Stroke::new(1.0, theme.tab_hover));
        let new_hint = format!("{} ({})", t.new_tab, self.config.settings.shortcuts.new_tab.label());
        if icon_button(ui, &painter, plus_rect, "ws-new-tab", &theme, paint_plus).on_hover_text(new_hint).clicked() {
            action = Some(TabAction::New);
        }
        self.apply_tab_action(ui, action);
    }

    /// The runs of chips, in order, each chip with its width (not laid out yet). The profiles outside
    /// groups get a run while a profile is dragged even when there are none, to be dropped there.
    fn bar_segments(&self, painter: &egui::Painter, dragging_profile: bool) -> Vec<Segment> {
        let chip = |c: Chip| (c, self.chip_width(painter, c), Rect::NOTHING);
        let is_profile = |id: &Uuid| self.config.profiles.iter().any(|p| p.id == *id);
        let mut segments = Vec::new();
        let plain: Vec<_> = (0..self.tabs.len()).filter(|&i| self.tabs[i].ssh.is_none() && self.tabs[i].db.is_none() && self.tabs[i].profile.is_none()).map(|i| chip(Chip::Tab(i))).collect();
        if !plain.is_empty() {
            segments.push(Segment { group: None, plain: true, label_w: 0.0, chips: plain, rect: Rect::NOTHING });
        }
        let loose: Vec<_> = self.config.ungrouped.iter().filter(|id| is_profile(id)).map(|id| chip(Chip::Profile(*id))).collect();
        if !loose.is_empty() || dragging_profile {
            segments.push(Segment { group: None, plain: false, label_w: 0.0, chips: loose, rect: Rect::NOTHING });
        }
        for (gi, g) in self.config.groups.iter().enumerate().filter(|(_, g)| g.local) {
            // Folded: only its open profiles.
            let open = |id: &Uuid| self.tabs.iter().any(|t| t.profile == Some(*id));
            let chips = g.items.iter().filter(|id| is_profile(id) && (!g.collapsed || open(id))).map(|id| chip(Chip::Profile(*id))).collect();
            let label_w = if self.group_rename.as_ref().is_some_and(|r| r.0 == g.id) {
                GROUP_EDIT_W + 28.0
            } else {
                group_name_width(painter, &g.name.to_uppercase()) + 28.0
            };
            segments.push(Segment { group: Some(gi), plain: false, label_w, chips, rect: Rect::NOTHING });
        }
        segments
    }

    /// The tab of a chip, when open.
    fn chip_tab(&self, chip: Chip) -> Option<usize> {
        match chip {
            Chip::Tab(i) => Some(i),
            Chip::Profile(id) => self.tabs.iter().position(|t| t.profile == Some(id)),
        }
    }

    fn chip_width(&self, painter: &egui::Painter, chip: Chip) -> f32 {
        let (name, renaming) = match chip {
            Chip::Tab(i) => (self.tabs[i].title().to_owned(), self.rename.as_ref().is_some_and(|r| r.tab == i)),
            Chip::Profile(id) => (self.item(id).map(|i| i.name).unwrap_or_default(), self.item_rename.as_ref().is_some_and(|r| r.id == id)),
        };
        let text_w = if renaming { CHIP_TEXT_W } else { (painter.layout_no_wrap(name, FontId::proportional(13.0), Color32::WHITE).size().x.ceil() + 1.0).min(CHIP_TEXT_W) };
        let open = self.chip_tab(chip).is_some();
        8.0 + 20.0 + 8.0 + text_w + if open { 26.0 } else { 12.0 }
    }

    /// Where `drag` would go with the pointer at `px`: the x of its mark, and the action moving it there.
    fn bar_drop(&self, drag: BarDrag, px: f32, segments: &[Segment]) -> Option<(f32, TabAction)> {
        match drag {
            BarDrag::Profile { id, .. } => {
                // The run under the pointer, else the nearest (not the tabs that are no profile).
                let runs = segments.iter().filter(|s| !s.plain);
                let seg = runs.min_by(|a, b| {
                    let d = |s: &Segment| if px < s.rect.min.x { s.rect.min.x - px } else if px > s.rect.max.x { px - s.rect.max.x } else { 0.0 };
                    d(a).total_cmp(&d(b))
                })?;
                let group = seg.group.map(|gi| self.config.groups[gi].id);
                let others: Vec<(Uuid, Rect)> = seg.chips.iter().filter_map(|(c, _, r)| match c {
                    Chip::Profile(other) if *other != id => Some((*other, *r)),
                    _ => None,
                }).collect();
                match others.iter().find(|(_, r)| r.center().x > px) {
                    Some((before, r)) => Some((r.min.x - CHIP_GAP / 2.0, TabAction::DropItem(id, group, Some(*before)))),
                    None => {
                        let end = others.last().map_or(seg.rect.min.x + SEG_PAD + seg.label_w + 2.0, |(_, r)| r.max.x + CHIP_GAP / 2.0);
                        Some((end, TabAction::DropItem(id, group, None)))
                    }
                }
            }
            BarDrag::Group { id, .. } => {
                let groups: Vec<(Uuid, Rect)> = segments.iter().filter_map(|s| s.group.map(|gi| (self.config.groups[gi].id, s.rect))).filter(|(g, _)| *g != id).collect();
                match groups.iter().find(|(_, r)| r.center().x > px) {
                    Some((before, r)) => Some((r.min.x - SEG_GAP / 2.0, TabAction::DropGroup(id, Some(*before)))),
                    None => {
                        let end = groups.last().map(|(_, r)| r.max.x + SEG_GAP / 2.0)?;
                        Some((end, TabAction::DropGroup(id, None)))
                    }
                }
            }
        }
    }

    /// A group's name at the head of its capsule: a click lists all its profiles, a drag moves it, a
    /// right click renames or deletes it.
    fn group_label(&mut self, ui: &mut Ui, painter: &egui::Painter, gi: usize, hit: Rect, rect: Rect, action: &mut Option<TabAction>) {
        let t = self.t();
        let theme = self.theme.clone();
        let (gid, collapsed) = (self.config.groups[gi].id, self.config.groups[gi].collapsed);
        let name = self.config.groups[gi].name.to_uppercase();
        if let Some(rename) = self.group_rename.as_mut().filter(|r| r.0 == gid) {
            let field = Rect::from_min_max(Pos2::new(rect.min.x + 22.0, rect.min.y + 5.0), Pos2::new(rect.max.x - 6.0, rect.max.y - 5.0));
            let edit = ui.put(field, egui::TextEdit::singleline(&mut rename.1).font(FontId::proportional(12.5)).frame(Frame::NONE));
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
            paint_chevron(painter, Pos2::new(rect.min.x + 11.0, rect.center().y), !collapsed, theme.text);
            return;
        }
        let resp = ui.interact(hit, ui.id().with(("ws-group", gid)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let w = group_name_width(painter, &name) + 28.0;
            self.bar_drag = Some(BarDrag::Group { id: gid, grab: p.x - rect.min.x, w });
        }
        let hovered = resp.hovered() && self.bar_drag.is_none();
        if hovered {
            painter.rect_filled(rect, 8.0, theme.tab_hover);
        }
        let color = if hovered { theme.text } else { theme.text_muted };
        paint_group_name(painter, rect, &name, color);
        paint_chevron(painter, Pos2::new(rect.min.x + 11.0, rect.center().y), !collapsed, color);
        // A click: all its profiles (folded, the bar shows only the open ones).
        let items: Vec<Uuid> = self.config.groups[gi].items.iter().copied().filter(|id| self.config.profiles.iter().any(|p| p.id == *id)).collect();
        // Its own id: the right click's menu has the default one, and would open with it.
        egui::Popup::menu(&resp).id(resp.id.with("projects")).width(220.0).show(|ui| {
            for id in items {
                let Some(item) = self.item(id) else { continue };
                let label = if item.open.is_some() { format!("●  {}", item.name) } else { format!("    {}", item.name) };
                menu_item(ui, &label, TabAction::OpenItem(id), action);
            }
            ui.separator();
            menu_item(ui, if collapsed { t.ws_group_all } else { t.ws_group_open_only }, TabAction::ToggleGroup(gid), action);
        });
        resp.context_menu(|ui| {
            ui.set_min_width(170.0);
            menu_item(ui, t.rename, TabAction::StartGroupRename(gid), action);
            menu_item(ui, t.new_group, TabAction::NewGroup(true), action);
            ui.separator();
            menu_item(ui, t.delete_group, TabAction::DeleteGroup(gid), action);
        });
    }

    /// A tab or a profile: a click shows it (opening the profile), a drag moves the profile, ✕ closes
    /// the tab, a double click renames, a right click gives its menu.
    #[allow(clippy::too_many_arguments)]
    fn bar_chip(&mut self, ui: &mut Ui, painter: &egui::Painter, chip: Chip, w: f32, r: Rect, area: Rect, action: &mut Option<TabAction>) {
        let theme = self.theme.clone();
        let index = self.chip_tab(chip);
        let id = match chip {
            Chip::Profile(id) => Some(id),
            Chip::Tab(_) => None,
        };
        let salt = match chip {
            Chip::Profile(id) => egui::Id::new(("ws-chip", id)),
            Chip::Tab(i) => egui::Id::new(("ws-tab", i)),
        };
        let (name, color) = match id {
            Some(id) => self.item(id).map(|i| (i.name, i.color)).unwrap_or_default(),
            None => index.map(|i| (self.tabs[i].title().to_owned(), self.tabs[i].color)).unwrap_or_default(),
        };
        // Dragged: only its place, dimmed (it follows the pointer).
        if id.is_some() && matches!(self.bar_drag, Some(BarDrag::Profile { id: d, .. }) if Some(d) == id) {
            painter.rect_stroke(r, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
            return;
        }
        let sense = if id.is_some() { Sense::click_and_drag() } else { Sense::click() };
        let resp = ui.interact(r.intersect(area), salt, sense);
        if resp.drag_started()
            && let (Some(id), Some(p)) = (id, resp.interact_pointer_pos())
        {
            self.bar_drag = Some(BarDrag::Profile { id, grab: p.x - r.min.x, w });
        }
        let hovered = resp.contains_pointer() && self.bar_drag.is_none();
        let active = index.is_some() && index == self.shown_tab();
        let close_rect = Rect::from_center_size(Pos2::new(r.max.x - 14.0, r.center().y), Vec2::splat(16.0));
        let text_rect = Rect::from_min_max(Pos2::new(r.min.x + 36.0, r.min.y), Pos2::new(if index.is_some() { close_rect.min.x - 2.0 } else { r.max.x - 8.0 }, r.max.y));
        if hovered && !active {
            painter.rect_filled(r, 8.0, theme.tab_hover.gamma_multiply(0.8));
        }
        let renamed = self.chip_rename(ui, index, id, text_rect.intersect(area), action);
        self.paint_chip(ui, painter, r, if renamed { "" } else { &name }, color, index, active, hovered);
        // Open: ✕ on hover (or shown); a long command ended in it meanwhile: a green or red dot there.
        if let Some(i) = index {
            if (hovered || active) && !renamed && area.contains_rect(close_rect) {
                let close = ui.interact(close_rect, salt.with("close"), Sense::click());
                if close.hovered() {
                    painter.rect_filled(close_rect, 4.0, theme.tab_hover.gamma_multiply(1.8));
                }
                paint_cross(painter, close_rect.center(), if close.hovered() { theme.text } else { theme.text_muted });
                if close.clicked() {
                    *action = Some(TabAction::Close(i));
                }
            } else if let Some(done) = &self.tabs[i].done {
                painter.circle_filled(close_rect.center(), 3.5, if done.ok { theme.ansi[2] } else { theme.ansi[1] });
            }
        }
        if renamed {
            return;
        }
        if resp.clicked() {
            action.get_or_insert(match (index, id) {
                (_, Some(id)) => TabAction::OpenItem(id),
                (Some(i), None) => TabAction::Select(i),
                _ => return,
            });
        }
        if resp.double_clicked() {
            *action = Some(match (index, id) {
                (_, Some(id)) => TabAction::StartItemRename(id),
                (Some(i), None) => TabAction::StartRename(i),
                _ => return,
            });
        }
        if let Some(i) = index.filter(|_| resp.middle_clicked()) {
            *action = Some(TabAction::Close(i));
        }
        // On hover: its folder and panes, what runs in it, how its last long command ended.
        let t = self.t();
        let mut hint: Vec<String> = Vec::new();
        if let Some(p) = id.and_then(|id| self.config.profiles.iter().find(|p| p.id == id)) {
            let folder = index.and_then(|i| self.ws_root(i)).or_else(|| p.tab.root.clone()).or_else(|| p.tab.layout.cwds().into_iter().next().flatten());
            if let Some(folder) = folder {
                hint.push(folder.display().to_string());
            }
            hint.push(format!("{} {}", p.tab.layout.panes(), t.layout_panes));
        }
        if let Some(i) = index {
            hint.extend(self.live.get(i).cloned().flatten().map(|p| format!("▶ {p}")));
            hint.extend(self.tabs[i].done.as_ref().map(|d| d.summary.clone()));
        }
        let resp = if hint.is_empty() || self.bar_drag.is_some() { resp } else { resp.on_hover_text(hint.join("\n")) };
        match (index, id) {
            (_, Some(id)) => {
                if let Some(item) = self.item(id) {
                    resp.context_menu(|ui| self.item_menu(ui, id, &item, action));
                }
            }
            (Some(i), None) => {
                resp.context_menu(|ui| self.tab_menu(ui, i, action));
            }
            _ => {}
        }
    }

    /// A chip's look: tinted with its color, filled when open, stronger and underlined when shown;
    /// closed, only its badge and its name, dimmed.
    #[allow(clippy::too_many_arguments)]
    fn paint_chip(&self, ui: &Ui, painter: &egui::Painter, r: Rect, name: &str, color: Option<Color32>, index: Option<usize>, active: bool, hovered: bool) {
        let theme = &self.theme;
        let tint = color.unwrap_or(theme.accent);
        let open = index.is_some();
        if active {
            painter.rect_filled(r, 8.0, tint.gamma_multiply(0.22));
            painter.rect_stroke(r, 8.0, Stroke::new(1.0, tint.gamma_multiply(0.75)), egui::StrokeKind::Inside);
            painter.hline(egui::Rangef::new(r.min.x + 10.0, r.max.x - 10.0), r.max.y - 1.5, Stroke::new(2.0, tint));
        } else if open {
            painter.rect_filled(r, 8.0, tint.gamma_multiply(if hovered { 0.16 } else { 0.1 }));
            painter.rect_stroke(r, 8.0, Stroke::new(1.0, tint.gamma_multiply(0.3)), egui::StrokeKind::Inside);
        }
        let dot = Pos2::new(r.min.x + 18.0, r.center().y);
        paint_badge(painter, dot, name, color, false, open, active, theme);
        if index.and_then(|i| self.live.get(i)).is_some_and(Option::is_some) {
            paint_live(ui, painter, dot, false, theme);
        }
        if name.is_empty() {
            return;
        }
        let text_color = if active { theme.text } else if open { theme.fg } else if hovered { theme.text } else { theme.text_muted };
        let text_rect = Rect::from_min_max(Pos2::new(r.min.x + 36.0, r.min.y), Pos2::new(r.max.x - if open { 26.0 } else { 12.0 }, r.max.y));
        let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), FontId::proportional(13.0), text_color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let galley = painter.layout_job(job);
        painter.with_clip_rect(text_rect.intersect(painter.clip_rect())).galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, text_color);
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
}

fn group_name_job(name: &str, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, egui::TextFormat { font_id: FontId::proportional(10.5), color, extra_letter_spacing: 1.2, ..Default::default() });
    job
}

fn group_name_width(painter: &egui::Painter, name: &str) -> f32 {
    painter.layout_job(group_name_job(name, Color32::WHITE)).size().x
}

/// A group's name in small spaced capitals, after its chevron.
fn paint_group_name(painter: &egui::Painter, rect: Rect, name: &str, color: Color32) {
    let galley = painter.layout_job(group_name_job(name, color));
    painter.galley(Pos2::new(rect.min.x + 22.0, rect.center().y - galley.size().y / 2.0), galley, color);
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
