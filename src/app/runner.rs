//! Ronnie Run, in the games: a singer with long curly hair runs across the stage (see `crate::runner`),
//! jumping over the gear (Space or ↑, held: higher, or a click) and ducking under the lights (↓). He
//! throws the horns in the air. The meters run are the score, floor keeps the farthest.

use super::four::HomeGame;
use super::*;
use crate::runner::{Input, Kind, Run, PLAYER_X};

/// The stage's width shown, in its units (obstacles come in just past it).
const VIEW_W: f32 = 1000.0;
/// Its height shown, above the floor.
const VIEW_H: f32 = 230.0;
/// The world moves in steps this long, whatever the frame rate.
const STEP: f32 = 1.0 / 120.0;

/// The run under way, or the last one.
pub(super) struct Runner {
    run: Option<Run>,
    /// When the run ended (app time): a key starts the next one a moment after.
    ended: Option<f64>,
    /// The run ended beat the player's best.
    record: bool,
    /// Time not stepped yet.
    acc: f32,
    /// The jump key went down, not stepped yet: kept for the next step (above 120 Hz, a frame can
    /// have none).
    jump_pending: bool,
    rng: u64,
    floor: crate::floor::GameFloor,
}

impl Default for Runner {
    fn default() -> Self {
        Self { run: None, ended: None, record: false, acc: 0.0, jump_pending: false, rng: 0, floor: crate::floor::GameFloor::new("ronnie-run", false) }
    }
}

impl Runner {
    /// A run is on.
    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.over)
    }

    /// `dt` seconds more of the run under way, in steps of `STEP`. The jump key pressed counts once, at
    /// the next step, even when this frame has none.
    fn advance(&mut self, dt: f32, input: Input) {
        let Some(run) = self.run.as_mut().filter(|r| !r.over) else { return };
        self.acc += dt;
        self.jump_pending |= input.jump;
        while self.acc >= STEP {
            self.acc -= STEP;
            run.step(STEP, Input { jump: std::mem::take(&mut self.jump_pending), ..input });
            if run.over {
                break;
            }
        }
    }

    /// The home page is left: a run under way stops.
    pub(super) fn leave(&mut self) {
        if self.running() {
            self.run = None;
            self.ended = None;
        }
    }
}

/// "123 m".
pub(super) fn meters_text(m: u32) -> String {
    format!("{m} m")
}

impl App {
    /// Ronnie Run, on the home page.
    pub(super) fn open_runner(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Runner;
    }

    pub(super) fn runner_page(&mut self, ui: &mut Ui, rect: Rect) {
        let running = self.runner.running();
        // A run under way is the half-day of work's; once played, the next one gives way.
        if running {
            self.count_round(HomeGame::Runner);
        } else if self.runner.ended.is_none()
            && let Some(p) = self.game_locked(HomeGame::Runner)
        {
            self.fold_for_game(false);
            return self.locked_page(ui, rect, HomeGame::Runner, &p);
        }
        // During a run the sidebar folds away, and unfolds after.
        self.fold_for_game(running);
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        self.runner.floor.poll();
        if !running {
            self.runner.floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.runner_name, 40.0, theme.accent, 1.0);
        if !running {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.home_game = false;
                return;
            }
        }

        // The stage, as wide as the page lets it, the board under it.
        let width = (rect.width() - 48.0).min(1100.0);
        let scale = width / VIEW_W;
        let stage = Rect::from_min_size(Pos2::new(rect.center().x - width / 2.0, rect.min.y + 100.0), Vec2::new(width, (VIEW_H + 24.0) * scale));
        let resp = ui.interact(stage, ui.id().with("runner-stage"), Sense::click_and_drag());

        // The keys: only when nothing else (a field of the sidebar...) has the keyboard.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        // The pointer read before the keys: the response reads the input itself (not inside `ui.input`,
        // which holds it).
        let (pressed, held) = (resp.drag_started() || resp.clicked(), resp.dragged() || resp.is_pointer_button_down_on());
        let (mut input, start) = ui.input(|i| {
            let key = |k: Key| free && i.key_pressed(k);
            let down = |k: Key| free && i.key_down(k);
            let input = Input { jump: key(Key::Space) || key(Key::ArrowUp) || pressed, jump_held: down(Key::Space) || down(Key::ArrowUp) || held, duck: down(Key::ArrowDown) };
            (input, key(Key::Space) || key(Key::ArrowUp) || key(Key::Enter) || pressed)
        });
        // A new run: away from the last one's end for a moment (a key still held doesn't start one).
        if !running && start && self.runner.ended.is_none_or(|at| now - at > 0.5) && self.game_locked(HomeGame::Runner).is_none() {
            let seed = super::game::random(&mut self.runner.rng);
            self.runner.run = Some(Run::new(seed));
            self.runner.ended = None;
            self.runner.record = false;
            self.runner.acc = 0.0;
            self.runner.jump_pending = false;
            input.jump = false;
        }

        if self.runner.running() {
            self.runner.advance(ui.input(|i| i.stable_dt).min(0.1), input);
            ui.ctx().request_repaint();
            if !self.runner.running() {
                self.runner_over(now);
            }
        }

        self.paint_stage(ui, stage, scale, &theme, now);

        // Under the stage: how the run ended, or how to start one; the keys below. Played enough this
        // half-day of work: when the next one can be run, instead.
        let locked = (!running).then(|| self.game_locked(HomeGame::Runner)).flatten().map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let (line, color) = match &self.runner.run {
            Some(run) if run.over && self.runner.record => (format!("{}  {}", t.runner_over.replace("{m}", &run.meters().to_string()), t.game_new_best), theme.accent),
            Some(run) if run.over => (t.runner_over.replace("{m}", &run.meters().to_string()), theme.text),
            Some(_) => (String::new(), theme.text_muted),
            None => (t.runner_start.to_owned(), theme.text),
        };
        let below = match (&self.runner.run, locked) {
            (_, Some(locked)) => locked,
            (Some(run), None) if run.over => t.runner_again.to_owned(),
            _ => t.runner_help.to_owned(),
        };
        ui.painter().text(Pos2::new(stage.center().x, stage.max.y + 24.0), Align2::CENTER_CENTER, line, FontId::proportional(14.0), color);
        ui.painter().text(Pos2::new(stage.center().x, stage.max.y + 46.0), Align2::CENTER_CENTER, below, FontId::proportional(12.5), theme.text_muted);

        // The board, under the stage, out of a run.
        let side = Rect::from_min_max(Pos2::new(stage.center().x - 160.0, stage.max.y + 70.0), Pos2::new(stage.center().x + 160.0, rect.max.y - 12.0));
        if !running && side.height() > 60.0 {
            let best = (self.config.settings.runner_best > 0).then(|| t.runner_best.replace("{m}", &self.config.settings.runner_best.to_string()));
            if self.game_board(ui, side, &theme, self.runner.floor.board(), best, meters_text) {
                self.ask_pseudo();
            }
        }
    }

    /// The run just ended: kept when it beats the best, and sent to floor.
    fn runner_over(&mut self, now: f64) {
        self.runner.ended = Some(now);
        let Some(run) = &self.runner.run else { return };
        let (meters, jumps) = (run.meters(), run.jumps);
        if meters == 0 {
            return;
        }
        self.runner.record = meters > self.config.settings.runner_best;
        if self.runner.record {
            self.config.settings.runner_best = meters;
            self.save_config();
        }
        // Online only with a pseudo.
        if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.runner.floor.finished(crate::floor::GameRound { score: meters, details: serde_json::json!({ "meters": meters, "jumps": jumps }) });
            self.runner.floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
        }
    }

    /// The stage: its lights at the back, its floor going by, the gear, the singer, the meters.
    fn paint_stage(&self, ui: &Ui, stage: Rect, scale: f32, theme: &Theme, now: f64) {
        let painter = ui.painter_at(stage);
        painter.rect_filled(stage, 10.0, theme.chrome_bg);
        let ground = stage.max.y - 24.0 * scale;
        // A point of the stage (x from its left, y up from its floor) on screen.
        let at = |x: f32, y: f32| Pos2::new(stage.min.x + x * scale, ground - y * scale);
        let run = self.runner.run.as_ref();
        let distance = run.map_or(0.0, |r| r.distance);

        // At the back, beams of light sweeping slowly.
        for k in 0..4 {
            let x = 140.0 + k as f32 * 240.0;
            let sway = ((now * 0.6 + k as f64 * 1.7).sin() as f32) * 90.0;
            let color = [theme.accent, theme.ansi[5], theme.ansi[4], theme.ansi[3]][k].gamma_multiply(0.07);
            painter.add(egui::Shape::convex_polygon(vec![at(x - 6.0, VIEW_H + 20.0), at(x + 6.0, VIEW_H + 20.0), at(x + sway + 70.0, 0.0), at(x + sway - 70.0, 0.0)], color, Stroke::NONE));
        }
        // The floor: its line, its boards going by.
        painter.line_segment([Pos2::new(stage.min.x, ground), Pos2::new(stage.max.x, ground)], Stroke::new(2.0, theme.text_muted));
        let plank = 90.0;
        let offset = distance % plank;
        let mut x = -offset;
        while x < VIEW_W {
            painter.line_segment([at(x, 0.0), at(x - 18.0, -22.0)], Stroke::new(1.0, theme.tab_hover));
            x += plank;
        }

        if let Some(run) = run {
            for o in &run.obstacles {
                paint_obstacle(&painter, &at, scale, o.kind, o.x, theme);
            }
        }
        // The singer: standing at the start, running, ducking, in the air.
        let (y, ducking, airborne) = run.map_or((0.0, false, false), |r| (r.y, r.ducking, !r.grounded()));
        let stride = if run.is_some_and(|r| !r.over) { distance / 26.0 } else { 0.0 };
        paint_singer(&painter, &at, scale, PLAYER_X, y, ducking, airborne, stride, theme);

        // The meters, and the best.
        let meters = run.map_or(0, |r| r.meters());
        painter.text(Pos2::new(stage.max.x - 14.0, stage.min.y + 12.0), Align2::RIGHT_TOP, format!("{meters:05} m"), FontId::monospace(18.0), theme.text);
        let best = self.config.settings.runner_best;
        if best > 0 {
            painter.text(Pos2::new(stage.max.x - 14.0, stage.min.y + 36.0), Align2::RIGHT_TOP, format!("HI {best:05} m"), FontId::monospace(13.0), theme.text_muted);
        }
    }
}

/// The gear's colors: its body, its insides (grille, case), its cables. Lighter on dark themes, to stand
/// out from the stage.
fn gear(theme: &Theme) -> (Color32, Color32, Color32) {
    if theme.dark { (Color32::from_rgb(122, 122, 136), Color32::from_rgb(72, 72, 82), Color32::from_rgb(180, 180, 194)) } else { (Color32::from_rgb(40, 40, 46), Color32::from_rgb(28, 28, 30), Color32::from_rgb(20, 20, 22)) }
}

fn paint_obstacle(painter: &egui::Painter, at: &dyn Fn(f32, f32) -> Pos2, scale: f32, kind: Kind, x: f32, theme: &Theme) {
    let (w, h, bottom) = kind.size();
    let body = Rect::from_two_pos(at(x, bottom), at(x + w, bottom + h));
    let metal = Color32::from_rgb(170, 170, 178);
    let (body_color, inside, cable) = gear(theme);
    // An outline, for every theme.
    let outline = Stroke::new(1.5 * scale, theme.text.gamma_multiply(0.6));
    match kind {
        Kind::Amp => {
            painter.rect_filled(body, 2.0 * scale, body_color);
            painter.rect_stroke(body, 2.0 * scale, outline, egui::StrokeKind::Outside);
            // The grille, the knobs on top.
            let grille = Rect::from_two_pos(at(x + 4.0, bottom + 4.0), at(x + w - 4.0, bottom + h - 10.0));
            painter.rect_filled(grille, 1.0, inside);
            for k in 0..4 {
                painter.circle_filled(at(x + 8.0 + k as f32 * 8.0, bottom + h - 5.0), 1.6 * scale, metal);
            }
            painter.line_segment([at(x + 6.0, bottom + h - 14.0), at(x + 16.0, bottom + h - 14.0)], Stroke::new(1.5 * scale, theme.accent));
        }
        Kind::Case | Kind::Stack => {
            // One case, or two one on the other.
            let tiers = if kind == Kind::Stack { 2 } else { 1 };
            let h = h / tiers as f32;
            for tier in 0..tiers {
                let bottom = bottom + tier as f32 * h;
                let body = Rect::from_two_pos(at(x, bottom), at(x + w, bottom + h));
                painter.rect_filled(body, 1.0, inside);
                painter.rect_stroke(body, 1.0, outline, egui::StrokeKind::Outside);
                painter.rect_stroke(body, 1.0, Stroke::new(1.5 * scale, metal), egui::StrokeKind::Inside);
                // Its corners, its latch.
                for (cx, cy) in [(x, bottom), (x + w - 5.0, bottom), (x, bottom + h - 5.0), (x + w - 5.0, bottom + h - 5.0)] {
                    painter.rect_filled(Rect::from_two_pos(at(cx, cy), at(cx + 5.0, cy + 5.0)), 0.0, metal);
                }
                painter.rect_filled(Rect::from_two_pos(at(x + w / 2.0 - 4.0, bottom + h / 2.0 - 2.0), at(x + w / 2.0 + 4.0, bottom + h / 2.0 + 2.0)), 0.0, metal);
            }
        }
        Kind::Cable => {
            // Loops of cable, flat on the floor.
            for k in 0..4 {
                let c = at(x + 10.0 + k as f32 * 14.0, bottom + h / 2.0);
                painter.add(egui::Shape::ellipse_stroke(c, Vec2::new(11.0 * scale, h / 2.0 * scale), Stroke::new(2.5 * scale, cable)));
            }
            painter.line_segment([at(x, 1.0), at(x - 30.0, 1.0)], Stroke::new(2.5 * scale, cable));
        }
        Kind::Light => {
            // Hung from the truss above, its beam to the floor.
            let mid = x + w / 2.0;
            painter.line_segment([at(mid, bottom + h), at(mid, VIEW_H + 20.0)], Stroke::new(2.0 * scale, metal));
            painter.add(egui::Shape::convex_polygon(vec![at(x + 6.0, bottom), at(x + w - 6.0, bottom), at(x + w + 26.0, 0.0), at(x - 26.0, 0.0)], theme.ansi[3].gamma_multiply(0.18), Stroke::NONE));
            painter.rect_filled(body, 3.0 * scale, body_color);
            painter.rect_stroke(body, 3.0 * scale, outline, egui::StrokeKind::Outside);
            painter.rect_filled(Rect::from_two_pos(at(x + 6.0, bottom), at(x + w - 6.0, bottom + 5.0)), 2.0, theme.ansi[3]);
        }
    }
}

/// The singer, his feet at (x, y): long dark curls, black stage clothes, a studded belt. In the air he
/// throws the horns.
#[allow(clippy::too_many_arguments)]
fn paint_singer(painter: &egui::Painter, at: &dyn Fn(f32, f32) -> Pos2, scale: f32, x: f32, y: f32, ducking: bool, airborne: bool, stride: f32, theme: &Theme) {
    let hair = Color32::from_rgb(62, 40, 28);
    let skin = Color32::from_rgb(226, 182, 146);
    let clothes = if theme.dark { Color32::from_rgb(78, 78, 90) } else { Color32::from_rgb(34, 34, 40) };
    let stud = Color32::from_rgb(200, 200, 208);
    let block = |x0: f32, y0: f32, x1: f32, y1: f32, color: Color32| painter.rect_filled(Rect::from_two_pos(at(x0, y0), at(x1, y1)), 0.0, color);
    if ducking {
        // Bent forward, the hair falling in front.
        block(x + 2.0, y, x + 8.0, y + 10.0, clothes);
        block(x + 14.0, y, x + 20.0, y + 10.0, clothes);
        block(x, y + 8.0, x + 22.0, y + 20.0, clothes);
        block(x + 4.0, y + 8.0, x + 22.0, y + 10.0, stud);
        block(x + 18.0, y + 10.0, x + 30.0, y + 24.0, hair);
        block(x + 22.0, y + 12.0, x + 28.0, y + 20.0, skin);
        for k in 0..3 {
            painter.circle_filled(at(x + 20.0 + k as f32 * 4.0, y + 9.0), 2.2 * scale, hair);
        }
        return;
    }
    // Legs: in stride when running, together in the air.
    let swing = if airborne { 0.0 } else { (stride * std::f32::consts::TAU).sin() * 5.0 };
    block(x + 4.0 + swing, y, x + 10.0 + swing, y + 18.0, clothes);
    block(x + 12.0 - swing, y, x + 18.0 - swing, y + 18.0, clothes);
    // The body, the studded belt.
    block(x + 3.0, y + 16.0, x + 19.0, y + 34.0, clothes);
    block(x + 3.0, y + 17.0, x + 19.0, y + 19.0, Color32::from_rgb(20, 20, 22));
    for k in 0..4 {
        block(x + 4.5 + k as f32 * 4.0, y + 17.5, x + 6.0 + k as f32 * 4.0, y + 18.5, stud);
    }
    // The hair behind the head, down to the shoulders, curly.
    block(x + 3.0, y + 26.0, x + 17.0, y + 46.0, hair);
    for k in 0..4 {
        painter.circle_filled(at(x + 3.0 + k as f32 * 4.5, y + 26.5), 2.6 * scale, hair);
    }
    // The face, turned forward.
    block(x + 9.0, y + 33.0, x + 19.0, y + 43.0, skin);
    block(x + 15.0, y + 39.0, x + 17.0, y + 41.0, Color32::from_rgb(30, 22, 18));
    block(x + 7.0, y + 41.0, x + 19.0, y + 47.0, hair);
    // The arm: along the body running, the horns up in the air.
    if airborne {
        block(x + 16.0, y + 30.0, x + 20.0, y + 50.0, clothes);
        block(x + 15.5, y + 50.0, x + 20.5, y + 54.0, skin);
        // The index and the little finger up.
        block(x + 15.5, y + 54.0, x + 17.0, y + 59.0, skin);
        block(x + 19.0, y + 54.0, x + 20.5, y + 59.0, skin);
    } else {
        let arm = -swing * 0.6;
        block(x + 8.0 + arm, y + 20.0, x + 12.0 + arm, y + 32.0, clothes);
        block(x + 8.0 + arm, y + 17.0, x + 12.0 + arm, y + 20.0, skin);
    }
}

/// Ronnie Run's icon: a hand throwing the horns.
pub(super) fn paint_runner_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let skin = theme.accent;
    let s = scale;
    let block = |x0: f32, y0: f32, x1: f32, y1: f32| painter.rect_filled(Rect::from_min_max(c + Vec2::new(x0 * s, y0 * s), c + Vec2::new(x1 * s, y1 * s)), 1.0, skin);
    // The fist, the index and the little finger up, the thumb across.
    block(-5.0, -1.0, 5.0, 6.0);
    block(-5.0, -9.0, -2.5, -1.0);
    block(2.5, -9.0, 5.0, -1.0);
    block(-1.0, 1.5, 6.5, 3.5);
    painter.rect_filled(Rect::from_min_max(c + Vec2::new(-3.5 * s, 6.0 * s), c + Vec2::new(3.5 * s, 9.0 * s)), 1.0, theme.text_muted);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At 144 Hz a frame has no step now and then: a jump pressed in one is kept for the next step.
    #[test]
    fn a_jump_pressed_between_two_steps_is_not_lost() {
        let mut runner = Runner { run: Some(Run::new(7)), ..Runner::default() };
        let press = Input { jump: true, jump_held: true, duck: false };
        // The first frame at 144 Hz is shorter than a step.
        runner.advance(1.0 / 144.0, press);
        assert_eq!(runner.run.as_ref().unwrap().jumps, 0);
        runner.advance(1.0 / 144.0, Input { jump: false, ..press });
        let run = runner.run.as_ref().unwrap();
        assert_eq!(run.jumps, 1);
        assert!(run.y > 0.0);
        // Counted once: the frames after don't jump again.
        for _ in 0..200 {
            runner.advance(1.0 / 144.0, Input::default());
        }
        assert_eq!(runner.run.as_ref().unwrap().jumps, 1);
    }
}
