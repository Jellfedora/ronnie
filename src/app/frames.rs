//! The frames around an avatar, unlocked in the games (for now at Bug Hunt, by the wave reached):
//! chosen in the account window, kept on floor so that the others see them too, drawn around the
//! avatar wherever a player shows (the chat, the boards, who is online). Development builds have them
//! all, to try them.

use super::*;

/// A frame: its id on floor, the Bug Hunt wave that unlocks it.
pub(super) struct FrameDef {
    pub id: &'static str,
    pub wave: u32,
}

/// The frames, in the order they unlock: the first once the first boss is beaten.
pub(super) const FRAMES: [FrameDef; 3] = [FrameDef { id: "glitch", wave: 6 }, FrameDef { id: "swarm", wave: 11 }, FrameDef { id: "circuit", wave: 16 }];

/// Whether a frame is unlocked, by the highest Bug Hunt wave reached.
pub(super) fn unlocked(def: &FrameDef, best_wave: u32) -> bool {
    !crate::config::OFFICIAL || best_wave >= def.wave
}

/// A frame's name.
pub(super) fn frame_name(t: &Strings, id: &str) -> &'static str {
    let names: Vec<&'static str> = t.frame_names.split('|').collect();
    FRAMES.iter().position(|f| f.id == id).and_then(|k| names.get(k).copied()).unwrap_or_default()
}

/// The frame `id` around an avatar of radius `r` centered at `c` (nothing for none or unknown).
/// Animated: the window is drawn again while one shows.
pub(super) fn paint_frame(painter: &egui::Painter, c: Pos2, r: f32, id: Option<&str>, theme: &Theme) {
    let Some(id) = id else { return };
    let now = painter.ctx().input(|i| i.time);
    let w = (r * 0.13).clamp(1.3, 6.0);
    let ring = r + w * 0.9;
    let tau = std::f32::consts::TAU;
    match id {
        // The Heisenbug's: a red ring, never quite in place, two ghosts beside it, bits flicking off.
        "glitch" => {
            let jitter = ((now * 17.0).sin() * 0.5 + (now * 41.0).sin() * 0.5) as f32 * w * 0.8;
            painter.circle_stroke(c + Vec2::new(-w - jitter, 0.0), ring, Stroke::new(w * 0.8, theme.ansi[6].gamma_multiply(0.55)));
            painter.circle_stroke(c + Vec2::new(w + jitter, 0.0), ring, Stroke::new(w * 0.8, theme.ansi[5].gamma_multiply(0.55)));
            painter.circle_stroke(c, ring, Stroke::new(w, theme.ansi[1]));
            for k in 0..5 {
                let seed = (now * 6.0).floor() as u32 * 7 + k;
                let h = seed.wrapping_mul(2_654_435_761);
                if h.is_multiple_of(3) {
                    continue;
                }
                let a = (h % 360) as f32 / 360.0 * tau;
                let at = c + Vec2::angled(a) * (ring + w * 1.6);
                let size = Vec2::new(w * (1.0 + (h >> 9) as f32 % 3.0), w * 0.9);
                painter.rect_filled(Rect::from_center_size(at, size), 0.0, if h & 1 == 0 { theme.ansi[6] } else { theme.ansi[5] });
            }
        }
        // The swarm's: a green ring with little bugs walking round it.
        "swarm" => {
            painter.circle_stroke(c, ring, Stroke::new(w, theme.ansi[2].gamma_multiply(0.85)));
            let n = if r < 12.0 { 3 } else { 5 };
            for k in 0..n {
                let a = now as f32 * 0.7 + k as f32 / n as f32 * tau;
                let at = c + Vec2::angled(a) * ring;
                let along = Vec2::angled(a + tau / 4.0);
                let side = Vec2::angled(a);
                let s = w * 1.25;
                let step = ((now * 9.0 + k as f64).sin() as f32) * s * 0.5;
                for (dy, kick) in [(-0.6, step), (0.6, -step)] {
                    let base = at + along * dy * s;
                    painter.line_segment([base - side * s * 1.6 + along * kick, base + side * s * 1.6 - along * kick], Stroke::new(w * 0.45, theme.text_muted));
                }
                painter.circle_filled(at, s, theme.ansi[3]);
                painter.circle_filled(at + along * s * 1.1, s * 0.55, theme.ansi[3].gamma_multiply(0.8));
            }
        }
        // The circuit's: a golden double ring, traces out to pads, light running along them.
        "circuit" => {
            let gold = theme.ansi[3];
            painter.circle_stroke(c, ring, Stroke::new(w, gold));
            painter.circle_stroke(c, ring + w * 1.6, Stroke::new(w * 0.45, gold.gamma_multiply(0.6)));
            let n = 8;
            for k in 0..n {
                let a = k as f32 / n as f32 * tau + 0.2;
                let from = c + Vec2::angled(a) * ring;
                let bend = c + Vec2::angled(a) * (ring + w * 2.4);
                let to = bend + Vec2::angled(a + tau / 4.0) * w * 1.8;
                painter.line_segment([from, bend], Stroke::new(w * 0.45, gold.gamma_multiply(0.8)));
                painter.line_segment([bend, to], Stroke::new(w * 0.45, gold.gamma_multiply(0.8)));
                painter.circle_filled(to, w * 0.7, gold);
            }
            // Two pulses of light round the ring.
            for k in 0..2 {
                let a = now as f32 * 1.6 + k as f32 * tau / 2.0;
                for trail in 0..4 {
                    let at = c + Vec2::angled(a - trail as f32 * 0.09) * ring;
                    painter.circle_filled(at, w * (0.9 - trail as f32 * 0.18), Color32::WHITE.gamma_multiply(0.85 - trail as f32 * 0.2));
                }
            }
        }
        _ => return,
    }
    painter.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}

/// What draws others' faces: the avatars fetched, with the account that may fetch them.
pub(super) struct Faces<'a> {
    pub avatars: &'a crate::floor::Avatars,
    pub account: Option<&'a crate::config::FloorAccount>,
}

/// A player's small avatar, with their frame, at the start of a row of `ui` (`r`: its radius).
#[allow(clippy::too_many_arguments)]
pub(super) fn row_avatar(ui: &mut Ui, r: f32, name: &str, id: Option<&str>, avatar: Option<&str>, frame: Option<&str>, faces: &Faces, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(r * 2.0 + 8.0), Sense::hover());
    let face = id.and_then(|id| faces.avatars.get(ui.ctx(), faces.account, id, avatar));
    super::talk::paint_avatar(ui.painter(), rect.center(), r, name, false, face.as_ref(), frame, theme);
}

impl App {
    /// Speed Metal's board with each one's face (the player's line: their own avatar and frame).
    pub(super) fn faced(&self, mut rows: Vec<crate::floor::Entry>) -> Vec<crate::floor::Entry> {
        let account = self.config.settings.floor.as_ref();
        for row in &mut rows {
            if row.me && let Some(a) = account {
                row.id = Some(a.id.clone());
                row.avatar = a.avatar.clone();
                row.frame = a.frame.clone();
            }
            row.face = row.id.as_deref().and_then(|id| self.avatars.get(&self.ctx, account, id, row.avatar.as_deref())).map(crate::floor::Face);
        }
        rows
    }
}
