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
        // The cards in rows of as many as fit (the last one centered): tall when their rows fit the page,
        // else flatter; the page scrolls when even those don't fit.
        let n = games.len();
        let gap = 20.0;
        let title_h = 96.0;
        let columns = |min_w: f32| (((rect.width() - 96.0 + gap) / (min_w + gap)).floor() as usize).clamp(1, n);
        let rows = |cols: usize| n.div_ceil(cols) as f32;
        let tall_cols = columns(200.0);
        let wide = 56.0 + title_h + rows(tall_cols) * (270.0 + gap) <= rect.height();
        let cols = if wide { tall_cols } else { columns(360.0) };
        let card_size = if wide { Vec2::new(((rect.width() - 96.0 - (cols - 1) as f32 * gap) / cols as f32).min(280.0), 270.0) } else { Vec2::new(((rect.width() - 48.0 - (cols - 1) as f32 * gap) / cols as f32).min(520.0), 104.0) };
        let block = Vec2::new(cols as f32 * card_size.x + (cols - 1) as f32 * gap, rows(cols) * (card_size.y + gap) - gap);
        let content_h = (56.0 + title_h + block.y + 24.0).max(rect.height());

        let (mut open, mut new_window, mut back) = (None, None, false);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        egui::ScrollArea::vertical().id_salt("arcade").auto_shrink([false; 2]).show(&mut child, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(rect.width(), content_h), Sense::hover());
            let painter = ui.painter().clone();

            // Back to the home page.
            let back_rect = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            back = ui.put(back_rect, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked();

            let top = (rect.center().y - (block.y + title_h) / 2.0).max(rect.min.y + 56.0);
            paint_metal(&painter, Pos2::new(rect.center().x, top + 34.0), Align2::CENTER_CENTER, t.cat_games, 46.0, theme.accent, 1.0);
            painter.text(Pos2::new(rect.center().x, top + 78.0), Align2::CENTER_CENTER, t.arcade_subtitle, FontId::proportional(13.5), theme.text_muted);

            let origin = Pos2::new(rect.center().x - block.x / 2.0, top + title_h);
            for (k, (game, name, note)) in games.into_iter().enumerate() {
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
                };
                let (icon, text_left, text_w) = if wide { (Pos2::new(card.center().x, card.min.y + 74.0), None, card.width() - 36.0) } else { (Pos2::new(card.min.x + 52.0, card.center().y), Some(card.min.x + 100.0), card.width() - 120.0) };
                let scale = if wide { 2.8 } else { 2.0 };
                match game {
                    HomeGame::SpeedMetal => paint_metal(&painter, icon, Align2::CENTER_CENTER, "R", 20.0 * scale, theme.accent, 1.0),
                    HomeGame::Four => super::four::paint_four_icon(&painter, icon, &theme, scale),
                    HomeGame::Blob => super::blob::paint_blob_icon(&painter, icon, &theme, scale),
                    HomeGame::Mines => super::mines::paint_mines_icon(&painter, icon, &theme, scale),
                    HomeGame::Runner => super::runner::paint_runner_icon(&painter, icon, &theme, scale),
                    HomeGame::Riff => super::riff::paint_riff_icon(&painter, icon, &theme, scale),
                    HomeGame::Backstage => super::backstage::paint_backstage_icon(&painter, icon, &theme, scale),
                }
                let name_color = if hot { theme.text } else { theme.text.gamma_multiply(0.92) };
                let desc_galley = painter.layout(desc.to_owned(), FontId::proportional(13.0), theme.text_muted, text_w);
                match text_left {
                    None => {
                        painter.text(Pos2::new(card.center().x, card.min.y + 150.0), Align2::CENTER_CENTER, name, FontId::proportional(20.0), name_color);
                        let mut job = egui::text::LayoutJob::simple(desc.to_owned(), FontId::proportional(13.0), theme.text_muted, text_w);
                        job.halign = egui::Align::Center;
                        let galley = painter.layout_job(job);
                        painter.galley(Pos2::new(card.center().x, card.min.y + 174.0), galley, theme.text_muted);
                        if let Some(note) = &note {
                            painter.text(Pos2::new(card.center().x, card.max.y - 24.0), Align2::CENTER_CENTER, note, FontId::proportional(12.5), theme.accent);
                        }
                    }
                    Some(x) => {
                        painter.text(Pos2::new(x, card.min.y + 26.0), Align2::LEFT_CENTER, name, FontId::proportional(18.0), name_color);
                        painter.galley(Pos2::new(x, card.min.y + 44.0), desc_galley, theme.text_muted);
                        if let Some(note) = &note {
                            painter.text(Pos2::new(card.max.x - 16.0, card.min.y + 26.0), Align2::RIGHT_CENTER, note, FontId::proportional(12.5), theme.accent);
                        }
                    }
                }
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
            painter.text(Pos2::new(area.min.x + 30.0, y), Align2::LEFT_TOP, name, FontId::proportional(13.0), color);
            painter.text(Pos2::new(area.max.x, y), Align2::RIGHT_TOP, score(entry.score), FontId::monospace(13.0), color);
            y += 22.0;
        }
        false
    }
}
