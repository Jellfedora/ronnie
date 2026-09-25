//! The sidebar: logo, LOCAL terminals and profiles, SSH hosts and groups, update card, settings button.

use super::*;

impl App {
    /// Shown when every tab is closed: the app stays open.
    pub(super) fn empty_state(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let mut open = None;
        let mut new = false;
        let width = 260.0;
        let area = Rect::from_center_size(rect.center(), Vec2::new(width, rect.height().min(420.0)));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new(t.no_tabs).size(18.0).color(self.theme.text));
            ui.add_space(12.0);
            let hint = self.config.settings.shortcuts.new_tab.label();
            if ui.add(egui::Button::new(t.new_terminal).shortcut_text(hint).min_size(Vec2::new(width, 32.0))).clicked() {
                new = true;
            }
            if !self.config.profiles.is_empty() {
                ui.add_space(18.0);
                ui.label(egui::RichText::new(t.profiles).size(11.0).color(self.theme.text_muted));
                ui.add_space(4.0);
                for p in &self.config.profiles {
                    let (dot, name) = (p.tab.color.unwrap_or(self.theme.text_muted), p.name(t.untitled));
                    let resp = ui.add(egui::Button::new(format!("     {name}")).min_size(Vec2::new(width, 30.0)).truncate());
                    ui.painter().circle_filled(Pos2::new(resp.rect.min.x + 14.0, resp.rect.center().y), 4.0, dot);
                    if resp.clicked() {
                        open = Some(p.id);
                    }
                }
            }
        });
        if new {
            self.new_tab(ui.ctx());
        }
        if let Some(id) = open {
            self.open_profile(id);
        }
    }

    /// A profile or an SSH host, as shown in the sidebar.
    pub(super) fn item(&self, id: Uuid) -> Option<Item> {
        let t = self.t();
        if let Some(p) = self.config.profiles.iter().find(|p| p.id == id) {
            let panes = p.tab.layout.panes();
            return Some(Item {
                name: p.name(t.untitled).to_owned(),
                color: p.tab.color,
                ssh: false,
                open: self.tabs.iter().position(|t| t.profile == Some(id)),
                hint: format!("{panes} {}", t.layout_panes),
            });
        }
        let host = self.config.ssh.iter().find(|h| h.id == id)?;
        Some(Item {
            name: host.name.clone(),
            color: host.color,
            ssh: true,
            open: self.tabs.iter().position(|t| t.ssh == Some(id)),
            hint: host.address(),
        })
    }

    /// Profiles (`local`, listed under the LOCAL terminals) or SSH hosts (with their own header), in
    /// user-defined, collapsible groups.
    pub(super) fn profiles_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>, local: bool) {
        let t = self.t();
        let painter = ui.painter().clone();
        // The dragged row is painted above the others.
        let drag_painter = painter.clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("item-drag")));

        // SSH header: title and a "+" menu.
        if !local {
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        painter.text(
            Pos2::new(header.min.x + 6.0, header.center().y),
            Align2::LEFT_CENTER,
            t.profiles,
            FontId::proportional(11.0),
            self.theme.text_muted.gamma_multiply(0.8),
        );
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        let plus = icon_button(ui, &painter, plus_rect, "profiles-plus", &self.theme, paint_plus);
        egui::Popup::menu(&plus).width(210.0).show(|ui| {
            menu_item(ui, t.new_host, TabAction::NewHost, action);
            menu_item(ui, t.new_group, TabAction::NewGroup(false), action);
        });
        *y += SECTION_HEADER_H;
        }

        if !local && self.config.ssh.is_empty() && !self.config.groups.iter().any(|g| !g.local) {
            let hint = painter.layout(t.no_profiles.to_owned(), FontId::proportional(12.0), self.theme.text_muted.gamma_multiply(0.7), row_w - 12.0);
            let h = hint.size().y;
            painter.galley(Pos2::new(left + 6.0, *y + 4.0), hint, self.theme.text_muted);
            *y += h + 12.0;
            return;
        }

        let of_kind = |id: &&Uuid| if local { self.config.profiles.iter().any(|p| p.id == **id) } else { self.config.ssh.iter().any(|h| h.id == **id) };
        let mut rows: Vec<Row> = self.config.ungrouped.iter().filter(of_kind).map(|id| Row::Item(None, *id)).collect();
        for (gi, g) in self.config.groups.iter().enumerate().filter(|(_, g)| g.local == local) {
            rows.push(Row::Group(gi));
            if !g.collapsed {
                rows.extend(g.items.iter().filter(of_kind).map(|id| Row::Item(Some(gi), *id)));
            }
        }
        let (pointer, pointer_down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));
        let mut rects: Vec<(Row, Rect)> = Vec::with_capacity(rows.len());

        for row in rows {
            let h = if matches!(row, Row::Group(_)) { GROUP_H } else { ROW_H };
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, h));
            *y += h + ROW_GAP;
            rects.push((row, slot));
            let dragged_here = self.item_drag.as_ref().is_some_and(|d| pointer_down && d.is(row, &self.config));
            // A row being dragged follows the pointer; its slot stays empty.
            let (rect, painter) = match (dragged_here, pointer, &self.item_drag) {
                (true, Some(p), Some(d)) => (slot.translate(Vec2::new(0.0, p.y - d.grab - slot.min.y)), &drag_painter),
                _ => (slot, &painter),
            };
            match row {
                Row::Group(gi) => self.group_row(ui, painter, gi, slot, rect, dragged_here, action),
                Row::Item(_, id) => self.item_row(ui, painter, id, slot, rect, dragged_here, action),
            }
        }

        // Drop target of a drag in progress, shown as a line (or a highlighted group); applied on release.
        if let (Some(drag), Some(p)) = (&self.item_drag, pointer) {
            let (target, mark) = drop_target(drag, p, &rects, &self.config);
            if pointer_down {
                match mark {
                    Mark::Line(y) => {
                        painter.hline(left + 4.0..=left + row_w - 4.0, y, Stroke::new(2.0, self.theme.accent));
                    }
                    Mark::Into(r) => {
                        painter.rect_stroke(r, 6.0, Stroke::new(1.5, self.theme.accent), egui::StrokeKind::Inside);
                    }
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else {
                *action = Some(target);
                self.item_drag = None;
            }
        }
    }

    pub(super) fn group_row(&mut self, ui: &mut Ui, painter: &egui::Painter, gi: usize, slot: Rect, rect: Rect, dragged: bool, action: &mut Option<TabAction>) {
        let t = self.t();
        let group = &self.config.groups[gi];
        let (gid, collapsed, group_local) = (group.id, group.collapsed, group.local);
        let resp = ui.interact(slot, ui.id().with(("group", gid)), Sense::click_and_drag());
        if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                self.item_drag = Some(ItemDrag { what: Dragged::Group(gid), grab: p.y - slot.min.y });
            }
        }
        let hovered = resp.contains_pointer() || dragged;
        if hovered {
            painter.rect_filled(rect, 6.0, self.theme.tab_hover);
        }
        let color = if hovered { self.theme.text } else { self.theme.text_muted };
        paint_chevron(painter, Pos2::new(rect.min.x + 12.0, rect.center().y), !collapsed, color);
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y), Pos2::new(rect.max.x - 30.0, rect.max.y));

        if let Some(rename) = self.group_rename.as_mut().filter(|r| r.0 == gid) {
            let edit = ui.put(text_rect.shrink2(Vec2::new(0.0, 3.0)), egui::TextEdit::singleline(&mut rename.1).font(FontId::proportional(12.5)).frame(Frame::NONE));
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
            return;
        }

        let name = egui::RichText::new(&group.name).size(12.5).strong().color(color);
        let mut job = egui::text::LayoutJob::simple_singleline(name.text().to_owned(), FontId::proportional(12.5), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
        let count = group.items.iter().filter(|id| if group_local { self.config.profiles.iter().any(|p| p.id == **id) } else { self.config.ssh.iter().any(|h| h.id == **id) }).count();
        if collapsed || hovered {
            painter.text(
                Pos2::new(rect.max.x - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                count.to_string(),
                FontId::proportional(11.0),
                self.theme.text_muted.gamma_multiply(0.8),
            );
        }
        if resp.double_clicked() {
            *action = Some(TabAction::StartGroupRename(gid));
        } else if resp.clicked() {
            *action = Some(TabAction::ToggleGroup(gid));
        }
        resp.context_menu(|ui| {
            ui.set_min_width(170.0);
            menu_item(ui, t.rename, TabAction::StartGroupRename(gid), action);
            menu_item(ui, t.new_group, TabAction::NewGroup(group_local), action);
            ui.separator();
            menu_item(ui, t.delete_group, TabAction::DeleteGroup(gid), action);
        });
    }

    pub(super) fn item_row(&mut self, ui: &mut Ui, painter: &egui::Painter, id: Uuid, slot: Rect, rect: Rect, dragged: bool, action: &mut Option<TabAction>) {
        let t = self.t();
        let Some(item) = self.item(id) else { return };
        let resp = ui.interact(slot, ui.id().with(("item", id)), Sense::click_and_drag());
        if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                self.item_drag = Some(ItemDrag { what: Dragged::Item(id), grab: p.y - slot.min.y });
            }
        }
        let hovered = resp.contains_pointer() && self.item_drag.is_none();
        let active = item.open == Some(self.active);
        let fill = if active || dragged {
            self.theme.tab_active
        } else if hovered {
            self.theme.tab_hover
        } else {
            self.theme.tab_bg
        };
        painter.rect_filled(rect, 6.0, fill);
        if active {
            paint_active_bar(painter, rect, self.theme.accent);
        }
        let dot = Pos2::new(rect.min.x + 14.0, rect.center().y);
        let live = item.open.and_then(|i| self.live.get(i).cloned().flatten());
        if live.is_some() {
            paint_live(ui, painter, dot, &self.theme);
        }
        match item.color {
            Some(color) => painter.circle_filled(dot, 4.0, color),
            None => painter.circle_stroke(dot, 3.5, Stroke::new(1.0, self.theme.text_muted.gamma_multiply(0.6))),
        };

        let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
        let show_close = item.open.is_some() && (active || hovered);
        let edit_rect = if show_close { close_rect.translate(Vec2::new(-20.0, 0.0)) } else { close_rect };
        let show_edit = hovered && item.ssh;
        let text_right = if show_edit {
            edit_rect.min.x - 4.0
        } else if show_close {
            close_rect.min.x - 4.0
        } else {
            rect.max.x - 34.0
        };
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + 28.0, rect.min.y), Pos2::new(text_right, rect.max.y));

        if let Some(rename) = self.item_rename.as_mut().filter(|r| r.id == id) {
            let color = if rename.error.is_some() { self.theme.ansi[1] } else { self.theme.text };
            let edit = ui.put(text_rect.shrink2(Vec2::new(0.0, 5.0)), egui::TextEdit::singleline(&mut rename.text).font(FontId::proportional(13.0)).frame(Frame::NONE).text_color(color));
            if rename.fresh {
                edit.request_focus();
                select_all(ui, edit.id, &rename.text);
                rename.fresh = false;
            }
            if edit.changed() {
                rename.error = None;
            }
            if let Some(err) = rename.error {
                edit.request_focus();
                painter.rect_stroke(rect, 6.0, Stroke::new(1.0, self.theme.ansi[1]), egui::StrokeKind::Inside);
                egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), edit.id.with("err"), egui::PopupAnchor::Position(rect.left_bottom() + Vec2::new(0.0, 4.0)))
                    .show(|ui| ui.label(egui::RichText::new(err).color(self.theme.ansi[1])));
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.item_rename = None;
            } else if enter {
                *action = Some(TabAction::RenameItem(id, rename.text.clone(), true));
            } else if edit.lost_focus() {
                *action = Some(TabAction::RenameItem(id, rename.text.clone(), false));
            }
            return;
        }

        let color = if active || (item.open.is_some() && hovered) {
            self.theme.text
        } else if item.open.is_some() || hovered {
            self.theme.text_muted
        } else {
            self.theme.text_muted.gamma_multiply(0.75)
        };
        let mut job = egui::text::LayoutJob::simple_singleline(item.name.clone(), FontId::proportional(13.0), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
        if show_close {
            let close = ui.interact(close_rect, ui.id().with(("item-close", id)), Sense::click());
            if close.hovered() {
                painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
            }
            paint_cross(painter, close_rect.center(), if close.hovered() { self.theme.text } else { self.theme.text_muted });
            if close.clicked() {
                *action = item.open.map(TabAction::Close);
            }
        }
        if show_edit {
            let edit = ui.interact(edit_rect, ui.id().with(("item-edit", id)), Sense::click());
            if edit.hovered() {
                painter.rect_filled(edit_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
            }
            paint_pencil(painter, edit_rect.center(), if edit.hovered() { self.theme.text } else { self.theme.text_muted });
            if edit.on_hover_text(t.edit).clicked() {
                *action = Some(TabAction::EditHost(id));
            }
        }

        let resp = match &live {
            Some(program) => resp.on_hover_text(format!("{}\n▶ {program}", item.hint)),
            None => resp.on_hover_text(&item.hint),
        };
        if resp.double_clicked() {
            *action = Some(if item.ssh { TabAction::EditHost(id) } else { TabAction::StartItemRename(id) });
        } else if resp.clicked() {
            action.get_or_insert(TabAction::OpenItem(id));
        }
        if resp.middle_clicked() {
            if let Some(i) = item.open {
                *action = Some(TabAction::Close(i));
            }
        }
        resp.context_menu(|ui| self.item_menu(ui, id, &item, action));
    }

    /// Right-click menu of a profile or an SSH host.
    pub(super) fn item_menu(&self, ui: &mut Ui, id: Uuid, item: &Item, action: &mut Option<TabAction>) {
        let t = self.t();
        ui.set_min_width(180.0);
        menu_item(ui, if item.ssh { t.connect } else { t.open }, TabAction::OpenItem(id), action);
        if item.ssh {
            menu_item(ui, t.edit, TabAction::EditHost(id), action);
        } else {
            menu_item(ui, t.edit, TabAction::EditProfile(id), action);
            menu_item(ui, t.rename, TabAction::StartItemRename(id), action);
        }
        if item.ssh {
            menu_item(ui, &format!("📁  {}", t.files_open), TabAction::OpenFiles(id), action);
        }
        if let (Some(i), true) = (item.open, item.ssh) {
            menu_item(ui, t.reconnect, TabAction::Reconnect(i), action);
        }
        {
            ui.menu_button(t.move_to, |ui| {
                let current = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
                if ui.add_enabled(current.is_some(), egui::Button::new(t.no_group)).clicked() {
                    *action = Some(TabAction::DropItem(id, None, None));
                    ui.close();
                }
                // Profiles go into local groups, hosts into SSH groups.
                for g in self.config.groups.iter().filter(|g| g.local != item.ssh) {
                    if ui.add_enabled(current != Some(g.id), egui::Button::new(&g.name)).clicked() {
                        *action = Some(TabAction::DropItem(id, Some(g.id), None));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t.new_group).clicked() {
                    *action = Some(TabAction::NewGroupWith(id));
                    ui.close();
                }
            });
        }
        ui.separator();
        ui.label(egui::RichText::new(t.color).size(12.0).strong().color(self.theme.text_muted));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for color in TAB_COLORS {
                let (r, s) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                ui.painter().circle_filled(r.center(), 7.0, color);
                if item.color == Some(color) || s.hovered() {
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, self.theme.text));
                }
                if s.clicked() {
                    *action = Some(TabAction::ItemColor(id, Some(color)));
                    ui.close();
                }
            }
        });
        if item.color.is_some() && ui.button(t.remove_color).clicked() {
            *action = Some(TabAction::ItemColor(id, None));
            ui.close();
        }
        ui.separator();
        if let Some(i) = item.open {
            menu_item(ui, t.close, TabAction::Close(i), action);
        }
        menu_item(ui, t.delete, if item.ssh { TabAction::DeleteHost(id) } else { TabAction::DeleteProfile(id) }, action);
    }

    /// Small caps section title with an optional "+" button on the right. Returns true when "+" is clicked.
    pub(super) fn section_header(&self, ui: &Ui, title: &str, pos: Pos2, width: f32, plus_hint: Option<&str>) -> bool {
        let rect = Rect::from_min_size(pos, Vec2::new(width, SECTION_HEADER_H));
        let painter = ui.painter();
        painter.text(
            Pos2::new(rect.min.x + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(11.0),
            self.theme.text_muted.gamma_multiply(0.8),
        );
        let Some(hint) = plus_hint else { return false };
        let plus_rect = Rect::from_center_size(Pos2::new(rect.max.x - 12.0, rect.center().y), Vec2::splat(20.0));
        let plus = ui.interact(plus_rect, ui.id().with(("section-plus", title)), Sense::click());
        if plus.hovered() {
            painter.rect_filled(plus_rect, 4.0, self.theme.tab_hover);
        }
        let stroke = Stroke::new(1.6, if plus.hovered() { self.theme.text } else { self.theme.text_muted });
        let (c, d) = (plus_rect.center(), 5.5);
        painter.line_segment([c - Vec2::new(d, 0.0), c + Vec2::new(d, 0.0)], stroke);
        painter.line_segment([c - Vec2::new(0.0, d), c + Vec2::new(0.0, d)], stroke);
        plus.on_hover_text(hint).clicked()
    }

    /// Right-click menu of a tab.
    pub(super) fn tab_menu(&self, ui: &mut Ui, i: usize, action: &mut Option<TabAction>) {
        let t = self.t();
        ui.set_min_width(170.0);
        let mut item = |ui: &mut Ui, label: &str, a: TabAction| {
            if ui.button(label).clicked() {
                *action = Some(a);
                ui.close();
            }
        };
        item(ui, t.rename, TabAction::StartRename(i));
        item(ui, t.split_right, TabAction::Split(i, Direction::Right));
        item(ui, t.split_down, TabAction::Split(i, Direction::Down));
        ui.separator();
        ui.label(egui::RichText::new(t.color).size(12.0).strong().color(self.theme.text_muted));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for color in TAB_COLORS {
                let (r, s) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                let selected = self.tabs[i].color == Some(color);
                ui.painter().circle_filled(r.center(), 7.0, color);
                if selected || s.hovered() {
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, self.theme.text));
                }
                if s.clicked() {
                    *action = Some(TabAction::SetColor(i, Some(color)));
                    ui.close();
                }
            }
        });
        if self.tabs[i].color.is_some() && ui.button(t.remove_color).clicked() {
            *action = Some(TabAction::SetColor(i, None));
            ui.close();
        }
        ui.separator();
        if ui.button(t.close).clicked() {
            *action = Some(TabAction::Close(i));
            ui.close();
        }
    }

    /// Invitation to install a new release (then to restart), drawn at the bottom of `area`. Returns
    /// the top of what it drew (`area.max.y` when nothing is shown).
    pub(super) fn update_card(&mut self, ui: &mut Ui, area: Rect) -> f32 {
        let t = self.t();
        let (title, detail) = match self.updater.state() {
            update::State::Available(a) if !self.update_dismissed => (t.update_available.replace("{v}", &a.version), None),
            update::State::Installing(v) => (t.installing.replace("{v}", &v), None),
            update::State::Installed(v) => (t.update_installed.replace("{v}", &v), None),
            update::State::Failed(e) if self.update_attempted => (t.update_failed.to_owned(), Some(e)),
            _ => return area.max.y,
        };
        let state = self.updater.state();
        let buttons = !matches!(state, update::State::Installing(_));
        let h = if buttons { 78.0 } else { 44.0 };
        let card = Rect::from_min_max(Pos2::new(area.min.x, area.max.y - h - 6.0), Pos2::new(area.max.x, area.max.y - 6.0));
        let painter = ui.painter();
        painter.rect_filled(card, 8.0, self.theme.tab_active);
        painter.rect_stroke(card, 8.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        let mut job = egui::text::LayoutJob::simple_singleline(title, FontId::proportional(13.0), self.theme.text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(card.width() - 24.0);
        painter.galley(card.min + Vec2::new(12.0, 11.0), painter.layout_job(job), self.theme.text);
        let hover = ui.interact(card, ui.id().with("update-card"), Sense::hover());
        if let Some(e) = &detail {
            hover.on_hover_text(e);
        }
        if matches!(state, update::State::Installing(_)) {
            ui.put(Rect::from_center_size(Pos2::new(card.max.x - 20.0, card.min.y + 20.0), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
            return card.min.y - 6.0;
        }

        let row = Rect::from_min_max(Pos2::new(card.min.x + 10.0, card.max.y - 38.0), Pos2::new(card.max.x - 10.0, card.max.y - 10.0));
        let half = (row.width() - 8.0) / 2.0;
        let (left_rect, right_rect) = (Rect::from_min_size(row.min, Vec2::new(half, row.height())), Rect::from_min_size(row.min + Vec2::new(half + 8.0, 0.0), Vec2::new(half, row.height())));
        let primary = |ui: &mut Ui, rect: Rect, label: &str| {
            let text = egui::RichText::new(label).size(12.5).color(self.theme.bg);
            ui.put(rect, egui::Button::new(text).fill(self.theme.accent).corner_radius(6.0)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        };
        let secondary = |ui: &mut Ui, rect: Rect, label: &str| ui.put(rect, egui::Button::new(egui::RichText::new(label).size(12.5)).corner_radius(6.0)).clicked();
        match state {
            update::State::Available(a) => {
                if primary(ui, left_rect, t.update_now) {
                    self.update_attempted = true;
                    self.updater.install(ui.ctx(), a.clone());
                }
                if secondary(ui, right_rect, t.later) {
                    self.update_dismissed = true;
                }
            }
            update::State::Installed(_) => {
                if primary(ui, left_rect, t.restart) {
                    self.request_close(CloseRequest::Restart);
                }
                if secondary(ui, right_rect, t.later) {
                    self.update_dismissed = true;
                }
            }
            update::State::Failed(_) => {
                if primary(ui, left_rect, t.check_now) {
                    self.updater.check(ui.ctx());
                }
                if secondary(ui, right_rect, t.close) {
                    self.update_attempted = false;
                }
            }
            _ => {}
        }
        card.min.y - 6.0
    }

    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        self.live = self.tabs.iter_mut().map(|tab| tab.panes.values_mut().find_map(|term| term.live_program(&ctx).map(str::to_owned))).collect();
        // New profiles and hosts show up outside groups; deleted ones disappear.
        self.config.normalize();
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, self.theme.chrome_bg);
        ui.painter().vline(bar.max.x - 0.5, bar.y_range(), Stroke::new(1.0, self.theme.tab_hover));

        // Empty space in the sidebar drags the window (the native title bar is hidden).
        let bg = ui.interact(bar, ui.id().with("sidebar-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }

        if !ui.input(|i| i.pointer.any_down()) {
            self.tab_grab = None;
        }
        let mut action: Option<TabAction> = None;
        let left = bar.min.x + SIDEBAR_PAD;
        let row_w = bar.width() - 2.0 * SIDEBAR_PAD;

        // Everything below the traffic lights scrolls when there are many tabs.
        let footer_top = bar.max.y - FOOTER_H;
        let logo_rect = Rect::from_min_size(Pos2::new(bar.min.x, bar.min.y + SIDEBAR_TOP), Vec2::new(bar.width() - 1.0, LOGO_H));
        paint_logo(ui.painter(), logo_rect, &self.theme);
        // Dev builds say so, next to the logo: they don't share the installed app's profiles.
        if !config::OFFICIAL {
            let at = Pos2::new(logo_rect.center().x + 58.0, logo_rect.center().y - 12.0);
            let galley = ui.painter().layout_no_wrap("DEV".to_owned(), FontId::monospace(9.5), self.theme.bg);
            let badge = Rect::from_min_size(at, galley.size() + Vec2::new(8.0, 2.0));
            ui.painter().rect_filled(badge, 3.0, self.theme.ansi[3]);
            ui.painter().galley(badge.min + Vec2::new(4.0, 1.0), galley, self.theme.bg);
        }
        let card_top = self.update_card(ui, Rect::from_min_max(Pos2::new(left, bar.min.y), Pos2::new(left + row_w, footer_top)));
        let scroll_rect = Rect::from_min_max(Pos2::new(bar.min.x, logo_rect.max.y), Pos2::new(bar.max.x - 1.0, card_top));

        // Footer: settings button, always visible.
        let button = Rect::from_min_max(Pos2::new(left, footer_top + 5.0), Pos2::new(left + row_w, bar.max.y - 7.0));
        let settings = ui.interact(button, ui.id().with("settings-btn"), Sense::click());
        let hot = settings.hovered() || self.settings_dialog;
        ui.painter().rect_filled(button, 6.0, if hot { self.theme.tab_active } else { self.theme.tab_hover.gamma_multiply(0.6) });
        ui.painter().rect_stroke(button, 6.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(if hot { 0.8 } else { 0.35 })), egui::StrokeKind::Inside);
        paint_gear(ui.painter(), Pos2::new(button.min.x + 16.0, button.center().y), self.theme.accent);
        ui.painter().text(Pos2::new(button.min.x + 32.0, button.center().y), Align2::LEFT_CENTER, t.settings, FontId::proportional(13.0), self.theme.text);
        let shortcut = self.config.settings.shortcuts.open_settings.label();
        ui.painter().text(Pos2::new(button.max.x - 10.0, button.center().y), Align2::RIGHT_CENTER, &shortcut, FontId::proportional(11.5), self.theme.text_muted);
        if settings.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.settings_dialog = true;
        }

        let mut scroll_ui = ui.new_child(egui::UiBuilder::new().max_rect(scroll_rect));
        // A slim handle that stays visible whenever the list overflows (egui's default only shows on hover).
        scroll_ui.spacing_mut().scroll = egui::style::ScrollStyle {
            bar_width: 6.0,
            floating_allocated_width: 4.0,
            dormant_background_opacity: 0.0,
            active_background_opacity: 0.0,
            dormant_handle_opacity: 0.6,
            ..egui::style::ScrollStyle::thin()
        };
        // Mouse drags never scroll it (egui's default): they move tabs, or the window from empty space.
        let scroll = egui::ScrollArea::vertical()
            .id_salt("sidebar-scroll")
            .auto_shrink(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
        scroll.show(&mut scroll_ui, |ui| {
        let base_painter = ui.painter().clone();
        // The dragged tab is painted above its neighbours.
        let drag_painter = base_painter.clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("tab-drag")));
        let origin = ui.max_rect().min;
        let mut y = origin.y;

        // Local section: open tabs that are neither a profile nor an SSH host, then local profiles.
        let new_hint = format!("{} ({})", t.new_tab, self.config.settings.shortcuts.new_tab.label());
        if self.section_header(ui, t.terminals, Pos2::new(left, y), row_w, Some(&new_hint)) {
            action = Some(TabAction::New);
        }
        y += SECTION_HEADER_H;

        let is_plain = |tab: &Tab| tab.ssh.is_none() && tab.profile.is_none();
        let first_y = y;
        let local_count = self.tabs.iter().filter(|t| is_plain(t)).count();
        let mut tab_rects: Vec<(usize, Rect)> = Vec::with_capacity(local_count);
        for i in 0..self.tabs.len() {
            if !is_plain(&self.tabs[i]) {
                continue;
            }
            let slot = Rect::from_min_size(Pos2::new(left, y), Vec2::new(row_w, ROW_H));
            tab_rects.push((i, slot));
            y += ROW_H + ROW_GAP;

            let id = ui.id().with(("tab", i));
            let resp = ui.interact(slot, id, Sense::click_and_drag());
            let pointer_y = ui.input(|inp| inp.pointer.interact_pos()).map(|p| p.y);
            // Only on the first frame: a drag moved to a new slot after a swap may report started again.
            if resp.drag_started() && self.tab_grab.is_none() {
                self.tab_grab = pointer_y.map(|py| py - slot.min.y);
            }
            let dragging = resp.dragged() && self.tab_grab.is_some();
            let (rect, painter) = match (dragging, self.tab_grab, pointer_y) {
                (true, Some(grab), Some(py)) => {
                    let max_y = first_y + (ROW_H + ROW_GAP) * (local_count as f32 - 1.0);
                    let top = (py - grab).clamp(first_y, max_y.max(first_y));
                    (slot.translate(Vec2::new(0.0, top - slot.min.y)), drag_painter.clone())
                }
                _ => (slot, base_painter.clone()),
            };
            let active = i == self.active;
            let tab = &self.tabs[i];
            // Not `hovered()`: that turns false when the pointer is over the ✕ (another widget), which
            // would hide the ✕ as soon as the pointer reaches it.
            let row_hovered = resp.contains_pointer();

            let fill = if active || dragging {
                self.theme.tab_active
            } else if row_hovered {
                self.theme.tab_hover
            } else {
                self.theme.tab_bg
            };
            painter.rect_filled(rect, 6.0, fill);
            if active {
                paint_active_bar(&painter, rect, self.theme.accent);
            }
            let dot = Pos2::new(rect.min.x + 14.0, rect.center().y);
            let live = self.live.get(i).cloned().flatten();
            if live.is_some() {
                paint_live(ui, &painter, dot, &self.theme);
            }
            match tab.color {
                Some(color) => painter.circle_filled(dot, 4.0, color),
                None => painter.circle_stroke(dot, 3.5, Stroke::new(1.0, self.theme.text_muted.gamma_multiply(0.6))),
            };

            let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
            let show_close = active || row_hovered;
            let text_left = rect.min.x + 28.0;

            let renaming = self.rename.as_ref().is_some_and(|r| r.tab == i);
            if renaming {
                let r = self.rename.as_mut().unwrap();
                let edit_rect = Rect::from_min_max(Pos2::new(text_left, rect.min.y + 5.0), Pos2::new(rect.max.x - 8.0, rect.max.y - 5.0));
                let edit = ui.put(
                    edit_rect,
                    egui::TextEdit::singleline(&mut r.text)
                        .font(FontId::proportional(13.0))
                        .frame(Frame::NONE)
                        .text_color(if r.error.is_some() { self.theme.ansi[1] } else { self.theme.text }),
                );
                if edit.changed() {
                    r.error = None;
                }
                if let Some(err) = r.error {
                    // Keep editing until the name is changed or the rename cancelled.
                    edit.request_focus();
                    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, self.theme.ansi[1]), egui::StrokeKind::Inside);
                    egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), edit.id.with("err"), egui::PopupAnchor::Position(rect.left_bottom() + Vec2::new(0.0, 4.0)))
                        .show(|ui| ui.label(egui::RichText::new(err).color(self.theme.ansi[1])));
                }
                if r.select_all > 0 {
                    edit.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
                        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(r.text.chars().count()));
                        state.cursor.set_char_range(Some(all));
                        state.store(ui.ctx(), edit.id);
                    }
                    if !ui.input(|inp| inp.pointer.any_down()) {
                        r.select_all -= 1;
                    }
                    ui.ctx().request_repaint();
                }
                let enter = ui.input(|inp| inp.key_pressed(Key::Enter));
                let escape = ui.input(|inp| inp.key_pressed(Key::Escape));
                if escape {
                    self.rename = None;
                    self.focus_terminal = true;
                } else if enter {
                    action = Some(TabAction::Rename(i, true));
                } else if edit.lost_focus() {
                    action = Some(TabAction::Rename(i, false));
                }
            } else {
                let text_color = if active { self.theme.text } else { self.theme.text_muted };
                let text_right = if show_close { close_rect.min.x - 4.0 } else { rect.max.x - 10.0 };
                let text_rect = Rect::from_min_max(Pos2::new(text_left, rect.min.y), Pos2::new(text_right, rect.max.y));
                let mut job = egui::text::LayoutJob::simple_singleline(tab.title().to_owned(), FontId::proportional(13.0), text_color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
                let galley = painter.layout_job(job);
                let pos = Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0);
                painter.with_clip_rect(text_rect).galley(pos, galley, text_color);
            }

            if show_close && !renaming {
                let close = ui.interact(close_rect, id.with("close"), Sense::click());
                if close.hovered() {
                    painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
                }
                let c = close_rect.center();
                let d = 3.5;
                let stroke = Stroke::new(1.4, if close.hovered() { self.theme.text } else { self.theme.text_muted });
                painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
                painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
                if close.clicked() {
                    action = Some(TabAction::Close(i));
                }
            }

            if resp.clicked() {
                action.get_or_insert(TabAction::Select(i));
            }
            if resp.double_clicked() {
                action = Some(TabAction::StartRename(i));
            }
            if resp.middle_clicked() {
                action = Some(TabAction::Close(i));
            }
            if dragging {
                action = Some(TabAction::DragOver(i, rect.center().y));
            }
            if resp.drag_stopped() {
                self.tab_grab = None;
            }
            let resp = match &live {
                Some(program) => resp.on_hover_text(format!("▶ {program}")),
                None => resp,
            };
            resp.context_menu(|ui| self.tab_menu(ui, i, &mut action));
        }

        // Local profiles sit with the terminals, in their own groups; the SSH section below lists hosts.
        self.profiles_section(ui, left, row_w, &mut y, &mut action, true);

        y += SECTION_GAP;
        self.profiles_section(ui, left, row_w, &mut y, &mut action, false);

        // Content height, so the scroll area knows how far it can go.
        ui.allocate_rect(Rect::from_min_max(origin, Pos2::new(origin.x + 1.0, y + ROW_H + SIDEBAR_PAD)), Sense::hover());

        match action {
            Some(TabAction::Select(i)) => self.select(i),
            Some(TabAction::Close(i)) => self.request_close(CloseRequest::Tab(i)),
            Some(TabAction::New) => self.new_tab(ui.ctx()),
            Some(TabAction::Split(i, side)) => self.split(ui.ctx(), i, side),
            Some(TabAction::DeleteProfile(id)) => self.delete_profile(id),
            Some(TabAction::NewHost) => self.host_editor = Some(HostEditor::new(SshHost::new(), true, None)),
            Some(TabAction::EditHost(id)) => {
                if let Some(host) = self.config.ssh.iter().find(|h| h.id == id) {
                    let group = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
                    self.host_editor = Some(HostEditor::new(host.clone(), false, group));
                }
            }
            Some(TabAction::DeleteHost(id)) => self.delete_host(id),
            Some(TabAction::OpenFiles(id)) => {
                self.open_ssh(id);
                self.toggle_files(self.active, true);
            }
            Some(TabAction::OpenItem(id)) => {
                if self.config.profiles.iter().any(|p| p.id == id) {
                    self.open_profile(id);
                } else {
                    self.open_ssh(id);
                }
            }
            Some(TabAction::ItemColor(id, color)) => {
                self.set_profile_color(id, color);
                if let Some(host) = self.config.ssh.iter_mut().find(|h| h.id == id) {
                    host.color = color;
                }
                for tab in self.tabs.iter_mut().filter(|t| t.ssh == Some(id)) {
                    tab.color = color;
                }
            }
            Some(TabAction::StartItemRename(id)) => {
                if let Some(item) = self.item(id) {
                    self.item_rename = Some(ItemRename { id, text: item.name, error: None, fresh: true });
                }
            }
            Some(TabAction::RenameItem(id, text, confirmed)) => match self.rename_profile(id, &text) {
                Ok(()) => self.item_rename = None,
                Err(err) if confirmed => {
                    if let Some(r) = self.item_rename.as_mut() {
                        r.error = Some(err);
                    }
                }
                Err(_) => self.item_rename = None,
            },
            Some(TabAction::ToggleGroup(id)) => {
                if let Some(g) = self.config.groups.iter_mut().find(|g| g.id == id) {
                    g.collapsed = !g.collapsed;
                }
            }
            Some(TabAction::StartGroupRename(id)) => {
                if let Some(g) = self.config.groups.iter().find(|g| g.id == id) {
                    self.group_rename = Some((id, g.name.clone(), true));
                }
            }
            Some(TabAction::RenameGroup(id, name)) => {
                if let Some(g) = self.config.groups.iter_mut().find(|g| g.id == id).filter(|_| !name.is_empty()) {
                    g.name = name;
                }
                self.group_rename = None;
            }
            Some(TabAction::NewGroup(local)) => {
                let group = config::Group::new(t.new_group_name, local);
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::EditProfile(id)) => {
                // An open profile tab has the latest layout: save it first.
                self.sync();
                if let Some(p) = self.config.profiles.iter().find(|p| p.id == id) {
                    let cwds = p.tab.layout.cwds().into_iter().map(|c| c.map(|c| c.display().to_string()).unwrap_or_default()).collect();
                    self.profile_editor = Some(ProfileEditor {
                        id,
                        name: p.tab.name.clone().unwrap_or_default(),
                        color: p.tab.color,
                        cwds,
                        commands: p.commands.clone(),
                        new_command: String::new(),
                        error: None,
                    });
                }
            }
            Some(TabAction::NewGroupWith(item)) => {
                let local = self.config.profiles.iter().any(|p| p.id == item);
                let mut group = config::Group::new(t.new_group_name, local);
                self.config.unplace(item);
                group.items.push(item);
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::DeleteGroup(id)) => {
                // Its items stay, outside groups.
                if let Some(index) = self.config.groups.iter().position(|g| g.id == id) {
                    let group = self.config.groups.remove(index);
                    self.config.ungrouped.extend(group.items);
                }
            }
            Some(TabAction::DropItem(item, group, before)) => {
                self.config.unplace(item);
                let list = match group.and_then(|g| self.config.groups.iter_mut().find(|x| x.id == g)) {
                    Some(g) => &mut g.items,
                    None => &mut self.config.ungrouped,
                };
                let at = before.and_then(|b| list.iter().position(|x| *x == b)).unwrap_or(list.len());
                list.insert(at, item);
            }
            Some(TabAction::Reconnect(i)) => {
                if let Some(tab) = self.tabs.get(i) {
                    let panes = tab.layout.leaves();
                    self.reconnect(ui.ctx(), i, &panes);
                    self.select(i);
                }
            }
            Some(TabAction::DropGroup(id, before)) => {
                if let Some(index) = self.config.groups.iter().position(|g| g.id == id) {
                    let group = self.config.groups.remove(index);
                    let at = before.and_then(|b| self.config.groups.iter().position(|g| g.id == b)).unwrap_or(self.config.groups.len());
                    self.config.groups.insert(at, group);
                }
            }
            Some(TabAction::SetColor(i, c)) => {
                self.tabs[i].color = c;
                if let Some(host) = self.tabs[i].ssh.and_then(|id| self.config.ssh.iter_mut().find(|h| h.id == id)) {
                    host.color = c;
                }
            }
            Some(TabAction::StartRename(i)) => {
                self.rename = Some(Rename { tab: i, text: self.tabs[i].title().to_owned(), select_all: 3, error: None });
            }
            Some(TabAction::Rename(i, confirmed)) => {
                if let Some(mut r) = self.rename.take() {
                    let name = r.text.trim();
                    let tab = &self.tabs[i];
                    let taken = tab.ssh.is_none()
                        && !name.is_empty()
                        && self.config.profiles.iter().any(|p| Some(p.id) != tab.profile && p.tab.name.as_deref() == Some(name));
                    if taken {
                        // Clicking away cancels; Enter keeps the editor open with the reason.
                        if confirmed {
                            r.error = Some(self.t().name_taken);
                            self.rename = Some(r);
                        }
                        return;
                    }
                    self.tabs[i].name = (!name.is_empty()).then(|| name.to_owned());
                    if let Some(host) = self.tabs[i].ssh.and_then(|id| self.config.ssh.iter_mut().find(|h| h.id == id)) {
                        if !name.is_empty() {
                            host.name = name.to_owned();
                        }
                    } else if self.tabs[i].name.is_some() && self.tabs[i].profile.is_none() {
                        // Naming a tab means it's worth keeping: it becomes a profile right away.
                        self.save_as_profile(i);
                    }
                }
                self.focus_terminal = true;
            }
            Some(TabAction::DragOver(i, py)) => {
                // Swap with the neighbour as soon as the dragged tab's middle enters its slot.
                let target = tab_rects.iter().find(|(_, r)| py >= r.min.y && py <= r.max.y + ROW_GAP).map(|(t, _)| *t);
                if let Some(t) = target {
                    if t != i {
                        let tab = self.tabs.remove(i);
                        self.tabs.insert(t, tab);
                        if self.active == i {
                            self.active = t;
                        } else if i < self.active && self.active <= t {
                            self.active -= 1;
                        } else if t <= self.active && self.active < i {
                            self.active += 1;
                        }
                        self.rename = None;
                        // Keep the drag going on the tab's new slot.
                        ui.ctx().set_dragged_id(ui.id().with(("tab", t)));
                    }
                }
            }
            None => {}
        }
        });
    }
}
