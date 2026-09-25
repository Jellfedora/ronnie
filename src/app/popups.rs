//! Popups over the panes: history / text search, saved commands, and the confirmation dialogs.

use super::*;

impl App {
    /// Scope of tab `index`'s own commands: its profile or SSH host, if any.
    pub(super) fn tab_scope(&self, index: usize) -> Option<(CommandScope, String)> {
        let tab = self.tabs.get(index)?;
        if let Some(host) = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)) {
            return Some((CommandScope::Host(host.id), host.name.clone()));
        }
        let profile = tab.profile.and_then(|id| self.config.profiles.iter().find(|p| p.id == id))?;
        Some((CommandScope::Profile(profile.id), profile.name(self.t().untitled).to_owned()))
    }

    pub(super) fn commands_mut(&mut self, scope: CommandScope) -> Option<&mut Vec<String>> {
        match scope {
            CommandScope::General => Some(&mut self.config.commands),
            CommandScope::Profile(id) => self.config.profiles.iter_mut().find(|p| p.id == id).map(|p| &mut p.commands),
            CommandScope::Host(id) => self.config.ssh.iter_mut().find(|h| h.id == id).map(|h| &mut h.commands),
        }
    }

    pub(super) fn commands_of(&self, scope: CommandScope) -> Vec<String> {
        match scope {
            CommandScope::General => self.config.commands.clone(),
            CommandScope::Profile(id) => self.config.profiles.iter().find(|p| p.id == id).map(|p| p.commands.clone()).unwrap_or_default(),
            CommandScope::Host(id) => self.config.ssh.iter().find(|h| h.id == id).map(|h| h.commands.clone()).unwrap_or_default(),
        }
    }

    /// Commands offered in tab `index`: its own first, then the general ones.
    pub(super) fn saved_commands(&self, index: usize) -> Vec<String> {
        let mut all = self.tab_scope(index).map(|(scope, _)| self.commands_of(scope)).unwrap_or_default();
        for c in &self.config.commands {
            if !all.contains(c) {
                all.push(c.clone());
            }
        }
        all
    }

    /// The ⚡ menu, under the pane's header on the right: click a command to write it at the prompt
    /// (it isn't run); ✕ removes it; the field at the bottom adds one.
    pub(super) fn commands_menu_ui(&mut self, ctx: &egui::Context) {
        let Some(menu) = &self.commands_menu else { return };
        let (index, pane) = (menu.tab, menu.pane);
        let Some(pane_rect) = self.tabs.get(index).filter(|_| index == self.active).and_then(|t| t.rects.iter().find(|(id, _)| *id == pane)).map(|(_, r)| *r) else {
            self.commands_menu = None;
            return;
        };
        let t = self.t();
        let tab_scope = self.tab_scope(index);
        let sections: Vec<(CommandScope, String, Vec<String>)> = tab_scope
            .iter()
            .map(|(scope, name)| (*scope, name.clone(), self.commands_of(*scope)))
            .chain(std::iter::once((CommandScope::General, t.commands_general.to_owned(), self.config.commands.clone())))
            .collect();
        let width = 360.0_f32.min(pane_rect.width() - 16.0);
        let pos = Pos2::new(pane_rect.max.x - width - 8.0, pane_rect.min.y + PANE_HEADER_H + 6.0);
        let theme = self.theme.clone();

        let mut insert = None;
        let mut remove = None;
        let mut add = None;
        let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let menu = self.commands_menu.as_mut().unwrap();
        let area = egui::Area::new(egui::Id::new("commands-menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.accent.gamma_multiply(0.6))).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(width - 20.0);
                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (scope, name, commands) in &sections {
                        ui.label(egui::RichText::new(name.to_uppercase()).size(11.0).strong().color(theme.text_muted));
                        if commands.is_empty() {
                            ui.label(egui::RichText::new(t.commands_empty).size(12.5).color(theme.text_muted.gamma_multiply(0.7)));
                        }
                        for (i, command) in commands.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let x = ui.add(egui::Button::new(egui::RichText::new("✕").size(11.0).color(theme.text_muted)).frame(false));
                                if x.clicked() {
                                    remove = Some((*scope, i));
                                }
                                let label = egui::RichText::new(command.replace('\n', " ⏎ ")).monospace().size(12.5).color(theme.text);
                                let row = ui.add(egui::Button::new(label).frame_when_inactive(false).truncate());
                                if row.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    insert = Some(command.clone());
                                }
                            });
                        }
                        ui.add_space(6.0);
                    }
                });
                ui.separator();
                let edit = ui.add(egui::TextEdit::singleline(&mut menu.new_command).hint_text(t.commands_new).font(FontId::monospace(12.5)).desired_width(f32::INFINITY));
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    if let Some((_, name)) = &tab_scope {
                        ui.selectable_value(&mut menu.for_tab, true, egui::RichText::new(name).size(12.5));
                        ui.selectable_value(&mut menu.for_tab, false, egui::RichText::new(t.commands_general).size(12.5));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button = egui::Button::new(egui::RichText::new(t.commands_add).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(5.0);
                        if (ui.add_enabled(!menu.new_command.trim().is_empty(), button).clicked() || enter) && !menu.new_command.trim().is_empty() {
                            let scope = match (&tab_scope, menu.for_tab) {
                                (Some((scope, _)), true) => *scope,
                                _ => CommandScope::General,
                            };
                            add = Some((scope, menu.new_command.trim().to_owned()));
                            menu.new_command.clear();
                        }
                    });
                });
                ui.label(egui::RichText::new(t.commands_hint).size(11.0).color(theme.text_muted));
            });
        });
        // A click elsewhere closes it (except on the pane header, where the ⚡ button toggles it). Tested
        // on the rect: `contains_pointer` is false over the menu's own widgets.
        let inside = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p));
        let clicked_outside = ctx.input(|i| i.pointer.any_click()) && !inside;

        if let Some((scope, i)) = remove {
            if let Some(list) = self.commands_mut(scope) {
                if i < list.len() {
                    list.remove(i);
                }
            }
        }
        if let Some((scope, command)) = add {
            if let Some(list) = self.commands_mut(scope) {
                if !list.contains(&command) {
                    list.push(command);
                }
            }
        }
        if let Some(command) = insert {
            if let Some(term) = self.tabs.get_mut(index).and_then(|t| t.panes.get_mut(&pane)) {
                term.paste_text(&command);
            }
            self.commands_menu = None;
            self.focus_terminal = true;
        } else if escape {
            self.commands_menu = None;
            self.focus_terminal = true;
        } else if clicked_outside && remove.is_none() {
            let over_header = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.y < pane_rect.min.y + PANE_HEADER_H && pane_rect.contains(p));
            if !over_header {
                self.commands_menu = None;
            }
        }
    }

    /// Opens the search box over a pane, on the displayed text (`text`) or on the commands typed there
    /// (local panes only). The same shortcut again closes it; the other one switches mode.
    pub(super) fn open_search(&mut self, index: usize, pane: PaneId, text: bool) {
        // One popup at a time: the search replaces the ⚡ menu.
        self.commands_menu = None;
        if let Some(search) = self.history_search.as_mut().filter(|s| s.tab == index && s.pane == pane) {
            if search.text == text {
                self.close_search();
            } else {
                search.text = text;
                search.fresh = true;
            }
            return;
        }
        self.close_search();
        let Some(tab) = self.tabs.get_mut(index) else { return };
        let local = tab.ssh.is_none();
        let entries = if local { tab.history_path(pane).map(|p| crate::shell::read_history(&p)).unwrap_or_default() } else { Vec::new() };
        let lower = entries.iter().map(|e| e.to_lowercase()).collect();
        self.history_search = Some(HistorySearch { tab: index, pane, query: String::new(), entries, lower, selected: 0, fresh: true, text: text || !local, local, status: (0, 0) });
    }

    pub(super) fn close_search(&mut self) {
        if let Some(search) = self.history_search.take() {
            if let Some(term) = self.tabs.get_mut(search.tab).and_then(|t| t.panes.get_mut(&search.pane)) {
                term.clear_find();
            }
            self.focus_terminal = true;
        }
    }

    /// The search box, at the top of its pane. Text mode highlights the occurrences in the output
    /// (Enter: older one, Shift+Enter: newer one). Commands mode lists the commands typed in the pane:
    /// Enter pastes one at the prompt, Cmd+Enter runs it.
    pub(super) fn history_search_ui(&mut self, ctx: &egui::Context) {
        let Some(search) = &mut self.history_search else { return };
        let Some(pane_rect) = self.tabs.get(search.tab).filter(|_| search.tab == self.active).and_then(|t| t.rects.iter().find(|(id, _)| *id == search.pane)).map(|(_, r)| *r) else {
            self.history_search = None;
            return;
        };
        let (index, pane) = (search.tab, search.pane);
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        let shortcuts = self.config.settings.shortcuts.clone();
        let width = (pane_rect.width() - 32.0).clamp(200.0, 620.0);
        let pos = Pos2::new(pane_rect.center().x - width / 2.0, pane_rect.min.y + PANE_HEADER_H + 10.0);

        let (up, down, newer, enter, run, escape) = ctx.input_mut(|i| {
            let run = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter));
            let newer = i.consume_key(Modifiers::SHIFT, Key::Enter);
            (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), newer, i.consume_key(Modifiers::NONE, Key::Enter), run, i.consume_key(Modifiers::NONE, Key::Escape))
        });
        let count = search.matches().len();
        if !search.text {
            if up {
                search.selected = search.selected.saturating_sub(1);
            }
            if down && count > 0 {
                search.selected = (search.selected + 1).min(count - 1);
            }
        }
        let mut chosen: Option<(String, bool)> = None;
        let mut find: Option<String> = None;
        let mut step: Option<bool> = None; // Some(true): older occurrence
        let mut close = escape;
        let was_text = search.text;

        egui::Area::new(egui::Id::new("history-search")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.accent.gamma_multiply(0.6))).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.horizontal(|ui| {
                    let text_label = format!("{}  {}", t.search_text, shortcuts.find_text.label());
                    if ui.selectable_label(search.text, egui::RichText::new(text_label).size(12.5)).clicked() {
                        search.text = true;
                    }
                    if search.local {
                        let commands_label = format!("{}  {}", t.search_commands, shortcuts.find_commands.label());
                        if ui.selectable_label(!search.text, egui::RichText::new(commands_label).size(12.5)).clicked() {
                            search.text = false;
                        }
                    }
                });
                if search.text != was_text {
                    search.fresh = true;
                    // Switching to text mode searches what was typed; leaving it clears the highlights.
                    find = Some(if search.text { search.query.clone() } else { String::new() });
                }
                ui.add_space(4.0);
                let hint = if search.text { t.search_text_hint } else { t.history_search };
                let edit = ui.add(egui::TextEdit::singleline(&mut search.query).hint_text(format!("🔍  {hint}")).font(FontId::monospace(13.0)).desired_width(f32::INFINITY));
                if search.fresh || !edit.has_focus() {
                    edit.request_focus();
                    search.fresh = false;
                }
                if edit.changed() {
                    search.selected = 0;
                    if search.text {
                        find = Some(search.query.clone());
                    }
                }
                ui.add_space(6.0);

                if search.text {
                    ui.horizontal(|ui| {
                        let (current, total) = search.status;
                        let status = if search.query.is_empty() { String::new() } else if total == 0 { t.search_none.to_owned() } else { format!("{current} / {total}") };
                        ui.label(egui::RichText::new(status).size(12.5).color(theme.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(total > 0, egui::Button::new("↓")).on_hover_text("⇧ ↩").clicked() {
                                step = Some(false);
                            }
                            if ui.add_enabled(total > 0, egui::Button::new("↑")).on_hover_text("↩").clicked() {
                                step = Some(true);
                            }
                        });
                    });
                    if enter || up {
                        step = Some(true);
                    }
                    if newer || down {
                        step = Some(false);
                    }
                    ui.label(egui::RichText::new(t.search_text_keys).size(11.0).color(theme.text_muted));
                    return;
                }

                let matches = search.matches();
                if matches.is_empty() {
                    ui.label(egui::RichText::new(t.history_empty).size(12.5).color(theme.text_muted));
                }
                egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, command) in matches.iter().enumerate() {
                        let selected = i == search.selected;
                        let one_line = command.replace('\n', " ⏎ ");
                        let text = egui::RichText::new(one_line).monospace().size(12.5).color(if selected { theme.text } else { theme.text_muted });
                        // Left-aligned in a row of its own height (a full-width button would center the command).
                        let row = ui.horizontal(|ui| ui.add(egui::Button::selectable(selected, text).truncate())).inner;
                        if selected && (up || down) {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            chosen = Some((command.to_string(), false));
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.history_search_hint).size(11.0).color(theme.text_muted));
                if (enter || run) && chosen.is_none() {
                    if let Some(command) = matches.get(search.selected) {
                        chosen = Some((command.to_string(), run));
                    }
                }
            });
        });

        if let Some(term) = self.tabs.get_mut(index).and_then(|t| t.panes.get_mut(&pane)) {
            let mut status = None;
            if let Some(query) = find {
                status = Some(if query.is_empty() {
                    term.clear_find();
                    (0, 0)
                } else {
                    term.find(&query)
                });
            }
            if let Some(older) = step {
                term.find_step(older);
                status = Some(term.find_status());
            }
            if let Some((command, run)) = &chosen {
                term.paste_text(command);
                if *run {
                    term.type_text("\r");
                }
                close = true;
            }
            if let (Some(status), Some(search)) = (status, self.history_search.as_mut()) {
                search.status = status;
            }
        }
        if close {
            self.close_search();
        }
    }

    /// Saves everything, starts the updated app and closes this one.
    pub(super) fn restart(&mut self) {
        self.sync();
        // Hand the config over to the new instance: nothing is written after this, and the lock is freed
        // so that it doesn't start read-only.
        self.read_only = true;
        self._instance_lock = None;
        match self.updater.relaunch() {
            Ok(()) => {
                self.close_confirmed = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(e) => self.error = Some(format!("{} : {e:#}", self.t().update_failed)),
        }
    }

    /// What closing these panes of tab `index` (all of them if None) would interrupt: programs running
    /// instead of the shell, and open SSH connections.
    pub(super) fn busy(&self, index: usize, panes: Option<&[PaneId]>) -> Vec<String> {
        let Some(tab) = self.tabs.get(index) else { return Vec::new() };
        let mut busy = Vec::new();
        for (id, term) in &tab.panes {
            if panes.is_some_and(|p| !p.contains(id)) || tab.dead.contains(id) || term.has_exited() {
                continue;
            }
            if tab.ssh.is_some() {
                busy.push(format!("{}  ·  ssh", tab.title()));
            } else if let Some(program) = term.foreground() {
                // The folder tells apart several panes running the same program.
                let dir = term.cwd().and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()));
                busy.push(match dir {
                    Some(dir) => format!("{}  ·  {program}  ({dir})", tab.title()),
                    None => format!("{}  ·  {program}", tab.title()),
                });
            }
        }
        // Panes are in a map: give the list a stable order.
        busy.sort();
        busy
    }

    pub(super) fn busy_for(&self, request: CloseRequest) -> Vec<String> {
        match request {
            CloseRequest::Pane(index, id) => self.busy(index, Some(&[id])),
            CloseRequest::Tab(index) => self.busy(index, None),
            CloseRequest::Window | CloseRequest::Restart => (0..self.tabs.len()).flat_map(|i| self.busy(i, None)).collect(),
        }
    }

    /// Closes right away, or asks first when that would interrupt running programs.
    pub(super) fn request_close(&mut self, request: CloseRequest) {
        let busy = self.busy_for(request);
        if busy.is_empty() {
            self.do_close(request);
        } else {
            self.confirm_close = Some(ConfirmClose { request, busy });
        }
    }

    pub(super) fn do_close(&mut self, request: CloseRequest) {
        match request {
            CloseRequest::Pane(index, id) => self.close_pane(index, id),
            CloseRequest::Tab(index) => self.close_tab(index),
            CloseRequest::Window => {
                self.close_confirmed = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            CloseRequest::Restart => self.restart(),
        }
    }

    /// "Paste N lines?" when the program would run each pasted line at once (no bracketed paste).
    pub(super) fn paste_confirm_window(&mut self, ctx: &egui::Context) {
        if self.paste_confirm.is_none() {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                self.paste_confirm = tab.panes.iter_mut().find_map(|(id, term)| Some((*id, term.take_pending_paste()?)));
            }
        }
        let Some((pane, text)) = self.paste_confirm.clone() else { return };
        let t = self.t();
        let lines: Vec<&str> = text.lines().collect();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-paste")).frame(frame).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(egui::RichText::new(t.paste_title.replace("{n}", &lines.len().to_string())).size(17.0).strong());
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.paste_body).size(13.5).color(self.theme.text_muted));
            ui.add_space(6.0);
            Frame::new().fill(self.theme.bg).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for line in lines.iter().take(8) {
                    ui.add(egui::Label::new(egui::RichText::new(*line).monospace().size(12.5)).truncate());
                }
                if lines.len() > 8 {
                    ui.label(egui::RichText::new(format!("…  +{}", lines.len() - 8)).size(12.5).color(self.theme.text_muted));
                }
            });
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(t.paste_confirm).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                cancel.request_focus();
                if cancel.clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        if let Some(paste) = answer {
            if paste {
                if let Some(term) = self.tabs.get_mut(self.active).and_then(|t| t.panes.get_mut(&pane)) {
                    term.paste_confirmed(&text);
                }
            }
            self.paste_confirm = None;
            self.focus_terminal = true;
        }
    }

    /// "Open this link?" for links that aren't web addresses, showing the exact target.
    pub(super) fn link_confirm_window(&mut self, ctx: &egui::Context) {
        // Links clicked in any pane of the visible tab.
        if self.link_confirm.is_none() {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                self.link_confirm = tab.panes.values_mut().find_map(Terminal::take_link_request);
            }
        }
        let Some(url) = self.link_confirm.clone() else { return };
        let t = self.t();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-link")).frame(frame).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(egui::RichText::new(t.open_link_title).size(17.0).strong());
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.open_link_body).size(13.5).color(self.theme.text_muted));
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(&url).monospace().size(12.5)).wrap());
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(t.open_link).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                cancel.request_focus();
                if cancel.clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        if let Some(open) = answer {
            if open {
                crate::terminal::open_url(&url);
            }
            self.link_confirm = None;
            self.focus_terminal = true;
        }
    }

    /// "Reset everything?" dialog: erases the whole configuration, then restarts.
    pub(super) fn confirm_reset_window(&mut self, ctx: &egui::Context) {
        if !self.confirm_reset {
            return;
        }
        let t = self.t();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-reset")).frame(frame).backdrop_color(Color32::from_black_alpha(190)).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(egui::RichText::new(format!("⚠  {}", t.reset_title)).size(17.0).strong().color(self.theme.ansi[1]));
            ui.add_space(10.0);
            ui.label(egui::RichText::new(t.reset_body).size(13.5));
            if let Some(dir) = config::config_dir() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(dir.display().to_string()).size(11.5).monospace().color(self.theme.text_muted));
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(t.reset_backup).size(12.5).color(self.theme.text_muted));
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let erase = egui::Button::new(egui::RichText::new(t.reset_confirm).size(13.5).color(Color32::WHITE)).fill(self.theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(110.0, 30.0));
                if ui.add(erase).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                // Cancel is the default: Enter or Escape erase nothing.
                cancel.request_focus();
                if cancel.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                self.confirm_reset = false;
                self.reset_everything();
            }
            Some(false) => self.confirm_reset = false,
            None => {}
        }
    }

    /// Erases every profile, host, setting, tab and history (and the saved passwords), then restarts.
    pub(super) fn reset_everything(&mut self) {
        for host in self.config.ssh.iter().filter(|h| h.password_saved) {
            ssh::delete_password(host.id);
        }
        // Nothing may be written again before quitting (not even the session on exit).
        self.read_only = true;
        self.config_writable = false;
        if let Err(e) = config::erase_all() {
            self.error = Some(format!("{e:#}"));
            return;
        }
        crate::log::info("configuration reset (moved to a backup directory)");
        self._instance_lock = None;
        match self.updater.relaunch() {
            Ok(()) => {
                self.close_confirmed = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    /// "Close anyway?" dialog listing the programs that would be stopped.
    pub(super) fn confirm_close_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm_close else { return };
        let t = self.t();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(self.theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("confirm-close")).frame(frame).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new(t.close_anyway_title).size(17.0).strong());
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.close_anyway_body).size(13.5).color(self.theme.text_muted));
            ui.add_space(6.0);
            for item in confirm.busy.iter().take(8) {
                ui.label(egui::RichText::new(format!("•  {item}")).size(13.0).monospace());
            }
            if confirm.busy.len() > 8 {
                ui.label(egui::RichText::new(format!("…  +{}", confirm.busy.len() - 8)).size(13.0).color(self.theme.text_muted));
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = egui::Button::new(egui::RichText::new(t.close).size(13.5).color(Color32::WHITE)).fill(self.theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0));
                if ui.add(close).clicked() {
                    answer = Some(true);
                }
                let cancel = ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0)));
                // Cancel is the default: Enter or Escape keep everything running.
                cancel.request_focus();
                if cancel.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                let request = confirm.request;
                self.confirm_close = None;
                self.do_close(request);
            }
            Some(false) => {
                self.confirm_close = None;
                self.focus_terminal = true;
            }
            None => {}
        }
    }
}
