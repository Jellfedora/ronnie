//! Game of Thrones' companion: the white direwolf cub trotting along the foot of the screen, sitting
//! down halfway to howl (desktop.rs walks it).

use super::*;

/// What the direwolf cub says when it sits down to howl.
pub(super) const WOLF_HOWL: &str = "Aouuuuh !";

const FUR: Color32 = Color32::from_rgb(0xf4, 0xf6, 0xf9);
const FUR_SHADE: Color32 = Color32::from_rgb(0xc5, 0xcd, 0xd8);
const FUR_LINE: Color32 = Color32::from_rgb(0x8a, 0x95, 0xa5);
const WOLF_EYE: Color32 = Color32::from_rgb(0xd2, 0x2b, 0x2b);
const NOSE: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x30);

/// The direwolf cub, `h` tall (standing, to its ears), its feet at `foot`, facing `dir` (1 right, -1
/// left): trotting at `step` (app time), or sitting, howling (`None`); `now` wags its tail.
pub(super) fn paint_wolf(painter: &egui::Painter, foot: Pos2, h: f32, dir: f32, step: Option<f32>, now: f64) {
    let k = h / 34.0;
    let line = Stroke::new(0.8 * k, FUR_LINE);
    match step {
        Some(step) => {
            let bob = (step * 10.0).sin().abs() * 1.0;
            let at = |x: f32, y: f32| foot + Vec2::new(x * dir, y - bob) * k;
            // The tail, bushy, wagging behind.
            let wag = ((now as f32) * 6.0).sin() * 2.0;
            for i in 0..5 {
                let f = i as f32 / 4.0;
                painter.circle_filled(at(-10.0 - f * 8.0, -17.0 - f * 4.0 + wag * f), (3.2 - f * 1.2) * k, if i == 4 { FUR } else { FUR_SHADE });
            }
            // The legs, in pairs, each pair its own way at each step.
            for (x, phase) in [(-8.0f32, 0.0f32), (-5.5, std::f32::consts::PI), (6.0, std::f32::consts::PI), (8.5, 0.0)] {
                let swing = (step * 10.0 + phase).sin() * 2.6;
                let top = at(x, -12.0);
                let paw = foot + Vec2::new((x + swing) * dir * k, 0.0);
                painter.line_segment([top, paw], Stroke::new(2.6 * k, FUR_SHADE));
                painter.line_segment([paw, paw + Vec2::new(1.6 * dir * k, 0.0)], Stroke::new(2.0 * k, FUR_LINE));
            }
            // The body, its belly lighter.
            painter.add(egui::Shape::ellipse_filled(at(0.0, -16.0), Vec2::new(12.0, 6.5) * k, FUR));
            painter.add(egui::Shape::ellipse_stroke(at(0.0, -16.0), Vec2::new(12.0, 6.5) * k, line));
            // The neck's ruff, the head.
            painter.circle_filled(at(9.5, -20.0), 5.0 * k, FUR);
            head(painter, at(13.0, -25.0), k, dir, false);
        }
        None => {
            let at = |x: f32, y: f32| foot + Vec2::new(x * dir, y) * k;
            // Sitting: the tail along the ground, the haunch, the chest up, the front legs straight.
            let wag = ((now as f32) * 3.0).sin() * 1.0;
            for i in 0..5 {
                let f = i as f32 / 4.0;
                painter.circle_filled(at(-8.0 - f * 9.0, -2.5 + wag * f), (3.0 - f * 1.0) * k, if i == 4 { FUR } else { FUR_SHADE });
            }
            painter.add(egui::Shape::ellipse_filled(at(-3.0, -7.5), Vec2::new(8.0, 7.5) * k, FUR_SHADE));
            painter.add(egui::Shape::ellipse_filled(at(2.0, -16.0), Vec2::new(6.5, 10.0) * k, FUR));
            painter.add(egui::Shape::ellipse_stroke(at(2.0, -16.0), Vec2::new(6.5, 10.0) * k, line));
            for x in [3.5f32, 6.0] {
                let paw = at(x, 0.0);
                painter.line_segment([at(x, -10.0), paw], Stroke::new(2.6 * k, FUR_SHADE));
                painter.line_segment([paw, paw + Vec2::new(1.6 * dir * k, 0.0)], Stroke::new(2.0 * k, FUR_LINE));
            }
            // The head thrown back, the muzzle to the sky.
            head(painter, at(5.0, -29.0), k, dir, true);
        }
    }
}

/// The direwolf's head around `c`: its ears, its muzzle forward (or up to the sky, howling), its red
/// eye.
fn head(painter: &egui::Painter, c: Pos2, k: f32, dir: f32, howling: bool) {
    let at = |x: f32, y: f32| c + Vec2::new(x * dir, y) * k;
    let line = Stroke::new(0.8 * k, FUR_LINE);
    // The ears, laid back when howling.
    let ears = if howling { [(-4.0, -2.0, -8.5, -6.0, -2.0, -5.0), (-1.5, -3.5, -5.0, -8.5, 0.5, -4.5)] } else { [(-3.5, -3.0, -3.0, -9.0, 0.0, -4.0), (-0.5, -4.0, 1.0, -9.5, 2.5, -3.5)] };
    for (ax, ay, bx, by, cx, cy) in ears {
        painter.add(egui::Shape::convex_polygon(vec![at(ax, ay), at(bx, by), at(cx, cy)], FUR_SHADE, line));
    }
    painter.circle_filled(c, 4.6 * k, FUR);
    painter.circle_stroke(c, 4.6 * k, line);
    // The muzzle, its nose at the tip; open when howling.
    let muzzle = if howling { vec![at(1.5, -3.5), at(6.5, -9.5), at(8.0, -8.0), at(4.5, -1.0)] } else { vec![at(2.5, -2.0), at(9.5, -0.5), at(9.0, 2.0), at(2.5, 3.0)] };
    let tip = if howling { at(7.6, -9.4) } else { at(9.6, 0.2) };
    painter.add(egui::Shape::convex_polygon(muzzle, FUR, line));
    painter.circle_filled(tip, 1.1 * k, NOSE);
    if howling {
        painter.line_segment([at(4.8, -4.5), at(7.2, -6.8)], Stroke::new(1.0 * k, NOSE));
        painter.circle_filled(at(0.8, -1.2), 0.9 * k, WOLF_EYE);
    } else {
        painter.circle_filled(at(1.2, -1.0), 0.9 * k, WOLF_EYE);
    }
}

/// The howl's bubble over the sitting cub (its feet at `foot`).
pub(super) fn paint_howl(painter: &egui::Painter, foot: Pos2) {
    let ink = Color32::from_rgb(0x1e, 0x26, 0x33);
    let paper = Color32::from_rgb(0xee, 0xf3, 0xf9);
    let galley = painter.layout_no_wrap(WOLF_HOWL.to_owned(), FontId::proportional(14.0), ink);
    let r = Rect::from_center_size(foot + Vec2::new(0.0, -62.0), galley.size() + Vec2::new(18.0, 10.0));
    painter.rect(r, 8.0, paper, Stroke::new(1.0, FUR_LINE), egui::StrokeKind::Inside);
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(r.center().x - 5.0, r.max.y), Pos2::new(r.center().x + 5.0, r.max.y), Pos2::new(r.center().x, r.max.y + 7.0)], paper, Stroke::NONE));
    painter.galley(r.center() - galley.size() / 2.0, galley, ink);
}
