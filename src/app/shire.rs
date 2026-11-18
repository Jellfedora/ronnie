//! The Shire theme (« La Comté »), there from the start: branches with their leaves around the panes,
//! curls of the elvish manuscripts at their corners and on the dialogs, a twig under the bar of the
//! pane in focus, the title in the gold of the Ring between two vines, and at the foot of the sidebar
//! the hills of Hobbiton at dusk: a round green door in the hill, smoke from the chimney, fireflies.

use super::*;
use std::f32::consts::{PI, TAU};

/// The guard's alert.
pub(super) const ALERT: &str = "Vous ne passerez pas !";
pub(super) const GOLD: Color32 = Color32::from_rgb(217, 180, 74);
const GOLD_PALE: Color32 = Color32::from_rgb(246, 222, 150);
const LEAF: Color32 = Color32::from_rgb(132, 178, 76);
const LEAF_DARK: Color32 = Color32::from_rgb(82, 128, 52);
const LEAF_YOUNG: Color32 = Color32::from_rgb(176, 206, 104);
const BARK: Color32 = Color32::from_rgb(112, 88, 54);
const BERRY: Color32 = Color32::from_rgb(196, 64, 52);

/// A number in 0..1 that only depends on `n`.
fn noise(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// A leaf growing from `base` along `angle`, `len` long: a pointed oval and its vein.
pub(super) fn leaf(painter: &egui::Painter, base: Pos2, angle: f32, len: f32, color: Color32) {
    if len < 1.0 {
        return;
    }
    let d = Vec2::angled(angle);
    let n = d.rot90();
    let half = |t: f32| (t * PI).sin() * len * 0.3 * (1.0 - t * 0.35);
    let mut points = Vec::with_capacity(12);
    for k in 0..=6 {
        let t = k as f32 / 6.0;
        points.push(base + d * len * t + n * half(t));
    }
    for k in (1..6).rev() {
        let t = k as f32 / 6.0;
        points.push(base + d * len * t - n * half(t));
    }
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    painter.line_segment(
        [base, base + d * len * 0.78],
        Stroke::new(
            (len * 0.06).max(0.5),
            color.lerp_to_gamma(Color32::BLACK, 0.35),
        ),
    );
}

/// A spiral around `c`, from `r` inwards, starting at `start` and turning `turns` times (`sign` 1
/// clockwise on screen, -1 the other way).
pub(super) fn curl(
    painter: &egui::Painter,
    c: Pos2,
    r: f32,
    start: f32,
    turns: f32,
    sign: f32,
    stroke: Stroke,
) {
    let n = ((turns * 28.0) as usize).max(4);
    let points = (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            c + Vec2::angled(start + sign * t * turns * TAU) * r * (1.0 - t * 0.82)
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}

/// Which way the leaves of a vine grow.
#[derive(Clone, Copy, PartialEq)]
enum Sides {
    Both,
    /// Only on the side of `normal * sign`.
    One(f32),
}

/// A branch from `a` towards `b`, grown `lit` of the way (0 to 1), its leaves `s` times their usual
/// size, a curling tendril now and then; `alpha` fades it all.
#[allow(clippy::too_many_arguments)]
fn vine(
    painter: &egui::Painter,
    a: Pos2,
    b: Pos2,
    lit: f32,
    s: f32,
    seed: u32,
    sides: Sides,
    alpha: f32,
) {
    let len = (b - a).length();
    if len < 2.0 || lit <= 0.0 {
        return;
    }
    let d = (b - a) / len;
    let n = d.rot90();
    let end = len * lit.min(1.0);
    let phase = noise(seed) * TAU;
    let wave = |x: f32| (x / (15.0 * s) + phase).sin() * 1.4 * s;
    let at = |x: f32| a + d * x + n * wave(x);
    let steps = ((end / 3.0) as usize).max(2);
    let stem: Vec<Pos2> = (0..=steps)
        .map(|k| at(end * k as f32 / steps as f32))
        .collect();
    painter.add(egui::Shape::line(
        stem,
        Stroke::new(1.2 * s, BARK.gamma_multiply(alpha)),
    ));
    let gap = 15.0 * s;
    let mut x = gap * 0.5;
    let mut i = 0u32;
    while x < end - 1.0 {
        let side = match sides {
            Sides::Both => {
                if i.is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                }
            }
            Sides::One(sign) => sign,
        };
        let r = noise(seed.wrapping_add(i * 7 + 1));
        let color = if r < 0.45 {
            LEAF
        } else if r < 0.8 {
            LEAF_DARK
        } else {
            LEAF_YOUNG
        };
        // The leaves near the tip are still small.
        let grow = ((end - x) / (gap * 1.5)).clamp(0.25, 1.0);
        let angle = (d * 0.75 + n * side).angle() + (r - 0.5) * 0.5;
        leaf(
            painter,
            at(x),
            angle,
            (6.0 + r * 3.0) * s * grow,
            color.gamma_multiply(alpha),
        );
        if i % 4 == 2 && grow >= 1.0 {
            // A tendril on the other side, curling up.
            let base = at(x + gap * 0.5);
            let out = -side;
            let tip = base + n * out * 5.0 * s + d * 2.0 * s;
            painter.line_segment(
                [base, tip],
                Stroke::new(0.8 * s, BARK.gamma_multiply(alpha)),
            );
            curl(
                painter,
                tip + n * out * 2.0 * s,
                2.4 * s,
                (n * -out).angle(),
                1.2,
                out,
                Stroke::new(0.8 * s, LEAF_DARK.gamma_multiply(alpha)),
            );
        }
        x += gap;
        i += 1;
    }
}

/// A branch along the middle of `rect`, grown `lit` of the way.
pub(super) fn vine_along(painter: &egui::Painter, rect: Rect, lit: f32) {
    let y = rect.center().y;
    vine(
        painter,
        Pos2::new(rect.min.x + 2.0, y),
        Pos2::new(rect.max.x - 2.0, y),
        lit,
        0.75,
        rect.min.x as u32,
        Sides::Both,
        1.0,
    );
}

/// A flourish at the corner `c` of a rect (`sx`, `sy` pointing inside it): a gold curl in the corner,
/// two branches along the edges with their leaves turned outwards, two berries. `size` is how far the
/// branches reach.
fn flourish(painter: &egui::Painter, c: Pos2, sx: f32, sy: f32, size: f32, alpha: f32) {
    let inward = Vec2::new(sx, sy);
    let k = c + inward * 7.0;
    let start = (-inward).angle();
    curl(
        painter,
        k,
        6.0,
        start,
        1.6,
        sx * sy,
        Stroke::new(1.3, GOLD.gamma_multiply(0.85 * alpha)),
    );
    painter.circle_filled(k, 1.4, GOLD.gamma_multiply(alpha));
    let seed = (c.x as u32).wrapping_mul(31) ^ c.y as u32;
    // Along the top or bottom edge, the leaves outside (normal of a vine going +x is +y).
    let h_out = -sy * sx;
    vine(
        painter,
        c + Vec2::new(sx * 12.0, sy * 1.5),
        c + Vec2::new(sx * size, sy * 1.5),
        1.0,
        0.8,
        seed,
        Sides::One(h_out),
        alpha,
    );
    // Along the left or right edge.
    let v_out = sx * sy;
    vine(
        painter,
        c + Vec2::new(sx * 1.5, sy * 12.0),
        c + Vec2::new(sx * 1.5, sy * size * 0.75),
        1.0,
        0.8,
        seed + 5,
        Sides::One(v_out),
        alpha,
    );
    for (dx, dy) in [(14.0, 4.0), (17.0, 6.5)] {
        painter.circle_filled(
            c + Vec2::new(sx * dx, sy * dy),
            1.6,
            BERRY.gamma_multiply(alpha),
        );
    }
}

/// A pane: a thin edge of moss and a flourish at each corner; `strength` 1 for the pane in focus, less
/// for the others.
pub(super) fn frame(painter: &egui::Painter, r: Rect, strength: f32) {
    let r = r.shrink(1.0);
    painter.rect_stroke(
        r,
        4.0,
        Stroke::new(1.0, LEAF.gamma_multiply(0.45 * strength)),
        egui::StrokeKind::Inside,
    );
    let size = (r.width().min(r.height()) * 0.28).clamp(26.0, 74.0);
    for (c, sx, sy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        flourish(painter, c, sx, sy, size, strength);
    }
}

/// The corners of a dialog.
pub(super) fn corners(painter: &egui::Painter, r: Rect) {
    for (c, sx, sy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        flourish(painter, c, sx, sy, 44.0, 1.0);
    }
}

/// The title in the sidebar: « Ronnie » in the gold of the Ring, a branch on each side ending in a curl.
pub(super) fn logo(painter: &egui::Painter, rect: Rect) {
    let c = rect.center() - Vec2::new(0.0, 2.0);
    super::paint_metal(painter, c, Align2::CENTER_CENTER, "Ronnie", 30.0, GOLD, 1.0);
    let w = (rect.width() / 2.0 - 6.0).min(104.0);
    if w < 64.0 {
        return;
    }
    for side in [-1.0f32, 1.0] {
        let from = c + Vec2::new(side * 54.0, 4.0);
        let to = c + Vec2::new(side * w, 4.0);
        vine(
            painter,
            from,
            to,
            1.0,
            0.7,
            if side < 0.0 { 3 } else { 11 },
            Sides::Both,
            1.0,
        );
        curl(
            painter,
            to + Vec2::new(0.0, -4.0),
            4.0,
            PI / 2.0,
            1.4,
            side,
            Stroke::new(1.1, GOLD.gamma_multiply(0.9)),
        );
    }
}

/// The R of the folded sidebar, with a leaf.
pub(super) fn mark(painter: &egui::Painter, c: Pos2) {
    super::paint_metal(painter, c, Align2::CENTER_CENTER, "R", 32.0, GOLD, 1.0);
    leaf(painter, c + Vec2::new(7.0, -11.0), -0.6, 9.0, LEAF);
}

/// After a title ending at `text_end`: a leaf, then a branch fading out across `rect`.
pub(super) fn title_marks(painter: &egui::Painter, rect: Rect, text_end: f32) {
    let y = rect.center().y;
    let from = text_end + 10.0;
    if from + 30.0 >= rect.max.x {
        return;
    }
    leaf(painter, Pos2::new(from, y + 1.0), -0.5, 8.0, LEAF);
    let to = (rect.max.x - 6.0).min(from + 220.0);
    let parts = 4;
    let step = (to - from - 8.0) / parts as f32;
    for p in 0..parts {
        let x = from + 8.0 + step * p as f32;
        vine(
            painter,
            Pos2::new(x, y),
            Pos2::new(x + step, y),
            1.0,
            0.6,
            p as u32 * 13 + 2,
            Sides::Both,
            1.0 - p as f32 / parts as f32,
        );
    }
}

/// What is selected: a leaf on its left.
pub(super) fn active_mark(painter: &egui::Painter, r: Rect) {
    leaf(
        painter,
        Pos2::new(r.min.x + 3.0, r.center().y + 7.0),
        -PI / 2.0 + 0.15,
        13.0,
        LEAF,
    );
}

/// The sidebar's panel: the green of the woods going dark towards its foot, ivy climbing its edge.
pub(super) fn panel_bg(painter: &egui::Painter, r: Rect) {
    let mut mesh = egui::Mesh::default();
    let (top, foot) = (Color32::from_rgb(34, 46, 30), Color32::from_rgb(17, 23, 15));
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.left_bottom(), foot);
    mesh.colored_vertex(r.right_bottom(), foot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
    let x = r.max.x - 3.0;
    vine(
        painter,
        Pos2::new(x, r.max.y),
        Pos2::new(x, r.min.y + 60.0),
        1.0,
        0.7,
        41,
        Sides::One(-1.0),
        0.55,
    );
}

/// A shape filled from the curve `top` (y for an x) down to `bottom`.
fn fill_under(
    painter: &egui::Painter,
    x0: f32,
    x1: f32,
    bottom: f32,
    top: impl Fn(f32) -> f32,
    color: Color32,
) {
    let mut mesh = egui::Mesh::default();
    let steps = (((x1 - x0) / 4.0) as u32).max(2);
    for k in 0..=steps {
        let x = x0 + (x1 - x0) * k as f32 / steps as f32;
        mesh.colored_vertex(Pos2::new(x, top(x)), color);
        mesh.colored_vertex(Pos2::new(x, bottom), color);
        if k > 0 {
            let i = k * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(mesh);
}

/// The hills of Hobbiton at dusk at the foot of `field`: a great tree on the far hill, a round green
/// door and a lit window in the near one, smoke from the chimney, fireflies. Returns when to paint again,
/// in seconds.
pub(super) fn scene(painter: &egui::Painter, field: Rect, now: f64) -> f32 {
    let painter = &painter.with_clip_rect(field.intersect(painter.clip_rect()));
    let (x0, x1, w, h) = (field.min.x, field.max.x, field.width(), field.height());
    let foot = field.max.y;
    // The far hill and its tree.
    let far = |x: f32| foot - h * 0.42 - ((x - x0) / w * PI * 1.3 + 0.4).sin() * h * 0.12;
    fill_under(painter, x0, x1, foot, far, Color32::from_rgb(40, 66, 36));
    let tx = x0 + w * 0.2;
    let ty = far(tx);
    painter.line_segment(
        [Pos2::new(tx, ty + 2.0), Pos2::new(tx, ty - h * 0.2)],
        Stroke::new(3.0, Color32::from_rgb(70, 54, 34)),
    );
    for (dx, dy, r) in [
        (0.0, -0.3, 0.13),
        (-0.07, -0.24, 0.09),
        (0.07, -0.25, 0.1),
        (0.0, -0.4, 0.08),
    ] {
        painter.circle_filled(
            Pos2::new(tx + dx * h, ty + dy * h),
            r * h,
            Color32::from_rgb(52, 90, 44),
        );
    }
    // The near hill.
    let near = |x: f32| foot - h * 0.26 - ((x - x0) / w * PI * 0.9 + 1.9).sin() * h * 0.13;
    fill_under(painter, x0, x1, foot, near, Color32::from_rgb(62, 104, 48));
    // Flowers in the grass.
    for i in 0..14u32 {
        let x = x0 + noise(i + 50) * w;
        let y = near(x) + 4.0 + noise(i + 70) * (foot - near(x) - 6.0).max(1.0);
        let c = [
            Color32::from_rgb(240, 214, 90),
            Color32::from_rgb(232, 120, 140),
            Color32::from_rgb(246, 244, 230),
        ][(i % 3) as usize];
        painter.circle_filled(Pos2::new(x, y), 1.2, c);
    }
    // The door, round and green, a brass knob in its middle; the round window beside it, lit.
    let dx = x0 + w * 0.64;
    let r = (h * 0.15).clamp(9.0, 15.0);
    let door = Pos2::new(dx, near(dx) + r * 1.45);
    painter.circle_filled(door, r + 2.5, Color32::from_rgb(132, 116, 86));
    painter.circle_filled(door, r, Color32::from_rgb(46, 112, 58));
    for k in -2..=2 {
        let x = door.x + k as f32 * r * 0.36;
        let half = (r * r - (x - door.x).powi(2)).max(0.0).sqrt();
        painter.line_segment(
            [Pos2::new(x, door.y - half), Pos2::new(x, door.y + half)],
            Stroke::new(0.7, Color32::from_rgb(30, 82, 42)),
        );
    }
    painter.circle_filled(door, r * 0.16, GOLD);
    let win = Pos2::new(door.x - r * 2.4, door.y - r * 0.35);
    let glow = 0.85 + ((now * 1.3).sin() as f32) * 0.08;
    painter.circle_filled(
        win,
        r * 0.75,
        Color32::from_rgb(242, 206, 110).gamma_multiply(glow * 0.25),
    );
    painter.circle_filled(win, r * 0.5, Color32::from_rgb(132, 116, 86));
    painter.circle_filled(
        win,
        r * 0.38,
        Color32::from_rgb(250, 212, 120).gamma_multiply(glow),
    );
    painter.line_segment(
        [
            win - Vec2::new(r * 0.38, 0.0),
            win + Vec2::new(r * 0.38, 0.0),
        ],
        Stroke::new(0.8, Color32::from_rgb(132, 116, 86)),
    );
    painter.line_segment(
        [
            win - Vec2::new(0.0, r * 0.38),
            win + Vec2::new(0.0, r * 0.38),
        ],
        Stroke::new(0.8, Color32::from_rgb(132, 116, 86)),
    );
    // The chimney on the hill, its smoke rising and drifting away.
    let cx = door.x + r * 1.8;
    let cy = near(cx);
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(cx - 2.5, cy - 9.0), Pos2::new(cx + 2.5, cy + 2.0)),
        1.0,
        Color32::from_rgb(120, 96, 76),
    );
    for i in 0..7u32 {
        let t = ((now * 0.22 + i as f64 / 7.0) % 1.0) as f32;
        let at = Pos2::new(
            cx + t * 16.0 + (t * 9.0 + i as f32).sin() * 2.5,
            cy - 10.0 - t * h * 0.55,
        );
        painter.circle_filled(
            at,
            1.5 + t * 4.5,
            Color32::from_rgb(200, 200, 190).gamma_multiply((1.0 - t) * 0.35),
        );
    }
    // Fireflies, blinking.
    for i in 0..9u32 {
        let x = x0 + noise(i + 90) * w + ((now * 0.4 + i as f64).sin() as f32) * 8.0;
        let y = field.min.y
            + h * (0.15 + noise(i + 110) * 0.5)
            + ((now * 0.7 + i as f64 * 2.0).cos() as f32) * 5.0;
        let on = (((now * 1.6 + i as f64 * 1.3).sin() as f32) * 0.5 + 0.5).powi(3);
        painter.circle_filled(Pos2::new(x, y), 3.0, GOLD_PALE.gamma_multiply(on * 0.18));
        painter.circle_filled(Pos2::new(x, y), 1.1, GOLD_PALE.gamma_multiply(on));
    }
    0.05
}

/// The One Ring at `c`, `r` wide: gold, its glint, a faint glow.
pub(super) fn ring(painter: &egui::Painter, c: Pos2, r: f32) {
    painter.circle_filled(c, r * 1.25, GOLD.gamma_multiply(0.12));
    painter.circle_stroke(c, r, Stroke::new(r * 0.32, GOLD));
    painter.circle_stroke(
        c,
        r * 1.12,
        Stroke::new(0.8, Color32::from_rgb(150, 112, 30)),
    );
    let arc: Vec<Pos2> = (0..=10)
        .map(|k| c + Vec2::angled(-2.6 + k as f32 * 0.09) * r)
        .collect();
    painter.add(egui::Shape::line(arc, Stroke::new(r * 0.12, GOLD_PALE)));
}
