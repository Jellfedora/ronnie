//! Pause Café, in the games (the rules are `crate::coffee`): the office in cross-section, a job picked
//! with 1 to 4 or a click, given by clicking an intern. Each level finished opens the next; floor keeps
//! the total of the best scores, and three levels win the Game of Thrones theme.

use super::four::HomeGame;
use super::*;
use crate::coffee::{Cell, Day, Intern, Job, Refusal, State, H, JOBS, LEVELS, TICK, W};

/// How far from an intern a click still reaches it, in cells.
const REACH: f32 = 2.6;
/// Seconds a refusal stays on screen.
const REFUSAL_SECS: f64 = 3.5;

/// The day under way, or the last one.
pub(super) struct Coffee {
    day: Option<Day>,
    /// The level picked.
    level: usize,
    job: Job,
    /// Time not ticked yet.
    acc: f32,
    /// When the day ended (app time): a key starts the next one a moment after.
    ended: Option<f64>,
    /// The day ended beat the level's best.
    record: bool,
    /// The day ended was won: in working hours, a run of wins is one game.
    won: bool,
    /// Time stands still: jobs can still be given.
    paused: bool,
    /// The last job refused, and when (app time): said under the office.
    refused: Option<(Refusal, f64)>,
    pub(super) floor: crate::floor::GameFloor,
}

impl Default for Coffee {
    fn default() -> Self {
        Self { day: None, level: 0, job: Job::DevOps, acc: 0.0, ended: None, record: false, won: false, paused: false, refused: None, floor: crate::floor::GameFloor::new("pause-cafe", false) }
    }
}

impl Coffee {
    /// A day is under way.
    fn playing(&self) -> bool {
        self.day.as_ref().is_some_and(|d| !d.over)
    }

    /// The home page is left: a day under way stops.
    pub(super) fn leave(&mut self) {
        if self.playing() {
            self.day = None;
            self.ended = None;
        }
    }

    /// The level under way, from the start.
    fn restart(&mut self) {
        if let Some(level) = self.day.as_ref().map(|d| d.level) {
            self.day = Some(Day::new(level));
            self.acc = 0.0;
            self.paused = false;
            self.refused = None;
        }
    }

    /// The intern the pointer at `p` would give the job to, and why it would refuse: the nearest that
    /// can take it, or else the nearest.
    fn aimed(&self, office: Rect, cell: f32, p: Pos2) -> Option<(usize, Option<Refusal>)> {
        let day = self.day.as_ref()?;
        let at = |i: &Intern| Pos2::new(office.min.x + (i.x as f32 + 0.5) * cell, office.min.y + (i.y as f32 + 0.5) * cell);
        let near = |able: bool| {
            let reached = day.interns.iter().enumerate().filter(|(_, i)| !matches!(i.state, State::Saved | State::Lost));
            reached.map(|(n, i)| (n, at(i).distance(p))).filter(|&(n, d)| d < cell * REACH && (!able || day.refusal(n, self.job).is_none())).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(n, _)| n)
        };
        let n = near(true).or_else(|| near(false))?;
        Some((n, day.refusal(n, self.job)))
    }

    /// A click in the office, aimed at this intern: the job given, or why not. Once a job is all
    /// given, the next one left is picked.
    fn click(&mut self, aimed: Option<(usize, Option<Refusal>)>, now: f64) {
        let job = self.job;
        let Some(day) = self.day.as_mut() else { return };
        let Some((n, refusal)) = aimed else {
            self.refused = Some((Refusal::Nobody, now));
            return;
        };
        if let Some(why) = refusal {
            self.refused = Some((why, now));
            return;
        }
        day.assign(n, job);
        self.refused = None;
        let left = |j: Job| JOBS.iter().position(|&x| x == j).map_or(0, |i| day.jobs[i]);
        if left(job) == 0
            && let Some(&next) = JOBS.iter().find(|&&j| left(j) > 0)
        {
            self.job = next;
        }
    }

    fn advance(&mut self, dt: f32) {
        let Some(day) = self.day.as_mut().filter(|d| !d.over) else { return };
        self.acc += dt;
        while self.acc >= TICK && !day.over {
            self.acc -= TICK;
            day.tick();
        }
    }
}

/// The level's best score, 0 if it was never finished.
fn best_of(bests: &[u32], level: usize) -> u32 {
    bests.get(level).copied().unwrap_or(0)
}

/// A level can be played once the one before it is finished.
fn is_open(bests: &[u32], level: usize) -> bool {
    level == 0 || best_of(bests, level - 1) > 0
}

impl App {
    /// Pause Café, on the home page.
    pub(super) fn open_coffee(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Coffee;
    }

    pub(super) fn coffee_page(&mut self, ui: &mut Ui, rect: Rect) {
        let playing = self.coffee.playing();
        // A day under way is the half-day of work's; a run of wins goes on, then the next gives way.
        if playing {
            self.count_round(HomeGame::Coffee);
        } else if self.coffee.ended.is_none()
            && let Some(p) = self.game_locked(HomeGame::Coffee)
        {
            // Its page still shown, to look at; nothing to start (it says why).
            self.locked_band(ui, rect, HomeGame::Coffee, &p);
        }
        self.fold_for_game(playing);
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        self.coffee.floor.poll();
        if !playing {
            self.coffee.floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.coffee_name, 40.0, theme.accent, 1.0);
        if !playing {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.home_game = false;
                return;
            }
        }

        // The office, as large as the page lets it, with the board beside it when there is room.
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 136.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 70.0));
        let side_w = 240.0;
        let wide = !playing && body.width() >= 900.0;
        let room = if wide { Vec2::new(body.width() - side_w - 32.0, body.height()) } else { body.size() };
        let cell = (room.x / W as f32).min(room.y / H as f32).clamp(9.0, 30.0).floor();
        let size = Vec2::new(cell * W as f32, cell * H as f32);
        let block_w = if wide { size.x + 32.0 + side_w } else { size.x };
        let office = Rect::from_min_size(Pos2::new(body.center().x - block_w / 2.0, body.min.y), size);
        let bar = Rect::from_min_max(Pos2::new(office.min.x, office.min.y - 40.0), Pos2::new(office.max.x, office.min.y - 8.0));

        let free = ui.ctx().memory(|m| m.focused().is_none());
        let (picked, start, stop, pause, restart) = ui.input(|i| {
            let key = |k: Key| free && i.key_pressed(k);
            let picked = if free { i.events.iter().find_map(|e| super::digit_key(e, 4)) } else { None };
            (picked, key(Key::Space) || key(Key::Enter), key(Key::Escape), key(Key::P), key(Key::R))
        });
        if playing {
            if let Some(i) = picked {
                self.coffee.job = JOBS[i];
            }
            if pause {
                self.coffee.paused = !self.coffee.paused;
            }
            if restart {
                self.coffee.restart();
            }
            self.jobs_bar(ui, bar, &theme);
        } else {
            self.levels_bar(ui, bar, &theme);
        }

        let can_start = self.coffee.won || self.game_locked(HomeGame::Coffee).is_none();
        if !playing && start && can_start && self.coffee.ended.is_none_or(|at| now - at > 0.6) {
            let c = &mut self.coffee;
            c.day = Some(Day::new(c.level));
            c.job = c.day.as_ref().and_then(|d| JOBS.iter().zip(d.jobs).find(|(_, n)| *n > 0).map(|(j, _)| *j)).unwrap_or(Job::DevOps);
            c.acc = 0.0;
            c.ended = None;
            c.record = false;
            c.won = false;
            c.paused = false;
            c.refused = None;
        } else if playing {
            let click = ui.interact(office, ui.id().with("coffee-office"), Sense::click());
            let aimed = click.hover_pos().and_then(|p| self.coffee.aimed(office, cell, p));
            if aimed.is_some_and(|(_, why)| why.is_none()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if click.clicked() {
                self.coffee.click(aimed, now);
            }
            if stop && let Some(day) = self.coffee.day.as_mut() {
                day.finish();
            }
            if !self.coffee.paused {
                self.coffee.advance(ui.input(|i| i.stable_dt).min(0.1));
            }
            ui.ctx().request_repaint();
            if !self.coffee.playing() {
                self.coffee_over(now);
            }
        }

        let hovered = ui.input(|i| i.pointer.hover_pos()).filter(|p| office.contains(*p)).and_then(|p| self.coffee.aimed(office, cell, p));
        self.paint_office(ui, office, cell, &theme, now, hovered);

        // Under the office: where the day stands, how it ended or how to start one.
        let locked = (!playing && !self.coffee.won).then(|| self.game_locked(HomeGame::Coffee)).flatten().map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let level = &LEVELS[self.coffee.level];
        let waiting = t.coffee_start.replace("{n}", &(self.coffee.level + 1).to_string()).replace("{name}", t.coffee_levels[self.coffee.level]).replace("{need}", &level.need.to_string()).replace("{all}", &level.interns.to_string());
        let (line, color) = match &self.coffee.day {
            Some(d) if d.over && d.won() => (format!("{}{}", t.coffee_won.replace("{s}", &d.score().to_string()), if self.coffee.record { format!("  {}", t.game_new_best) } else { String::new() }), theme.accent),
            Some(d) if d.over => (t.coffee_lost.replace("{s}", &d.saved().to_string()).replace("{n}", &d.level().need.to_string()), theme.text),
            Some(d) => (t.coffee_hud.replace("{s}", &d.saved().to_string()).replace("{n}", &d.level().need.to_string()).replace("{l}", &d.lost().to_string()).replace("{t}", &d.secs_left().to_string()), theme.text),
            None => (waiting.clone(), theme.text),
        };
        let below = match (&self.coffee.day, locked) {
            (_, Some(locked)) => locked,
            (Some(d), None) if d.over => waiting,
            _ => t.coffee_help.to_owned(),
        };
        ui.painter().text(Pos2::new(office.center().x, office.max.y + 22.0), Align2::CENTER_CENTER, line, FontId::proportional(14.0), color);
        // A job refused says why, in place of the help.
        let refused = self.coffee.refused.filter(|(_, at)| playing && now - at < REFUSAL_SECS).map(|(why, _)| t.coffee_refusals[why as usize]);
        let (below, below_color) = match refused {
            Some(why) => (why.to_owned(), theme.ansi[1]),
            None => (below, theme.text_muted),
        };
        ui.painter().text(Pos2::new(office.center().x, office.max.y + 44.0), Align2::CENTER_CENTER, below, FontId::proportional(if refused.is_some() { 14.0 } else { 12.5 }), below_color);

        if wide {
            let side = Rect::from_min_max(Pos2::new(office.max.x + 32.0, office.min.y), Pos2::new(office.max.x + 32.0 + side_w, office.max.y));
            let total: u32 = self.config.settings.coffee_bests.iter().sum();
            let best = (total > 0).then(|| t.coffee_best.replace("{s}", &total.to_string()));
            if self.game_board(ui, side, &theme, self.coffee.floor.board(), best, |s| s.to_string()) {
                self.ask_pseudo();
            }
        }
    }

    /// The jobs of the day, each with what is left of it.
    fn jobs_bar(&mut self, ui: &mut Ui, bar: Rect, theme: &Theme) {
        let t = self.t();
        let Some(left) = self.coffee.day.as_ref().map(|d| d.jobs) else { return };
        // Pause and restart at the end of the bar, the jobs sharing what is left.
        let side = 118.0;
        let w = (bar.width() - side * 2.0 - 40.0) / 4.0;
        let at = |n: f32| bar.max.x - side * n - 8.0 * (n - 1.0);
        let paused = self.coffee.paused;
        let pause = egui::Button::new(egui::RichText::new(format!("P  {}", if paused { t.coffee_resume } else { t.coffee_pause })).size(12.5)).corner_radius(6.0).stroke(if paused { Stroke::new(1.5, theme.accent) } else { Stroke::NONE });
        if ui.put(Rect::from_min_size(Pos2::new(at(2.0), bar.min.y), Vec2::new(side, bar.height())), pause).clicked() {
            self.coffee.paused = !paused;
        }
        let restart = egui::Button::new(egui::RichText::new(format!("R  {}", t.coffee_restart)).size(12.5)).corner_radius(6.0);
        if ui.put(Rect::from_min_size(Pos2::new(at(1.0), bar.min.y), Vec2::new(side, bar.height())), restart).clicked() {
            self.coffee.restart();
        }
        for (i, &job) in JOBS.iter().enumerate() {
            let r = Rect::from_min_size(bar.min + Vec2::new(i as f32 * (w + 8.0), 0.0), Vec2::new(w, bar.height()));
            let on = self.coffee.job == job;
            let text = egui::RichText::new(format!("{}  {}  ×{}", i + 1, t.coffee_jobs[i], left[i])).size(12.5).color(if left[i] == 0 { theme.text_muted } else { theme.text });
            let mut button = egui::Button::new(text).corner_radius(6.0).stroke(if on { Stroke::new(2.0, theme.accent) } else { Stroke::NONE });
            if on {
                button = button.fill(theme.accent.gamma_multiply(0.3));
            }
            if ui.put(r, button).on_hover_text(t.coffee_job_tips[i]).clicked() {
                self.coffee.job = job;
            }
        }
    }

    /// The levels: the ones finished ticked, the ones not open yet greyed out.
    fn levels_bar(&mut self, ui: &mut Ui, bar: Rect, theme: &Theme) {
        let t = self.t();
        let n = LEVELS.len();
        let w = ((bar.width() - (n as f32 - 1.0) * 6.0) / n as f32).min(52.0);
        let left = bar.center().x - (w * n as f32 + 6.0 * (n as f32 - 1.0)) / 2.0;
        for i in 0..n {
            let r = Rect::from_min_size(Pos2::new(left + i as f32 * (w + 6.0), bar.min.y), Vec2::new(w, bar.height()));
            let bests = &self.config.settings.coffee_bests;
            let (open, best) = (is_open(bests, i), best_of(bests, i));
            let label = if best > 0 { format!("{} ✓", i + 1) } else { (i + 1).to_string() };
            let button = egui::Button::new(egui::RichText::new(label).size(12.5)).corner_radius(6.0).stroke(if self.coffee.level == i { Stroke::new(1.5, theme.accent) } else { Stroke::NONE });
            if ui.add_enabled_ui(open, |ui| ui.put(r, button).on_hover_text(t.coffee_levels[i])).inner.clicked() {
                self.coffee.level = i;
                self.coffee.day = None;
                self.coffee.ended = None;
            }
        }
    }

    /// The day just ended: the level's best kept, the total sent to floor, the theme won at the third.
    fn coffee_over(&mut self, now: f64) {
        self.coffee.ended = Some(now);
        let Some(day) = &self.coffee.day else { return };
        let (level, score) = (day.level, day.score());
        self.coffee.won = day.won();
        if !self.coffee.won {
            return;
        }
        let bests = &mut self.config.settings.coffee_bests;
        let had_theme = crate::coffee::thrones_won(bests);
        if bests.len() <= level {
            bests.resize(level + 1, 0);
        }
        self.coffee.record = score > bests[level];
        if self.coffee.record {
            bests[level] = score;
        }
        let won_theme = !had_theme && crate::coffee::thrones_won(bests);
        let (total, levels): (u32, u32) = (bests.iter().sum(), bests.iter().filter(|&&b| b > 0).count() as u32);
        self.save_config();
        if won_theme {
            let t = self.t();
            self.push_toast(super::Toast { ok: true, title: t.thrones_unlocked_title.to_owned(), body: t.thrones_unlocked_body.to_owned(), tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
            self.winter = Some(f64::NAN);
        }
        if level + 1 < LEVELS.len() {
            self.coffee.level = level + 1;
        }
        // Online only with a pseudo.
        if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.coffee.floor.finished(crate::floor::GameRound { score: total, details: serde_json::json!({ "score": total, "levels": levels }) });
            self.coffee.floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
        }
    }

    /// The office in cross-section: its floors, the lift, the coffee machine and the interns.
    fn paint_office(&self, ui: &Ui, office: Rect, cell: f32, theme: &Theme, now: f64, hovered: Option<(usize, Option<Refusal>)>) {
        let painter = ui.painter_at(office.expand(2.0));
        painter.rect_filled(office, 6.0, super::sidebar::lerp_color(theme.bg, Color32::BLACK, 0.2));
        let shown;
        let day = match &self.coffee.day {
            Some(d) => d,
            None => {
                shown = Day::new(self.coffee.level);
                &shown
            }
        };
        let at = |x: f32, y: f32| Pos2::new(office.min.x + x * cell, office.min.y + y * cell);
        for y in 0..H {
            for x in 0..W {
                let c = day.cell(x, y);
                if c == Cell::Empty {
                    continue;
                }
                let r = Rect::from_min_size(at(x as f32, y as f32), Vec2::splat(cell));
                match c {
                    Cell::Soft => {
                        painter.rect_filled(r, 0.0, Color32::from_rgb(196, 176, 142));
                        painter.hline(r.x_range(), r.min.y + 0.5, Stroke::new(1.0, Color32::from_rgb(222, 206, 176)));
                    }
                    Cell::Hard => {
                        painter.rect_filled(r, 0.0, Color32::from_rgb(92, 96, 104));
                        painter.circle_filled(r.min + Vec2::splat(cell * 0.3), cell * 0.06, Color32::from_rgb(120, 124, 132));
                        painter.circle_filled(r.min + Vec2::splat(cell * 0.7), cell * 0.05, Color32::from_rgb(70, 74, 82));
                    }
                    _ => {
                        let color = if (x + y) % 2 == 0 { theme.ansi[4] } else { theme.ansi[1] };
                        painter.rect_filled(r.shrink(0.5), 1.5, color);
                        painter.rect_filled(Rect::from_center_size(r.center(), Vec2::new(cell * 0.5, cell * 0.22)), 1.0, Color32::WHITE);
                    }
                }
            }
        }
        let l = day.level();
        // The lift, its doors open.
        let lift = Rect::from_min_max(at(l.lift.0 as f32 - 0.7, l.lift.1 as f32 - 1.6), at(l.lift.0 as f32 + 0.7, l.lift.1 as f32 + 1.0));
        painter.rect_filled(lift, 2.0, Color32::from_rgb(52, 56, 64));
        painter.rect_stroke(lift, 2.0, Stroke::new(1.5, Color32::from_rgb(170, 176, 186)), egui::StrokeKind::Inside);
        paint_machine(&painter, at(l.coffee.0 as f32 + 0.5, l.coffee.1 as f32 + 1.0), cell, now);
        let frac = (self.coffee.acc / TICK).clamp(0.0, 1.0);
        for (n, i) in day.interns.iter().enumerate() {
            if matches!(i.state, State::Saved | State::Lost) {
                continue;
            }
            let x = i.was.0 as f32 + (i.x - i.was.0) as f32 * frac;
            let y = i.was.1 as f32 + (i.y - i.was.1) as f32 * frac;
            paint_intern(&painter, at(x + 0.5, y + 1.0), cell, i, theme, now);
            // The one a click would reach: green if it can take the job, red if not, the job above it.
            if let Some((_, why)) = hovered.filter(|(aimed, _)| *aimed == n && self.coffee.playing()) {
                let color = if why.is_none() { theme.ansi[2] } else { theme.ansi[1] };
                let c = at(x + 0.5, y + 0.4);
                painter.circle_stroke(c, cell * 1.05, Stroke::new(2.0, color));
                let slot = JOBS.iter().position(|&j| j == self.coffee.job).unwrap_or(0);
                painter.text(c - Vec2::new(0.0, cell * 1.5), Align2::CENTER_BOTTOM, self.t().coffee_jobs[slot], FontId::proportional(12.0), color);
            }
        }
        if self.coffee.paused && self.coffee.playing() {
            painter.text(office.center_top() + Vec2::new(0.0, cell * 1.6), Align2::CENTER_CENTER, self.t().coffee_paused, FontId::proportional(cell * 1.3), theme.accent);
        }
    }
}

/// The coffee machine standing at `foot`, steam going up.
fn paint_machine(painter: &egui::Painter, foot: Pos2, cell: f32, now: f64) {
    let body = Rect::from_min_max(foot + Vec2::new(-cell * 0.8, -cell * 2.0), foot + Vec2::new(cell * 0.8, 0.0));
    painter.rect_filled(body, 3.0, Color32::from_rgb(170, 40, 44));
    painter.rect_filled(Rect::from_center_size(body.center() + Vec2::new(0.0, cell * 0.25), Vec2::new(cell * 0.9, cell * 0.8)), 2.0, Color32::from_rgb(30, 26, 26));
    painter.rect_filled(Rect::from_center_size(body.center() + Vec2::new(0.0, cell * 0.45), Vec2::new(cell * 0.4, cell * 0.36)), 1.0, Color32::WHITE);
    for n in 0..3 {
        let phase = ((now * 0.6 + n as f64 / 3.0) % 1.0) as f32;
        let p = body.center_top() + Vec2::new((phase * 9.0 + n as f32).sin() * cell * 0.25, -phase * cell * 1.4);
        painter.circle_filled(p, cell * (0.1 + phase * 0.14), Color32::WHITE.gamma_multiply(0.5 * (1.0 - phase)));
    }
}

/// An intern standing at `foot`: a lanyard on a shirt, and what its job shows.
fn paint_intern(painter: &egui::Painter, foot: Pos2, cell: f32, i: &Intern, theme: &Theme, now: f64) {
    let s = cell * 0.95;
    let swing = if i.state == State::Walking { ((now * 9.0).sin() as f32) * s * 0.14 } else { 0.0 };
    let shirt = match i.state {
        State::Stopping => theme.ansi[1],
        State::Digging => theme.ansi[2],
        State::Building(_) => theme.ansi[3],
        _ => Color32::from_rgb(120, 180, 240),
    };
    let legs = Stroke::new(s * 0.15, Color32::from_rgb(48, 52, 74));
    let hip = foot - Vec2::new(0.0, s * 0.42);
    painter.line_segment([hip, foot + Vec2::new(swing, 0.0)], legs);
    painter.line_segment([hip, foot - Vec2::new(swing, 0.0)], legs);
    let body = Rect::from_center_size(foot - Vec2::new(0.0, s * 0.7), Vec2::new(s * 0.5, s * 0.6));
    painter.rect_filled(body, s * 0.1, shirt);
    let head = foot - Vec2::new(0.0, s * 1.2);
    painter.circle_filled(head, s * 0.24, Color32::from_rgb(244, 208, 170));
    painter.circle_filled(head + Vec2::new(i.dir as f32 * s * 0.1, -s * 0.02), s * 0.04, Color32::BLACK);
    let arm = Stroke::new(s * 0.12, shirt);
    let shoulder = body.center_top() + Vec2::new(0.0, s * 0.08);
    match i.state {
        State::Stopping => {
            painter.line_segment([shoulder, shoulder + Vec2::new(-s * 0.5, -s * 0.1)], arm);
            painter.line_segment([shoulder, shoulder + Vec2::new(s * 0.5, -s * 0.1)], arm);
            painter.line_segment([body.center_top(), body.center() + Vec2::new(0.0, s * 0.14)], Stroke::new(s * 0.09, Color32::from_rgb(40, 40, 60)));
        }
        State::Digging => {
            let lift = ((now * 10.0).sin() as f32 * 0.5 + 0.5) * s * 0.3;
            let tip = foot + Vec2::new(i.dir as f32 * s * 0.25, -lift);
            painter.line_segment([shoulder, tip], Stroke::new(s * 0.08, Color32::from_rgb(200, 200, 210)));
        }
        State::Building(_) => {
            let hand = shoulder + Vec2::new(i.dir as f32 * s * 0.4, s * 0.05);
            painter.line_segment([shoulder, hand], arm);
            painter.rect_filled(Rect::from_center_size(hand, Vec2::new(s * 0.26, s * 0.34)), 1.0, theme.ansi[4]);
        }
        _ => {}
    }
    if i.umbrella && i.state == State::Falling {
        let top = head - Vec2::new(0.0, s * 0.62);
        let canopy: Vec<Pos2> = (0..=10).map(|n| top + Vec2::angled(std::f32::consts::PI * (1.0 + n as f32 / 10.0)) * s * 0.6).collect();
        painter.add(egui::Shape::convex_polygon(canopy, theme.ansi[5], Stroke::NONE));
        painter.line_segment([top, head], Stroke::new(s * 0.05, Color32::WHITE));
    }
}

/// Pause Café's icon: a steaming cup.
pub(super) fn paint_coffee_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = 10.0 * scale;
    let cup = Rect::from_center_size(c + Vec2::new(-s * 0.15, s * 0.3), Vec2::new(s * 1.3, s * 1.1));
    painter.rect_filled(cup, s * 0.2, theme.accent);
    painter.rect_filled(Rect::from_center_size(cup.center_top() + Vec2::new(0.0, s * 0.14), Vec2::new(s * 1.05, s * 0.18)), 1.0, Color32::from_rgb(70, 42, 26));
    painter.circle_stroke(cup.right_center() + Vec2::new(s * 0.06, 0.0), s * 0.28, Stroke::new(s * 0.14, theme.accent));
    for n in [-0.35_f32, 0.05, 0.45] {
        let x = cup.center().x + n * s;
        painter.line_segment([Pos2::new(x, cup.min.y - s * 0.18), Pos2::new(x + s * 0.1, cup.min.y - s * 0.72)], Stroke::new(s * 0.1, theme.text_muted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_goes_back_to_the_start_of_the_same_level() {
        let mut c = Coffee { day: Some(Day::new(2)), paused: true, ..Coffee::default() };
        let day = c.day.as_mut().unwrap();
        day.tick();
        day.assign(0, Job::Manager);
        c.restart();
        let day = c.day.as_ref().unwrap();
        assert_eq!((day.level, day.ticks, day.interns.len()), (2, 0, 0));
        assert_eq!(day.jobs, LEVELS[2].jobs);
        assert!(!c.paused);
    }

    #[test]
    fn a_level_opens_when_the_one_before_is_finished() {
        assert!(is_open(&[], 0));
        assert!(!is_open(&[], 1));
        assert!(is_open(&[800], 1));
        assert!(!is_open(&[800, 0], 2));
    }
}
