//! The Jurassic Park theme's own touches: yellow and black safety stripes (under the profiles' bar, the
//! focused pane's strip, above the settings), the corners of the focused pane taped off, the logo as the
//! park's (an amber disc ringed in red, a T-rex skeleton, the name across it), ferns at the foot of the
//! sidebar, and Nedry's line when a command is held back.

use super::*;

/// The stripes' colors.
const HAZARD_YELLOW: Color32 = Color32::from_rgb(0xf5, 0xc4, 0x00);
const HAZARD_BLACK: Color32 = Color32::from_rgb(0x14, 0x13, 0x0f);
/// The logo's red and black.
const PARK_RED: Color32 = Color32::from_rgb(0xd7, 0x26, 0x1e);
const PARK_BLACK: Color32 = Color32::from_rgb(0x0c, 0x0b, 0x08);

/// What the guard's window says instead of its title.
pub(super) const MAGIC_WORD: &str = "Ah ah ah, you didn't say the magic word!";

/// Yellow and black stripes filling `rect`, slanted; aligned on the window, so that bands side by side
/// carry on.
pub(super) fn hazard(painter: &egui::Painter, rect: Rect) {
    let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    p.rect_filled(rect, 0.0, HAZARD_BLACK);
    let h = rect.height();
    let w = (h * 1.2).clamp(5.0, 9.0);
    let mut x = (rect.min.x / (2.0 * w)).floor() * 2.0 * w - h;
    while x < rect.max.x {
        let points = vec![Pos2::new(x, rect.max.y), Pos2::new(x + w, rect.max.y), Pos2::new(x + w + h, rect.min.y), Pos2::new(x + h, rect.min.y)];
        p.add(egui::Shape::convex_polygon(points, HAZARD_YELLOW, Stroke::NONE));
        x += 2.0 * w;
    }
}

/// The corners of the focused pane taped off with stripes, and its edge lit faintly.
pub(super) fn fence(painter: &egui::Painter, r: Rect, theme: &Theme) {
    painter.rect_stroke(r.shrink(1.0), 3.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.45)), egui::StrokeKind::Inside);
    let (len, t) = (26.0_f32.min(r.width() / 3.0), 4.0);
    for (corner, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        let along = Rect::from_two_pos(corner, corner + Vec2::new(dx * len, dy * t));
        let down = Rect::from_two_pos(corner, corner + Vec2::new(dx * t, dy * len));
        hazard(painter, along);
        hazard(painter, down);
    }
}

/// The sidebar's logo as the park's: an amber disc ringed in red and black, a T-rex skeleton walking
/// across it, the name in red over it all.
pub(super) fn logo(painter: &egui::Painter, rect: Rect, theme: &Theme) {
    let c = rect.center() - Vec2::new(0.0, 3.0);
    let r = 19.0;
    painter.circle_filled(c, r + 3.5, PARK_BLACK);
    painter.circle_filled(c, r + 2.0, PARK_RED);
    painter.circle_filled(c, r, theme.accent);
    trex(painter, c, r * 0.82, PARK_BLACK, theme.accent);
    super::paint_metal(painter, c + Vec2::new(0.0, 9.0), Align2::CENTER_CENTER, "Ronnie", 28.0, PARK_RED, 1.0);
}

/// A T-rex skeleton walking left, its jaws open, around `c` (`s`: half its size), made of convex pieces;
/// its ribs in `bone`.
fn trex(painter: &egui::Painter, c: Pos2, s: f32, color: Color32, bone: Color32) {
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let piece = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::convex_polygon(points.iter().map(|&(x, y)| at(x, y)).collect(), color, Stroke::NONE));
    };
    // The skull, its lower jaw (the mouth open between them), the neck.
    piece(&[(-0.98, -0.52), (-0.6, -0.78), (-0.36, -0.66), (-0.38, -0.4), (-0.94, -0.4)]);
    piece(&[(-0.9, -0.3), (-0.46, -0.34), (-0.5, -0.2)]);
    piece(&[(-0.44, -0.62), (-0.2, -0.48), (-0.16, -0.26), (-0.42, -0.36)]);
    // The body, the tail raised behind.
    piece(&[(-0.26, -0.48), (0.28, -0.44), (0.48, -0.18), (0.22, 0.14), (-0.18, 0.1), (-0.32, -0.12)]);
    piece(&[(0.36, -0.42), (1.02, -0.66), (0.44, -0.14)]);
    // The legs: the near one (thigh, shin, foot), the far one a step ahead.
    piece(&[(0.0, 0.0), (0.26, 0.04), (0.2, 0.46), (0.02, 0.42)]);
    piece(&[(0.04, 0.4), (0.19, 0.42), (0.13, 0.76), (0.0, 0.76)]);
    piece(&[(-0.14, 0.74), (0.16, 0.72), (0.16, 0.82), (-0.14, 0.82)]);
    piece(&[(-0.16, 0.04), (0.0, 0.08), (-0.12, 0.72), (-0.24, 0.7)]);
    piece(&[(-0.38, 0.7), (-0.1, 0.69), (-0.1, 0.78), (-0.38, 0.79)]);
    // The small arms.
    painter.line_segment([at(-0.24, -0.16), at(-0.36, 0.02)], Stroke::new(s * 0.08, color));
    // Its ribs, in the color of bone.
    for x in [-0.14, -0.02, 0.1, 0.22] {
        painter.line_segment([at(x, -0.36), at(x + 0.04, -0.02)], Stroke::new((s * 0.06).max(1.0), bone.gamma_multiply(0.85)));
    }
    // An eye socket.
    painter.circle_filled(at(-0.62, -0.58), s * 0.06, bone);
}

/// Ferns at the foot of `rect` (the sidebar's panel), faint: the jungle around the park.
pub(super) fn ferns(painter: &egui::Painter, rect: Rect, theme: &Theme) {
    let foot = rect.max.y;
    // Those behind darker, those in front lighter.
    let back = theme.ansi[2].lerp_to_gamma(theme.chrome_bg, 0.82);
    let front = theme.ansi[2].lerp_to_gamma(theme.chrome_bg, 0.68);
    let fronds = [
        (0.04, -1.2, 0.3, back), (0.3, -1.75, 0.36, back), (0.55, -1.45, 0.32, back), (0.8, -2.0, 0.34, back),
        (0.14, -1.45, 0.42, front), (0.42, -2.15, 0.34, front), (0.66, -1.15, 0.38, front), (0.92, -1.75, 0.44, front),
    ];
    for (x, angle, len, color) in fronds {
        let base = Pos2::new(rect.min.x + rect.width() * x, foot);
        frond(painter, base, angle, rect.width() * len, color);
    }
}

/// A frond growing from `base` towards `angle` (radians, 0 to the right, up negative), bending as it
/// goes, with its leaflets on both sides, smaller towards the tip.
fn frond(painter: &egui::Painter, base: Pos2, angle: f32, len: f32, color: Color32) {
    const STEPS: usize = 14;
    let bend = if angle < -std::f32::consts::FRAC_PI_2 { -0.5 } else { 0.5 };
    let mut points = Vec::with_capacity(STEPS + 1);
    let mut p = base;
    for i in 0..=STEPS {
        let k = i as f32 / STEPS as f32;
        let a = angle + bend * k * k;
        points.push(p);
        p += Vec2::angled(a) * (len / STEPS as f32);
        if i == 0 || i == STEPS {
            continue;
        }
        // A leaflet each side, pointing along the frond.
        let size = len * 0.16 * (1.0 - k * 0.8);
        for side in [-1.0, 1.0] {
            let dir = Vec2::angled(a + side * 1.0);
            let tip = points[i] + dir * size;
            let back = points[i] - Vec2::angled(a) * (len / STEPS as f32) * 0.9;
            painter.add(egui::Shape::convex_polygon(vec![points[i], tip, back + dir * size * 0.3], color, Stroke::NONE));
        }
    }
    painter.add(egui::Shape::line(points, Stroke::new(1.2, color)));
}
