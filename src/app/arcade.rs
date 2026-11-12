//! The games' page: each game in a card (its icon, what it is, its best score or who is online),
//! reached from the sidebar's Games icon. A click opens it, a right click in a new window.

use super::four::HomeGame;
use super::*;

impl App {
    pub(super) fn arcade_page(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, theme.bg);

        let games = self.games_list();
        // What is still to unlock, game by game, and in all.
        let rewards: Vec<Option<(usize, usize)>> = games.iter().map(|(g, _, _)| self.unlocks_count(*g)).collect();
        let (left_all, total_all) = rewards.iter().flatten().fold((0, 0), |(l, t), (a, b)| (l + a, t + b));

        // The cards in rows of as many as fit (the last one centered): tall when their rows fit the page,
        // else flat ones, as few columns as fit the page's height, the page scrolling only when even
        // the narrowest don't.
        let n = games.len();
        let gap = 20.0;
        let title_h = 96.0;
        let columns = |min_w: f32| (((rect.width() - 96.0 + gap) / (min_w + gap)).floor() as usize).clamp(1, n);
        let rows = |cols: usize| n.div_ceil(cols) as f32;
        let tall_cols = columns(200.0);
        let wide = 56.0 + title_h + rows(tall_cols) * (290.0 + gap) <= rect.height();
        let (cols, card_size, gap, title_h) = if wide {
            (tall_cols, Vec2::new(((rect.width() - 96.0 - (tall_cols - 1) as f32 * gap) / tall_cols as f32).min(280.0), 290.0), gap, title_h)
        } else {
            let (gap, title_h) = (14.0, 78.0);
            let room_h = rect.height() - 40.0 - title_h - 16.0;
            let width = |cols: usize| ((rect.width() - 48.0 - (cols - 1) as f32 * gap) / cols as f32).min(520.0);
            let height = |cols: usize| ((room_h - (rows(cols) - 1.0) * gap) / rows(cols)).min(128.0);
            let fits = (1..=n).take_while(|&c| c == 1 || width(c) >= 270.0).find(|&c| height(c) >= 92.0);
            let most = (1..=n).take_while(|&c| c == 1 || width(c) >= 270.0).last().unwrap_or(1);
            let cols = fits.unwrap_or(most);
            (cols, Vec2::new(width(cols), height(cols).max(92.0)), gap, title_h)
        };
        let block = Vec2::new(cols as f32 * card_size.x + (cols - 1) as f32 * gap, rows(cols) * (card_size.y + gap) - gap);
        let content_h = (40.0 + title_h + block.y + 16.0).max(rect.height());

        let (mut open, mut new_window, mut back) = (None, None, false);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        egui::ScrollArea::vertical().id_salt("arcade").auto_shrink([false; 2]).show(&mut child, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(rect.width(), content_h), Sense::hover());
            let painter = ui.painter().clone();

            // Back to the home page.
            let back_rect = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            back = ui.put(back_rect, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked();

            let top = (rect.center().y - (block.y + title_h) / 2.0).max(rect.min.y + 40.0);
            let (title_size, sub_y) = if wide { (46.0, 78.0) } else { (36.0, 58.0) };
            paint_metal(&painter, Pos2::new(rect.center().x, top + title_size * 0.7), Align2::CENTER_CENTER, t.cat_games, title_size, theme.accent, 1.0);
            // The subtitle, and the rewards unlocked in all.
            let mut job = egui::text::LayoutJob::default();
            job.append(t.arcade_subtitle, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), theme.text_muted));
            if total_all > 0 {
                let color = if left_all == 0 { theme.ansi[2] } else { theme.accent };
                job.append(&t.arcade_rewards.replace("{n}", &(total_all - left_all).to_string()).replace("{max}", &total_all.to_string()), 14.0, egui::TextFormat::simple(FontId::proportional(13.0), color));
            }
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(rect.center().x - galley.size().x / 2.0, top + sub_y - galley.size().y / 2.0), galley, theme.text_muted);

            let origin = Pos2::new(rect.center().x - block.x / 2.0, top + title_h);
            for (k, ((game, name, note), reward)) in games.into_iter().zip(rewards.iter().copied()).enumerate() {
                let (row, col) = (k / cols, k % cols);
                // The last row, shorter, centered.
                let in_row = (n - row * cols).min(cols);
                let shift = (cols - in_row) as f32 * (card_size.x + gap) / 2.0;
                let at = origin + Vec2::new(shift + col as f32 * (card_size.x + gap), row as f32 * (card_size.y + gap));
                let card = Rect::from_min_size(at, card_size);
                let resp = ui.interact(card, ui.id().with(("arcade", game as u8)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                let hot = resp.hovered();
                painter.rect_filled(card, 14.0, if hot { theme.tab_active } else { theme.chrome_bg });
                painter.rect_stroke(card, 14.0, Stroke::new(if hot { 1.5 } else { 1.0 }, if hot { theme.accent.gamma_multiply(0.7) } else { theme.tab_hover }), egui::StrokeKind::Inside);
                let desc = match game {
                    HomeGame::SpeedMetal => t.arcade_metal,
                    HomeGame::Four => t.arcade_four,
                    HomeGame::Blob => t.blob_tagline,
                    HomeGame::Mines => t.arcade_mines,
                    HomeGame::Runner => t.arcade_runner,
                    HomeGame::Riff => t.arcade_riff,
                    HomeGame::Backstage => t.arcade_backstage,
                    HomeGame::Coffee => t.arcade_coffee,
                    HomeGame::Larsen => t.arcade_larsen,
                    HomeGame::President => t.arcade_president,
                    HomeGame::BugHunt => t.arcade_bughunt,
                    HomeGame::Fishing => t.fishing_arcade,
                    HomeGame::Monday => t.arcade_monday,
                };
                let (icon, text_left, text_w) = if wide { (Pos2::new(card.center().x, card.min.y + 74.0), None, card.width() - 36.0) } else { (Pos2::new(card.min.x + 44.0, card.center().y), Some(card.min.x + 84.0), card.width() - 100.0) };
                let scale = if wide { 2.8 } else { 1.8 };
                match game {
                    HomeGame::SpeedMetal => paint_metal(&painter, icon, Align2::CENTER_CENTER, "R", 20.0 * scale, theme.accent, 1.0),
                    HomeGame::Four => super::four::paint_four_icon(&painter, icon, &theme, scale),
                    HomeGame::Blob => super::blob::paint_blob_icon(&painter, icon, &theme, scale),
                    HomeGame::Mines => super::mines::paint_mines_icon(&painter, icon, &theme, scale),
                    HomeGame::Runner => super::runner::paint_runner_icon(&painter, icon, &theme, scale),
                    HomeGame::Riff => super::riff::paint_riff_icon(&painter, icon, &theme, scale),
                    HomeGame::Backstage => super::backstage::paint_backstage_icon(&painter, icon, &theme, scale),
                    HomeGame::Coffee => super::coffee::paint_coffee_icon(&painter, icon, &theme, scale),
                    HomeGame::Larsen => super::larsen::paint_larsen_icon(&painter, icon, &theme, scale),
                    HomeGame::President => super::president::paint_president_icon(&painter, icon, &theme, scale),
                    HomeGame::BugHunt => super::bughunt::paint_bughunt_icon(&painter, icon, &theme, scale),
                    HomeGame::Fishing => super::fishing::paint_fishing_icon(&painter, icon, &theme, scale),
                    HomeGame::Monday => super::zombie::paint_monday_icon(&painter, icon, &theme, scale),
                }
                let name_color = if hot { theme.text } else { theme.text.gamma_multiply(0.92) };
                // Its rewards: how many still to unlock (a padlock), or all of them.
                let reward_text = reward.map(|(left, _)| if left == 0 { (t.unlock_all_done.to_owned(), theme.ansi[2], true) } else { (t.unlock_left.replace("{n}", &left.to_string()), theme.accent, false) });
                let paint_reward = |p: Pos2, centered: bool| {
                    if let Some((text, color, done)) = &reward_text {
                        let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(12.0), *color);
                        let w = galley.size().x + 18.0;
                        let x = if centered { p.x - w / 2.0 } else { p.x };
                        super::unlocks::paint_padlock(&painter, Pos2::new(x + 5.0, p.y), *done, *color);
                        painter.galley(Pos2::new(x + 18.0, p.y - galley.size().y / 2.0), galley, *color);
                    }
                };
                let elided;
                match text_left {
                    None => {
                        painter.text(Pos2::new(card.center().x, card.min.y + 150.0), Align2::CENTER_CENTER, name, FontId::proportional(20.0), name_color);
                        let mut job = egui::text::LayoutJob::simple(desc.to_owned(), FontId::proportional(13.0), theme.text_muted, text_w);
                        job.halign = egui::Align::Center;
                        job.wrap.max_rows = 3;
                        let galley = painter.layout_job(job);
                        elided = galley.elided;
                        let desc_bottom = card.min.y + 174.0 + galley.size().y;
                        painter.galley(Pos2::new(card.center().x, card.min.y + 174.0), galley, theme.text_muted);
                        // Under the description, however many lines it takes; the rewards at the bottom.
                        if let Some(note) = &note {
                            painter.text(Pos2::new(card.center().x, desc_bottom + 14.0), Align2::CENTER_CENTER, note, FontId::proportional(12.5), theme.accent);
                        }
                        paint_reward(Pos2::new(card.center().x, card.max.y - 18.0), true);
                    }
                    Some(x) => {
                        painter.text(Pos2::new(x, card.min.y + 24.0), Align2::LEFT_CENTER, name, FontId::proportional(17.0), name_color);
                        if let Some(note) = &note {
                            painter.text(Pos2::new(card.max.x - 16.0, card.min.y + 24.0), Align2::RIGHT_CENTER, note, FontId::proportional(12.5), theme.accent);
                        }
                        // As many lines of the description as fit above the rewards.
                        let bottom = if reward_text.is_some() { card.max.y - 30.0 } else { card.max.y - 10.0 };
                        let lines = (((bottom - card.min.y - 40.0) / 16.0).floor() as usize).max(1);
                        let mut job = egui::text::LayoutJob::simple(desc.to_owned(), FontId::proportional(12.5), theme.text_muted, text_w);
                        job.wrap.max_rows = lines;
                        let galley = painter.layout_job(job);
                        elided = galley.elided;
                        painter.galley(Pos2::new(x, card.min.y + 40.0), galley, theme.text_muted);
                        paint_reward(Pos2::new(x, card.max.y - 17.0), false);
                    }
                }
                // Hovered: the whole description if cut, and what it unlocks.
                let resp = if elided || reward.is_some() {
                    resp.on_hover_ui(|ui| {
                        ui.set_max_width(340.0);
                        if elided {
                            ui.add(egui::Label::new(egui::RichText::new(desc).size(12.5).color(theme.text)).wrap());
                        }
                        if reward.is_some() {
                            if elided {
                                ui.add_space(8.0);
                            }
                            self.unlocks_ui(ui, game);
                        }
                    })
                } else {
                    resp
                };
                if resp.clicked() {
                    open = Some(game);
                }
                resp.context_menu(|ui| menu_item(ui, &format!("⧉  {}", t.new_window_open), game, &mut new_window));
            }
        });
        if back {
            self.home_game = false;
            self.games_hub = false;
        }
        if let Some(game) = new_window {
            self.game_to_new_window(game);
        } else if let Some(game) = open.filter(|&g| !self.show_game_elsewhere(g)) {
            self.open_home_game(game);
        }
    }

    /// A game's board from floor, beside its page: the player's best (`best`, worded), then floor's
    /// best rounds (`score` words each one); without a pseudo, why and a button to pick one (true: clicked).
    pub(super) fn game_board(&self, ui: &mut Ui, area: Rect, theme: &Theme, board: Option<&[crate::floor::GameEntry]>, best: Option<String>, score: fn(u32) -> String) -> bool {
        self.game_board_noted(ui, area, theme, board, best, score, &|_| None)
    }

    /// The same, each line with a note before its score (`note` words it from the line: Bug Hunt's wave).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn game_board_noted(&self, ui: &mut Ui, area: Rect, theme: &Theme, board: Option<&[crate::floor::GameEntry]>, best: Option<String>, score: fn(u32) -> String, note: &dyn Fn(&crate::floor::GameEntry) -> Option<String>) -> bool {
        let t = self.t();
        let painter = ui.painter_at(area);
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let mut y = area.min.y + 4.0;
        painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, t.game_board_world, FontId::proportional(15.0), theme.text);
        y += 28.0;
        if let Some(best) = best {
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, best, FontId::proportional(13.0), theme.accent);
            y += 24.0;
        }
        if pseudo.is_none() {
            let galley = painter.layout(t.game_need_pseudo.to_owned(), FontId::proportional(12.5), theme.text_muted, area.width());
            let h = galley.size().y;
            painter.galley(Pos2::new(area.min.x, y), galley, theme.text_muted);
            y += h + 10.0;
            return ui.put(Rect::from_min_size(Pos2::new(area.min.x, y), Vec2::new(150.0, 28.0)), egui::Button::new(egui::RichText::new(t.pseudo_pick).size(13.0)).corner_radius(6.0)).clicked();
        }
        let Some(board) = board else {
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, "…", FontId::proportional(13.0), theme.text_muted);
            return false;
        };
        if board.is_empty() {
            let galley = painter.layout(t.game_board_empty.to_owned(), FontId::proportional(12.5), theme.text_muted, area.width());
            painter.galley(Pos2::new(area.min.x, y), galley, theme.text_muted);
            return false;
        }
        for entry in board {
            if y + 18.0 > area.max.y {
                break;
            }
            let color = if entry.me { theme.accent } else { theme.text };
            let name = if entry.me { pseudo.as_deref() } else { entry.name.as_deref() }.unwrap_or(t.game_anonymous);
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, format!("{}.", entry.rank), FontId::monospace(13.0), theme.text_muted);
            // Their avatar, with their frame.
            let account = self.config.settings.floor.as_ref();
            let face = entry.id.as_deref().and_then(|id| self.avatars.get(ui.ctx(), account, id, entry.avatar.as_deref()));
            super::talk::paint_avatar(&painter, Pos2::new(area.min.x + 40.0, y + 8.0), 8.0, name, false, face.as_ref(), entry.frame.as_deref(), theme);
            super::office::paint_badge(&painter, Pos2::new(area.min.x + 40.0, y + 8.0), 8.0, &entry.badges);
            painter.text(Pos2::new(area.min.x + 56.0, y), Align2::LEFT_TOP, name, FontId::proportional(13.0), color);
            painter.text(Pos2::new(area.max.x, y), Align2::RIGHT_TOP, score(entry.score), FontId::monospace(13.0), color);
            if let Some(note) = note(entry) {
                painter.text(Pos2::new(area.max.x - 76.0, y + 1.0), Align2::RIGHT_TOP, note, FontId::proportional(12.0), theme.text_muted);
            }
            y += 22.0;
        }
        false
    }
}
