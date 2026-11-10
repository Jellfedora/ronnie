//! The Game of Thrones theme, won at Pause Café: the title between two swords, the Wall under the snow
//! with White Walkers at its foot, a dragon diving down the sidebar now and then, a sword under the bar of the pane in focus, and when the theme is picked
//! or won, the Iron Throne in the dark and the words of House Stark.

use super::*;

/// The guard's alert.
pub(super) const ALERT: &str = "Dracarys !";
const STEEL: Color32 = Color32::from_rgb(196, 206, 218);
const STEEL_DARK: Color32 = Color32::from_rgb(92, 102, 116);
pub(super) const GOLD: Color32 = Color32::from_rgb(201, 162, 75);
const ICE: Color32 = Color32::from_rgb(176, 214, 236);
const FIRE: Color32 = Color32::from_rgb(255, 128, 40);
/// Seconds between two flights of the dragon, and how long one lasts.
pub(super) const FLIGHT_EVERY: f64 = 30.0;
pub(super) const FLIGHT_SECS: f32 = 8.0;

/// A number in 0..1 that only depends on `n`.
fn noise(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// A sword lying from `a` to `b`: its blade, guard and pommel.
fn sword(painter: &egui::Painter, a: Pos2, b: Pos2, width: f32) {
    let along = (b - a).normalized();
    let across = along.rot90();
    let hilt = a + along * width * 3.0;
    painter.line_segment([a, hilt], Stroke::new(width * 0.8, STEEL_DARK));
    painter.circle_filled(a, width * 0.8, GOLD);
    painter.line_segment([hilt - across * width * 2.0, hilt + across * width * 2.0], Stroke::new(width, GOLD));
    painter.add(egui::Shape::convex_polygon(vec![hilt - across * width * 0.7, hilt + across * width * 0.7, b], STEEL, Stroke::NONE));
}

/// The pointer as a sword (only for the plain arrow).
pub(super) fn pointer(ctx: &egui::Context) {
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    if ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default {
        return;
    }
    ctx.set_cursor_icon(egui::CursorIcon::None);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("sword-pointer")));
    pointer_sword(&painter, at);
}

/// The sword of the pointer, its point at `at`, the hilt down and to the right: steel, a gold guard and
/// pommel, a glint along the blade.
pub(super) fn pointer_sword(painter: &egui::Painter, at: Pos2) {
    let dir = Vec2::new(-0.55, -1.0).normalized();
    let pommel = at - dir * 30.0;
    sword(painter, pommel, at, 2.4);
    // The edge catching the light.
    let hilt = pommel + dir * 7.2;
    painter.line_segment([hilt + dir * 2.0, at - dir * 3.0], Stroke::new(0.7, Color32::WHITE.gamma_multiply(0.7)));
}

/// The title in the sidebar, between two swords.
pub(super) fn logo(painter: &egui::Painter, rect: Rect) {
    let c = rect.center();
    painter.text(c - Vec2::new(0.0, 7.0), Align2::CENTER_CENTER, "GAME OF", FontId::proportional(11.0), STEEL);
    painter.text(c + Vec2::new(0.0, 7.0), Align2::CENTER_CENTER, "THRONES", FontId::proportional(19.0), STEEL);
    let w = rect.width().min(190.0) / 2.0;
    sword(painter, c + Vec2::new(-w + 2.0, 10.0), c + Vec2::new(-52.0, -10.0), 1.5);
    sword(painter, c + Vec2::new(w - 2.0, 10.0), c + Vec2::new(52.0, -10.0), 1.5);
}

/// A gold rivet.
pub(super) fn rivet(painter: &egui::Painter, c: Pos2, d: f32) {
    painter.circle_filled(c, d, GOLD);
    painter.circle_filled(c - Vec2::splat(d * 0.3), d * 0.35, Color32::from_rgb(250, 228, 170));
}

/// A line of steel across `rect`, riveted every so often.
pub(super) fn iron_line(painter: &egui::Painter, rect: Rect) {
    let y = rect.center().y;
    painter.hline(rect.x_range(), y, Stroke::new(1.5, STEEL_DARK));
    painter.hline(rect.x_range(), y - 1.0, Stroke::new(0.6, STEEL.gamma_multiply(0.6)));
    let mut x = rect.min.x + 14.0;
    while x < rect.max.x - 6.0 {
        rivet(painter, Pos2::new(x, y), 1.8);
        x += 72.0;
    }
}

/// A pane in focus: edged in steel, a rivet at each corner.
pub(super) fn frame(painter: &egui::Painter, r: Rect) {
    let r = r.shrink(1.0);
    painter.rect_stroke(r, 2.0, Stroke::new(1.2, STEEL.gamma_multiply(0.75)), egui::StrokeKind::Inside);
    for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
        rivet(painter, c + (r.center() - c).normalized() * 5.0, 2.0);
    }
}

/// Iron brackets at the corners of a window, a rivet in each.
pub(super) fn corners(painter: &egui::Painter, r: Rect) {
    let arm = 16.0;
    let stroke = Stroke::new(2.0, GOLD.gamma_multiply(0.85));
    for (c, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        painter.line_segment([c, c + Vec2::new(dx * arm, 0.0)], stroke);
        painter.line_segment([c, c + Vec2::new(0.0, dy * arm)], stroke);
        rivet(painter, c + Vec2::new(dx * 5.0, dy * 5.0), 1.8);
    }
}

/// After a title ending at `text_end`: a rivet, then a steel line fading out across `rect`.
pub(super) fn title_marks(painter: &egui::Painter, rect: Rect, text_end: f32) {
    let y = rect.center().y;
    let from = text_end + 10.0;
    if from + 20.0 >= rect.max.x {
        return;
    }
    rivet(painter, Pos2::new(from, y), 2.0);
    let parts = 8;
    let step = (rect.max.x - from - 6.0) / parts as f32;
    for n in 0..parts {
        let x = from + 6.0 + step * n as f32;
        painter.hline(x..=x + step, y, Stroke::new(1.0, STEEL.gamma_multiply(0.6 * (1.0 - n as f32 / parts as f32))));
    }
}

/// What is selected: a bar of ice along its left, a rivet at its right.
pub(super) fn active_mark(painter: &egui::Painter, r: Rect) {
    painter.rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 3.0), Vec2::new(3.0, r.height() - 6.0)), 1.0, ICE);
    rivet(painter, Pos2::new(r.max.x - 9.0, r.center().y), 1.8);
}

/// The sidebar's panel: iron going dark towards its foot, riveted along its edge.
pub(super) fn panel_bg(painter: &egui::Painter, r: Rect) {
    let mut mesh = egui::Mesh::default();
    let (top, foot) = (Color32::from_rgb(38, 44, 54), Color32::from_rgb(14, 17, 22));
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.left_bottom(), foot);
    mesh.colored_vertex(r.right_bottom(), foot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
    painter.vline(r.max.x - 1.0, r.y_range(), Stroke::new(1.5, STEEL_DARK));
    let mut y = r.min.y + 40.0;
    while y < r.max.y - 10.0 {
        rivet(painter, Pos2::new(r.max.x - 1.0, y), 1.6);
        y += 96.0;
    }
}

/// A sword under a bar, lit by how much of it shows (0 to 1).
pub(super) fn sword_along(painter: &egui::Painter, rect: Rect, lit: f32) {
    let y = rect.center().y;
    let end = rect.min.x + rect.width() * lit;
    sword(painter, Pos2::new(rect.min.x + 3.0, y), Pos2::new(end, y), (rect.height() / 3.0).max(1.0));
}

/// The dragon at `c`, `s` long from its middle to its snout, heading along `angle` (0 to the right, a
/// quarter turn down), its wings at `flap` (-1 to 1), breathing fire if `breath`, `now` making the
/// flames dance.
fn dragon(painter: &egui::Painter, c: Pos2, s: f32, angle: f32, flap: f32, breath: bool, now: f64) {
    let hide = Color32::from_rgb(150, 34, 28);
    let wing = Color32::from_rgb(96, 22, 22);
    let belly = Color32::from_rgb(226, 170, 80);
    let turn = egui::emath::Rot2::from_angle(angle);
    let p = |x: f32, y: f32| c + turn * Vec2::new(x * s, y * s);
    // The far wing, behind the body.
    let tip = -0.95 * flap;
    painter.add(egui::Shape::convex_polygon(vec![p(0.1, -0.1), p(0.5, -0.12), p(0.25, tip * 0.9 - 0.1), p(-0.25, tip * 0.7)], wing.gamma_multiply(0.75), Stroke::NONE));
    // From the tail to the neck, thinner at both ends.
    let spine = [(-2.1, 0.32, 0.03), (-1.7, 0.16, 0.07), (-1.2, 0.04, 0.13), (-0.7, 0.0, 0.2), (-0.1, -0.02, 0.27), (0.5, -0.05, 0.22), (0.95, -0.22, 0.13), (1.3, -0.36, 0.1)];
    for pair in spine.windows(2) {
        let ((x0, y0, w0), (x1, y1, w1)) = (pair[0], pair[1]);
        painter.line_segment([p(x0, y0), p(x1, y1)], Stroke::new(s * (w0 + w1), hide));
        painter.line_segment([p(x0, y0 + w0 * 0.55), p(x1, y1 + w1 * 0.55)], Stroke::new(s * 0.05, belly));
    }
    painter.add(egui::Shape::convex_polygon(vec![p(-2.1, 0.32), p(-2.45, 0.2), p(-2.4, 0.46)], hide, Stroke::NONE));
    // The head, its jaw, its horn and its eye.
    painter.add(egui::Shape::convex_polygon(vec![p(1.2, -0.48), p(1.5, -0.46), p(1.82, -0.3), p(1.76, -0.22), p(1.3, -0.22)], hide, Stroke::NONE));
    painter.line_segment([p(1.26, -0.46), p(0.98, -0.66)], Stroke::new(s * 0.05, belly));
    painter.circle_filled(p(1.46, -0.38), s * 0.035, Color32::from_rgb(255, 226, 90));
    // The legs, tucked in.
    painter.line_segment([p(-0.5, 0.2), p(-0.72, 0.42)], Stroke::new(s * 0.09, hide));
    painter.line_segment([p(0.3, 0.2), p(0.1, 0.42)], Stroke::new(s * 0.09, hide));
    // The near wing, its ribs lit.
    let (elbow, far) = (p(-0.1, tip * 0.75 - 0.2), p(-0.95, tip * 1.25));
    painter.add(egui::Shape::convex_polygon(vec![p(-0.5, -0.05), p(0.4, -0.1), elbow, far], wing, Stroke::NONE));
    painter.line_segment([p(0.4, -0.1), elbow], Stroke::new(s * 0.06, hide));
    painter.line_segment([elbow, far], Stroke::new(s * 0.05, hide));
    painter.line_segment([elbow, p(-0.5, -0.05)], Stroke::new(s * 0.025, FIRE.gamma_multiply(0.8)));
    if breath {
        for n in 0..26u32 {
            let along = n as f32 / 25.0;
            let shake = ((now * 24.0 + n as f64 * 1.7).sin() as f32) * 0.05 * (0.3 + along * 2.0);
            let at = p(1.85 + along * 2.7, -0.26 + along * 0.5 + shake);
            let color = Color32::from_rgb(255, 236, 150).lerp_to_gamma(FIRE, (along * 1.6).min(1.0)).lerp_to_gamma(Color32::from_rgb(200, 30, 16), (along * 1.4 - 0.4).clamp(0.0, 1.0));
            painter.circle_filled(at, s * (0.06 + along * 0.26), color.gamma_multiply(1.0 - along * 0.75));
        }
    }
}

/// The dragon diving down the sidebar `bar`, `t` seconds into its flight, swaying from side to side.
pub(super) fn flight(painter: &egui::Painter, bar: Rect, t: f32, now: f64) {
    let along = (t / FLIGHT_SECS).clamp(0.0, 1.0);
    let s = (bar.width() * 0.2).clamp(34.0, 60.0);
    let margin = s * 3.0;
    let y = bar.min.y - margin + (bar.height() + margin * 2.0) * along;
    let x = bar.center().x + (along * 9.0).sin() * bar.width() * 0.14;
    let heading = std::f32::consts::FRAC_PI_2 - (along * 9.0).cos() * 0.3;
    dragon(painter, Pos2::new(x, y), s, heading, (now * 4.5).sin() as f32, (0.15..0.85).contains(&along), now);
}

/// A White Walker standing at `foot`, `h` tall, going `dir`: a ragged cloak, a crown of ice, its eyes
/// burning blue and a spear of ice.
fn walker(painter: &egui::Painter, foot: Pos2, h: f32, dir: f32, now: f64) {
    let pale = Color32::from_rgb(190, 214, 228);
    let cloak = Color32::from_rgb(52, 66, 84);
    let eye = Color32::from_rgb(70, 190, 255);
    let step = (now * 3.0 + foot.x as f64 * 0.05).sin() as f32 * h * 0.12;
    let hip = foot - Vec2::new(0.0, h * 0.42);
    painter.line_segment([hip, foot + Vec2::new(step, 0.0)], Stroke::new(h * 0.09, pale.gamma_multiply(0.7)));
    painter.line_segment([hip, foot - Vec2::new(step, 0.0)], Stroke::new(h * 0.09, pale.gamma_multiply(0.7)));
    let neck = foot - Vec2::new(0.0, h * 0.8);
    painter.add(egui::Shape::convex_polygon(vec![neck - Vec2::new(h * 0.13, 0.0), neck + Vec2::new(h * 0.13, 0.0), hip + Vec2::new(h * 0.2, h * 0.14), hip - Vec2::new(h * 0.2, -h * 0.14)], cloak, Stroke::NONE));
    let head = foot - Vec2::new(0.0, h * 0.9);
    painter.circle_filled(head, h * 0.11, pale);
    for spike in [-0.08, -0.03, 0.03, 0.08] {
        let base = head + Vec2::new(spike * h, -h * 0.08);
        painter.line_segment([base, base + Vec2::new(spike * h * 0.5, -h * 0.1)], Stroke::new(h * 0.025, pale));
    }
    let eyes = head + Vec2::new(dir * h * 0.05, -h * 0.01);
    painter.circle_filled(eyes, h * 0.08, eye.gamma_multiply(0.3));
    painter.circle_filled(eyes, h * 0.028, eye);
    let hand = neck + Vec2::new(dir * h * 0.22, h * 0.14);
    painter.line_segment([neck, hand], Stroke::new(h * 0.06, pale.gamma_multiply(0.8)));
    painter.line_segment([hand + Vec2::new(-dir * h * 0.1, h * 0.46), hand + Vec2::new(dir * h * 0.1, -h * 0.5)], Stroke::new(h * 0.03, ICE));
}

/// The Wall at the foot of `field`, its White Walkers marching, the snow falling. Returns when to paint
/// again, in seconds.
pub(super) fn wall(painter: &egui::Painter, field: Rect, now: f64) -> f32 {
    let top = field.max.y - field.height().min(120.0) * 0.45;
    let mut ice = Vec::new();
    let mut x = field.min.x;
    let mut n = 0;
    while x < field.max.x {
        let h = top + noise(n) * 8.0;
        ice.push((x, h));
        x += 14.0;
        n += 1;
    }
    for pair in ice.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(a.0, a.1), Pos2::new(b.0, b.1), Pos2::new(b.0, field.max.y), Pos2::new(a.0, field.max.y)], ICE.gamma_multiply(0.28), Stroke::NONE));
        painter.line_segment([Pos2::new(a.0, a.1), Pos2::new(b.0, b.1)], Stroke::new(1.0, ICE.gamma_multiply(0.6)));
    }
    // Three of them, along the Wall, one way then the other.
    let h = (field.height() * 0.3).clamp(30.0, 42.0);
    let gap = h * 1.1;
    let span = field.width() + gap * 5.0;
    let lap = (now * 14.0 / span as f64) as u64;
    let dir = if lap.is_multiple_of(2) { 1.0 } else { -1.0 };
    let gone = (now * 14.0) as f32 % span;
    for n in 0..3 {
        let x = gone - gap * (1.0 + n as f32);
        let x = if dir > 0.0 { field.min.x + x } else { field.max.x - x };
        walker(painter, Pos2::new(x, field.max.y - 3.0), h, dir, now);
    }
    for i in 0..70u32 {
        let speed = 10.0 + noise(i) * 22.0;
        let y = (noise(i + 100) * field.height() + now as f32 * speed) % field.height();
        let sway = ((now as f32 * 0.7 + i as f32).sin()) * 7.0;
        let x = field.min.x + noise(i + 200) * field.width() + sway;
        painter.circle_filled(Pos2::new(x, field.min.y + y), 0.7 + noise(i + 300) * 1.1, Color32::WHITE.gamma_multiply(0.35 + noise(i + 400) * 0.5));
    }
    0.05
}

/// The Iron Throne at `c`: a fan of swords behind a seat.
pub(super) fn throne(painter: &egui::Painter, c: Pos2, s: f32, alpha: f32) {
    let iron = Color32::from_rgb(74, 78, 88).gamma_multiply(alpha);
    let edge = STEEL.gamma_multiply(alpha);
    for n in 0..23u32 {
        let spread = n as f32 / 22.0 - 0.5;
        let reach = s * (1.0 + noise(n + 40) * 0.5 - spread.abs() * 0.5);
        let a = c + Vec2::new(spread * s * 0.7, s * 0.2);
        let b = a + Vec2::new(spread * s * 1.3, -reach);
        painter.line_segment([a, b], Stroke::new(s * 0.022, edge));
        let across = (b - a).normalized().rot90() * s * 0.035;
        let guard = a + (b - a) * 0.86;
        painter.line_segment([guard - across, guard + across], Stroke::new(s * 0.012, GOLD.gamma_multiply(alpha)));
    }
    painter.rect_filled(Rect::from_center_size(c + Vec2::new(0.0, s * 0.24), Vec2::new(s * 0.72, s * 0.5)), 2.0, iron);
    painter.rect_filled(Rect::from_center_size(c + Vec2::new(0.0, s * 0.6), Vec2::new(s * 1.0, s * 0.24)), 2.0, iron);
    painter.rect_filled(Rect::from_center_size(c + Vec2::new(0.0, s * 0.82), Vec2::new(s * 1.4, s * 0.22)), 2.0, iron);
    for side in [-1.0, 1.0] {
        painter.rect_filled(Rect::from_center_size(c + Vec2::new(side * s * 0.42, s * 0.12), Vec2::new(s * 0.14, s * 0.46)), 2.0, iron);
    }
}

/// The theme's opening, `t` seconds in: false once it is over.
pub(super) fn intro(painter: &egui::Painter, screen: Rect, t: f32, words: &str) -> bool {
    const END: f32 = 5.0;
    if t >= END {
        return false;
    }
    let ease = |a: f32, b: f32| ((t - a) / (b - a)).clamp(0.0, 1.0);
    let veil = ease(0.0, 0.4) * (1.0 - ease(END - 0.6, END));
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha((veil * 245.0) as u8));
    let c = screen.center();
    let s = screen.height().min(screen.width()) * 0.24;
    throne(painter, c - Vec2::new(0.0, s * 0.3), s, ease(0.3, 1.5) * veil);
    let shown = ease(1.4, 2.2) * veil;
    painter.text(c + Vec2::new(0.0, s * 1.05), Align2::CENTER_CENTER, words, FontId::proportional(s * 0.2), STEEL.gamma_multiply(shown));
    for i in 0..120u32 {
        let y = (noise(i) * screen.height() + t * (30.0 + noise(i + 100) * 50.0)) % screen.height();
        let x = screen.min.x + noise(i + 200) * screen.width() + (t + i as f32).sin() * 9.0;
        painter.circle_filled(Pos2::new(x, screen.min.y + y), 0.9 + noise(i + 300) * 1.4, Color32::WHITE.gamma_multiply(veil * 0.7));
    }
    true
}
