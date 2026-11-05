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
    // A glint running along it now and then (1.4 s every 7), its place along by its own position so
    // that the lines don't all shine at once.
    let ctx = painter.ctx();
    let now = ctx.input(|i| i.time) + (rect.min.x * 0.003 + rect.min.y * 0.002) as f64;
    let into = now % 7.0;
    if into < 1.4 {
        let k = (into / 1.4) as f32;
        let x = rect.min.x - 40.0 + (rect.width() + 80.0) * k;
        let clip = painter.with_clip_rect(rect.expand2(Vec2::new(0.0, 3.0)).intersect(painter.clip_rect()));
        for (w, a) in [(46.0, 0.12), (24.0, 0.3), (10.0, 0.7)] {
            clip.hline(x - w / 2.0..=x + w / 2.0, y, Stroke::new(if w > 30.0 { 4.0 } else { 2.0 }, Color32::from_rgb(0xff, 0xf2, 0xc8).gamma_multiply(a)));
        }
        ctx.request_repaint();
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(7.0 - into));
    }
}

/// A curl of gold from corner `c`: a line along the edge `e` (away from the corner), `n` pointing into
/// the pane, then a small spiral turning inward.
fn curl(painter: &egui::Painter, c: Pos2, e: Vec2, n: Vec2) {
    let stroke = Stroke::new(1.3, GOLD);
    let from = c + n * 2.5 + e * 7.0;
    let to = c + n * 2.5 + e * 17.0;
    painter.line_segment([from, to], stroke);
    let centre = to + n * 3.2;
    let start = (to - centre).normalized();
    // Turning toward the edge's way first, then inward.
    let turn = |a: f32| Vec2::new(start.x * a.cos() - start.y * a.sin(), start.x * a.sin() + start.y * a.cos());
    let sign = if turn(0.3).dot(e) > start.dot(e) { 1.0 } else { -1.0 };
    let pts: Vec<Pos2> = (0..=14).map(|k| {
        let t = k as f32 / 14.0;
        centre + turn(sign * t * 4.4) * 3.2 * (1.0 - t * 0.55)
    }).collect();
    painter.add(egui::Shape::line(pts, stroke));
    painter.circle_filled(from, 1.3, GOLD);
}

/// The focused pane framed in gold, a soft glow inward, a small diamond at each corner.
pub(super) fn frame(painter: &egui::Painter, r: Rect) {
    for (k, alpha) in [0.95, 0.4, 0.2, 0.1, 0.04].into_iter().enumerate() {
        painter.rect_stroke(r.shrink(1.0 + k as f32), 4.0, Stroke::new(1.0, GOLD.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    }
    let r = r.shrink(1.5);
    for (c, sx, sy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        // Two curls from each corner, along both edges, then a diamond on the corner.
        curl(painter, c, Vec2::new(sx, 0.0), Vec2::new(0.0, sy));
        curl(painter, c, Vec2::new(0.0, sy), Vec2::new(sx, 0.0));
        let d = 4.5;
        let c = c + Vec2::new(sx, sy) * 2.0;
        painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, -d), c + Vec2::new(d, 0.0), c + Vec2::new(0.0, d), c + Vec2::new(-d, 0.0)], GOLD, Stroke::NONE));
        painter.circle_filled(c, 1.2, SCARLET);
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
    let font = FontId::proportional(28.0);
    let galley = painter.layout_no_wrap("Ronnie".to_owned(), font.clone(), GOLD);
    // The crest, then the name beside it, the two centered together.
    let h = (rect.height() - 6.0).clamp(30.0, 46.0);
    let shield = Vec2::new(h * 0.82, h);
    let gap = 10.0;
    let left = rect.center().x - (shield.x + gap + galley.size().x) / 2.0;
    crest(painter, Rect::from_min_size(Pos2::new(left, rect.center().y - h / 2.0 - 1.0), shield));
    let at = Pos2::new(left + shield.x + gap, rect.center().y - 2.0);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        painter.text(at + Vec2::new(dx, dy), Align2::LEFT_CENTER, "Ronnie", font.clone(), SCARLET.gamma_multiply(0.8));
    }
    painter.text(at, Align2::LEFT_CENTER, "Ronnie", font, GOLD);
}

/// The house's crest in `r`: a scarlet shield edged in gold, a gold lion rampant on it (facing left,
/// its forepaws up, its tail raised), a lightning bolt over its shoulder.
pub(super) fn crest(painter: &egui::Painter, r: Rect) {
    let p = |x: f32, y: f32| Pos2::new(r.min.x + x * r.width(), r.min.y + y * r.height());
    let u = r.width();
    // The shield: flat on top, its sides straight then curving to a point.
    let outline = |inset: f32| -> Vec<Pos2> {
        let mut pts = vec![p(inset, inset * 0.82), p(1.0 - inset, inset * 0.82)];
        pts.push(p(1.0 - inset, 0.5));
        for k in 1..=10 {
            let a = k as f32 / 10.0 * std::f32::consts::FRAC_PI_2;
            pts.push(p(0.5 + (0.5 - inset) * a.cos(), 0.5 + (0.5 - inset * 0.82) * a.sin()));
        }
        for k in 1..=10 {
            let a = std::f32::consts::FRAC_PI_2 + k as f32 / 10.0 * std::f32::consts::FRAC_PI_2;
            pts.push(p(0.5 + (0.5 - inset) * a.cos(), 0.5 + (0.5 - inset * 0.82) * a.sin()));
        }
        pts
    };
    painter.add(egui::Shape::convex_polygon(outline(-0.03), Color32::from_black_alpha(160), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(outline(0.0), GOLD, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(outline(0.08), Color32::from_rgb(0xa8, 0x1c, 0x22), Stroke::NONE));
    // A sheen on the field's upper left.
    painter.add(egui::Shape::convex_polygon(vec![p(0.1, 0.08), p(0.45, 0.08), p(0.1, 0.42)], Color32::WHITE.gamma_multiply(0.07), Stroke::NONE));

    // The lion, in pieces (each convex): body, legs, tail, mane, head.
    let gold = GOLD;
    let ellipse = |c: Pos2, rx: f32, ry: f32, tilt: f32| -> Vec<Pos2> {
        (0..20).map(|k| {
            let a = k as f32 / 20.0 * TAU;
            let (x, y) = (a.cos() * rx, a.sin() * ry);
            c + Vec2::new(x * tilt.cos() - y * tilt.sin(), x * tilt.sin() + y * tilt.cos())
        }).collect()
    };
    let limb = |from: Pos2, to: Pos2, w: f32| {
        painter.line_segment([from, to], Stroke::new(w, gold));
        painter.circle_filled(to, w * 0.62, gold);
    };
    // The tail, raised behind, its tuft.
    painter.add(egui::Shape::line(vec![p(0.62, 0.72), p(0.78, 0.62), p(0.8, 0.46), p(0.72, 0.36)], Stroke::new(u * 0.045, gold)));
    painter.circle_filled(p(0.71, 0.34), u * 0.055, gold);
    // The hind legs, one forward, one back.
    limb(p(0.55, 0.74), p(0.44, 0.88), u * 0.08);
    limb(p(0.6, 0.72), p(0.66, 0.86), u * 0.08);
    // The body, rearing.
    painter.add(egui::Shape::convex_polygon(ellipse(p(0.53, 0.6), u * 0.13, u * 0.22, -0.45), gold, Stroke::NONE));
    // The forepaws, up and out.
    limb(p(0.45, 0.48), p(0.27, 0.4), u * 0.07);
    limb(p(0.47, 0.55), p(0.3, 0.58), u * 0.07);
    // The mane, then the head and the muzzle.
    painter.add(egui::Shape::convex_polygon(ellipse(p(0.43, 0.32), u * 0.14, u * 0.15, 0.0), Color32::from_rgb(0xd4, 0x96, 0x24), Stroke::NONE));
    painter.circle_filled(p(0.4, 0.31), u * 0.09, gold);
    painter.add(egui::Shape::convex_polygon(vec![p(0.27, 0.32), p(0.34, 0.27), p(0.36, 0.38), p(0.29, 0.37)], gold, Stroke::NONE));
    painter.circle_filled(p(0.37, 0.29), u * 0.018, Color32::from_rgb(0x5a, 0x10, 0x14));
    // An ear.
    painter.add(egui::Shape::convex_polygon(vec![p(0.42, 0.22), p(0.46, 0.17), p(0.48, 0.24)], gold, Stroke::NONE));
}

/// Where the castle stands in the scene `rect` (to hover it).
pub(super) fn castle_rect(rect: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(rect.min.x + rect.width() * 0.18, rect.max.y - 70.0), Pos2::new(rect.min.x + rect.width() * 0.86, rect.max.y - 14.0))
}

/// The castle at night at the foot of `rect` (the sidebar's panel), painted in layers: the sky and its
/// violet haze, stars, the moon in its halo with clouds drifting across, the mountains paler with the
/// distance, the castle on its rock (stone walls lit on the moon's side, cone roofs, battlements, a
/// viaduct, the great hall, arched windows glowing), the lake with the moon's path and the windows'
/// reflections, mist drifting; candles in their halos, gold dust; now and then a shooting star,
/// fireworks, the Snitch, an owl with a letter, a Patronus galloping. When to paint again (seconds).
pub(super) fn scene(painter: &egui::Painter, rect: Rect, now: f64) -> f32 {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let tm = now as f32;
    let (w, x0) = (rect.width(), rect.min.x);
    let shore = rect.max.y - 18.0;
    let base = shore - 10.0;
    // The sky: night blue fading into the sidebar at the top, a violet haze low on the horizon.
    vgrad(&painter, Rect::from_min_max(rect.min, Pos2::new(rect.max.x, shore)), NIGHT.gamma_multiply(0.0), NIGHT);
    vgrad(&painter, Rect::from_min_max(Pos2::new(rect.min.x, base - 60.0), Pos2::new(rect.max.x, shore)), Color32::TRANSPARENT, Color32::from_rgb(0x3a, 0x2c, 0x5a).gamma_multiply(0.55));
    stars(&painter, Rect::from_min_max(rect.min, Pos2::new(rect.max.x, base - 30.0)), 30, 31, now);
    // The moon: its halo, its disc, darker seas on it.
    let moon = Pos2::new(x0 + w * 0.84, rect.min.y + rect.height() * 0.26);
    let moonlight = Color32::from_rgb(0xf4, 0xee, 0xd8);
    glow(&painter, moon, 34.0, moonlight, 0.5);
    painter.circle_filled(moon, 8.5, moonlight);
    for (dx, dy, r) in [(-2.5, -1.5, 2.2), (2.0, 2.5, 1.6), (2.8, -2.6, 1.0), (-1.0, 3.2, 1.1)] {
        painter.circle_filled(moon + Vec2::new(dx, dy), r, Color32::from_rgb(0xd8, 0xd0, 0xb8));
    }
    // Wisps of cloud across the moon, drifting.
    for k in 0..3 {
        let x = x0 + ((tm * (3.0 + k as f32) + k as f32 * 90.0) % (w + 120.0)) - 60.0;
        let y = moon.y - 6.0 + k as f32 * 9.0;
        soft_ellipse(&painter, Pos2::new(x, y), Vec2::new(30.0, 3.5), Color32::from_rgb(0x8a, 0x86, 0xa8), 0.18);
    }

    // A shooting star, every 17 s.
    let shoot = (now + 6.0) % 17.0;
    if shoot < 0.9 {
        let k = (shoot / 0.9) as f32;
        let from = Pos2::new(rect.min.x + rect.width() * 0.15, rect.min.y + rect.height() * 0.08);
        let head = from + Vec2::new(rect.width() * 0.55, rect.height() * 0.22) * k;
        let tail = head - Vec2::new(26.0, 10.5);
        let fade = (1.0 - k).min(k * 5.0);
        painter.line_segment([tail, head], Stroke::new(1.2, Color32::WHITE.gamma_multiply(0.7 * fade)));
        painter.circle_filled(head, 1.6, Color32::WHITE.gamma_multiply(fade));
    }
    // Fireworks over the castle, every 31 s: a rocket up, then a burst in scarlet and gold.
    let fire = (now + 20.0) % 31.0;
    if fire < 2.6 {
        for (k, delay, fx, high) in [(0u32, 0.0f64, 0.38f32, 0.3f32), (1, 0.5, 0.66, 0.2)] {
            let t = (fire - delay) as f32;
            if t < 0.0 {
                continue;
            }
            let base = Pos2::new(rect.min.x + rect.width() * fx, rect.max.y - 40.0);
            let top = Pos2::new(base.x, rect.min.y + rect.height() * high);
            if t < 0.6 {
                let p = base + (top - base) * (t / 0.6);
                painter.circle_filled(p, 1.4, GOLD);
                painter.line_segment([p, p + Vec2::new(0.0, 8.0)], Stroke::new(1.0, GOLD.gamma_multiply(0.4)));
            } else if t < 2.1 {
                let a = (t - 0.6) / 1.5;
                for j in 0..22u32 {
                    let ang = j as f32 / 22.0 * TAU + k as f32;
                    let d = 4.0 + 26.0 * (1.0 - (1.0 - a).powi(2)) * (0.8 + noise(j + k * 40) * 0.4);
                    let p = top + Vec2::angled(ang) * d + Vec2::new(0.0, a * a * 14.0);
                    let color = if j % 2 == 0 { GOLD } else { Color32::from_rgb(0xff, 0x4a, 0x3a) };
                    painter.circle_filled(p, 1.3, color.gamma_multiply(1.0 - a));
                    painter.circle_filled(p, 3.2, color.gamma_multiply(0.15 * (1.0 - a)));
                }
            }
        }
    }

    // The mountains behind, paler with the distance.
    let far: Vec<Pos2> = (0..=24).map(|k| { let t = k as f32 / 24.0; Pos2::new(x0 + w * t, base - 26.0 - 14.0 * (t * 7.0 + 1.0).sin().abs() - 6.0 * (t * 19.0).sin()) }).collect();
    ridge(&painter, &far, shore, Color32::from_rgb(0x2c, 0x28, 0x46));
    let near: Vec<Pos2> = (0..=24).map(|k| { let t = k as f32 / 24.0; Pos2::new(x0 + w * t, base - 8.0 - 10.0 * (t * 5.0 + 2.0).sin().abs()) }).collect();
    ridge(&painter, &near, shore, Color32::from_rgb(0x1c, 0x18, 0x2e));

    // The castle: the rock it stands on, the viaduct to the left, the great hall, the towers.
    let rock: Vec<Pos2> = (0..=16).map(|k| { let t = k as f32 / 16.0; Pos2::new(x0 + w * (0.16 + t * 0.72), base + 2.0 - 8.0 * (t * std::f32::consts::PI).sin() - (t * 23.0).sin() * 1.2) }).collect();
    ridge(&painter, &rock, shore, Color32::from_rgb(0x16, 0x12, 0x20));
    // The viaduct: its deck, its arches' piers, lit along the top.
    let deck_y = base - 6.0;
    painter.rect_filled(Rect::from_min_max(Pos2::new(x0, deck_y), Pos2::new(x0 + w * 0.24, deck_y + 3.0)), 0.0, STONE);
    painter.hline(x0..=x0 + w * 0.24, deck_y, Stroke::new(0.7, moonlight.gamma_multiply(0.25)));
    for k in 0..5 {
        let x = x0 + 4.0 + k as f32 * w * 0.05;
        painter.rect_filled(Rect::from_min_max(Pos2::new(x, deck_y), Pos2::new(x + 2.2, shore)), 0.0, STONE);
    }
    // The great hall: its wall, battlements, its roof lit on the moon's side, its tall arched windows.
    let hall = Rect::from_min_max(Pos2::new(x0 + w * 0.3, base - 24.0), Pos2::new(x0 + w * 0.66, base));
    stone_wall(&painter, hall, moonlight);
    let ridge_y = hall.min.y - 12.0;
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(hall.min.x - 1.0, hall.min.y), Pos2::new(hall.min.x + 6.0, ridge_y), Pos2::new(hall.max.x - 6.0, ridge_y), Pos2::new(hall.max.x + 1.0, hall.min.y)], Color32::from_rgb(0x1e, 0x1a, 0x2c), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(hall.max.x - 6.0, ridge_y), Pos2::new(hall.max.x + 1.0, hall.min.y), Pos2::new(hall.max.x - 14.0, hall.min.y)], Color32::from_rgb(0x34, 0x30, 0x4c), Stroke::NONE));
    painter.hline(hall.min.x + 6.0..=hall.max.x - 6.0, ridge_y, Stroke::new(0.8, moonlight.gamma_multiply(0.35)));
    let mut lit: Vec<(Pos2, f32)> = vec![];
    for k in 0..6 {
        let p = Pos2::new(hall.min.x + 6.0 + k as f32 * (hall.width() - 12.0) / 5.0, hall.center().y + 2.0);
        let flicker = 0.8 + 0.2 * (tm * 2.0 + k as f32 * 1.3).sin();
        arched_window(&painter, p, Vec2::new(2.6, 8.0), flicker);
        lit.push((p, flicker));
    }
    // The towers: (place, width, height, roof's height, battlements instead of a roof).
    let towers: [(f32, f32, f32, f32, bool); 8] = [(0.2, 9.0, 30.0, 13.0, false), (0.27, 7.0, 40.0, 15.0, false), (0.36, 12.0, 52.0, 0.0, true), (0.48, 13.0, 66.0, 24.0, false), (0.57, 8.0, 44.0, 16.0, false), (0.66, 11.0, 56.0, 20.0, false), (0.75, 9.0, 38.0, 0.0, true), (0.83, 7.0, 30.0, 12.0, false)];
    for (k, &(fx, tw, th, roof, flat)) in towers.iter().enumerate() {
        let cx = x0 + w * fx;
        let wall = Rect::from_min_max(Pos2::new(cx - tw / 2.0, base - th), Pos2::new(cx + tw / 2.0, base));
        stone_wall(&painter, wall, moonlight);
        if flat {
            // Battlements.
            let n = (tw / 3.0) as i32;
            for j in 0..=n {
                if j % 2 == 0 {
                    let x = wall.min.x + j as f32 * tw / n as f32;
                    painter.rect_filled(Rect::from_min_size(Pos2::new(x - 1.0, wall.min.y - 2.5), Vec2::new(2.0, 2.5)), 0.0, STONE);
                }
            }
        } else {
            // A cone: its shadowed side, its side lit by the moon, a rim of light, a finial.
            let apex = Pos2::new(cx + tw * 0.05, wall.min.y - roof);
            painter.add(egui::Shape::convex_polygon(vec![Pos2::new(wall.min.x - 1.8, wall.min.y), apex, Pos2::new(cx + 0.5, wall.min.y)], Color32::from_rgb(0x1a, 0x16, 0x28), Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![Pos2::new(cx + 0.5, wall.min.y), apex, Pos2::new(wall.max.x + 1.8, wall.min.y)], Color32::from_rgb(0x3a, 0x34, 0x54), Stroke::NONE));
            painter.line_segment([apex, Pos2::new(wall.max.x + 1.8, wall.min.y)], Stroke::new(0.8, moonlight.gamma_multiply(0.45)));
            painter.line_segment([apex, apex - Vec2::new(0.0, 4.0)], Stroke::new(0.8, Color32::from_rgb(0x8a, 0x80, 0xa0)));
            // A banner on the highest, waving.
            if k == 3 {
                let top = apex - Vec2::new(0.0, 4.0);
                painter.line_segment([top, top - Vec2::new(0.0, 7.0)], Stroke::new(0.8, Color32::from_rgb(0x8a, 0x80, 0xa0)));
                let pts: Vec<Pos2> = (0..=6).map(|j| { let t = j as f32 / 6.0; top - Vec2::new(0.0, 7.0) + Vec2::new(t * 10.0, (tm * 4.0 - t * 3.0).sin() * 1.3 * t) }).collect();
                let mut flag = pts.clone();
                flag.extend(pts.iter().rev().map(|p| *p + Vec2::new(0.0, 3.6 - (p.x - top.x) * 0.12)));
                painter.add(egui::Shape::convex_polygon(flag, SCARLET, Stroke::NONE));
                painter.circle_filled(top - Vec2::new(-4.0, 5.3), 0.9, GOLD);
            }
        }
        // Its windows, some lit, flickering.
        let rows = (th / 11.0) as u32;
        for r in 0..rows {
            let s = k as u32 * 17 + r * 5;
            if noise(s) < 0.3 {
                continue;
            }
            let flicker = 0.7 + 0.3 * (tm * (1.0 + noise(s + 1) * 3.0) + noise(s + 2) * TAU).sin();
            let p = Pos2::new(cx + (noise(s + 3) - 0.5) * tw * 0.3, wall.min.y + 7.0 + r as f32 * 10.0);
            arched_window(&painter, p, Vec2::new(1.8, 3.6), flicker);
            lit.push((p, flicker));
        }
    }

    // The lake: dark water, the moon's path of light, the windows' reflections trembling.
    vgrad(&painter, Rect::from_min_max(Pos2::new(rect.min.x, shore), rect.max), Color32::from_rgb(0x14, 0x16, 0x30), Color32::from_rgb(0x06, 0x08, 0x16));
    painter.hline(rect.x_range(), shore, Stroke::new(0.8, moonlight.gamma_multiply(0.18)));
    for k in 0..9 {
        let y = shore + 2.0 + k as f32 * 1.8;
        let half = 9.0 - k as f32 * 0.5 + (tm * 2.0 + k as f32).sin() * 2.0;
        let x = moon.x + (tm * 1.3 + k as f32 * 0.7).sin() * 1.5;
        painter.hline(x - half..=x + half, y, Stroke::new(0.9, moonlight.gamma_multiply(0.35 - k as f32 * 0.03)));
    }
    for (p, flicker) in &lit {
        let depth = base - p.y;
        let y = shore + 2.0 + depth * 0.25;
        if y < rect.max.y - 1.0 {
            let wobble = (tm * 3.0 + p.x * 0.3).sin() * 1.2;
            painter.hline(p.x - 1.6 + wobble..=p.x + 1.6 + wobble, y, Stroke::new(0.9, WINDOW.gamma_multiply(0.35 * flicker)));
        }
    }
    // Mist drifting over the water and the castle's foot.
    for k in 0..6 {
        let speed = 4.0 + k as f32 * 1.5;
        let x = x0 + ((tm * speed + k as f32 * 70.0) % (w + 160.0)) - 80.0;
        let y = shore - 3.0 + (k % 3) as f32 * 4.0;
        soft_ellipse(&painter, Pos2::new(x, y), Vec2::new(46.0, 5.0), Color32::from_rgb(0xb8, 0xb4, 0xd4), 0.12);
    }

    // Floating candles, bobbing, their flames flickering, each in its halo.
    for k in 0..5u32 {
        let x = x0 + w * (0.1 + noise(k * 3 + 1) * 0.8);
        let y = rect.min.y + rect.height() * (0.12 + noise(k * 3 + 2) * 0.25) + (tm * 0.8 + k as f32).sin() * 3.0;
        painter.rect_filled(Rect::from_min_size(Pos2::new(x - 1.5, y), Vec2::new(3.0, 9.0)), 0.5, Color32::from_rgb(0xf2, 0xe8, 0xd0));
        let flame = Pos2::new(x + (tm * 7.0 + k as f32).sin() * 0.5, y - 2.5);
        glow(&painter, flame, 12.0, WINDOW, 0.9);
        painter.add(egui::Shape::convex_polygon(vec![flame + Vec2::new(0.0, -3.5), flame + Vec2::new(1.5, 0.5), flame + Vec2::new(-1.5, 0.5)], WINDOW, Stroke::NONE));
    }

    // Gold dust drifting up, twinkling.
    for k in 0..14u32 {
        let speed = 6.0 + noise(k + 300) * 8.0;
        let y = rect.max.y - ((tm * speed + noise(k + 310) * rect.height()) % rect.height());
        let x = x0 + noise(k + 320) * w + (tm * 0.7 + k as f32).sin() * 6.0;
        let tw = 0.4 + 0.6 * ((tm * 2.0 + k as f32 * 1.7).sin() * 0.5 + 0.5);
        painter.circle_filled(Pos2::new(x, y), 0.9, GOLD.gamma_multiply(0.7 * tw));
        painter.circle_filled(Pos2::new(x, y), 2.6, GOLD.gamma_multiply(0.1 * tw));
    }
    // A Patronus, a silver stag, galloping across the shore every 45 s.
    let stag_into = (now + 30.0) % 45.0;
    let mut again_stag = false;
    if stag_into < 5.0 {
        let p = (stag_into / 5.0) as f32;
        let dir = if ((now + 30.0) / 45.0) as i64 % 2 == 0 { 1.0 } else { -1.0 };
        let (from, to) = (rect.min.x - 30.0, rect.max.x + 30.0);
        let x = if dir > 0.0 { from + (to - from) * p } else { to - (to - from) * p };
        patronus(&painter, Pos2::new(x, shore - 12.0), dir, tm);
        again_stag = true;
    }

    // The Snitch: 9 s every 21, darting on loops, its wings a blur.
    let into = (now + 4.0) % 21.0;
    let mut again: f32 = 0.12;
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
    // The dust and the candles move all along; the rest when it shows.
    if again_stag || shoot < 0.9 || fire < 2.6 {
        again = 1.0 / 40.0;
    }
    again.min(1.0 / 20.0)
}

/// A Patronus stag at `c` (its body's middle), running `dir`-ward: made of light, motes shimmering
/// along its shape (its body, neck, head, antlers, legs in a gallop), a halo around it, wisps left
/// behind.
fn patronus(painter: &egui::Painter, c: Pos2, dir: f32, tm: f32) {
    let silver = Color32::from_rgb(0xc8, 0xec, 0xff);
    let at = |x: f32, y: f32| c + Vec2::new(x * dir, y);
    glow(painter, c + Vec2::new(0.0, -4.0), 34.0, silver, 0.9);
    // Wisps trailing behind, curling and fading.
    for k in 0..18 {
        let back = k as f32 * 2.6 + (tm * 40.0) % 2.6;
        let p = at(-12.0 - back, (k as f32 * 0.9 - tm * 4.0).sin() * (2.0 + k as f32 * 0.25) - 2.0);
        let fade = 1.0 - k as f32 / 18.0;
        painter.circle_filled(p, 2.4 * fade + 0.4, silver.gamma_multiply(0.08 * fade));
        painter.circle_filled(p, 0.8, silver.gamma_multiply(0.55 * fade));
    }
    // Its shape, as segments to scatter motes along.
    let swing = (tm * 13.0).sin();
    let mut lines: Vec<(Pos2, Pos2, f32)> = vec![];
    for (hip, phase) in [(-7.0f32, 0.0f32), (-4.5, 1.6), (5.5, 3.1), (8.0, 4.7)] {
        let s = (tm * 13.0 + phase).sin();
        let knee = at(hip + s * 3.0, 6.5);
        lines.push((at(hip, 2.0), knee, 0.9));
        lines.push((knee, at(hip + s * 4.5 - 1.5, 12.0), 0.8));
    }
    lines.push((at(7.0, -2.5), at(11.0, -8.5), 1.6));
    lines.push((at(10.0, -10.0), at(16.5, -8.0), 1.2));
    for (a, b) in [((10.5, -10.5), (8.0, -16.0)), ((8.0, -16.0), (4.5, -19.5)), ((8.8, -14.0), (11.5, -18.5)), ((12.0, -10.5), (14.5, -16.5)), ((14.5, -16.5), (18.0, -19.0)), ((13.8, -14.5), (12.0, -18.0))] {
        lines.push((at(a.0, a.1), at(b.0, b.1), 0.6));
    }
    lines.push((at(-10.0, -1.5), at(-13.0, -3.5 + swing), 0.8));
    // The body: motes filling an ellipse.
    for k in 0..46u32 {
        let a = noise(k + 500) * TAU;
        let r = noise(k + 510).sqrt();
        let jitter = Vec2::new((tm * 9.0 + k as f32).sin(), (tm * 7.0 + k as f32 * 1.3).cos()) * 0.5;
        let p = at(a.cos() * r * 10.0, a.sin() * r * 4.6 + swing * 0.4) + jitter;
        let tw = 0.6 + 0.4 * (tm * 6.0 + k as f32).sin();
        painter.circle_filled(p, 1.1, silver.gamma_multiply(0.75 * tw));
    }
    // The limbs, neck, head and antlers: motes along each line, a faint stroke under.
    for (k, (a, b, w)) in lines.iter().enumerate() {
        painter.line_segment([*a, *b], Stroke::new(w * 2.6, silver.gamma_multiply(0.12)));
        let n = ((*b - *a).length() / 1.6) as usize + 1;
        for j in 0..=n {
            let t = j as f32 / n as f32;
            let s = (k * 31 + j) as f32;
            let jitter = Vec2::new((tm * 11.0 + s).sin(), (tm * 8.0 + s * 1.7).cos()) * 0.35;
            painter.circle_filled(*a + (*b - *a) * t + jitter, 0.55 + w * 0.35, silver.gamma_multiply(0.85));
        }
    }
    // An eye, brighter.
    painter.circle_filled(at(12.5, -9.5), 0.9, Color32::WHITE);
}

/// A rect filled from `top` to `bottom`.
fn vgrad(painter: &egui::Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.left_bottom(), bottom);
    mesh.colored_vertex(r.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

/// A skyline `pts` (left to right) filled down to `bottom`.
fn ridge(painter: &egui::Painter, pts: &[Pos2], bottom: f32, color: Color32) {
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
}

/// A soft light: rings fading outward, `r` its reach.
fn glow(painter: &egui::Painter, c: Pos2, r: f32, color: Color32, strength: f32) {
    for (k, a) in [(1.0, 0.04), (0.72, 0.06), (0.5, 0.09), (0.32, 0.14)] {
        painter.circle_filled(c, r * k, color.gamma_multiply(a * strength));
    }
}

/// A soft blur of an ellipse (mist, a wisp of cloud): layers growing fainter outward.
fn soft_ellipse(painter: &egui::Painter, c: Pos2, half: Vec2, color: Color32, strength: f32) {
    for (k, a) in [(1.0, 0.3), (0.75, 0.35), (0.5, 0.35)] {
        let pts: Vec<Pos2> = (0..18).map(|j| { let t = j as f32 / 18.0 * TAU; c + Vec2::new(t.cos() * half.x * k, t.sin() * half.y * k) }).collect();
        painter.add(egui::Shape::convex_polygon(pts, color.gamma_multiply(a * strength), Stroke::NONE));
    }
}

/// A stone wall: dark, courses of stone faintly marked, its right side lit by the moon.
fn stone_wall(painter: &egui::Painter, r: Rect, moonlight: Color32) {
    painter.rect_filled(r, 0.0, STONE);
    let lit = Rect::from_min_max(Pos2::new(r.max.x - (r.width() * 0.3).max(1.5), r.min.y), r.max);
    painter.rect_filled(lit, 0.0, Color32::from_rgb(0x2c, 0x26, 0x3e));
    painter.vline(r.max.x - 0.4, r.y_range(), Stroke::new(0.7, moonlight.gamma_multiply(0.28)));
    let mut y = r.min.y + 3.0;
    let mut k = 0;
    while y < r.max.y {
        painter.hline(r.min.x..=r.max.x, y, Stroke::new(0.5, Color32::BLACK.gamma_multiply(0.25)));
        let off = if k % 2 == 0 { 0.0 } else { 2.0 };
        let mut x = r.min.x + off;
        while x < r.max.x {
            painter.vline(x, y - 3.0..=y, Stroke::new(0.4, Color32::BLACK.gamma_multiply(0.18)));
            x += 4.0;
        }
        y += 3.0;
        k += 1;
    }
}

/// A lit window, arched, `size` its width and height, in a warm halo.
fn arched_window(painter: &egui::Painter, c: Pos2, size: Vec2, flicker: f32) {
    glow(painter, c, size.y * 2.2, WINDOW, 0.9 * flicker);
    let body = Rect::from_center_size(c + Vec2::new(0.0, size.x * 0.25), Vec2::new(size.x, size.y - size.x * 0.5));
    painter.rect_filled(body, 0.0, WINDOW.gamma_multiply(flicker));
    painter.circle_filled(Pos2::new(c.x, body.min.y), size.x / 2.0, WINDOW.gamma_multiply(flicker));
    painter.circle_filled(c, size.x * 0.3, Color32::from_rgb(0xff, 0xf0, 0xc0).gamma_multiply(flicker));
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
    /// Now and then a spell's name rising from a click: which, where, when.
    word: Option<(&'static str, Pos2, f64)>,
}

/// The spells whose name may rise from a click.
const SPELLS: [&str; 10] = ["Lumos !", "Alohomora !", "Wingardium Leviosa !", "Accio café !", "Expecto Patronum !", "Reparo !", "Riddikulus !", "Nox !", "Finite Incantatem !", "Sudo Maxima !"];

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
                // One click in six, a spell's name.
                if self.random() < 1.0 / 6.0 {
                    let k = (self.random() * SPELLS.len() as f32) as usize % SPELLS.len();
                    self.word = Some((SPELLS[k], tip, now));
                }
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
        // The spell's name, rising and fading, gold edged in scarlet.
        if let Some((word, p, when)) = self.word {
            let k = ((now - when) / 1.3) as f32;
            if k < 1.0 {
                let at = p + Vec2::new(14.0, -14.0 - k * 26.0);
                let alpha = (1.0 - k).min(k * 6.0);
                let font = FontId::proportional(15.0);
                for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                    painter.text(at + Vec2::new(dx, dy), Align2::LEFT_CENTER, word, font.clone(), SCARLET.gamma_multiply(alpha));
                }
                painter.text(at, Align2::LEFT_CENTER, word, font, GOLD.gamma_multiply(alpha));
                ctx.request_repaint();
            } else {
                self.word = None;
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

/// The Marauder's Map along the foot of `area` (the terminals) every 70 s: footprints walking across,
/// each one fading after a moment, a parchment tag over the walker with its name.
pub(super) fn footprints(ctx: &egui::Context, area: Rect, name: &str) {
    let now = ctx.input(|i| i.time);
    let into = (now + 25.0) % 70.0;
    const WALK: f64 = 14.0;
    if into > WALK + 2.5 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(70.0 - into));
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("marauder"))).with_clip_rect(area);
    let rightward = ((now + 25.0) / 70.0) as i64 % 2 == 0;
    let ink = Color32::from_rgb(0xe8, 0xd6, 0xa8);
    let step = 16.0;
    let y = area.max.y - 14.0;
    let span = area.width() + 40.0;
    let steps = (span / step) as usize;
    let mut walker = None;
    for k in 0..steps {
        let placed = k as f64 / steps as f64 * WALK;
        let age = (into - placed) as f32;
        if age < 0.0 {
            break;
        }
        let along = area.min.x - 20.0 + k as f32 * step;
        let x = if rightward { along } else { area.max.x + area.min.x - along };
        let side = if k % 2 == 0 { -3.5 } else { 3.5 };
        let p = Pos2::new(x, y + side);
        walker = Some(p);
        let alpha = (1.0 - age / 2.5).max(0.0);
        if alpha <= 0.0 {
            continue;
        }
        let dir = if rightward { 1.0 } else { -1.0 };
        // A sole, then its toes ahead.
        let sole: Vec<Pos2> = (0..12).map(|j| { let a = j as f32 / 12.0 * TAU; p + Vec2::new(a.cos() * 3.6, a.sin() * 1.8) }).collect();
        painter.add(egui::Shape::convex_polygon(sole, ink.gamma_multiply(0.75 * alpha), Stroke::NONE));
        for t in 0..3 {
            painter.circle_filled(p + Vec2::new(dir * 4.8, (t as f32 - 1.0) * 1.3), 0.8, ink.gamma_multiply(0.75 * alpha));
        }
    }
    if let Some(p) = walker.filter(|_| into < WALK) {
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(11.0), Color32::from_rgb(0x3a, 0x24, 0x14));
        let tag = Rect::from_center_size(p - Vec2::new(0.0, 18.0), galley.size() + Vec2::new(12.0, 4.0));
        painter.rect(tag, 3.0, Color32::from_rgb(0xf0, 0xe0, 0xb8), Stroke::new(1.0, Color32::from_rgb(0x8a, 0x60, 0x30)), egui::StrokeKind::Inside);
        painter.galley(tag.center() - galley.size() / 2.0, galley, Color32::BLACK);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(60));
}
