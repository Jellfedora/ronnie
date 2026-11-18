//! The Shire theme (« La Comté »), there from the start, sober: warm wood and parchment, a gold curl and
//! a sprig of leaves at the corners of the pane in focus, curls on the dialogs, a twig under the bar of
//! the pane in focus, the title in the gold of the Ring, and at the foot of the sidebar
//! the hills of Hobbiton at dusk: a round green door in the hill, smoke from the chimney, fireflies.

use super::*;
use std::f32::consts::{PI, TAU};

/// The guard's alert.
pub(super) const ALERT: &str = "Vous ne passerez pas !";
pub(super) const GOLD: Color32 = Color32::from_rgb(217, 180, 74);
const GOLD_PALE: Color32 = Color32::from_rgb(246, 222, 150);
const LEAF: Color32 = Color32::from_rgb(132, 178, 76);
const LEAF_DARK: Color32 = Color32::from_rgb(82, 128, 52);
const BARK: Color32 = Color32::from_rgb(112, 88, 54);

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

/// A branch from `a` towards `b`, grown `lit` of the way (0 to 1), a leaf every `gap` points, `s`
/// times their usual size; `alpha` fades it all.
#[allow(clippy::too_many_arguments)]
fn vine(
    painter: &egui::Painter,
    a: Pos2,
    b: Pos2,
    lit: f32,
    s: f32,
    gap: f32,
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
    let wave = |x: f32| (x / (18.0 * s) + phase).sin() * 1.1 * s;
    let at = |x: f32| a + d * x + n * wave(x);
    let steps = ((end / 3.0) as usize).max(2);
    let stem: Vec<Pos2> = (0..=steps)
        .map(|k| at(end * k as f32 / steps as f32))
        .collect();
    painter.add(egui::Shape::line(
        stem,
        Stroke::new(1.0 * s, BARK.gamma_multiply(alpha)),
    ));
    let mut x = gap * 0.6;
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
        let color = if r < 0.6 { LEAF_DARK } else { LEAF };
        let grow = ((end - x) / (gap * 0.8)).clamp(0.3, 1.0);
        let angle = (d * 0.8 + n * side).angle() + (r - 0.5) * 0.4;
        leaf(
            painter,
            at(x),
            angle,
            (5.5 + r * 2.0) * s * grow,
            color.gamma_multiply(alpha),
        );
        x += gap;
        i += 1;
    }
}

/// A twig along the middle of `rect`, grown `lit` of the way: a few leaves only.
pub(super) fn vine_along(painter: &egui::Painter, rect: Rect, lit: f32) {
    let y = rect.center().y;
    vine(
        painter,
        Pos2::new(rect.min.x + 2.0, y),
        Pos2::new(rect.max.x - 2.0, y),
        lit,
        0.7,
        70.0,
        rect.min.x as u32,
        Sides::Both,
        0.9,
    );
}

/// A thin line of gold across `rect`, fading at both ends, a small curl in its middle.
pub(super) fn fine_line(painter: &egui::Painter, rect: Rect) {
    let y = rect.center().y;
    let parts = 16;
    let step = rect.width() / parts as f32;
    for p in 0..parts {
        let mid = ((p as f32 + 0.5) / parts as f32 - 0.5).abs() * 2.0;
        let x = rect.min.x + step * p as f32;
        painter.hline(
            x..=x + step,
            y,
            Stroke::new(1.0, GOLD.gamma_multiply(0.55 * (1.0 - mid * mid))),
        );
    }
}

/// A flourish at the corner `c` of a rect (`sx`, `sy` pointing inside it): a gold curl in the corner, a
/// short sprig along each edge with two or three leaves turned outwards.
fn flourish(painter: &egui::Painter, c: Pos2, sx: f32, sy: f32, size: f32) {
    let inward = Vec2::new(sx, sy);
    let k = c + inward * 6.0;
    curl(
        painter,
        k,
        5.0,
        (-inward).angle(),
        1.5,
        sx * sy,
        Stroke::new(1.2, GOLD.gamma_multiply(0.85)),
    );
    painter.circle_filled(k, 1.2, GOLD);
    let seed = (c.x as u32).wrapping_mul(31) ^ c.y as u32;
    // Along the top or bottom edge, the leaves outside (the normal of a vine going +x is +y).
    vine(
        painter,
        c + Vec2::new(sx * 11.0, sy * 1.5),
        c + Vec2::new(sx * size, sy * 1.5),
        1.0,
        0.75,
        16.0,
        seed,
        Sides::One(-sy * sx),
        0.9,
    );
    vine(
        painter,
        c + Vec2::new(sx * 1.5, sy * 11.0),
        c + Vec2::new(sx * 1.5, sy * size * 0.7),
        1.0,
        0.75,
        16.0,
        seed + 5,
        Sides::One(sx * sy),
        0.9,
    );
}

/// The pane in focus: a flourish at each corner, nothing else.
pub(super) fn frame(painter: &egui::Painter, r: Rect) {
    let r = r.shrink(1.0);
    let size = (r.width().min(r.height()) * 0.2).clamp(22.0, 40.0);
    for (c, sx, sy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        flourish(painter, c, sx, sy, size);
    }
}

/// The corners of a dialog: gold curls only.
pub(super) fn corners(painter: &egui::Painter, r: Rect) {
    for (c, sx, sy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        let inward = Vec2::new(sx, sy);
        curl(
            painter,
            c + inward * 7.0,
            6.0,
            (-inward).angle(),
            1.5,
            sx * sy,
            Stroke::new(1.3, GOLD.gamma_multiply(0.85)),
        );
    }
}

/// The title in the sidebar: « Ronnie » in the gold of the Ring, a fine gold line on each side ending in
/// a curl.
pub(super) fn logo(painter: &egui::Painter, rect: Rect) {
    let c = rect.center() - Vec2::new(0.0, 2.0);
    super::paint_metal(painter, c, Align2::CENTER_CENTER, "Ronnie", 30.0, GOLD, 1.0);
    let w = (rect.width() / 2.0 - 6.0).min(100.0);
    if w < 66.0 {
        return;
    }
    for side in [-1.0f32, 1.0] {
        let (from, to) = (c.x + side * 54.0, c.x + side * w);
        painter.line_segment(
            [Pos2::new(from, c.y + 4.0), Pos2::new(to, c.y + 4.0)],
            Stroke::new(1.0, GOLD.gamma_multiply(0.6)),
        );
        curl(
            painter,
            Pos2::new(to, c.y),
            4.0,
            PI / 2.0,
            1.4,
            side,
            Stroke::new(1.1, GOLD.gamma_multiply(0.8)),
        );
    }
}

/// The R of the folded sidebar, with a leaf.
pub(super) fn mark(painter: &egui::Painter, c: Pos2) {
    super::paint_metal(painter, c, Align2::CENTER_CENTER, "R", 32.0, GOLD, 1.0);
    leaf(painter, c + Vec2::new(7.0, -11.0), -0.6, 8.0, LEAF_DARK);
}

/// After a title ending at `text_end`: a small leaf, then a fine gold line fading out across `rect`.
pub(super) fn title_marks(painter: &egui::Painter, rect: Rect, text_end: f32) {
    let y = rect.center().y;
    let from = text_end + 10.0;
    if from + 30.0 >= rect.max.x {
        return;
    }
    leaf(painter, Pos2::new(from, y + 1.0), -0.5, 7.0, LEAF_DARK);
    let to = (rect.max.x - 6.0).min(from + 200.0);
    let parts = 8;
    let step = (to - from - 8.0) / parts as f32;
    for p in 0..parts {
        let x = from + 8.0 + step * p as f32;
        painter.hline(
            x..=x + step,
            y,
            Stroke::new(
                1.0,
                GOLD.gamma_multiply(0.5 * (1.0 - p as f32 / parts as f32)),
            ),
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

/// The sidebar's panel: warm wood going dark towards its foot.
pub(super) fn panel_bg(painter: &egui::Painter, r: Rect) {
    let mut mesh = egui::Mesh::default();
    let (top, foot) = (Color32::from_rgb(60, 52, 41), Color32::from_rgb(44, 38, 30));
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.left_bottom(), foot);
    mesh.colored_vertex(r.right_bottom(), foot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
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
