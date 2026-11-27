//! The Wall Street looks:
//! - the pointer's trail in bank notes;
//! - the Wall Street theme: a skyline at night at the foot of the sidebar with a stock ticker scrolling,
//!   a ticker tape over the settings and the active tab, the opening bell over a pane when a long
//!   command ends in sight;
//! - the unicorn galloping along the foot of the screen, a rainbow behind it (`super::desktop`).

use super::*;

// --- the bank notes

const NOTE: Color32 = Color32::from_rgb(0x7c, 0xc5, 0x7a);
const NOTE_DARK: Color32 = Color32::from_rgb(0x2f, 0x6b, 0x34);

/// A bank note `w` wide centered at `c`, tilted by `angle`, squashed across by `flip` (1: flat, toward
/// 0: seen edge-on), faded by `alpha`.
pub(super) fn paint_bill(painter: &egui::Painter, c: Pos2, w: f32, angle: f32, flip: f32, alpha: f32) {
    let h = w * 0.5 * flip;
    let (cos, sin) = (angle.cos(), angle.sin());
    let at = |x: f32, y: f32| c + Vec2::new(x * cos - y * sin, x * sin + y * cos);
    let corners = |hw: f32, hh: f32| vec![at(-hw, -hh), at(hw, -hh), at(hw, hh), at(-hw, hh)];
    painter.add(egui::Shape::convex_polygon(corners(w / 2.0, h / 2.0), NOTE.gamma_multiply(alpha), Stroke::new(0.8, NOTE_DARK.gamma_multiply(alpha))));
    if flip > 0.4 {
        // The inner border, the round portrait, the corners' figures.
        painter.add(egui::Shape::closed_line(corners(w / 2.0 - 1.8, h / 2.0 - 1.6), Stroke::new(0.6, NOTE_DARK.gamma_multiply(0.6 * alpha))));
        painter.circle_filled(c, h * 0.3, NOTE_DARK.gamma_multiply(0.55 * alpha));
        for x in [-0.34, 0.34] {
            painter.circle_filled(at(w * x, 0.0), h * 0.12, NOTE_DARK.gamma_multiply(0.45 * alpha));
        }
    }
}

// --- the theme

const GOLD: Color32 = Color32::from_rgb(0xf5, 0xb7, 0x00);
const GREEN: Color32 = Color32::from_rgb(0x19, 0xc3, 0x7d);
const RED: Color32 = Color32::from_rgb(0xf0, 0x45, 0x4d);

/// A small hash, for the windows and the buildings.
fn hash(n: u32) -> u32 {
    let mut x = n.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^ (x >> 13)
}

/// The quotes going by on the ticker.
fn quotes(now: f64) -> Vec<(String, bool)> {
    let mut list = Vec::new();
    // The others move a bit each minute.
    let minute = (now / 60.0) as u32;
    for (k, name) in ["CAC40", "NASDAQ", "DAX", "NIKKEI", "BTC", "OR", "CAFÉ", "PIZZA"].iter().enumerate() {
        let h = hash(minute * 31 + k as u32);
        let up = h % 5 < 3;
        let change = (h >> 8) % 300;
        list.push((format!("{name} {}{}.{:02} %", if up { "▲" } else { "▼" }, change / 100, change % 100), up));
    }
    list
}

/// Wall Street at the foot of the sidebar, in `field`: towers at night, their windows lit and going out,
/// a beacon blinking on the tallest, and over them the ticker scrolling the quotes.
/// Seconds before it is drawn again.
pub(super) fn scene(painter: &egui::Painter, field: Rect, theme: &Theme, now: f64) -> f32 {
    let painter = painter.with_clip_rect(field.intersect(painter.clip_rect()));
    let base = field.max.y - 2.0;
    // The towers, back row dimmer.
    for (row, shade, step) in [(0u32, 0.55, 17.0), (1, 1.0, 23.0)] {
        let mut x = field.min.x - 6.0 + row as f32 * 9.0;
        let mut k = 0u32;
        while x < field.max.x {
            let h = hash(k * 7 + row * 1000);
            let w = step * (0.7 + (h % 40) as f32 / 100.0);
            let tall = if row == 0 { 40.0 + (h >> 6) as f32 % 45.0 } else { 22.0 + (h >> 6) as f32 % 50.0 };
            let tower = Rect::from_min_max(Pos2::new(x, base - tall), Pos2::new(x + w - 2.0, base));
            let body = super::sidebar::lerp_color(theme.chrome_bg, theme.tab_active, 0.9 * shade);
            painter.rect_filled(tower, 0.0, body);
            // Its windows: lit at random, some going on and off by the minute.
            let cols = ((w - 6.0) / 4.0).floor().max(1.0) as u32;
            let rows = ((tall - 6.0) / 5.0).floor().max(1.0) as u32;
            for i in 0..cols {
                for j in 0..rows {
                    let hw = hash(k * 131 + i * 17 + j * 7919 + row * 7 + (now / 20.0) as u32 * ((i + j) % 9 == 0) as u32);
                    if hw.is_multiple_of(3) {
                        let at = Pos2::new(tower.min.x + 3.0 + i as f32 * 4.0, tower.min.y + 4.0 + j as f32 * 5.0);
                        painter.rect_filled(Rect::from_min_size(at, Vec2::new(2.0, 2.5)), 0.0, GOLD.gamma_multiply(0.35 * shade + 0.15));
                    }
                }
            }
            // The tallest's beacon.
            if row == 0 && k == 2 {
                let tip = Pos2::new(tower.center().x, tower.min.y - 9.0);
                painter.line_segment([Pos2::new(tip.x, tower.min.y), tip], Stroke::new(1.0, body));
                let blink = ((now * 1.3) % 1.0) < 0.5;
                painter.circle_filled(tip, 1.8, if blink { RED } else { RED.gamma_multiply(0.25) });
                if blink {
                    painter.circle_filled(tip, 4.0, RED.gamma_multiply(0.15));
                }
            }
            x += w;
            k += 1;
        }
    }
    // The street.
    painter.hline(field.x_range(), base, Stroke::new(1.0, theme.tab_hover));
    // The ticker, over the towers.
    let band = Rect::from_min_max(Pos2::new(field.min.x, field.min.y + 6.0), Pos2::new(field.max.x, field.min.y + 22.0));
    painter.rect_filled(band, 0.0, Color32::from_black_alpha(200));
    painter.hline(band.x_range(), band.min.y, Stroke::new(1.0, GOLD.gamma_multiply(0.5)));
    painter.hline(band.x_range(), band.max.y, Stroke::new(1.0, GOLD.gamma_multiply(0.5)));
    let mut job = egui::text::LayoutJob::default();
    let font = FontId::new(11.0, egui::FontFamily::Name("mono".into()));
    for (text, up) in quotes(now) {
        let color = if up { GREEN } else { RED };
        job.append(&text, 0.0, egui::TextFormat { font_id: font.clone(), color, ..Default::default() });
        job.append("   ·   ", 0.0, egui::TextFormat { font_id: font.clone(), color: theme.text_muted, ..Default::default() });
    }
    let galley = painter.layout_job(job);
    let width = galley.size().x.max(1.0);
    let shift = (now as f32 * 28.0) % width;
    let ticker = painter.with_clip_rect(band.intersect(painter.clip_rect()));
    let y = band.center().y - galley.size().y / 2.0;
    let mut x = band.min.x - shift;
    while x < band.max.x {
        ticker.galley(Pos2::new(x, y), galley.clone(), theme.text);
        x += width;
    }
    1.0 / 30.0
}

/// A ticker tape along `rect` (over the settings, over the active tab): the market's green, a gold edge.
pub(super) fn tape(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, GREEN.gamma_multiply(0.85));
    painter.hline(rect.x_range(), rect.max.y, Stroke::new(1.0, GOLD.gamma_multiply(0.7)));
    // Small marks, as on a tape.
    let mut x = rect.min.x + 6.0;
    while x < rect.max.x - 2.0 {
        painter.vline(x, rect.y_range(), Stroke::new(1.0, Color32::from_black_alpha(60)));
        x += 9.0;
    }
}

/// The opening bell over `pane` when a long command ended (`k`: 1 as it rings, fading to 0): a brass
/// bell swinging at the top, the sound around it, `text` under it.
pub(super) fn bell(painter: &egui::Painter, pane: Rect, k: f32, text: &str) {
    let alpha = (k * 3.0).min(1.0);
    let pivot = Pos2::new(pane.center().x, pane.min.y + 18.0);
    let swing = ((1.0 - k) * 26.0).sin() * 0.45 * k;
    let (cos, sin) = (swing.cos(), swing.sin());
    let at = |x: f32, y: f32| pivot + Vec2::new(x * cos - y * sin, x * sin + y * cos);
    let brass = Color32::from_rgb(0xd9, 0xa9, 0x3f).gamma_multiply(alpha);
    let dark = Color32::from_rgb(0x8a, 0x62, 0x1a).gamma_multiply(alpha);
    // The mount.
    painter.line_segment([pivot - Vec2::new(16.0, 0.0), pivot + Vec2::new(16.0, 0.0)], Stroke::new(3.0, dark));
    // The bell: a dome flaring out.
    let outline: Vec<Pos2> = [(-5.0, 2.0), (5.0, 2.0), (9.0, 8.0), (11.0, 18.0), (16.0, 26.0), (-16.0, 26.0), (-11.0, 18.0), (-9.0, 8.0)].iter().map(|&(x, y)| at(x, y)).collect();
    painter.add(egui::Shape::convex_polygon(outline, brass, Stroke::new(1.2, dark)));
    painter.line_segment([at(-14.0, 22.5), at(14.0, 22.5)], Stroke::new(1.0, dark));
    painter.circle_filled(at(0.0, 30.0), 3.5, dark);
    // The sound, both sides.
    for side in [-1.0f32, 1.0] {
        for ring in 0..3 {
            let r = 22.0 + ring as f32 * 7.0 + (1.0 - k) * 10.0;
            let points: Vec<Pos2> = (0..=8).map(|s| {
                let a = -0.5 + s as f32 / 8.0;
                pivot + Vec2::new(16.0 * side, 14.0) + Vec2::new(a.cos() * side, a.sin()) * r * 0.5
            }).collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.4, GOLD.gamma_multiply(alpha * (0.8 - ring as f32 * 0.22)))));
        }
    }
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(14.0), Color32::WHITE);
    let label = Rect::from_center_size(Pos2::new(pivot.x, pivot.y + 50.0), galley.size() + Vec2::new(20.0, 10.0));
    painter.rect(label, 6.0, Color32::from_rgb(0x05, 0x08, 0x0c).gamma_multiply(0.92 * alpha), Stroke::new(1.0, GOLD.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    painter.galley(label.center() - galley.size() / 2.0, galley, GOLD.gamma_multiply(alpha));
}

// --- the unicorn

/// The points of an ellipse centered at `c`, turned by `rot`.
fn ellipse(c: Pos2, rx: f32, ry: f32, rot: f32) -> Vec<Pos2> {
    let (cos, sin) = (rot.cos(), rot.sin());
    (0..20).map(|k| {
        let a = k as f32 / 20.0 * std::f32::consts::TAU;
        let (x, y) = (a.cos() * rx, a.sin() * ry);
        c + Vec2::new(x * cos - y * sin, x * sin + y * cos)
    }).collect()
}

const RAINBOW: [Color32; 6] = [
    Color32::from_rgb(0xff, 0x5a, 0x5a),
    Color32::from_rgb(0xff, 0xa8, 0x3c),
    Color32::from_rgb(0xff, 0xe0, 0x4a),
    Color32::from_rgb(0x6c, 0xd8, 0x6a),
    Color32::from_rgb(0x5a, 0xb4, 0xff),
    Color32::from_rgb(0xa8, 0x7c, 0xff),
];

/// The unicorn standing on `foot`, `s` high, facing `dir` (1: right, -1: left), galloping (`now`).
pub(super) fn paint_unicorn(painter: &egui::Painter, foot: Pos2, s: f32, dir: f32, now: f64) {
    let p = |x: f32, y: f32| foot + Vec2::new(x * s * dir, y * s);
    let white = Color32::from_rgb(0xfb, 0xf8, 0xff);
    let shade = Color32::from_rgb(0xd8, 0xd0, 0xea);
    let line = Stroke::new((s * 0.03).max(0.8), Color32::from_rgb(0x6a, 0x5a, 0x8a));
    let gait = (now * 9.0) as f32;
    // A bound: the body up and down.
    let bob = gait.sin().abs() * s * 0.05;
    let up = Vec2::new(0.0, -bob);
    // The legs, two by two out of step.
    for (k, (x, phase)) in [(-0.3, 0.0), (-0.22, 3.1), (0.22, 1.6), (0.3, 4.7)].iter().enumerate() {
        let swing = (gait + phase).sin() * 0.45;
        let hip = p(*x, -0.48) + up;
        let knee = hip + Vec2::new(swing.sin() * dir, swing.cos()) * s * 0.22;
        let hoof = knee + Vec2::new((swing * 1.4).sin() * dir * 0.6, 1.0) * s * 0.2;
        let color = if k % 2 == 0 { shade } else { white };
        painter.line_segment([hip, knee], Stroke::new(s * 0.09, color));
        painter.line_segment([knee, hoof], Stroke::new(s * 0.075, color));
        painter.circle_filled(hoof, s * 0.045, Color32::from_rgb(0xc9, 0xa2, 0x4b));
    }
    // The tail, a rainbow waving.
    for (k, color) in RAINBOW.iter().enumerate() {
        let off = k as f32 * 0.035 - 0.09;
        let points: Vec<Pos2> = (0..=8).map(|i| {
            let t = i as f32 / 8.0;
            p(-0.42 - t * 0.38, -0.62 + off + t * 0.18 + (gait * 0.8 - t * 4.0).sin() * 0.06 * t) + up
        }).collect();
        painter.add(egui::Shape::line(points, Stroke::new(s * 0.045, *color)));
    }
    // The body, the neck, the head.
    painter.add(egui::Shape::convex_polygon(ellipse(p(0.0, -0.58) + up, s * 0.45, s * 0.2, 0.0), white, line));
    painter.add(egui::Shape::convex_polygon(vec![p(0.22, -0.72) + up, p(0.4, -1.0) + up, p(0.55, -0.95) + up, p(0.42, -0.55) + up], white, Stroke::NONE));
    let head = p(0.56, -1.0) + up;
    painter.add(egui::Shape::convex_polygon(ellipse(head + Vec2::new(0.06 * s * dir, 0.02 * s), s * 0.17, s * 0.095, 0.45 * dir), white, line));
    painter.circle_filled(head + Vec2::new(0.11 * s * dir, 0.04 * s), s * 0.012 + 0.6, Color32::from_rgb(0x8a, 0x70, 0x90));
    painter.circle_filled(head + Vec2::new(-0.01 * s * dir, -0.02 * s), s * 0.022 + 0.5, Color32::from_rgb(0x2a, 0x20, 0x3a));
    // The ear, the golden horn.
    painter.add(egui::Shape::convex_polygon(vec![head + Vec2::new(-0.08 * s * dir, -0.06 * s), head + Vec2::new(-0.03 * s * dir, -0.08 * s), head + Vec2::new(-0.1 * s * dir, -0.2 * s)], shade, Stroke::NONE));
    let horn = [head + Vec2::new(-0.01 * s * dir, -0.08 * s), head + Vec2::new(0.05 * s * dir, -0.08 * s), head + Vec2::new(0.1 * s * dir, -0.36 * s)];
    painter.add(egui::Shape::convex_polygon(horn.to_vec(), Color32::from_rgb(0xf2, 0xc2, 0x3a), Stroke::new(line.width, Color32::from_rgb(0xb0, 0x80, 0x1a))));
    // The mane, along the neck.
    for (k, color) in RAINBOW.iter().enumerate() {
        let t = k as f32 / 5.0;
        let at = p(0.5 - t * 0.28, -1.05 + t * 0.32) + up;
        painter.circle_filled(at + Vec2::new(-0.03 * s * dir, (gait + k as f32).sin() * s * 0.012), s * 0.065, *color);
    }
    // A glint on the horn, now and then.
    let glint = ((now * 0.7) % 1.0) as f32;
    if glint < 0.25 {
        let at = horn[2];
        let r = s * 0.12 * (1.0 - (glint * 8.0 - 1.0).abs());
        painter.line_segment([at - Vec2::new(r, 0.0), at + Vec2::new(r, 0.0)], Stroke::new(1.0, Color32::WHITE));
        painter.line_segment([at - Vec2::new(0.0, r), at + Vec2::new(0.0, r)], Stroke::new(1.0, Color32::WHITE));
    }
}

/// The rainbow left behind, from `from` to `to` above `ground` (`s`: the unicorn's height), fading toward
/// its start.
fn rainbow(painter: &egui::Painter, from: f32, to: f32, ground: f32, s: f32, now: f64) {
    let n = 24;
    for (k, color) in RAINBOW.iter().enumerate() {
        let points: Vec<Pos2> = (0..=n).map(|i| {
            let t = i as f32 / n as f32;
            let x = from + (to - from) * t;
            Pos2::new(x, ground - s * (0.62 - k as f32 * 0.06) + ((now as f32 * 6.0) - t * 9.0).sin() * s * 0.03)
        }).collect();
        // Fading in pieces: one line per piece.
        for w in points.windows(2).enumerate() {
            let t = w.0 as f32 / n as f32;
            painter.line_segment([w.1[0], w.1[1]], Stroke::new(s * 0.06, color.gamma_multiply(t * t * 0.85)));
        }
    }
}

/// How often the unicorn gallops by, how long it takes.
const UNICORN_EVERY: f64 = 540.0;
const UNICORN_RUN: f64 = 7.0;

/// Where the unicorn is at `now`: how far along (0 to 1), rightward; out of sight, the seconds before it
/// comes.
pub(super) fn unicorn_at(now: f64) -> Result<(f64, bool), f32> {
    // Not at the sheep’s, the droid’s or the house-elf’s time, mostly.
    let t = (now + 20.0) % UNICORN_EVERY;
    if t > UNICORN_RUN {
        return Err((UNICORN_EVERY - t) as f32);
    }
    Ok((t / UNICORN_RUN, ((now + 20.0) / UNICORN_EVERY).floor() as i64 % 2 == 0))
}

/// Which run it is (the same all through one: a screen of its own each time, with several).
pub(super) fn unicorn_lap(now: f64) -> f64 {
    ((now + 20.0) / UNICORN_EVERY).floor()
}

/// The unicorn galloping along `strip` now and then, one way then the other, its rainbow behind it:
/// whether it shows, and when to draw again.
pub(super) fn unicorn_run(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    let (along, rightward) = match unicorn_at(now) {
        Ok(at) => at,
        Err(again) => return (false, again),
    };
    let s = 46.0;
    let x = strip.min.x - 60.0 + (strip.width() + 120.0) * along as f32;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    let ground = strip.max.y - 2.0;
    let back = if rightward { x - s * 0.45 } else { x + s * 0.45 };
    let start = if rightward { (back - 320.0).max(strip.min.x - 60.0) } else { (back + 320.0).min(strip.max.x + 60.0) };
    rainbow(painter, start, back, ground, s, now);
    paint_unicorn(painter, Pos2::new(x, ground), s, if rightward { 1.0 } else { -1.0 }, now);
    (true, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unicorn_comes_by_now_and_then() {
        let shown = (0..5400).filter(|k| unicorn_at(*k as f64 / 10.0).is_ok()).count();
        // 7 s out of 540.
        assert!((65..=75).contains(&shown), "{shown}");
        assert!(unicorn_at(50.0).is_err_and(|again| again > 0.0));
    }
}
