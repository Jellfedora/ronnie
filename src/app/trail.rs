//! The pointer's trail, unlocked at Ronnie Run (a run of `TRAIL_METERS`, all of it in development):
//! sparks left behind where the pointer goes, in the theme's colors, drifting down and fading. Switched
//! off in the settings.

use super::*;

/// The run that unlocks it.
pub(super) const TRAIL_METERS: u32 = 1000;
/// How long a spark lives, how many at most.
const LIFE: f32 = 0.7;
const MAX: usize = 220;

struct Spark {
    at: Pos2,
    v: Vec2,
    life: f32,
    size: f32,
    color: usize,
}

#[derive(Default)]
pub(super) struct Trail {
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
        if let (Some(at), Some(last)) = (at, self.last) {
            let way = at - last;
            self.carry += way.length();
            let n = ((self.carry / 5.0) as usize).min(8);
            self.carry = if n == 8 { 0.0 } else { self.carry % 5.0 };
            for k in 0..n {
                if self.sparks.len() >= MAX {
                    break;
                }
                let along = last + way * ((k as f32 + self.random()) / n as f32);
                let a = self.random() * std::f32::consts::TAU;
                let speed = 10.0 + self.random() * 30.0;
                let size = 1.2 + self.random() * 2.3;
                self.hue = (self.hue + 0.07) % 4.0;
                let color = self.hue as usize;
                let life = LIFE * (0.6 + self.random() * 0.4);
                self.sparks.push(Spark { at: along, v: Vec2::angled(a) * speed - way.normalized() * 15.0, life, size, color });
            }
        }
        self.last = at;
        for s in &mut self.sparks {
            s.at += s.v * dt;
            s.v.y += 60.0 * dt;
            s.v *= 1.0 - 2.0 * dt;
            s.life -= dt;
        }
        self.sparks.retain(|s| s.life > 0.0);
    }

    /// Sparks are still flying.
    pub(super) fn alive(&self) -> bool {
        !self.sparks.is_empty()
    }

    /// The sparks, in the theme's colors.
    pub(super) fn paint(&self, painter: &egui::Painter, theme: &Theme) {
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
}
