//! The settings dialog and its pages, and the SSH host and profile editors.

use super::*;

impl App {
    pub(super) fn delete_profile(&mut self, id: Uuid) {
        self.config.profiles.retain(|p| p.id != id);
        self.closed.retain(|c| c.profile != Some(id));
        for tab in self.tabs.iter_mut().filter(|t| t.profile == Some(id)) {
            tab.profile = None;
        }
    }

    /// Renames a profile (and its open tab). Refused when empty or used by another profile.
    pub(super) fn rename_profile(&mut self, id: Uuid, name: &str) -> Result<(), &'static str> {
        let name = name.trim();
        if name.is_empty() {
            return Err(self.t().host_required);
        }
        if self.config.profiles.iter().any(|p| p.id != id && p.tab.name.as_deref() == Some(name)) {
            return Err(self.t().name_taken);
        }
        if let Some(p) = self.config.profiles.iter_mut().find(|p| p.id == id) {
            p.tab.name = Some(name.to_owned());
        }
        for tab in self.tabs.iter_mut().filter(|t| t.profile == Some(id)) {
            tab.name = Some(name.to_owned());
        }
        Ok(())
    }

    /// "Name (copy)", or "Name (copy 2)"... the first one not taken.
    fn copy_name(&self, name: &str, taken: impl Fn(&str) -> bool) -> String {
        let suffix = self.t().copy_suffix;
        (1..)
            .map(|n| if n == 1 { format!("{name} ({suffix})") } else { format!("{name} ({suffix} {n})") })
            .find(|candidate| !taken(candidate))
            .expect("some name is free")
    }

    /// A profile is copied right after itself; a host opens in the editor as a new host (with the same
    /// saved password), to change its address or user before saving.
    pub(super) fn duplicate_item(&mut self, id: Uuid) {
        let group = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
        if let Some(host) = self.config.ssh.iter().find(|h| h.id == id) {
            let mut copy = host.clone();
            copy.id = Uuid::new_v4();
            copy.imported = false;
            copy.name = self.copy_name(&host.name, |n| self.config.ssh.iter().any(|h| h.name == n));
            let password = host.uses_saved_password().then(|| ssh::load_password(id)).flatten();
            copy.password_saved = false;
            let mut editor = HostEditor::new(copy, true, group);
            if let Some(password) = password {
                editor.password = password;
                editor.password_changed = true;
            }
            self.host_editor = Some(editor);
            return;
        }
        // An open profile tab has the latest layout: save it first.
        self.sync();
        let Some(profile) = self.config.profiles.iter().find(|p| p.id == id) else { return };
        let mut copy = profile.clone();
        copy.id = Uuid::new_v4();
        let name = profile.name(self.t().untitled).to_owned();
        copy.tab.name = Some(self.copy_name(&name, |n| self.config.profiles.iter().any(|p| p.tab.name.as_deref() == Some(n))));
        let new_id = copy.id;
        let index = self.config.profiles.iter().position(|p| p.id == id).map_or(self.config.profiles.len(), |i| i + 1);
        self.config.profiles.insert(index, copy);
        let list = match group.and_then(|g| self.config.groups.iter_mut().find(|x| x.id == g)) {
            Some(g) => &mut g.items,
            None => &mut self.config.ungrouped,
        };
        let at = list.iter().position(|i| *i == id).map_or(list.len(), |i| i + 1);
        list.insert(at, new_id);
    }

    pub(super) fn set_profile_color(&mut self, id: Uuid, color: Option<Color32>) {
        if let Some(p) = self.config.profiles.iter_mut().find(|p| p.id == id) {
            p.tab.color = color;
        }
        for tab in self.tabs.iter_mut().filter(|t| t.profile == Some(id)) {
            tab.color = color;
        }
    }

    /// Settings page listing the profiles: rename, recolor, open or delete them.
    /// Profiles (`local`) or SSH hosts as in the sidebar: each group in order, then those without a
    /// group. The section name is None when there are no groups at all (a plain list).
    pub(super) fn grouped_ids(&self, local: bool) -> Vec<(Option<String>, Vec<Uuid>)> {
        let of_kind = |id: &Uuid| if local { self.config.profiles.iter().any(|p| p.id == *id) } else { self.config.ssh.iter().any(|h| h.id == *id) };
        let mut sections: Vec<(Option<String>, Vec<Uuid>)> = self
            .config
            .groups
            .iter()
            .filter(|g| g.local == local)
            .map(|g| (Some(g.name.clone()), g.items.iter().copied().filter(of_kind).collect::<Vec<_>>()))
            .filter(|(_, ids)| !ids.is_empty())
            .collect();
        let loose: Vec<Uuid> = self.config.ungrouped.iter().copied().filter(of_kind).collect();
        if !loose.is_empty() {
            let name = (!sections.is_empty()).then(|| self.t().no_group.to_owned());
            sections.push((name, loose));
        }
        sections
    }

    pub(super) fn profiles_ui(&mut self, ui: &mut Ui, t: &Strings) {
        let theme = self.theme.clone();
        if self.config.profiles.is_empty() {
            card(ui, &theme, None, |ui| {
                ui.add_space(14.0);
                ui.label(egui::RichText::new(t.profiles_none).size(13.0).color(theme.text_muted));
                ui.add_space(14.0);
            });
            return;
        }
        let (mut rename, mut recolor, mut open, mut delete, mut edit) = (None, None, None, None, None);
        let sections = self.grouped_ids(true);
        egui::ScrollArea::vertical().id_salt("settings-profiles").max_height(ui.available_height()).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 12.0);
            if let Some(err) = self.profile_error {
                ui.label(egui::RichText::new(format!("⚠  {err}")).size(13.0).color(theme.ansi[1]));
                ui.add_space(10.0);
            }
            for (name, ids) in &sections {
                let profiles: Vec<&Profile> = ids.iter().filter_map(|id| self.config.profiles.iter().find(|p| p.id == *id)).collect();
                if profiles.is_empty() {
                    continue;
                }
                let title = format!("{}  ·  {}", name.as_deref().unwrap_or(t.no_group), profiles.len());
                card(ui, &theme, Some(&title), |ui| {
                    for (i, p) in profiles.iter().enumerate() {
                        if i > 0 {
                            divider(ui, &theme);
                        }
                        let (rect, row) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());
                        if row.hovered() {
                            ui.painter().rect_filled(rect.expand2(Vec2::new(10.0, -3.0)), 8.0, theme.tab_hover.gamma_multiply(0.45));
                        }
                        // The badge picks the color.
                        let badge = Rect::from_center_size(Pos2::new(rect.min.x + 14.0, rect.center().y), Vec2::splat(26.0));
                        let badge_resp = ui.interact(badge, ui.id().with(("profile-color", p.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                        super::sidebar::paint_badge(ui.painter(), badge.center(), p.name(t.untitled), p.tab.color, false, true, badge_resp.hovered(), &theme);
                        egui::Popup::menu(&badge_resp).show(|ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for color in TAB_COLORS {
                                    let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                                    ui.painter().circle_filled(r.center(), 7.5, color);
                                    if p.tab.color == Some(color) || s.hovered() {
                                        ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, theme.text));
                                    }
                                    if s.clicked() {
                                        recolor = Some((p.id, Some(color)));
                                        ui.close();
                                    }
                                }
                            });
                            if p.tab.color.is_some() && ui.button(t.remove_color).clicked() {
                                recolor = Some((p.id, None));
                                ui.close();
                            }
                        });
                        // The name, edited in place; the panes below.
                        let current = p.name(t.untitled).to_owned();
                        let buffer = self.profile_names.entry(p.id).or_insert_with(|| current.clone());
                        let field = Rect::from_min_size(Pos2::new(rect.min.x + 36.0, rect.min.y + 8.0), Vec2::new(260.0_f32.min(rect.width() - 330.0), 24.0));
                        let id = ui.id().with(("profile-name", p.id));
                        let focused = ui.memory(|m| m.has_focus(id));
                        if focused || ui.rect_contains_pointer(field) {
                            ui.painter().rect_filled(field.expand2(Vec2::new(4.0, 0.0)), 5.0, theme.chrome_bg);
                            ui.painter().rect_stroke(field.expand2(Vec2::new(4.0, 0.0)), 5.0, Stroke::new(1.0, if focused { theme.accent } else { theme.tab_hover }), egui::StrokeKind::Inside);
                        }
                        // In a child: the row is already laid out.
                        let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        let name_edit = field_ui.add(egui::TextEdit::singleline(buffer).id(id).frame(Frame::NONE).font(FontId::proportional(14.5)).text_color(theme.text).desired_width(field.width()));
                        if name_edit.lost_focus() && *buffer != current {
                            rename = Some((p.id, buffer.clone()));
                        }
                        let panes = p.tab.layout.panes();
                        ui.painter().text(Pos2::new(rect.min.x + 36.0, rect.min.y + 42.0), Align2::LEFT_CENTER, format!("{panes} {}", if panes == 1 { t.layout_pane } else { t.layout_panes }), FontId::proportional(12.0), theme.text_muted);
                        // Actions, on the right.
                        let actions = Rect::from_min_max(Pos2::new(rect.max.x - 300.0, rect.min.y), rect.max);
                        let mut ui_actions = ui.new_child(egui::UiBuilder::new().max_rect(actions).layout(egui::Layout::right_to_left(egui::Align::Center)));
                        if ui_actions.add(ghost_button(t.delete, theme.ansi[1])).clicked() {
                            delete = Some(p.id);
                        }
                        if ui_actions.add(ghost_button(t.edit, theme.text_muted)).clicked() {
                            edit = Some(p.id);
                        }
                        let open_button = egui::Button::new(egui::RichText::new(t.open).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(72.0, 28.0));
                        if ui_actions.add(open_button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() || row.double_clicked() {
                            open = Some(p.id);
                        }
                    }
                });
            }
        });
        if let Some((id, name)) = rename {
            match self.rename_profile(id, &name) {
                Ok(()) => self.profile_error = None,
                Err(err) => {
                    // Put the current name back in the field.
                    self.profile_names.remove(&id);
                    self.profile_error = Some(err);
                }
            }
        }
        if let Some((id, color)) = recolor {
            self.set_profile_color(id, color);
        }
        if let Some(id) = delete {
            self.delete_profile(id);
            self.profile_names.remove(&id);
        }
        if let Some(id) = edit {
            self.open_profile_editor(id);
        }
        if let Some(id) = open {
            self.open_profile(id);
            self.settings_dialog = false;
        }
    }

    pub(super) fn delete_host(&mut self, id: Uuid) {
        self.config.ssh.retain(|h| h.id != id);
        ssh::delete_password(id);
        // Open sessions keep running as plain tabs.
        for tab in self.tabs.iter_mut().filter(|t| t.ssh == Some(id)) {
            tab.ssh = None;
        }
    }

    /// Hosts of ~/.ssh/config that Ronnie doesn't have yet.
    pub(super) fn new_ssh_config_hosts(&self) -> std::io::Result<(usize, Vec<SshHost>)> {
        let hosts = ssh::import_ssh_config()?;
        let total = hosts.len();
        let new = hosts.into_iter().filter(|host| !self.config.ssh.iter().any(|h| h.name == host.name && h.host == host.host)).collect();
        Ok((total, new))
    }

    /// Adds the hosts of ~/.ssh/config that Ronnie doesn't have yet (in their groups).
    pub(super) fn import_ssh(&mut self) {
        match self.new_ssh_config_hosts() {
            Ok((_, hosts)) => {
                self.config.ssh.extend(hosts);
                self.config.normalize();
            }
            Err(e) => self.error = Some(format!("{} : {e}", self.t().ssh_import_failed)),
        }
    }

    /// Settings page for SSH: import from ~/.ssh/config (explained first) and the list of hosts.
    pub(super) fn ssh_settings_ui(&mut self, ui: &mut Ui, t: &Strings) {
        let theme = self.theme.clone();
        let (mut edit, mut delete, mut new_host, mut import) = (None, None, false, false);
        let sections = self.grouped_ids(false);
        let found = self.new_ssh_config_hosts();
        egui::ScrollArea::vertical().id_salt("settings-ssh").max_height(ui.available_height()).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 12.0);
            // Import from ~/.ssh/config.
            card(ui, &theme, Some(t.import_title), |ui| {
                match &found {
                    Err(_) => setting_row(ui, &theme, t.no_ssh_config, Some(t.import_explain), |_| {}),
                    Ok((total, new)) => {
                        let summary = t.import_found.replace("{total}", &total.to_string()).replace("{new}", &new.len().to_string());
                        setting_row(ui, &theme, &summary, Some(t.import_explain), |ui| {
                            let label = t.import_button.replace("{new}", &new.len().to_string());
                            let (fill, color) = if new.is_empty() { (theme.tab_hover, theme.text_muted) } else { (theme.accent, theme.bg) };
                            let button = egui::Button::new(egui::RichText::new(label).size(13.0).color(color)).fill(fill).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
                            if ui.add_enabled(!new.is_empty(), button).clicked() {
                                import = true;
                            }
                        });
                    }
                }
            });

            // The hosts, by group, with "New host" by the heading.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.ssh_hosts.to_uppercase()).size(11.5).strong().color(theme.text_muted).extra_letter_spacing(0.8));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = egui::Button::new(egui::RichText::new(format!("+  {}", t.new_host)).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0));
                    if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        new_host = true;
                    }
                });
            });
            ui.add_space(8.0);
            if self.config.ssh.is_empty() {
                card(ui, &theme, None, |ui| {
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new(t.no_ssh_hosts).size(13.0).color(theme.text_muted));
                    ui.add_space(14.0);
                });
            }
            for (name, ids) in &sections {
                let hosts: Vec<&SshHost> = ids.iter().filter_map(|id| self.config.ssh.iter().find(|h| h.id == *id)).collect();
                if hosts.is_empty() {
                    continue;
                }
                let title = format!("{}  ·  {}", name.as_deref().unwrap_or(t.no_group), hosts.len());
                card(ui, &theme, Some(&title), |ui| {
                    for (i, host) in hosts.iter().enumerate() {
                        if i > 0 {
                            divider(ui, &theme);
                        }
                        let (rect, row) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
                        if row.hovered() {
                            ui.painter().rect_filled(rect.expand2(Vec2::new(10.0, -3.0)), 8.0, theme.tab_hover.gamma_multiply(0.45));
                        }
                        let open = self.tabs.iter().any(|t| t.ssh == Some(host.id));
                        super::sidebar::paint_badge(ui.painter(), Pos2::new(rect.min.x + 14.0, rect.center().y), &host.name, host.color, true, open, false, &theme);
                        ui.painter().text(Pos2::new(rect.min.x + 36.0, rect.min.y + 18.0), Align2::LEFT_CENTER, &host.name, FontId::proportional(14.5), theme.text);
                        ui.painter().text(Pos2::new(rect.min.x + 36.0, rect.min.y + 38.0), Align2::LEFT_CENTER, host.address(), FontId::monospace(12.0), theme.text_muted);
                        // How it logs in, in a pill.
                        let auth = match host.auth_method() {
                            SshAuth::Auto => t.auth_auto,
                            SshAuth::Password => t.auth_password,
                            SshAuth::Ask => t.auth_ask,
                            SshAuth::Interactive => t.auth_interactive,
                            SshAuth::Key => t.auth_key,
                        };
                        let galley = ui.painter().layout_no_wrap(auth.to_owned(), FontId::proportional(11.0), theme.text_muted);
                        let pill = Rect::from_min_size(Pos2::new(rect.max.x - 210.0 - galley.size().x - 16.0, rect.center().y - 10.0), Vec2::new(galley.size().x + 16.0, 20.0));
                        ui.painter().rect_filled(pill, 10.0, theme.tab_hover.gamma_multiply(0.9));
                        ui.painter().galley(pill.center() - galley.size() / 2.0, galley, theme.text_muted);
                        let actions = Rect::from_min_max(Pos2::new(rect.max.x - 200.0, rect.min.y), rect.max);
                        let mut ui_actions = ui.new_child(egui::UiBuilder::new().max_rect(actions).layout(egui::Layout::right_to_left(egui::Align::Center)));
                        if ui_actions.add(ghost_button(t.delete, theme.ansi[1])).clicked() {
                            delete = Some(host.id);
                        }
                        if ui_actions.add(ghost_button(t.edit, theme.text)).clicked() || row.double_clicked() {
                            edit = Some(host.id);
                        }
                    }
                });
            }

            // Hosts imported from ~/.ssh/config: all removed at once.
            let imported = self.config.ssh.iter().filter(|h| h.imported).count();
            if imported > 0 {
                card(ui, &theme, Some(t.reset_section), |ui| {
                    let title = format!("{} ({imported})", t.delete_imported);
                    let desc = if self.confirm_delete_imported { Some(t.delete_imported_confirm) } else { None };
                    setting_row(ui, &theme, &title, desc, |ui| {
                        if self.confirm_delete_imported {
                            let yes = egui::Button::new(egui::RichText::new(t.confirm).size(13.0).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
                            if ui.add(yes).clicked() {
                                let ids: Vec<Uuid> = self.config.ssh.iter().filter(|h| h.imported).map(|h| h.id).collect();
                                for id in ids {
                                    self.delete_host(id);
                                }
                                self.confirm_delete_imported = false;
                            }
                            if ui.add(ghost_button(t.cancel, theme.text_muted)).clicked() {
                                self.confirm_delete_imported = false;
                            }
                        } else {
                            let button = egui::Button::new(egui::RichText::new(t.delete).size(13.0).color(theme.ansi[1])).stroke(Stroke::new(1.0, theme.ansi[1].gamma_multiply(0.7))).fill(Color32::TRANSPARENT).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
                            if ui.add(button).clicked() {
                                self.confirm_delete_imported = true;
                            }
                        }
                    });
                });
            }
        });
        if import {
            self.import_ssh();
        }
        if new_host {
            self.host_editor = Some(HostEditor::new(SshHost::new(), true, None));
        }
        if let Some(id) = delete {
            self.delete_host(id);
        }
        if let Some(id) = edit {
            if let Some(host) = self.config.ssh.iter().find(|h| h.id == id) {
                let group = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
                self.host_editor = Some(HostEditor::new(host.clone(), false, group));
            }
        }
    }

    /// Dialog to create or edit a database connection, with a connection test.
    pub(super) fn db_editor_window(&mut self, ctx: &egui::Context) {
        let hosts: Vec<(Uuid, String)> = self.config.ssh.iter().map(|h| (h.id, if h.name.is_empty() { h.address() } else { h.name.clone() })).collect();
        let Some(editor) = &mut self.db_editor else { return };
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        if let Some(result) = editor.testing.as_ref().and_then(|rx| rx.try_recv().ok()) {
            editor.tested = Some(result);
            editor.testing = None;
        }
        let (mut save, mut cancel, mut delete, mut test) = (false, false, false, false);
        let frame = Frame::popup(&ctx.global_style()).inner_margin(24.0).fill(theme.chrome_bg).corner_radius(12.0);
        let modal = egui::Modal::new(egui::Id::new("db-editor")).frame(frame).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                super::dbview::paint_db_icon(ui.painter(), r.center(), theme.accent);
                ui.label(egui::RichText::new(if editor.is_new { t.db_new_connection } else { t.db_edit_connection }).size(18.0).strong());
            });
            ui.add_space(16.0);
            let d = &mut editor.draft;
            let field = |ui: &mut Ui, value: &mut String, hint: &str, password: bool| {
                ui.add(egui::TextEdit::singleline(value).hint_text(hint).password(password).desired_width(f32::INFINITY).margin(Vec2::new(8.0, 6.0)))
            };
            egui::Grid::new("db-editor-grid").num_columns(2).spacing([16.0, 12.0]).min_col_width(130.0).show(ui, |ui| {
                ui.label(egui::RichText::new(t.db_name).color(theme.text_muted));
                field(ui, &mut d.name, &d.host.clone(), false);
                ui.end_row();
                ui.label(egui::RichText::new(format!("{}  ·  {}", t.db_host, t.db_port)).color(theme.text_muted));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut d.host).hint_text(t.db_host_hint).desired_width(ui.available_width() - 90.0).margin(Vec2::new(8.0, 6.0)));
                    ui.add(egui::TextEdit::singleline(&mut editor.port).hint_text("3306").desired_width(80.0).margin(Vec2::new(8.0, 6.0)));
                });
                ui.end_row();
                ui.label("");
                ui.label(egui::RichText::new(if d.ssh.is_some() { t.db_via_ssh_hint } else { t.db_localhost_hint }).size(11.5).color(theme.text_muted));
                ui.end_row();
                // Through an SSH host: for servers that only listen on their own machine.
                ui.label(egui::RichText::new(t.db_via_ssh).color(theme.text_muted));
                let current = d.ssh.and_then(|id| hosts.iter().find(|(h, _)| *h == id)).map_or(t.db_direct.to_owned(), |(_, n)| format!("🔒  {n}"));
                egui::ComboBox::from_id_salt("db-editor-ssh").selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut d.ssh, None, t.db_direct);
                    for (id, name) in &hosts {
                        ui.selectable_value(&mut d.ssh, Some(*id), format!("🔒  {name}"));
                    }
                });
                ui.end_row();
                ui.label(egui::RichText::new(t.db_user).color(theme.text_muted));
                field(ui, &mut d.user, "root", false);
                ui.end_row();
                ui.label(egui::RichText::new(t.db_password).color(theme.text_muted));
                ui.horizontal(|ui| {
                    let hint = if d.password_saved && !editor.password_changed { t.password_saved } else { t.optional };
                    let edit = ui.add(egui::TextEdit::singleline(&mut editor.password).hint_text(hint).password(!editor.reveal).desired_width(ui.available_width() - 40.0).margin(Vec2::new(8.0, 6.0)));
                    if edit.changed() {
                        editor.password_changed = true;
                    }
                    if ui.add(egui::Button::new(if editor.reveal { "🙈" } else { "👁" }).frame_when_inactive(false)).clicked() {
                        editor.reveal = !editor.reveal;
                    }
                });
                ui.end_row();
                ui.label(egui::RichText::new(t.db_default_db).color(theme.text_muted));
                let mut database = d.database.clone().unwrap_or_default();
                field(ui, &mut database, t.optional, false);
                d.database = Some(database.trim().to_owned()).filter(|x| !x.is_empty());
                ui.end_row();
                ui.label(egui::RichText::new(capitalized(t.color)).color(theme.text_muted));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for color in TAB_COLORS {
                        let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 7.5, color);
                        if d.color == Some(color) || s.hovered() {
                            ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, theme.text));
                        }
                        if s.clicked() {
                            d.color = if d.color == Some(color) { None } else { Some(color) };
                        }
                    }
                });
                ui.end_row();
            });
            ui.add_space(14.0);
            // Test the connection with what is typed.
            ui.horizontal(|ui| {
                if ui.add_enabled(editor.testing.is_none(), egui::Button::new(egui::RichText::new(format!("⚡  {}", t.db_test)).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    test = true;
                }
                match (&editor.testing, &editor.tested) {
                    (Some(_), _) => {
                        ui.spinner();
                        ui.label(egui::RichText::new(t.db_testing).size(12.5).color(theme.text_muted));
                    }
                    (None, Some(Ok(v))) => {
                        ui.label(egui::RichText::new(format!("✓  {}", t.db_test_ok.replace("{v}", v))).size(12.5).color(theme.ansi[2]));
                    }
                    (None, Some(Err(e))) => {
                        ui.add(egui::Label::new(egui::RichText::new(format!("✗  {e}")).size(12.5).color(theme.ansi[1])).wrap());
                    }
                    _ => {}
                }
            });
            if let Some(e) = &editor.error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(e).size(12.5).color(theme.ansi[1]));
            }
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                if !editor.is_new && ui.add(egui::Button::new(egui::RichText::new(t.delete).size(13.5).color(theme.ansi[1])).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    delete = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = egui::Button::new(egui::RichText::new(t.save).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(110.0, 32.0));
                    if ui.add(ok).clicked() {
                        save = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(96.0, 32.0))).clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        let Some(editor) = &mut self.db_editor else { return };
        let port = editor.port.trim().parse::<u16>().ok().filter(|p| *p > 0);
        if test {
            let password = if editor.password_changed || !editor.draft.password_saved { Some(editor.password.clone()).filter(|p| !p.is_empty()) } else { ssh::load_password(editor.draft.id) };
            let host = editor.draft.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)).cloned();
            let tunnel = host.as_ref().map(|h| h.tunnel_command());
            let target = crate::db::Target { host: editor.draft.host.trim().to_owned(), port: port.unwrap_or(3306), user: editor.draft.user.trim().to_owned(), password, database: editor.draft.database.clone(), tunnel };
            editor.tested = None;
            let (rx, pid) = crate::db::test(ctx, target);
            editor.testing = Some(rx);
            editor.test_pid = pid;
            // The forward's ssh may ask for the host's password or key: in the window.
            #[cfg(unix)]
            if let (Some(pid), Some(h)) = (pid, &host) {
                self.askpass.allow_interactive(pid, h);
            }
        }
        if delete {
            let id = editor.draft.id;
            self.db_editor = None;
            self.delete_db(id);
            return;
        }
        if cancel {
            self.db_editor = None;
            return;
        }
        if !save {
            return;
        }
        let mut c = editor.draft.clone();
        c.host = c.host.trim().to_owned();
        c.user = c.user.trim().to_owned();
        if c.host.is_empty() {
            editor.error = Some(t.host_required.to_owned());
            return;
        }
        let Some(port) = port else {
            editor.error = Some(t.invalid_port.to_owned());
            return;
        };
        c.port = port;
        c.name = c.name.trim().to_owned();
        if c.name.is_empty() {
            c.name = c.host.clone();
        }
        if editor.password_changed {
            if editor.password.is_empty() {
                ssh::delete_password(c.id);
                c.password_saved = false;
            } else {
                match ssh::save_password(c.id, &editor.password) {
                    Ok(()) => c.password_saved = true,
                    Err(e) => {
                        editor.error = Some(format!("{} : {e}", t.keychain_failed));
                        return;
                    }
                }
            }
        }
        match self.config.databases.iter_mut().find(|x| x.id == c.id) {
            Some(existing) => *existing = c.clone(),
            None => self.config.databases.push(c.clone()),
        }
        // An open view reconnects with the new settings.
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.db == Some(c.id)) {
            tab.db_view = None;
            tab.name = Some(c.name.clone());
            tab.color = c.color;
        }
        self.db_editor = None;
        self.save_config();
    }

    /// Dialog to create or edit an SSH host.
    pub(super) fn profile_editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.profile_editor else { return };
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        let mut result: Option<bool> = None; // Some(true) = save, Some(false) = cancel
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("profile-editor")).frame(frame).backdrop_color(Color32::from_black_alpha(170)).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(egui::RichText::new(t.edit_profile).size(18.0).strong());
            ui.add_space(14.0);
            let label = |ui: &mut Ui, text: &str| ui.label(egui::RichText::new(text).size(12.0).strong().color(theme.text_muted));

            label(ui, &t.host_name.to_uppercase());
            let name = ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)));
            if name.changed() {
                editor.error = None;
            }
            if let Some(err) = editor.error {
                ui.label(egui::RichText::new(err).size(12.5).color(theme.ansi[1]));
            }
            ui.add_space(12.0);

            label(ui, t.color);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for color in TAB_COLORS {
                    let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 8.0, color);
                    if editor.color == Some(color) || s.hovered() {
                        ui.painter().circle_stroke(r.center(), 9.5, Stroke::new(1.5, theme.text));
                    }
                    if s.clicked() {
                        editor.color = Some(color);
                    }
                }
                if editor.color.is_some() && ui.small_button(t.remove_color).clicked() {
                    editor.color = None;
                }
            });
            ui.add_space(12.0);

            label(ui, &t.profile_panes.to_uppercase());
            egui::Grid::new("profile-panes").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (i, cwd) in editor.cwds.iter_mut().enumerate() {
                    ui.label(egui::RichText::new(t.pane_n.replace("{n}", &(i + 1).to_string())).size(13.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(cwd).hint_text("~").font(FontId::monospace(12.5)).desired_width(370.0).margin(Vec2::new(6.0, 4.0)));
                    ui.end_row();
                }
            });
            ui.add_space(12.0);

            label(ui, &t.commands.to_uppercase());
            let mut remove = None;
            for (i, command) in editor.commands.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(command).font(FontId::monospace(12.5)).desired_width(420.0).margin(Vec2::new(6.0, 4.0)));
                    if ui.add(egui::Button::new(egui::RichText::new("✕").color(theme.text_muted)).frame(false)).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                editor.commands.remove(i);
            }
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut editor.new_command).hint_text(t.commands_new).font(FontId::monospace(12.5)).desired_width(360.0).margin(Vec2::new(6.0, 4.0)));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (ui.button(t.commands_add).clicked() || enter) && !editor.new_command.trim().is_empty() {
                    editor.commands.push(editor.new_command.trim().to_owned());
                    editor.new_command.clear();
                }
            });

            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = egui::Button::new(egui::RichText::new(t.save).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0));
                if ui.add(save).clicked() {
                    result = Some(true);
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                    result = Some(false);
                }
            });
        });
        if modal.should_close() {
            result = Some(false);
        }
        match result {
            Some(true) => self.save_profile_editor(),
            Some(false) => self.profile_editor = None,
            None => {}
        }
    }

    /// Applies the profile editor: to the profile, and to its tab when open.
    pub(super) fn save_profile_editor(&mut self) {
        let Some(editor) = self.profile_editor.take() else { return };
        if let Err(err) = self.rename_profile(editor.id, &editor.name) {
            self.profile_editor = Some(ProfileEditor { error: Some(err), ..editor });
            return;
        }
        self.set_profile_color(editor.id, editor.color);
        let cwds: Vec<Option<PathBuf>> = editor.cwds.iter().map(|c| c.trim()).map(|c| (!c.is_empty()).then(|| ssh::expand_home(Path::new(c)))).collect();
        if let Some(p) = self.config.profiles.iter_mut().find(|p| p.id == editor.id) {
            p.tab.layout.set_cwds(&mut cwds.clone().into_iter());
            p.commands = editor.commands.iter().map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()).collect();
        }
        // An open tab would write its own directories back into the profile: update them too (they apply
        // to shells started from now on).
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.profile == Some(editor.id)) {
            for (leaf, cwd) in tab.layout.leaves().into_iter().zip(cwds) {
                match cwd {
                    Some(cwd) => tab.cwds.insert(leaf, cwd),
                    None => tab.cwds.remove(&leaf),
                };
            }
        }
        self.focus_terminal = true;
    }

    pub(super) fn host_editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.host_editor else { return };
        let t = self.config.settings.language.strings();
        let theme = &self.theme;
        let groups: Vec<(Uuid, String)> = self.config.groups.iter().map(|g| (g.id, g.name.clone())).collect();
        let mut result: Option<bool> = None; // Some(true) = save, Some(false) = cancel
        let mut delete = false;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("host-editor")).frame(frame).show(ctx, |ui| {
            ui.set_width(440.0);
            let title = if editor.is_new { t.new_host } else { t.edit_host };
            ui.label(egui::RichText::new(title).size(18.0).strong());
            ui.add_space(14.0);
            let field_w = 300.0;
            let d = &mut editor.draft;
            egui::Grid::new("host-fields").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                let label = |ui: &mut Ui, text: &str| ui.label(egui::RichText::new(text).size(13.0).color(theme.text_muted));
                let text_field = |ui: &mut Ui, value: &mut String, hint: &str| {
                    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(field_w).margin(Vec2::new(6.0, 5.0)))
                };

                label(ui, t.host_address);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut d.host).hint_text("exemple.com").desired_width(field_w - 80.0).margin(Vec2::new(6.0, 5.0)));
                    ui.add(egui::TextEdit::singleline(&mut editor.port).hint_text("22").desired_width(66.0).margin(Vec2::new(6.0, 5.0)));
                });
                ui.end_row();

                label(ui, t.host_name);
                text_field(ui, &mut d.name, &d.host.clone());
                ui.end_row();

                label(ui, t.user);
                let mut user = d.user.clone().unwrap_or_default();
                text_field(ui, &mut user, "root");
                d.user = Some(user.trim().to_owned()).filter(|u| !u.is_empty());
                ui.end_row();

                label(ui, t.auth);
                ui.vertical(|ui| {
                    let name = |auth: SshAuth| match auth {
                        SshAuth::Auto => t.auth_auto,
                        SshAuth::Password => t.auth_password,
                        SshAuth::Ask => t.auth_ask,
                        SshAuth::Interactive => t.auth_interactive,
                        SshAuth::Key => t.auth_key,
                    };
                    let current = d.auth_method();
                    egui::ComboBox::from_id_salt("host-auth").selected_text(name(current)).width(field_w).show_ui(ui, |ui| {
                        for auth in SshAuth::ALL {
                            if ui.selectable_label(current == auth, name(auth)).clicked() {
                                d.auth = Some(auth);
                            }
                        }
                    });
                    let hint = match current {
                        SshAuth::Auto => Some(t.auth_auto_hint),
                        SshAuth::Ask => Some(t.auth_ask_hint),
                        SshAuth::Interactive => Some(t.auth_interactive_hint),
                        SshAuth::Password | SshAuth::Key => None,
                    };
                    if let Some(hint) = hint {
                        ui.add_sized([field_w, 0.0], egui::Label::new(egui::RichText::new(hint).size(12.0).color(theme.text_muted)).wrap());
                    }
                });
                ui.end_row();

                if d.auth_method() == SshAuth::Key {
                    label(ui, t.key);
                    ui.vertical(|ui| {
                        let current = match &d.identity_file {
                            None => t.key_none.to_owned(),
                            Some(k) if editor.keys.contains(k) => ssh::display_path(k),
                            Some(_) => t.key_other.to_owned(),
                        };
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("host-key").selected_text(current).width(field_w - 88.0).show_ui(ui, |ui| {
                                for key in &editor.keys {
                                    if ui.selectable_label(d.identity_file.as_ref() == Some(key), ssh::display_path(key)).clicked() {
                                        d.identity_file = Some(key.clone());
                                    }
                                }
                                let custom = d.identity_file.as_ref().is_some_and(|k| !editor.keys.contains(k));
                                if ui.selectable_label(custom, t.key_other).clicked() {
                                    d.identity_file = Some(PathBuf::from(editor.custom_key.trim()));
                                }
                            });
                            if ui.add(egui::Button::new(t.key_browse).min_size(Vec2::new(80.0, 24.0))).clicked() {
                                let start = d.identity_file.as_ref().map(|k| ssh::expand_home(k)).and_then(|k| k.parent().map(Path::to_path_buf));
                                let dir = start.filter(|p| p.is_dir()).or_else(ssh::ssh_dir).filter(|p| p.is_dir());
                                let mut dialog = rfd::FileDialog::new().set_title(t.key);
                                if let Some(dir) = dir {
                                    dialog = dialog.set_directory(dir);
                                }
                                if let Some(path) = dialog.pick_file() {
                                    if !editor.keys.contains(&path) {
                                        editor.custom_key = ssh::display_path(&path);
                                    }
                                    d.identity_file = Some(path);
                                }
                            }
                        });
                        if d.identity_file.as_ref().is_some_and(|k| !editor.keys.contains(k)) {
                            if ui.add(egui::TextEdit::singleline(&mut editor.custom_key).hint_text(ssh::example_key_path(t.key_example)).desired_width(field_w).margin(Vec2::new(6.0, 5.0))).changed() {
                                d.identity_file = Some(PathBuf::from(editor.custom_key.trim()));
                            }
                        }
                        // Read once per chosen file, not at every frame.
                        if editor.putty_check.as_ref().map(|(path, _)| path) != d.identity_file.as_ref() {
                            editor.putty_check = d.identity_file.clone().map(|path| {
                                let putty = ssh::is_putty_key(&path);
                                (path, putty)
                            });
                        }
                        if editor.putty_check.as_ref().is_some_and(|(_, putty)| *putty) {
                            ui.add_sized([field_w, 0.0], egui::Label::new(egui::RichText::new(t.putty_key).size(12.0).color(theme.ansi[3])).wrap());
                        }
                    });
                    ui.end_row();

                }

                // Required for the "saved password" method; optional with a key (servers asking for both).
                let password_mode = d.auth_method() == SshAuth::Password;
                if password_mode || d.auth_method() == SshAuth::Key {
                    // With a key: its passphrase, typed when ssh unlocks it (or a server's password).
                    label(ui, if password_mode { t.password } else { t.key_password });
                    ui.vertical(|ui| {
                        let hint = if d.password_saved && !editor.password_changed { "••••••••" } else if password_mode { "" } else { t.optional };
                        ui.horizontal(|ui| {
                            let field = egui::TextEdit::singleline(&mut editor.password)
                                .password(!editor.reveal)
                                .hint_text(hint)
                                .desired_width(field_w - 80.0)
                                .margin(Vec2::new(6.0, 5.0));
                            if ui.add(field).changed() {
                                editor.password_changed = true;
                            }
                            let toggle = if editor.reveal { t.hide_password } else { t.show_password };
                            if ui.add(egui::Button::new(toggle).min_size(Vec2::new(72.0, 24.0))).clicked() {
                                editor.reveal = !editor.reveal;
                                // Show the saved password so it can be checked or corrected.
                                if editor.reveal && d.password_saved && !editor.password_changed && editor.password.is_empty() {
                                    editor.password = ssh::load_password(d.id).unwrap_or_default();
                                }
                            }
                        });
                        if d.password_saved && !editor.password_changed && !password_mode {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(t.password_saved).size(12.0).color(theme.text_muted));
                                if ui.small_button(t.forget_password).clicked() {
                                    editor.password.clear();
                                    editor.password_changed = true;
                                }
                            });
                        }
                    });
                    ui.end_row();

                }

                label(ui, t.jump);
                let mut jump = d.jump.clone().unwrap_or_default();
                text_field(ui, &mut jump, t.optional);
                d.jump = Some(jump.trim().to_owned()).filter(|j| !j.is_empty());
                ui.end_row();

                label(ui, t.start_dir);
                let mut dir = d.start_dir.clone().unwrap_or_default();
                text_field(ui, &mut dir, t.start_dir_hint);
                d.start_dir = Some(dir).filter(|d| !d.trim().is_empty());
                ui.end_row();

                label(ui, t.host_colors);
                ui.checkbox(&mut d.colors, egui::RichText::new(t.host_colors_hint).size(12.5).color(theme.text_muted));
                ui.end_row();

                label(ui, t.host_group);
                let current = groups.iter().find(|(id, _)| Some(*id) == editor.group).map_or(t.no_group, |(_, name)| name.as_str());
                egui::ComboBox::from_id_salt("host-group").selected_text(current).width(field_w).show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor.group, None, t.no_group);
                    for (id, name) in &groups {
                        ui.selectable_value(&mut editor.group, Some(*id), name);
                    }
                });
                ui.end_row();

                // Menu headings are in capitals; here it's a field label like the others.
                label(ui, &capitalized(t.color));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for color in TAB_COLORS {
                        let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 7.5, color);
                        if d.color == Some(color) || s.hovered() {
                            ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, theme.text));
                        }
                        if s.clicked() {
                            d.color = if d.color == Some(color) { None } else { Some(color) };
                        }
                    }
                });
                ui.end_row();
            });

            ui.add_space(16.0);
            if let Some(err) = &editor.error {
                ui.label(egui::RichText::new(err).size(13.0).color(theme.ansi[1]));
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                if !editor.is_new && ui.button(egui::RichText::new(t.delete).size(14.0).color(theme.ansi[1])).clicked() {
                    delete = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.save).size(14.0)).min_size(Vec2::new(110.0, 30.0))).clicked() {
                        result = Some(true);
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(14.0)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                        result = Some(false);
                    }
                });
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }

        if delete {
            let id = editor.draft.id;
            self.host_editor = None;
            self.delete_host(id);
            return;
        }
        match result {
            Some(false) => self.host_editor = None,
            Some(true) => self.save_host(),
            None => {}
        }
    }

    /// Validates and stores the host being edited, with its saved password.
    pub(super) fn save_host(&mut self) {
        let t = self.t();
        let Some(editor) = &mut self.host_editor else { return };
        let mut host = editor.draft.clone();
        host.host = host.host.trim().to_owned();
        if host.host.is_empty() {
            editor.error = Some(t.host_required.to_owned());
            return;
        }
        // ssh would read a value starting with "-" as an option.
        let dashed = |v: &Option<String>| v.as_deref().is_some_and(|v| v.trim_start().starts_with('-'));
        if host.host.starts_with('-') || dashed(&host.user) || dashed(&host.jump) {
            editor.error = Some(t.invalid_dash.to_owned());
            return;
        }
        host.port = match editor.port.trim() {
            "" => None,
            p => match p.parse::<u16>() {
                Ok(p) if p > 0 => Some(p),
                _ => {
                    editor.error = Some(t.invalid_port.to_owned());
                    return;
                }
            },
        };
        host.start_dir = host.start_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()).map(str::to_owned);
        host.name = host.name.trim().to_owned();
        if host.name.is_empty() {
            host.name = host.host.clone();
        }
        if host.identity_file.as_ref().is_some_and(|k| k.as_os_str().is_empty()) {
            host.identity_file = None;
        }
        let auth = host.auth_method();
        if auth == SshAuth::Key && host.identity_file.is_none() {
            editor.error = Some(t.key_required.to_owned());
            return;
        }
        if auth != SshAuth::Key {
            host.identity_file = None;
        }
        if auth == SshAuth::Password && editor.password.is_empty() && (editor.password_changed || !host.password_saved) {
            editor.error = Some(t.password_required.to_owned());
            return;
        }
        // Only the "saved password" and key methods keep one.
        if !matches!(auth, SshAuth::Password | SshAuth::Key) && host.password_saved {
            editor.password.clear();
            editor.password_changed = true;
        }
        if editor.password_changed {
            if editor.password.is_empty() {
                ssh::delete_password(host.id);
                host.password_saved = false;
            } else {
                match ssh::save_password(host.id, &editor.password) {
                    Ok(()) => host.password_saved = true,
                    Err(e) => {
                        editor.error = Some(format!("{} : {e}", t.keychain_failed));
                        return;
                    }
                }
            }
        }
        let (id, group) = (host.id, editor.group);
        match self.config.ssh.iter_mut().find(|h| h.id == id) {
            Some(existing) => *existing = host,
            None => self.config.ssh.push(host),
        }
        // Move it only if its group changed, so it keeps its place otherwise.
        let current = self.config.groups.iter().find(|g| g.items.contains(&id)).map(|g| g.id);
        let placed = current.is_some() || self.config.ungrouped.contains(&id);
        if !placed || current != group {
            self.config.unplace(id);
            match group.and_then(|g| self.config.groups.iter_mut().find(|x| x.id == g)) {
                Some(g) => g.items.push(id),
                None => self.config.ungrouped.push(id),
            }
        }
        self.host_editor = None;
    }

    /// The settings dialog: language and color theme, applied as soon as they are picked.
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_dialog {
            return;
        }
        let t = self.t();
        let theme = self.theme.clone();
        let mut picked = self.config.settings.clone();
        let mut close = false;
        let screen = ctx.content_rect();
        let size = Vec2::new(SETTINGS_WIDTH.min(screen.width() - 60.0).max(640.0), SETTINGS_HEIGHT.min(screen.height() - 80.0).max(380.0));
        let frame = Frame::popup(&ctx.global_style()).inner_margin(0.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let modal = egui::Modal::new(egui::Id::new("settings")).frame(frame).backdrop_color(Color32::from_black_alpha(190)).show(ctx, |ui| {
            // Same size on every page: pages scroll inside, the window never jumps around.
            let (whole, _) = ui.allocate_exact_size(size, Sense::hover());
            let nav = Rect::from_min_size(whole.min, Vec2::new(SETTINGS_NAV_WIDTH, whole.height()));
            let content = Rect::from_min_max(Pos2::new(nav.max.x, whole.min.y), whole.max);
            ui.painter().rect_filled(nav, egui::CornerRadius { nw: 14, sw: 14, ne: 0, se: 0 }, theme.bg);
            ui.painter().vline(nav.max.x, nav.y_range(), Stroke::new(1.0, theme.tab_hover));

            // Navigation.
            ui.scope_builder(egui::UiBuilder::new().max_rect(nav.shrink2(Vec2::new(12.0, 20.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.settings).size(17.0).strong());
                });
                ui.add_space(18.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                let pages = [
                    (SettingsTab::General, "⚙", t.general),
                    (SettingsTab::Appearance, "🎨", t.appearance),
                    (SettingsTab::Shortcuts, "⌨", t.shortcuts),
                    (SettingsTab::Profiles, "▣", t.manage_profiles),
                    (SettingsTab::Ssh, "🖧", t.ssh_tab),
                    (SettingsTab::ConfigFile, "{ }", t.config_nav),
                    (SettingsTab::Logs, "☰", t.logs_nav),
                ];
                for (tab, icon, label) in pages {
                    if nav_item(ui, &theme, icon, label, self.settings_tab == tab) {
                        self.settings_tab = tab;
                    }
                }
                // "Features" and "About" at the bottom, apart (drawn upwards: About last).
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(4.0);
                    if nav_item(ui, &theme, "i", t.about, self.settings_tab == SettingsTab::About) {
                        self.settings_tab = SettingsTab::About;
                    }
                    if nav_item(ui, &theme, "★", t.features_nav, self.settings_tab == SettingsTab::Features) {
                        self.settings_tab = SettingsTab::Features;
                    }
                });
            });

            // The page: title, subtitle, then its content.
            let inner = content.shrink2(Vec2::new(32.0, 24.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                let (title, subtitle) = match self.settings_tab {
                    SettingsTab::General => (t.general, t.sub_general),
                    SettingsTab::Appearance => (t.appearance, t.sub_appearance),
                    SettingsTab::Shortcuts => (t.shortcuts, t.sub_shortcuts),
                    SettingsTab::Profiles => (t.manage_profiles, t.sub_profiles),
                    SettingsTab::Ssh => (t.ssh_tab, t.sub_ssh),
                    SettingsTab::ConfigFile => (t.config_file, t.sub_config),
                    SettingsTab::Logs => (t.logs_nav, t.sub_logs),
                    SettingsTab::Features => (t.features_nav, t.sub_features),
                    SettingsTab::About => (t.about, t.sub_about),
                };
                // Title and subtitle, with the close button at the right.
                let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
                ui.painter().text(head.left_top(), Align2::LEFT_TOP, title, FontId::proportional(22.0), theme.text);
                ui.painter().text(head.left_top() + Vec2::new(0.0, 32.0), Align2::LEFT_TOP, subtitle, FontId::proportional(13.0), theme.text_muted);
                let x_rect = Rect::from_min_size(Pos2::new(head.max.x - 28.0, head.min.y), Vec2::splat(28.0));
                let x = egui::Button::new(egui::RichText::new("✕").size(15.0).color(theme.text_muted)).frame_when_inactive(false).corner_radius(6.0);
                if ui.put(x_rect, x).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    close = true;
                }
                // The page's content, in its own area under the header.
                let body = Rect::from_min_max(Pos2::new(inner.min.x, head.max.y + 18.0), inner.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                    ui.set_clip_rect(body.expand2(Vec2::new(8.0, 0.0)));
                    let height = body.height();
                    match self.settings_tab {
                        SettingsTab::ConfigFile => self.config_editor_ui(ui, t),
                        SettingsTab::Logs => self.logs_ui(ui, t, &theme),
                        SettingsTab::Profiles => self.profiles_ui(ui, t),
                        SettingsTab::Ssh => self.ssh_settings_ui(ui, t),
                        SettingsTab::About => {
                            egui::ScrollArea::vertical().id_salt("settings-about").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                                ui.set_width(ui.available_width() - 12.0);
                                self.about_ui(ui, ctx, t, &mut picked)
                            });
                        }
                        SettingsTab::Shortcuts => self.shortcuts_ui(ui, t, &mut picked),
                        SettingsTab::Features => {
                            egui::ScrollArea::vertical().id_salt("settings-features").max_height(height).auto_shrink([false, false]).show(ui, |ui| features_page(ui, t, &theme));
                        }
                        SettingsTab::General => {
                            egui::ScrollArea::vertical().id_salt("settings-general").max_height(height).auto_shrink([false, false]).show(ui, |ui| self.general_page(ui, t, &theme, &mut picked));
                        }
                        SettingsTab::Appearance => {
                            egui::ScrollArea::vertical().id_salt("settings-appearance").max_height(height).auto_shrink([false, false]).show(ui, |ui| self.appearance_page(ui, t, &theme, &mut picked));
                        }
                    }
                });
            });
        });
        if self.settings_tab != SettingsTab::Shortcuts {
            self.shortcut_capture = None;
        }
        // Read again the next time the page opens.
        if self.settings_tab != SettingsTab::Logs {
            self.logs = None;
        }
        if close || modal.should_close() {
            self.settings_dialog = false;
            self.shortcut_capture = None;
            self.editor = None;
            self.logs = None;
            self.profile_names.clear();
            self.profile_error = None;
            self.focus_terminal = true;
        }
        if picked != self.config.settings {
            let mut config = self.config.clone();
            config.settings = picked;
            self.apply_config(config);
            self.save_config();
        }
    }

    /// "General" page: language, terminal, typing, notifications, reset.
    fn general_page(&mut self, ui: &mut Ui, t: &Strings, theme: &Theme, picked: &mut config::Settings) {
        ui.set_width(ui.available_width() - 12.0);
        let mut test_clicked = false;
        card(ui, theme, Some(t.language), |ui| {
            setting_row(ui, theme, t.language, Some(t.language_desc), |ui| {
                for lang in Lang::ALL.iter().rev() {
                    let label = egui::RichText::new(lang.label()).size(13.5);
                    if ui.add(egui::Button::selectable(picked.language == *lang, label).min_size(Vec2::new(96.0, 30.0)).corner_radius(6.0)).clicked() {
                        picked.language = *lang;
                    }
                }
            });
        });
        card(ui, theme, Some(t.set_terminal), |ui| {
            setting_row(ui, theme, t.show_cwd, Some(t.show_cwd_desc), |ui| {
                toggle(ui, theme, &mut picked.show_cwd);
            });
            divider(ui, theme);
            setting_row(ui, theme, t.restore_scrollback, Some(t.restore_scrollback_desc), |ui| {
                if toggle(ui, theme, &mut picked.restore_scrollback).changed() && !picked.restore_scrollback {
                    crate::shell::forget_scrollbacks();
                }
            });
            divider(ui, theme);
            setting_row(ui, theme, t.scrollback, Some(t.scrollback_desc), |ui| {
                ui.add(egui::Slider::new(&mut picked.scrollback, config::SCROLLBACK_LINES).logarithmic(true));
            });
            divider(ui, theme);
            setting_row(ui, theme, t.clipboard_from_programs, Some(t.clipboard_desc), |ui| {
                toggle(ui, theme, &mut picked.clipboard_from_programs);
            });
        });
        card(ui, theme, Some(t.set_typing), |ui| {
            setting_row(ui, theme, t.path_suggestions, Some(t.path_suggestions_desc), |ui| {
                toggle(ui, theme, &mut picked.path_suggestions);
            });
            divider(ui, theme);
            setting_row(ui, theme, t.metal_guard, Some(t.metal_guard_desc), |ui| {
                toggle(ui, theme, &mut picked.metal_guard);
            });
        });
        card(ui, theme, Some(t.set_databases), |ui| {
            setting_row(ui, theme, t.db_confirm_changes, Some(t.db_confirm_changes_desc), |ui| {
                toggle(ui, theme, &mut picked.db_confirm_changes);
            });
        });
        card(ui, theme, Some(t.set_notifications), |ui| {
            setting_row(ui, theme, t.notify_commands, Some(t.notify_commands_hint), |ui| {
                toggle(ui, theme, &mut picked.notify_commands);
            });
            divider(ui, theme);
            ui.add_enabled_ui(picked.notify_commands, |ui| {
                setting_row(ui, theme, t.notify_after, Some(t.notify_after_desc), |ui| {
                    ui.add(egui::Slider::new(&mut picked.notify_after, config::NOTIFY_AFTER_SECS).logarithmic(true).suffix(" s"));
                });
                divider(ui, theme);
                setting_row(ui, theme, t.notify_style, Some(t.notify_style_desc), |ui| {
                    for (style, label) in [(config::NotifyStyle::Both, t.notify_both), (config::NotifyStyle::System, t.notify_system), (config::NotifyStyle::InApp, t.notify_in_app)] {
                        if ui.add(egui::Button::selectable(picked.notify_style == style, egui::RichText::new(label).size(12.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))).clicked() {
                            picked.notify_style = style;
                        }
                    }
                });
                if picked.notify_style.in_app() {
                    divider(ui, theme);
                    setting_row(ui, theme, t.toast_position, Some(t.toast_position_desc), |ui| {
                        position_picker(ui, theme, &mut picked.toast_position);
                    });
                }
                divider(ui, theme);
                setting_row(ui, theme, t.notify_test, Some(t.notify_test_desc), |ui| {
                    let button = egui::Button::new(egui::RichText::new(format!("🔔  {}", t.notify_test_button)).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
                    if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        test_clicked = true;
                    }
                });
            });
        });
        card(ui, theme, Some(t.reset_section), |ui| {
            setting_row(ui, theme, t.reset_row, Some(t.reset_desc), |ui| {
                let reset = egui::Button::new(egui::RichText::new(t.reset_button).size(13.5).color(theme.ansi[1]))
                    .stroke(Stroke::new(1.0, theme.ansi[1].gamma_multiply(0.7)))
                    .fill(Color32::TRANSPARENT)
                    .corner_radius(6.0)
                    .min_size(Vec2::new(0.0, 30.0));
                if ui.add(reset).clicked() {
                    self.confirm_reset = true;
                }
            });
        });
        // With the style picked on this page (it may have just changed).
        if test_clicked {
            let saved = self.config.settings.notify_style;
            self.config.settings.notify_style = picked.notify_style;
            self.test_notification();
            self.config.settings.notify_style = saved;
        }
    }

    /// "Appearance" page: interface and text size, then the themes.
    fn appearance_page(&mut self, ui: &mut Ui, t: &Strings, theme: &Theme, picked: &mut config::Settings) {
        ui.set_width(ui.available_width() - 12.0);
        card(ui, theme, Some(t.set_interface), |ui| {
            setting_row(ui, theme, t.ui_zoom, Some(t.ui_zoom_desc), |ui| {
                // Applied once the slider is released: zooming while dragging would move the slider
                // under the pointer, which would drag it further.
                let id = ui.id().with("ui-zoom-drag");
                let mut percent = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or((picked.ui_zoom * 100.0).round());
                let resp = ui.add(egui::Slider::new(&mut percent, 60.0..=200.0).step_by(10.0).suffix(" %"));
                if resp.dragged() {
                    ui.data_mut(|d| d.insert_temp(id, percent));
                } else {
                    ui.data_mut(|d| d.remove::<f32>(id));
                    if resp.changed() || resp.drag_stopped() {
                        picked.ui_zoom = percent / 100.0;
                    }
                }
            });
            divider(ui, theme);
            setting_row(ui, theme, t.font_size, Some(t.font_size_desc), |ui| {
                ui.add(egui::Slider::new(&mut picked.font_size, config::FONT_SIZES).step_by(1.0).suffix(" pt"));
            });
        });
        for (dark, title) in [(true, t.themes_dark), (false, t.themes_light)] {
            section_title(ui, theme, title);
            let presets: Vec<&Preset> = PRESETS.iter().filter(|p| p.dark == dark).collect();
            let per_row = ((ui.available_width() + 12.0) / (THEME_CARD.x + 12.0)).floor().max(1.0) as usize;
            for row in presets.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for preset in row {
                        if theme_card(ui, preset, picked.theme == preset.id, theme.text).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            picked.theme = preset.id.to_owned();
                        }
                    }
                });
                ui.add_space(12.0);
            }
            ui.add_space(10.0);
        }
    }

    /// "Shortcuts" settings page: each action's shortcut can be recorded (click, then type the
    /// combination); a few fixed ones are listed below.
    pub(super) fn shortcuts_ui(&mut self, ui: &mut Ui, t: &Strings, picked: &mut config::Settings) {
        let muted = self.theme.text_muted;
        // Recording: the next key pressed with a modifier becomes the shortcut; Escape cancels.
        if let Some(action) = self.shortcut_capture {
            let typed = ui.input_mut(|i| {
                let found = i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                });
                if found.is_some() {
                    i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
                }
                found
            });
            match typed {
                Some((Key::Escape, _)) => self.shortcut_capture = None,
                Some((key, m)) if m.command || m.ctrl || m.alt || m.mac_cmd => {
                    *action.get_mut(&mut picked.shortcuts) = config::Shortcut::typed(m, key);
                    self.shortcut_capture = None;
                }
                _ => {}
            }
        }

        let defaults = config::Shortcuts::default();
        let parsed: Vec<Option<KeyboardShortcut>> = ShortcutAction::ALL.iter().map(|a| a.get(&picked.shortcuts).parse()).collect();
        let theme = self.theme.clone();
        egui::ScrollArea::vertical().max_height(ui.available_height()).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 12.0);
            card(ui, &theme, None, |ui| {
                for (i, action) in ShortcutAction::ALL.into_iter().enumerate() {
                    if i > 0 {
                        divider(ui, &theme);
                    }
                    let current = action.get(&picked.shortcuts).clone();
                    let default = action.get(&defaults).clone();
                    let recording = self.shortcut_capture == Some(action);
                    // The same combination on two actions: only one would work.
                    let conflict = parsed[i].is_some() && parsed.iter().enumerate().any(|(j, p)| j != i && *p == parsed[i]);
                    setting_row(ui, &theme, action.label(t), conflict.then_some(t.shortcut_conflict), |ui| {
                        let text = if recording { t.shortcut_press.to_owned() } else { current.label() };
                        let color = if recording { theme.accent } else if conflict { theme.ansi[1] } else { theme.text };
                        let stroke = if recording { Stroke::new(1.5, theme.accent) } else { Stroke::new(1.0, theme.tab_hover) };
                        let button = egui::Button::new(egui::RichText::new(text).size(13.5).monospace().color(color)).stroke(stroke).corner_radius(6.0).min_size(Vec2::new(150.0, 30.0));
                        if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.shortcut_capture = if recording { None } else { Some(action) };
                        }
                        if current != default {
                            let reset = egui::Button::new(egui::RichText::new("↺").size(14.0).color(theme.text_muted)).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::splat(30.0));
                            if ui.add(reset).on_hover_text(t.shortcut_reset).clicked() {
                                *action.get_mut(&mut picked.shortcuts) = default;
                            }
                        }
                    });
                }
            });
            let mac = cfg!(target_os = "macos");
            let fixed = [
                (t.copy, if mac { "⌘ C" } else { "Ctrl+Shift+C" }),
                (t.paste, if mac { "⌘ V" } else { "Ctrl+Shift+V" }),
                (t.shortcut_move_pane, if mac { "⌘ ← ↑ → ↓" } else { "Ctrl+Alt+← ↑ → ↓" }),
                (t.shortcut_clear_line, if mac { "⌘ ⌫" } else { "Ctrl+U" }),
                (t.shortcut_zoom, if mac { "⌘ +   ⌘ −   ⌘ 0" } else { "Ctrl++   Ctrl+−   Ctrl+0" }),
            ];
            card(ui, &theme, Some(t.shortcut_fixed), |ui| {
                for (i, (label, keys)) in fixed.into_iter().enumerate() {
                    if i > 0 {
                        divider(ui, &theme);
                    }
                    setting_row(ui, &theme, label, None, |ui| {
                        ui.label(egui::RichText::new(keys).size(13.5).monospace().color(theme.text_muted));
                    });
                }
            });
        });
        let _ = muted;
    }

    /// "Informations" settings page: logo, version and updates, link to the project.
    pub(super) fn about_ui(&mut self, ui: &mut Ui, ctx: &egui::Context, t: &Strings, picked: &mut config::Settings) {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            let (logo, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::hover());
            paint_metal(ui.painter(), logo.center(), Align2::CENTER_CENTER, "Ronnie", 64.0, self.theme.accent, 1.0);
            ui.add_space(4.0);
            // Tagline and the sign of the horns (an image: egui's emoji font lacks it).
            let hand = self.metal_hand.get_or_insert_with(|| load_png(ctx, "metal-hand", include_bytes!("../../assets/icon/metal-hand.png")));
            let galley = ui.painter().layout_no_wrap(t.tagline.to_owned(), FontId::proportional(14.0), self.theme.text_muted);
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            let left = row.center().x - (galley.size().x + 26.0) / 2.0;
            let text_pos = Pos2::new(left, row.center().y - galley.size().y / 2.0);
            let hand_rect = Rect::from_center_size(Pos2::new(left + galley.size().x + 15.0, row.center().y), Vec2::splat(20.0));
            ui.painter().galley(text_pos, galley, self.theme.text_muted);
            egui::Image::new(&*hand).paint_at(ui, hand_rect);
            ui.add_space(22.0);
        });
        ui.add_space(8.0);
        let theme = self.theme.clone();
        card(ui, &theme, Some(t.updates), |ui| {
            setting_row(ui, &theme, &t.version.replace("{v}", update::VERSION), None, |ui| {
                if ui.add_enabled(!self.updater.busy(), egui::Button::new(egui::RichText::new(t.check_now).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    self.update_dismissed = false;
                    self.updater.check(ctx);
                }
                let muted = |s: &str| egui::RichText::new(s.to_owned()).size(13.0).color(theme.text_muted);
                match self.updater.state() {
                    update::State::Checking => {
                        ui.label(muted(t.checking));
                        ui.spinner();
                    }
                    update::State::UpToDate => {
                        ui.label(muted(t.up_to_date));
                    }
                    update::State::Available(a) => {
                        // Through Ronnie's own opener: egui's links do nothing in this build of eframe.
                        if ui.link(t.release_notes).on_hover_text(&a.url).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && a.url.starts_with("https://") {
                            crate::terminal::open_url(&a.url);
                        }
                        if ui.button(t.update_now).clicked() {
                            self.update_attempted = true;
                            self.update_dismissed = false;
                            self.updater.install(ctx, a.clone());
                        }
                        ui.label(egui::RichText::new(t.update_available.replace("{v}", &a.version)).size(13.0).color(theme.accent));
                    }
                    update::State::Installing(v) => {
                        ui.label(muted(&format!("{}  {}", t.installing.replace("{v}", &v), super::sidebar::download_text(self.updater.progress(), t))));
                        ui.spinner();
                    }
                    update::State::Installed(v) => {
                        if ui.button(t.restart).clicked() {
                            self.request_close(CloseRequest::Restart);
                        }
                        ui.label(muted(&t.update_installed.replace("{v}", &v)));
                    }
                    update::State::Failed(e) => {
                        ui.label(egui::RichText::new(t.update_failed).size(13.0).color(theme.ansi[1])).on_hover_text(e);
                    }
                    update::State::Idle => {}
                }
            });
            divider(ui, &theme);
            setting_row(ui, &theme, t.auto_update, None, |ui| {
                toggle(ui, &theme, &mut picked.auto_update);
            });
        });
        let icon = self.github_icon.get_or_insert_with(|| load_png(ctx, "github-mark", include_bytes!("../../assets/icon/github-mark.png"))).clone();
        card(ui, &theme, Some(t.project), |ui| {
            setting_row(ui, &theme, update::REPO, None, |ui| {
                let releases = egui::Button::new(egui::RichText::new(t.all_releases).size(13.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
                if ui.add(releases).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    crate::terminal::open_url(&format!("https://github.com/{}/releases", update::REPO));
                }
                let logo = egui::Image::new(&icon).fit_to_exact_size(Vec2::splat(16.0)).tint(theme.text);
                let github = egui::Button::image_and_text(logo, egui::RichText::new("GitHub").size(13.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
                if ui.add(github).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    crate::terminal::open_url(&format!("https://github.com/{}", update::REPO));
                }
            });
        });
    }

    /// The config file, editable as JSON. Saving validates it first: a mistake is reported, never written.
    /// The log (ronnie.log), newest at the bottom: copy it for a bug report, or clear it.
    pub(super) fn logs_ui(&mut self, ui: &mut Ui, t: &Strings, theme: &Theme) {
        use chrono::TimeZone as _;
        let entries = self.logs.get_or_insert_with(crate::log::entries);
        ui.label(egui::RichText::new(t.logs_hint).size(13.0).color(theme.text_muted));
        ui.add_space(6.0);
        let path = crate::log::log_path();
        ui.horizontal(|ui| {
            if let Some(path) = &path {
                let label = egui::RichText::new(path.display().to_string()).monospace().size(12.0).color(theme.text_muted);
                ui.add_sized(Vec2::new(EDITOR_WIDTH - 200.0, 20.0), egui::Label::new(label).truncate()).on_hover_text(path.display().to_string());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let exists = path.as_ref().is_some_and(|p| p.exists());
                if ui.add_enabled(exists, egui::Button::new(t.reveal_file)).clicked() {
                    if let Some(path) = &path {
                        config::reveal(path);
                    }
                }
            });
        });
        ui.add_space(6.0);

        let time = |secs: i64| chrono::Local.timestamp_opt(secs, 0).single().map(|d| d.format("%d/%m/%Y %H:%M:%S").to_string()).unwrap_or_default();
        let font = egui::FontId::new(12.5, egui::FontFamily::Name("mono".into()));
        let row_height = ui.fonts_mut(|f| f.row_height(&font)) + 2.0;
        let frame = Frame::new().fill(theme.bg).corner_radius(6.0).inner_margin(8.0).stroke(Stroke::new(1.0, theme.tab_hover));
        frame.show(ui, |ui| {
            if entries.is_empty() {
                ui.label(egui::RichText::new(t.logs_empty).size(13.0).color(theme.text_muted));
                return;
            }
            egui::ScrollArea::both().max_height(ui.available_height() - 50.0).auto_shrink(false).stick_to_bottom(true).show_rows(ui, row_height, entries.len(), |ui, rows| {
                for entry in &entries[rows] {
                    let mut job = egui::text::LayoutJob::default();
                    let stamp = entry.time.map(time).unwrap_or_default();
                    job.append(&format!("{stamp:<19}  "), 0.0, egui::TextFormat::simple(font.clone(), theme.text_muted));
                    let color = if entry.error { theme.ansi[1] } else { theme.fg };
                    job.append(&entry.text, 0.0, egui::TextFormat::simple(font.clone(), color));
                    ui.add(egui::Label::new(job).extend());
                }
            });
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear = egui::Button::new(egui::RichText::new(t.logs_clear).size(14.0).color(theme.ansi[1]))
                    .stroke(Stroke::new(1.0, theme.ansi[1].gamma_multiply(0.7)))
                    .fill(Color32::TRANSPARENT)
                    .min_size(Vec2::new(0.0, 30.0));
                if ui.add_enabled(!entries.is_empty(), clear).clicked() {
                    match crate::log::clear() {
                        Ok(()) => entries.clear(),
                        Err(e) => self.error = Some(format!("{e:#}")),
                    }
                }
                if ui.add_enabled(!entries.is_empty(), egui::Button::new(egui::RichText::new(t.copy).size(14.0)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    let text: Vec<String> = entries.iter().map(|e| format!("{}  {}{}", e.time.map(time).unwrap_or_default(), if e.error { "ERROR " } else { "" }, e.text)).collect();
                    ui.ctx().copy_text(text.join("\n"));
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.logs_refresh).size(14.0)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    *entries = crate::log::entries();
                }
            });
        });
    }

    pub(super) fn config_editor_ui(&mut self, ui: &mut Ui, t: &Strings) {
        let current = self.config.to_json();
        let editor = self.editor.get_or_insert_with(|| ConfigEditor::new(current.clone()));
        if editor.text == editor.base && editor.base != current {
            // Nothing typed: follow changes made elsewhere (renamed tab, outside edit...).
            *editor = ConfigEditor::new(current.clone());
        }
        let dirty = editor.text != editor.base;

        ui.label(egui::RichText::new(t.config_hint).size(13.0).color(self.theme.text_muted));
        ui.add_space(6.0);
        let path = config::config_path();
        ui.horizontal(|ui| {
            if let Some(path) = &path {
                let label = egui::RichText::new(path.display().to_string()).monospace().size(12.0).color(self.theme.text_muted);
                ui.add_sized(Vec2::new(EDITOR_WIDTH - 200.0, 20.0), egui::Label::new(label).truncate())
                    .on_hover_text(path.display().to_string());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The file only exists once something was saved.
                let exists = path.as_ref().is_some_and(|p| p.exists());
                if ui.add_enabled(exists, egui::Button::new(t.reveal_file)).clicked() {
                    if let Some(path) = &path {
                        config::reveal(path);
                    }
                }
            });
        });
        ui.add_space(6.0);

        let frame = Frame::new().fill(self.theme.bg).corner_radius(6.0).inner_margin(8.0).stroke(Stroke::new(1.0, self.theme.tab_hover));
        frame.show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(ui.available_height() - 50.0).auto_shrink(false).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut editor.text)
                        .font(egui::FontId::new(13.0, egui::FontFamily::Name("mono".into())))
                        .code_editor()
                        .frame(Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .desired_rows(24)
                        .text_color(self.theme.fg),
                );
            });
        });
        ui.add_space(8.0);

        let mut apply = None;
        ui.horizontal(|ui| {
            if let Some(err) = &editor.error {
                ui.label(egui::RichText::new(format!("{} : {err}", t.invalid_json)).size(13.0).color(self.theme.ansi[1]));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(dirty, egui::Button::new(egui::RichText::new(t.save).size(14.0)).min_size(Vec2::new(110.0, 30.0))).clicked() {
                    match Config::from_json(&editor.text) {
                        Ok(config) => apply = Some(config),
                        Err(e) => editor.error = Some(e.to_string()),
                    }
                }
                if ui.add_enabled(dirty, egui::Button::new(egui::RichText::new(t.revert).size(14.0)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    *editor = ConfigEditor::new(current.clone());
                }
            });
        });
        if let Some(config) = apply {
            self.apply_config(config);
            self.config_writable = !self.read_only;
            self.save_config();
            // Show the file as written (normalized formatting, defaults filled in).
            self.editor = Some(ConfigEditor::new(self.config.to_json()));
        }
    }
}

/// A page of the settings in the side navigation. Returns whether it was clicked.
fn nav_item(ui: &mut Ui, theme: &Theme, icon: &str, label: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
    let fill = if selected { theme.tab_active } else if resp.hovered() { theme.tab_hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, 8.0, fill);
    if selected {
        ui.painter().rect_filled(Rect::from_min_size(rect.min + Vec2::new(0.0, 8.0), Vec2::new(3.0, rect.height() - 16.0)), 2.0, theme.accent);
    }
    let color = if selected { theme.text } else { theme.text_muted };
    let icon_color = if selected { theme.accent } else { color };
    let at = Pos2::new(rect.min.x + 22.0, rect.center().y);
    // "About": an i in a circle (the font has no ⓘ).
    if icon == "i" {
        ui.painter().circle_stroke(at, 7.0, Stroke::new(1.3, icon_color));
        ui.painter().text(at + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, "i", FontId::proportional(11.0), icon_color);
    } else if icon == "★" {
        // "Features": a five-pointed star, drawn (the font may lack ★).
        let points = (0..10)
            .map(|k| {
                let angle = std::f32::consts::PI * (k as f32 / 5.0 - 0.5);
                let r = if k % 2 == 0 { 7.5 } else { 3.2 };
                at + Vec2::new(angle.cos(), angle.sin()) * r + Vec2::new(0.0, 0.5)
            })
            .collect();
        ui.painter().add(egui::Shape::closed_line(points, Stroke::new(1.3, icon_color)));
    } else {
        ui.painter().text(at, Align2::CENTER_CENTER, icon, FontId::proportional(14.0), icon_color);
    }
    // Cut with "…" if it doesn't fit.
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), FontId::proportional(14.0), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 50.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(Pos2::new(rect.min.x + 42.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A small heading over a group of settings.
fn section_title(ui: &mut Ui, theme: &Theme, title: &str) {
    ui.label(egui::RichText::new(title.to_uppercase()).size(11.5).strong().color(theme.text_muted).extra_letter_spacing(0.8));
    ui.add_space(8.0);
}

/// A group of settings in a rounded card, under its heading.
fn card(ui: &mut Ui, theme: &Theme, title: Option<&str>, add: impl FnOnce(&mut Ui)) {
    if let Some(title) = title {
        section_title(ui, theme, title);
    }
    Frame::NONE.fill(theme.bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(18, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    ui.add_space(26.0);
}

/// "Features" page: what Ronnie does, a card per theme, a line per feature.
fn features_page(ui: &mut Ui, t: &Strings, theme: &Theme) {
    ui.set_width(ui.available_width() - 12.0);
    for (title, features) in t.features {
        card(ui, theme, Some(title), |ui| {
            for (k, (name, what)) in features.iter().enumerate() {
                if k > 0 {
                    divider(ui, theme);
                }
                ui.add_space(9.0);
                ui.label(egui::RichText::new(*name).size(14.0).color(theme.text));
                ui.add_space(1.0);
                ui.add(egui::Label::new(egui::RichText::new(*what).size(12.0).color(theme.text_muted)).wrap());
                ui.add_space(9.0);
            }
        });
    }
}

/// Thin line between two settings of a card.
fn divider(ui: &mut Ui, theme: &Theme) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.8)));
}

/// One setting: its name and what it does on the left, its control on the right, centered on the row.
fn setting_row(ui: &mut Ui, theme: &Theme, title: &str, desc: Option<&str>, control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let control_w = 260.0_f32.min(width * 0.45);
    let text_w = (width - control_w - 16.0).max(80.0);
    let title_galley = ui.painter().layout(title.to_owned(), FontId::proportional(14.0), theme.text, text_w);
    let desc_galley = desc.map(|d| ui.painter().layout(d.to_owned(), FontId::proportional(12.0), theme.text_muted, text_w));
    let text_h = title_galley.size().y + desc_galley.as_ref().map_or(0.0, |g| g.size().y + 3.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, (text_h + 26.0).max(52.0)), Sense::hover());
    let top = rect.center().y - text_h / 2.0;
    let title_h = title_galley.size().y;
    ui.painter().galley(Pos2::new(rect.min.x, top), title_galley, theme.text);
    if let Some(g) = desc_galley {
        ui.painter().galley(Pos2::new(rect.min.x, top + title_h + 3.0), g, theme.text_muted);
    }
    let controls = Rect::from_min_max(Pos2::new(rect.max.x - control_w, rect.min.y), rect.max);
    // A child that leaves the row's layout alone (the row is already allocated).
    let mut controls_ui = ui.new_child(egui::UiBuilder::new().max_rect(controls).layout(egui::Layout::right_to_left(egui::Align::Center)));
    controls_ui.spacing_mut().slider_width = 150.0;
    control(&mut controls_ui);
}

/// An on / off switch. Changed when clicked.
fn toggle(ui: &mut Ui, theme: &Theme, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let k = ui.ctx().animate_bool_responsive(resp.id, *on);
    let off = theme.tab_hover.gamma_multiply(1.6);
    let fill = Color32::from(egui::lerp(egui::Rgba::from(off)..=egui::Rgba::from(theme.accent), k));
    ui.painter().rect_filled(rect, 11.0, fill);
    let x = egui::lerp(rect.min.x + 11.0..=rect.max.x - 11.0, k);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), 8.0, if *on { theme.bg } else { theme.text_muted });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Where the notices show: a small window with its six places, the chosen one lit.
fn position_picker(ui: &mut Ui, theme: &Theme, value: &mut config::ToastPosition) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(112.0, 64.0), Sense::hover());
    ui.painter().rect_filled(rect, 7.0, theme.chrome_bg);
    ui.painter().rect_stroke(rect, 7.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(1.4)), egui::StrokeKind::Inside);
    // The sidebar, as in the window.
    ui.painter().rect_filled(Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(18.0, rect.height() - 8.0)), 4.0, theme.tab_hover);
    let inner = Rect::from_min_max(rect.min + Vec2::new(26.0, 6.0), rect.max - Vec2::new(6.0, 6.0));
    for place in config::ToastPosition::ALL {
        let x = match place.side() {
            -1 => inner.min.x + 12.0,
            0 => inner.center().x,
            _ => inner.max.x - 12.0,
        };
        let y = if place.top() { inner.min.y + 7.0 } else { inner.max.y - 7.0 };
        let spot = Rect::from_center_size(Pos2::new(x, y), Vec2::new(22.0, 12.0));
        let resp = ui.interact(spot.expand(3.0), ui.id().with(("toast-place", place as u8)), Sense::click());
        let selected = *value == place;
        let fill = if selected { theme.accent } else if resp.hovered() { theme.text_muted.gamma_multiply(0.6) } else { theme.tab_hover.gamma_multiply(1.6) };
        ui.painter().rect_filled(spot, 3.0, fill);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *value = place;
        }
    }
}

/// A quiet button: its text only, a light fill on hover.
fn ghost_button(text: &str, color: Color32) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).size(13.0).color(color)).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))
}

