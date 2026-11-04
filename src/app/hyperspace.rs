//! The Star Wars theme's own touches (unlocked at Larsen): lightsaber beams, a white core in a colored
//! glow (under the profiles' bar, the focused pane's bar, above the settings, over the active tab), the
//! focused pane framed by a blade, the logo in yellow outline among the stars, and at the foot of the
//! sidebar a twinkling starfield, the Death Star, an astromech droid and, now and then, an X-wing chased
//! by a TIE fighter. The guard's window shows the Dark Lord's helmet; picking the theme jumps to
//! hyperspace, after the opening words.

use super::*;
use std::f32::consts::{PI, TAU};

/// What the guard's window says instead of its title.
pub(super) const ALERT: &str = "It's a trap!";

const SW_YELLOW: Color32 = Color32::from_rgb(0xff, 0xe8, 0x1f);
/// The blue of the opening words.
const INTRO_BLUE: Color32 = Color32::from_rgb(0x4b, 0xd5, 0xee);
const HULL: Color32 = Color32::from_rgb(0xd8, 0xdc, 0xe2);
const HULL_DARK: Color32 = Color32::from_rgb(0x7d, 0x84, 0x90);
const STEEL: Color32 = Color32::from_rgb(0x55, 0x5b, 0x66);
const LASER_RED: Color32 = Color32::from_rgb(0xff, 0x3b, 0x30);
const LASER_GREEN: Color32 = Color32::from_rgb(0x4c, 0xff, 0x5a);
const DROID_BLUE: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xd6);

/// A lightsaber beam along `rect` (horizontal): its colored glow filling it, a white core in its middle.
pub(super) fn beam(painter: &egui::Painter, rect: Rect, color: Color32) {
    let y = rect.center().y;
    let h = rect.height().max(2.0);
    for (w, alpha) in [(h, 0.18), (h * 0.66, 0.35), (h * 0.4, 0.8)] {
        painter.hline(rect.x_range(), y, Stroke::new(w, color.gamma_multiply(alpha)));
    }
    painter.hline(rect.x_range(), y, Stroke::new((h * 0.18).max(1.0), color.lerp_to_gamma(Color32::WHITE, 0.75)));
}

/// The focused pane framed by a blade: the theme's color fading towards the inside, a white core on
/// the edge.
pub(super) fn blade(painter: &egui::Painter, r: Rect, theme: &Theme) {
    for (k, alpha) in [0.9, 0.45, 0.24, 0.12, 0.05].into_iter().enumerate() {
        painter.rect_stroke(r.shrink(1.0 + k as f32), 4.0, Stroke::new(1.0, theme.accent.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    }
    painter.rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.0, theme.accent.lerp_to_gamma(Color32::WHITE, 0.7).gamma_multiply(0.8)), egui::StrokeKind::Inside);
}

/// A lightsaber: its hilt from `hilt` along `dir` (a unit vector), then its blade, `len` long when
/// lit, out to `lit` of it (0 to 1: the ignition), `w` thick: the colored glow around a white core,
/// its tip rounded. The hilt: the pommel, the ridged grip, the button, the emitter.
pub(super) fn saber(painter: &egui::Painter, hilt: Pos2, dir: Vec2, len: f32, lit: f32, w: f32, color: Color32) {
    let n = dir.rot90();
    let hilt_len = (w * 5.5).max(14.0);
    let hw = (w * 0.75).max(2.2);
    let quad = |from: f32, to: f32, half: f32, fill: Color32| {
        let (a, b) = (hilt + dir * from, hilt + dir * to);
        painter.add(egui::Shape::convex_polygon(vec![a + n * half, b + n * half, b - n * half, a - n * half], fill, Stroke::NONE));
    };
    // The blade first: the hilt goes over its foot.
    let foot = hilt + dir * hilt_len;
    let tip = foot + dir * len * lit.clamp(0.0, 1.0);
    if lit > 0.0 {
        for (k, alpha) in [(2.6, 0.12), (1.8, 0.25), (1.2, 0.6)] {
            let c = color.gamma_multiply(alpha);
            painter.line_segment([foot, tip], Stroke::new(w * k, c));
            painter.circle_filled(tip, w * k / 2.0, c);
        }
        let core = color.lerp_to_gamma(Color32::WHITE, 0.82);
        painter.line_segment([foot, tip], Stroke::new(w * 0.55, core));
        painter.circle_filled(tip, w * 0.275, core);
    }
    // The pommel, the grip and its ridges, the emitter.
    quad(0.0, hilt_len * 0.12, hw * 1.05, Color32::from_rgb(0x3a, 0x3e, 0x46));
    quad(hilt_len * 0.12, hilt_len * 0.72, hw * 0.9, Color32::from_rgb(0x9a, 0xa1, 0xab));
    for k in 0..5 {
        let at = hilt + dir * (hilt_len * (0.18 + k as f32 * 0.1));
        painter.line_segment([at + n * hw * 0.9, at - n * hw * 0.9], Stroke::new(1.0, Color32::from_rgb(0x1c, 0x1f, 0x24)));
    }
    painter.circle_filled(hilt + dir * hilt_len * 0.62 + n * hw * 0.55, (hw * 0.32).max(0.9), LASER_RED);
    quad(hilt_len * 0.72, hilt_len, hw * 1.1, Color32::from_rgb(0xc9, 0xcf, 0xd8));
    painter.line_segment([hilt + dir * hilt_len * 0.86 + n * hw * 1.1, hilt + dir * hilt_len * 0.86 - n * hw * 1.1], Stroke::new(1.0, Color32::from_rgb(0x55, 0x5b, 0x66)));
}

/// A lightsaber lying along `rect` (horizontal), its hilt on the left, lit out to `lit`.
pub(super) fn saber_along(painter: &egui::Painter, rect: Rect, lit: f32, color: Color32) {
    let w = rect.height().clamp(2.0, 6.0) * 0.6;
    let hilt = Pos2::new(rect.min.x + 4.0, rect.center().y);
    let hilt_len = (w * 5.5).max(14.0);
    saber(painter, hilt, Vec2::X, (rect.width() - hilt_len - 8.0 - w).max(0.0), lit, w, color);
}

/// A duel along `rect`: a blue blade from the left, a red one from the right, crossed in the middle,
/// sparks where they meet (`now`: seconds, for the sparks).
pub(super) fn duel(painter: &egui::Painter, rect: Rect, now: f64, theme: &Theme) {
    let y = rect.center().y;
    let w: f32 = 2.4;
    let hilt_len = (w * 5.5).max(14.0);
    let mid = rect.center().x;
    let reach = mid - rect.min.x - 6.0 - hilt_len + 10.0;
    saber(painter, Pos2::new(rect.min.x + 6.0, y + 1.5), Vec2::new(1.0, -0.035).normalized(), reach, 1.0, w, theme.accent);
    saber(painter, Pos2::new(rect.max.x - 6.0, y + 1.5), Vec2::new(-1.0, -0.035).normalized(), reach, 1.0, w, theme.ansi[1]);
    // Where they meet: a white glow, sparks flying.
    let at = Pos2::new(mid, y + 1.5 - 0.035 * (reach + hilt_len - 6.0));
    painter.circle_filled(at, 7.0, Color32::from_white_alpha(30));
    painter.circle_filled(at, 3.2, Color32::from_white_alpha(170));
    let tick = (now * 12.0) as u32;
    for i in 0..7u32 {
        let s = tick.wrapping_mul(31).wrapping_add(i * 17);
        let a = noise(s) * TAU;
        let d = 3.0 + noise(s + 1) * 9.0;
        let p = at + Vec2::angled(a) * d;
        let c = if noise(s + 2) > 0.5 { SW_YELLOW } else { Color32::WHITE };
        painter.line_segment([p, p + Vec2::angled(a) * 2.5], Stroke::new(1.0, c.gamma_multiply(0.9)));
    }
}

/// The pointer as a lightsaber: its tip on the pointer, the blade up from the hilt (only for the
/// plain arrow).
pub(super) fn pointer(ctx: &egui::Context, theme: &Theme) {
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    if ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default {
        return;
    }
    ctx.set_cursor_icon(egui::CursorIcon::None);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("saber-pointer")));
    let dir = Vec2::new(-0.55, -1.0).normalized();
    let (w, len): (f32, f32) = (2.6, 17.0);
    let hilt_len = (w * 5.5).max(14.0);
    let hilt = at - dir * (len + hilt_len + w * 0.3);
    saber(&painter, hilt, dir, len, 1.0, w, theme.accent);
}

/// Deterministic numbers in [0, 1) (the same stars at each frame).
fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65536.0
}

/// `n` stars scattered in `rect`, some brighter, some tinted, twinkling at `now` (seconds).
fn stars(painter: &egui::Painter, rect: Rect, n: u32, seed: u32, theme: &Theme, fade: f32, now: f64) {
    for i in 0..n {
        let s = seed.wrapping_add(i * 7);
        let at = Pos2::new(rect.min.x + noise(s) * rect.width(), rect.min.y + noise(s + 1) * rect.height());
        let bright = noise(s + 2);
        let twinkle = 0.7 + 0.3 * ((now as f32) * (0.8 + noise(s + 3) * 2.2) + noise(s + 4) * TAU).sin();
        let color = if bright > 0.9 { theme.ansi[6] } else if bright > 0.82 { SW_YELLOW } else { theme.fg };
        let size = if bright > 0.93 { 1.4 } else { 0.8 };
        painter.circle_filled(at, size, color.gamma_multiply((0.25 + bright * 0.6) * fade * twinkle));
        // The brightest ones sparkle: a small cross.
        if bright > 0.96 {
            let c = color.gamma_multiply(0.45 * fade * twinkle);
            painter.hline(at.x - 3.5..=at.x + 3.5, at.y, Stroke::new(0.7, c));
            painter.vline(at.x, at.y - 3.5..=at.y + 3.5, Stroke::new(0.7, c));
        }
    }
}

/// The sidebar's logo among the stars: the name in yellow outline, hollow, as on the films' title.
pub(super) fn logo(painter: &egui::Painter, rect: Rect, theme: &Theme) {
    let field = Rect::from_center_size(rect.center(), Vec2::new(rect.width().min(190.0), rect.height()));
    stars(painter, field, 26, 11, theme, 0.8, 0.0);
    let at = rect.center() - Vec2::new(0.0, 2.0);
    let font = FontId::new(32.0, egui::FontFamily::Name("metal".into()));
    for k in 0..16 {
        let a = k as f32 / 16.0 * TAU;
        painter.text(at + Vec2::angled(a) * 1.6, Align2::CENTER_CENTER, "Ronnie", font.clone(), SW_YELLOW);
    }
    painter.text(at, Align2::CENTER_CENTER, "Ronnie", font, theme.chrome_bg);
}

/// The Death Star, of radius `r`: grey, its dish up on the left, its trench across, its night side.
fn death_star(painter: &egui::Painter, c: Pos2, r: f32) {
    painter.circle_filled(c, r, HULL_DARK);
    painter.circle_filled(c - Vec2::splat(r * 0.12), r * 0.86, HULL_DARK.lerp_to_gamma(HULL, 0.35));
    // Its night side, a crescent on the right.
    for k in 0..6 {
        let t = k as f32 / 6.0;
        painter.circle_stroke(c + Vec2::new(r * 0.05 * t, r * 0.05 * t), r * (1.0 - t * 0.04), Stroke::new(1.0, Color32::from_black_alpha(40)));
    }
    // A few panels.
    for i in 0..9 {
        let y = c.y + (noise(500 + i) - 0.5) * r * 1.5;
        let half = (r * r - (y - c.y).powi(2)).max(0.0).sqrt() * 0.8;
        let x = c.x + (noise(600 + i) - 0.5) * half * 2.0;
        painter.hline(x..=x + r * 0.18, y, Stroke::new(0.8, STEEL.gamma_multiply(0.7)));
    }
    // The trench at its equator.
    painter.hline(c.x - r..=c.x + r, c.y + r * 0.04, Stroke::new((r * 0.06).max(1.0), Color32::from_rgb(0x3c, 0x41, 0x4a)));
    // The superlaser's dish.
    let dish = c + Vec2::new(-r * 0.38, -r * 0.4);
    painter.circle_filled(dish, r * 0.27, STEEL);
    painter.circle_stroke(dish, r * 0.27, Stroke::new(0.8, HULL_DARK.lerp_to_gamma(Color32::BLACK, 0.4)));
    painter.circle_filled(dish + Vec2::splat(r * 0.03), r * 0.07, HULL_DARK.lerp_to_gamma(Color32::BLACK, 0.5));
}

/// An X-wing seen from above, flying towards `dir` (1: right, -1: left), `s` its half-length: the
/// fuselage, the wings opened in an X with their red stripes, a cannon at each tip, the engines lit.
fn xwing(painter: &egui::Painter, c: Pos2, s: f32, dir: f32) {
    let at = |x: f32, y: f32| c + Vec2::new(x * dir, y) * s;
    let poly = |points: &[(f32, f32)], color: Color32| {
        painter.add(egui::Shape::convex_polygon(points.iter().map(|&(x, y)| at(x, y)).collect(), color, Stroke::NONE));
    };
    // The engines' glow behind.
    for y in [-0.2, 0.2] {
        painter.circle_filled(at(-0.78, y), s * 0.12, Color32::from_rgb(0xff, 0x8a, 0x5c).gamma_multiply(0.85));
    }
    // The wings, the cannons along their tips.
    for side in [-1.0, 1.0] {
        poly(&[(-0.5, side * 0.08), (-0.1, side * 0.08), (-0.3, side * 0.72), (-0.62, side * 0.72)], HULL_DARK.lerp_to_gamma(HULL, 0.5));
        poly(&[(-0.42, side * 0.3), (-0.22, side * 0.3), (-0.26, side * 0.44), (-0.48, side * 0.44)], LASER_RED);
        painter.line_segment([at(-0.65, side * 0.74), at(0.35, side * 0.74)], Stroke::new((s * 0.06).max(1.0), HULL));
        painter.line_segment([at(-0.62, side * 0.2), at(-0.85, side * 0.2)], Stroke::new(s * 0.14, STEEL));
    }
    // The fuselage, the nose, the cockpit.
    poly(&[(-0.75, -0.1), (0.5, -0.07), (1.0, 0.0), (0.5, 0.07), (-0.75, 0.1)], HULL);
    poly(&[(0.05, -0.05), (0.25, -0.04), (0.25, 0.04), (0.05, 0.05)], STEEL);
    painter.line_segment([at(0.6, 0.0), at(0.9, 0.0)], Stroke::new(1.0, LASER_RED));
}

/// A TIE fighter as it's always shown, front on (its two hexagonal wings, the ball between), `s` its
/// half-width.
fn tie(painter: &egui::Painter, c: Pos2, s: f32) {
    let wing = |side: f32| {
        let x = c.x + side * s * 0.85;
        let points: Vec<Pos2> = (0..6).map(|k| {
            let a = k as f32 / 6.0 * TAU;
            Pos2::new(x + a.cos() * s * 0.16, c.y + a.sin() * s * 0.9)
        }).collect();
        painter.add(egui::Shape::convex_polygon(points.clone(), Color32::from_rgb(0x2a, 0x2e, 0x36), Stroke::new(1.0, HULL_DARK)));
        painter.vline(x, c.y - s * 0.8..=c.y + s * 0.8, Stroke::new(0.8, STEEL));
    };
    wing(-1.0);
    wing(1.0);
    painter.hline(c.x - s * 0.8..=c.x + s * 0.8, c.y, Stroke::new((s * 0.12).max(1.2), HULL_DARK));
    painter.circle_filled(c, s * 0.3, HULL_DARK.lerp_to_gamma(HULL, 0.3));
    painter.circle_filled(c, s * 0.16, Color32::from_rgb(0x18, 0x1b, 0x22));
    painter.circle_stroke(c, s * 0.16, Stroke::new(0.8, STEEL));
}

/// An astromech droid standing on `foot`, `h` tall: its silver dome with blue panels and its eye, its
/// white body with blue panels, its two legs; its light blinks red and blue, it rocks now and then.
fn droid(painter: &egui::Painter, foot: Pos2, h: f32, now: f64) {
    let k = h / 34.0;
    // Every six seconds, a little dance.
    let phase = (now % 6.0) as f32;
    let rock = if phase < 0.8 { (phase * 4.0 * PI).sin() * 1.2 * k } else { 0.0 };
    let c = foot + Vec2::new(rock, 0.0);
    let body = Rect::from_min_max(Pos2::new(c.x - 8.0 * k, c.y - 24.0 * k), Pos2::new(c.x + 8.0 * k, c.y - 4.0 * k));
    // The legs, behind the body.
    for side in [-1.0, 1.0] {
        let x = c.x + side * 10.0 * k;
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(x - 2.5 * k, c.y - 21.0 * k), Pos2::new(x + 2.5 * k, c.y - 21.0 * k), Pos2::new(x + 3.0 * k, c.y), Pos2::new(x - 3.0 * k, c.y)], HULL, Stroke::new(0.8, HULL_DARK)));
        painter.rect_filled(Rect::from_min_size(Pos2::new(x - 1.5 * k, c.y - 15.0 * k), Vec2::new(3.0 * k, 6.0 * k)), 1.0, DROID_BLUE);
    }
    painter.rect_filled(body, 1.5, HULL);
    painter.rect_stroke(body, 1.5, Stroke::new(0.8, HULL_DARK), egui::StrokeKind::Inside);
    for (x, y, w, hh) in [(-6.0, -21.0, 5.0, 7.0), (1.5, -21.0, 4.5, 4.0), (1.5, -15.5, 4.5, 6.0), (-6.0, -12.0, 5.0, 4.0)] {
        painter.rect_filled(Rect::from_min_size(c + Vec2::new(x, y) * k, Vec2::new(w, hh) * k), 0.8, DROID_BLUE);
    }
    // The center foot.
    painter.rect_filled(Rect::from_min_max(Pos2::new(c.x - 3.0 * k, c.y - 4.0 * k), Pos2::new(c.x + 3.0 * k, c.y)), 1.0, HULL_DARK);
    // The dome: a half disc.
    let dome_c = Pos2::new(c.x, body.min.y + 0.5);
    let r = 8.6 * k;
    let dome: Vec<Pos2> = (0..=16).map(|i| dome_c + Vec2::angled(PI + PI * i as f32 / 16.0) * r).collect();
    painter.add(egui::Shape::convex_polygon(dome, Color32::from_rgb(0xc4, 0xcb, 0xd6), Stroke::new(0.8, HULL_DARK)));
    painter.rect_filled(Rect::from_min_size(dome_c + Vec2::new(-6.0, -4.0) * k, Vec2::new(4.0, 3.0) * k), 0.5, DROID_BLUE);
    painter.rect_filled(Rect::from_min_size(dome_c + Vec2::new(2.5, -4.0) * k, Vec2::new(3.5, 3.0) * k), 0.5, DROID_BLUE);
    // Its eye, its light.
    painter.circle_filled(dome_c + Vec2::new(-0.5, -5.5) * k, 1.9 * k, Color32::from_rgb(0x10, 0x12, 0x18));
    let light = if (now * 1.4) as i64 % 2 == 0 { LASER_RED } else { Color32::from_rgb(0x4f, 0x9d, 0xff) };
    painter.circle_filled(dome_c + Vec2::new(4.0, -6.5) * k, 1.1 * k, light);
    painter.circle_filled(dome_c + Vec2::new(4.0, -6.5) * k, 2.4 * k, light.gamma_multiply(0.25));
}

/// Where the droid stands at the foot of the starfield in `rect` (to hover it).
pub(super) fn droid_rect(rect: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(rect.min.x + 8.0, rect.max.y - 40.0), Pos2::new(rect.min.x + 40.0, rect.max.y - 4.0))
}

/// How often the X-wing passes, how long it takes.
const PASS_EVERY: f64 = 26.0;
const PASS_LASTS: f64 = 4.5;

/// The starfield at the foot of `rect` (the sidebar's panel), thinning out upwards, the Death Star in it,
/// the droid below, the chase now and then. When to paint again (seconds).
pub(super) fn starfield(painter: &egui::Painter, rect: Rect, theme: &Theme, now: f64) -> f32 {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let thirds = rect.height() / 3.0;
    for (k, (n, fade)) in [(8, 0.35), (14, 0.6), (22, 0.9)].into_iter().enumerate() {
        let band = Rect::from_min_size(Pos2::new(rect.min.x, rect.min.y + thirds * k as f32), Vec2::new(rect.width(), thirds));
        stars(&painter, band, n, 101 + k as u32 * 53, theme, fade, now);
    }
    death_star(&painter, Pos2::new(rect.min.x + rect.width() * 0.76, rect.max.y - rect.height() * 0.45), 16.0);
    let foot = droid_rect(rect);
    droid(&painter, Pos2::new(foot.center().x, foot.max.y), 30.0, now);
    // The chase: an X-wing, a TIE behind it shooting, one way then the other.
    let into = (now + 8.0) % PASS_EVERY;
    if into < PASS_LASTS {
        let dir = if ((now + 8.0) / PASS_EVERY) as i64 % 2 == 0 { 1.0 } else { -1.0 };
        let p = (into / PASS_LASTS) as f32;
        let (from, to) = (rect.min.x - 40.0, rect.max.x + 100.0);
        let x_at = |p: f32| if dir > 0.0 { from + (to - from) * p } else { to - (to - from) * p };
        let y = rect.min.y + rect.height() * 0.32;
        let xw = Pos2::new(x_at(p), y + (p * 9.0).sin() * 6.0);
        let tie_at = Pos2::new(x_at(p) - dir * 58.0, y + (p * 7.0 + 1.0).sin() * 4.0);
        // Its green bolts, one every half second, fast.
        for k in 0..3 {
            let fired = (into - (into % 0.5)) - k as f64 * 0.5;
            let age = (into - fired) as f32;
            if fired < 0.0 {
                continue;
            }
            let from_x = x_at((fired / PASS_LASTS) as f32) - dir * 58.0;
            let bx = from_x + dir * (14.0 + age * 260.0);
            painter.line_segment([Pos2::new(bx, tie_at.y), Pos2::new(bx + dir * 10.0, tie_at.y)], Stroke::new(1.6, LASER_GREEN));
        }
        xwing(&painter, xw, 15.0, dir);
        tie(&painter, tie_at, 9.0);
        return 1.0 / 40.0;
    }
    0.2
}

/// The Dark Lord's helmet in a `size` square: its black dome, the flared sides, the mask, the eyes,
/// the grille; rimmed in red to stand out on the dark.
pub(super) fn helmet(painter: &egui::Painter, rect: Rect) {
    let c = rect.center();
    let s = rect.width() / 2.0;
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let black = Color32::from_rgb(0x0c, 0x0d, 0x10);
    let poly = |points: &[(f32, f32)], color: Color32, stroke: Stroke| {
        painter.add(egui::Shape::convex_polygon(points.iter().map(|&(x, y)| at(x, y)).collect(), color, stroke));
    };
    let rim = Stroke::new(1.0, LASER_RED.gamma_multiply(0.8));
    // The dome.
    painter.circle_filled(at(0.0, -0.2), s * 0.66, black);
    painter.circle_stroke(at(0.0, -0.2), s * 0.66, rim);
    // The flared sides, down to the shoulders.
    poly(&[(-0.66, -0.2), (-0.2, -0.2), (-0.32, 0.7), (-0.92, 0.82)], black, rim);
    poly(&[(0.2, -0.2), (0.66, -0.2), (0.92, 0.82), (0.32, 0.7)], black, rim);
    // The mask, the eyes, the nose, the grille.
    poly(&[(-0.4, -0.05), (0.4, -0.05), (0.16, 0.88), (-0.16, 0.88)], Color32::from_rgb(0x26, 0x29, 0x2f), Stroke::NONE);
    poly(&[(-0.38, -0.02), (-0.06, 0.02), (-0.08, 0.22), (-0.34, 0.16)], black, Stroke::NONE);
    poly(&[(0.06, 0.02), (0.38, -0.02), (0.34, 0.16), (0.08, 0.22)], black, Stroke::NONE);
    painter.line_segment([at(-0.3, 0.06), at(-0.14, 0.08)], Stroke::new(1.0, Color32::from_gray(120)));
    painter.line_segment([at(0.14, 0.08), at(0.3, 0.06)], Stroke::new(1.0, Color32::from_gray(120)));
    poly(&[(-0.05, 0.22), (0.05, 0.22), (0.07, 0.42), (-0.07, 0.42)], Color32::from_rgb(0x3a, 0x3e, 0x46), Stroke::NONE);
    poly(&[(-0.2, 0.5), (0.2, 0.5), (0.0, 0.86)], STEEL, Stroke::NONE);
    for x in [-0.08, 0.0, 0.08] {
        painter.line_segment([at(x, 0.54), at(x * 0.5, 0.74)], Stroke::new(0.8, black));
    }
    // A glint on the dome.
    painter.circle_filled(at(-0.28, -0.52), s * 0.08, Color32::from_white_alpha(70));
}

/// The jump to hyperspace over the whole window, `t` seconds in: the opening words in blue, then the
/// stars stretch into streaks, a flash, and it clears. Returns false once it is over.
pub(super) fn jump(painter: &egui::Painter, screen: Rect, t: f32, intro: &str) -> bool {
    const WORDS_END: f32 = 3.0;
    const STREAKS_END: f32 = 4.7;
    const END: f32 = 5.2;
    if t >= END {
        return false;
    }
    let ease = |a: f32, b: f32| ((t - a) / (b - a)).clamp(0.0, 1.0);
    let veil = ease(0.0, 0.35) * (1.0 - ease(STREAKS_END, END));
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha((veil * 245.0) as u8));
    let c = screen.center();
    let reach = screen.size().length() / 2.0;
    // The stars, still, then streaking out from the middle, faster and faster.
    let speed = ease(WORDS_END, STREAKS_END).powi(2);
    for i in 0..220u32 {
        let a = noise(9000 + i) * TAU;
        let d = (0.08 + noise(9100 + i) * 0.92) * reach;
        let dir = Vec2::angled(a);
        let p = c + dir * d;
        let len = speed * reach * (0.25 + noise(9200 + i) * 0.9);
        let color = Color32::WHITE.lerp_to_gamma(INTRO_BLUE, noise(9300 + i) * 0.6).gamma_multiply(veil * (0.4 + noise(9400 + i) * 0.6));
        if len < 1.0 {
            painter.circle_filled(p, 0.9, color);
        } else {
            painter.line_segment([p - dir * len * 0.2, p + dir * len], Stroke::new(1.0 + speed, color));
        }
    }
    // The opening words.
    let words = ease(0.35, 0.9) * (1.0 - ease(WORDS_END - 0.5, WORDS_END));
    if words > 0.0 {
        let galley = painter.layout(intro.to_owned(), FontId::proportional(26.0), INTRO_BLUE.gamma_multiply(words), (screen.width() * 0.6).min(620.0));
        painter.galley(c - galley.size() / 2.0, galley, INTRO_BLUE);
    }
    // The flash, as it goes.
    let flash = ease(STREAKS_END - 0.25, STREAKS_END) * (1.0 - ease(STREAKS_END, END));
    if flash > 0.0 {
        painter.rect_filled(screen, 0.0, Color32::from_white_alpha((flash * 200.0) as u8));
    }
    true
}
