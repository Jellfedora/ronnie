//! The Christmas theme's own touches: the pointer as a candy cane (snowflakes behind it, a burst of
//! stars at each click), strings of fairy lights under the bars and over the active tab, the focused
//! pane framed with holly, the name under the snow beside a bauble, and at the foot of the sidebar a
//! snowy village at night (chalets lit, smoke from the chimneys, the tree in its lights, a snowman,
//! snow falling), Santa's sleigh crossing the moon now and then. The guard's window says « Ho ho
//! ho ! »; picking the theme, a present opens. Outside the window: the sleigh along the foot of the
//! screen, and a snow shower now and then.
//!
//! Given to everyone from the 1st of December to the 6th of January (`crate::theme::christmas_season`),
//! and kept for good once had.

use super::*;
use std::f32::consts::TAU;

/// What the guard's window says instead of its title.
pub(super) const ALERT: &str = "Ho ho ho !";

pub(super) const RED: Color32 = Color32::from_rgb(0xd6, 0x2a, 0x36);
const RED_DARK: Color32 = Color32::from_rgb(0x8a, 0x14, 0x1e);
pub(super) const GREEN: Color32 = Color32::from_rgb(0x1f, 0x7a, 0x4a);
const GREEN_DARK: Color32 = Color32::from_rgb(0x0f, 0x45, 0x2a);
pub(super) const GOLD: Color32 = Color32::from_rgb(0xf2, 0xc1, 0x4e);
const SNOW: Color32 = Color32::from_rgb(0xf6, 0xf8, 0xff);
const SNOW_SHADE: Color32 = Color32::from_rgb(0xc4, 0xd2, 0xe8);
const NIGHT: Color32 = Color32::from_rgb(0x08, 0x10, 0x24);
const WINDOW: Color32 = Color32::from_rgb(0xff, 0xc8, 0x5a);
const WOOD: Color32 = Color32::from_rgb(0x5a, 0x34, 0x1c);
/// The fairy lights' colours.
const BULBS: [Color32; 5] = [Color32::from_rgb(0xff, 0x4a, 0x4a), Color32::from_rgb(0xff, 0xd3, 0x4f), Color32::from_rgb(0x4f, 0xd8, 0x7a), Color32::from_rgb(0x5a, 0xb4, 0xff), Color32::from_rgb(0xff, 0x8a, 0xe0)];

/// `v` turned by `a` (radians).
trait Turn {
    fn rot(self, a: f32) -> Vec2;
}

impl Turn for Vec2 {
    fn rot(self, a: f32) -> Vec2 {
        let (s, c) = a.sin_cos();
        Vec2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }
}

/// Deterministic numbers in [0, 1) (the same flakes at each frame).
fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65536.0
}

fn vgrad(painter: &egui::Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (p, c) in [(r.left_top(), top), (r.right_top(), top), (r.left_bottom(), bottom), (r.right_bottom(), bottom)] {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

/// A soft round glow at `c`, `r` wide.
fn glow(painter: &egui::Painter, c: Pos2, r: f32, color: Color32, strength: f32) {
    for k in 0..6 {
        let f = 1.0 - k as f32 / 6.0;
        painter.circle_filled(c, r * f, color.gamma_multiply(0.05 * strength));
    }
}

fn soft_ellipse(painter: &egui::Painter, c: Pos2, half: Vec2, color: Color32, strength: f32) {
    for k in 0..4 {
        let f = 1.0 - k as f32 / 4.0;
        let pts: Vec<Pos2> = (0..20).map(|i| {
            let a = i as f32 / 20.0 * TAU;
            c + Vec2::new(a.cos() * half.x * f, a.sin() * half.y * f)
        }).collect();
        painter.add(egui::Shape::convex_polygon(pts, color.gamma_multiply(0.25 * strength), Stroke::NONE));
    }
}

/// A five-pointed star at `c`, `r` its outer radius, turned by `turn`.
pub(super) fn star(painter: &egui::Painter, c: Pos2, r: f32, color: Color32, turn: f32) {
    let inner = r * 0.45;
    let pt = |k: i32, rr: f32| c + Vec2::angled(-TAU / 4.0 + turn + k as f32 * TAU / 10.0) * rr;
    let core: Vec<Pos2> = (0..5).map(|k| pt(k * 2 + 1, inner)).collect();
    painter.add(egui::Shape::convex_polygon(core, color, Stroke::NONE));
    for k in 0..5 {
        painter.add(egui::Shape::convex_polygon(vec![pt(k * 2 - 1, inner), pt(k * 2, r), pt(k * 2 + 1, inner)], color, Stroke::NONE));
    }
}

/// A snowflake at `c`, `r` wide: six arms, two twigs on each.
pub(super) fn snowflake(painter: &egui::Painter, c: Pos2, r: f32, color: Color32, turn: f32) {
    let w = (r * 0.16).max(0.7);
    for k in 0..6 {
        let d = Vec2::angled(turn + k as f32 * TAU / 6.0);
        painter.line_segment([c, c + d * r], Stroke::new(w, color));
        if r > 4.0 {
            let m = c + d * r * 0.55;
            for s in [-1.0f32, 1.0] {
                painter.line_segment([m, m + Vec2::angled(turn + k as f32 * TAU / 6.0 + s * 0.8) * r * 0.32], Stroke::new(w * 0.8, color));
            }
        }
    }
}

/// A sprig of holly at `c`, `s` big, turned by `turn`: two pointed leaves, three berries.
pub(super) fn holly(painter: &egui::Painter, c: Pos2, s: f32, turn: f32) {
    for side in [-1.0f32, 1.0] {
        let dir = Vec2::angled(turn + side * 0.55);
        let n = dir.rot90();
        // A leaf: a spine, its edge in points (three bites on each side), as convex pieces.
        let at = |t: f32, out: f32| c + dir * (s * t) + n * (s * out);
        let pts = [(0.0, 0.0), (0.25, 0.16), (0.5, 0.2), (0.75, 0.16), (1.0, 0.0), (0.75, -0.16), (0.5, -0.2), (0.25, -0.16)];
        let leaf: Vec<Pos2> = pts.iter().map(|&(t, o)| at(t, o)).collect();
        painter.add(egui::Shape::convex_polygon(leaf, GREEN, Stroke::new((s * 0.04).max(0.6), GREEN_DARK)));
        for &(t, o) in &[(0.25, 0.16), (0.5, 0.2), (0.75, 0.16), (0.25, -0.16), (0.5, -0.2), (0.75, -0.16)] {
            let tip = at(t + 0.06, o * 1.45);
            painter.add(egui::Shape::convex_polygon(vec![at(t - 0.04, o * 0.9), tip, at(t + 0.12, o * 0.85)], GREEN, Stroke::NONE));
        }
        painter.line_segment([at(0.05, 0.0), at(0.9, 0.0)], Stroke::new((s * 0.035).max(0.5), Color32::from_rgb(0x6c, 0xc0, 0x84)));
    }
    for (dx, dy) in [(-0.09, 0.02), (0.08, 0.04), (0.0, -0.08)] {
        let b = c + Vec2::new(dx, dy) * s;
        painter.circle_filled(b, s * 0.09, RED);
        painter.circle_filled(b + Vec2::new(-0.03, -0.03) * s, s * 0.03, Color32::from_white_alpha(170));
    }
}

/// Fairy lights along `rect` (under the bars, over the active tab): a dark wire sagging between hooks,
/// bulbs in turn red, gold, green, blue, pink, twinkling one after the other.
pub(super) fn garland(painter: &egui::Painter, rect: Rect) {
    let ctx = painter.ctx();
    let now = ctx.input(|i| i.time);
    let y = rect.center().y;
    let every = 16.0;
    let sag = (rect.height() * 0.5).clamp(1.0, 3.0);
    let n = ((rect.width() / every).floor() as i32).max(1);
    let wire = |x: f32| y - sag + sag * 2.0 * (((x - rect.min.x) / (every * 3.0) * std::f32::consts::PI).sin()).abs();
    let pts: Vec<Pos2> = (0..=(rect.width() / 4.0) as i32).map(|k| {
        let x = rect.min.x + k as f32 * 4.0;
        Pos2::new(x, wire(x))
    }).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_rgb(0x1c, 0x30, 0x24))));
    let seed = (rect.min.x * 0.37 + rect.min.y * 0.11) as i32;
    for k in 0..n {
        let x = rect.min.x + every / 2.0 + k as f32 * every;
        let p = Pos2::new(x, wire(x) + 2.6);
        let color = BULBS[((k + seed).rem_euclid(5)) as usize];
        // A chase: each bulb brightest in turn, all of them breathing.
        let phase = (now * 2.2 - k as f64 * 0.35).rem_euclid(TAU as f64) as f32;
        let lit = 0.45 + 0.55 * (phase.sin() * 0.5 + 0.5);
        painter.circle_filled(p, 6.0, color.gamma_multiply(0.10 * lit));
        painter.circle_filled(p, 3.4, color.gamma_multiply(0.18 * lit));
        painter.rect_filled(Rect::from_center_size(p - Vec2::new(0.0, 2.6), Vec2::new(2.2, 1.6)), 0.4, Color32::from_rgb(0x3a, 0x48, 0x3e));
        let bulb: Vec<Pos2> = (0..10).map(|i| {
            let a = i as f32 / 10.0 * TAU;
            p + Vec2::new(a.cos() * 1.7, a.sin() * 2.3)
        }).collect();
        painter.add(egui::Shape::convex_polygon(bulb, color.gamma_multiply(0.55 + 0.45 * lit), Stroke::NONE));
        painter.circle_filled(p + Vec2::new(-0.5, -0.8), 0.6, Color32::from_white_alpha((200.0 * lit) as u8));
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(60));
}

/// The focused pane framed: a soft warm glow inward, holly at each corner.
pub(super) fn frame(painter: &egui::Painter, r: Rect) {
    for (k, alpha) in [0.9, 0.4, 0.2, 0.1, 0.04].into_iter().enumerate() {
        let color = if k == 0 { GOLD } else { RED };
        painter.rect_stroke(r.shrink(1.0 + k as f32), 4.0, Stroke::new(1.0, color.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    }
    corners(painter, r.shrink(1.5));
}

/// Holly at each corner of `r`, the leaves along the edges.
pub(super) fn corners(painter: &egui::Painter, r: Rect) {
    for (c, a) in [(r.left_top(), TAU / 8.0), (r.right_top(), TAU * 3.0 / 8.0), (r.left_bottom(), -TAU / 8.0), (r.right_bottom(), -TAU * 3.0 / 8.0)] {
        holly(painter, c + Vec2::angled(a) * 4.0, 16.0, a);
    }
}

/// A bell, `h` high, its middle at `c` (the guard's window), a red bow on it.
pub(super) fn paint_bell(painter: &egui::Painter, c: Pos2, h: f32) {
    let p = |x: f32, y: f32| c + Vec2::new(x, y) * h;
    let tilt = (painter.ctx().input(|i| i.time) * 6.0).sin() as f32 * 0.04;
    let body: Vec<Pos2> = (0..=16).map(|k| {
        let t = k as f32 / 16.0;
        let a = std::f32::consts::PI * (1.0 + t);
        p(a.cos() * 0.26 + tilt, -0.12 + a.sin() * 0.3)
    }).chain([p(0.42 + tilt, 0.3), p(-0.42 + tilt, 0.3)]).collect();
    painter.add(egui::Shape::convex_polygon(body, GOLD, Stroke::new(h * 0.03, Color32::from_rgb(0xa8, 0x7a, 0x1a))));
    painter.rect_filled(Rect::from_min_max(p(-0.44 + tilt, 0.27), p(0.44 + tilt, 0.34)), h * 0.03, Color32::from_rgb(0xd8, 0xa0, 0x2a));
    painter.circle_filled(p(tilt * 2.0, 0.38), h * 0.07, Color32::from_rgb(0xa8, 0x7a, 0x1a));
    painter.circle_filled(p(-0.1 + tilt, -0.2), h * 0.05, Color32::from_white_alpha(160));
    holly(painter, p(0.0, -0.44), h * 0.3, -TAU / 4.0);
}

/// A bauble in `r`: a glossy red ball (its shade, a gold band with stars, a gleam), its cap and hook.
pub(super) fn bauble(painter: &egui::Painter, r: Rect) {
    let d = r.width().min(r.height() * 0.85);
    let c = Pos2::new(r.center().x, r.max.y - d / 2.0);
    let rad = d / 2.0;
    // The hook, the cap.
    let cap = Rect::from_center_size(c - Vec2::new(0.0, rad + rad * 0.06), Vec2::new(rad * 0.5, rad * 0.24));
    painter.circle_stroke(cap.center_top() - Vec2::new(0.0, rad * 0.12), rad * 0.12, Stroke::new((rad * 0.05).max(0.8), GOLD));
    // The ball: dark edge, the red, lighter toward the light.
    painter.circle_filled(c, rad, RED_DARK);
    for k in 0..6 {
        let f = 1.0 - k as f32 * 0.13;
        painter.circle_filled(c - Vec2::new(rad * 0.12, rad * 0.12) * (k as f32 / 6.0), rad * f, RED.lerp_to_gamma(Color32::from_rgb(0xff, 0x7a, 0x7a), k as f32 / 10.0));
    }
    // The gold band, its stars.
    let band_h = rad * 0.26;
    let clip = painter.with_clip_rect(Rect::from_center_size(c, Vec2::new(rad * 2.0, band_h)).intersect(painter.clip_rect()));
    clip.circle_filled(c, rad - 0.5, GOLD);
    clip.circle_filled(c, rad - 0.5, Color32::from_black_alpha(20));
    for k in -2..=2 {
        let x = c.x + k as f32 * rad * 0.36;
        star(&clip, Pos2::new(x, c.y), band_h * 0.36, RED_DARK, 0.0);
    }
    painter.rect_filled(cap, rad * 0.05, Color32::from_rgb(0xd8, 0xa0, 0x2a));
    painter.hline(cap.x_range(), cap.center().y, Stroke::new((rad * 0.03).max(0.5), Color32::from_rgb(0xa8, 0x7a, 0x1a)));
    // The gleam.
    let gleam: Vec<Pos2> = (0..12).map(|k| {
        let a = k as f32 / 12.0 * TAU;
        c + Vec2::new(-rad * 0.4, -rad * 0.42) + Vec2::new(a.cos() * rad * 0.2, a.sin() * rad * 0.11)
    }).collect();
    painter.add(egui::Shape::convex_polygon(gleam, Color32::from_white_alpha(120), Stroke::NONE));
    painter.circle_filled(c + Vec2::new(-rad * 0.5, -rad * 0.2), rad * 0.06, Color32::from_white_alpha(150));
}

/// Snow on top of `r` (a word, a roof): a white cap along its top, lumpy, drips here and there.
fn snow_cap(painter: &egui::Painter, r: Rect, h: f32, seed: u32) {
    let n = (r.width() / (h * 1.3)).ceil().max(2.0) as u32;
    for k in 0..=n {
        let x = r.min.x + r.width() * k as f32 / n as f32;
        let lump = h * (0.55 + noise(seed + k) * 0.5);
        painter.circle_filled(Pos2::new(x, r.min.y), lump, SNOW);
        if noise(seed + k + 50) > 0.75 {
            painter.circle_filled(Pos2::new(x + h * 0.3, r.min.y + lump * 1.1), h * 0.25, SNOW);
        }
    }
    painter.rect_filled(Rect::from_min_max(Pos2::new(r.min.x, r.min.y - h * 0.4), Pos2::new(r.max.x, r.min.y + h * 0.35)), h * 0.3, SNOW);
    painter.hline(r.x_range(), r.min.y + h * 0.45, Stroke::new(0.8, SNOW_SHADE.gamma_multiply(0.7)));
}

/// The sidebar's logo: a bauble, the name in snow white under a cap of snow, flakes around.
pub(super) fn logo(painter: &egui::Painter, rect: Rect) {
    let now = painter.ctx().input(|i| i.time);
    let field = Rect::from_center_size(rect.center(), Vec2::new(rect.width().min(200.0), rect.height()));
    for k in 0..10u32 {
        let x = field.min.x + noise(k * 5 + 1) * field.width();
        let fall = (now as f32 * (6.0 + noise(k * 5 + 2) * 6.0) + noise(k * 5 + 3) * field.height()) % field.height();
        let p = Pos2::new(x + (now as f32 + k as f32).sin() * 3.0, field.min.y + fall);
        snowflake(painter, p, 1.6 + noise(k * 5 + 4) * 1.6, SNOW.gamma_multiply(0.55), now as f32 * 0.5 + k as f32);
    }
    let font = FontId::proportional(28.0);
    let galley = painter.layout_no_wrap("Ronnie".to_owned(), font.clone(), SNOW);
    let h = (rect.height() - 6.0).clamp(30.0, 46.0);
    let ball = Vec2::new(h * 0.78, h);
    let gap = 10.0;
    let left = rect.center().x - (ball.x + gap + galley.size().x) / 2.0;
    bauble(painter, Rect::from_min_size(Pos2::new(left, rect.center().y - h / 2.0), ball));
    let at = Pos2::new(left + ball.x + gap, rect.center().y);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        painter.text(at + Vec2::new(dx, dy), Align2::LEFT_CENTER, "Ronnie", font.clone(), RED_DARK);
    }
    let r = painter.text(at, Align2::LEFT_CENTER, "Ronnie", font, SNOW);
    snow_cap(painter, Rect::from_min_max(Pos2::new(r.min.x + 2.0, r.min.y + r.height() * 0.3), Pos2::new(r.max.x - 2.0, r.max.y)), 2.4, 11);
    painter.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}

/// Where the village is in the sidebar's foot (its hover).
pub(super) fn village_rect(rect: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(rect.min.x + rect.width() * 0.08, rect.max.y - 74.0), Pos2::new(rect.min.x + rect.width() * 0.92, rect.max.y - 10.0))
}

/// A fir tree, its foot at `foot`, `h` high, snow on its branches.
fn fir(painter: &egui::Painter, foot: Pos2, h: f32, color: Color32, snowy: bool) {
    painter.rect_filled(Rect::from_min_max(foot - Vec2::new(h * 0.05, h * 0.12), foot + Vec2::new(h * 0.05, 0.0)), 0.0, WOOD);
    for k in 0..3 {
        let top = foot.y - h + k as f32 * h * 0.26;
        let bottom = foot.y - h * 0.1 - (2 - k) as f32 * h * 0.18;
        let half = h * (0.18 + k as f32 * 0.09);
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(foot.x, top), Pos2::new(foot.x + half, bottom), Pos2::new(foot.x - half, bottom)], color, Stroke::NONE));
        if snowy {
            painter.add(egui::Shape::convex_polygon(vec![Pos2::new(foot.x, top), Pos2::new(foot.x + half * 0.45, top + (bottom - top) * 0.45), Pos2::new(foot.x - half * 0.45, top + (bottom - top) * 0.45)], SNOW.gamma_multiply(0.85), Stroke::NONE));
            painter.hline(foot.x - half..=foot.x + half, bottom, Stroke::new((h * 0.03).max(0.8), SNOW.gamma_multiply(0.8)));
        }
    }
}

/// A chalet, its foot's middle at `foot`, `w` wide: the walls, a roof under snow, lit windows, a
/// chimney smoking.
fn chalet(painter: &egui::Painter, foot: Pos2, w: f32, tm: f32, seed: u32) -> Vec<Pos2> {
    let h = w * 0.62;
    let wall = Rect::from_min_max(foot - Vec2::new(w / 2.0, h), foot + Vec2::new(w / 2.0, 0.0));
    painter.rect_filled(wall, 0.0, Color32::from_rgb(0x4a, 0x2a, 0x1a));
    for k in 1..4 {
        let y = wall.min.y + wall.height() * k as f32 / 4.0;
        painter.hline(wall.x_range(), y, Stroke::new(0.6, Color32::from_rgb(0x30, 0x1a, 0x10)));
    }
    // The chimney, its smoke rising and drifting.
    let chim = Rect::from_min_size(Pos2::new(wall.max.x - w * 0.28, wall.min.y - h * 0.72), Vec2::new(w * 0.12, h * 0.5));
    painter.rect_filled(chim, 0.0, Color32::from_rgb(0x5a, 0x40, 0x3a));
    for k in 0..5 {
        let t = (tm * 0.35 + k as f32 / 5.0 + noise(seed)) % 1.0;
        let p = chim.center_top() + Vec2::new(t * w * 0.5 + (tm + k as f32).sin() * 2.0, -t * h * 1.6);
        painter.circle_filled(p, 1.5 + t * w * 0.12, Color32::from_rgb(0xc8, 0xcc, 0xd8).gamma_multiply(0.35 * (1.0 - t)));
    }
    // The roof, snow on it.
    let apex = Pos2::new(foot.x, wall.min.y - h * 0.62);
    let eave = w * 0.12;
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(wall.min.x - eave, wall.min.y), apex, Pos2::new(wall.max.x + eave, wall.min.y)], Color32::from_rgb(0x3a, 0x22, 0x18), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(wall.min.x - eave, wall.min.y + 1.0), apex - Vec2::new(0.0, 1.5), Pos2::new(wall.max.x + eave, wall.min.y + 1.0), Pos2::new(wall.max.x + eave - 2.0, wall.min.y + 3.0), apex + Vec2::new(0.0, 2.0), Pos2::new(wall.min.x - eave + 2.0, wall.min.y + 3.0)], SNOW, Stroke::NONE));
    // The windows, warm, flickering a little; a wreath on the door.
    let mut lit = Vec::new();
    for (dx, k) in [(-0.27f32, 0u32), (0.27, 1)] {
        let p = Pos2::new(foot.x + dx * w, wall.min.y + h * 0.42);
        let flicker = 0.8 + 0.2 * (tm * 2.0 + (seed + k) as f32).sin();
        glow(painter, p, w * 0.22, WINDOW, 0.6 * flicker);
        let r = Rect::from_center_size(p, Vec2::new(w * 0.18, h * 0.3));
        painter.rect_filled(r, 0.5, WINDOW.gamma_multiply(flicker));
        painter.vline(p.x, r.y_range(), Stroke::new(0.6, Color32::from_rgb(0x4a, 0x2a, 0x1a)));
        painter.hline(r.x_range(), p.y, Stroke::new(0.6, Color32::from_rgb(0x4a, 0x2a, 0x1a)));
        lit.push(p);
    }
    let door = Rect::from_min_max(Pos2::new(foot.x - w * 0.08, foot.y - h * 0.5), Pos2::new(foot.x + w * 0.08, foot.y));
    painter.rect_filled(door, 0.5, Color32::from_rgb(0x2a, 0x16, 0x0c));
    painter.circle_stroke(door.center() - Vec2::new(0.0, h * 0.08), w * 0.05, Stroke::new(w * 0.03, GREEN));
    lit
}

/// A snowman, his foot at `foot`, `h` high: three balls, coal eyes and buttons, a carrot, a red scarf
/// waving, a top hat.
fn snowman(painter: &egui::Painter, foot: Pos2, h: f32, tm: f32) {
    let r = [h * 0.22, h * 0.16, h * 0.12];
    let c0 = foot - Vec2::new(0.0, r[0]);
    let c1 = c0 - Vec2::new(0.0, r[0] + r[1] * 0.75);
    let c2 = c1 - Vec2::new(0.0, r[1] + r[2] * 0.7);
    for (c, rr) in [(c0, r[0]), (c1, r[1]), (c2, r[2])] {
        painter.circle_filled(c, rr, SNOW_SHADE);
        painter.circle_filled(c - Vec2::new(rr * 0.15, rr * 0.15), rr * 0.86, SNOW);
    }
    for k in 0..3 {
        painter.circle_filled(c1 + Vec2::new(0.0, (k as f32 - 1.0) * r[1] * 0.45), (h * 0.02).max(0.7), Color32::from_rgb(0x20, 0x20, 0x26));
    }
    for s in [-1.0f32, 1.0] {
        painter.circle_filled(c2 + Vec2::new(s * r[2] * 0.35, -r[2] * 0.2), (h * 0.018).max(0.6), Color32::from_rgb(0x20, 0x20, 0x26));
    }
    painter.add(egui::Shape::convex_polygon(vec![c2 + Vec2::new(0.0, -r[2] * 0.05), c2 + Vec2::new(r[2] * 1.1, r[2] * 0.08), c2 + Vec2::new(0.0, r[2] * 0.2)], Color32::from_rgb(0xff, 0x8a, 0x2a), Stroke::NONE));
    // The scarf, an end waving.
    let neck = c2 + Vec2::new(0.0, r[2] * 0.85);
    painter.rect_filled(Rect::from_center_size(neck, Vec2::new(r[2] * 2.0, h * 0.06)), h * 0.02, RED);
    let wave = (tm * 3.0).sin() * h * 0.04;
    painter.add(egui::Shape::convex_polygon(vec![neck + Vec2::new(-r[2] * 0.7, 0.0), neck + Vec2::new(-r[2] * 1.3 + wave, h * 0.16), neck + Vec2::new(-r[2] * 0.9 + wave, h * 0.18), neck + Vec2::new(-r[2] * 0.4, 0.0)], RED, Stroke::NONE));
    // The hat.
    let brim = c2 - Vec2::new(0.0, r[2] * 0.8);
    painter.rect_filled(Rect::from_center_size(brim, Vec2::new(r[2] * 1.8, h * 0.035)), 0.5, Color32::from_rgb(0x18, 0x18, 0x20));
    painter.rect_filled(Rect::from_min_max(brim + Vec2::new(-r[2] * 0.6, -r[2] * 1.1), brim + Vec2::new(r[2] * 0.6, 0.0)), 0.5, Color32::from_rgb(0x18, 0x18, 0x20));
    painter.rect_filled(Rect::from_min_max(brim + Vec2::new(-r[2] * 0.6, -r[2] * 0.4), brim + Vec2::new(r[2] * 0.6, -r[2] * 0.2)), 0.0, RED);
}

/// The Christmas tree, its foot at `foot`, `h` high: green tiers, snow on them, lights twinkling,
/// baubles, the star on top shining, presents at its foot.
fn tree(painter: &egui::Painter, foot: Pos2, h: f32, tm: f32) {
    painter.rect_filled(Rect::from_min_max(foot - Vec2::new(h * 0.05, h * 0.1), foot + Vec2::new(h * 0.05, 0.0)), 0.0, WOOD);
    let tiers = 4;
    let mut lights = Vec::new();
    for k in 0..tiers {
        let top = foot.y - h * 0.92 + k as f32 * h * 0.19;
        let bottom = foot.y - h * 0.08 - (tiers - 1 - k) as f32 * h * 0.16;
        let half = h * (0.14 + k as f32 * 0.08);
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(foot.x, top), Pos2::new(foot.x + half, bottom), Pos2::new(foot.x - half, bottom)], GREEN_DARK, Stroke::NONE));
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(foot.x, top), Pos2::new(foot.x + half, bottom), Pos2::new(foot.x + half * 0.2, bottom)], GREEN, Stroke::NONE));
        painter.hline(foot.x - half..=foot.x + half, bottom, Stroke::new((h * 0.02).max(0.8), SNOW.gamma_multiply(0.75)));
        for j in 0..(3 + k) {
            let t = (j as f32 + 0.5) / (3 + k) as f32;
            lights.push(Pos2::new(foot.x - half * 0.9 + half * 1.8 * t, bottom - 1.5 - (t * std::f32::consts::PI).sin() * h * 0.03));
        }
    }
    for (i, p) in lights.iter().enumerate() {
        let color = BULBS[i % BULBS.len()];
        let lit = 0.4 + 0.6 * ((tm * 2.5 + i as f32 * 1.7).sin() * 0.5 + 0.5);
        painter.circle_filled(*p, (h * 0.04).max(1.6), color.gamma_multiply(0.2 * lit));
        painter.circle_filled(*p, (h * 0.016).max(0.8), color.gamma_multiply(lit));
    }
    for (dx, dy, c) in [(-0.1f32, 0.55, RED), (0.12, 0.4, GOLD), (0.0, 0.72, Color32::from_rgb(0x5a, 0xb4, 0xff)), (-0.16, 0.8, GOLD), (0.18, 0.78, RED)] {
        let p = Pos2::new(foot.x + dx * h, foot.y - h * (1.0 - dy));
        painter.circle_filled(p, (h * 0.03).max(1.2), c);
    }
    // The star, shining.
    let top = Pos2::new(foot.x, foot.y - h * 0.95);
    let shine = 0.7 + 0.3 * (tm * 3.0).sin();
    glow(painter, top, h * 0.28, GOLD, shine);
    star(painter, top, h * 0.08, GOLD, (tm * 0.4).sin() * 0.15);
    // Presents.
    for (dx, w, c, ribbon) in [(-0.2f32, 0.12f32, RED, GOLD), (0.06, 0.1, Color32::from_rgb(0x3a, 0x8a, 0xd8), SNOW), (0.2, 0.09, GOLD, RED)] {
        let r = Rect::from_min_max(Pos2::new(foot.x + dx * h - w * h / 2.0, foot.y - w * h * 0.8), Pos2::new(foot.x + dx * h + w * h / 2.0, foot.y));
        painter.rect_filled(r, 0.5, c);
        painter.vline(r.center().x, r.y_range(), Stroke::new((h * 0.015).max(0.6), ribbon));
        painter.hline(r.x_range(), r.center().y, Stroke::new((h * 0.015).max(0.6), ribbon));
    }
}

/// A reindeer, its body's middle at `c`, `s` big, facing `dir`, galloping (`tm`); Rudolph's nose lit.
fn reindeer(painter: &egui::Painter, c: Pos2, s: f32, dir: f32, tm: f32, rudolph: bool) {
    let p = |x: f32, y: f32| c + Vec2::new(x * s * dir, y * s);
    let brown = Color32::from_rgb(0x8a, 0x5a, 0x34);
    let dark = Color32::from_rgb(0x5a, 0x38, 0x20);
    let gait = tm * 9.0;
    for (k, x) in [-0.32f32, -0.22, 0.24, 0.34].iter().enumerate() {
        let swing = (gait + k as f32 * 1.6).sin() * 0.5;
        let hip = p(*x, 0.1);
        let hoof = hip + Vec2::new(swing.sin() * dir * 0.3, 0.9).normalized() * s * 0.42;
        painter.line_segment([hip, hoof], Stroke::new(s * 0.07, if k % 2 == 0 { dark } else { brown }));
    }
    let body: Vec<Pos2> = (0..16).map(|k| {
        let a = k as f32 / 16.0 * TAU;
        p(a.cos() * 0.45, a.sin() * 0.18)
    }).collect();
    painter.add(egui::Shape::convex_polygon(body, brown, Stroke::NONE));
    painter.circle_filled(p(-0.45, -0.05), s * 0.07, SNOW);
    painter.add(egui::Shape::convex_polygon(vec![p(0.3, -0.08), p(0.5, -0.42), p(0.62, -0.36), p(0.45, 0.02)], brown, Stroke::NONE));
    let head = p(0.62, -0.42);
    painter.add(egui::Shape::convex_polygon((0..12).map(|k| {
        let a = k as f32 / 12.0 * TAU;
        head + Vec2::new(a.cos() * s * 0.15 * dir.abs(), a.sin() * s * 0.09) + Vec2::new(0.06 * s * dir, 0.02 * s)
    }).collect(), brown, Stroke::NONE));
    painter.circle_filled(head + Vec2::new(0.02 * s * dir, -0.03 * s), s * 0.025 + 0.4, Color32::from_rgb(0x20, 0x14, 0x0c));
    // The antlers.
    for (off, k) in [(-0.05f32, 0.8f32), (0.04, 1.0)] {
        let base = head + Vec2::new(off * s * dir, -0.07 * s);
        let tip = base + Vec2::new(-0.12 * s * dir, -0.3 * s) * k;
        painter.line_segment([base, tip], Stroke::new(s * 0.03, Color32::from_rgb(0xd8, 0xc0, 0x90)));
        for t in [0.4f32, 0.7] {
            let b = base + (tip - base) * t;
            painter.line_segment([b, b + Vec2::new(0.1 * s * dir, -0.06 * s) * k], Stroke::new(s * 0.025, Color32::from_rgb(0xd8, 0xc0, 0x90)));
        }
    }
    let nose = head + Vec2::new(0.2 * s * dir, 0.04 * s);
    if rudolph {
        glow(painter, nose, s * 0.35, Color32::from_rgb(0xff, 0x30, 0x30), 0.7 + 0.3 * (tm * 4.0).sin());
        painter.circle_filled(nose, s * 0.055, Color32::from_rgb(0xff, 0x30, 0x30));
    } else {
        painter.circle_filled(nose, s * 0.04, Color32::from_rgb(0x30, 0x1a, 0x10));
    }
}

/// Santa's sleigh and his reindeer, the sleigh's middle at `c`, `s` big (a reindeer's size), flying
/// `dir`-ward: two pairs ahead (Rudolph leading), the reins, the red sleigh on gold runners, Santa in
/// it waving, the sack of presents, a trail of gold sparkles behind.
pub(super) fn paint_sleigh(painter: &egui::Painter, c: Pos2, s: f32, dir: f32, tm: f32) {
    let bob = |k: f32| (tm * 4.0 + k).sin() * s * 0.06;
    // Sparkles behind.
    for k in 0..18u32 {
        let t = k as f32 / 18.0;
        let p = c + Vec2::new(-dir * s * (0.8 + t * 3.2), s * 0.25 + (tm * 3.0 + k as f32).sin() * s * 0.12 * t);
        let tw = 0.5 + 0.5 * (tm * 6.0 + k as f32 * 2.1).sin();
        painter.circle_filled(p, s * 0.06 * (1.0 - t) + 0.5, GOLD.gamma_multiply((1.0 - t) * tw));
    }
    let deer: [(f32, bool); 3] = [(2.0, false), (3.3, false), (4.6, true)];
    let harness = c + Vec2::new(dir * s * 0.5, -s * 0.05);
    for (k, &(x, rudolph)) in deer.iter().enumerate() {
        let at = c + Vec2::new(dir * s * x, -s * 0.1 + bob(k as f32));
        painter.line_segment([harness, at - Vec2::new(dir * s * 0.1, -s * 0.05)], Stroke::new((s * 0.025).max(0.6), Color32::from_rgb(0xb0, 0x30, 0x30)));
        reindeer(painter, at, s * 0.9, dir, tm + k as f32 * 0.4, rudolph);
        // A collar with a little bell.
        painter.circle_filled(at + Vec2::new(dir * s * 0.38, -s * 0.12), s * 0.045, GOLD);
    }
    let sb = bob(9.0);
    let sled = |x: f32, y: f32| c + Vec2::new(x * s * dir, y * s + sb);
    // The runners, curling up at the front.
    painter.add(egui::Shape::line((0..=10).map(|k| {
        let t = k as f32 / 10.0;
        sled(-0.7 + t * 1.3, 0.42)
    }).chain((0..=8).map(|k| {
        let a = k as f32 / 8.0 * std::f32::consts::PI * 1.2;
        sled(0.6, 0.27) + Vec2::new(a.sin() * s * 0.15 * dir, a.cos() * s * 0.15)
    })).collect(), Stroke::new((s * 0.05).max(0.8), GOLD)));
    for x in [-0.4f32, 0.3] {
        painter.line_segment([sled(x, 0.42), sled(x, 0.25)], Stroke::new((s * 0.04).max(0.6), GOLD));
    }
    // The sack, Santa, the sleigh's body in front of them.
    painter.circle_filled(sled(-0.42, -0.12), s * 0.3, Color32::from_rgb(0x8a, 0x6a, 0x3a));
    painter.rect_filled(Rect::from_center_size(sled(-0.5, -0.4), Vec2::new(s * 0.2, s * 0.18)), 1.0, RED);
    painter.rect_filled(Rect::from_center_size(sled(-0.28, -0.44), Vec2::new(s * 0.16, s * 0.22)), 1.0, Color32::from_rgb(0x3a, 0x8a, 0xd8));
    let santa = sled(0.08, -0.32);
    painter.circle_filled(santa, s * 0.22, RED);
    painter.circle_filled(santa - Vec2::new(0.0, s * 0.26), s * 0.13, Color32::from_rgb(0xf4, 0xc8, 0xa8));
    painter.circle_filled(santa - Vec2::new(-0.02 * s * dir, s * 0.18), s * 0.12, SNOW);
    painter.add(egui::Shape::convex_polygon(vec![santa + Vec2::new(-s * 0.14, -s * 0.33), santa + Vec2::new(s * 0.14, -s * 0.33), santa + Vec2::new(-s * 0.2 * dir, -s * 0.6)], RED, Stroke::NONE));
    painter.circle_filled(santa + Vec2::new(-s * 0.2 * dir, -s * 0.6), s * 0.05, SNOW);
    painter.rect_filled(Rect::from_center_size(santa - Vec2::new(0.0, s * 0.33), Vec2::new(s * 0.3, s * 0.06)), 1.0, SNOW);
    // His arm waving.
    let wave = (tm * 5.0).sin() * 0.4;
    let shoulder = santa + Vec2::new(-s * 0.12 * dir, -s * 0.08);
    let hand = shoulder + Vec2::new(-(0.1 + wave * 0.1) * s * dir, -s * 0.3);
    painter.line_segment([shoulder, hand], Stroke::new(s * 0.08, RED));
    painter.circle_filled(hand, s * 0.05, SNOW);
    let body = [sled(-0.7, 0.25), sled(0.55, 0.25), sled(0.62, -0.05), sled(0.4, -0.15), sled(-0.7, -0.1)];
    painter.add(egui::Shape::convex_polygon(body.to_vec(), RED, Stroke::new((s * 0.03).max(0.6), RED_DARK)));
    painter.line_segment([sled(-0.7, -0.1), sled(0.4, -0.15)], Stroke::new((s * 0.04).max(0.7), GOLD));
    painter.circle_filled(sled(-0.1, 0.07), s * 0.06, GOLD);
}

/// The village at night at the foot of `rect` (the sidebar's panel), painted in layers: the sky, stars,
/// the moon, the northern lights waving now and then, hills under the snow, firs, the chalets (lit, their
/// chimneys smoking), the great tree in its lights, the snowman, snow falling all along; now and then a
/// shooting star, and Santa's sleigh crossing the moon. When to paint again (seconds).
pub(super) fn scene(painter: &egui::Painter, rect: Rect, now: f64) -> f32 {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let tm = now as f32;
    let (w, x0) = (rect.width(), rect.min.x);
    let ground = rect.max.y - 16.0;
    vgrad(&painter, Rect::from_min_max(rect.min, Pos2::new(rect.max.x, ground)), NIGHT.gamma_multiply(0.0), NIGHT);
    vgrad(&painter, Rect::from_min_max(Pos2::new(rect.min.x, ground - 60.0), Pos2::new(rect.max.x, ground)), Color32::TRANSPARENT, Color32::from_rgb(0x1a, 0x3a, 0x5a).gamma_multiply(0.5));
    // The northern lights, waving, now and then.
    let aurora = ((tm / 40.0 * TAU).sin() * 0.5 + 0.5).powi(2);
    if aurora > 0.05 {
        for band in 0..2 {
            for k in 0..40 {
                let t = k as f32 / 40.0;
                let x = x0 + w * t;
                let y = rect.min.y + rect.height() * (0.18 + band as f32 * 0.08) + (t * 6.0 + tm * 0.4 + band as f32).sin() * 8.0;
                let color = if band == 0 { Color32::from_rgb(0x4f, 0xe8, 0xa0) } else { Color32::from_rgb(0x8a, 0x6a, 0xff) };
                painter.vline(x, y..=y + 18.0, Stroke::new(w / 40.0 + 0.5, color.gamma_multiply(0.07 * aurora * (1.0 - (t - 0.5).abs()))));
            }
        }
    }
    for i in 0..30u32 {
        let p = Pos2::new(x0 + noise(i * 3 + 1) * w, rect.min.y + noise(i * 3 + 2) * (ground - rect.min.y - 40.0));
        let tw = 0.6 + 0.4 * (tm * (0.7 + noise(i) * 2.0) + noise(i + 9) * TAU).sin();
        painter.circle_filled(p, if noise(i + 5) > 0.9 { 1.2 } else { 0.7 }, SNOW.gamma_multiply(0.6 * tw));
    }
    let moon = Pos2::new(x0 + w * 0.78, rect.min.y + rect.height() * 0.3);
    let moonlight = Color32::from_rgb(0xf4, 0xf0, 0xe0);
    glow(&painter, moon, 34.0, moonlight, 0.5);
    painter.circle_filled(moon, 9.0, moonlight);
    painter.circle_filled(moon + Vec2::new(3.0, -2.0), 8.0, Color32::from_rgb(0xe4, 0xe0, 0xd0));
    // A shooting star.
    let shoot = (now + 6.0) % 19.0;
    if shoot < 0.9 {
        let k = (shoot / 0.9) as f32;
        let from = Pos2::new(x0 + w * 0.1, rect.min.y + rect.height() * 0.1);
        let head = from + Vec2::new(w * 0.5, rect.height() * 0.2) * k;
        let fade = (1.0 - k).min(k * 5.0);
        painter.line_segment([head - Vec2::new(24.0, 9.5), head], Stroke::new(1.2, Color32::WHITE.gamma_multiply(0.7 * fade)));
    }
    // The sleigh across the moon, every 41 s.
    let ride = (now + 12.0) % 41.0;
    let riding = ride < 6.0;
    if riding {
        let p = (ride / 6.0) as f32;
        let dir = if ((now + 12.0) / 41.0) as i64 % 2 == 0 { 1.0 } else { -1.0 };
        let (from, to) = (x0 - 60.0, rect.max.x + 60.0);
        let x = if dir > 0.0 { from + (to - from) * p } else { to - (to - from) * p };
        paint_sleigh(&painter, Pos2::new(x, moon.y + 4.0 - (p - 0.5).powi(2) * 30.0), 9.0, dir, tm);
    }
    // The hills under the snow, the firs on them.
    let far: Vec<Pos2> = (0..=24).map(|k| { let t = k as f32 / 24.0; Pos2::new(x0 + w * t, ground - 30.0 - 12.0 * (t * 5.0 + 1.0).sin().abs()) }).collect();
    hill(&painter, &far, ground, Color32::from_rgb(0xa8, 0xb8, 0xd4));
    for k in 0..9u32 {
        let x = x0 + w * (0.04 + noise(k + 70) * 0.92);
        let y = ground - 28.0 - noise(k + 80) * 6.0;
        fir(&painter, Pos2::new(x, y), 14.0 + noise(k + 90) * 8.0, Color32::from_rgb(0x2a, 0x4a, 0x48), true);
    }
    let near: Vec<Pos2> = (0..=24).map(|k| { let t = k as f32 / 24.0; Pos2::new(x0 + w * t, ground - 8.0 - 8.0 * (t * 4.0 + 2.0).sin().abs()) }).collect();
    hill(&painter, &near, rect.max.y, Color32::from_rgb(0xdc, 0xe6, 0xf6));
    // The chalets, the tree, the snowman.
    let mut lit = Vec::new();
    lit.extend(chalet(&painter, Pos2::new(x0 + w * 0.2, ground - 4.0), 30.0, tm, 3));
    lit.extend(chalet(&painter, Pos2::new(x0 + w * 0.42, ground - 9.0), 22.0, tm, 7));
    lit.extend(chalet(&painter, Pos2::new(x0 + w * 0.82, ground - 5.0), 26.0, tm, 11));
    tree(&painter, Pos2::new(x0 + w * 0.62, ground - 2.0), 58.0, tm);
    snowman(&painter, Pos2::new(x0 + w * 0.05 + 12.0, ground + 2.0), 30.0, tm);
    painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, ground), rect.max), 0.0, Color32::from_rgb(0xe6, 0xee, 0xfa));
    // The windows' light on the snow.
    for p in &lit {
        soft_ellipse(&painter, Pos2::new(p.x, ground + 3.0), Vec2::new(10.0, 2.5), WINDOW, 0.25);
    }
    // Snow falling all along, in two depths.
    for k in 0..46u32 {
        let near = k % 3 == 0;
        let speed = if near { 16.0 } else { 9.0 } + noise(k + 400) * 6.0;
        let y = rect.min.y + ((tm * speed + noise(k + 410) * rect.height()) % rect.height());
        let x = x0 + noise(k + 420) * w + (tm * 0.8 + k as f32).sin() * 5.0;
        let r = if near { 1.5 } else { 0.9 };
        painter.circle_filled(Pos2::new(x, y), r, SNOW.gamma_multiply(if near { 0.9 } else { 0.55 }));
    }
    if riding || shoot < 0.9 { 1.0 / 40.0 } else { 1.0 / 25.0 }
}

/// A hill along `pts` down to `bottom`, snow white, its crest lit.
fn hill(painter: &egui::Painter, pts: &[Pos2], bottom: f32, color: Color32) {
    // One mesh (no seams between the pieces).
    let mut mesh = egui::Mesh::default();
    for p in pts {
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(Pos2::new(p.x, bottom), color);
    }
    for k in 0..pts.len().saturating_sub(1) as u32 {
        let i = k * 2;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    painter.add(mesh);
    painter.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.7))));
}

/// The candy cane's flakes, kept from frame to frame.
struct Flake {
    at: Pos2,
    v: Vec2,
    life: f32,
    size: f32,
    star: bool,
    color: Color32,
    spin: f32,
}

/// The candy cane for a pointer: snowflakes where it went, a burst of stars at a click, now and then
/// a word rising.
#[derive(Default)]
pub(super) struct Cane {
    flakes: Vec<Flake>,
    last: Option<Pos2>,
    rng: u64,
    cast: Option<(Pos2, f64)>,
    word: Option<(&'static str, Pos2, f64)>,
}

/// The words that may rise from a click.
const WORDS: [&str; 10] = ["Ho ho ho !", "Joyeux Noël !", "Vin chaud ?", "Encore une bûche ?", "Merry deploy !", "Jingle bugs !", "Pas de prod le 24 !", "Un chocolat ?", "Vive le vent !", "Bonnes fêtes !"];

impl Cane {
    fn random(&mut self) -> f32 {
        (super::game::random(&mut self.rng) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// One frame: the pointer as a candy cane (for the plain arrow only), flakes where it went, a burst
    /// at a click.
    pub(super) fn frame(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let at = ctx.input(|i| i.pointer.hover_pos());
        let cane = at.filter(|&p| super::plain_pointer(ctx, p));
        self.step(cane, ctx.input(|i| i.pointer.any_pressed()), now, dt);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("cane")));
        if self.paint(&painter, cane, now) {
            ctx.request_repaint();
        }
        if cane.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
    }

    /// `dt` seconds more, the cane's tip at `cane` (None: not shown), `pressed` a click.
    pub(super) fn step(&mut self, cane: Option<Pos2>, pressed: bool, now: f64, dt: f32) {
        if let Some(tip) = cane {
            let moved = self.last.map_or(0.0, |l| l.distance(tip));
            let n = ((moved / 9.0) as usize).min(4) + usize::from(self.random() < 0.08);
            for _ in 0..n {
                let (a, s) = (self.random() * TAU, 6.0 + self.random() * 16.0);
                let size = 1.6 + self.random() * 2.2;
                let spin = self.random() * TAU;
                self.flakes.push(Flake { at: tip, v: Vec2::angled(a) * s, life: 1.0, size, star: false, color: SNOW, spin });
            }
            if pressed {
                self.cast = Some((tip, now));
                if self.random() < 1.0 / 3.0 {
                    let k = (self.random() * WORDS.len() as f32) as usize % WORDS.len();
                    self.word = Some((WORDS[k], tip, now));
                }
                for k in 0..16 {
                    let a = k as f32 / 16.0 * TAU + self.random() * 0.3;
                    let s = 60.0 + self.random() * 70.0;
                    let size = 2.2 + self.random() * 1.6;
                    let color = BULBS[k % BULBS.len()];
                    let spin = self.random() * TAU;
                    self.flakes.push(Flake { at: tip, v: Vec2::angled(a) * s, life: 1.0, size, star: true, color, spin });
                }
            }
        }
        self.last = cane;
        for f in &mut self.flakes {
            f.at += f.v * dt;
            // Snow drifts down slowly; stars slow down and fall.
            f.v = f.v * (1.0 - 2.5 * dt) + Vec2::new(0.0, if f.star { 40.0 } else { 14.0 } * dt);
            f.life -= dt / if f.star { 0.8 } else { 1.1 };
            f.spin += dt * 1.5;
        }
        self.flakes.retain(|f| f.life > 0.0);
        if self.flakes.len() > 220 {
            let extra = self.flakes.len() - 220;
            self.flakes.drain(..extra);
        }
    }

    /// The flakes, the ring of a click, its word, and the cane at `cane`. Whether something still moves.
    pub(super) fn paint(&mut self, painter: &egui::Painter, cane: Option<Pos2>, now: f64) -> bool {
        for f in &self.flakes {
            if f.star {
                painter.circle_filled(f.at, f.size * 2.0, f.color.gamma_multiply(0.15 * f.life));
                star(painter, f.at, f.size * f.life.max(0.4), f.color.lerp_to_gamma(Color32::WHITE, 0.3).gamma_multiply(f.life), f.spin);
            } else {
                snowflake(painter, f.at, f.size, f.color.gamma_multiply(0.85 * f.life), f.spin);
            }
        }
        if let Some((p, when)) = self.cast {
            let k = ((now - when) / 0.45) as f32;
            if k < 1.0 {
                painter.circle_stroke(p, 4.0 + k * 26.0, Stroke::new(2.0 * (1.0 - k), GOLD.gamma_multiply(1.0 - k)));
            } else {
                self.cast = None;
            }
        }
        if let Some((word, p, when)) = self.word {
            let k = ((now - when) / 1.3) as f32;
            if k < 1.0 {
                let at = p + Vec2::new(14.0, -14.0 - k * 26.0);
                let alpha = (1.0 - k).min(k * 6.0);
                let font = FontId::proportional(15.0);
                for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                    painter.text(at + Vec2::new(dx, dy), Align2::LEFT_CENTER, word, font.clone(), RED_DARK.gamma_multiply(alpha));
                }
                painter.text(at, Align2::LEFT_CENTER, word, font, SNOW.gamma_multiply(alpha));
            } else {
                self.word = None;
            }
        }
        if let Some(tip) = cane {
            paint_cane(painter, tip);
        }
        !self.flakes.is_empty() || self.cast.is_some() || self.word.is_some()
    }
}

/// The candy cane, its straight end on `tip`, going down to the right then hooking: white, red stripes
/// wound round it, a dark edge, a gleam.
pub(super) fn paint_cane(painter: &egui::Painter, tip: Pos2) {
    let dir = Vec2::new(0.45, 1.0).normalized();
    let n = dir.rot90();
    let len = 24.0;
    let r = 5.0;
    // Its path: the shaft from the tip, then a hook turning back.
    let end = tip + dir * len;
    let centre = end + n * r;
    let mut path: Vec<Pos2> = (0..=8).map(|k| tip + dir * (len * k as f32 / 8.0)).collect();
    for k in 1..=10 {
        let a = k as f32 / 10.0 * std::f32::consts::PI;
        path.push(centre - n * (r * a.cos()) + dir * (r * a.sin()));
    }
    painter.add(egui::Shape::line(path.clone(), Stroke::new(5.2, Color32::from_black_alpha(180))));
    painter.add(egui::Shape::line(path.clone(), Stroke::new(3.6, SNOW)));
    // The stripes: short red marks across, slanted.
    for k in 0..path.len() - 1 {
        if k % 2 == 0 {
            let (a, b) = (path[k], path[k + 1]);
            let along = (b - a).normalized();
            let across = along.rot90();
            let m = a + (b - a) * 0.5;
            painter.line_segment([m - across * 1.8 - along * 1.0, m + across * 1.8 + along * 1.0], Stroke::new(1.6, RED));
        }
    }
    painter.line_segment([tip + n * 0.9 + dir * 2.0, tip + n * 0.9 + dir * (len - 2.0)], Stroke::new(0.7, Color32::from_white_alpha(200)));
    painter.circle_filled(tip, 1.8, SNOW);
}

/// The great opening, `t` seconds in (at the start and when the theme is picked), over `screen`: a
/// winter night, snow falling thicker and thicker; a present drops in the middle, shakes, its lid flies
/// off in a burst of light and stars; the bauble rises out of it, rays turning, the sleigh crossing the
/// moon; then `title` letter by letter, `words` under it. False once over.
pub(super) fn intro(painter: &egui::Painter, screen: Rect, t: f32, title: &str, words: &str) -> bool {
    const LASTS: f32 = 7.0;
    if t > LASTS {
        return false;
    }
    let ease = |x: f32| 1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3);
    let fade_all = ((LASTS - t) / 0.5).clamp(0.0, 1.0);
    let c = screen.center();
    let s = (screen.height() / 700.0).clamp(0.6, 1.6);
    let a = (t / 0.35).min(1.0) * fade_all;
    let mut mesh = egui::Mesh::default();
    let (top, bottom) = (Color32::from_rgb(0x05, 0x0a, 0x1c), Color32::from_rgb(0x0c, 0x2a, 0x22));
    for (p, col) in [(screen.left_top(), top), (screen.right_top(), top), (screen.left_bottom(), bottom), (screen.right_bottom(), bottom)] {
        mesh.colored_vertex(p, col.gamma_multiply(a));
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
    let k = fade_all * (t / 0.6).min(1.0);
    // Stars, the moon, the sleigh across it.
    for i in 0..120u32 {
        let p = Pos2::new(screen.min.x + noise(i * 3 + 1) * screen.width(), screen.min.y + noise(i * 3 + 2) * screen.height() * 0.6);
        let tw = 0.5 + 0.5 * (t * (1.0 + noise(i) * 3.0) + noise(i + 9) * TAU).sin();
        painter.circle_filled(p, if noise(i + 5) > 0.9 { 1.6 } else { 0.9 } * s, Color32::WHITE.gamma_multiply(0.7 * tw * k));
    }
    let moon = Pos2::new(c.x + screen.width() * 0.26, screen.min.y + screen.height() * 0.2);
    glow(painter, moon, 150.0 * s, Color32::from_rgb(0xf4, 0xf0, 0xe0), 0.7 * k);
    painter.circle_filled(moon, 34.0 * s, Color32::from_rgb(0xf4, 0xf0, 0xe0).gamma_multiply(k));
    let ride = (t - 0.5) / 4.0;
    if (0.0..1.0).contains(&ride) {
        let x = screen.min.x - 200.0 + (screen.width() + 400.0) * ride;
        paint_sleigh(painter, Pos2::new(x, moon.y + 20.0 * s - (ride - 0.5).powi(2) * 120.0 * s), 30.0 * s, 1.0, t);
    }
    // Snowy ground, firs.
    let ground = screen.max.y - screen.height() * 0.16;
    painter.rect_filled(Rect::from_min_max(Pos2::new(screen.min.x, ground), screen.max), 0.0, Color32::from_rgb(0xdc, 0xe6, 0xf6).gamma_multiply(k));
    for i in 0..14u32 {
        let x = screen.min.x + noise(i + 600) * screen.width();
        fir(painter, Pos2::new(x, ground + 4.0 * s), (60.0 + noise(i + 610) * 80.0) * s, Color32::from_rgb(0x1a, 0x40, 0x36).gamma_multiply(k), true);
    }

    // The present: it drops, bounces, shakes, then its lid flies off.
    let drop = ease((t - 0.4) / 0.6);
    let size = 150.0 * s;
    let shake = if t > 1.1 && t < 2.1 { ((t - 1.1) * 40.0).sin() * 6.0 * s * (t - 1.1) } else { 0.0 };
    let foot = Pos2::new(c.x + shake, screen.min.y - size + (c.y + size * 0.55 - screen.min.y + size) * drop);
    let opened = t - 2.1;
    if t > 0.4 {
        let body = Rect::from_min_max(foot - Vec2::new(size / 2.0, size * 0.8), foot + Vec2::new(size / 2.0, 0.0));
        painter.rect_filled(body.translate(Vec2::new(6.0, 6.0) * s), 6.0 * s, Color32::from_black_alpha((80.0 * fade_all) as u8));
        painter.rect_filled(body, 6.0 * s, RED.gamma_multiply(fade_all));
        painter.rect_filled(Rect::from_min_max(Pos2::new(body.max.x - size * 0.25, body.min.y), body.max), 6.0 * s, RED_DARK.gamma_multiply(0.6 * fade_all));
        painter.rect_filled(Rect::from_center_size(body.center(), Vec2::new(size * 0.14, body.height())), 0.0, GOLD.gamma_multiply(fade_all));
        // The lid, then the bow on it.
        let lift = if opened > 0.0 { ease(opened / 0.6) } else { 0.0 };
        let lid_c = Pos2::new(body.center().x + lift * 180.0 * s, body.min.y - size * 0.06 - lift * 260.0 * s);
        let turn = lift * 1.2;
        let lid_alpha = fade_all * (1.0 - ((opened - 0.6) / 0.4).clamp(0.0, 1.0));
        let lid: Vec<Pos2> = [(-0.56, -0.08), (0.56, -0.08), (0.56, 0.08), (-0.56, 0.08)].iter().map(|&(x, y)| lid_c + Vec2::new(x * size, y * size).rot(turn)).collect();
        painter.add(egui::Shape::convex_polygon(lid, RED.lerp_to_gamma(Color32::WHITE, 0.08).gamma_multiply(lid_alpha), Stroke::NONE));
        for side in [-1.0f32, 1.0] {
            let bow: Vec<Pos2> = (0..12).map(|j| {
                let a = j as f32 / 12.0 * TAU;
                lid_c + (Vec2::new(side * size * 0.16, -size * 0.12) + Vec2::new(a.cos() * size * 0.14, a.sin() * size * 0.08)).rot(turn)
            }).collect();
            painter.add(egui::Shape::convex_polygon(bow, GOLD.gamma_multiply(lid_alpha), Stroke::NONE));
        }
    }
    // The burst out of it: a flash, a ring, stars and flakes everywhere, rays, the bauble rising.
    if opened > 0.0 {
        let o = c - Vec2::new(0.0, 0.0) + Vec2::new(0.0, size * 0.1);
        let flash = (1.0 - opened / 0.35).max(0.0);
        painter.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xd8, (170.0 * flash) as u8));
        let ring = ease(opened / 0.9);
        painter.circle_stroke(o, ring * screen.width() * 0.6, Stroke::new(14.0 * (1.0 - ring) * s, GOLD.gamma_multiply((1.0 - ring) * fade_all)));
        for i in 0..90u32 {
            let ang = noise(i + 900) * TAU;
            let speed = (180.0 + noise(i + 910) * 520.0) * s;
            let life = 0.7 + noise(i + 920) * 1.0;
            if opened > life {
                continue;
            }
            let p = o + Vec2::angled(ang) * speed * ease(opened / life) + Vec2::new(0.0, opened * opened * 100.0 * s);
            let f = 1.0 - opened / life;
            if i % 3 == 0 {
                snowflake(painter, p, 7.0 * s * f + 1.0, SNOW.gamma_multiply(f * fade_all), opened * 2.0 + i as f32);
            } else {
                star(painter, p, 6.0 * s * f + 1.0, BULBS[i as usize % BULBS.len()].gamma_multiply(f * fade_all), opened * 3.0 + i as f32);
            }
        }
        let rays = (opened / 0.5).min(1.0) * fade_all;
        for i in 0..14 {
            let a0 = i as f32 / 14.0 * TAU + t * 0.25;
            let len = screen.width() * 0.7;
            painter.add(egui::Shape::convex_polygon(vec![o, o + Vec2::angled(a0 - 0.05) * len, o + Vec2::angled(a0 + 0.05) * len], GOLD.gamma_multiply(0.06 * rays), Stroke::NONE));
        }
        let rise = ease(opened / 0.6);
        let h = 170.0 * s * rise;
        let at = o - Vec2::new(0.0, 150.0 * s * rise);
        glow(painter, at, 200.0 * s, GOLD, 0.8 * rays);
        if h > 2.0 {
            bauble(painter, Rect::from_center_size(at, Vec2::new(h * 0.85, h)));
            holly(painter, at - Vec2::new(0.0, h * 0.5), h * 0.3, -TAU / 4.0);
        }
        // The title, letter by letter.
        let font = FontId::proportional(64.0 * s);
        let full = painter.layout_no_wrap(title.to_owned(), font.clone(), SNOW).size().x;
        let mut x = c.x - full / 2.0;
        let y = c.y + 110.0 * s;
        for (i, ch) in title.chars().enumerate() {
            let at = opened - 0.55 - i as f32 * 0.07;
            let w = painter.layout_no_wrap(ch.to_string(), font.clone(), SNOW).size().x;
            if at > 0.0 {
                let e = ease(at / 0.3);
                let p = Pos2::new(x + w / 2.0, y - (1.0 - e) * 40.0 * s);
                let alpha = e * fade_all;
                for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
                    painter.text(p + Vec2::new(dx, dy) * s, Align2::CENTER_CENTER, ch, font.clone(), RED_DARK.gamma_multiply(alpha));
                }
                painter.text(p, Align2::CENTER_CENTER, ch, font.clone(), SNOW.gamma_multiply(alpha));
                if ch != ' ' {
                    snow_cap(painter, Rect::from_center_size(p - Vec2::new(0.0, 18.0 * s), Vec2::new(w * 0.7, 4.0 * s)), 4.0 * s, i as u32 * 7);
                }
            }
            x += w;
        }
        let wa = ((opened - 1.2) / 0.4).clamp(0.0, 1.0) * fade_all;
        painter.text(Pos2::new(c.x, y + 58.0 * s), Align2::CENTER_CENTER, words, FontId::proportional(18.0 * s), Color32::from_rgb(0xf6, 0xf0, 0xe4).gamma_multiply(wa));
    }
    // Snow, thicker and thicker, over all of it.
    let thick = (t / 2.0).min(1.0);
    for i in 0..(180.0 * thick) as u32 {
        let speed = (40.0 + noise(i + 1000) * 80.0) * s;
        let y = screen.min.y + ((t * speed + noise(i + 1010) * screen.height()) % screen.height());
        let x = screen.min.x + noise(i + 1020) * screen.width() + (t * 0.8 + i as f32).sin() * 10.0 * s;
        let r = (1.0 + noise(i + 1030) * 2.2) * s;
        painter.circle_filled(Pos2::new(x, y), r, SNOW.gamma_multiply(0.8 * fade_all));
    }
    true
}

/// Hoofprints in the snow along the foot of `area` (the terminals) every 70 s: a reindeer's steps
/// walking across, each fading after a moment, a gift tag over the walker with its name.
pub(super) fn footprints(ctx: &egui::Context, area: Rect, name: &str) {
    let now = ctx.input(|i| i.time);
    let into = (now + 25.0) % 70.0;
    const WALK: f64 = 14.0;
    if into > WALK + 2.5 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(70.0 - into));
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("hoofprints"))).with_clip_rect(area);
    let rightward = ((now + 25.0) / 70.0) as i64 % 2 == 0;
    let ink = Color32::from_rgb(0xdc, 0xe6, 0xf6);
    let step = 16.0;
    let y = area.max.y - 14.0;
    let steps = ((area.width() + 40.0) / step) as usize;
    let mut walker = None;
    for k in 0..steps {
        let placed = k as f64 / steps as f64 * WALK;
        let age = (into - placed) as f32;
        if age < 0.0 {
            break;
        }
        let along = area.min.x - 20.0 + k as f32 * step;
        let x = if rightward { along } else { area.max.x + area.min.x - along };
        let p = Pos2::new(x, y + if k % 2 == 0 { -3.5 } else { 3.5 });
        walker = Some(p);
        let alpha = (1.0 - age / 2.5).max(0.0);
        if alpha <= 0.0 {
            continue;
        }
        // A cloven hoof: two halves.
        for s in [-1.0f32, 1.0] {
            let half: Vec<Pos2> = (0..10).map(|j| {
                let a = j as f32 / 10.0 * TAU;
                p + Vec2::new(a.cos() * 2.4, s * 1.4 + a.sin() * 1.2)
            }).collect();
            painter.add(egui::Shape::convex_polygon(half, ink.gamma_multiply(0.7 * alpha), Stroke::NONE));
        }
    }
    if let Some(p) = walker.filter(|_| into < WALK) {
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(11.0), Color32::from_rgb(0x5a, 0x10, 0x18));
        let tag = Rect::from_center_size(p - Vec2::new(0.0, 18.0), galley.size() + Vec2::new(14.0, 4.0));
        painter.rect(tag, 3.0, Color32::from_rgb(0xfb, 0xf4, 0xe4), Stroke::new(1.0, RED), egui::StrokeKind::Inside);
        painter.circle_filled(Pos2::new(tag.min.x + 4.0, tag.center().y), 1.4, RED);
        painter.galley(tag.center() - galley.size() / 2.0 + Vec2::new(2.0, 0.0), galley, Color32::BLACK);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(60));
}

/// The strip of categories (or the folded rail) in `r`: a deep fir green fading darker, snow along the
/// top, a ribbon in red and white on its right edge (a candy stripe), slowly turning.
pub(super) fn strip_bg(painter: &egui::Painter, r: Rect) {
    vgrad(painter, r, Color32::from_rgb(0x14, 0x3e, 0x2c), Color32::from_rgb(0x08, 0x20, 0x16));
    let x = r.max.x - 2.0;
    let ctx = painter.ctx();
    let now = ctx.input(|i| i.time) as f32;
    let clip = painter.with_clip_rect(Rect::from_min_max(Pos2::new(x - 2.0, r.min.y), Pos2::new(x + 2.0, r.max.y)).intersect(painter.clip_rect()));
    clip.vline(x, r.y_range(), Stroke::new(4.0, SNOW));
    let mut y = r.min.y - 10.0 + (now * 6.0) % 10.0;
    while y < r.max.y {
        clip.line_segment([Pos2::new(x - 2.0, y), Pos2::new(x + 2.0, y - 4.0)], Stroke::new(2.0, RED));
        y += 10.0;
    }
    snow_cap(painter, Rect::from_min_max(Pos2::new(r.min.x, r.min.y + 2.0), Pos2::new(r.max.x - 3.0, r.min.y + 6.0)), 3.0, 21);
    ctx.request_repaint_after(std::time::Duration::from_millis(80));
}

/// The active category's slot: a warm glow, a thin gold ring, three little lights circling it.
pub(super) fn active_slot(painter: &egui::Painter, slot: Rect) {
    let ctx = painter.ctx();
    let tm = ctx.input(|i| i.time) as f32;
    let c = slot.center();
    glow(painter, c, 30.0, GOLD, 0.8);
    painter.rect_stroke(slot, 9.0, Stroke::new(1.0, GOLD.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    for k in 0..3 {
        let a = tm * 1.2 + k as f32 * TAU / 3.0;
        let p = c + Vec2::new(a.cos() * 22.0, a.sin() * 20.0);
        let color = BULBS[k % BULBS.len()];
        painter.circle_filled(p, 3.0, color.gamma_multiply(0.2));
        painter.circle_filled(p, 1.3, color);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(40));
}

/// Over the panel: a cold light from the top, its sides a little darker.
pub(super) fn panel_bg(painter: &egui::Painter, r: Rect) {
    vgrad(painter, Rect::from_min_size(r.min, Vec2::new(r.width(), 180.0)), SNOW.gamma_multiply(0.04), Color32::TRANSPARENT);
    for (x0, x1, left) in [(r.min.x, r.min.x + 14.0, true), (r.max.x - 14.0, r.max.x, false)] {
        let mut mesh = egui::Mesh::default();
        let dark = Color32::from_black_alpha(40);
        let (a, b) = if left { (dark, Color32::TRANSPARENT) } else { (Color32::TRANSPARENT, dark) };
        mesh.colored_vertex(Pos2::new(x0, r.min.y), a);
        mesh.colored_vertex(Pos2::new(x1, r.min.y), b);
        mesh.colored_vertex(Pos2::new(x0, r.max.y), a);
        mesh.colored_vertex(Pos2::new(x1, r.max.y), b);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        painter.add(mesh);
    }
}

/// An ornament across `width` centered at `c`: holly in the middle, fairy lights fading outward.
pub(super) fn ornament(painter: &egui::Painter, c: Pos2, width: f32) {
    let half = width / 2.0;
    for s in [-1.0f32, 1.0] {
        for k in 0..10 {
            let t = k as f32 / 10.0;
            let p = c + Vec2::new(s * (14.0 + (half - 14.0) * t), (t * 9.0).sin() * 1.2);
            let color = BULBS[k % BULBS.len()];
            painter.circle_filled(p, 1.3, color.gamma_multiply(0.9 * (1.0 - t)));
        }
    }
    holly(painter, c, 12.0, -TAU / 4.0);
}

/// Before a section's title: a small gold star; after it, a line of lights fading out.
pub(super) fn title_marks(painter: &egui::Painter, rect: Rect, text_end: f32) {
    let y = rect.center().y;
    star(painter, Pos2::new(rect.min.x + 3.0, y), 3.6, GOLD, 0.0);
    let from = text_end + 8.0;
    let to = (rect.max.x - 60.0).max(from + 10.0);
    painter.hline(from..=to, y, Stroke::new(0.7, GOLD.gamma_multiply(0.25)));
    let n = ((to - from) / 12.0) as i32;
    for k in 0..n {
        let t = k as f32 / n.max(1) as f32;
        painter.circle_filled(Pos2::new(from + 6.0 + k as f32 * 12.0, y), 1.2, BULBS[k as usize % BULBS.len()].gamma_multiply(0.8 * (1.0 - t)));
    }
}

/// A small gold star at `c`, `d` its half-size (the Gryffindor diamonds' place).
pub(super) fn mark(painter: &egui::Painter, c: Pos2, d: f32) {
    star(painter, c, d * 1.3, GOLD, 0.0);
}

/// The tab shown in the bar of tabs: a gold outline, a warm glow under it, a star on its top middle.
pub(super) fn active_chip(painter: &egui::Painter, r: Rect) {
    glow(painter, Pos2::new(r.center().x, r.max.y), r.width() * 0.45, GOLD, 0.5);
    painter.rect_stroke(r, 8.0, Stroke::new(1.0, GOLD.gamma_multiply(0.75)), egui::StrokeKind::Inside);
    star(painter, Pos2::new(r.center().x, r.min.y + 0.5), 3.8, GOLD, 0.0);
}

/// A settings entry picked: a warm glow and a red outline, a sprig of holly.
pub(super) fn active_nav(painter: &egui::Painter, r: Rect) {
    glow(painter, Pos2::new(r.min.x + 22.0, r.center().y), 26.0, GOLD, 0.6);
    painter.rect_stroke(r, 8.0, Stroke::new(1.0, RED.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    holly(painter, Pos2::new(r.max.x - 12.0, r.min.y + 4.0), 10.0, TAU * 3.0 / 8.0);
}

/// The opening's tune: « Jingle Bells » (James Lord Pierpont, 1857, in the public domain), its chorus on
/// bells: each note's MIDI number (0: a rest) and length in seconds.
pub(super) const OPENING_TUNE: [(f32, f32); 27] = [
    (76.0, 0.2), (76.0, 0.2), (76.0, 0.4), (76.0, 0.2), (76.0, 0.2), (76.0, 0.4),
    (76.0, 0.2), (79.0, 0.2), (72.0, 0.3), (74.0, 0.1), (76.0, 0.8),
    (77.0, 0.2), (77.0, 0.2), (77.0, 0.3), (77.0, 0.1), (77.0, 0.2), (76.0, 0.2), (76.0, 0.2), (76.0, 0.1), (76.0, 0.1),
    (79.0, 0.2), (79.0, 0.2), (77.0, 0.2), (74.0, 0.2), (72.0, 0.6), (0.0, 0.1), (0.0, 0.1),
];

// --- outside the window: the sleigh along the foot of the screen, a snow shower

/// How often the sleigh flies by, how long it takes.
const SLEIGH_EVERY: f64 = 480.0;
const SLEIGH_RUN: f64 = 9.0;

/// Where the sleigh is at `now`: how far along (0 to 1), rightward; out of sight, the seconds before it
/// comes.
pub(super) fn sleigh_at(now: f64) -> Result<(f64, bool), f32> {
    let t = (now + 200.0) % SLEIGH_EVERY;
    if t > SLEIGH_RUN {
        return Err((SLEIGH_EVERY - t) as f32);
    }
    Ok((t / SLEIGH_RUN, ((now + 200.0) / SLEIGH_EVERY).floor() as i64 % 2 == 0))
}

/// Which run it is.
pub(super) fn sleigh_lap(now: f64) -> f64 {
    ((now + 200.0) / SLEIGH_EVERY).floor()
}

/// The sleigh flying along above `strip` now and then, one way then the other: whether it shows, and
/// when to draw again.
pub(super) fn sleigh_run(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    let (along, rightward) = match sleigh_at(now) {
        Ok(at) => at,
        Err(again) => return (false, again),
    };
    let s = 34.0;
    let x = strip.min.x - 200.0 + (strip.width() + 400.0) * along as f32;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    let y = strip.max.y - 70.0 - ((along as f32 - 0.5) * 2.0).powi(2) * -20.0;
    paint_sleigh(painter, Pos2::new(x, y), s, if rightward { 1.0 } else { -1.0 }, now as f32);
    (true, 0.0)
}

/// How often the snow shower comes, how long it lasts.
const SNOW_EVERY: f64 = 600.0;
const SNOW_LASTS: f64 = 45.0;

/// The snow shower at `now`: how far in (0 to 1); out of it, the seconds before it.
pub(super) fn snow_at(now: f64) -> Result<f64, f32> {
    let t = (now + 300.0) % SNOW_EVERY;
    if t > SNOW_LASTS {
        return Err((SNOW_EVERY - t) as f32);
    }
    Ok(t / SNOW_LASTS)
}

/// Snow falling over `whole` now and then, a few flakes then more, then fewer: whether it shows, when to
/// draw again.
pub(super) fn snow_shower(painter: &egui::Painter, whole: Rect, now: f64) -> (bool, f32) {
    let k = match snow_at(now) {
        Ok(k) => k as f32,
        Err(again) => return (false, again),
    };
    let thick = (k * 6.0).min(1.0).min((1.0 - k) * 6.0);
    let tm = now as f32;
    let n = (90.0 * thick) as u32;
    for i in 0..n {
        let speed = 30.0 + noise(i + 2000) * 50.0;
        let y = whole.min.y + ((tm * speed + noise(i + 2010) * whole.height()) % whole.height());
        let x = whole.min.x + noise(i + 2020) * whole.width() + (tm * 0.6 + i as f32).sin() * 14.0;
        let r = 1.6 + noise(i + 2030) * 3.0;
        if r > 3.6 {
            snowflake(painter, Pos2::new(x, y), r * 1.4, SNOW.gamma_multiply(0.85), tm * 0.5 + i as f32);
        } else {
            painter.circle_filled(Pos2::new(x, y), r * 0.6, SNOW.gamma_multiply(0.8));
        }
    }
    (true, 0.0)
}
