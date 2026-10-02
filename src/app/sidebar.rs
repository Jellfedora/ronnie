//! The sidebar: logo, LOCAL terminals and profiles, SSH hosts and groups, update card, settings button.

use super::*;

impl App {
    /// Shown when every tab is closed: the app stays open.
    /// The home page: what just closed, the typing game, tips scrolling below (the sidebar leads back
    /// to the tabs).
    pub(super) fn home_page(&mut self, ui: &mut Ui, rect: Rect) {
        if !self.home_game {
            self.game.leave();
            self.fold_for_game(false);
            self.home_dashboard(ui, rect);
            return;
        }
        let t = self.t();
        let now = ui.input(|i| i.time);
        if let Some(line) = self.home.as_ref().filter(|l| !l.is_empty()) {
            let a = self.game.entrance(now, 0.15) * (1.0 - self.game.focus(ui.ctx()));
            ui.painter().text(Pos2::new(rect.center().x, rect.min.y + 22.0 - (1.0 - a) * 10.0), Align2::CENTER_CENTER, line, FontId::proportional(13.5), self.theme.text_muted.gamma_multiply(a));
        }
        let french = matches!(self.config.settings.language, Lang::Fr);
        // At the bottom, tips about Ronnie scroll by; the game above. They rise in with the page, and
        // sink away during a round (the game spreading over their room).
        let focus = self.game.focus(ui.ctx());
        let sunk = focus.max(1.0 - self.game.entrance(now, 0.5));
        let tips_h = if self.config.settings.home_tips { 44.0 } else { 0.0 };
        let game_rect = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.max.y - tips_h * (1.0 - focus)));
        if sunk < 0.999 && self.config.settings.home_tips {
            let ticker = Rect::from_min_size(Pos2::new(rect.min.x, rect.max.y - 44.0 + 44.0 * sunk), Vec2::new(rect.width(), 44.0));
            tips_ticker(ui, ticker, &self.theme, t, &self.config.settings.shortcuts);
        }
        let out = self.game.ui(ui, game_rect, &self.theme, t, &self.config.settings.typing_scores, french, self.config.settings.game_sound, &self.config.settings.player_name);
        // During a round the sidebar folds away, and unfolds after (if it was open).
        self.fold_for_game(self.game.playing());
        if let Some(on) = out.sound {
            self.config.settings.game_sound = Some(on);
            self.save_config();
        }
        if let Some(name) = out.player {
            self.config.settings.player_name = name;
            self.save_config();
        }
        if let Some(score) = out.finished {
            // On the board, below the rounds it ties with (stable sort).
            let scores = &mut self.config.settings.typing_scores;
            let before = scores.first().map_or(0, |s| s.letters);
            let unlocked = before < crate::theme::METAL_UNLOCK && score.letters >= crate::theme::METAL_UNLOCK;
            scores.push(score);
            scores.sort_by(|a, b| b.letters.cmp(&a.letters));
            scores.truncate(config::TYPING_SCORES);
            self.save_config();
            if unlocked {
                self.toasts.push(super::Toast { ok: true, title: t.metal_unlocked.to_owned(), body: t.metal_unlocked_body.to_owned(), tab: self.active, at: std::time::Instant::now() });
            }
        }
        // Back to the home page, out of a round.
        if !self.game.playing() {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(0.0, 0.0));
            let resp = ui.put(Rect::from_min_size(back.min, Vec2::new(110.0, 28.0)), egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0));
            if resp.clicked() {
                self.home_game = false;
            }
        }

    }

    /// The logo leads to the home page; three clicks, to the typing game.
    fn logo_clicked(&mut self, logo: &egui::Response) {
        if logo.triple_clicked() {
            if self.home.is_none() {
                self.go_home(None);
            }
            self.home_game = true;
        } else if logo.clicked() && (self.home.is_none() || self.home_game) && !logo.double_clicked() {
            self.go_home(None);
        }
    }

    /// Folds the sidebar for a round of the game (`playing`), unfolds it after if the game folded it.
    pub(super) fn fold_for_game(&mut self, playing: bool) {
        match (playing, self.game_folded) {
            (true, false) if !self.config.settings.sidebar_folded => {
                self.config.settings.sidebar_folded = true;
                self.game_folded = true;
            }
            (false, true) => {
                self.config.settings.sidebar_folded = false;
                self.game_folded = false;
            }
            _ => {}
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
        let collapsed = self.section_title(ui, header, t.profiles, true, RailSection::Ssh);
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        let plus = icon_button(ui, &painter, plus_rect, "profiles-plus", &self.theme, paint_plus);
        egui::Popup::menu(&plus).width(210.0).show(|ui| {
            menu_item(ui, t.new_host, TabAction::NewHost, action);
            menu_item(ui, t.new_group, TabAction::NewGroup(false), action);
        });
        *y += SECTION_HEADER_H;
        if collapsed {
            return;
        }
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
            painter.rect_filled(rect, 7.0, self.theme.tab_hover.gamma_multiply(0.7));
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
        // How many items, in a small pill.
        {
            let galley = painter.layout_no_wrap(count.to_string(), FontId::proportional(10.5), self.theme.text_muted);
            let pill = Rect::from_center_size(Pos2::new(rect.max.x - 8.0 - (galley.size().x + 12.0) / 2.0, rect.center().y), Vec2::new(galley.size().x + 12.0, 16.0));
            painter.rect_filled(pill, 8.0, self.theme.tab_hover.gamma_multiply(if hovered { 1.4 } else { 0.9 }));
            painter.galley(pill.center() - galley.size() / 2.0, galley, self.theme.text_muted);
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
        let active = item.open.is_some() && item.open == self.shown_tab();
        paint_row_bg(painter, rect, active || dragged, hovered, &self.theme);
        let dot = Pos2::new(rect.min.x + 17.0, rect.center().y);
        let live = item.open.and_then(|i| self.live.get(i).cloned().flatten());
        if live.is_some() {
            paint_live(ui, painter, dot, &self.theme);
        }
        paint_badge(painter, dot, &item.name, item.color, item.ssh, item.open.is_some(), active, &self.theme);

        let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
        let show_close = item.open.is_some() && (active || hovered);
        // A long command ended in its tab while it wasn't shown: ✓ or ✗ where the ✕ goes.
        let done = item.open.and_then(|i| self.tabs.get(i)).and_then(|tab| tab.done.as_ref()).map(|d| (d.ok, d.summary.clone()));
        if let Some((ok, _)) = done.as_ref().filter(|_| !show_close) {
            paint_done(painter, close_rect.center(), *ok, &self.theme);
        }
        let edit_rect = if show_close { close_rect.translate(Vec2::new(-20.0, 0.0)) } else { close_rect };
        let show_edit = hovered && item.ssh;
        let text_right = if show_edit {
            edit_rect.min.x - 4.0
        } else if show_close {
            close_rect.min.x - 4.0
        } else {
            rect.max.x - 34.0
        };
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + 36.0, rect.min.y), Pos2::new(text_right, rect.max.y));

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

        let mut hint = item.hint.clone();
        if let Some(program) = &live {
            hint.push_str(&format!("\n▶ {program}"));
        }
        if let Some((_, summary)) = &done {
            hint.push_str(&format!("\n{summary}"));
        }
        let resp = resp.on_hover_text(hint);
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

    /// The folded sidebar: a button to unfold it, then everything the sidebar lists as badges (the
    /// open terminals and local profiles, the SSH hosts, the databases), each with its "+", and the settings.
    pub(super) fn sidebar_rail(&mut self, ui: &mut Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        self.live = self.tabs.iter_mut().map(|tab| tab.panes.values_mut().find_map(|term| term.live_program(&ctx).map(str::to_owned))).collect();
        self.config.normalize();
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, self.theme.chrome_bg);
        ui.painter().vline(bar.max.x - 0.5, bar.y_range(), Stroke::new(1.0, self.theme.tab_hover));
        // Empty space drags the window, as in the sidebar.
        let bg = ui.interact(bar, ui.id().with("rail-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let top = if cfg!(target_os = "macos") { SIDEBAR_TOP / ui.ctx().zoom_factor() } else { SIDEBAR_TOP };
        let cx = bar.center().x;
        // The R of the logo, where the logo sits unfolded.
        let r_at = Pos2::new(cx, bar.min.y + top + LOGO_H / 2.0 - 2.0);
        super::paint_metal(ui.painter(), r_at, Align2::CENTER_CENTER, "R", 32.0, self.theme.accent, 1.0);
        let logo = ui.interact(Rect::from_center_size(r_at, Vec2::splat(34.0)), ui.id().with("rail-home"), Sense::click()).on_hover_text(t.home_tip).on_hover_cursor(egui::CursorIcon::PointingHand);
        self.logo_clicked(&logo);
        let unfold = Rect::from_center_size(Pos2::new(cx, bar.min.y + top + LOGO_H + 10.0), Vec2::splat(26.0));
        if icon_button(ui, ui.painter(), unfold, "unfold-sidebar", &self.theme, paint_unfold).on_hover_text(format!("{}  ({})", t.unfold_sidebar, self.config.settings.shortcuts.toggle_sidebar.label())).clicked() {
            self.config.settings.sidebar_folded = false;
        }

        // What the sidebar lists, section by section.
        let kind = |local: bool| {
            let of_kind = |id: &Uuid| if local { self.config.profiles.iter().any(|p| p.id == *id) } else { self.config.ssh.iter().any(|h| h.id == *id) };
            let mut ids: Vec<Uuid> = self.config.ungrouped.iter().copied().filter(of_kind).collect();
            for g in self.config.groups.iter().filter(|g| g.local == local) {
                ids.extend(g.items.iter().copied().filter(of_kind));
            }
            ids
        };
        let plain: Vec<RailEntry> = (0..self.tabs.len())
            .filter(|&i| {
                let tab = &self.tabs[i];
                tab.ssh.is_none() && tab.profile.is_none() && tab.db.is_none()
            })
            .map(RailEntry::Tab)
            .collect();
        let sections: Vec<(RailSection, &str, Vec<RailEntry>)> = vec![
            (RailSection::Local, t.terminals, plain.into_iter().chain(kind(true).into_iter().map(RailEntry::Item)).collect()),
            (RailSection::Ssh, t.profiles, kind(false).into_iter().map(RailEntry::Item).collect()),
            (RailSection::Db, t.db_section, self.config.databases.iter().map(|c| RailEntry::Db(c.id)).collect()),
        ];

        let footer = bar.max.y - 54.0;
        let list = Rect::from_min_max(Pos2::new(bar.min.x, unfold.max.y + 8.0), Pos2::new(bar.max.x - 1.0, footer));
        let mut action: Option<TabAction> = None;
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
        list_ui.spacing_mut().scroll = egui::style::ScrollStyle { bar_width: 4.0, floating_allocated_width: 2.0, dormant_handle_opacity: 0.5, ..egui::style::ScrollStyle::thin() };
        egui::ScrollArea::vertical().id_salt("rail-scroll").auto_shrink(false).show(&mut list_ui, |ui| {
            let origin = ui.max_rect().min;
            let mut y = origin.y;
            for (k, (section, title, entries)) in sections.iter().enumerate() {
                // Each section: a line, its "+" (with the section's name on hover), then its badges.
                if k > 0 {
                    ui.painter().hline(bar.min.x + 14.0..=bar.max.x - 14.0, y + 2.0, Stroke::new(1.0, self.theme.tab_hover));
                    y += 8.0;
                }
                // The section's icon (lit when the tab shown is in it), and its "+".
                let here = self.shown_tab().is_some() && entries.iter().any(|e| self.rail_open(*e) == self.shown_tab());
                let icon_rect = Rect::from_center_size(Pos2::new(cx - 11.0, y + 12.0), Vec2::splat(22.0));
                let color = if here { self.theme.accent } else { self.theme.text_muted };
                match section {
                    RailSection::Local => paint_prompt_icon(ui.painter(), icon_rect.center(), color),
                    RailSection::Ssh => paint_server_icon(ui.painter(), icon_rect.center(), color),
                    RailSection::Db => super::dbview::paint_db_icon(ui.painter(), icon_rect.center(), color),
                }
                let collapsed = *self.collapsed(*section);
                let icon = ui.interact(icon_rect, ui.id().with(("rail-section", k)), Sense::click());
                if collapsed || icon.hovered() {
                    ui.painter().rect_filled(icon_rect, 5.0, self.theme.tab_hover.gamma_multiply(if icon.hovered() { 1.0 } else { 0.6 }));
                }
                let hint = if collapsed { format!("{title} ({})  ·  {}", entries.len(), t.unfold_section) } else { format!("{title}  ·  {}", t.fold_section) };
                if icon.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    let c = self.collapsed(*section);
                    *c = !*c;
                }
                let plus_rect = Rect::from_center_size(Pos2::new(cx + 13.0, y + 12.0), Vec2::splat(20.0));
                y += 26.0;
                let plus = icon_button(ui, ui.painter(), plus_rect, &format!("rail-plus-{k}"), &self.theme, paint_plus);
                match section {
                    RailSection::Local => {
                        if plus.on_hover_text(format!("{}  ·  {} ({})", title, t.new_tab, self.config.settings.shortcuts.new_tab.label())).clicked() {
                            action = Some(TabAction::New);
                        }
                    }
                    RailSection::Ssh => {
                        let plus = plus.on_hover_text(*title);
                        egui::Popup::menu(&plus).width(210.0).show(|ui| {
                            menu_item(ui, t.new_host, TabAction::NewHost, &mut action);
                            menu_item(ui, t.new_group, TabAction::NewGroup(false), &mut action);
                        });
                    }
                    RailSection::Db => {
                        if plus.on_hover_text(format!("{}  ·  {}", title, t.db_new_connection)).clicked() {
                            action = Some(if self.config.databases.is_empty() && self.local_db { TabAction::AddLocalDb } else { TabAction::NewDb });
                        }
                    }
                }
                for entry in entries.iter().filter(|_| !collapsed) {
                    let slot = Rect::from_center_size(Pos2::new(cx, y + 18.0), Vec2::new(46.0, 36.0));
                    y += 40.0;
                    self.rail_entry(ui, *entry, slot, bar.min.x, &mut action);
                }
            }
            ui.allocate_rect(Rect::from_min_max(origin, Pos2::new(origin.x + 1.0, y + 6.0)), Sense::hover());
        });

        ui.painter().hline(bar.min.x + 16.0..=bar.max.x - 16.0, footer + 2.0, Stroke::new(1.0, self.theme.tab_hover));
        let gear = Rect::from_center_size(Pos2::new(cx, bar.max.y - 26.0), Vec2::splat(32.0));
        let settings = ui.interact(gear, ui.id().with("rail-settings"), Sense::click());
        if settings.hovered() || self.settings_dialog {
            ui.painter().rect_filled(gear, 8.0, self.theme.tab_active);
        }
        paint_gear(ui.painter(), gear.center(), self.theme.accent);
        if settings.on_hover_text(t.settings).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.settings_dialog = true;
        }
        self.apply_tab_action(ui, action, &[]);
    }

    /// Whether a section of the sidebar is folded to its title.
    fn collapsed(&mut self, section: RailSection) -> &mut bool {
        let s = &mut self.config.settings;
        match section {
            RailSection::Local => &mut s.local_collapsed,
            RailSection::Ssh => &mut s.ssh_collapsed,
            RailSection::Db => &mut s.db_collapsed,
        }
    }

    /// A section's title (with its chevron), which folds or unfolds it when clicked. `plus`: room kept
    /// on the right for its "+". True when the section is folded.
    fn section_title(&mut self, ui: &Ui, rect: Rect, title: &str, plus: bool, section: RailSection) -> bool {
        let hit = Rect::from_min_max(rect.min, Pos2::new(if plus { rect.max.x - 26.0 } else { rect.max.x }, rect.max.y));
        let resp = ui.interact(hit, ui.id().with(("section-title", section as u8)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            let c = self.collapsed(section);
            *c = !*c;
        }
        let collapsed = *self.collapsed(section);
        paint_section_title(ui.painter(), rect, title, plus, collapsed, resp.hovered(), &self.theme);
        collapsed
    }

    /// The tab an entry of the folded sidebar is open in.
    fn rail_open(&self, entry: RailEntry) -> Option<usize> {
        match entry {
            RailEntry::Tab(i) => Some(i),
            RailEntry::Item(id) => self.item(id).and_then(|item| item.open),
            RailEntry::Db(id) => self.tabs.iter().position(|tab| tab.db == Some(id)),
        }
    }

    /// One badge of the folded sidebar: an open tab, a profile or host, or a database connection.
    fn rail_entry(&self, ui: &mut Ui, entry: RailEntry, slot: Rect, left: f32, action: &mut Option<TabAction>) {
        let t = self.t();
        let (name, color, ssh, open, hint, click) = match entry {
            RailEntry::Tab(i) => {
                let tab = &self.tabs[i];
                (tab.title().to_owned(), tab.color, false, Some(i), tab.title().to_owned(), TabAction::Select(i))
            }
            RailEntry::Item(id) => {
                let Some(item) = self.item(id) else { return };
                let hint = format!("{}\n{}", item.name, item.hint);
                (item.name, item.color, item.ssh, item.open, hint, TabAction::OpenItem(id))
            }
            RailEntry::Db(id) => {
                let Some(c) = self.config.databases.iter().find(|c| c.id == id) else { return };
                let open = self.tabs.iter().position(|tab| tab.db == Some(id));
                (c.name.clone(), c.color, false, open, format!("{}\n{}", c.name, c.address()), TabAction::OpenDb(id))
            }
        };
        let resp = ui.interact(slot, ui.id().with(("rail", entry)), Sense::click());
        let active = open.is_some() && open == self.shown_tab();
        if active {
            ui.painter().rect_filled(slot, 9.0, self.theme.tab_active);
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(left + 3.0, slot.min.y + 8.0), Vec2::new(3.0, slot.height() - 16.0)), 1.5, self.theme.accent);
        } else if resp.hovered() {
            ui.painter().rect_filled(slot, 9.0, self.theme.tab_hover.gamma_multiply(0.75));
        }
        if open.and_then(|i| self.live.get(i)).is_some_and(|l| l.is_some()) {
            paint_live(ui, ui.painter(), slot.center(), &self.theme);
        }
        paint_badge(ui.painter(), slot.center(), &name, color, ssh, open.is_some(), active, &self.theme);
        let done = open.and_then(|i| self.tabs.get(i)).and_then(|tab| tab.done.as_ref());
        if let Some(done) = done {
            paint_done(ui.painter(), slot.right_top() + Vec2::new(-6.0, 7.0), done.ok, &self.theme);
        }
        let hint = match done {
            Some(d) => format!("{hint}\n{}", d.summary),
            None => hint,
        };
        let resp = resp.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            *action = Some(click);
        }
        resp.context_menu(|ui| match entry {
            RailEntry::Tab(i) => self.tab_menu(ui, i, action),
            RailEntry::Item(id) => {
                if let Some(item) = self.item(id) {
                    self.item_menu(ui, id, &item, action);
                }
            }
            RailEntry::Db(id) => {
                ui.set_min_width(170.0);
                menu_item(ui, t.connect, TabAction::OpenDb(id), action);
                menu_item(ui, t.edit, TabAction::EditDb(id), action);
                ui.separator();
                if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                    *action = Some(TabAction::DeleteDb(id));
                    ui.close();
                }
            }
        });
    }

    /// The "Databases" section: the saved connections (the local server offered while there is none).
    pub(super) fn db_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>) {
        let t = self.t();
        let painter = ui.painter().clone();
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        let collapsed = self.section_title(ui, header, t.db_section, true, RailSection::Db);
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        if icon_button(ui, &painter, plus_rect, "db-plus", &self.theme, paint_plus).on_hover_text(t.db_new_connection).clicked() {
            *action = Some(TabAction::NewDb);
        }
        *y += SECTION_HEADER_H;
        if collapsed {
            return;
        }
        if self.config.databases.is_empty() {
            if self.local_db {
                // The button fits its label; the text before it goes on two lines when the sidebar is
                // narrow, the card growing with it.
                let label_w = painter.layout_no_wrap(t.db_add.to_owned(), FontId::proportional(12.0), self.theme.bg).size().x;
                let text_w = row_w - label_w - 24.0 - 36.0;
                let mut job = egui::text::LayoutJob::simple(t.db_local_found.to_owned(), FontId::proportional(12.0), self.theme.text, text_w);
                job.wrap.max_rows = 2;
                let galley = painter.layout_job(job);
                let r = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, (ROW_H + 6.0).max(galley.size().y + 14.0)));
                painter.rect_filled(r, 8.0, self.theme.accent.gamma_multiply(0.08));
                painter.rect_stroke(r, 8.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.35)), egui::StrokeKind::Inside);
                super::dbview::paint_db_icon(&painter, Pos2::new(r.min.x + 16.0, r.center().y), self.theme.accent);
                let button = Rect::from_min_size(Pos2::new(r.max.x - label_w - 24.0, r.center().y - 11.0), Vec2::new(label_w + 16.0, 22.0));
                painter.galley(Pos2::new(r.min.x + 30.0, r.center().y - galley.size().y / 2.0), galley, self.theme.text);
                ui.interact(Rect::from_min_max(r.min, Pos2::new(button.min.x - 4.0, r.max.y)), ui.id().with("db-local-hint"), Sense::hover()).on_hover_text(t.db_local_found);
                let add = ui.put(button, egui::Button::new(egui::RichText::new(t.db_add).size(12.0).color(self.theme.bg)).fill(self.theme.accent).corner_radius(5.0));
                if add.clicked() {
                    *action = Some(TabAction::AddLocalDb);
                }
                *y += r.height() + 6.0;
            } else {
                let hint = painter.layout(t.db_no_connections.to_owned(), FontId::proportional(12.0), self.theme.text_muted.gamma_multiply(0.7), row_w - 12.0);
                let h = hint.size().y;
                painter.galley(Pos2::new(left + 6.0, *y + 4.0), hint, self.theme.text_muted);
                *y += h + 12.0;
            }
            return;
        }
        for c in self.config.databases.clone() {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H));
            *y += ROW_H + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("db-item", c.id)), Sense::click());
            let open = self.tabs.iter().position(|tab| tab.db == Some(c.id));
            let active = open.is_some() && open == self.shown_tab();
            let hovered = resp.contains_pointer();
            paint_row_bg(&painter, slot, active, hovered, &self.theme);
            let dot = Pos2::new(slot.min.x + 17.0, slot.center().y);
            paint_badge(&painter, dot, &c.name, c.color, false, open.is_some(), active, &self.theme);
            let close_rect = Rect::from_center_size(Pos2::new(slot.max.x - 14.0, slot.center().y), Vec2::splat(18.0));
            let show_close = open.is_some() && (active || hovered);
            let color = if active { self.theme.text } else if open.is_some() || hovered { self.theme.text_muted } else { self.theme.text_muted.gamma_multiply(0.75) };
            let text_rect = Rect::from_min_max(Pos2::new(slot.min.x + 36.0, slot.min.y), Pos2::new(if show_close { close_rect.min.x - 4.0 } else { slot.max.x - 10.0 }, slot.max.y));
            let mut job = egui::text::LayoutJob::simple_singleline(c.name.clone(), FontId::proportional(13.0), color);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
            if show_close {
                let close = ui.interact(close_rect, ui.id().with(("db-close", c.id)), Sense::click());
                if close.hovered() {
                    painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
                }
                paint_cross(&painter, close_rect.center(), if close.hovered() { self.theme.text } else { self.theme.text_muted });
                if close.clicked() {
                    *action = open.map(TabAction::Close);
                }
            }
            let resp = resp.on_hover_text(c.address());
            if resp.double_clicked() {
                *action = Some(TabAction::EditDb(c.id));
            } else if resp.clicked() {
                action.get_or_insert(TabAction::OpenDb(c.id));
            }
            resp.context_menu(|ui| {
                ui.set_min_width(170.0);
                menu_item(ui, t.connect, TabAction::OpenDb(c.id), action);
                menu_item(ui, t.edit, TabAction::EditDb(c.id), action);
                ui.separator();
                if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                    *action = Some(TabAction::DeleteDb(c.id));
                    ui.close();
                }
            });
        }
    }

    /// Removes a database connection (its tab closes, its password is forgotten).
    pub(super) fn delete_db(&mut self, id: Uuid) {
        if let Some(i) = self.tabs.iter().position(|t| t.db == Some(id)) {
            self.close_tab(i);
        }
        self.config.databases.retain(|c| c.id != id);
        ssh::delete_password(id);
    }

    /// The profile editor on profile `id`.
    pub(super) fn open_profile_editor(&mut self, id: Uuid) {
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
        menu_item(ui, t.duplicate, TabAction::Duplicate(id), action);
        menu_item(ui, &format!("⧉  {}", t.new_window_open), TabAction::ItemNewWindow(id), action);
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
    pub(super) fn section_header(&mut self, ui: &Ui, title: &str, pos: Pos2, width: f32, plus_hint: Option<&str>, section: RailSection) -> bool {
        let rect = Rect::from_min_size(pos, Vec2::new(width, SECTION_HEADER_H));
        self.section_title(ui, rect, title, plus_hint.is_some(), section);
        let painter = ui.painter();
        let Some(hint) = plus_hint else { return false };
        let plus_rect = Rect::from_center_size(Pos2::new(rect.max.x - 12.0, rect.center().y), Vec2::splat(20.0));
        let plus = ui.interact(plus_rect, ui.id().with(("section-plus", title)), Sense::click());
        if plus.hovered() {
            painter.circle_filled(plus_rect.center(), 10.0, self.theme.accent.gamma_multiply(0.25));
        }
        let stroke = Stroke::new(1.6, if plus.hovered() { self.theme.accent } else { self.theme.text_muted });
        let (c, d) = (plus_rect.center(), 5.0);
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
        item(ui, &format!("⧉  {}", t.new_window_move), TabAction::TabNewWindow(i));
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
            // The progress on its own line, under.
            update::State::Installing(v) => (t.installing.replace("{v}", &v), None),
            update::State::Installed(v) => (t.update_installed.replace("{v}", &v), None),
            update::State::Failed(e) if self.update_attempted => (t.update_failed.to_owned(), Some(e)),
            _ => return area.max.y,
        };
        let state = self.updater.state();
        let buttons = !matches!(state, update::State::Installing(_));
        let installing = matches!(state, update::State::Installing(_));
        let h = if buttons { 78.0 } else { 58.0 };
        let card = Rect::from_min_max(Pos2::new(area.min.x, area.max.y - h - 6.0), Pos2::new(area.max.x, area.max.y - 6.0));
        let painter = ui.painter();
        painter.rect_filled(card, 8.0, self.theme.tab_active);
        painter.rect_stroke(card, 8.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        let mut job = egui::text::LayoutJob::simple_singleline(title, FontId::proportional(13.0), self.theme.text);
        // Room kept for the spinner on the right while installing.
        job.wrap = egui::text::TextWrapping::truncate_at_width(card.width() - if installing { 52.0 } else { 24.0 });
        painter.galley(card.min + Vec2::new(12.0, 11.0), painter.layout_job(job), self.theme.text);
        let hover = ui.interact(card, ui.id().with("update-card"), Sense::hover());
        if let Some(e) = &detail {
            hover.on_hover_text(e);
        }
        if matches!(state, update::State::Installing(_)) {
            ui.put(Rect::from_center_size(Pos2::new(card.max.x - 20.0, card.min.y + 20.0), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
            // How much has arrived: in words, and a bar.
            let progress = download_text(self.updater.progress(), t);
            if !progress.is_empty() {
                let mut job = egui::text::LayoutJob::simple_singleline(progress, FontId::proportional(11.5), self.theme.text_muted);
                job.wrap = egui::text::TextWrapping::truncate_at_width(card.width() - 24.0);
                ui.painter().galley(Pos2::new(card.min.x + 12.0, card.min.y + 30.0), ui.painter().layout_job(job), self.theme.text_muted);
            }
            let (done, total) = self.updater.progress();
            if total > 0 {
                let bar = Rect::from_min_max(Pos2::new(card.min.x + 12.0, card.max.y - 10.0), Pos2::new(card.max.x - 12.0, card.max.y - 7.0));
                ui.painter().rect_filled(bar, 1.5, self.theme.tab_hover);
                let filled = Rect::from_min_max(bar.min, Pos2::new(bar.min.x + bar.width() * (done as f32 / total as f32).min(1.0), bar.max.y));
                ui.painter().rect_filled(filled, 1.5, self.theme.accent);
            }
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
        // A slight gradient, darker at the bottom.
        {
            let (top, bottom) = (self.theme.chrome_bg, lerp_color(self.theme.chrome_bg, self.theme.bg, 0.55));
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(bar.left_top(), top);
            mesh.colored_vertex(bar.right_top(), top);
            mesh.colored_vertex(bar.right_bottom(), bottom);
            mesh.colored_vertex(bar.left_bottom(), bottom);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(egui::Shape::mesh(mesh));
        }
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
        // macOS: the traffic lights don't scale with the interface zoom, so the space kept for them doesn't either.
        let top = if cfg!(target_os = "macos") { SIDEBAR_TOP / ui.ctx().zoom_factor() } else { SIDEBAR_TOP };
        let logo_rect = Rect::from_min_size(Pos2::new(bar.min.x, bar.min.y + top), Vec2::new(bar.width() - 1.0, LOGO_H));
        paint_logo(ui.painter(), logo_rect, &self.theme);
        // The logo leads home (the typing game).
        let logo_hit = Rect::from_center_size(logo_rect.center(), Vec2::new(150.0, logo_rect.height()));
        let logo = ui.interact(logo_hit, ui.id().with("logo-home"), Sense::click()).on_hover_text(t.home_tip).on_hover_cursor(egui::CursorIcon::PointingHand);
        self.logo_clicked(&logo);
        // Dev builds say so, next to the logo: they don't share the installed app's profiles.
        if !config::OFFICIAL {
            let at = Pos2::new(logo_rect.center().x + 58.0, logo_rect.center().y - 12.0);
            let galley = ui.painter().layout_no_wrap("DEV".to_owned(), FontId::monospace(9.5), self.theme.bg);
            let badge = Rect::from_min_size(at, galley.size() + Vec2::new(8.0, 2.0));
            ui.painter().rect_filled(badge, 3.0, self.theme.ansi[3]);
            ui.painter().galley(badge.min + Vec2::new(4.0, 1.0), galley, self.theme.bg);
        }
        // Fold into a rail («), on the row of the window buttons.
        let fold_rect = Rect::from_center_size(Pos2::new(bar.max.x - 20.0, bar.min.y + if cfg!(target_os = "macos") { 14.0 } else { 18.0 }), Vec2::splat(22.0));
        if icon_button(ui, ui.painter(), fold_rect, "fold-sidebar", &self.theme, paint_fold).on_hover_text(format!("{}  ({})", t.fold_sidebar, self.config.settings.shortcuts.toggle_sidebar.label())).clicked() {
            self.config.settings.sidebar_folded = true;
        }
        let card_top = self.update_card(ui, Rect::from_min_max(Pos2::new(left, bar.min.y), Pos2::new(left + row_w, footer_top)));
        let scroll_rect = Rect::from_min_max(Pos2::new(bar.min.x, logo_rect.max.y), Pos2::new(bar.max.x - 1.0, card_top));

        // Footer: settings button, always visible.
        let button = Rect::from_min_max(Pos2::new(left, footer_top + 5.0), Pos2::new(left + row_w, bar.max.y - 7.0));
        let settings = ui.interact(button, ui.id().with("settings-btn"), Sense::click());
        let hot = settings.hovered() || self.settings_dialog;
        ui.painter().hline(bar.min.x + 12.0..=bar.max.x - 12.0, footer_top, Stroke::new(1.0, self.theme.tab_hover.gamma_multiply(0.8)));
        ui.painter().rect_filled(button, 8.0, if hot { self.theme.tab_active } else { Color32::TRANSPARENT });
        if hot {
            ui.painter().rect_stroke(button, 8.0, Stroke::new(1.0, self.theme.accent.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        }
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
        if self.section_header(ui, t.terminals, Pos2::new(left, y), row_w, Some(&new_hint), RailSection::Local) {
            action = Some(TabAction::New);
        }
        y += SECTION_HEADER_H;

        let local_collapsed = self.config.settings.local_collapsed;
        let is_plain = |tab: &Tab| tab.ssh.is_none() && tab.profile.is_none() && tab.db.is_none() && !local_collapsed;
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
            let active = Some(i) == self.shown_tab();
            let tab = &self.tabs[i];
            // Not `hovered()`: that turns false when the pointer is over the ✕ (another widget), which
            // would hide the ✕ as soon as the pointer reaches it.
            let row_hovered = resp.contains_pointer();

            paint_row_bg(&painter, rect, active || dragging, row_hovered, &self.theme);
            let dot = Pos2::new(rect.min.x + 17.0, rect.center().y);
            let live = self.live.get(i).cloned().flatten();
            if live.is_some() {
                paint_live(ui, &painter, dot, &self.theme);
            }
            paint_badge(&painter, dot, tab.title(), tab.color, false, true, active, &self.theme);

            let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
            let show_close = active || row_hovered;
            let text_left = rect.min.x + 36.0;
            // A long command ended in this tab while it wasn't shown: ✓ or ✗ where the ✕ goes.
            let done = tab.done.as_ref().filter(|_| !show_close);
            if let Some(done) = done {
                paint_done(&painter, close_rect.center(), done.ok, &self.theme);
            }

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
                let text_right = if show_close || done.is_some() { close_rect.min.x - 4.0 } else { rect.max.x - 10.0 };
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
            let hint = [live.map(|program| format!("▶ {program}")), self.tabs[i].done.as_ref().map(|d| d.summary.clone())];
            let hint: Vec<String> = hint.into_iter().flatten().collect();
            let resp = if hint.is_empty() { resp } else { resp.on_hover_text(hint.join("\n")) };
            resp.context_menu(|ui| self.tab_menu(ui, i, &mut action));
        }

        // Local profiles sit with the terminals, in their own groups; the SSH section below lists hosts.
        if !local_collapsed {
            self.profiles_section(ui, left, row_w, &mut y, &mut action, true);
        }

        y += SECTION_GAP;
        self.profiles_section(ui, left, row_w, &mut y, &mut action, false);

        y += SECTION_GAP;
        self.db_section(ui, left, row_w, &mut y, &mut action);

        // Content height, so the scroll area knows how far it can go.
        ui.allocate_rect(Rect::from_min_max(origin, Pos2::new(origin.x + 1.0, y + ROW_H + SIDEBAR_PAD)), Sense::hover());

        self.apply_tab_action(ui, action, &tab_rects);
        });
    }

    /// Carries out what was clicked in the sidebar (folded or not). `tab_rects`: the slots of the
    /// local tabs, for a tab dragged over them.
    pub(super) fn apply_tab_action(&mut self, ui: &Ui, action: Option<TabAction>, tab_rects: &[(usize, Rect)]) {
        let t = self.t();
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
            Some(TabAction::EditProfile(id)) => self.open_profile_editor(id),
            Some(TabAction::NewGroupWith(item)) => {
                let local = self.config.profiles.iter().any(|p| p.id == item);
                let mut group = config::Group::new(t.new_group_name, local);
                self.config.unplace(item);
                group.items.push(item);
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::Duplicate(id)) => self.duplicate_item(id),
            Some(TabAction::NewDb) => self.db_editor = Some(DbEditor::new(config::DbConnection::new(), true)),
            Some(TabAction::AddLocalDb) => {
                let mut c = config::DbConnection::new();
                c.name = "Local".into();
                c.user = std::env::var("USER").unwrap_or_default();
                self.db_editor = Some(DbEditor::new(c, true));
            }
            Some(TabAction::OpenDb(id)) => self.open_db(id),
            Some(TabAction::EditDb(id)) => {
                if let Some(c) = self.config.databases.iter().find(|c| c.id == id) {
                    self.db_editor = Some(DbEditor::new(c.clone(), false));
                }
            }
            Some(TabAction::DeleteDb(id)) => self.delete_db(id),
            Some(TabAction::ItemNewWindow(id)) => self.item_to_new_window(id),
            Some(TabAction::TabNewWindow(i)) => self.tab_to_new_window(i),
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
    }
}

/// ✓ (green) or ✗ (red) in a soft disc: a long command ended in a tab that wasn't shown.
fn paint_done(painter: &egui::Painter, c: Pos2, ok: bool, theme: &crate::theme::Theme) {
    let color = if ok { theme.ansi[2] } else { theme.ansi[1] };
    painter.circle_filled(c, 7.5, color.gamma_multiply(0.18));
    let stroke = Stroke::new(1.6, color);
    if ok {
        painter.line_segment([c + Vec2::new(-3.5, 0.2), c + Vec2::new(-1.0, 2.8)], stroke);
        painter.line_segment([c + Vec2::new(-1.0, 2.8), c + Vec2::new(3.8, -2.6)], stroke);
    } else {
        let d = 3.0;
        painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
        painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
    }
}

/// Background of a sidebar row: nothing at rest, a light fill on hover, and for the open one a fill
/// with a touch of the accent and a bar on its left.
fn paint_row_bg(painter: &egui::Painter, rect: Rect, active: bool, hovered: bool, theme: &crate::theme::Theme) {
    if active {
        painter.rect_filled(rect, 8.0, theme.tab_active);
        painter.rect_filled(rect, 8.0, theme.accent.gamma_multiply(0.07));
        let bar = Rect::from_min_size(Pos2::new(rect.min.x, rect.min.y + 8.0), Vec2::new(3.0, rect.height() - 16.0));
        painter.rect_filled(bar, 1.5, theme.accent);
    } else if hovered {
        painter.rect_filled(rect, 8.0, theme.tab_hover.gamma_multiply(0.75));
    }
}

/// The badge before a name: its initial on its color (a rounded square for terminals and profiles, a
/// circle for SSH hosts), with a small green light when it is open.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_badge(painter: &egui::Painter, c: Pos2, name: &str, color: Option<Color32>, ssh: bool, open: bool, active: bool, theme: &crate::theme::Theme) {
    let tint = color.unwrap_or(theme.text_muted);
    let r = Rect::from_center_size(c, Vec2::splat(20.0));
    let fill = tint.gamma_multiply(if active { 0.32 } else if open { 0.22 } else { 0.12 });
    if ssh {
        painter.circle_filled(c, 10.0, fill);
    } else {
        painter.rect_filled(r, 6.0, fill);
    }
    let initial: String = name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_else(|| "·".into());
    let text = if open || active { tint } else { tint.gamma_multiply(0.75) };
    painter.text(c + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, initial, FontId::proportional(11.0), text);
    if open && ssh {
        let dot = c + Vec2::new(7.0, 7.0);
        painter.circle_filled(dot, 3.5, theme.chrome_bg);
        painter.circle_filled(dot, 2.5, theme.ansi[2]);
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from(egui::lerp(egui::Rgba::from(a)..=egui::Rgba::from(b), t))
}

/// A sidebar section's title: small spaced capitals, then a thin line (up to the + button).
/// A section's title, after the chevron that folds it.
fn paint_section_title(painter: &egui::Painter, rect: Rect, title: &str, plus: bool, collapsed: bool, hovered: bool, theme: &crate::theme::Theme) {
    let color = if hovered { theme.text } else { theme.text_muted.gamma_multiply(0.85) };
    paint_chevron(painter, Pos2::new(rect.min.x + 9.0, rect.center().y), !collapsed, color);
    let mut job = egui::text::LayoutJob::default();
    job.append(&title.to_uppercase(), 0.0, egui::TextFormat { font_id: FontId::proportional(10.5), color, extra_letter_spacing: 1.2, ..Default::default() });
    let galley = painter.layout_job(job);
    let text_end = rect.min.x + 20.0 + galley.size().x;
    painter.galley(Pos2::new(rect.min.x + 20.0, rect.center().y - galley.size().y / 2.0), galley, theme.text_muted);
    let line_end = if plus { rect.max.x - 28.0 } else { rect.max.x - 6.0 };
    if line_end > text_end + 10.0 {
        painter.hline(text_end + 8.0..=line_end, rect.center().y, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.9)));
    }
}

/// « : fold the sidebar.
fn paint_fold(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    for dx in [-2.5, 2.5] {
        painter.line_segment([c + Vec2::new(dx + 2.0, -4.0), c + Vec2::new(dx - 2.0, 0.0)], stroke);
        painter.line_segment([c + Vec2::new(dx - 2.0, 0.0), c + Vec2::new(dx + 2.0, 4.0)], stroke);
    }
}

/// »: unfold it.
fn paint_unfold(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    for dx in [-2.5, 2.5] {
        painter.line_segment([c + Vec2::new(dx - 2.0, -4.0), c + Vec2::new(dx + 2.0, 0.0)], stroke);
        painter.line_segment([c + Vec2::new(dx + 2.0, 0.0), c + Vec2::new(dx - 2.0, 4.0)], stroke);
    }
}


/// A badge of the folded sidebar.
#[derive(Clone, Copy, Debug, Hash)]
enum RailEntry {
    /// A local tab that is no profile.
    Tab(usize),
    /// A profile or an SSH host.
    Item(Uuid),
    Db(Uuid),
}

/// A section of the sidebar (folded or not).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum RailSection {
    Local,
    Ssh,
    Db,
}

/// A terminal prompt (">_" in a window): the local section of the folded sidebar.
fn paint_prompt_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let r = Rect::from_center_size(c, Vec2::new(15.0, 12.0));
    painter.rect_stroke(r, 2.5, stroke, egui::StrokeKind::Middle);
    let (x, y) = (r.min.x + 3.5, r.center().y);
    painter.line_segment([Pos2::new(x, y - 2.5), Pos2::new(x + 3.0, y)], stroke);
    painter.line_segment([Pos2::new(x + 3.0, y), Pos2::new(x, y + 2.5)], stroke);
    painter.line_segment([Pos2::new(x + 5.0, y + 2.5), Pos2::new(x + 8.5, y + 2.5)], stroke);
}

/// Two stacked server units: the SSH section of the folded sidebar.
fn paint_server_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    for dy in [-3.25, 3.25] {
        let r = Rect::from_center_size(c + Vec2::new(0.0, dy), Vec2::new(14.0, 5.5));
        painter.rect_stroke(r, 1.5, stroke, egui::StrokeKind::Middle);
        painter.circle_filled(Pos2::new(r.max.x - 3.0, r.center().y), 0.9, color);
    }
}

/// "42 % · 8,1 / 19,3 Mo" while an update downloads (empty until it starts).
pub(super) fn download_text((done, total): (u64, u64), t: &Strings) -> String {
    let mb = |n: u64| {
        let s = format!("{:.1}", n as f64 / 1_048_576.0);
        if t.decimal_comma { s.replace('.', ",") } else { s }
    };
    match (done, total) {
        (0, _) => String::new(),
        (_, 0) => format!("{} {}", mb(done), t.unit_mb),
        _ => format!("{} %  ·  {} / {} {}", done * 100 / total, mb(done), mb(total), t.unit_mb),
    }
}

/// Tips about Ronnie scrolling from right to left along `rect` (with the shortcuts as set), fading at
/// both ends; the pointer over them holds them still.
pub(super) fn tips_ticker(ui: &mut Ui, rect: Rect, theme: &crate::theme::Theme, t: &Strings, shortcuts: &config::Shortcuts) {
    const SPEED: f32 = 45.0; // points a second
    const GAP: f32 = 56.0;
    let painter = ui.painter_at(rect);
    painter.hline(rect.x_range(), rect.min.y, Stroke::new(1.0, theme.tab_hover));
    let keys = [
        ("{split_right}", &shortcuts.split_right),
        ("{split_down}", &shortcuts.split_down),
        ("{find}", &shortcuts.find_text),
        ("{find_commands}", &shortcuts.find_commands),
        ("{reopen}", &shortcuts.reopen_tab),
        ("{files}", &shortcuts.toggle_files),
        ("{new_window}", &shortcuts.new_window),
        ("{sidebar}", &shortcuts.toggle_sidebar),
    ];
    let with_keys = |text: &str| keys.iter().fold(text.to_owned(), |text, (key, shortcut)| text.replace(key, &shortcut.label()));
    let tips: Vec<(&str, String)> = t.home_tips.iter().map(|(title, how)| (*title, with_keys(how))).collect();
    if tips.is_empty() {
        return;
    }
    let galleys: Vec<_> = tips
        .iter()
        .map(|(title, how)| {
            let mut job = egui::text::LayoutJob::default();
            job.append(title, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), theme.accent));
            job.append(&format!("   {how}"), 0.0, egui::TextFormat::simple(FontId::proportional(12.5), theme.text));
            painter.layout_job(job)
        })
        .collect();
    let total: f32 = galleys.iter().map(|g| g.size().x + GAP).sum();
    // Where the strip is: moving on, or held under the pointer.
    let id = ui.id().with("tips-ticker");
    let hovered = ui.rect_contains_pointer(rect);
    let (now, dt) = ui.input(|i| (i.time, i.stable_dt.min(0.1)));
    let offset = ui.data_mut(|d| {
        let offset = d.get_temp_mut_or_insert_with(id, || (now as f32 * 7.0) % total);
        if !hovered {
            *offset = (*offset + SPEED * dt) % total;
        }
        *offset
    });
    // Right to left: the strip drawn end to end, as often as needed, so that it never runs out.
    let y = rect.center().y + 1.0;
    let mut x = rect.min.x - offset;
    while x < rect.max.x {
        for g in &galleys {
            let w = g.size().x;
            if x + w > rect.min.x && x < rect.max.x {
                painter.galley(Pos2::new(x, y - g.size().y / 2.0), g.clone(), theme.text);
            }
            // A small glowing diamond between two tips.
            let d = Pos2::new(x + w + GAP / 2.0, y);
            let pulse = ((now * 3.0 + (x as f64) * 0.01).sin() * 0.5 + 0.5) as f32;
            painter.circle_filled(d, 6.0, theme.accent.gamma_multiply(0.08 + 0.1 * pulse));
            let r = 3.2;
            painter.add(egui::Shape::convex_polygon(vec![d + Vec2::new(0.0, -r), d + Vec2::new(r, 0.0), d + Vec2::new(0.0, r), d + Vec2::new(-r, 0.0)], theme.accent.gamma_multiply(0.6 + 0.4 * pulse), Stroke::NONE));
            x += w + GAP;
        }
    }
    // Both ends fade into the page.
    let fade = 90.0_f32.min(rect.width() / 4.0);
    let steps = 18;
    for k in 0..steps {
        let a = 1.0 - k as f32 / steps as f32;
        let w = fade / steps as f32;
        let color = theme.bg.gamma_multiply(a);
        painter.rect_filled(Rect::from_min_size(Pos2::new(rect.min.x + k as f32 * w, rect.min.y + 1.0), Vec2::new(w + 0.5, rect.height())), 0.0, color);
        painter.rect_filled(Rect::from_min_size(Pos2::new(rect.max.x - (k + 1) as f32 * w, rect.min.y + 1.0), Vec2::new(w + 0.5, rect.height())), 0.0, color);
    }
    // Smooth, and only while the home page shows.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
}


#[cfg(test)]
mod tests {
    #[test]
    fn tips_name_only_known_shortcuts() {
        let known = ["{split_right}", "{split_down}", "{find}", "{find_commands}", "{reopen}", "{files}", "{new_window}", "{sidebar}"];
        let (fr, en) = (crate::i18n::Lang::Fr.strings(), crate::i18n::Lang::En.strings());
        assert_eq!(fr.home_tips.len(), en.home_tips.len());
        for (_, how) in fr.home_tips.iter().chain(en.home_tips) {
            let left = known.iter().fold(how.to_string(), |s, k| s.replace(k, ""));
            assert!(!left.contains('{'), "unknown shortcut in: {how}");
        }
    }
}
