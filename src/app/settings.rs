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
        if self.config.profiles.is_empty() {
            ui.label(egui::RichText::new(t.no_profiles).size(13.0).color(self.theme.text_muted));
            return;
        }
        let (mut rename, mut recolor, mut open, mut delete) = (None, None, None, None);
        let sections = self.grouped_ids(true);
        egui::ScrollArea::vertical().max_height(ui.available_height()).show(ui, |ui| {
            for (name, ids) in &sections {
                if let Some(name) = name {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(name.to_uppercase()).size(12.0).strong().color(self.theme.text_muted));
                }
            for p in ids.iter().filter_map(|id| self.config.profiles.iter().find(|p| p.id == *id)) {
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    let dot = egui::RichText::new("●").size(18.0).color(p.tab.color.unwrap_or(self.theme.text_muted));
                    ui.menu_button(dot, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for color in TAB_COLORS {
                                let (r, s) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                                ui.painter().circle_filled(r.center(), 7.5, color);
                                if p.tab.color == Some(color) || s.hovered() {
                                    ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, self.theme.text));
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
                    let current = p.name(t.untitled).to_owned();
                    let buffer = self.profile_names.entry(p.id).or_insert_with(|| current.clone());
                    let edit = ui.add(egui::TextEdit::singleline(buffer).desired_width(250.0).margin(Vec2::new(6.0, 5.0)).font(FontId::proportional(14.0)));
                    if edit.lost_focus() && *buffer != current {
                        rename = Some((p.id, buffer.clone()));
                    }
                    let panes = p.tab.layout.panes();
                    ui.label(egui::RichText::new(format!("{panes} {}", t.layout_panes)).size(12.0).color(self.theme.text_muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                            delete = Some(p.id);
                        }
                        if ui.button(t.open).clicked() {
                            open = Some(p.id);
                        }
                    });
                });
            }
            }
        });
        if let Some(err) = self.profile_error {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(err).size(13.0).color(self.theme.ansi[1]));
        }
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
        let muted = self.theme.text_muted;
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).size(12.0).strong().color(muted));
            ui.add_space(4.0);
        };
        heading(ui, t.import_title);
        Frame::new().fill(self.theme.bg).corner_radius(6.0).inner_margin(12.0).stroke(Stroke::new(1.0, self.theme.tab_hover)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(t.import_explain).size(13.0));
            ui.add_space(8.0);
            match self.new_ssh_config_hosts() {
                Err(_) => {
                    ui.label(egui::RichText::new(t.no_ssh_config).size(13.0).color(muted));
                }
                Ok((total, new)) => {
                    ui.horizontal(|ui| {
                        let summary = t.import_found.replace("{total}", &total.to_string()).replace("{new}", &new.len().to_string());
                        ui.label(egui::RichText::new(summary).size(13.0).color(muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = t.import_button.replace("{new}", &new.len().to_string());
                            let button = egui::Button::new(egui::RichText::new(label).size(14.0)).min_size(Vec2::new(0.0, 30.0));
                            if ui.add_enabled(!new.is_empty(), button).clicked() {
                                self.import_ssh();
                            }
                        });
                    });
                }
            }
        });
        ui.add_space(16.0);

        heading(ui, t.ssh_hosts);
        if self.config.ssh.is_empty() {
            ui.label(egui::RichText::new(t.no_ssh_hosts).size(13.0).color(muted));
            return;
        }
        let (mut edit, mut delete) = (None, None);
        let sections = self.grouped_ids(false);
        egui::ScrollArea::vertical().max_height(ui.available_height()).show(ui, |ui| {
            for (name, ids) in &sections {
                if let Some(name) = name {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(name.to_uppercase()).size(12.0).strong().color(muted));
                }
            for host in ids.iter().filter_map(|id| self.config.ssh.iter().find(|h| h.id == *id)) {
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    ui.label(egui::RichText::new("●").size(16.0).color(host.color.unwrap_or(muted)));
                    ui.label(egui::RichText::new(&host.name).size(14.0));
                    ui.label(egui::RichText::new(host.address()).size(12.0).color(muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                            delete = Some(host.id);
                        }
                        if ui.button(t.edit).clicked() {
                            edit = Some(host.id);
                        }
                    });
                });
            }
            }
        });
        let imported = self.config.ssh.iter().filter(|h| h.imported).count();
        if imported > 0 {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = format!("{} ({imported})", t.delete_imported);
                if self.confirm_delete_imported {
                    ui.label(egui::RichText::new(t.delete_imported_confirm).size(13.0).color(self.theme.ansi[1]));
                    if ui.button(egui::RichText::new(t.confirm).color(self.theme.ansi[1])).clicked() {
                        let ids: Vec<Uuid> = self.config.ssh.iter().filter(|h| h.imported).map(|h| h.id).collect();
                        for id in ids {
                            self.delete_host(id);
                        }
                        self.confirm_delete_imported = false;
                    }
                    if ui.button(t.cancel).clicked() {
                        self.confirm_delete_imported = false;
                    }
                } else if ui.button(egui::RichText::new(label).color(self.theme.ansi[1])).clicked() {
                    self.confirm_delete_imported = true;
                }
            });
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
                    label(ui, t.password);
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
        let mut picked = self.config.settings.clone();
        let mut close = false;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("settings")).frame(frame).backdrop_color(Color32::from_black_alpha(190)).show(ctx, |ui| {
            // Same size on every tab: pages scroll inside, the window never jumps around.
            ui.set_width(SETTINGS_WIDTH);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.settings).size(18.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("✕").size(14.0)).frame(false)).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (tab, label) in [(SettingsTab::About, t.about), (SettingsTab::General, t.general), (SettingsTab::Profiles, t.manage_profiles), (SettingsTab::Ssh, t.ssh_tab), (SettingsTab::Shortcuts, t.shortcuts), (SettingsTab::ConfigFile, t.config_file)] {
                    let text = egui::RichText::new(label).size(14.0);
                    if ui.add(egui::Button::selectable(self.settings_tab == tab, text).min_size(Vec2::new(0.0, 28.0))).clicked() {
                        self.settings_tab = tab;
                    }
                }
            });
            ui.separator();
            ui.add_space(8.0);
            let body = Vec2::new(SETTINGS_WIDTH, SETTINGS_BODY_HEIGHT.min(ctx.content_rect().height() - 200.0).max(200.0));
            ui.allocate_ui_with_layout(body, egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(body);
            ui.set_max_height(body.y);
            match self.settings_tab {
                SettingsTab::ConfigFile => return self.config_editor_ui(ui, t),
                SettingsTab::Profiles => return self.profiles_ui(ui, t),
                SettingsTab::Ssh => return self.ssh_settings_ui(ui, t),
                SettingsTab::About => return self.about_ui(ui, ctx, t, &mut picked),
                SettingsTab::Shortcuts => return self.shortcuts_ui(ui, t, &mut picked),
                SettingsTab::General => {}
            }

            let heading_color = self.theme.text_muted;
            let heading = move |ui: &mut Ui, text: &str| {
                ui.label(egui::RichText::new(text).size(12.0).strong().color(heading_color));
                ui.add_space(4.0);
            };
            let max_height = ui.available_height();
            egui::ScrollArea::vertical().max_height(max_height).min_scrolled_height(max_height).show(ui, |ui| {
            heading(ui, &t.language.to_uppercase());
            ui.horizontal(|ui| {
                for lang in Lang::ALL {
                    let label = egui::RichText::new(lang.label()).size(14.0);
                    if ui.add(egui::Button::selectable(picked.language == lang, label).min_size(Vec2::new(110.0, 30.0))).clicked() {
                        picked.language = lang;
                    }
                }
            });
            ui.add_space(18.0);

            heading(ui, &t.display.to_uppercase());
            ui.checkbox(&mut picked.show_cwd, egui::RichText::new(t.show_cwd).size(14.0));
            ui.checkbox(&mut picked.clipboard_from_programs, egui::RichText::new(t.clipboard_from_programs).size(14.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.ui_zoom).size(14.0));
                let mut percent = (picked.ui_zoom * 100.0).round();
                if ui.add(egui::Slider::new(&mut percent, 60.0..=200.0).step_by(10.0).suffix(" %")).changed() {
                    picked.ui_zoom = percent / 100.0;
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.font_size).size(14.0));
                ui.add(egui::Slider::new(&mut picked.font_size, config::FONT_SIZES).step_by(1.0).suffix(" pt"));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.scrollback).size(14.0));
                ui.add(egui::Slider::new(&mut picked.scrollback, config::SCROLLBACK_LINES).logarithmic(true));
            });
            ui.add_space(18.0);

            heading(ui, &t.theme.to_uppercase());
            for (dark, label) in [(true, t.dark), (false, t.light)] {
                ui.label(egui::RichText::new(label).size(13.0).color(self.theme.text_muted));
                let presets: Vec<&Preset> = PRESETS.iter().filter(|p| p.dark == dark).collect();
                for pair in presets.chunks(3) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for preset in pair {
                            if theme_card(ui, preset, picked.theme == preset.id, self.theme.text).clicked() {
                                picked.theme = preset.id.to_owned();
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
            }
            ui.add_space(12.0);

            heading(ui, &t.reset_section.to_uppercase());
            let reset = egui::Button::new(egui::RichText::new(t.reset_button).size(13.5).color(self.theme.ansi[1]))
                .stroke(Stroke::new(1.0, self.theme.ansi[1].gamma_multiply(0.7)))
                .fill(Color32::TRANSPARENT)
                .corner_radius(6.0)
                .min_size(Vec2::new(0.0, 30.0));
            if ui.add(reset).clicked() {
                self.confirm_reset = true;
            }
            ui.add_space(8.0);
            });
            });
        });
        if self.settings_tab != SettingsTab::Shortcuts {
            self.shortcut_capture = None;
        }
        if close || modal.should_close() {
            self.settings_dialog = false;
            self.shortcut_capture = None;
            self.editor = None;
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
        egui::ScrollArea::vertical().max_height(ui.available_height()).show(ui, |ui| {
            egui::Grid::new("shortcuts-grid").num_columns(3).spacing([16.0, 8.0]).show(ui, |ui| {
                for (i, action) in ShortcutAction::ALL.into_iter().enumerate() {
                    let current = action.get(&picked.shortcuts).clone();
                    let default = action.get(&defaults).clone();
                    ui.label(egui::RichText::new(action.label(t)).size(14.0));
                    let recording = self.shortcut_capture == Some(action);
                    // The same combination on two actions: only one would work.
                    let conflict = parsed[i].is_some() && parsed.iter().enumerate().any(|(j, p)| j != i && *p == parsed[i]);
                    let text = if recording { t.shortcut_press.to_owned() } else { current.label() };
                    let color = if recording { self.theme.accent } else if conflict { self.theme.ansi[1] } else { self.theme.text };
                    let button = egui::Button::new(egui::RichText::new(text).size(13.5).monospace().color(color)).corner_radius(6.0).min_size(Vec2::new(150.0, 26.0));
                    let resp = ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let resp = if conflict { resp.on_hover_text(t.shortcut_conflict) } else { resp };
                    if resp.clicked() {
                        self.shortcut_capture = if recording { None } else { Some(action) };
                    }
                    if current != default {
                        if ui.button(t.shortcut_reset).clicked() {
                            *action.get_mut(&mut picked.shortcuts) = default;
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });

            ui.add_space(22.0);
            ui.label(egui::RichText::new(t.shortcut_fixed.to_uppercase()).size(12.0).strong().color(muted));
            ui.add_space(6.0);
            let mac = cfg!(target_os = "macos");
            let fixed = [
                (t.copy, if mac { "⌘ C" } else { "Ctrl+Shift+C" }),
                (t.paste, if mac { "⌘ V" } else { "Ctrl+Shift+V" }),
                (t.shortcut_move_pane, if mac { "⌘ ← ↑ → ↓" } else { "Ctrl+Alt+← ↑ → ↓" }),
                (t.shortcut_clear_line, if mac { "⌘ ⌫" } else { "Ctrl+U" }),
                (t.shortcut_zoom, if mac { "⌘ +   ⌘ −   ⌘ 0" } else { "Ctrl++   Ctrl+−   Ctrl+0" }),
            ];
            egui::Grid::new("fixed-shortcuts").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                for (label, keys) in fixed {
                    ui.label(egui::RichText::new(label).size(13.5).color(muted));
                    ui.label(egui::RichText::new(keys).size(13.5).monospace());
                    ui.end_row();
                }
            });
        });
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
        ui.separator();
        ui.add_space(14.0);
        ui.label(egui::RichText::new(t.updates.to_uppercase()).size(12.0).strong().color(self.theme.text_muted));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.version.replace("{v}", update::VERSION)).size(14.0));
            ui.add_space(12.0);
            if ui.add_enabled(!self.updater.busy(), egui::Button::new(egui::RichText::new(t.check_now).size(13.0))).clicked() {
                self.update_dismissed = false;
                self.updater.check(ctx);
            }
            let muted_color = self.theme.text_muted;
            let muted = move |s: &str| egui::RichText::new(s.to_owned()).size(13.0).color(muted_color);
            match self.updater.state() {
                update::State::Checking => {
                    ui.spinner();
                    ui.label(muted(t.checking));
                }
                update::State::UpToDate => {
                    ui.label(muted(t.up_to_date));
                }
                update::State::Available(a) => {
                    ui.label(egui::RichText::new(t.update_available.replace("{v}", &a.version)).size(13.0).color(self.theme.accent));
                    if ui.button(t.update_now).clicked() {
                        self.update_attempted = true;
                        self.update_dismissed = false;
                        self.updater.install(ctx, a.clone());
                    }
                    ui.hyperlink_to(t.release_notes, &a.url);
                }
                update::State::Installing(v) => {
                    ui.spinner();
                    ui.label(muted(&t.installing.replace("{v}", &v)));
                }
                update::State::Installed(v) => {
                    ui.label(muted(&t.update_installed.replace("{v}", &v)));
                    if ui.button(t.restart).clicked() {
                        self.request_close(CloseRequest::Restart);
                    }
                }
                update::State::Failed(e) => {
                    ui.label(egui::RichText::new(t.update_failed).size(13.0).color(self.theme.ansi[1])).on_hover_text(e);
                }
                update::State::Idle => {}
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut picked.auto_update, egui::RichText::new(t.auto_update).size(14.0));
        ui.add_space(22.0);

        ui.label(egui::RichText::new(t.project.to_uppercase()).size(12.0).strong().color(self.theme.text_muted));
        ui.add_space(4.0);
        let icon = self.github_icon.get_or_insert_with(|| load_png(ctx, "github-mark", include_bytes!("../../assets/icon/github-mark.png")));
        let logo = egui::Image::new(&*icon).fit_to_exact_size(Vec2::splat(16.0)).tint(self.theme.text);
        ui.horizontal(|ui| {
            let github = egui::Button::image_and_text(logo, egui::RichText::new(update::REPO).size(13.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
            if ui.add(github).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                crate::terminal::open_url(&format!("https://github.com/{}", update::REPO));
            }
            let releases = egui::Button::new(egui::RichText::new(t.all_releases).size(13.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
            if ui.add(releases).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                crate::terminal::open_url(&format!("https://github.com/{}/releases", update::REPO));
            }
        });
    }

    /// The config file, editable as JSON. Saving validates it first: a mistake is reported, never written.
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
