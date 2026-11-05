//! The Gryffindor theme's own touches: the pointer as a wand (golden sparks behind it, a burst at each
//! click), a gold line under the bars and over the active tab, the
//! focused pane framed in gold, the name in gold under a lightning bolt, and at the foot of the
//! sidebar the castle at night (its windows lit, the moon, the stars, floating candles), the Golden
//! Snitch darting about now and then and an owl crossing with a letter. The guard's window says
//! « Expelliarmus ! »; picking the theme, the Sorting Hat calls the house.

use super::*;
use std::f32::consts::TAU;

/// What the guard's window says instead of its title.
pub(super) const ALERT: &str = "Expelliarmus !";

pub(super) const SCARLET: Color32 = Color32::from_rgb(0xc8, 0x10, 0x20);
pub(super) const GOLD: Color32 = Color32::from_rgb(0xee, 0xba, 0x30);
const GOLD_DARK: Color32 = Color32::from_rgb(0xa8, 0x7a, 0x1a);
const NIGHT: Color32 = Color32::from_rgb(0x0c, 0x0a, 0x1e);
const STONE: Color32 = Color32::from_rgb(0x1a, 0x16, 0x26);
const WINDOW: Color32 = Color32::from_rgb(0xff, 0xc8, 0x5a);
const WOOD: Color32 = Color32::from_rgb(0x4a, 0x2c, 0x16);
const WOOD_LIGHT: Color32 = Color32::from_rgb(0x7a, 0x4e, 0x2a);

/// Deterministic numbers in [0, 1) (the same stars at each frame).
fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65536.0
}

/// A gold line along `rect`, softly glowing (under the bars, over the active tab).
pub(super) fn gold_line(painter: &egui::Painter, rect: Rect) {
    let y = rect.center().y;
    painter.hline(rect.x_range(), y, Stroke::new(rect.height().max(3.0), GOLD.gamma_multiply(0.18)));
    painter.hline(rect.x_range(), y, Stroke::new(1.5, GOLD));
}

/// The focused pane framed in gold, a soft glow inward, a small diamond at each corner.
pub(super) fn frame(painter: &egui::Painter, r: Rect) {
    for (k, alpha) in [0.95, 0.4, 0.2, 0.1, 0.04].into_iter().enumerate() {
        painter.rect_stroke(r.shrink(1.0 + k as f32), 4.0, Stroke::new(1.0, GOLD.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    }
    let r = r.shrink(1.5);
    for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
        let d = 4.5;
        painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, -d), c + Vec2::new(d, 0.0), c + Vec2::new(0.0, d), c + Vec2::new(-d, 0.0)], GOLD, Stroke::NONE));
    }
}

/// A lightning bolt, `h` high, its middle at `c`.
fn bolt(painter: &egui::Painter, c: Pos2, h: f32, color: Color32) {
    let p = |x: f32, y: f32| c + Vec2::new(x, y) * h;
    let pts = vec![p(0.12, -0.5), p(-0.2, 0.04), p(0.02, 0.04), p(-0.12, 0.5)];
    painter.add(egui::Shape::line(pts.clone(), Stroke::new(h * 0.16, color.gamma_multiply(0.3))));
    painter.add(egui::Shape::line(pts, Stroke::new(h * 0.07, color)));
}

/// Stars scattered in `rect`, twinkling.
fn stars(painter: &egui::Painter, rect: Rect, n: u32, seed: u32, now: f64) {
    for i in 0..n {
        let s = seed.wrapping_add(i * 7);
        let at = Pos2::new(rect.min.x + noise(s) * rect.width(), rect.min.y + noise(s + 1) * rect.height());
        let bright = noise(s + 2);
        let twinkle = 0.6 + 0.4 * ((now as f32) * (0.7 + noise(s + 3) * 2.0) + noise(s + 4) * TAU).sin();
        painter.circle_filled(at, if bright > 0.9 { 1.2 } else { 0.7 }, Color32::from_rgb(0xf6, 0xee, 0xd8).gamma_multiply((0.3 + bright * 0.6) * twinkle));
    }
}

/// The sidebar's logo: the name in gold, a lightning bolt before it, a few stars.
pub(super) fn logo(painter: &egui::Painter, rect: Rect) {
    let field = Rect::from_center_size(rect.center(), Vec2::new(rect.width().min(190.0), rect.height()));
    stars(painter, field, 18, 7, 0.0);
    let at = rect.center() - Vec2::new(-8.0, 2.0);
    let font = FontId::proportional(28.0);
    let galley = painter.layout_no_wrap("Ronnie".to_owned(), font.clone(), GOLD);
    let left = at.x - galley.size().x / 2.0;
    bolt(painter, Pos2::new(left - 14.0, at.y), 26.0, GOLD);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        painter.text(at + Vec2::new(dx, dy), Align2::CENTER_CENTER, "Ronnie", font.clone(), SCARLET);
    }
    painter.text(at, Align2::CENTER_CENTER, "Ronnie", font, GOLD);
}

/// Where the castle stands in the scene `rect` (to hover it).
pub(super) fn castle_rect(rect: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(rect.min.x + rect.width() * 0.18, rect.max.y - 70.0), Pos2::new(rect.min.x + rect.width() * 0.86, rect.max.y - 14.0))
}

/// The castle at night at the foot of `rect` (the sidebar's panel): the sky darkening upward, stars,
/// the moon, the castle and its lit windows over the lake, floating candles; now and then the Snitch,
/// and an owl with a letter. When to paint again (seconds).
pub(super) fn scene(painter: &egui::Painter, rect: Rect, now: f64) -> f32 {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let tm = now as f32;
    // The sky, fading into the sidebar at the top.
    let mut mesh = egui::Mesh::default();
    let (top, low) = (NIGHT.gamma_multiply(0.0), NIGHT);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), low);
    mesh.colored_vertex(rect.right_bottom(), low);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
    stars(&painter, Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.max.y - 40.0)), 22, 31, now);
    // The moon, a crescent.
    let moon = Pos2::new(rect.min.x + rect.width() * 0.84, rect.min.y + rect.height() * 0.3);
    painter.circle_filled(moon, 16.0, Color32::from_rgb(0xf6, 0xee, 0xd0).gamma_multiply(0.08));
    painter.circle_filled(moon, 9.0, Color32::from_rgb(0xf6, 0xee, 0xd0));
    painter.circle_filled(moon + Vec2::new(-3.5, -2.0), 8.0, NIGHT.gamma_multiply(0.97));

    // The lake, the castle's reflection in it.
    let shore = rect.max.y - 14.0;
    painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, shore), rect.max), 0.0, Color32::from_rgb(0x08, 0x0c, 0x1c));
    for k in 0..6 {
        let y = shore + 3.0 + k as f32 * 2.0;
        let x = rect.min.x + noise(k * 13 + 5) * rect.width() + (tm * 0.6 + k as f32).sin() * 6.0;
        painter.hline(x - 10.0..=x + 10.0, y, Stroke::new(0.8, WINDOW.gamma_multiply(0.15)));
    }

    // The castle: a hill, then the towers, each a wall and a pointed roof, the great hall between.
    let w = rect.width();
    let x0 = rect.min.x;
    let hill = vec![Pos2::new(x0 + w * 0.08, shore), Pos2::new(x0 + w * 0.25, shore - 12.0), Pos2::new(x0 + w * 0.6, shore - 16.0), Pos2::new(x0 + w * 0.92, shore)];
    painter.add(egui::Shape::convex_polygon(hill, STONE, Stroke::NONE));
    // (x, width, height, roof) in the sidebar's width and in points.
    let towers: [(f32, f32, f32, f32); 7] = [(0.22, 9.0, 34.0, 12.0), (0.31, 12.0, 46.0, 16.0), (0.42, 8.0, 30.0, 10.0), (0.5, 14.0, 58.0, 20.0), (0.6, 10.0, 40.0, 14.0), (0.7, 12.0, 50.0, 17.0), (0.8, 8.0, 28.0, 10.0)];
    let base = shore - 12.0;
    painter.rect_filled(Rect::from_min_max(Pos2::new(x0 + w * 0.24, base - 22.0), Pos2::new(x0 + w * 0.78, base)), 0.0, STONE);
    // The great hall's roof.
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(x0 + w * 0.34, base - 22.0), Pos2::new(x0 + w * 0.47, base - 32.0), Pos2::new(x0 + w * 0.6, base - 22.0)], STONE, Stroke::NONE));
    for (k, &(fx, tw, th, roof)) in towers.iter().enumerate() {
        let cx = x0 + w * fx;
        let wall = Rect::from_min_max(Pos2::new(cx - tw / 2.0, base - th), Pos2::new(cx + tw / 2.0, base));
        painter.rect_filled(wall, 0.0, STONE);
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(cx - tw / 2.0 - 1.5, wall.min.y), Pos2::new(cx, wall.min.y - roof), Pos2::new(cx + tw / 2.0 + 1.5, wall.min.y)], STONE, Stroke::NONE));
        // A flag on the highest.
        if k == 3 {
            let top = Pos2::new(cx, wall.min.y - roof);
            painter.line_segment([top, top - Vec2::new(0.0, 8.0)], Stroke::new(0.8, STONE));
            let wave = (tm * 3.0).sin() * 1.2;
            painter.add(egui::Shape::convex_polygon(vec![top - Vec2::new(0.0, 8.0), top + Vec2::new(8.0, -6.0 + wave), top - Vec2::new(0.0, 4.0)], SCARLET, Stroke::NONE));
        }
        // Its windows, lit, some flickering.
        let rows = (th / 12.0) as u32;
        for r in 0..rows {
            let s = k as u32 * 17 + r * 5;
            if noise(s) < 0.35 {
                continue;
            }
            let flicker = 0.75 + 0.25 * (tm * (1.0 + noise(s + 1) * 3.0) + noise(s + 2) * TAU).sin();
            let p = Pos2::new(cx, wall.min.y + 6.0 + r as f32 * 11.0);
            painter.rect_filled(Rect::from_center_size(p, Vec2::new(2.2, 3.4)), 0.8, WINDOW.gamma_multiply(flicker));
            painter.circle_filled(p, 3.5, WINDOW.gamma_multiply(0.08 * flicker));
        }
    }
    // The great hall's tall windows.
    for k in 0..5 {
        let p = Pos2::new(x0 + w * (0.38 + k as f32 * 0.045), base - 11.0);
        let flicker = 0.8 + 0.2 * (tm * 2.0 + k as f32).sin();
        painter.rect_filled(Rect::from_center_size(p, Vec2::new(2.4, 7.0)), 1.0, WINDOW.gamma_multiply(flicker));
    }

    // Floating candles, bobbing, their flames flickering.
    for k in 0..5u32 {
        let x = x0 + w * (0.1 + noise(k * 3 + 1) * 0.8);
        let y = rect.min.y + rect.height() * (0.12 + noise(k * 3 + 2) * 0.25) + (tm * 0.8 + k as f32).sin() * 3.0;
        painter.rect_filled(Rect::from_min_size(Pos2::new(x - 1.5, y), Vec2::new(3.0, 9.0)), 0.5, Color32::from_rgb(0xf2, 0xe8, 0xd0));
        let flame = Pos2::new(x + (tm * 7.0 + k as f32).sin() * 0.5, y - 2.5);
        painter.circle_filled(flame, 5.0, WINDOW.gamma_multiply(0.12));
        painter.add(egui::Shape::convex_polygon(vec![flame + Vec2::new(0.0, -3.5), flame + Vec2::new(1.5, 0.5), flame + Vec2::new(-1.5, 0.5)], WINDOW, Stroke::NONE));
    }

    // The Snitch: 9 s every 21, darting on loops, its wings a blur.
    let into = (now + 4.0) % 21.0;
    let mut again = 0.12;
    if into < 9.0 {
        let p = into as f32;
        let fade = (p * 2.0).min(1.0).min((9.0 - p) * 2.0);
        let c = Pos2::new(x0 + w * (0.5 + 0.38 * (p * 0.9).sin() * (p * 0.37).cos()), rect.min.y + rect.height() * (0.35 + 0.22 * (p * 1.7).sin()));
        snitch(&painter, c, 4.0, tm, fade);
        again = 1.0 / 40.0;
    }
    // An owl with a letter, every 37 s, across.
    let owl_into = (now + 15.0) % 37.0;
    if owl_into < 6.0 {
        let p = (owl_into / 6.0) as f32;
        let dir = if ((now + 15.0) / 37.0) as i64 % 2 == 0 { 1.0 } else { -1.0 };
        let (from, to) = (rect.min.x - 20.0, rect.max.x + 20.0);
        let x = if dir > 0.0 { from + (to - from) * p } else { to - (to - from) * p };
        let y = rect.min.y + rect.height() * 0.22 + (p * 9.0).sin() * 4.0;
        owl(&painter, Pos2::new(x, y), dir, tm);
        again = 1.0 / 40.0;
    }
    again
}

/// The Golden Snitch at `c`, `r` its radius: a gold ball, its silver wings beating.
fn snitch(painter: &egui::Painter, c: Pos2, r: f32, tm: f32, alpha: f32) {
    let beat = (tm * 40.0).sin();
    for side in [-1.0f32, 1.0] {
        let tip = c + Vec2::new(side * r * 3.2, -r * (0.6 + beat * 0.9));
        let back = c + Vec2::new(side * r * 2.6, r * 0.4);
        painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(side * r * 0.6, -r * 0.2), tip, back], Color32::from_rgb(0xe8, 0xec, 0xf2).gamma_multiply(0.8 * alpha), Stroke::NONE));
    }
    painter.circle_filled(c, r * 2.4, GOLD.gamma_multiply(0.15 * alpha));
    painter.circle_filled(c, r, GOLD.gamma_multiply(alpha));
    painter.circle_filled(c + Vec2::new(-r * 0.3, -r * 0.35), r * 0.35, Color32::from_rgb(0xff, 0xf2, 0xc0).gamma_multiply(alpha));
}

/// An owl flying `dir`-ward (1 right) at `c`, a letter in its claws.
fn owl(painter: &egui::Painter, c: Pos2, dir: f32, tm: f32) {
    let body = Color32::from_rgb(0xd8, 0xd0, 0xc0);
    let flap = (tm * 9.0).sin();
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-2.0, -1.0), c + Vec2::new(-12.0, -4.0 - flap * 6.0), c + Vec2::new(-4.0, 2.0)], body.gamma_multiply(0.9), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(2.0, -1.0), c + Vec2::new(12.0, -4.0 - flap * 6.0), c + Vec2::new(4.0, 2.0)], body.gamma_multiply(0.9), Stroke::NONE));
    painter.circle_filled(c, 4.0, body);
    painter.circle_filled(c + Vec2::new(dir * 3.0, -3.0), 2.8, body);
    painter.circle_filled(c + Vec2::new(dir * 4.0, -3.5), 0.7, Color32::BLACK);
    // The letter, its red seal.
    let letter = Rect::from_center_size(c + Vec2::new(0.0, 6.5), Vec2::new(6.0, 4.0));
    painter.rect_filled(letter, 0.5, Color32::from_rgb(0xf0, 0xe2, 0xc0));
    painter.circle_filled(letter.center(), 1.0, SCARLET);
}

/// A spark of the wand.
struct Spark {
    at: Pos2,
    v: Vec2,
    life: f32,
    size: f32,
    gold: bool,
}

/// The wand's sparks, kept from frame to frame.
#[derive(Default)]
pub(super) struct Wand {
    sparks: Vec<Spark>,
    last: Option<Pos2>,
    rng: u64,
    /// A spell cast (a click) where, when.
    cast: Option<(Pos2, f64)>,
}

impl Wand {
    fn random(&mut self) -> f32 {
        (super::game::random(&mut self.rng) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// One frame: the pointer as a wand (for the plain arrow and the hand), sparks where it went, a
    /// burst and a ring of light at a click.
    pub(super) fn frame(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let at = ctx.input(|i| i.pointer.hover_pos());
        let icon = ctx.output(|o| o.cursor_icon);
        let wand = at.filter(|_| matches!(icon, egui::CursorIcon::Default | egui::CursorIcon::PointingHand));
        if let Some(tip) = wand {
            // Sparks along the way, a few even still.
            let moved = self.last.map_or(0.0, |l| l.distance(tip));
            let n = ((moved / 6.0) as usize).min(6) + usize::from(self.random() < 0.15);
            for _ in 0..n {
                let (a, s) = (self.random() * TAU, 10.0 + self.random() * 30.0);
                let gold = self.random() < 0.8;
                let size = 0.8 + self.random() * 1.4;
                self.sparks.push(Spark { at: tip, v: Vec2::angled(a) * s, life: 1.0, size, gold });
            }
            if ctx.input(|i| i.pointer.any_pressed()) {
                self.cast = Some((tip, now));
                for k in 0..18 {
                    let a = k as f32 / 18.0 * TAU + self.random() * 0.3;
                    let s = 60.0 + self.random() * 70.0;
                    let size = 1.4 + self.random();
                    self.sparks.push(Spark { at: tip, v: Vec2::angled(a) * s, life: 1.0, size, gold: k % 3 != 0 });
                }
            }
        }
        self.last = wand;
        for s in &mut self.sparks {
            s.at += s.v * dt;
            s.v = s.v * (1.0 - 2.5 * dt) + Vec2::new(0.0, 30.0 * dt);
            s.life -= dt / 0.75;
        }
        self.sparks.retain(|s| s.life > 0.0);
        if self.sparks.len() > 260 {
            let extra = self.sparks.len() - 260;
            self.sparks.drain(..extra);
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("wand")));
        for s in &self.sparks {
            let color = if s.gold { GOLD } else { Color32::from_rgb(0xff, 0x6a, 0x4a) };
            painter.circle_filled(s.at, s.size * 2.2, color.gamma_multiply(0.15 * s.life));
            painter.circle_filled(s.at, s.size * s.life.max(0.3), color.lerp_to_gamma(Color32::WHITE, 0.4).gamma_multiply(s.life));
        }
        // Lumos: a ring of light spreading from the click.
        if let Some((p, when)) = self.cast {
            let k = ((now - when) / 0.45) as f32;
            if k < 1.0 {
                painter.circle_stroke(p, 4.0 + k * 26.0, Stroke::new(2.0 * (1.0 - k), GOLD.gamma_multiply(1.0 - k)));
                painter.circle_filled(p, 10.0 * (1.0 - k), Color32::WHITE.gamma_multiply(0.25 * (1.0 - k)));
            } else {
                self.cast = None;
            }
        }
        if let Some(tip) = wand {
            ctx.set_cursor_icon(egui::CursorIcon::None);
            paint_wand(&painter, tip, now);
        }
        if !self.sparks.is_empty() || self.cast.is_some() {
            ctx.request_repaint();
        }
    }
}

/// The wand, its tip on `tip`, the handle down and to the right: dark wood tapering to the tip, the
/// handle ridged, the tip glowing.
fn paint_wand(painter: &egui::Painter, tip: Pos2, now: f64) {
    let dir = Vec2::new(0.5, 1.0).normalized();
    let n = dir.rot90();
    let len = 30.0;
    let end = tip + dir * len;
    // The shaft, tapering: a dark outline, then the wood.
    let shaft = |w0: f32, w1: f32, color: Color32| {
        painter.add(egui::Shape::convex_polygon(vec![tip + n * w0, end + n * w1, end - n * w1, tip - n * w0], color, Stroke::NONE));
    };
    shaft(1.6, 3.2, Color32::from_black_alpha(200));
    shaft(0.9, 2.4, WOOD);
    painter.line_segment([tip + n * 0.3, end + n * 0.9], Stroke::new(0.7, WOOD_LIGHT));
    // The handle: rings, a knob at the end.
    for k in 0..3 {
        let p = tip + dir * (len - 10.0 + k as f32 * 3.0);
        painter.line_segment([p + n * 2.9, p - n * 2.9], Stroke::new(1.4, WOOD_LIGHT));
    }
    painter.circle_filled(end + dir * 1.5, 3.2, WOOD);
    painter.circle_stroke(end + dir * 1.5, 3.2, Stroke::new(0.8, Color32::from_black_alpha(200)));
    // The tip's light, breathing.
    let glow = 0.7 + 0.3 * ((now * 3.0).sin() as f32);
    painter.circle_filled(tip, 6.0, GOLD.gamma_multiply(0.18 * glow));
    painter.circle_filled(tip, 2.2, Color32::from_rgb(0xff, 0xf4, 0xc8).gamma_multiply(glow));
}

/// Picking the theme, `t` seconds in: the screen darkens, golden sparks swirl, the Sorting Hat's words,
/// then the house's name in gold. False once over (2.8 s).
pub(super) fn sorting(painter: &egui::Painter, screen: Rect, t: f32, words: &str) -> bool {
    const LASTS: f32 = 2.8;
    if t > LASTS {
        return false;
    }
    let fade = (t * 4.0).min(1.0).min((LASTS - t) * 3.0).max(0.0);
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha((200.0 * fade) as u8));
    let c = screen.center();
    // Sparks swirling inward.
    for k in 0..60u32 {
        let a = noise(k) * TAU + t * (1.0 + noise(k + 99) * 1.5);
        let r = (0.2 + noise(k + 7) * 0.8) * screen.width().min(screen.height()) * 0.45 * (1.0 - (t / LASTS) * 0.6);
        let p = c + Vec2::angled(a) * r;
        painter.circle_filled(p, 1.0 + noise(k + 3) * 1.6, GOLD.gamma_multiply(fade * (0.4 + 0.6 * noise(k + 5))));
    }
    // The hat's words, then the name, growing in.
    painter.text(c - Vec2::new(0.0, 46.0), Align2::CENTER_CENTER, words, FontId::proportional(17.0), Color32::from_rgb(0xf3, 0xe6, 0xc4).gamma_multiply(fade));
    let k = ((t - 0.9) / 0.4).clamp(0.0, 1.0);
    if k > 0.0 {
        let size = 30.0 + 36.0 * (1.0 - (1.0 - k).powi(3));
        let font = FontId::proportional(size);
        for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
            painter.text(c + Vec2::new(dx, dy + 14.0), Align2::CENTER_CENTER, "GRYFFONDOR !", font.clone(), SCARLET.gamma_multiply(fade * k));
        }
        painter.text(c + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, "GRYFFONDOR !", font, GOLD.gamma_multiply(fade * k));
        // A flash as it lands.
        let flash = (1.0 - (t - 1.3).abs() * 4.0).max(0.0);
        if flash > 0.0 {
            painter.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(0xee, 0xba, 0x30, (40.0 * flash) as u8));
        }
    }
    true
}

pub(super) fn paint_bolt(painter: &egui::Painter, c: Pos2, h: f32) {
    bolt(painter, c, h, GOLD_DARK.lerp_to_gamma(GOLD, 0.6));
}
