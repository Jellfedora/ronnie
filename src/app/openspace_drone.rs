//! Open Space's rewards outside the game: the delivery drone flying along the foot of the screen now
//! and then (50 campaign stars), dropping a parcel under a small parachute that lands and pops. Drawn
//! by the companions (`super::desktop`), over every screen or in Ronnie's window, and on the rewards'
//! page.

use super::*;

/// The campaign's stars for the drone.
pub(super) const DRONE_STARS: u64 = 50;
/// The boss's office level for the « Pare-feu » frame.
pub(super) const FRAME_HQ: u8 = crate::openspace::HQ_MAX;

fn stars(s: &crate::config::Settings) -> u64 {
    s.openspace.as_ref().map_or(0, |o| o.campaign.iter().map(|&n| n as u64).sum())
}

/// The campaign's stars won, toward the drone.
pub(super) fn drone_progress(s: &crate::config::Settings) -> (u32, u32) {
    (stars(s).min(DRONE_STARS) as u32, DRONE_STARS as u32)
}

pub(super) fn drone_unlocked(s: &crate::config::Settings) -> bool {
    !crate::config::OFFICIAL || stars(s) >= DRONE_STARS
}

/// The boss's office's level, toward the frame.
pub(super) fn hq_level(s: &crate::config::Settings) -> u8 {
    s.openspace.as_ref().map_or(0, |o| o.hq_level())
}

const BODY: Color32 = Color32::from_rgb(0x3a, 0x40, 0x4c);
const ARM: Color32 = Color32::from_rgb(0x55, 0x5d, 0x6b);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x8a, 0x2a);
const BOX: Color32 = Color32::from_rgb(0xc8, 0x94, 0x5a);
const TAPE: Color32 = Color32::from_rgb(0xe8, 0xd2, 0x8a);

/// A parcel, `s` wide, its bottom at `foot`.
fn parcel(painter: &egui::Painter, foot: Pos2, s: f32) {
    let r = Rect::from_min_max(foot - Vec2::new(s / 2.0, s * 0.8), foot + Vec2::new(s / 2.0, 0.0));
    painter.rect_filled(r, s * 0.08, BOX);
    painter.rect_stroke(r, s * 0.08, Stroke::new((s * 0.06).max(0.8), Color32::from_rgb(0x8a, 0x5e, 0x30)), egui::StrokeKind::Inside);
    painter.rect_filled(Rect::from_center_size(r.center(), Vec2::new(s * 0.16, r.height())), 0.0, TAPE);
    painter.text(r.center() + Vec2::new(s * 0.26, 0.0), Align2::CENTER_CENTER, "!", FontId::proportional(s * 0.42), Color32::from_rgb(0xd0, 0x30, 0x30));
}

/// The drone, centered at `c`, `s` wide, flying (`now`: the rotors, the sway), facing `dir`, a parcel
/// under it or not.
pub(super) fn paint_drone(painter: &egui::Painter, c: Pos2, s: f32, dir: f32, now: f64, carrying: bool) {
    let sway = (now * 2.3).sin() as f32 * s * 0.03;
    let tilt = 0.08 * dir;
    let c = c + Vec2::new(0.0, sway);
    let at = |x: f32, y: f32| c + Vec2::new(x * s + y * s * tilt, y * s - x * s * tilt);
    if carrying {
        // The parcel on two strings.
        let top = at(0.0, 0.42);
        painter.line_segment([at(-0.08, 0.1), top + Vec2::new(-s * 0.1, 0.0)], Stroke::new(1.0, Color32::from_gray(60)));
        painter.line_segment([at(0.08, 0.1), top + Vec2::new(s * 0.1, 0.0)], Stroke::new(1.0, Color32::from_gray(60)));
        parcel(painter, top + Vec2::new(0.0, s * 0.3), s * 0.36);
    }
    // The arms, the rotors spinning (a blurred disc and a blade turning), the body, its eye.
    for x in [-0.42f32, 0.42] {
        painter.line_segment([at(0.0, 0.0), at(x, -0.12)], Stroke::new(s * 0.06, ARM));
        let hub = at(x, -0.16);
        painter.add(egui::Shape::convex_polygon(
            (0..16).map(|k| {
                let a = k as f32 / 16.0 * std::f32::consts::TAU;
                hub + Vec2::new(a.cos() * s * 0.24, a.sin() * s * 0.05)
            }).collect(),
            Color32::from_white_alpha(50),
            Stroke::new(0.8, Color32::from_white_alpha(90)),
        ));
        let spin = (now * 40.0 + x as f64 * 3.0).sin() as f32;
        painter.line_segment([hub - Vec2::new(spin * s * 0.22, 0.0), hub + Vec2::new(spin * s * 0.22, 0.0)], Stroke::new(s * 0.03, Color32::from_gray(40)));
        painter.rect_filled(Rect::from_center_size(hub + Vec2::new(0.0, s * 0.03), Vec2::new(s * 0.06, s * 0.08)), 1.0, ARM);
    }
    let body = Rect::from_center_size(at(0.0, 0.0), Vec2::new(s * 0.46, s * 0.2));
    painter.rect_filled(body, s * 0.08, BODY);
    painter.rect_filled(Rect::from_min_size(body.min, Vec2::new(body.width(), s * 0.06)), egui::CornerRadius { nw: (s * 0.08) as u8, ne: (s * 0.08) as u8, sw: 0, se: 0 }, ORANGE);
    let eye = body.center() + Vec2::new(s * 0.12 * dir, s * 0.02);
    painter.circle_filled(eye, s * 0.05, Color32::from_rgb(0x20, 0x24, 0x2c));
    painter.circle_filled(eye, s * 0.025, Color32::from_rgb(0x4f, 0xc3, 0xff));
    // A blinking light.
    if (now * 2.0) % 1.0 < 0.3 {
        painter.circle_filled(at(-0.2 * dir, -0.07), s * 0.025, Color32::from_rgb(0xff, 0x40, 0x40));
    }
}

/// How often the drone flies by, how long its whole pass takes (the parcel landed and popped included).
const DRONE_EVERY: f64 = 600.0;
const DRONE_RUN: f64 = 9.0;
/// Its crossing, within the pass; when it lets go of the parcel; the parcel's fall.
const CROSS: f64 = 8.0;
const DROP: f64 = 3.6;
const FALL: f64 = 1.8;

/// Where the drone is at `now`: the seconds into its pass, rightward; out of sight, the seconds before it
/// comes.
pub(super) fn drone_at(now: f64) -> Result<(f64, bool), f32> {
    // Mostly not at the other companions' time.
    let t = (now + 130.0) % DRONE_EVERY;
    if t > DRONE_RUN {
        return Err((DRONE_EVERY - t) as f32);
    }
    Ok((t, ((now + 130.0) / DRONE_EVERY).floor() as i64 % 2 == 0))
}

/// Which pass it is (the same all through one).
pub(super) fn drone_lap(now: f64) -> f64 {
    ((now + 130.0) / DRONE_EVERY).floor()
}

/// The parcel falling under its parachute from `from` to the ground `ground`, `k` (0 to 1) along; then
/// landed and popping, `pop` (0 to 1) along.
fn parcel_fall(painter: &egui::Painter, x: f32, from: f32, ground: f32, k: f32, pop: Option<f32>, s: f32) {
    if let Some(pop) = pop {
        // Landed: it bursts into confetti and a puff.
        let foot = Pos2::new(x, ground);
        if pop < 0.25 {
            parcel(painter, foot, s * 0.36);
        }
        let colors = [Color32::from_rgb(0xff, 0x5a, 0x5a), Color32::from_rgb(0xff, 0xd3, 0x4f), Color32::from_rgb(0x4f, 0xc3, 0xff), Color32::from_rgb(0x8f, 0xd1, 0x6a), Color32::from_rgb(0xc7, 0x9b, 0xff)];
        let fade = (1.0 - pop).clamp(0.0, 1.0);
        painter.circle_filled(foot - Vec2::new(0.0, s * 0.15), s * 0.5 * pop.sqrt(), Color32::from_white_alpha((fade * 120.0) as u8));
        for k in 0..14 {
            let a = -std::f32::consts::PI * (0.1 + 0.8 * k as f32 / 13.0);
            let d = s * (0.4 + 1.1 * pop) * (0.7 + 0.3 * ((k * 7) % 5) as f32 / 4.0);
            let p = foot + Vec2::angled(a) * d + Vec2::new(0.0, pop * pop * s * 0.8);
            painter.rect_filled(Rect::from_center_size(p, Vec2::splat(s * 0.07)), 1.0, colors[k % colors.len()].gamma_multiply(fade));
        }
        return;
    }
    let y = from + (ground - from) * k;
    let sway = (k * 9.0).sin() * s * 0.08;
    let foot = Pos2::new(x + sway, y);
    // The parachute, the strings, the parcel.
    let top = foot - Vec2::new(0.0, s * 0.95);
    let canopy: Vec<Pos2> = (0..=12).map(|i| {
        let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
        top + Vec2::new(a.cos() * s * 0.38, a.sin() * s * 0.26)
    }).collect();
    painter.add(egui::Shape::convex_polygon(canopy.clone(), ORANGE, Stroke::new(1.0, Color32::from_rgb(0xb0, 0x50, 0x10))));
    let edge = top + Vec2::new(0.0, 0.0);
    for dx in [-0.38f32, 0.0, 0.38] {
        painter.line_segment([edge + Vec2::new(dx * s, 0.0), foot - Vec2::new(dx * s * 0.25, s * 0.3)], Stroke::new(0.8, Color32::from_gray(70)));
    }
    parcel(painter, foot, s * 0.36);
}

/// The drone flying along above `strip` now and then, one way then the other, dropping its parcel:
/// whether it shows, and when to draw again.
pub(super) fn drone_run(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    let (t, rightward) = match drone_at(now) {
        Ok(at) => at,
        Err(again) => return (false, again),
    };
    let s = 54.0;
    let ground = strip.max.y - 2.0;
    let fly = ground - 96.0;
    let x_at = |t: f64| {
        let along = (t / CROSS).clamp(0.0, 1.0) as f32;
        let x = strip.min.x - 60.0 + (strip.width() + 120.0) * along;
        if rightward { x } else { strip.max.x + strip.min.x - x }
    };
    let dir = if rightward { 1.0 } else { -1.0 };
    if t < CROSS {
        paint_drone(painter, Pos2::new(x_at(t), fly + ((t * 1.7).sin() as f32) * 6.0), s, dir, now, t < DROP);
    }
    if t >= DROP {
        let x = x_at(DROP);
        let k = ((t - DROP) / FALL) as f32;
        let pop = (k >= 1.0).then(|| ((t - DROP - FALL) / 0.8) as f32);
        if pop.is_none_or(|p| p < 1.0) {
            parcel_fall(painter, x, fly + s * 0.72, ground, k.min(1.0), pop, s);
        }
    }
    (true, 0.0)
}

/// The drone hovering in `rect`, dropping a parcel now and then, for the rewards' page.
pub(super) fn drone_preview(painter: &egui::Painter, rect: Rect, now: f64) {
    let s = rect.height() * 0.5;
    let t = now % 4.0;
    let c = Pos2::new(rect.center().x, rect.min.y + s * 0.5);
    paint_drone(painter, c, s, 1.0, now, t < 1.4);
    if t >= 1.4 {
        let k = ((t - 1.4) / 1.6) as f32;
        let pop = (k >= 1.0).then(|| ((t - 3.0) / 0.8) as f32);
        parcel_fall(painter, c.x, c.y + s * 0.72, rect.max.y - 6.0, k.min(1.0), pop, s);
    }
}
