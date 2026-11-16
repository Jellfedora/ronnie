//! The pointer's trail, unlocked at Ronnie Run (a run of `TRAIL_METERS`, all of it in development):
//! sparks left behind where the pointer goes, in the theme's colors, drifting down and fading. Switched
//! off in the settings. Once Ronnie Corp is a small business, bank notes fluttering down in place of
//! the sparks.

use super::*;

/// The run that unlocks it.
pub(super) const TRAIL_METERS: u32 = 1000;
/// How long a spark lives, how many at most.
const LIFE: f32 = 0.7;
const BILL_LIFE: f32 = 1.4;
const MAX: usize = 220;

struct Spark {
    at: Pos2,
    v: Vec2,
    life: f32,
    size: f32,
    color: usize,
    /// A bank note's tilt, and how fast it turns as it flutters down.
    angle: f32,
    spin: f32,
}

#[derive(Default)]
pub(super) struct Trail {
    /// Bank notes in place of the sparks (Ronnie Corp's, `super::wallstreet::bills_unlocked`).
    pub(super) bills: bool,
    sparks: Vec<Spark>,
    last: Option<Pos2>,
    rng: u64,
    hue: f32,
    /// The way gone since the last spark (a slow pointer still leaves some, at any frame rate).
    carry: f32,
}

/// Whether it is unlocked.
pub(super) fn unlocked(runner_best: u32) -> bool {
    !crate::config::OFFICIAL || runner_best >= TRAIL_METERS
}

impl Trail {
    fn random(&mut self) -> f32 {
        (super::game::random(&mut self.rng) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// One frame: sparks along the way the pointer went, the others moved on; all drawn over the window.
    pub(super) fn frame(&mut self, ctx: &egui::Context, theme: &Theme) {
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        self.step(ctx.input(|i| i.pointer.hover_pos()), dt);
        if self.sparks.is_empty() {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("pointer-trail")));
        self.paint(&painter, theme);
        ctx.request_repaint();
    }

    /// `dt` seconds more, the pointer now at `at`: new sparks along its way, the others moved on.
    pub(super) fn step(&mut self, at: Option<Pos2>, dt: f32) {
        // The notes: fewer, bigger, slower to fall.
        let (gap, most, lives) = if self.bills { (22.0, 3, BILL_LIFE) } else { (5.0, 8, LIFE) };
        if let (Some(at), Some(last)) = (at, self.last) {
            let way = at - last;
            self.carry += way.length();
            let n = ((self.carry / gap) as usize).min(most);
            self.carry = if n == most { 0.0 } else { self.carry % gap };
            for k in 0..n {
                if self.sparks.len() >= MAX {
                    break;
                }
                let along = last + way * ((k as f32 + self.random()) / n as f32);
                let a = self.random() * std::f32::consts::TAU;
                let speed = if self.bills { 8.0 + self.random() * 14.0 } else { 10.0 + self.random() * 30.0 };
                let size = if self.bills { 0.8 + self.random() * 0.45 } else { 1.2 + self.random() * 2.3 };
                self.hue = (self.hue + 0.07) % 4.0;
                let color = self.hue as usize;
                let life = lives * (0.6 + self.random() * 0.4);
                let angle = (self.random() - 0.5) * 1.2;
                let spin = (self.random() - 0.5) * 5.0;
                self.sparks.push(Spark { at: along, v: Vec2::angled(a) * speed - way.normalized() * 15.0, life, size, color, angle, spin });
            }
        }
        self.last = at;
        let (fall, drag) = if self.bills { (26.0, 3.0) } else { (60.0, 2.0) };
        for s in &mut self.sparks {
            s.at += s.v * dt;
            s.v.y += fall * dt;
            s.v *= 1.0 - drag * dt;
            s.life -= dt;
            s.angle += s.spin * dt;
        }
        self.sparks.retain(|s| s.life > 0.0);
    }

    /// Sparks are still flying.
    pub(super) fn alive(&self) -> bool {
        !self.sparks.is_empty()
    }

    /// The sparks, in the theme's colors.
    pub(super) fn paint(&self, painter: &egui::Painter, theme: &Theme) {
        if self.bills {
            for s in &self.sparks {
                let k = (s.life / BILL_LIFE * 2.5).min(1.0);
                // Fluttering: the note seen edge-on now and then.
                let flip = (s.angle * 2.3).cos().abs().max(0.25);
                super::wallstreet::paint_bill(painter, s.at, 11.0 * s.size, s.angle, flip, k);
            }
            return;
        }
        let colors = [theme.accent, theme.ansi[5], theme.ansi[6], theme.ansi[3]];
        for s in &self.sparks {
            let k = s.life / LIFE;
            let color = colors[s.color % colors.len()];
            painter.circle_filled(s.at, s.size * (0.4 + 0.6 * k) * 2.2, color.gamma_multiply(0.12 * k));
            painter.circle_filled(s.at, s.size * (0.4 + 0.6 * k), color.gamma_multiply(0.9 * k));
            // A star's glint on the biggest ones.
            if s.size > 3.0 {
                let r = s.size * 2.2 * k;
                let stroke = Stroke::new(0.8, Color32::WHITE.gamma_multiply(0.6 * k));
                painter.line_segment([s.at - Vec2::new(r, 0.0), s.at + Vec2::new(r, 0.0)], stroke);
                painter.line_segment([s.at - Vec2::new(0.0, r), s.at + Vec2::new(0.0, r)], stroke);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pointer as slow as the preview's (1.5 points a frame at 60 fps) leaves sparks.
    #[test]
    fn a_slow_pointer_leaves_sparks() {
        let mut trail = Trail::default();
        for k in 0..60 {
            trail.step(Some(Pos2::new(100.0 + k as f32 * 1.5, 50.0)), 1.0 / 60.0);
        }
        assert!(!trail.sparks.is_empty());
    }

    /// The bank notes: fewer than the sparks, the same way gone.
    #[test]
    fn the_notes_are_fewer() {
        let (mut sparks, mut notes) = (Trail::default(), Trail { bills: true, ..Default::default() });
        for k in 0..30 {
            let at = Some(Pos2::new(100.0 + k as f32 * 6.0, 50.0));
            sparks.step(at, 1.0 / 60.0);
            notes.step(at, 1.0 / 60.0);
        }
        assert!(!notes.sparks.is_empty() && notes.sparks.len() * 3 < sparks.sparks.len());
    }
}
