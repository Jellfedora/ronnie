//! The sidebar: logo, LOCAL terminals and profiles, SSH hosts and groups, update card, settings button.

use super::*;

impl App {
    /// Shown when every tab is closed: the app stays open.
    /// The home page: what just closed, the typing game, tips scrolling below (the sidebar leads back
    /// to the tabs).
    pub(super) fn home_page(&mut self, ui: &mut Ui, rect: Rect) {
        // The games' page: each game, to pick one.
        if self.home_game && self.games_hub {
            self.game.leave();
            self.fold_for_game(false);
            self.arcade_page(ui, rect);
            return;
        }
        if !self.home_game || self.home_which != super::four::HomeGame::SpeedMetal {
            self.game.leave();
            if self.home_game && self.home_which == super::four::HomeGame::Blob {
                // Its minutes of the half-day of work up: out of the game.
                if let Some(p) = self.game_locked(super::four::HomeGame::Blob) {
                    self.blob_leave();
                    self.fold_for_game(false);
                    self.locked_page(ui, rect, super::four::HomeGame::Blob, &p);
                    return;
                }
                // It folds the sidebar itself, during a life.
                self.games_drawn.push(super::four::HomeGame::Blob);
                self.blob_page(ui, rect);
                let dt = ui.input(|i| i.stable_dt).min(0.1);
                if let Some(left) = self.count_blob(dt) {
                    self.paint_blob_left(ui, rect, left);
                }
                return;
            }
            // These fold the sidebar themselves, during a run, a song, a show.
            if self.home_game && matches!(self.home_which, super::four::HomeGame::Runner | super::four::HomeGame::Riff | super::four::HomeGame::Backstage) {
                self.games_drawn.push(self.home_which);
                return match self.home_which {
                    super::four::HomeGame::Runner => self.runner_page(ui, rect),
                    super::four::HomeGame::Riff => self.riff_page(ui, rect),
                    _ => self.backstage_page(ui, rect),
                };
            }
            self.fold_for_game(false);
            if self.home_game && self.home_which == super::four::HomeGame::Mines {
                self.mines_page(ui, rect);
            } else if self.home_game {
                self.four_page(ui, rect);
            } else {
                self.home_dashboard(ui, rect);
            }
            return;
        }
        // Its round of the half-day of work played: none more (one under way or just over stays).
        let locked = self.game_locked(super::four::HomeGame::SpeedMetal);
        self.game.locked = locked.is_some();
        if let Some(p) = locked.filter(|_| self.game.idle()) {
            self.fold_for_game(false);
            self.locked_page(ui, rect, super::four::HomeGame::SpeedMetal, &p);
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
        // floor's board, fetched again every minute out of a round; the player's line from here.
        self.poll_floor();
        let settings = &self.config.settings;
        if !self.game.playing() {
            self.floor.sync(ui.ctx(), now, Some(60.0), settings.floor.as_ref());
        }
        let pseudo = crate::floor::pseudo(settings.floor.as_ref());
        let board = crate::floor::shown(self.floor.board(), &settings.typing_scores, pseudo);
        let best = settings.typing_scores.first().map_or(0, |s| s.letters);
        let out = self.game.ui(ui, game_rect, &self.theme, t, &board, best, french, settings.game_sound, pseudo.unwrap_or(&settings.player_name));
        // During a round the sidebar folds away, and unfolds after (if it was open).
        self.fold_for_game(self.game.playing());
        if self.game.playing() {
            self.count_round(super::four::HomeGame::SpeedMetal);
        }
        if let Some(on) = out.sound {
            self.config.settings.game_sound = Some(on);
            self.save_config();
        }
        // The name typed on a card: the next cards' (the pseudo only changes in its window).
        if let Some(name) = out.player {
            self.config.settings.player_name = name;
            self.save_config();
        }
        if let Some(score) = out.finished {
            // On the board, below the rounds it ties with (stable sort).
            let scores = &mut self.config.settings.typing_scores;
            let before = scores.first().map_or(0, |s| s.letters);
            let unlocked = before < crate::theme::METAL_UNLOCK && score.letters >= crate::theme::METAL_UNLOCK;
            // Online only with a pseudo.
            if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
                self.floor.finished(score.clone());
            }
            let scores = &mut self.config.settings.typing_scores;
            scores.push(score);
            scores.sort_by(|a, b| b.letters.cmp(&a.letters));
            scores.truncate(config::TYPING_SCORES);
            self.save_config();
            self.floor.sync(ui.ctx(), now, None, self.config.settings.floor.as_ref());
            if unlocked {
                self.toasts.push(super::Toast { ok: true, title: t.metal_unlocked.to_owned(), body: t.metal_unlocked_body.to_owned(), tab: self.active, at: std::time::Instant::now(), mail: None });
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

    /// The logo leads to the home page.
    fn logo_clicked(&mut self, logo: &egui::Response) {
        if logo.clicked() && (self.home.is_none() || self.home_game) {
            self.go_home(None);
        }
    }

    /// The typing game, on the home page.
    pub(super) fn open_game(&mut self) {
        if self.home.is_none() {
            self.go_home(None);
        }
        self.home_game = true;
        self.games_hub = false;
        self.dev_page = false;
        self.modules_page = false;
        self.procs_page = false;
        self.home_which = super::four::HomeGame::SpeedMetal;
        self.music_page = false;
        self.notes_page = false;
    }

    /// The categories in the sidebar's strip, in order: those not hidden in the settings, the music
    /// only with a server (always one, the local terminals at least).
    pub(super) fn categories(&self) -> Vec<Category> {
        let s = &self.config.settings;
        let music = !s.subsonic.url.is_empty() && s.subsonic.password_saved;
        let shown: Vec<Category> = Category::ordered(&s.category_order).into_iter().filter(|c| !s.hidden_categories.contains(c) && (*c != Category::Music || music)).collect();
        if shown.is_empty() { vec![Category::Local] } else { shown }
    }

    /// The category the panel lists: the one picked, or the first shown when it is hidden.
    pub(super) fn category(&self) -> Category {
        let shown = self.categories();
        let picked = self.config.settings.sidebar_category;
        if shown.contains(&picked) { picked } else { shown[0] }
    }

    /// What is shown belongs to another category than `category`: a tab of another kind, the notes,
    /// the music, the Dev page, a game (the home page without its game belongs to none).
    fn shows_other_category(&self, category: Category) -> bool {
        let shown = if self.notes_page {
            Category::Notes
        } else if self.music_page {
            Category::Music
        } else if self.dev_page {
            Category::Dev
        } else if self.modules_page {
            Category::Modules
        } else if self.home.is_some() {
            if !self.home_game {
                return self.category_screen(category);
            }
            Category::Games
        } else if let Some(i) = self.shown_tab() {
            self.tab_category(i)
        } else {
            return false;
        };
        shown != category
    }

    /// `category` has a screen of its own to show: a tab open, the notes, the music, the games.
    fn category_screen(&self, category: Category) -> bool {
        match category {
            Category::Local | Category::Ssh | Category::Db => self.last_tab_of(category).is_some(),
            Category::Notes | Category::Music | Category::Games | Category::Dev | Category::Modules => true,
            Category::Files => false,
        }
    }

    /// Shows `category`'s screen: its last tab (the home page when it has none open), the notes, the
    /// music, the games' page, the Dev page.
    fn show_category(&mut self, category: Category) {
        match category {
            Category::Local | Category::Ssh | Category::Db => match self.last_tab_of(category) {
                Some(index) => self.select(index),
                None => self.go_home(None),
            },
            Category::Notes => self.open_notes(),
            Category::Music => self.open_music_page(None),
            Category::Games => self.open_arcade(),
            Category::Dev => self.open_dev_page(),
            Category::Modules => self.open_modules_page(),
            Category::Files => {}
        }
    }

    /// The tab of `category` to show again: the active one if it is of it, else the one last shown
    /// of it, else the last one of it.
    fn last_tab_of(&self, category: Category) -> Option<usize> {
        if !matches!(category, Category::Local | Category::Ssh | Category::Db) {
            return None;
        }
        if self.active < self.tabs.len() && self.tab_category(self.active) == category {
            return Some(self.active);
        }
        let seen = self.category_panes.iter().find(|(c, _)| *c == category).map(|&(_, pane)| pane);
        seen.and_then(|pane| (0..self.tabs.len()).find(|&i| self.tab_category(i) == category && self.tabs[i].panes.contains_key(&pane)))
            .or_else(|| (0..self.tabs.len()).rev().find(|&i| self.tab_category(i) == category))
    }

    /// The category of a tab.
    fn tab_category(&self, i: usize) -> Category {
        match self.tabs.get(i) {
            Some(tab) if tab.ssh.is_some() => Category::Ssh,
            Some(tab) if tab.db.is_some() => Category::Db,
            _ => Category::Local,
        }
    }

    /// Notes the pane shown, for its category to show its tab again (see `last_tab_of`).
    fn note_shown_pane(&mut self) {
        if let Some(i) = self.shown_tab() {
            let (category, pane) = (self.tab_category(i), self.tabs[i].focused);
            self.category_panes.retain(|(c, _)| *c != category);
            self.category_panes.push((category, pane));
        }
    }

    /// When what is shown changes (another tab, the notes, the music, the game), the sidebar shows its
    /// category; the home page leaves it as it is.
    fn follow_shown(&mut self) {
        let game = self.home.is_some() && self.home_game;
        let seen = (self.shown_tab(), self.notes_page, self.music_page, game, self.dev_page, self.modules_page);
        if seen == self.sidebar_seen {
            return;
        }
        self.sidebar_seen = seen;
        let category = if self.notes_page {
            Category::Notes
        } else if self.music_page {
            Category::Music
        } else if self.dev_page {
            Category::Dev
        } else if self.modules_page {
            Category::Modules
        } else if game {
            Category::Games
        } else if let Some(i) = seen.0 {
            self.tab_category(i)
        } else {
            return;
        };
        if self.categories().contains(&category) {
            self.config.settings.sidebar_category = category;
        }
    }

    /// The strip of category icons, on the left of the sidebar: a click shows a category in the
    /// panel; on the one shown, it folds the sidebar.
    /// Folded (`rail`): a click pins the flyout of a category instead; the one under the pointer is
    /// returned, with its slot.
    fn category_strip(&mut self, ui: &Ui, strip: Rect, top: f32, rail: bool) -> Option<(Category, Rect)> {
        let t = self.t();
        let flyout = self.rail_flyout.as_ref().map(|f| f.category);
        let shown = if rail { flyout.unwrap_or_else(|| self.category()) } else { self.category() };
        let mut hovered = None;
        let mut y = top;
        let shown_list = self.categories();
        let mut moved = None;
        for &category in &shown_list {
            let slot = Rect::from_center_size(Pos2::new(strip.center().x, y + 20.0), Vec2::splat(40.0));
            y += 46.0;
            // Dragged up or down: the categories make way for it.
            let resp = ui.interact(slot, ui.id().with(("category", category as u8)), Sense::click_and_drag());
            if resp.hovered() {
                hovered = Some((category, slot));
            }
            if resp.dragged()
                && let Some(py) = ui.input(|i| i.pointer.interact_pos()).map(|p| p.y)
            {
                let to = (((py - top) / 46.0).floor().max(0.0) as usize).min(shown_list.len() - 1);
                if shown_list[to] != category {
                    moved = Some((category, shown_list[to]));
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            let active = category == shown;
            if active {
                ui.painter().rect_filled(slot, 9.0, self.theme.tab_active);
                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(strip.min.x + 2.0, slot.min.y + 10.0), Vec2::new(3.0, slot.height() - 20.0)), 1.5, self.theme.accent);
            } else if resp.hovered() {
                ui.painter().rect_filled(slot, 9.0, self.theme.tab_hover.gamma_multiply(0.75));
            }
            let color = if active { self.theme.accent } else if resp.hovered() { self.theme.text } else { self.theme.text_muted };
            paint_category_icon(ui.painter(), slot.center(), category, color);
            // The tabs open in it: how many have a program running, and a long command ended in one (✓ or ✗).
            let open: Vec<usize> = (0..self.tabs.len()).filter(|&i| matches!(category, Category::Local | Category::Ssh | Category::Db) && self.tab_category(i) == category).collect();
            let running = open.iter().filter(|&&i| matches!(self.live.get(i), Some(Some(_)))).count();
            if running > 0 {
                let galley = ui.painter().layout_no_wrap(running.to_string(), FontId::proportional(9.5), self.theme.text);
                let pill = Rect::from_center_size(slot.right_bottom() + Vec2::new(-8.0, -8.0), Vec2::new((galley.size().x + 7.0).max(14.0), 14.0));
                ui.painter().rect_filled(pill, 7.0, self.theme.chrome_bg);
                ui.painter().rect_filled(pill.shrink(1.0), 6.0, if active { self.theme.accent.gamma_multiply(0.3) } else { self.theme.tab_hover });
                ui.painter().galley(pill.center() - galley.size() / 2.0, galley, self.theme.text);
            }
            if let Some(ok) = open.iter().filter_map(|&i| self.tabs[i].done.as_ref()).map(|d| d.ok).reduce(|a, b| a && b) {
                let dot = slot.right_top() + Vec2::new(-8.0, 8.0);
                ui.painter().circle_filled(dot, 4.0, self.theme.chrome_bg);
                ui.painter().circle_filled(dot, 3.0, if ok { self.theme.ansi[2] } else { self.theme.ansi[1] });
            }
            if category == Category::Music && self.music.playing() {
                let dot = slot.right_top() + Vec2::new(-8.0, 8.0);
                ui.painter().circle_filled(dot, 4.0, self.theme.chrome_bg);
                ui.painter().circle_filled(dot, 3.0, self.theme.accent);
            }
            // A module has an update waiting.
            if category == Category::Modules && self.modules.any_update() {
                let dot = slot.right_top() + Vec2::new(-8.0, 8.0);
                ui.painter().circle_filled(dot, 4.0, self.theme.chrome_bg);
                ui.painter().circle_filled(dot, 3.0, self.theme.accent);
            }
            if rail {
                // The flyout shows its name.
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    match &mut self.rail_flyout {
                        Some(f) if f.category == category => f.pinned = !f.pinned,
                        _ => self.rail_flyout = Some(Flyout { category, away: None, pinned: true }),
                    }
                }
                continue;
            }
            let hint = if active { format!("{}  ·  {}", category_name(t, category), t.fold_sidebar) } else { category_name(t, category).to_owned() };
            if resp.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                // What is shown follows: the category's own screen, never one of another's left
                // behind (on the one already shown, the click folds the sidebar).
                // In a game, the Games icon leads back to the games' page.
                let in_game = category == Category::Games && self.home.is_some() && self.home_game && !self.games_hub;
                let elsewhere = in_game || self.shows_other_category(category);
                if active && !elsewhere {
                    self.config.settings.sidebar_folded = true;
                } else {
                    self.config.settings.sidebar_category = category;
                    if elsewhere {
                        self.show_category(category);
                        // Seen: the sidebar stays on the category clicked (the home page belongs to none).
                        let game = self.home.is_some() && self.home_game;
                        self.sidebar_seen = (self.shown_tab(), self.notes_page, self.music_page, game, self.dev_page, self.modules_page);
                    }
                }
            }
        }
        // Takes the place of the one under the pointer (hidden ones keep theirs among the others).
        if let Some((category, onto)) = moved {
            let mut order = Category::ordered(&self.config.settings.category_order);
            let from = order.iter().position(|c| *c == category);
            let to = order.iter().position(|c| *c == onto);
            if let (Some(from), Some(to)) = (from, to) {
                order.remove(from);
                order.insert(to, category);
                self.config.settings.category_order = order;
            }
        }
        hovered
    }

    /// The "Games" category: Speed Metal (with the best score), Puissance 4 (with the players online),
    /// Ronnie.io (with the best mass), the minesweeper (with the best time), Ronnie Run (with the
    /// farthest run), Riff Hero and Backstage (with the best score).
    fn games_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        use super::four::HomeGame;
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        paint_panel_title(&painter, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H)), t.cat_games, &theme);
        *y += SECTION_HEADER_H;
        let (mut open, mut new_window) = (None, None);
        for (game, name, note) in self.games_list() {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 8.0));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("game", game as u8)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let active = self.home.is_some() && self.home_game && !self.games_hub && self.home_which == game;
            paint_row_bg(&painter, slot, active, resp.hovered(), &theme);
            let icon = Pos2::new(slot.min.x + 17.0, slot.center().y);
            match game {
                HomeGame::SpeedMetal => super::paint_metal(&painter, icon, Align2::CENTER_CENTER, "R", 20.0, theme.accent, 1.0),
                HomeGame::Four => super::four::paint_four_icon(&painter, icon, &theme, 1.0),
                HomeGame::Blob => super::blob::paint_blob_icon(&painter, icon, &theme, 1.0),
                HomeGame::Mines => super::mines::paint_mines_icon(&painter, icon, &theme, 1.0),
                HomeGame::Runner => super::runner::paint_runner_icon(&painter, icon, &theme, 1.0),
                HomeGame::Riff => super::riff::paint_riff_icon(&painter, icon, &theme, 1.0),
                HomeGame::Backstage => super::backstage::paint_backstage_icon(&painter, icon, &theme, 1.0),
            }
            painter.text(Pos2::new(slot.min.x + 36.0, slot.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), if active || resp.hovered() { theme.text } else { theme.text_muted });
            if let Some(note) = note {
                painter.text(Pos2::new(slot.max.x - 10.0, slot.center().y), Align2::RIGHT_CENTER, note, FontId::proportional(11.5), theme.text_muted.gamma_multiply(0.85));
            }
            if resp.clicked() {
                open = Some(game);
            }
            resp.context_menu(|ui| menu_item(ui, &format!("⧉  {}", t.new_window_open), game, &mut new_window));
        }
        if let Some(game) = new_window {
            self.game_to_new_window(game);
        } else if let Some(game) = open.filter(|&g| !self.show_game_elsewhere(g)) {
            self.open_home_game(game);
        }
    }

    /// The games, with their name and a note: the best score of Speed Metal, Ronnie.io, the
    /// minesweeper, Ronnie Run, Riff Hero and Backstage, the players online at Puissance 4; when played enough this half-day of work, when
    /// it's back.
    pub(super) fn games_list(&mut self) -> [(super::four::HomeGame, &'static str, Option<String>); 7] {
        use super::four::HomeGame;
        let t = self.t();
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let best = self.config.settings.typing_scores.first().map(|s| s.letters.to_string());
        let online = (pseudo.is_some() && self.versus.reachable == Some(true)).then(|| t.four_online.replace("{n}", &self.versus.online.len().to_string()));
        let blob_best = (self.config.settings.blob_best > 0).then(|| self.config.settings.blob_best.to_string());
        let runner_best = (self.config.settings.runner_best > 0).then(|| super::runner::meters_text(self.config.settings.runner_best));
        let mines_best = (self.config.settings.mines_best > 0).then(|| super::mines::time_text(self.config.settings.mines_best));
        let riff_best = self.config.settings.riff_bests.iter().max().filter(|&&b| b > 0).map(u32::to_string);
        let backstage_best = (self.config.settings.backstage_best > 0).then(|| self.config.settings.backstage_best.to_string());
        let mut games = [(HomeGame::SpeedMetal, "Speed Metal", best), (HomeGame::Four, t.four_name, online), (HomeGame::Blob, t.blob_name, blob_best), (HomeGame::Mines, t.mines_name, mines_best), (HomeGame::Runner, t.runner_name, runner_best), (HomeGame::Riff, t.riff_name, riff_best), (HomeGame::Backstage, t.backstage_name, backstage_best)];
        for (game, _, note) in &mut games {
            if let Some(p) = self.game_locked(*game) {
                *note = Some(t.playtime_after.replace("{h}", &self.hour_text(p.end)));
            }
        }
        games
    }

    /// At the end of each frame: the games whose page no window drew (left for another game, a tab, the
    /// games' page...) stop what was under way. Their state is the app's, shared by the windows: one
    /// window can't stop them while another shows them.
    pub(super) fn leave_games(&mut self) {
        use super::four::HomeGame;
        let drawn = std::mem::take(&mut self.games_drawn);
        if !drawn.contains(&HomeGame::Blob) {
            self.blob_leave();
        }
        if !drawn.contains(&HomeGame::Runner) {
            self.runner.leave();
        }
        if !drawn.contains(&HomeGame::Riff) {
            self.riff.leave();
        }
        if !drawn.contains(&HomeGame::Backstage) {
            self.backstage.leave();
        }
    }

    /// The games' page, on this window's home page.
    pub(super) fn open_arcade(&mut self) {
        self.open_game();
        self.games_hub = true;
    }

    /// Opens `game` on this window's home page.
    pub(super) fn open_home_game(&mut self, game: super::four::HomeGame) {
        use super::four::HomeGame;
        match game {
            HomeGame::SpeedMetal => self.open_game(),
            HomeGame::Four => self.open_four(),
            HomeGame::Blob => self.open_blob(),
            HomeGame::Mines => self.open_mines(),
            HomeGame::Runner => self.open_runner(),
            HomeGame::Riff => self.open_riff(),
            HomeGame::Backstage => self.open_backstage(),
        }
    }

    /// The player's pseudo, in a card at the bottom of the "Games" category, with a pencil to change
    /// it (or a button to pick one). Returns the card's top.
    fn pseudo_card(&mut self, ui: &mut Ui, left: f32, width: f32, bottom: f32) -> f32 {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let card = Rect::from_min_max(Pos2::new(left, bottom - 62.0 - 6.0), Pos2::new(left + width, bottom - 6.0));
        let resp = ui.interact(card, ui.id().with("game-pseudo"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let fill = if resp.hovered() { lerp_color(theme.tab_active, theme.accent, 0.08) } else { theme.tab_active };
        painter.rect_filled(card, 10.0, fill);
        painter.rect_stroke(card, 10.0, Stroke::new(1.0, theme.accent.gamma_multiply(if resp.hovered() { 0.6 } else { 0.3 })), egui::StrokeKind::Inside);
        // The avatar: the pseudo's first letter in a disc of the accent, a green dot when online.
        let avatar = Pos2::new(card.min.x + 30.0, card.center().y);
        painter.circle_filled(avatar, 18.0, theme.accent.gamma_multiply(if pseudo.is_some() { 1.0 } else { 0.25 }));
        let initial = pseudo.as_deref().and_then(|p| p.chars().next()).map_or("?".to_owned(), |c| c.to_uppercase().to_string());
        painter.text(avatar + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, initial, FontId::proportional(17.0), if pseudo.is_some() { theme.bg } else { theme.text_muted });
        if pseudo.is_some() && self.versus.reachable == Some(true) {
            let dot = avatar + Vec2::new(13.0, 13.0);
            painter.circle_filled(dot, 5.0, theme.tab_active);
            painter.circle_filled(dot, 3.5, theme.ansi[2]);
        }
        let x = avatar.x + 28.0;
        let mut job = egui::text::LayoutJob::default();
        job.append(&t.pseudo_row.to_uppercase(), 0.0, egui::TextFormat { font_id: FontId::proportional(10.0), color: theme.text_muted, extra_letter_spacing: 1.2, ..Default::default() });
        painter.galley(Pos2::new(x, card.center().y - 17.0), painter.layout_job(job), theme.text_muted);
        let (name, color) = match &pseudo {
            Some(name) => (name.clone(), theme.text),
            None => (t.pseudo_pick.to_owned(), theme.accent),
        };
        let mut job = egui::text::LayoutJob::simple_singleline(name, FontId::proportional(16.0), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(card.max.x - 40.0 - x);
        painter.galley(Pos2::new(x, card.center().y - 4.0), painter.layout_job(job), color);
        // The pencil (or a plus, without a pseudo yet).
        let edit = Rect::from_center_size(Pos2::new(card.max.x - 20.0, card.center().y), Vec2::splat(26.0));
        let pencil = icon_button(ui, &painter, edit, "game-pseudo-edit", &theme, if pseudo.is_some() { super::paint_pencil } else { paint_plus });
        let tip = if pseudo.is_some() { t.pseudo_edit } else { t.pseudo_pick };
        if pencil.on_hover_text(tip).clicked() || resp.on_hover_text(tip).clicked() {
            self.ask_pseudo();
        }
        card.min.y
    }

    /// Folds the sidebar for a round of the game (`playing`), unfolds it after if the game folded it;
    /// the music pauses the same way.
    pub(super) fn fold_for_game(&mut self, playing: bool) {
        // The music from the server pauses during the round.
        self.music.hold(playing);
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
        let kind = if local { config::GroupKind::Profiles } else { config::GroupKind::Ssh };

        // SSH header: title and a "+" menu.
        if !local {
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        paint_panel_title(&painter, header, t.profiles, &self.theme);
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        let plus = icon_button(ui, &painter, plus_rect, "profiles-plus", &self.theme, paint_plus);
        egui::Popup::menu(&plus).width(210.0).show(|ui| {
            menu_item(ui, t.new_host, TabAction::NewHost, action);
            menu_item(ui, t.new_group, TabAction::NewGroup(config::GroupKind::Ssh), action);
        });
        *y += SECTION_HEADER_H;
        }

        if !local && self.config.ssh.is_empty() && !self.config.groups.iter().any(|g| g.kind() == kind) {
            let hint = painter.layout(t.no_profiles.to_owned(), FontId::proportional(12.0), self.theme.text_muted.gamma_multiply(0.7), row_w - 12.0);
            let h = hint.size().y;
            painter.galley(Pos2::new(left + 6.0, *y + 4.0), hint, self.theme.text_muted);
            *y += h + 12.0;
            return;
        }
        self.grouped_rows(ui, left, row_w, y, action, kind);
    }

    /// The items of one kind (profiles, SSH hosts or databases) and their groups: rows dragged to
    /// reorder them or move them into a group.
    fn grouped_rows(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>, kind: config::GroupKind) {
        let painter = ui.painter().clone();
        // The dragged row is painted above the others.
        let drag_painter = painter.clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("item-drag")));
        let rows = section_rows(&self.config, kind, false);
        let (pointer, pointer_down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));
        let mut rects: Vec<(Row, Rect)> = Vec::with_capacity(rows.len());

        for row in rows {
            let h = if matches!(row, Row::Group(_)) { GROUP_H } else { ROW_H };
            // Sub-groups (and what they hold) shifted right, a step per level.
            let group = match row {
                Row::Group(gi) | Row::Item(Some(gi), _) => Some(self.config.groups[gi].id),
                Row::Item(None, _) => None,
            };
            let indent = group.map_or(0.0, |g| self.config.group_depth(g) as f32 * SUBGROUP_INDENT);
            let slot = Rect::from_min_size(Pos2::new(left + indent, *y), Vec2::new(row_w - indent, h));
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
                Row::Item(_, id) if kind == config::GroupKind::Databases => self.db_row(ui, painter, id, slot, rect, dragged_here, action),
                Row::Item(_, id) => self.item_row(ui, painter, id, slot, rect, dragged_here, action),
            }
        }

        // Several picked: how many, and a way to let them go.
        let picked = self.selected_items.iter().filter(|id| self.config.item_kind(**id) == Some(kind)).count();
        if picked > 0 {
            let t = self.t();
            let line = Rect::from_min_size(Pos2::new(left, *y + 2.0), Vec2::new(row_w, 20.0));
            let clear_rect = Rect::from_center_size(Pos2::new(line.max.x - 12.0, line.center().y), Vec2::splat(18.0));
            let mut job = egui::text::LayoutJob::simple_singleline(t.n_selected.replace("{n}", &picked.to_string()), FontId::proportional(11.5), self.theme.text_muted);
            job.wrap = egui::text::TextWrapping::truncate_at_width(clear_rect.min.x - line.min.x - 10.0);
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(line.min.x + 6.0, line.center().y - galley.size().y / 2.0), galley, self.theme.text_muted);
            let clear = ui.interact(clear_rect, ui.id().with(("clear-selection", kind as u8)), Sense::click());
            if clear.hovered() {
                painter.rect_filled(clear_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
            }
            paint_cross(&painter, clear_rect.center(), if clear.hovered() { self.theme.text } else { self.theme.text_muted });
            if clear.clicked() {
                self.selected_items.clear();
            }
            *y += 24.0;
            // A plain click anywhere but on a row lets them go too.
            let (clicked, mods) = ui.input(|i| (i.pointer.primary_clicked(), i.modifiers));
            let on_row = pointer.is_some_and(|p| rects.iter().any(|(row, r)| matches!(row, Row::Item(..)) && r.contains(p)));
            if clicked && !mods.command && !mods.shift && !on_row {
                self.selected_items.clear();
            }
        }

        // Drop target of a drag in progress, shown as a line (or a highlighted group); applied on release.
        if let (Some(drag), Some(p)) = (&self.item_drag, pointer) {
            let (target, mark) = drop_target(drag, p, &rects, &self.config);
            // Several rows moving: how many, beside the one under the pointer.
            if let (Dragged::Items(ids), true) = (&drag.what, pointer_down) {
                if ids.len() > 1 {
                    let galley = painter.layout_no_wrap(ids.len().to_string(), FontId::proportional(11.0), self.theme.bg);
                    let pill = Rect::from_min_size(Pos2::new(p.x + 10.0, p.y - 20.0), Vec2::new((galley.size().x + 10.0).max(18.0), 18.0));
                    drag_painter.rect_filled(pill, 9.0, self.theme.accent);
                    drag_painter.galley(pill.center() - galley.size() / 2.0, galley, self.theme.bg);
                }
            }
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
        let (gid, collapsed, kind) = (group.id, group.collapsed, group.kind());
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
        let nested = self.config.group_and_subgroups(gid);
        let count = self.config.groups.iter().filter(|g| nested.contains(&g.id)).flat_map(|g| &g.items).filter(|id| self.config.item_kind(**id) == Some(kind)).count();
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
            menu_item(ui, t.new_subgroup, TabAction::NewSubgroup(gid), action);
            menu_item(ui, t.new_group, TabAction::NewGroup(kind), action);
            ui.separator();
            menu_item(ui, t.delete_group, TabAction::DeleteGroup(gid), action);
        });
    }

    pub(super) fn item_row(&mut self, ui: &mut Ui, painter: &egui::Painter, id: Uuid, slot: Rect, rect: Rect, dragged: bool, action: &mut Option<TabAction>) {
        let t = self.t();
        let Some(item) = self.item(id) else { return };
        let resp = ui.interact(slot, ui.id().with(("item", id)), Sense::click_and_drag());
        self.start_item_drag(ui, &resp, id, slot);
        let hovered = resp.contains_pointer() && self.item_drag.is_none();
        let active = item.open.is_some() && item.open == self.shown_tab();
        paint_row_bg(painter, rect, active || dragged, hovered, &self.theme);
        if self.selected_items.contains(&id) {
            paint_selected(painter, rect, &self.theme);
        }
        let dot = Pos2::new(rect.min.x + 17.0, rect.center().y);
        paint_badge(painter, dot, &item.name, item.color, item.ssh, item.open.is_some(), active, &self.theme);
        let live = item.open.and_then(|i| self.live.get(i).cloned().flatten());
        if live.is_some() {
            paint_live(ui, painter, dot, item.ssh, &self.theme);
        }

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

        // Open: its name in full; closed: dimmed, so the open ones stand out even with nothing running.
        let color = if active || item.open.is_some() {
            self.theme.text
        } else if hovered {
            self.theme.text_muted
        } else {
            self.theme.text_muted.gamma_multiply(0.6)
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
        if let Some(range) = picked(ui, &resp) {
            *action = Some(TabAction::SelectItem(id, range));
        } else if resp.double_clicked() {
            *action = Some(if item.ssh { TabAction::EditHost(id) } else { TabAction::StartItemRename(id) });
        } else if resp.clicked() {
            self.selected_items.clear();
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
        self.note_shown_pane();
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

        // The categories, as in the strip of the sidebar: the one under the pointer shows what it
        // lists beside, over what is there.
        let footer = bar.max.y - 54.0;
        let hovered = self.category_strip(ui, bar, unfold.max.y + 14.0, true);
        self.rail_flyout_ui(ui, bar, hovered);
        // The machine's resources at its bottom, above the settings.
        let below_categories = unfold.max.y + 14.0 + 46.0 * self.categories().len() as f32;
        self.resources_gauges(ui, egui::Rangef::new(bar.min.x + 12.0, bar.max.x - 12.0), below_categories, footer - 10.0);

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
    }

    /// The width of the panel listing `category`.
    pub(super) fn panel_width(&self, category: Category) -> f32 {
        match category {
            Category::Local => self.config.settings.files_width.clamp(PANEL_W, 900.0),
            _ => PANEL_W,
        }
    }

    /// The sidebar's width unfolded: its strip and the panel of the category shown.
    pub(super) fn sidebar_width(&self) -> f32 {
        STRIP_W + self.panel_width(self.category())
    }

    /// A handle on the edge at `x` of the file tree's panel: dragged, it widens or narrows it.
    fn resize_handle(&mut self, ctx: &egui::Context, x: f32, y: egui::Rangef) {
        let rect = Rect::from_x_y_ranges(x - 3.0..=x + 3.0, y);
        egui::Area::new(egui::Id::new("files-resize")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            let resp = ui.allocate_rect(Rect::from_min_size(rect.min, rect.size()), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if resp.hovered() || resp.dragged() {
                ui.painter().vline(x, y, Stroke::new(2.0, self.theme.accent));
            }
            if resp.dragged() {
                // Not wider than most of the window.
                let most = (ctx.content_rect().width() * 0.7).max(PANEL_W);
                let width = self.config.settings.files_width.clamp(PANEL_W, 900.0) + resp.drag_delta().x;
                self.config.settings.files_width = width.clamp(PANEL_W, most.min(900.0));
            }
            if resp.double_clicked() {
                self.config.settings.files_width = PANEL_W;
            }
        });
    }

    /// The panel of the folded sidebar's category under the pointer (or pinned by a click): what the
    /// sidebar would list, floating beside the rail. It goes once the pointer has left it a moment.
    fn rail_flyout_ui(&mut self, ui: &Ui, rail: Rect, hovered: Option<(Category, Rect)>) {
        let ctx = ui.ctx().clone();
        let now = ui.input(|i| i.time);
        if let Some((category, _)) = hovered {
            match &mut self.rail_flyout {
                Some(f) if f.category == category => f.away = None,
                // Another one pinned stays until a click elsewhere.
                Some(f) if f.pinned => {}
                _ => self.rail_flyout = Some(Flyout { category, away: None, pinned: false }),
            }
        }
        let Some(category) = self.rail_flyout.as_ref().map(|f| f.category) else { return };
        let width = self.panel_width(category);
        let panel = Rect::from_min_max(Pos2::new(rail.max.x + 6.0, rail.min.y + 8.0), Pos2::new(rail.max.x + 6.0 + width, rail.max.y - 8.0));
        let mut action: Option<TabAction> = None;
        let theme = self.theme.clone();
        egui::Area::new(egui::Id::new("rail-flyout")).order(egui::Order::Foreground).fixed_pos(panel.min).show(&ctx, |ui| {
            ui.painter().add(egui::Shadow { offset: [4, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(110) }.as_shape(panel, 10.0));
            ui.painter().rect_filled(panel, 10.0, theme.chrome_bg);
            ui.painter().rect_stroke(panel, 10.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
            ui.set_min_size(panel.size());
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(panel.shrink2(Vec2::new(0.0, 8.0))));
            inner.spacing_mut().scroll = egui::style::ScrollStyle { bar_width: 4.0, floating_allocated_width: 2.0, dormant_handle_opacity: 0.5, ..egui::style::ScrollStyle::thin() };
            egui::ScrollArea::vertical().id_salt(("rail-flyout-scroll", category as u8)).auto_shrink(false).show(&mut inner, |ui| {
                let origin = ui.max_rect().min;
                let mut y = origin.y;
                self.panel_content(ui, category, origin.x + SIDEBAR_PAD, width - 2.0 * SIDEBAR_PAD, &mut y, &mut action);
                ui.allocate_rect(Rect::from_min_max(origin, Pos2::new(origin.x + 1.0, y + SIDEBAR_PAD)), Sense::hover());
                self.apply_tab_action(ui, action.take());
            });
        });

        if category == Category::Local {
            self.resize_handle(&ctx, panel.max.x, panel.y_range());
        }
        // Stays while used: under the pointer, a menu or a dialog of it open, something dragged, a
        // field of it typed in.
        let pointer = ctx.pointer_hover_pos();
        let over = pointer.is_some_and(|p| panel.contains(p) || (p.x <= rail.max.x + 6.0 && hovered.is_some()));
        let typing = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| panel.contains_rect(r.rect));
        let busy = egui::Popup::is_any_open(&ctx) || ctx.memory(|m| m.top_modal_layer().is_some()) || ctx.dragged_id().is_some() || ui.input(|i| !i.raw.hovered_files.is_empty());
        let Some(f) = self.rail_flyout.as_mut() else { return };
        let clicked_out = ui.input(|i| i.pointer.any_pressed()) && pointer.is_some_and(|p| !panel.contains(p) && !rail.contains(p));
        if ui.input(|i| i.key_pressed(Key::Escape)) && !typing {
            self.rail_flyout = None;
        } else if f.pinned {
            if clicked_out && !busy {
                self.rail_flyout = None;
            }
        } else if over || typing || busy {
            f.away = None;
        } else {
            let since = *f.away.get_or_insert(now);
            if now - since > 0.35 {
                self.rail_flyout = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
    }

    /// The "Databases" section: the saved connections (the local server offered while there is none).
    pub(super) fn db_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>) {
        let t = self.t();
        let painter = ui.painter().clone();
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        paint_panel_title(&painter, header, t.db_section, &self.theme);
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        let plus = icon_button(ui, &painter, plus_rect, "db-plus", &self.theme, paint_plus);
        egui::Popup::menu(&plus).width(210.0).show(|ui| {
            menu_item(ui, t.db_new_connection, TabAction::NewDb, action);
            menu_item(ui, t.new_group, TabAction::NewGroup(config::GroupKind::Databases), action);
        });
        *y += SECTION_HEADER_H;
        if self.config.databases.is_empty() && !self.config.groups.iter().any(|g| g.kind() == config::GroupKind::Databases) {
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
        self.grouped_rows(ui, left, row_w, y, action, config::GroupKind::Databases);
    }

    /// A database connection in the sidebar.
    fn db_row(&mut self, ui: &mut Ui, painter: &egui::Painter, id: Uuid, slot: Rect, rect: Rect, dragged: bool, action: &mut Option<TabAction>) {
        let t = self.t();
        let Some(c) = self.config.databases.iter().find(|c| c.id == id).cloned() else { return };
        let resp = ui.interact(slot, ui.id().with(("db-item", c.id)), Sense::click_and_drag());
        self.start_item_drag(ui, &resp, id, slot);
        let open = self.tabs.iter().position(|tab| tab.db == Some(c.id));
        let active = open.is_some() && open == self.shown_tab();
        let hovered = resp.contains_pointer() && self.item_drag.is_none();
        paint_row_bg(painter, rect, active || dragged, hovered, &self.theme);
        if self.selected_items.contains(&id) {
            paint_selected(painter, rect, &self.theme);
        }
        let dot = Pos2::new(rect.min.x + 17.0, rect.center().y);
        paint_badge(painter, dot, &c.name, c.color, false, open.is_some(), active, &self.theme);
        let close_rect = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
        let show_close = open.is_some() && (active || hovered);
        let color = if active { self.theme.text } else if open.is_some() || hovered { self.theme.text_muted } else { self.theme.text_muted.gamma_multiply(0.75) };
        let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + 36.0, rect.min.y), Pos2::new(if show_close { close_rect.min.x - 4.0 } else { rect.max.x - 10.0 }, rect.max.y));
        let mut job = egui::text::LayoutJob::simple_singleline(c.name.clone(), FontId::proportional(13.0), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
        if show_close {
            let close = ui.interact(close_rect, ui.id().with(("db-close", c.id)), Sense::click());
            if close.hovered() {
                painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
            }
            paint_cross(painter, close_rect.center(), if close.hovered() { self.theme.text } else { self.theme.text_muted });
            if close.clicked() {
                *action = open.map(TabAction::Close);
            }
        }
        let resp = resp.on_hover_text(c.address());
        if let Some(range) = picked(ui, &resp) {
            *action = Some(TabAction::SelectItem(id, range));
        } else if resp.double_clicked() {
            *action = Some(TabAction::EditDb(c.id));
        } else if resp.clicked() {
            self.selected_items.clear();
            action.get_or_insert(TabAction::OpenDb(c.id));
        }
        resp.context_menu(|ui| {
            ui.set_min_width(170.0);
            menu_item(ui, t.connect, TabAction::OpenDb(c.id), action);
            menu_item(ui, t.edit, TabAction::EditDb(c.id), action);
            menu_item(ui, t.duplicate, TabAction::Duplicate(c.id), action);
            self.move_to_menu(ui, c.id, config::GroupKind::Databases, action);
            ui.separator();
            if ui.button(egui::RichText::new(t.delete).color(self.theme.ansi[1])).clicked() {
                *action = Some(TabAction::DeleteDb(c.id));
                ui.close();
            }
        });
    }

    /// The "Notes" section: the folders of notes, and their bin when it holds something. A click
    /// shows the folder in the notes panel; a note dragged from the panel onto one moves there.
    pub(super) fn notes_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>) {
        use super::notes::{Action, DraggedNote, Entry, Place};
        let t = self.t();
        let painter = ui.painter().clone();
        let header = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        paint_panel_title(&painter, header, t.notes, &self.theme);
        let plus_rect = Rect::from_center_size(Pos2::new(header.max.x - 12.0, header.center().y), Vec2::splat(20.0));
        let plus = icon_button(ui, &painter, plus_rect, "notes-plus", &self.theme, paint_plus).on_hover_text(format!("{}  ({})", t.notes_toggle, self.config.settings.shortcuts.toggle_notes.label()));
        egui::Popup::menu(&plus).width(200.0).show(|ui| {
            menu_item(ui, t.notes_new, TabAction::Notes(Action::NewNote), action);
            menu_item(ui, t.notes_new_folder, TabAction::Notes(Action::NewFolder), action);
        });
        *y += SECTION_HEADER_H;
        let open = self.notes_page;
        for (entry, name, count) in self.notes.entries(t) {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H));
            *y += ROW_H + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("notes-row", entry)), Sense::click());
            let active = open && self.notes.entry_shown(entry);
            // A note in no folder: opened by a click.
            let Entry::Place(place) = entry else {
                let Entry::Note(id) = entry else { continue };
                paint_row_bg(&painter, slot, active, resp.contains_pointer(), &self.theme);
                super::notes::paint_notes_icon(&painter, Pos2::new(slot.min.x + 17.0, slot.center().y), if active { self.theme.accent } else { self.theme.text_muted });
                let color = if active { self.theme.text } else { self.theme.text_muted.gamma_multiply(0.85) };
                let mut job = egui::text::LayoutJob::simple_singleline(name, FontId::proportional(13.0), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(row_w - 50.0);
                let galley = painter.layout_job(job);
                painter.galley(Pos2::new(slot.min.x + 36.0, slot.center().y - galley.size().y / 2.0), galley, color);
                if resp.clicked() {
                    action.get_or_insert(TabAction::Notes(Action::Open(id)));
                }
                resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| self.loose_note_menu(ui, id, action));
                continue;
            };
            let hovered = resp.contains_pointer();
            // A note dragged from the panel: dropped here, it moves into this folder (or the bin).
            let dropping = resp.dnd_hover_payload::<DraggedNote>().is_some();
            if let Some(note) = resp.dnd_release_payload::<DraggedNote>() {
                *action = Some(TabAction::Notes(match place {
                    Place::Folder(f) => Action::Move(note.0, f),
                    Place::Trash => Action::Trash(note.0),
                }));
            }
            paint_row_bg(&painter, slot, active, hovered || dropping, &self.theme);
            if dropping {
                painter.rect_stroke(slot, 8.0, Stroke::new(1.0, self.theme.accent), egui::StrokeKind::Inside);
            }
            paint_place_icon(&painter, Pos2::new(slot.min.x + 17.0, slot.center().y), place, if active { self.theme.accent } else { self.theme.text_muted });
            let text_rect = Rect::from_min_max(Pos2::new(slot.min.x + 36.0, slot.min.y), Pos2::new(slot.max.x - 34.0, slot.max.y));
            if let Place::Folder(Some(id)) = place
                && self.notes.renaming() == Some(id)
            {
                let theme = self.theme.clone();
                self.notes.rename_field(ui, Rect::from_min_max(Pos2::new(text_rect.min.x - 6.0, slot.min.y + 4.0), Pos2::new(slot.max.x - 6.0, slot.max.y - 4.0)), &theme);
                continue;
            }
            let color = if active { self.theme.text } else if hovered { self.theme.text_muted } else { self.theme.text_muted.gamma_multiply(0.85) };
            let mut job = egui::text::LayoutJob::simple_singleline(name, FontId::proportional(13.0), color);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0), galley, color);
            // Its count, or on the shown one under the pointer a cross that closes the panel.
            let close_rect = Rect::from_center_size(Pos2::new(slot.max.x - 14.0, slot.center().y), Vec2::splat(18.0));
            if active && hovered {
                let close = ui.interact(close_rect, ui.id().with(("notes-close", place)), Sense::click());
                if close.hovered() {
                    painter.rect_filled(close_rect, 4.0, self.theme.tab_hover.gamma_multiply(1.8));
                }
                paint_cross(&painter, close_rect.center(), if close.hovered() { self.theme.text } else { self.theme.text_muted });
                if close.on_hover_text(t.notes_close).clicked() {
                    *action = Some(TabAction::Notes(Action::Close));
                }
            } else if count > 0 {
                painter.text(Pos2::new(slot.max.x - 12.0, slot.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(11.5), self.theme.text_muted.gamma_multiply(0.8));
            }
            if resp.double_clicked()
                && let Place::Folder(Some(id)) = place
            {
                *action = Some(TabAction::Notes(Action::RenameFolder(id)));
            } else if resp.clicked() {
                action.get_or_insert(TabAction::Notes(Action::Place(place)));
            }
            resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| self.place_menu(ui, place, action));
        }
    }

    /// Right-click menu of a note in no folder, in the sidebar.
    fn loose_note_menu(&self, ui: &mut Ui, id: Uuid, action: &mut Option<TabAction>) {
        use super::notes::Action;
        let t = self.t();
        ui.set_min_width(170.0);
        menu_item(ui, t.notes_open, TabAction::Notes(Action::Open(id)), action);
        menu_item(ui, t.duplicate, TabAction::Notes(Action::Duplicate(id)), action);
        ui.separator();
        if ui.button(egui::RichText::new(t.notes_delete).color(self.theme.ansi[1])).clicked() {
            *action = Some(TabAction::Notes(Action::Trash(id)));
            ui.close();
        }
    }

    /// Right-click menu of a folder of notes (or of their bin).
    fn place_menu(&self, ui: &mut Ui, place: super::notes::Place, action: &mut Option<TabAction>) {
        use super::notes::{Action, Place};
        let t = self.t();
        ui.set_min_width(190.0);
        match place {
            Place::Trash => {
                if ui.button(egui::RichText::new(t.notes_empty_trash).color(self.theme.ansi[1])).clicked() {
                    *action = Some(TabAction::Notes(Action::EmptyTrash));
                    ui.close();
                }
            }
            Place::Folder(folder) => {
                menu_item(ui, t.notes_new, TabAction::Notes(Action::NewNoteIn(folder)), action);
                if let Some(id) = folder {
                    menu_item(ui, t.notes_new_subfolder, TabAction::Notes(Action::NewSubfolder(Some(id))), action);
                    menu_item(ui, t.notes_rename, TabAction::Notes(Action::RenameFolder(id)), action);
                    ui.separator();
                    if ui.button(egui::RichText::new(t.notes_delete_folder).color(self.theme.ansi[1])).clicked() {
                        *action = Some(TabAction::Notes(Action::DeleteFolder(id)));
                        ui.close();
                    }
                }
            }
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
        self.move_to_menu(ui, id, if item.ssh { config::GroupKind::Ssh } else { config::GroupKind::Profiles }, action);
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

    /// `items` in the order the sidebar lists them (those it doesn't list last).
    fn in_sidebar_order(&self, mut items: Vec<Uuid>) -> Vec<Uuid> {
        let Some(kind) = items.first().and_then(|i| self.config.item_kind(*i)) else { return items };
        let order: Vec<Uuid> = section_rows(&self.config, kind, true).into_iter().filter_map(|r| if let Row::Item(_, i) = r { Some(i) } else { None }).collect();
        items.sort_by_key(|i| order.iter().position(|o| o == i).unwrap_or(usize::MAX));
        items.dedup();
        items
    }

    /// "Move to" submenu of an item (of all those picked, when it is one of them): out of groups, into
    /// another group of its kind (sub-groups under theirs), or a new one.
    fn move_to_menu(&self, ui: &mut Ui, id: Uuid, kind: config::GroupKind, action: &mut Option<TabAction>) {
        let t = self.t();
        let ids = if self.selected_items.contains(&id) { self.selected_items.clone() } else { vec![id] };
        let label = if ids.len() > 1 { t.move_n_to.replace("{n}", &ids.len().to_string()) } else { t.move_to.to_owned() };
        ui.menu_button(label, |ui| {
            let group_of = |item: &Uuid| self.config.groups.iter().find(|g| g.items.contains(item)).map(|g| g.id);
            let all_in = |group: Option<Uuid>| ids.iter().all(|i| group_of(i) == group);
            if ui.add_enabled(!all_in(None), egui::Button::new(t.no_group)).clicked() {
                *action = Some(TabAction::DropItem(ids.clone(), None, None));
                ui.close();
            }
            for row in section_rows(&self.config, kind, true) {
                let Row::Group(gi) = row else { continue };
                let g = &self.config.groups[gi];
                let name = format!("{}{}", "    ".repeat(self.config.group_depth(g.id)), g.name);
                if ui.add_enabled(!all_in(Some(g.id)), egui::Button::new(name)).clicked() {
                    *action = Some(TabAction::DropItem(ids.clone(), Some(g.id), None));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(t.new_group).clicked() {
                *action = Some(TabAction::NewGroupWith(ids.clone()));
                ui.close();
            }
        });
    }

    /// A row of the sidebar starts being dragged: it takes along the others picked with it.
    fn start_item_drag(&mut self, ui: &Ui, resp: &egui::Response, id: Uuid, slot: Rect) {
        if !resp.drag_started() {
            return;
        }
        let Some(p) = ui.input(|i| i.pointer.interact_pos()) else { return };
        let ids = if self.selected_items.contains(&id) {
            std::iter::once(id).chain(self.selected_items.iter().copied().filter(|i| *i != id)).collect()
        } else {
            self.selected_items.clear();
            vec![id]
        };
        self.item_drag = Some(ItemDrag { what: Dragged::Items(ids), grab: p.y - slot.min.y });
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
        self.note_shown_pane();
        self.follow_shown();
        let category = self.category();
        // On the way into the games, the pseudo, if there is none yet.
        let games = category == Category::Games;
        if games && !self.games_seen && crate::floor::pseudo(self.config.settings.floor.as_ref()).is_none() {
            self.ask_pseudo();
        }
        self.games_seen = games;
        let whole = ui.max_rect();
        // A slight gradient, darker at the bottom.
        {
            let (top, bottom) = (self.theme.chrome_bg, lerp_color(self.theme.chrome_bg, self.theme.bg, 0.55));
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(whole.left_top(), top);
            mesh.colored_vertex(whole.right_top(), top);
            mesh.colored_vertex(whole.right_bottom(), bottom);
            mesh.colored_vertex(whole.left_bottom(), bottom);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(egui::Shape::mesh(mesh));
        }
        ui.painter().vline(whole.max.x - 0.5, whole.y_range(), Stroke::new(1.0, self.theme.tab_hover));
        // On the left the strip of categories, a shade darker; the panel of the one picked beside it.
        let strip = Rect::from_min_max(whole.min, Pos2::new(whole.min.x + STRIP_W, whole.max.y));
        let bar = Rect::from_min_max(Pos2::new(strip.max.x, whole.min.y), whole.max);
        ui.painter().rect_filled(strip, 0.0, self.theme.bg.gamma_multiply(0.3));
        ui.painter().vline(strip.max.x - 0.5, strip.y_range(), Stroke::new(1.0, self.theme.tab_hover.gamma_multiply(0.7)));

        // Empty space in the sidebar drags the window (the native title bar is hidden).
        let bg = ui.interact(whole, ui.id().with("sidebar-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }

        let mut action: Option<TabAction> = None;
        let left = bar.min.x + SIDEBAR_PAD;
        let row_w = bar.width() - 2.0 * SIDEBAR_PAD;

        // Everything below the traffic lights scrolls when there are many tabs.
        let footer_top = bar.max.y - FOOTER_H;
        // macOS: the traffic lights don't scale with the interface zoom, so the space kept for them doesn't either.
        let top = if cfg!(target_os = "macos") { SIDEBAR_TOP / ui.ctx().zoom_factor() } else { SIDEBAR_TOP };
        // Its first icon level with the panel's title, under the logo (the slots are 40 high).
        let strip_top = strip.min.y + top + LOGO_H + SECTION_HEADER_H / 2.0 - 20.0;
        self.category_strip(ui, strip, strip_top, false);
        // The machine's resources at its bottom, above the version.
        let below_categories = strip_top + 46.0 * self.categories().len() as f32;
        self.resources_gauges(ui, egui::Rangef::new(strip.min.x + 7.0, strip.max.x - 7.0), below_categories, strip.max.y - 28.0);
        // The version, at the bottom of the strip: small, and smaller still if it doesn't fit.
        let version = format!("v{}", update::VERSION);
        let mut size = 9.5;
        let mut galley = ui.painter().layout_no_wrap(version.clone(), FontId::proportional(size), self.theme.text_muted);
        while galley.size().x > STRIP_W - 8.0 && size > 6.5 {
            size -= 0.5;
            galley = ui.painter().layout_no_wrap(version.clone(), FontId::proportional(size), self.theme.text_muted);
        }
        let at = Pos2::new(strip.center().x - galley.size().x / 2.0, strip.max.y - 12.0 - galley.size().y / 2.0);
        ui.painter().galley(at, galley, self.theme.text_muted.gamma_multiply(0.7));
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
        // Jurassic Park: ferns at the foot of the panel, drawn first: the update and music cards and
        // the list go over them.
        if self.theme.jurassic {
            super::jurassic::ferns(ui.painter(), Rect::from_min_max(Pos2::new(bar.min.x, footer_top - 110.0), Pos2::new(bar.max.x - 1.0, footer_top)), &self.theme);
        }
        let card_top = self.update_card(ui, Rect::from_min_max(Pos2::new(left, bar.min.y), Pos2::new(left + row_w, footer_top)));
        let card_top = self.music_card(ui, left, row_w, card_top);
        let card_top = if self.category() == Category::Games { self.pseudo_card(ui, left, row_w, card_top) } else { card_top };
        let scroll_rect = Rect::from_min_max(Pos2::new(bar.min.x, logo_rect.max.y), Pos2::new(bar.max.x - 1.0, card_top));
        // The file tree's panel widens from its edge.
        // Not while it folds or unfolds (cut by the window's edge then).
        let wide = category == Category::Local;
        if wide && ui.clip_rect().max.x >= whole.max.x - 0.5 {
            self.resize_handle(&ui.ctx().clone(), whole.max.x - 1.0, whole.y_range());
        }

        // Footer: settings button, always visible.
        let button = Rect::from_min_max(Pos2::new(left, footer_top + 5.0), Pos2::new(left + row_w, bar.max.y - 7.0));
        let settings = ui.interact(button, ui.id().with("settings-btn"), Sense::click());
        let hot = settings.hovered() || self.settings_dialog;
        if self.theme.jurassic {
            // Jurassic Park: safety stripes above the settings (the ferns are under the cards).
            super::jurassic::hazard(ui.painter(), Rect::from_min_max(Pos2::new(bar.min.x, footer_top - 2.0), Pos2::new(bar.max.x - 1.0, footer_top + 2.0)));
        } else {
            ui.painter().hline(bar.min.x + 12.0..=bar.max.x - 12.0, footer_top, Stroke::new(1.0, self.theme.tab_hover.gamma_multiply(0.8)));
        }
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
            let origin = ui.max_rect().min;
            let mut y = origin.y;
            self.panel_content(ui, category, left, row_w, &mut y, &mut action);
            // Content height, so the scroll area knows how far it can go.
            ui.allocate_rect(Rect::from_min_max(origin, Pos2::new(origin.x + 1.0, y + ROW_H + SIDEBAR_PAD)), Sense::hover());
            self.apply_tab_action(ui, action);
        });
    }

    /// What the sidebar's panel lists for `category`, from `y` down (also in the folded sidebar's
    /// flyout).
    pub(super) fn panel_content(&mut self, ui: &mut Ui, category: Category, left: f32, row_w: f32, y: &mut f32, action: &mut Option<TabAction>) {
        match category {
            // The local profiles are in a bar on top: the panel shows the files (or git) of the one shown.
            Category::Local => self.ws_panel(ui, left, row_w, y),
            Category::Ssh => self.profiles_section(ui, left, row_w, y, action, false),
            Category::Db => self.db_section(ui, left, row_w, y, action),
            Category::Music => self.music_section(ui, left, row_w, y),
            Category::Notes => self.notes_section(ui, left, row_w, y, action),
            // Retired: the file tree is in the terminals' panel.
            Category::Files => {}
            Category::Games => self.games_section(ui, left, row_w, y),
            Category::Dev => self.dev_section(ui, left, row_w, y),
            Category::Modules => self.modules_section(ui, left, row_w, y),
        }
    }

    /// Carries out what was clicked in the sidebar (folded or not), or in the profiles' bar.
    pub(super) fn apply_tab_action(&mut self, ui: &Ui, action: Option<TabAction>) {
        let t = self.t();
        match action {
            Some(TabAction::Notes(super::notes::Action::Close)) => self.close_notes(),
            Some(TabAction::Notes(a)) => {
                if a.shows() {
                    self.open_notes();
                }
                self.notes.apply(a, ui, t);
            }
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
            Some(TabAction::NewGroup(kind)) => {
                let group = config::Group::new(t.new_group_name, kind);
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::EditProfile(id)) => self.open_profile_editor(id),
            Some(TabAction::NewGroupWith(items)) => {
                let kind = items.first().and_then(|i| self.config.item_kind(*i)).unwrap_or(config::GroupKind::Ssh);
                let mut group = config::Group::new(t.new_group_name, kind);
                for item in self.in_sidebar_order(items) {
                    self.config.unplace(item);
                    group.items.push(item);
                }
                self.selected_items.clear();
                self.group_rename = Some((group.id, group.name.clone(), true));
                self.config.groups.push(group);
            }
            Some(TabAction::NewSubgroup(parent)) => {
                if let Some(p) = self.config.groups.iter_mut().find(|g| g.id == parent) {
                    p.collapsed = false;
                    let mut group = config::Group::new(t.new_group_name, p.kind());
                    group.parent = Some(parent);
                    self.group_rename = Some((group.id, group.name.clone(), true));
                    self.config.groups.push(group);
                }
            }
            Some(TabAction::SelectItem(id, range)) => {
                let kind = self.config.item_kind(id);
                // One section at a time.
                if self.selected_items.first().is_some_and(|s| self.config.item_kind(*s) != kind) {
                    self.selected_items.clear();
                }
                let order: Vec<Uuid> = kind.map_or_else(Vec::new, |k| section_rows(&self.config, k, false).into_iter().filter_map(|r| if let Row::Item(_, i) = r { Some(i) } else { None }).collect());
                let anchor = self.selected_items.last().copied();
                let span = anchor.and_then(|a| Some((order.iter().position(|i| *i == a)?, order.iter().position(|i| *i == id)?)));
                match span.filter(|_| range) {
                    // From the last one picked to this one; that one stays the start of the next range.
                    Some((a, b)) => {
                        for i in &order[a.min(b)..=a.max(b)] {
                            if !self.selected_items.contains(i) {
                                self.selected_items.push(*i);
                            }
                        }
                        let a = order[a];
                        self.selected_items.retain(|i| *i != a);
                        self.selected_items.push(a);
                    }
                    None if self.selected_items.contains(&id) => self.selected_items.retain(|i| *i != id),
                    None => self.selected_items.push(id),
                }
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
                // Its items and sub-groups stay, in the group it was in (or outside groups).
                if let Some(index) = self.config.groups.iter().position(|g| g.id == id) {
                    let group = self.config.groups.remove(index);
                    for g in self.config.groups.iter_mut().filter(|g| g.parent == Some(id)) {
                        g.parent = group.parent;
                    }
                    match group.parent.and_then(|p| self.config.groups.iter_mut().find(|g| g.id == p)) {
                        Some(parent) => parent.items.extend(group.items),
                        None => self.config.ungrouped.extend(group.items),
                    }
                }
            }
            Some(TabAction::DropItem(items, group, before)) => {
                let items = self.in_sidebar_order(items);
                for item in &items {
                    self.config.unplace(*item);
                }
                let list = match group.and_then(|g| self.config.groups.iter_mut().find(|x| x.id == g)) {
                    Some(g) => &mut g.items,
                    None => &mut self.config.ungrouped,
                };
                let at = before.and_then(|b| list.iter().position(|x| *x == b)).unwrap_or(list.len());
                list.splice(at..at, items);
                self.selected_items.clear();
            }
            Some(TabAction::Reconnect(i)) => {
                if let Some(tab) = self.tabs.get(i) {
                    let panes = tab.layout.leaves();
                    self.reconnect(ui.ctx(), i, &panes);
                    self.select(i);
                }
            }
            Some(TabAction::DropGroup(id, parent, before)) => {
                // Never into itself or one of its own sub-groups.
                let inside_itself = parent.is_some_and(|p| self.config.group_and_subgroups(id).contains(&p));
                if let Some(index) = self.config.groups.iter().position(|g| g.id == id).filter(|_| !inside_itself) {
                    let mut group = self.config.groups.remove(index);
                    group.parent = parent;
                    if let Some(p) = parent.and_then(|p| self.config.groups.iter_mut().find(|g| g.id == p)) {
                        p.collapsed = false;
                    }
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

/// The rows of the SSH or Databases section, top to bottom: the items outside groups, then each group
/// with its items and then its sub-groups (not what a folded group holds, unless `all`).
fn section_rows(config: &config::Config, kind: config::GroupKind, all: bool) -> Vec<Row> {
    fn push_groups(config: &config::Config, kind: config::GroupKind, all: bool, parent: Option<Uuid>, depth: usize, rows: &mut Vec<Row>) {
        if depth > config.groups.len() {
            return;
        }
        for (gi, g) in config.groups.iter().enumerate().filter(|(_, g)| g.kind() == kind && g.parent == parent) {
            rows.push(Row::Group(gi));
            if all || !g.collapsed {
                rows.extend(g.items.iter().filter(|id| config.item_kind(**id) == Some(kind)).map(|id| Row::Item(Some(gi), *id)));
                push_groups(config, kind, all, Some(g.id), depth + 1, rows);
            }
        }
    }
    let mut rows: Vec<Row> = config.ungrouped.iter().filter(|id| config.item_kind(**id) == Some(kind)).map(|id| Row::Item(None, *id)).collect();
    push_groups(config, kind, all, None, 0, &mut rows);
    rows
}

/// A click on a row with ⌘ / Ctrl (Some(false): picks it or lets it go) or ⇧ (Some(true): picks a range).
fn picked(ui: &Ui, resp: &egui::Response) -> Option<bool> {
    let mods = ui.input(|i| i.modifiers);
    (resp.clicked() && (mods.command || mods.shift)).then_some(mods.shift)
}

/// A row picked to move with others: a touch of the accent, outlined.
fn paint_selected(painter: &egui::Painter, rect: Rect, theme: &crate::theme::Theme) {
    painter.rect_filled(rect, 8.0, theme.accent.gamma_multiply(0.12));
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.55)), egui::StrokeKind::Inside);
}

/// Background of a sidebar row: nothing at rest, a light fill on hover, and for the open one a fill
/// with a touch of the accent and a bar on its left.
pub(super) fn paint_row_bg(painter: &egui::Painter, rect: Rect, active: bool, hovered: bool, theme: &crate::theme::Theme) {
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
    let fill = tint.gamma_multiply(if active { 0.36 } else if open { 0.28 } else { 0.08 });
    if ssh {
        painter.circle_filled(c, 10.0, fill);
    } else {
        painter.rect_filled(r, 6.0, fill);
    }
    let initial: String = name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_else(|| "·".into());
    let text = if open || active { tint } else { tint.gamma_multiply(0.5) };
    painter.text(c + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, initial, FontId::proportional(11.0), text);
    if open && ssh {
        let dot = c + Vec2::new(7.0, 7.0);
        painter.circle_filled(dot, 3.5, theme.chrome_bg);
        painter.circle_filled(dot, 2.5, theme.ansi[2]);
    }
}

pub(super) fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from(egui::lerp(egui::Rgba::from(a)..=egui::Rgba::from(b), t))
}

/// The title of the sidebar's panel: the category it lists, in small spaced capitals.
pub(super) fn paint_panel_title(painter: &egui::Painter, rect: Rect, title: &str, theme: &crate::theme::Theme) {
    let mut job = egui::text::LayoutJob::default();
    job.append(&title.to_uppercase(), 0.0, egui::TextFormat { font_id: FontId::proportional(11.0), color: theme.text_muted, extra_letter_spacing: 1.4, ..Default::default() });
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0), galley, theme.text_muted);
}

/// The name of a category of the sidebar.
pub(super) fn category_name(t: &Strings, category: Category) -> &'static str {
    match category {
        Category::Local => t.cat_local,
        Category::Files => t.cat_files,
        Category::Ssh => t.profiles,
        Category::Db => t.db_section,
        Category::Music => t.music_nav,
        Category::Notes => t.notes,
        Category::Games => t.cat_games,
        Category::Dev => t.cat_dev,
        Category::Modules => t.cat_modules,
    }
}

/// The icon of a category, in the sidebar's strip.
pub(super) fn paint_category_icon(painter: &egui::Painter, c: Pos2, category: Category, color: Color32) {
    match category {
        Category::Local => paint_prompt_icon(painter, c, color),
        Category::Files => super::notes::paint_folder_icon(painter, c, color),
        Category::Ssh => paint_server_icon(painter, c, color),
        Category::Db => super::dbview::paint_db_icon(painter, c, color),
        Category::Music => super::music::paint_note(painter, c, color),
        Category::Notes => super::notes::paint_notes_icon(painter, c, color),
        Category::Games => paint_gamepad_icon(painter, c, color),
        Category::Dev => super::dev::paint_dev_icon(painter, c, color),
        Category::Modules => super::modules::paint_modules_icon(painter, c, color),
    }
}

/// A game controller: the games category.
fn paint_gamepad_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.rect_stroke(Rect::from_center_size(c, Vec2::new(17.0, 11.0)), 5.0, stroke, egui::StrokeKind::Middle);
    let pad = c + Vec2::new(-4.0, 0.0);
    painter.line_segment([pad - Vec2::new(2.5, 0.0), pad + Vec2::new(2.5, 0.0)], stroke);
    painter.line_segment([pad - Vec2::new(0.0, 2.5), pad + Vec2::new(0.0, 2.5)], stroke);
    painter.circle_filled(c + Vec2::new(3.5, 1.2), 1.1, color);
    painter.circle_filled(c + Vec2::new(5.5, -1.2), 1.1, color);
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


/// The icon of a folder of notes: a page for "Notes", a folder for the others, a bin.
fn paint_place_icon(painter: &egui::Painter, c: Pos2, place: super::notes::Place, color: Color32) {
    use super::notes::{paint_folder_icon, paint_notes_icon, paint_trash_icon, Place};
    match place {
        Place::Folder(None) => paint_notes_icon(painter, c, color),
        Place::Folder(Some(_)) => paint_folder_icon(painter, c, color),
        Place::Trash => paint_trash_icon(painter, c, color),
    }
}

/// The panel of a category shown beside the folded sidebar.
pub(super) struct Flyout {
    pub category: Category,
    /// Since when the pointer is away from it.
    pub away: Option<f64>,
    /// Opened by a click: stays until a click elsewhere.
    pub pinned: bool,
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
