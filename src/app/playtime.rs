//! The games during working hours (see `crate::playtime`): what was played counted, and in place of a
//! game played enough, a page saying when it is back.

use super::*;
use super::four::HomeGame;
use crate::playtime::{reseal, Period, BLOB_SECS, MINES_GAMES};

impl App {
    /// The half-day of work under way, what was played in it checked (see `crate::playtime::checked`).
    /// Development builds, even in release, are never limited (the games are to be tried as often as
    /// needed).
    fn work_period(&mut self) -> Option<Period> {
        if !crate::config::OFFICIAL {
            return None;
        }
        crate::playtime::checked(&mut self.config.settings.game_usage, chrono::Local::now().naive_local())
    }

    /// The half-day `game` was played enough in, if so.
    pub(super) fn game_locked(&mut self, game: HomeGame) -> Option<Period> {
        let p = self.work_period()?;
        let usage = &self.config.settings.game_usage;
        let used = match game {
            HomeGame::SpeedMetal => usage.speed_metal,
            HomeGame::Four => usage.four,
            HomeGame::Blob => usage.blob_secs >= BLOB_SECS,
            HomeGame::Mines => usage.mines,
            HomeGame::Runner => usage.runner,
            HomeGame::Riff => usage.riff,
            HomeGame::Backstage => usage.backstage,
        };
        used.then_some(p)
    }

    /// A game of `game` under way: it is the half-day's (of Speed Metal, Puissance 4, the minesweeper,
    /// Ronnie Run, Riff Hero or Backstage).
    pub(super) fn count_round(&mut self, game: HomeGame) {
        if self.work_period().is_none() {
            return;
        }
        let usage = &mut self.config.settings.game_usage;
        // The minesweeper's games, counted as each one starts (not at each frame of it).
        if game == HomeGame::Mines {
            usage.mines_games = (usage.mines_games + 1).min(MINES_GAMES);
            usage.mines = usage.mines_games >= MINES_GAMES;
            reseal(usage);
            self.save_config();
            return;
        }
        let played = match game {
            HomeGame::SpeedMetal => &mut usage.speed_metal,
            HomeGame::Four => &mut usage.four,
            HomeGame::Runner => &mut usage.runner,
            HomeGame::Riff => &mut usage.riff,
            HomeGame::Backstage => &mut usage.backstage,
            HomeGame::Blob | HomeGame::Mines => return,
        };
        if !*played {
            *played = true;
            reseal(usage);
            self.save_config();
        }
    }

    /// `dt` seconds more on Ronnie.io's page; the seconds it has left in the half-day (none: free).
    pub(super) fn count_blob(&mut self, dt: f32) -> Option<u32> {
        self.work_period()?;
        self.blob_played += dt;
        if self.blob_played >= 1.0 {
            let whole = self.blob_played.floor();
            self.blob_played -= whole;
            let usage = &mut self.config.settings.game_usage;
            usage.blob_secs = (usage.blob_secs + whole as u32).min(BLOB_SECS);
            reseal(usage);
            // Saved now and then, and once up (not every second).
            if usage.blob_secs % 15 == 0 || usage.blob_secs >= BLOB_SECS {
                self.save_config();
            }
        }
        Some(BLOB_SECS - self.config.settings.game_usage.blob_secs)
    }

    /// "12h00", "12:00".
    pub(super) fn hour_text(&self, time: chrono::NaiveTime) -> String {
        use chrono::Timelike;
        self.t().playtime_hour.replace("{h}", &time.hour().to_string()).replace("{m}", &format!("{:02}", time.minute()))
    }

    /// In place of `game`, played enough this half-day: why, when it's back, and its board still.
    pub(super) fn locked_page(&mut self, ui: &mut Ui, rect: Rect, game: HomeGame, p: &Period) {
        let t = self.t();
        let theme = self.theme.clone();
        let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
        if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
            self.home_game = false;
        }
        let painter = &ui.painter().clone();
        // Raised, to leave room for the board under it.
        let c = rect.center() - Vec2::new(0.0, ((rect.height() - 520.0) / 2.0).clamp(0.0, 150.0));
        let icon = Pos2::new(c.x, c.y - 120.0);
        match game {
            HomeGame::SpeedMetal => paint_metal(painter, icon, Align2::CENTER_CENTER, "R", 56.0, theme.accent, 1.0),
            HomeGame::Four => super::four::paint_four_icon(painter, icon, &theme, 2.6),
            HomeGame::Blob => super::blob::paint_blob_icon(painter, icon, &theme, 2.6),
            HomeGame::Mines => super::mines::paint_mines_icon(painter, icon, &theme, 2.6),
            HomeGame::Runner => super::runner::paint_runner_icon(painter, icon, &theme, 2.6),
            HomeGame::Riff => super::riff::paint_riff_icon(painter, icon, &theme, 2.6),
            HomeGame::Backstage => super::backstage::paint_backstage_icon(painter, icon, &theme, 2.6),
        }
        paint_metal(painter, Pos2::new(c.x, c.y - 50.0), Align2::CENTER_CENTER, t.playtime_title, 44.0, theme.accent, 1.0);
        let name = match game {
            HomeGame::SpeedMetal => "Speed Metal",
            HomeGame::Four => t.four_name,
            HomeGame::Blob => t.blob_name,
            HomeGame::Mines => t.mines_name,
            HomeGame::Runner => t.runner_name,
            HomeGame::Riff => t.riff_name,
            HomeGame::Backstage => t.backstage_name,
        };
        let why = match (game, p.morning) {
            (HomeGame::Blob, true) => t.playtime_blob_morning.to_owned(),
            (HomeGame::Blob, false) => t.playtime_blob_afternoon.to_owned(),
            (HomeGame::Mines, true) => t.playtime_mines_morning.replace("{n}", &MINES_GAMES.to_string()),
            (HomeGame::Mines, false) => t.playtime_mines_afternoon.replace("{n}", &MINES_GAMES.to_string()),
            (_, true) => t.playtime_round_morning.replace("{game}", name),
            (_, false) => t.playtime_round_afternoon.replace("{game}", name),
        };
        let width = (rect.width() - 48.0).clamp(200.0, 460.0);
        let mut y = c.y + 4.0;
        for (text, size, color) in [(why, 16.0, theme.text), (t.playtime_back_at.replace("{h}", &self.hour_text(p.end)), 15.0, theme.accent), (t.playtime_rule.to_owned(), 12.5, theme.text_muted)] {
            let galley = painter.layout(text, FontId::proportional(size), color, width);
            painter.galley(Pos2::new(c.x - galley.size().x / 2.0, y), galley.clone(), color);
            y += galley.size().y + 12.0;
        }
        let area = Rect::from_min_max(Pos2::new(c.x - 170.0, y + 12.0), Pos2::new(c.x + 170.0, rect.max.y - 12.0));
        if area.height() > 80.0 {
            self.locked_board(ui, area, game);
        }
    }

    /// `game`'s board on floor (fetched again every minute), under the page saying it is played enough.
    fn locked_board(&mut self, ui: &mut Ui, area: Rect, game: HomeGame) {
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        let ctx = ui.ctx().clone();
        let account = self.config.settings.floor.clone();
        let settings = &self.config.settings;
        let bests = |n: u32, text: &str, key: &str, value: String| (n > 0).then(|| text.replace(key, &value));
        let (floor, best, score): (&mut crate::floor::GameFloor, Option<String>, fn(u32) -> String) = match game {
            HomeGame::SpeedMetal => {
                self.poll_floor();
                self.floor.sync(&ctx, now, Some(60.0), account.as_ref());
                let pseudo = crate::floor::pseudo(account.as_ref());
                let scores = crate::floor::shown(self.floor.board(), &self.config.settings.typing_scores, pseudo);
                let board = Rect::from_min_size(area.min, Vec2::new(area.width(), (104.0 + scores.len().max(1) as f32 * 28.0).min(area.height())));
                super::game::paint_board(&ui.painter_at(area), board, &scores, None, &theme, t, 1.0, now);
                return;
            }
            HomeGame::Four => {
                let pseudo = crate::floor::pseudo(account.as_ref()).map(str::to_owned);
                if pseudo.is_some() && self.four.board_at.is_none_or(|at| now - at > 30.0) {
                    self.four.board_at = Some(now);
                    self.versus.fetch_board("puissance-4");
                }
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
                super::four::wins_ui(&mut child, &self.versus, "puissance-4", t.four_wins, pseudo.as_deref(), t, &theme);
                return;
            }
            HomeGame::Blob => (self.blob.floor.get_or_insert_with(|| crate::floor::GameFloor::new("ronnie-io", false)), bests(settings.blob_best, t.blob_best, "{n}", settings.blob_best.to_string()), |s| s.to_string()),
            HomeGame::Mines => (&mut self.mines.floor, bests(settings.mines_best, t.mines_best, "{t}", super::mines::time_text(settings.mines_best)), super::mines::time_text),
            HomeGame::Runner => (&mut self.runner.floor, bests(settings.runner_best, t.runner_best, "{m}", settings.runner_best.to_string()), super::runner::meters_text),
            HomeGame::Riff => {
                let level = self.riff.level.index();
                let best = settings.riff_bests.get(level).copied().unwrap_or(0);
                (&mut self.riff.floors[level], bests(best, t.riff_best, "{s}", best.to_string()), |s| s.to_string())
            }
            HomeGame::Backstage => (&mut self.backstage.floor, bests(settings.backstage_best, t.backstage_best, "{s}", settings.backstage_best.to_string()), |s| s.to_string()),
        };
        floor.poll();
        floor.sync(&ctx, now, Some(60.0), account.as_ref());
        let board = floor.board().map(<[_]>::to_vec);
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        if self.game_board(ui, area, &theme, board.as_deref(), best, score) {
            self.ask_pseudo();
        }
    }

    /// The seconds of Ronnie.io left, at the top of its page once there are few.
    pub(super) fn paint_blob_left(&self, ui: &Ui, rect: Rect, left: u32) {
        if left > 120 {
            return;
        }
        let t = self.t();
        let color = if left <= 30 { self.theme.ansi[1] } else { self.theme.text };
        let text = t.playtime_left.replace("{t}", &format!("{}:{:02}", left / 60, left % 60));
        let painter = ui.painter();
        let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), color);
        let pill = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 92.0), galley.size() + Vec2::new(20.0, 10.0));
        painter.rect_filled(pill, pill.height() / 2.0, self.theme.chrome_bg.gamma_multiply(0.9));
        painter.rect_stroke(pill, pill.height() / 2.0, Stroke::new(1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        painter.galley(pill.center() - galley.size() / 2.0, galley, color);
    }
}
