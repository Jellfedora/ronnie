//! Backstage, in the games: the corridors behind the stage seen from above (see `crate::backstage`),
//! the singer moved with the arrows (or W A S D), an amp dropped with Space, Krog and his mates hunting
//! him. The score is the picks picked, worth more as Krog gets angrier; floor keeps the best.

use super::four::HomeGame;
use super::*;
use crate::backstage::{Dir, Input, Show, Walker, Who, AMP_SECS, H, PER_RAGE, W};

/// The world moves in steps this long, whatever the frame rate.
const STEP: f32 = 1.0 / 120.0;

/// The show under way, or the last one.
pub(super) struct Backstage {
    show: Option<Show>,
    /// When the show ended (app time): a key starts the next one a moment after.
    ended: Option<f64>,
    /// The show ended beat the player's best.
    record: bool,
    /// Time not stepped yet.
    acc: f32,
    /// The amp key went down, not stepped yet (a frame can have no step).
    amp_pending: bool,
    /// The way last pressed, held or not (the singer keeps going).
    dir: Option<Dir>,
    /// When the singer was last caught, spotted (app time): a flash, a "!".
    caught_at: f64,
    spotted_at: f64,
    /// What Krog last did, and when (app time): shown big a moment.
    said: Option<(Said, f64)>,
    rng: u64,
    floor: crate::floor::GameFloor,
}

/// Krog's doings, told in the middle of the maze.
#[derive(Clone, Copy)]
enum Said {
    Angrier,
    Called(Who),
}

impl Default for Backstage {
    fn default() -> Self {
        Self { show: None, ended: None, record: false, acc: 0.0, amp_pending: false, dir: None, caught_at: f64::NEG_INFINITY, spotted_at: f64::NEG_INFINITY, said: None, rng: 0, floor: crate::floor::GameFloor::new("backstage", false) }
    }
}

impl Backstage {
    /// A show is on.
    fn playing(&self) -> bool {
        self.show.as_ref().is_some_and(|s| !s.over)
    }

    /// The home page is left: a show under way stops.
    pub(super) fn leave(&mut self) {
        if self.playing() {
            self.show = None;
            self.ended = None;
        }
    }

    /// `dt` seconds more of the show, in steps of `STEP`; the amp key counts once, at the next step.
    fn advance(&mut self, dt: f32, amp: bool, now: f64) {
        let Some(show) = self.show.as_mut().filter(|s| !s.over) else { return };
        self.acc += dt;
        self.amp_pending |= amp;
        while self.acc >= STEP {
            self.acc -= STEP;
            let events = show.step(STEP, Input { dir: self.dir, amp: std::mem::take(&mut self.amp_pending) });
            if events.caught {
                self.caught_at = now;
            }
            if events.spotted {
                self.spotted_at = now;
            }
            if events.angrier {
                self.said = Some((Said::Angrier, now));
            }
            // A mate called says more than the anger that came with him.
            if let Some(who) = events.called {
                self.said = Some((Said::Called(who), now));
            }
            if show.over {
                break;
            }
        }
    }
}

impl App {
    /// Backstage, on the home page.
    pub(super) fn open_backstage(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Backstage;
    }

    pub(super) fn backstage_page(&mut self, ui: &mut Ui, rect: Rect) {
        let playing = self.backstage.playing();
        // A show under way is the half-day of work's; once played, the next one gives way.
        if playing {
            self.count_round(HomeGame::Backstage);
        } else if self.backstage.ended.is_none()
            && let Some(p) = self.game_locked(HomeGame::Backstage)
        {
            self.fold_for_game(false);
            return self.locked_page(ui, rect, HomeGame::Backstage, &p);
        }
        // During a show the sidebar folds away, and unfolds after.
        self.fold_for_game(playing);
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        self.backstage.floor.poll();
        if !playing {
            self.backstage.floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.backstage_name, 40.0, theme.accent, 1.0);
        if !playing {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.home_game = false;
                return;
            }
        }

        // The maze, as large as the page lets it, with the board beside it when there is room.
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 110.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 70.0));
        let side_w = 240.0;
        let wide = !playing && body.width() >= 900.0;
        let room = if wide { Vec2::new(body.width() - side_w - 32.0, body.height()) } else { body.size() };
        let cell = (room.x / W as f32).min(room.y / H as f32).clamp(12.0, 44.0).floor();
        let size = Vec2::new(cell * W as f32, cell * H as f32);
        let block_w = if wide { size.x + 32.0 + side_w } else { size.x };
        let maze = Rect::from_min_size(Pos2::new(body.center().x - block_w / 2.0, body.min.y), size);

        // The keys: only when nothing else (a field of the sidebar...) has the keyboard.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let (dir, amp, start) = ui.input(|i| {
            let key = |k: Key| free && i.key_pressed(k);
            let dir = [(Key::ArrowLeft, Key::A, Dir::Left), (Key::ArrowRight, Key::D, Dir::Right), (Key::ArrowUp, Key::W, Dir::Up), (Key::ArrowDown, Key::S, Dir::Down)].into_iter().find(|&(a, b, _)| key(a) || key(b)).map(|(_, _, d)| d);
            (dir, key(Key::Space), key(Key::Space) || key(Key::Enter))
        });
        if dir.is_some() {
            self.backstage.dir = dir;
        }
        // A new show: away from the last one's end for a moment.
        if !playing && start && self.backstage.ended.is_none_or(|at| now - at > 0.6) && self.game_locked(HomeGame::Backstage).is_none() {
            let seed = super::game::random(&mut self.backstage.rng);
            let b = &mut self.backstage;
            b.show = Some(Show::new(seed));
            b.ended = None;
            b.record = false;
            b.acc = 0.0;
            b.amp_pending = false;
            b.dir = None;
            b.said = None;
        } else if playing {
            self.backstage.advance(ui.input(|i| i.stable_dt).min(0.1), amp, now);
            ui.ctx().request_repaint();
            if !self.backstage.playing() {
                self.backstage_over(now);
            }
        }

        self.paint_backstage(ui, maze, cell, &theme, now);

        // Under the maze: how the show ended, or how to start one; the keys below. Played enough this
        // half-day of work: when the next one can be played, instead.
        let locked = (!playing).then(|| self.game_locked(HomeGame::Backstage)).flatten().map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let (line, color) = match &self.backstage.show {
            Some(s) if s.over && self.backstage.record => (format!("{}  {}", t.backstage_over.replace("{s}", &s.score.to_string()).replace("{p}", &s.picked.to_string()), t.game_new_best), theme.accent),
            Some(s) if s.over => (t.backstage_over.replace("{s}", &s.score.to_string()).replace("{p}", &s.picked.to_string()), theme.text),
            Some(s) => (t.backstage_picks.replace("{n}", &(PER_RAGE - s.picked % PER_RAGE).to_string()), theme.text_muted),
            None => (t.backstage_start.to_owned(), theme.text),
        };
        let below = match (&self.backstage.show, locked) {
            (_, Some(locked)) => locked,
            (Some(s), None) if s.over => t.backstage_again.to_owned(),
            _ => t.backstage_help.to_owned(),
        };
        ui.painter().text(Pos2::new(maze.center().x, maze.max.y + 22.0), Align2::CENTER_CENTER, line, FontId::proportional(14.0), color);
        ui.painter().text(Pos2::new(maze.center().x, maze.max.y + 44.0), Align2::CENTER_CENTER, below, FontId::proportional(12.5), theme.text_muted);

        // The board, beside the maze, out of a show.
        if wide {
            let side = Rect::from_min_max(Pos2::new(maze.max.x + 32.0, maze.min.y), Pos2::new(maze.max.x + 32.0 + side_w, maze.max.y));
            let best = (self.config.settings.backstage_best > 0).then(|| t.backstage_best.replace("{s}", &self.config.settings.backstage_best.to_string()));
            if self.game_board(ui, side, &theme, self.backstage.floor.board(), best, |s| s.to_string()) {
                self.ask_pseudo();
            }
        }
    }

    /// The show just ended: kept when it beats the best, and sent to floor.
    fn backstage_over(&mut self, now: f64) {
        self.backstage.ended = Some(now);
        let Some(show) = &self.backstage.show else { return };
        let (score, rage, picks) = (show.score, show.rage, show.picked);
        if score == 0 {
            return;
        }
        self.backstage.record = score > self.config.settings.backstage_best;
        if self.backstage.record {
            self.config.settings.backstage_best = score;
            self.save_config();
        }
        // Online only with a pseudo.
        if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.backstage.floor.finished(crate::floor::GameRound { score, details: serde_json::json!({ "score": score, "rage": rage, "picks": picks }) });
            self.backstage.floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
        }
    }

    /// The maze from above: road cases for walls, the picks, the amps, the singer, Krog and his mates;
    /// along its top Krog's anger, the amps left, the score.
    fn paint_backstage(&self, ui: &Ui, maze: Rect, cell: f32, theme: &Theme, now: f64) {
        let painter = ui.painter_at(maze.expand(4.0));
        let b = &self.backstage;
        let floor = super::sidebar::lerp_color(theme.bg, Color32::BLACK, 0.25);
        painter.rect_filled(maze, 8.0, floor);
        let Some(show) = &b.show else {
            // Before the first show: Krog waiting in the dark.
            paint_chaser(&painter, maze.center(), cell * 2.2, Who::Krog, theme, now, false);
            return;
        };
        let at = |x: f32, y: f32| Pos2::new(maze.min.x + x * cell, maze.min.y + y * cell);
        let (case, edge) = if theme.dark { (Color32::from_rgb(58, 58, 68), Color32::from_rgb(96, 96, 110)) } else { (Color32::from_rgb(60, 60, 66), Color32::from_rgb(30, 30, 34)) };

        // The road cases: each wall cell a box, with its metal corners.
        for y in 0..H {
            for x in 0..W {
                if !show.wall((x, y)) {
                    continue;
                }
                let r = Rect::from_min_size(at(x as f32, y as f32), Vec2::splat(cell)).shrink(cell * 0.04);
                painter.rect_filled(r, cell * 0.12, case);
                painter.rect_stroke(r, cell * 0.12, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
                for corner in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
                    painter.circle_filled(corner + (r.center() - corner) * 0.18, cell * 0.06, edge);
                }
            }
        }

        // The picks, turning slowly.
        for (k, &(x, y)) in show.picks.iter().enumerate() {
            let c = at(x as f32 + 0.5, y as f32 + 0.5);
            let bob = ((now * 3.0 + k as f64).sin() as f32) * cell * 0.05;
            paint_pick(&painter, c + Vec2::new(0.0, bob), cell * 0.28, theme.ansi[3]);
        }

        // The amps dropped: blinking in their last second.
        for &((x, y), left) in &show.amps_down {
            let r = Rect::from_center_size(at(x as f32 + 0.5, y as f32 + 0.5), Vec2::splat(cell * 0.7));
            let alpha = if left < 1.0 { 0.4 + 0.6 * ((now * 12.0).sin() as f32).abs() } else { 1.0 };
            painter.rect_filled(r, cell * 0.08, Color32::from_rgb(30, 30, 32).gamma_multiply(alpha));
            painter.rect_stroke(r, cell * 0.08, Stroke::new(1.5, theme.text_muted.gamma_multiply(alpha)), egui::StrokeKind::Inside);
            painter.circle_stroke(r.center(), cell * 0.2, Stroke::new(1.5, theme.text_muted.gamma_multiply(alpha)));
            // How long it holds yet.
            let frac = left / AMP_SECS;
            painter.line_segment([r.left_bottom() + Vec2::new(2.0, -3.0), r.left_bottom() + Vec2::new(2.0 + (r.width() - 4.0) * frac, -3.0)], Stroke::new(2.0, theme.accent.gamma_multiply(alpha)));
        }

        // The singer, blinking a moment after a catch; them, red when hunting.
        let pos = |w: &Walker| {
            let (x, y) = w.pos();
            at(x, y)
        };
        paint_singer_top(&painter, pos(&show.player), cell, show.player.facing, theme);
        for c in &show.chasers {
            let size = if c.who == Who::Krog { 0.95 } else { 0.82 };
            paint_chaser(&painter, pos(&c.walker), cell * size, c.who, theme, now, c.hunting);
            if now - b.spotted_at < 0.8 && c.hunting && c.who != Who::Gnar {
                let p = ((now - b.spotted_at) / 0.8) as f32;
                painter.text(pos(&c.walker) + Vec2::new(0.0, -cell * (0.8 + 0.3 * p)), Align2::CENTER_CENTER, "!", FontId::proportional(cell * 0.8), theme.ansi[1].gamma_multiply(1.0 - p));
            }
        }
        if now - b.caught_at < 0.4 {
            painter.rect_filled(maze, 8.0, theme.ansi[1].gamma_multiply(0.25 * (1.0 - ((now - b.caught_at) / 0.4) as f32)));
        }

        // Along the top: Krog's anger, the amps, the score.
        let top = maze.min.y - 16.0;
        let ui_painter = ui.painter();
        let t = self.t();
        ui_painter.text(Pos2::new(maze.min.x, top), Align2::LEFT_CENTER, t.backstage_rage.replace("{r}", &show.rage.to_string()), FontId::proportional(14.0), if show.rage > 0 { theme.ansi[1] } else { theme.text });
        ui_painter.text(Pos2::new(maze.center().x, top), Align2::CENTER_CENTER, t.backstage_amps.replace("{n}", &show.amps.to_string()), FontId::proportional(13.0), theme.text_muted);
        ui_painter.text(Pos2::new(maze.max.x, top), Align2::RIGHT_CENTER, format!("{:06}", show.score), FontId::monospace(16.0), theme.text);
        // What Krog just did, big, fading.
        if let Some((said, at)) = b.said
            && now - at < 2.0
            && !show.over
        {
            let text = match said {
                Said::Angrier => t.backstage_angrier.to_owned(),
                Said::Called(who) => t.backstage_called.replace("{n}", who.name()),
            };
            paint_metal(ui_painter, maze.center(), Align2::CENTER_CENTER, &text, cell * 1.1, theme.ansi[1], ((2.0 - (now - at)) as f32).min(1.0));
            ui.ctx().request_repaint();
        }
    }
}

/// A guitar pick, point down.
fn paint_pick(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let mut points = Vec::new();
    for k in 0..=10 {
        let a = std::f32::consts::PI * (1.0 + k as f32 / 10.0);
        points.push(c + Vec2::new(a.cos() * r, a.sin() * r * 0.8 - r * 0.1));
    }
    points.push(c + Vec2::new(0.0, r * 1.15));
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.4))));
}

/// The singer from above: his long hair, his head, facing his way.
fn paint_singer_top(painter: &egui::Painter, c: Pos2, cell: f32, facing: Dir, theme: &Theme) {
    let (dx, dy) = facing.delta();
    let ahead = Vec2::new(dx as f32, dy as f32);
    // The hair, trailing behind.
    painter.circle_filled(c - ahead * cell * 0.12, cell * 0.36, Color32::from_rgb(52, 34, 22));
    painter.circle_filled(c, cell * 0.27, theme.accent);
    // His eyes, the way he looks.
    let side = Vec2::new(-ahead.y, ahead.x);
    for s in [-1.0, 1.0] {
        painter.circle_filled(c + ahead * cell * 0.13 + side * s * cell * 0.09, cell * 0.04, theme.bg);
    }
}

/// Krog and his mates from above: their heads, red-faced and bouncing when hunting, each his own hair
/// (Krog's spikes, Gnar's horns, Slug's crest), their brows frowning, their initial.
fn paint_chaser(painter: &egui::Painter, c: Pos2, size: f32, who: Who, theme: &Theme, now: f64, hunting: bool) {
    let base = match who {
        Who::Krog => theme.ansi[4],
        Who::Gnar => theme.ansi[2],
        Who::Slug => theme.ansi[5],
    };
    let face = if hunting { super::sidebar::lerp_color(base, theme.ansi[1], 0.7) } else { base };
    let bounce = if hunting { ((now * 14.0).sin() as f32).abs() * size * 0.05 } else { 0.0 };
    let c = c - Vec2::new(0.0, bounce);
    let hair = Color32::from_rgb(30, 30, 34);
    let spike = |a: f32, width: f32, length: f32| {
        let tip = c + Vec2::new(a.cos(), a.sin()) * size * length;
        let base = |da: f32| c + Vec2::new((a + da).cos(), (a + da).sin()) * size * 0.3;
        painter.add(egui::Shape::convex_polygon(vec![base(-width), tip, base(width)], hair, Stroke::NONE));
    };
    match who {
        Who::Krog => (0..5).for_each(|k| spike(std::f32::consts::PI * (1.15 + 0.175 * k as f32), 0.3, 0.55)),
        Who::Gnar => [1.2f32, 1.8].into_iter().for_each(|a| spike(std::f32::consts::PI * a, 0.22, 0.62)),
        Who::Slug => (0..4).for_each(|k| spike(std::f32::consts::PI * (1.38 + 0.08 * k as f32), 0.12, 0.6)),
    }
    painter.circle_filled(c, size * 0.36, face);
    painter.circle_stroke(c, size * 0.36, Stroke::new(1.5, Color32::BLACK.gamma_multiply(0.5)));
    // The eyes and the frown.
    for s in [-1.0f32, 1.0] {
        let eye = c + Vec2::new(s * size * 0.12, -size * 0.05);
        painter.circle_filled(eye, size * 0.05, Color32::WHITE);
        painter.line_segment([eye + Vec2::new(-s * size * 0.08, -size * 0.1), eye + Vec2::new(s * size * 0.06, -size * 0.06)], Stroke::new(size * 0.04, Color32::BLACK));
    }
    painter.text(c + Vec2::new(0.0, size * 0.17), Align2::CENTER_CENTER, &who.name()[..1], FontId::proportional(size * 0.24), Color32::WHITE);
}

/// Krog's head: the game's icon.
pub(super) fn paint_backstage_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    paint_chaser(painter, c, 17.0 * scale, Who::Krog, theme, 0.0, true);
}
