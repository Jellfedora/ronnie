//! Bug Hunt, in the games (its world in `crate::bughunt`): the bugs come down in ranks toward
//! production, the debugger at the bottom squashes them (← → or Q D to move, Space to fire, P to pause).
//! The score is kept when it beats the best.

use super::four::HomeGame;
use super::*;
use crate::bughunt::{self as world, Game, Input, Kind, Power};

/// The world moves in steps this long, whatever the frame rate.
const STEP: f32 = 1.0 / 120.0;

#[derive(Default)]
pub(super) struct BugHunt {
    game: Option<Game>,
    /// When the game ended (app time): a key starts the next one a moment after.
    ended: Option<f64>,
    record: bool,
    paused: bool,
    acc: f32,
    rng: u64,
}

impl BugHunt {
    fn playing(&self) -> bool {
        self.game.as_ref().is_some_and(|g| !g.over)
    }

    /// The home page is left: a game under way stops.
    pub(super) fn leave(&mut self) {
        if self.playing() {
            self.game = None;
            self.ended = None;
            self.paused = false;
        }
    }
}

impl App {
    pub(super) fn open_bughunt(&mut self) {
        self.open_game();
        self.home_which = HomeGame::BugHunt;
    }

    pub(super) fn bughunt_page(&mut self, ui: &mut Ui, rect: Rect) {
        let playing = self.bughunt.playing();
        // A game under way is the half-day of work's; once played, the next one gives way.
        if playing {
            self.count_round(HomeGame::BugHunt);
        } else if self.bughunt.ended.is_none()
            && let Some(p) = self.game_locked(HomeGame::BugHunt)
        {
            self.fold_for_game(false);
            return self.locked_page(ui, rect, HomeGame::BugHunt, &p);
        }
        self.fold_for_game(playing);
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        if !playing {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.home_game = false;
                return;
            }
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 40.0), Align2::CENTER_CENTER, t.bughunt_name, 34.0, theme.accent, 1.0);

        // The field, as big as the page lets it, the lines under it.
        let room = Vec2::new(rect.width() - 48.0, rect.height() - 70.0 - 64.0);
        let scale = (room.x / world::W).min(room.y / world::H).max(0.2);
        let size = Vec2::new(world::W, world::H) * scale;
        let field = Rect::from_min_size(Pos2::new(rect.center().x - size.x / 2.0, rect.min.y + 70.0), size);
        let resp = ui.interact(field, ui.id().with("bughunt-field"), Sense::click());

        // The keys: only when nothing else (a field of the sidebar...) has the keyboard.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let clicked = resp.clicked();
        let (input, start, pause) = ui.input(|i| {
            let down = |k: Key| free && i.key_down(k);
            let pressed = |k: Key| free && i.key_pressed(k);
            let input = Input { left: down(Key::ArrowLeft) || down(Key::Q) || down(Key::A), right: down(Key::ArrowRight) || down(Key::D), fire: down(Key::Space) || down(Key::ArrowUp) };
            (input, pressed(Key::Space) || pressed(Key::Enter) || clicked, pressed(Key::P) || pressed(Key::Escape))
        });
        if playing && pause {
            self.bughunt.paused = !self.bughunt.paused;
            self.bughunt.acc = 0.0;
        }
        // A new game: away from the last one's end for a moment (a key still held doesn't start one).
        if !playing && start && self.bughunt.ended.is_none_or(|at| now - at > 0.8) && self.game_locked(HomeGame::BugHunt).is_none() {
            let seed = super::game::random(&mut self.bughunt.rng);
            self.bughunt.game = Some(Game::new(seed));
            self.bughunt.ended = None;
            self.bughunt.record = false;
            self.bughunt.paused = false;
            self.bughunt.acc = 0.0;
        }

        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if !self.bughunt.paused
            && let Some(game) = self.bughunt.game.as_mut()
        {
            // Over, its sparks still fly a moment.
            self.bughunt.acc += dt;
            while self.bughunt.acc >= STEP {
                self.bughunt.acc -= STEP;
                game.step(STEP, input);
            }
            if game.over && self.bughunt.ended.is_none() {
                self.bughunt_over(now);
            }
        }
        // Always: before the first game too, the bugs waiting flap and the digits fall.
        ui.ctx().request_repaint();

        self.paint_field(ui, field, scale, &theme, now);

        // Under the field: how the game ended, or how to start one; the keys below.
        let locked = (!self.bughunt.playing()).then(|| self.game_locked(HomeGame::BugHunt)).flatten().map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let (line, color) = match &self.bughunt.game {
            Some(g) if g.over => {
                let text = t.bughunt_over.replace("{s}", &g.score.to_string()).replace("{w}", &g.wave.to_string());
                if self.bughunt.record { (format!("{text}  {}", t.game_new_best), theme.accent) } else { (text, theme.text) }
            }
            Some(_) => (String::new(), theme.text),
            None => (t.bughunt_start.to_owned(), theme.text),
        };
        let below = match (&self.bughunt.game, locked) {
            (_, Some(locked)) => locked,
            (Some(g), None) if g.over => t.bughunt_again.to_owned(),
            _ => t.bughunt_help.to_owned(),
        };
        ui.painter().text(Pos2::new(field.center().x, field.max.y + 20.0), Align2::CENTER_CENTER, line, FontId::proportional(14.0), color);
        ui.painter().text(Pos2::new(field.center().x, field.max.y + 42.0), Align2::CENTER_CENTER, below, FontId::proportional(12.5), theme.text_muted);
    }

    /// The game just ended: kept when it beats the best.
    fn bughunt_over(&mut self, now: f64) {
        self.bughunt.ended = Some(now);
        let Some(game) = &self.bughunt.game else { return };
        let score = game.score;
        self.bughunt.record = score > self.config.settings.bughunt_best;
        if self.bughunt.record {
            self.config.settings.bughunt_best = score;
            self.save_config();
        }
    }

    /// The field: bits of code drifting down at the back, the firewalls, the bugs, the shots, the
    /// debugger, the score and lives on top.
    fn paint_field(&self, ui: &Ui, field: Rect, scale: f32, theme: &Theme, now: f64) {
        let t = self.t();
        let painter = ui.painter_at(field);
        painter.rect_filled(field, 10.0 * scale.max(0.6), theme.chrome_bg);
        let at = |x: f32, y: f32| field.min + Vec2::new(x, y) * scale;

        // At the back, ones and zeros falling slowly.
        let mono = FontId::monospace(11.0 * scale.max(0.6));
        for k in 0..36u32 {
            let h = k.wrapping_mul(2_654_435_761);
            let x = (h % 1000) as f32 / 1000.0 * world::W;
            let speed = 12.0 + (h >> 10) as f32 % 30.0;
            let y = ((h >> 4) as f32 % world::H + now as f32 * speed) % world::H;
            painter.text(at(x, y), Align2::CENTER_CENTER, if h & 1 == 0 { "0" } else { "1" }, mono.clone(), theme.text_muted.gamma_multiply(0.18));
        }
        // Production, under the debugger.
        painter.line_segment([at(0.0, world::PLAYER_Y + 22.0), at(world::W, world::PLAYER_Y + 22.0)], Stroke::new(1.0, theme.ansi[1].gamma_multiply(0.35)));
        painter.text(at(world::W - 10.0, world::PLAYER_Y + 34.0), Align2::RIGHT_CENTER, "prod", FontId::monospace(11.0 * scale.max(0.6)), theme.ansi[1].gamma_multiply(0.5));

        let best = self.config.settings.bughunt_best;
        let Some(g) = &self.bughunt.game else {
            // Before the first game: a few bugs waiting, and the debugger.
            for (k, kind) in [Kind::Spider, Kind::Beetle, Kind::Moth].into_iter().enumerate() {
                for c in 0..5 {
                    let x = world::W / 2.0 + (c as f32 - 2.0) * world::CELL_W;
                    paint_bug(&painter, at(x, 200.0 + k as f32 * world::CELL_H), scale, kind, now as f32 * 2.0 + c as f32, theme);
                }
            }
            for k in 0..4 {
                let center = world::W * (k as f32 + 1.0) / 5.0;
                let r = Rect::from_min_size(at(center - world::WALL_COLS as f32 * world::WALL_CELL / 2.0, world::WALL_Y), Vec2::new(world::WALL_COLS as f32 * world::WALL_CELL, world::WALL_ROWS as f32 * world::WALL_CELL) * scale);
                painter.rect_filled(r, 4.0 * scale, theme.ansi[4].gamma_multiply(0.7));
            }
            paint_debugger(&painter, at(world::W / 2.0, world::PLAYER_Y), scale, theme.accent);
            if best > 0 {
                painter.text(at(world::W - 14.0, 14.0), Align2::RIGHT_TOP, format!("HI {best:06}"), FontId::monospace(14.0 * scale.max(0.7)), theme.text_muted);
            }
            return;
        };

        // The firewalls.
        let cell = Vec2::splat(world::WALL_CELL * scale);
        for wall in &g.walls {
            for (r, row) in wall.cells.iter().enumerate() {
                for (c, &on) in row.iter().enumerate() {
                    if on {
                        let r = Rect::from_min_size(at(wall.x + c as f32 * world::WALL_CELL, wall.y + r as f32 * world::WALL_CELL), cell).shrink(0.4);
                        painter.rect_filled(r, 1.0, theme.ansi[4].gamma_multiply(0.75));
                    }
                }
            }
        }
        // The bugs: their wings and legs on the beat.
        for b in g.bugs.iter().filter(|b| b.alive) {
            let phase = g.beat + (b.col + b.row) as f32 * 0.35;
            paint_bug(&painter, at(b.x, b.y), scale, b.kind, phase, theme);
            if b.dive.is_some_and(|d| !d.back) {
                // Diving: a trail.
                painter.circle_stroke(at(b.x, b.y), 19.0 * scale, Stroke::new(1.0, bug_color(b.kind, theme).gamma_multiply(0.35)));
            }
        }
        // The fixes falling.
        for d in &g.drops {
            paint_power(&painter, at(d.x, d.y), scale, d.power, theme, now);
        }
        // The patches, the exceptions.
        for s in &g.shots {
            painter.line_segment([at(s.x, s.y - 7.0), at(s.x, s.y + 7.0)], Stroke::new(3.0 * scale, theme.accent));
        }
        for b in &g.bolts {
            let w = 4.0;
            let points = vec![at(b.x - w, b.y - 9.0), at(b.x + w, b.y - 3.0), at(b.x - w, b.y + 3.0), at(b.x + w, b.y + 9.0)];
            painter.add(egui::Shape::line(points, Stroke::new(2.0 * scale, theme.ansi[1])));
        }
        // The sparks.
        for s in &g.sparks {
            let color = s.kind.map_or(theme.accent, |k| bug_color(k, theme)).gamma_multiply(s.life / world::SPARK_LIFE);
            painter.rect_filled(Rect::from_center_size(at(s.x, s.y), Vec2::splat(3.0 * scale)), 0.0, color);
        }
        // The debugger: blinking just hit, ringed with a shield.
        if !g.over {
            let blink = g.invincible > 0.0 && (now * 12.0) as i64 % 2 == 0;
            if !blink {
                paint_debugger(&painter, at(g.x, world::PLAYER_Y), scale, theme.accent);
            }
            if g.shield {
                let pulse = ((now * 4.0).sin() * 0.5 + 0.5) as f32;
                painter.circle_stroke(at(g.x, world::PLAYER_Y - 2.0), 30.0 * scale, Stroke::new(1.5 + pulse, theme.ansi[4].gamma_multiply(0.8)));
            }
        }

        // On top: the score and the best, the wave, the lives; the fixes under way.
        let hud = FontId::monospace(15.0 * scale.max(0.7));
        painter.text(at(14.0, 12.0), Align2::LEFT_TOP, format!("{:06}", g.score), hud.clone(), theme.text);
        let hi = best.max(g.score);
        if hi > 0 {
            painter.text(at(14.0, 34.0), Align2::LEFT_TOP, format!("HI {hi:06}"), FontId::monospace(11.5 * scale.max(0.7)), theme.text_muted);
        }
        painter.text(at(world::W / 2.0, 12.0), Align2::CENTER_TOP, t.bughunt_wave.replace("{n}", &g.wave.to_string()).to_uppercase(), hud, theme.text_muted);
        for k in 0..g.lives.saturating_sub(1) {
            paint_debugger(&painter, at(world::W - 24.0 - k as f32 * 34.0, 24.0), scale * 0.6, theme.accent.gamma_multiply(0.8));
        }
        let mut y = 58.0;
        for (left, power) in [(g.triple, Power::Triple), (g.rapid, Power::Rapid)] {
            if left > 0.0 {
                paint_power(&painter, at(world::W - 26.0, y), scale * 0.8, power, theme, now);
                let bar = Rect::from_min_size(at(world::W - 92.0, y - 2.0), Vec2::new(54.0 * left / 9.0, 4.0) * scale);
                painter.rect_filled(bar, 2.0, theme.accent.gamma_multiply(0.7));
                y += 26.0;
            }
        }

        // The wave's title, the pause, the end.
        let big = |text: &str, color: Color32| {
            paint_metal(&painter, at(world::W / 2.0, world::H * 0.45), Align2::CENTER_CENTER, text, 40.0 * scale.max(0.6), color, 1.0);
        };
        if g.over {
            big(t.bughunt_prod, theme.ansi[1]);
        } else if self.bughunt.paused {
            painter.rect_filled(field, 10.0, theme.bg.gamma_multiply(0.55));
            big(t.bughunt_paused, theme.text);
        } else if g.banner > 0.0 {
            big(&t.bughunt_wave.replace("{n}", &g.wave.to_string()), theme.accent.gamma_multiply((g.banner / 0.4).min(1.0)));
        }
    }
}

fn bug_color(kind: Kind, theme: &Theme) -> Color32 {
    match kind {
        Kind::Moth => theme.ansi[3],
        Kind::Beetle => theme.ansi[2],
        Kind::Spider => theme.ansi[5],
    }
}

/// A bug, facing down, its middle at `c`: a moth flapping its wings, a beetle in its shell walking, a
/// spider on its eight legs. `phase`: where it is in its beat.
fn paint_bug(painter: &egui::Painter, c: Pos2, scale: f32, kind: Kind, phase: f32, theme: &Theme) {
    let color = bug_color(kind, theme);
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * scale;
    let line = |a: (f32, f32), b: (f32, f32), w: f32| painter.line_segment([at(a.0, a.1), at(b.0, b.1)], Stroke::new(w * scale, color));
    let swing = (phase * std::f32::consts::TAU).sin();
    match kind {
        Kind::Moth => {
            let open = 0.65 + 0.35 * swing;
            for side in [-1.0, 1.0] {
                let wing = vec![at(side * 2.0, -6.0), at(side * 15.0 * open, -12.0), at(side * 16.0 * open, 1.0), at(side * 2.0, 3.0)];
                painter.add(egui::Shape::convex_polygon(wing, color.gamma_multiply(0.8), Stroke::NONE));
                let low = vec![at(side * 2.0, 2.0), at(side * 11.0 * open, 4.0), at(side * 7.0 * open, 11.0), at(side * 1.5, 7.0)];
                painter.add(egui::Shape::convex_polygon(low, color.gamma_multiply(0.6), Stroke::NONE));
                line((side * 1.0, 8.0), (side * 5.0, 14.0), 1.0);
            }
            painter.add(egui::Shape::convex_polygon(vec![at(0.0, -9.0), at(2.5, -2.0), at(2.0, 9.0), at(-2.0, 9.0), at(-2.5, -2.0)], color, Stroke::NONE));
        }
        Kind::Beetle => {
            for (k, y) in [-4.0, 1.0, 6.0].into_iter().enumerate() {
                let kick = if k % 2 == 0 { swing } else { -swing } * 2.5;
                for side in [-1.0, 1.0] {
                    line((side * 7.0, y), (side * 14.0, y + kick + 2.0), 1.5);
                }
            }
            painter.circle_filled(at(0.0, -1.0), 10.0 * scale, color);
            painter.line_segment([at(0.0, -10.0), at(0.0, 9.0)], Stroke::new(1.2 * scale, theme.chrome_bg));
            painter.circle_filled(at(0.0, 10.0), 4.5 * scale, color.gamma_multiply(0.75));
            for side in [-1.0, 1.0] {
                line((side * 2.0, 13.0), (side * 5.0, 16.0 + swing), 1.0);
                painter.circle_filled(at(side * 4.5, -3.0), 1.6 * scale, theme.chrome_bg.gamma_multiply(0.6));
            }
        }
        Kind::Spider => {
            for k in 0..4 {
                let y = -6.0 + k as f32 * 4.0;
                let kick = if k % 2 == 0 { swing } else { -swing } * 2.0;
                for side in [-1.0, 1.0] {
                    let knee = (side * 11.0, y - 5.0 + kick);
                    line((side * 3.0, y * 0.5), knee, 1.4);
                    line(knee, (side * 16.0, y + 5.0 + kick), 1.4);
                }
            }
            painter.circle_filled(at(0.0, -3.0), 7.0 * scale, color);
            painter.circle_filled(at(0.0, 6.0), 4.5 * scale, color);
            for side in [-1.0, 1.0] {
                painter.circle_filled(at(side * 1.8, 8.0), 1.3 * scale, theme.ansi[1]);
            }
        }
    }
}

/// The debugger, its middle at `c`: a base, a dome, its cannon up.
fn paint_debugger(painter: &egui::Painter, c: Pos2, scale: f32, color: Color32) {
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * scale;
    painter.rect_filled(Rect::from_min_max(at(-22.0, 2.0), at(22.0, 12.0)), 3.0 * scale, color);
    painter.add(egui::Shape::convex_polygon(vec![at(-14.0, 2.0), at(-9.0, -6.0), at(9.0, -6.0), at(14.0, 2.0)], color, Stroke::NONE));
    painter.rect_filled(Rect::from_min_max(at(-2.5, -16.0), at(2.5, -4.0)), 1.0, color);
    // The prompt on its side.
    painter.text(at(0.0, 7.0), Align2::CENTER_CENTER, ">_", FontId::monospace(8.0 * scale), Color32::from_black_alpha(150));
}

/// A fix, its middle at `c`: in a rounded capsule, three shots, two arrows, or a ring.
fn paint_power(painter: &egui::Painter, c: Pos2, scale: f32, power: Power, theme: &Theme, now: f64) {
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * scale;
    let glow = ((now * 5.0).sin() * 0.25 + 0.75) as f32;
    let color = match power {
        Power::Shield => theme.ansi[4],
        _ => theme.accent,
    };
    painter.rect(Rect::from_center_size(c, Vec2::new(22.0, 16.0) * scale), 5.0 * scale, color.gamma_multiply(0.25 * glow), Stroke::new(1.2 * scale, color.gamma_multiply(glow)), egui::StrokeKind::Inside);
    let stroke = Stroke::new(1.6 * scale, color);
    match power {
        Power::Triple => {
            for dx in [-5.0, 0.0, 5.0] {
                painter.line_segment([at(dx, -4.0), at(dx * 0.6, 4.0)], stroke);
            }
        }
        Power::Rapid => {
            for dy in [-1.5, 3.0] {
                painter.add(egui::Shape::line(vec![at(-4.0, dy + 2.0), at(0.0, dy - 2.0), at(4.0, dy + 2.0)], stroke));
            }
        }
        Power::Shield => {
            painter.circle_stroke(c, 4.5 * scale, stroke);
        }
    }
}

/// Bug Hunt's icon: a beetle seen from above.
pub(super) fn paint_bughunt_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let color = theme.accent;
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    for y in [-3.0, 0.5, 4.0] {
        for side in [-1.0, 1.0] {
            painter.line_segment([at(side * 3.0, y), at(side * 8.5, y + 1.5)], Stroke::new(1.0 * s, theme.text_muted));
        }
    }
    painter.circle_filled(at(0.0, 0.5), 5.5 * s, color);
    painter.line_segment([at(0.0, -5.0), at(0.0, 6.0)], Stroke::new(0.8 * s, theme.bg));
    painter.circle_filled(at(0.0, -6.5), 2.6 * s, color.gamma_multiply(0.8));
    for side in [-1.0, 1.0] {
        painter.line_segment([at(side * 1.0, -8.5), at(side * 3.5, -11.0)], Stroke::new(0.9 * s, theme.text_muted));
    }
}
