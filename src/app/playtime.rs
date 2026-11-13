//! The games during working hours (see `crate::playtime`): what was played counted, and in place of a
//! game played enough, a page saying when it is back.

use super::*;
use super::four::HomeGame;
use crate::playtime::{reseal, Period, BLOB_SECS, FISH_CATCHES, MINES_GAMES};

impl App {
    /// The half-day of work under way, what was played in it checked (see `crate::playtime::checked`).
    /// Development builds, even in release, are never limited (the games are to be tried as often as
    /// needed).
    pub(super) fn work_period(&mut self) -> Option<Period> {
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
            HomeGame::Coffee => usage.coffee,
            HomeGame::Larsen => usage.larsen,
            HomeGame::President => usage.president,
            HomeGame::BugHunt => usage.bughunt,
            HomeGame::Fishing => usage.fish >= FISH_CATCHES,
            HomeGame::Monday => usage.monday,
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
            HomeGame::Coffee => &mut usage.coffee,
            HomeGame::Larsen => &mut usage.larsen,
            HomeGame::President => &mut usage.president,
            HomeGame::BugHunt => &mut usage.bughunt,
            HomeGame::Monday => &mut usage.monday,
            HomeGame::Blob | HomeGame::Mines | HomeGame::Fishing => return,
        };
        if !*played {
            *played = true;
            reseal(usage);
            self.save_config();
        }
    }

    /// A candidate hired at the fishing: counted in the half-day.
    pub(super) fn count_fish(&mut self) {
        if self.work_period().is_none() {
            return;
        }
        let usage = &mut self.config.settings.game_usage;
        usage.fish = (usage.fish + 1).min(FISH_CATCHES);
        reseal(usage);
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

    /// Why `game` can't be played again this half-day.
    fn locked_why(&self, game: HomeGame, p: &Period) -> String {
        let t = self.t();
        let name = match game {
            HomeGame::SpeedMetal => "Speed Metal",
            HomeGame::Four => t.four_name,
            HomeGame::Blob => t.blob_name,
            HomeGame::Mines => t.mines_name,
            HomeGame::Runner => t.runner_name,
            HomeGame::Riff => t.riff_name,
            HomeGame::Backstage => t.backstage_name,
            HomeGame::Coffee => t.coffee_name,
            HomeGame::Larsen => t.larsen_name,
            HomeGame::President => t.president_name,
            HomeGame::BugHunt => t.bughunt_name,
            HomeGame::Fishing => t.fishing_name,
            HomeGame::Monday => t.monday_name,
        };
        match (game, p.morning) {
            (HomeGame::Blob, true) => t.playtime_blob_morning.to_owned(),
            (HomeGame::Blob, false) => t.playtime_blob_afternoon.to_owned(),
            (HomeGame::Mines, true) => t.playtime_mines_morning.replace("{n}", &MINES_GAMES.to_string()),
            (HomeGame::Mines, false) => t.playtime_mines_afternoon.replace("{n}", &MINES_GAMES.to_string()),
            (_, true) => t.playtime_round_morning.replace("{game}", name),
            (_, false) => t.playtime_round_afternoon.replace("{game}", name),
        }
    }

    /// Over `game`'s page, played enough this half-day (the page still there, to look at): why, and
    /// when it's back.
    pub(super) fn locked_band(&self, ui: &Ui, rect: Rect, game: HomeGame, p: &Period) {
        let t = self.t();
        let theme = &self.theme;
        let text = format!("{}  {}", self.locked_why(game, p), t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new(("locked-band", game as u8))));
        let galley = painter.layout(text, FontId::proportional(12.5), theme.text, (rect.width() - 300.0).max(220.0));
        let band = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 18.0 + galley.size().y / 2.0), galley.size() + Vec2::new(28.0, 12.0));
        painter.rect(band, 12.0, theme.chrome_bg, Stroke::new(1.0, theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
        painter.galley(band.center() - galley.size() / 2.0, galley, theme.text);
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
