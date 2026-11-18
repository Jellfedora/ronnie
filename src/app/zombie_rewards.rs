//! Lundi matin: what an objective done brings, seen on the field (over everyone): the kitchen blowing
//! up, the distress flare climbing and bursting (its red light then hanging over the floor), the rescue
//! helicopter passing over it, the supplies coming down under parachutes; a golden burst for the
//! others (their reward is said in the banner).

use super::*;
use super::zombie::RESCUE_AT;

/// How long the flare's rocket climbs, then how long its red light hangs over the floor.
const FLARE_CLIMB: f64 = 1.4;
const FLARE_HANG: f64 = 7.0;

/// Additive light (added to what's under it, never darkening).
fn glow(r: f32, g: f32, b: f32) -> Color32 {
    Color32::from_rgba_premultiplied(r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 0)
}

/// Each reward (which, where in the floor, how far, since when) as it is at `now`.
pub(super) fn paint_rewards(painter: &egui::Painter, rewards: &[(String, f32, f32, f32, f64)], at: &dyn Fn(f32, f32) -> Pos2, s: f32, now: f64, field: Rect) {
    for (id, x, y, r, since) in rewards {
        let t = now - since;
        let c = at(*x, *y);
        match id.as_str() {
            "gas" => blast(painter, c, r.max(200.0) * s, t as f32, s, field),
            "flare" => flare(painter, c, t, s, now, field),
            "rescue" => supplies(painter, *x, *y, at, t as f32, s),
            _ => burst(painter, c, t as f32, s),
        }
    }
}

/// The kitchen blowing up: a fireball, the ring of its blast running out to `reach`, embers, a flash.
fn blast(painter: &egui::Painter, c: Pos2, reach: f32, t: f32, s: f32, field: Rect) {
    let k = t / 1.1;
    if !(0.0..1.0).contains(&k) {
        return;
    }
    let left = 1.0 - k;
    if t < 0.15 {
        painter.rect_filled(field, 0.0, glow(120.0, 70.0, 20.0));
    }
    painter.circle_filled(c, reach * (0.25 + 0.5 * k), glow(255.0 * left, 140.0 * left, 30.0 * left));
    painter.circle_filled(c, reach * 0.3 * left, Color32::from_rgba_unmultiplied(255, 240, 200, (230.0 * left) as u8));
    painter.circle_stroke(c, reach * k, Stroke::new(18.0 * s * left, Color32::from_rgba_unmultiplied(255, 170, 60, (200.0 * left) as u8)));
    for m in 0..18 {
        let a = m as f32 / 18.0 * std::f32::consts::TAU + (m % 3) as f32 * 0.2;
        let d = reach * (0.2 + 0.85 * k) * (0.7 + 0.3 * ((m * 7) % 5) as f32 / 5.0);
        painter.circle_filled(c + Vec2::angled(a) * d, (10.0 - 8.0 * k) * s, Color32::from_rgba_unmultiplied(255, 200, 90, (255.0 * left) as u8));
    }
}

/// The flare: its rocket climbing toward us (bigger, higher on the screen, smoke behind), then its
/// burst (sparks falling away) and the red star hanging, its light over the whole floor; the rescue
/// helicopter passing over it at RESCUE_AT.
fn flare(painter: &egui::Painter, c: Pos2, t: f64, s: f32, now: f64, field: Rect) {
    let path = |q: f32| c + Vec2::new((q * 9.0).sin() * 14.0, -q * q * 320.0) * s;
    if t < FLARE_CLIMB {
        let k = (t / FLARE_CLIMB) as f32;
        for m in 0..14 {
            let q = k * m as f32 / 14.0;
            painter.circle_filled(path(q), (6.0 + 10.0 * (k - q)) * s, Color32::from_rgba_unmultiplied(200, 200, 210, (90.0 * (1.0 - (k - q))) as u8));
        }
        let head = path(k);
        painter.circle_filled(head, (16.0 + 30.0 * k) * s, glow(160.0, 30.0, 20.0));
        painter.circle_filled(head, (4.0 + 4.0 * k) * s, Color32::from_rgb(255, 230, 200));
    } else if t < FLARE_CLIMB + FLARE_HANG {
        let h = (t - FLARE_CLIMB) as f32;
        let left = 1.0 - h / FLARE_HANG as f32;
        let star = path(1.0) + Vec2::new(0.0, h * 12.0 * s);
        let sparks = (1.0 - h / 2.2).clamp(0.0, 1.0);
        if sparks > 0.0 {
            for m in 0..30 {
                let a = m as f32 / 30.0 * std::f32::consts::TAU;
                let d = (60.0 + 260.0 * (h / 1.6).min(1.0)) * s * (0.6 + 0.4 * ((m * 13) % 7) as f32 / 7.0);
                let p = star + Vec2::angled(a) * d + Vec2::new(0.0, h * h * 40.0 * s);
                painter.circle_filled(p, 3.0 * s, Color32::from_rgba_unmultiplied(255, 120, 90, (255.0 * sparks) as u8));
            }
        }
        let flicker = 0.85 + 0.15 * (now * 23.0).sin() as f32;
        painter.rect_filled(field, 0.0, glow(40.0 * left * flicker, 0.0, 0.0));
        painter.circle_filled(star, 180.0 * s * left, glow(90.0 * left * flicker, 14.0 * left, 10.0 * left));
        painter.circle_filled(star, 9.0 * s, Color32::from_rgba_unmultiplied(255, 200, 190, (255.0 * left) as u8));
    }
    let pass = (t - RESCUE_AT) as f32;
    if (-2.0..2.0).contains(&pass) {
        heli(painter, c + Vec2::new(pass * 520.0, -140.0) * s, s, now);
    }
}

/// The supplies (floor's five, around the flare) coming down under their parachutes.
fn supplies(painter: &egui::Painter, x: f32, y: f32, at: &dyn Fn(f32, f32) -> Pos2, t: f32, s: f32) {
    let k = t / 1.2;
    if !(0.0..1.0).contains(&k) {
        return;
    }
    for m in 0..5 {
        let a = m as f32 / 5.0 * std::f32::consts::TAU;
        let p = at(x + a.cos() * 90.0, y + a.sin() * 90.0);
        let size = 1.0 + (1.0 - k) * 1.2;
        painter.circle_filled(p, 10.0 * s * k, Color32::from_black_alpha(80));
        let crate_at = p - Vec2::new(0.0, (1.0 - k) * 140.0 * s);
        let canopy = crate_at - Vec2::new(0.0, 26.0 * s * size);
        for side in [-14.0, 14.0] {
            painter.line_segment([crate_at, canopy + Vec2::new(side, 0.0) * s * size], Stroke::new(1.0, Color32::from_white_alpha(180)));
        }
        painter.circle_filled(canopy, 16.0 * s * size, if m % 2 == 0 { Color32::from_rgb(235, 70, 60) } else { Color32::from_rgb(245, 245, 240) });
        painter.rect_filled(Rect::from_center_size(crate_at, Vec2::splat(12.0 * s * size)), 2.0, Color32::from_rgb(120, 96, 60));
    }
}

/// A golden burst where an objective was done.
fn burst(painter: &egui::Painter, c: Pos2, t: f32, s: f32) {
    let k = t / 0.9;
    if !(0.0..1.0).contains(&k) {
        return;
    }
    let left = 1.0 - k;
    painter.circle_stroke(c, (30.0 + 160.0 * k) * s, Stroke::new(6.0 * s * left, Color32::from_rgba_unmultiplied(255, 214, 90, (230.0 * left) as u8)));
    for m in 0..12 {
        let a = m as f32 / 12.0 * std::f32::consts::TAU;
        painter.circle_filled(c + Vec2::angled(a) * (20.0 + 120.0 * k) * s, 4.0 * s * left, Color32::from_rgb(255, 230, 140));
    }
}

/// The rescue helicopter, from above: red and white, its rotor a blur, its searchlight on the ground
/// below it, its shadow, its lights blinking.
fn heli(painter: &egui::Painter, c: Pos2, s: f32, now: f64) {
    let ground = c + Vec2::new(30.0, 140.0) * s;
    painter.circle_filled(ground, 120.0 * s, glow(70.0, 68.0, 52.0));
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-6.0, 0.0) * s, c + Vec2::new(6.0, 0.0) * s, ground + Vec2::new(90.0, 0.0) * s, ground + Vec2::new(-90.0, 0.0) * s], glow(26.0, 26.0, 20.0), Stroke::NONE));
    painter.circle_filled(ground + Vec2::new(10.0, 0.0) * s, 60.0 * s, Color32::from_black_alpha(50));
    painter.line_segment([c, c - Vec2::new(110.0, 0.0) * s], Stroke::new(12.0 * s, Color32::from_rgb(200, 40, 36)));
    painter.line_segment([c - Vec2::new(110.0, 18.0) * s, c - Vec2::new(110.0, -18.0) * s], Stroke::new(4.0 * s, Color32::from_rgb(60, 60, 66)));
    let body = (0..24).map(|k| {
        let a = k as f32 / 24.0 * std::f32::consts::TAU;
        c + Vec2::new(a.cos() * 64.0, a.sin() * 34.0) * s
    });
    painter.add(egui::Shape::convex_polygon(body.collect(), Color32::from_rgb(220, 46, 40), Stroke::new(2.0 * s, Color32::from_rgb(250, 250, 245))));
    painter.rect_filled(Rect::from_center_size(c - Vec2::new(8.0, 0.0) * s, Vec2::new(14.0, 50.0) * s), 0.0, Color32::from_rgb(250, 250, 245));
    painter.circle_filled(c + Vec2::new(36.0, 0.0) * s, 20.0 * s, Color32::from_rgb(110, 170, 210));
    painter.circle_filled(c, 120.0 * s, Color32::from_white_alpha(22));
    for k in 0..2 {
        let a = now as f32 * 30.0 + k as f32 * std::f32::consts::FRAC_PI_2;
        painter.line_segment([c - Vec2::angled(a) * 120.0 * s, c + Vec2::angled(a) * 120.0 * s], Stroke::new(5.0 * s, Color32::from_rgba_unmultiplied(40, 40, 46, 150)));
    }
    painter.circle_filled(c, 8.0 * s, Color32::from_rgb(70, 70, 76));
    let blink = (now * 2.0).fract() < 0.3;
    painter.circle_filled(c + Vec2::new(0.0, -34.0) * s, 4.0 * s, if blink { Color32::from_rgb(255, 60, 60) } else { Color32::from_rgb(90, 30, 30) });
    painter.circle_filled(c + Vec2::new(0.0, 34.0) * s, 4.0 * s, if blink { Color32::from_rgb(80, 255, 120) } else { Color32::from_rgb(30, 90, 40) });
}
