//! The Lundi matin theme's touches, won by beating the executive committee on the roof: blood dripping
//! from the logo, the bars and the active tab, splatters on the sidebar, bloody hands on the corners of
//! the dialogs, the zombies shambling under a red moon at the foot of the sidebar (a hand bursting out
//! of the ground now and then), the stapler for a pointer, and a bloody entrance when it is picked.

use super::*;

/// Blood: dried, fresh, its shine.
pub(super) const DRIED: Color32 = Color32::from_rgb(92, 6, 10);
pub(super) const BLOOD: Color32 = Color32::from_rgb(170, 14, 22);
const SHINE: Color32 = Color32::from_rgb(232, 72, 72);
const SKIN: Color32 = Color32::from_rgb(143, 209, 106);
/// The guard's alert, in this theme.
pub(super) const ALERT: &str = "ÇA MORD !";

/// A number between 0 and 1, the same for the same `n`.
fn noise(n: u32) -> f32 {
    let h = n.wrapping_mul(2_654_435_761) ^ n.rotate_left(13).wrapping_mul(40_503);
    ((h ^ (h >> 15)) & 0xffff) as f32 / 65535.0
}

/// A splatter of blood around `c`, `r` wide: a blot, its droplets thrown around.
pub(super) fn splat(painter: &egui::Painter, c: Pos2, r: f32, seed: u32, alpha: f32) {
    let color = BLOOD.gamma_multiply(alpha);
    let dark = DRIED.gamma_multiply(alpha);
    painter.circle_filled(c, r * 0.55, dark);
    for k in 0..7 {
        let a = noise(seed + k) * std::f32::consts::TAU;
        painter.circle_filled(c + Vec2::angled(a) * r * 0.35, r * (0.25 + 0.2 * noise(seed + k + 20)), color);
    }
    for k in 0..12 {
        let a = noise(seed + k + 40) * std::f32::consts::TAU;
        let d = r * (0.7 + 0.9 * noise(seed + k + 60));
        painter.circle_filled(c + Vec2::angled(a) * d, r * (0.04 + 0.08 * noise(seed + k + 80)), color);
    }
    painter.circle_filled(c - Vec2::splat(r * 0.15), r * 0.12, SHINE.gamma_multiply(alpha * 0.5));
}

/// Blood along the top of `rect`, dripping down: drips growing and shrinking slowly, a drop falling now
/// and then. `fall` is how far the drops may fall under it.
pub(super) fn drips(painter: &egui::Painter, rect: Rect, now: f64, seed: u32, fall: f32) {
    painter.rect_filled(rect, rect.height() / 2.0, BLOOD);
    painter.line_segment([rect.left_top() + Vec2::new(2.0, 1.0), rect.right_top() + Vec2::new(-2.0, 1.0)], Stroke::new(1.0, SHINE.gamma_multiply(0.6)));
    let n = ((rect.width() / 34.0) as u32).max(1);
    for k in 0..n {
        let x = rect.min.x + rect.width() * (k as f32 + 0.2 + 0.6 * noise(seed + k)) / n as f32;
        let speed = 0.15 + 0.2 * noise(seed + k + 7);
        let grow = ((now * f64::from(speed) + f64::from(noise(seed + k + 9)) * 10.0).sin() as f32 * 0.5 + 0.5) * (4.0 + 10.0 * noise(seed + k + 3));
        let w = 2.0 + 2.0 * noise(seed + k + 5);
        let top = Pos2::new(x, rect.center().y);
        let tip = top + Vec2::new(0.0, rect.height() / 2.0 + grow);
        painter.line_segment([top, tip], Stroke::new(w, BLOOD));
        painter.circle_filled(tip, w * 0.8, BLOOD);
        // A drop, falling.
        let cycle = 3.0 + 4.0 * f64::from(noise(seed + k + 11));
        let t = ((now + f64::from(noise(seed + k + 13)) * cycle) % cycle) as f32;
        if t < 0.6 && fall > 0.0 {
            painter.circle_filled(tip + Vec2::new(0.0, t / 0.6 * fall), w * 0.7, BLOOD.gamma_multiply(1.0 - t / 0.6 * 0.5));
        }
    }
}

/// Faint old splatters on the sidebar's panel (always in the same places).
pub(super) fn panel_bg(painter: &egui::Painter, rect: Rect) {
    for k in 0..5 {
        let c = Pos2::new(rect.min.x + rect.width() * noise(k * 3 + 1), rect.min.y + rect.height() * (0.1 + 0.8 * noise(k * 3 + 2)));
        splat(painter, c, 26.0 + 30.0 * noise(k * 3 + 3), k * 97, 0.1);
    }
}

/// The category picked: a smear of blood along its slot.
pub(super) fn active_mark(painter: &egui::Painter, slot: Rect) {
    let smear = Rect::from_min_max(slot.left_bottom() - Vec2::new(-6.0, 5.0), slot.right_bottom() - Vec2::new(6.0, 2.0));
    painter.rect_filled(smear, 2.0, BLOOD.gamma_multiply(0.7));
    painter.circle_filled(slot.right_bottom() - Vec2::new(8.0, -2.0), 2.0, BLOOD.gamma_multiply(0.7));
}

/// A zombie's hand out of the ground at `c` (the wrist), `s` tall, risen by `rise` (0 to 1).
pub(super) fn hand(painter: &egui::Painter, c: Pos2, s: f32, rise: f32) {
    let up = s * rise;
    let wrist = c - Vec2::new(0.0, up * 0.45);
    painter.line_segment([c, wrist], Stroke::new(s * 0.26, SKIN.gamma_multiply(0.85)));
    let palm = wrist - Vec2::new(0.0, up * 0.18);
    painter.circle_filled(palm, s * 0.17 * rise.max(0.2), SKIN);
    for k in 0..5 {
        let a = -std::f32::consts::FRAC_PI_2 + (k as f32 - 2.0) * 0.32;
        let tip = palm + Vec2::angled(a) * up * (0.32 + if k == 2 { 0.08 } else { 0.0 });
        painter.line_segment([palm, tip], Stroke::new(s * 0.07, SKIN));
        painter.circle_filled(tip, s * 0.04, DRIED);
    }
    painter.circle_filled(palm + Vec2::new(s * 0.05, 0.0), s * 0.06, BLOOD);
    // The ground broken open around it.
    for k in 0..5 {
        let d = (k as f32 - 2.0) * s * 0.14;
        painter.circle_filled(c + Vec2::new(d, s * 0.02), s * (0.05 + 0.03 * noise(k)), Color32::from_rgb(60, 40, 34));
    }
}

/// The R of the logo on the rail: dripping blood.
pub(super) fn mark(painter: &egui::Painter, c: Pos2, now: f64) {
    paint_metal(painter, c, Align2::CENTER_CENTER, "R", 32.0, BLOOD, 1.0);
    for (k, dx) in [-6.0f32, 3.0, 8.0].into_iter().enumerate() {
        let grow = ((now * 0.4 + k as f64 * 1.3).sin() as f32 * 0.5 + 0.5) * 7.0;
        let top = c + Vec2::new(dx, 12.0);
        painter.line_segment([top, top + Vec2::new(0.0, 3.0 + grow)], Stroke::new(2.0, BLOOD));
        painter.circle_filled(top + Vec2::new(0.0, 3.0 + grow), 1.8, BLOOD);
    }
}

/// The title in the sidebar: LUNDI MATIN, dripping.
pub(super) fn logo(painter: &egui::Painter, rect: Rect, now: f64) {
    let c = rect.center();
    let galley = painter.layout_no_wrap("LUNDI MATIN".to_owned(), FontId::proportional(21.0), BLOOD);
    let at = c - galley.size() / 2.0 - Vec2::new(0.0, 3.0);
    let w = galley.size().x;
    painter.galley(at + Vec2::new(1.5, 1.5), painter.layout_no_wrap("LUNDI MATIN".to_owned(), FontId::proportional(21.0), Color32::from_black_alpha(160)), Color32::BLACK);
    painter.galley(at, galley.clone(), BLOOD);
    for k in 0..7u32 {
        let x = at.x + w * (0.06 + 0.88 * noise(k + 300));
        let grow = ((now * 0.35 + f64::from(k)).sin() as f32 * 0.5 + 0.5) * (4.0 + 8.0 * noise(k + 310));
        let top = Pos2::new(x, at.y + galley.size().y - 4.0);
        painter.line_segment([top, top + Vec2::new(0.0, 3.0 + grow)], Stroke::new(1.8, BLOOD));
        painter.circle_filled(top + Vec2::new(0.0, 3.0 + grow), 1.7, BLOOD);
    }
}

/// After a panel's title: a drop, then a thin line of blood fading out.
pub(super) fn title_marks(painter: &egui::Painter, rect: Rect, text_end: f32) {
    let y = rect.center().y;
    let x = text_end + 8.0;
    painter.circle_filled(Pos2::new(x, y + 1.0), 2.6, BLOOD);
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(x - 2.2, y), Pos2::new(x + 2.2, y), Pos2::new(x, y - 5.0)], BLOOD, Stroke::NONE));
    let end = rect.max.x - 8.0;
    if end > x + 12.0 {
        let n = 12;
        for k in 0..n {
            let a = x + 8.0 + (end - x - 8.0) * k as f32 / n as f32;
            let b = x + 8.0 + (end - x - 8.0) * (k + 1) as f32 / n as f32;
            painter.line_segment([Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(1.0, BLOOD.gamma_multiply(0.7 * (1.0 - k as f32 / n as f32))));
        }
    }
}

/// A split pane's frame: a red line, blood running down from its top.
pub(super) fn frame(painter: &egui::Painter, r: Rect, now: f64) {
    painter.rect_stroke(r.shrink(1.0), 3.0, Stroke::new(1.5, BLOOD.gamma_multiply(0.85)), egui::StrokeKind::Inside);
    drips(painter, Rect::from_min_size(r.min + Vec2::new(1.0, 0.0), Vec2::new(r.width() - 2.0, 3.0)), now, (r.min.x as u32).wrapping_add(r.min.y as u32), 0.0);
}

/// A bloody hand print, the fingers toward `dir` (radians), `s` wide.
fn print(painter: &egui::Painter, c: Pos2, s: f32, dir: f32) {
    let color = BLOOD.gamma_multiply(0.75);
    painter.circle_filled(c, s * 0.32, color);
    painter.circle_filled(c - Vec2::angled(dir) * s * 0.2, s * 0.26, color);
    for k in 0..4 {
        let a = dir + (k as f32 - 1.5) * 0.3;
        let base = c + Vec2::angled(a) * s * 0.28;
        let tip = c + Vec2::angled(a) * s * (0.62 + if k == 1 || k == 2 { 0.08 } else { 0.0 });
        painter.line_segment([base, tip], Stroke::new(s * 0.12, color));
        painter.circle_filled(tip, s * 0.06, color);
    }
    let thumb = dir + 1.4;
    painter.line_segment([c + Vec2::angled(thumb) * s * 0.2, c + Vec2::angled(thumb) * s * 0.48], Stroke::new(s * 0.12, color));
}

/// Two bloody hands on the corners of a dialog.
pub(super) fn corners(painter: &egui::Painter, r: Rect) {
    print(painter, r.left_top() + Vec2::new(10.0, 12.0), 30.0, -2.4);
    print(painter, r.right_bottom() - Vec2::new(12.0, 10.0), 30.0, 0.7);
}

/// A zombie from the side, shambling toward `dir` (1 right, -1 left), its feet at `foot`, `h` tall.
pub(super) fn shambler(painter: &egui::Painter, foot: Pos2, h: f32, dir: f32, now: f64, seed: u32) {
    let at = |x: f32, y: f32| foot + Vec2::new(x * dir, -y) * h;
    let step = (now * 2.4 + f64::from(seed)).sin() as f32;
    let dark = Color32::from_rgb(20, 14, 18);
    let shirt = [Color32::from_rgb(60, 70, 96), Color32::from_rgb(90, 40, 46), Color32::from_rgb(70, 74, 60)][seed as usize % 3];
    painter.line_segment([at(0.0, 0.45), at(0.12 * step, 0.0)], Stroke::new(h * 0.1, dark));
    painter.line_segment([at(0.0, 0.45), at(-0.12 * step, 0.0)], Stroke::new(h * 0.1, dark));
    painter.rect_filled(Rect::from_center_size(at(0.02, 0.62), Vec2::new(0.26, 0.36) * h), h * 0.05, shirt);
    painter.circle_filled(at(0.05, 0.62), h * 0.05, BLOOD);
    let arm = Stroke::new(h * 0.07, SKIN.gamma_multiply(0.8));
    painter.line_segment([at(0.05, 0.74), at(0.38, 0.7 + 0.03 * step)], arm);
    painter.line_segment([at(0.0, 0.7), at(0.34, 0.64 - 0.03 * step)], arm);
    let head = at(0.06 + 0.03 * step, 0.9);
    painter.circle_filled(head, h * 0.12, SKIN);
    painter.circle_filled(head + Vec2::new(0.05 * dir, -0.02) * h, h * 0.025, Color32::from_rgb(255, 230, 120));
    painter.line_segment([head + Vec2::new(0.02 * dir, 0.05) * h, head + Vec2::new(0.1 * dir, 0.05) * h], Stroke::new(h * 0.03, DRIED));
}

/// Night over the office tower at the foot of the sidebar: a red moon, the lit windows, the zombies
/// shambling by, a hand bursting out of the ground now and then. When to draw again.
pub(super) fn scene(painter: &egui::Painter, field: Rect, now: f64) -> f32 {
    let painter = painter.with_clip_rect(field);
    let ground = field.max.y - 14.0;
    // The sky going red toward the ground.
    let mut mesh = egui::Mesh::default();
    let top = Color32::TRANSPARENT;
    let low = Color32::from_rgba_unmultiplied(120, 10, 16, 70);
    mesh.colored_vertex(field.left_top(), top);
    mesh.colored_vertex(field.right_top(), top);
    mesh.colored_vertex(Pos2::new(field.min.x, ground), low);
    mesh.colored_vertex(Pos2::new(field.max.x, ground), low);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
    let moon = Pos2::new(field.max.x - 40.0, field.min.y + 38.0);
    painter.circle_filled(moon, 26.0, Color32::from_rgba_unmultiplied(200, 30, 30, 40));
    painter.circle_filled(moon, 17.0, Color32::from_rgb(196, 52, 44));
    painter.circle_filled(moon + Vec2::new(-5.0, -4.0), 4.0, Color32::from_rgb(170, 40, 36));
    painter.circle_filled(moon + Vec2::new(6.0, 5.0), 3.0, Color32::from_rgb(170, 40, 36));
    // The tower, a few windows still lit (one flickering).
    let tower = Rect::from_min_max(Pos2::new(field.min.x + 16.0, field.min.y + 30.0), Pos2::new(field.min.x + 74.0, ground));
    painter.rect_filled(tower, 0.0, Color32::from_rgb(16, 18, 26));
    for row in 0..((tower.height() - 10.0) / 12.0) as u32 {
        for col in 0..4u32 {
            let k = row * 4 + col;
            if noise(k + 500) < 0.3 {
                let flicker = if k % 7 == 0 { ((now * 9.0).sin() > 0.3) as u8 as f32 } else { 1.0 };
                let w = Rect::from_min_size(tower.min + Vec2::new(6.0 + col as f32 * 13.0, 8.0 + row as f32 * 12.0), Vec2::new(7.0, 6.0));
                painter.rect_filled(w, 0.0, Color32::from_rgb(255, 214, 120).gamma_multiply(0.75 * flicker));
            }
        }
    }
    painter.rect_filled(Rect::from_min_max(Pos2::new(field.min.x, ground), field.max), 0.0, Color32::from_rgb(28, 20, 22));
    // The zombies, each at its pace, coming back from the other side.
    for k in 0..4u32 {
        let pace = 9.0 + 7.0 * f64::from(noise(k + 600));
        let span = f64::from(field.width() + 60.0);
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        let along = ((now * pace + f64::from(noise(k + 610)) * span) % span) as f32 - 30.0;
        let x = if dir > 0.0 { field.min.x + along } else { field.max.x - along };
        shambler(&painter, Pos2::new(x, ground + 1.0), 34.0 + 8.0 * noise(k + 620), dir, now, k);
    }
    // A hand out of the ground, every 12 s, somewhere else each time.
    let cycle = 12.0;
    let n = (now / cycle) as u32;
    let t = (now % cycle) as f32;
    if t < 3.0 {
        let x = field.min.x + 90.0 + (field.width() - 130.0) * noise(n + 700);
        let rise = (t / 0.5).min(1.0) * (1.0 - ((t - 2.4) / 0.6).clamp(0.0, 1.0));
        hand(&painter, Pos2::new(x, ground + 2.0), 30.0, rise);
    }
    0.04
}

/// The stapler of the pointer, its mouth at `at`: grey and black, a staple ready, a drop of blood.
pub(super) fn pointer_stapler(painter: &egui::Painter, at: Pos2) {
    let dir = Vec2::new(0.62, 0.78).normalized();
    let side = Vec2::new(-dir.y, dir.x);
    let quad = |c: Pos2, len: f32, wide: f32| vec![c - dir * len / 2.0 - side * wide / 2.0, c + dir * len / 2.0 - side * wide / 2.0, c + dir * len / 2.0 + side * wide / 2.0, c - dir * len / 2.0 + side * wide / 2.0];
    let base = at + dir * 13.0 + side * 2.5;
    painter.add(egui::Shape::convex_polygon(quad(base + Vec2::new(1.5, 2.0), 26.0, 9.0), Color32::from_black_alpha(90), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(quad(base, 26.0, 9.0), Color32::from_rgb(40, 42, 48), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(quad(base - side * 3.0, 24.0, 5.0), Color32::from_rgb(170, 176, 186), Stroke::NONE));
    painter.line_segment([at, at + side * 4.0], Stroke::new(1.5, Color32::from_rgb(220, 224, 230)));
    painter.circle_filled(at - side * 1.0 + dir * 1.0, 1.8, BLOOD);
}

/// The pointer as a stapler (only for the plain arrow).
pub(super) fn pointer(ctx: &egui::Context) {
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    if !super::plain_pointer(ctx, at) {
        return;
    }
    ctx.set_cursor_icon(egui::CursorIcon::None);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("stapler-pointer")));
    pointer_stapler(&painter, at);
}

/// The theme just picked: blood splashes on the screen, runs down from the top, a hand slams on the
/// glass, the title. `t` seconds in; false once over.
pub(super) fn intro(painter: &egui::Painter, screen: Rect, t: f32, words: &str) -> bool {
    const END: f32 = 4.5;
    if t >= END {
        return false;
    }
    let ease = |a: f32, b: f32| ((t - a) / (b - a)).clamp(0.0, 1.0);
    let fade = 1.0 - ease(END - 0.7, END);
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha((ease(0.0, 0.3) * fade * 225.0) as u8));
    // The blood running down from the top, unevenly.
    let n = 40;
    for k in 0..n {
        let x = screen.min.x + screen.width() * (k as f32 + noise(k + 800)) / n as f32;
        let len = screen.height() * ease(0.2 + 0.4 * noise(k + 810), 2.6 + noise(k + 820)) * (0.25 + 0.6 * noise(k + 830));
        let w = 6.0 + 16.0 * noise(k + 840);
        painter.line_segment([Pos2::new(x, screen.min.y), Pos2::new(x, screen.min.y + len)], Stroke::new(w, BLOOD.gamma_multiply(fade)));
        painter.circle_filled(Pos2::new(x, screen.min.y + len), w * 0.6, BLOOD.gamma_multiply(fade));
    }
    painter.rect_filled(Rect::from_min_size(screen.min, Vec2::new(screen.width(), 18.0)), 0.0, BLOOD.gamma_multiply(fade));
    // Splashes, one after the other.
    for k in 0..9u32 {
        let at = 0.15 + k as f32 * 0.16;
        if t > at {
            let c = Pos2::new(screen.min.x + screen.width() * (0.1 + 0.8 * noise(k + 900)), screen.min.y + screen.height() * (0.15 + 0.7 * noise(k + 910)));
            splat(painter, c, (40.0 + 80.0 * noise(k + 920)) * ease(at, at + 0.12).sqrt(), k * 31, 0.9 * fade);
        }
    }
    // The hand on the glass, then the title.
    let c = screen.center();
    let s = screen.height().min(screen.width()) * 0.32;
    if t > 1.4 {
        let slam = ease(1.4, 1.55);
        print(painter, c - Vec2::new(0.0, s * 0.15), s * (1.4 - 0.4 * slam), -std::f32::consts::FRAC_PI_2);
    }
    let shown = ease(1.9, 2.6) * fade;
    paint_metal(painter, c + Vec2::new(0.0, s * 0.62), Align2::CENTER_CENTER, "LUNDI MATIN", s * 0.24, SHINE.gamma_multiply(shown), shown);
    painter.text(c + Vec2::new(0.0, s * 0.9), Align2::CENTER_CENTER, words, FontId::proportional(s * 0.08), Color32::from_rgb(220, 210, 200).gamma_multiply(shown));
    true
}
