//! Lundi matin's four floors of the tower, seen from above: the hall (marble, the reception, the
//! fountain), the open space (carpet tiles, the islands of desks), the boardroom (parquet, the long
//! table on its rug) and the roof (concrete, the CEO's helicopter on its pad). Floor gives the
//! furniture (where it stands, what it is: `Desk::k`); here, how it looks. Then the lights over it all,
//! and the shade on the edges.

use super::*;
use crate::zombie::{Desk, Map};

/// Where a point of the floor is on the page.
pub(super) type To<'a> = &'a dyn Fn(f32, f32) -> Pos2;

/// A number between 0 and 1, the same for the same `i, j` (the patterns' little variations).
fn hash(i: i32, j: i32) -> f32 {
    let h = (i as u32).wrapping_mul(374_761_393) ^ (j as u32).wrapping_mul(668_265_263);
    let h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// `rect` filled from `a` (top, or left) to `b` (bottom, or right).
fn gradient(painter: &egui::Painter, rect: Rect, a: Color32, b: Color32, vertical: bool) {
    let mut mesh = egui::Mesh::default();
    let (tl, tr, bl, br) = if vertical { (a, a, b, b) } else { (a, b, a, b) };
    mesh.colored_vertex(rect.left_top(), tl);
    mesh.colored_vertex(rect.right_top(), tr);
    mesh.colored_vertex(rect.left_bottom(), bl);
    mesh.colored_vertex(rect.right_bottom(), br);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
}

/// A disc from `inner` at its middle to `outer` on its edge (a light, a shade).
fn radial(painter: &egui::Painter, c: Pos2, r: f32, inner: Color32, outer: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, inner);
    let n = 40;
    for k in 0..=n {
        mesh.colored_vertex(c + Vec2::angled(k as f32 / n as f32 * std::f32::consts::TAU) * r, outer);
    }
    for k in 0..n {
        mesh.add_triangle(0, k + 1, k + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// An ellipse's outline, `w` × `h` around `c`, turned by `angle`.
fn ellipse(c: Pos2, w: f32, h: f32, angle: f32) -> Vec<Pos2> {
    let (dir, side) = (Vec2::angled(angle), Vec2::angled(angle + std::f32::consts::FRAC_PI_2));
    (0..28).map(|k| {
        let a = k as f32 / 28.0 * std::f32::consts::TAU;
        c + dir * a.cos() * w / 2.0 + side * a.sin() * h / 2.0
    }).collect()
}

fn poly(painter: &egui::Painter, points: Vec<Pos2>, color: Color32) {
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A box's rect on the page.
fn rect_of(to: To, d: &Desk) -> Rect {
    Rect::from_min_max(to(d.x, d.y), to(d.x + d.w, d.y + d.h))
}

/// The ground, the walls, the doors and the furniture of `map` (under everyone).
pub(super) fn paint_floor(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64, boss: bool) {
    let field = Rect::from_min_max(to(0.0, 0.0), to(map.w, map.h));
    match map.id.as_str() {
        "hall" => ground_marble(painter, map, to, s),
        "board" => ground_parquet(painter, map, to, s),
        "roof" => ground_roof(painter, map, to, s, now),
        "canteen" => ground_lino(painter, map, to, s),
        "servers" => ground_raised(painter, map, to, s, now),
        "openspace" => ground_office(painter, map, to, s),
        _ => ground_carpet(painter, map, to, s),
    }
    // The lights of the ceiling on the ground (the sun, on the roof).
    if map.id != "roof" {
        // One every 400 × 500 of the floor, whatever its size.
        for i in 0..(map.w / 400.0).round().max(1.0) as i32 {
            for j in 0..(map.h / 500.0).round().max(1.0) as i32 {
                let c = to(200.0 + i as f32 * 400.0, 250.0 + j as f32 * 500.0);
                radial(painter, c, 230.0 * s, Color32::from_rgba_unmultiplied(255, 244, 214, 26), Color32::TRANSPARENT);
            }
        }
    }
    rooms(painter, map, to, s);
    decals(painter, map, to, s);
    walls(painter, map, to, s, field);
    inner_walls(painter, map, to, s, now);
    doors(painter, map, to, s, now);
    for d in &map.desks {
        furniture(painter, d, to, s, now, boss);
    }
    match map.id.as_str() {
        "roof" => roof_night(painter, map, to, s, now),
        "hall" => hall_morning(painter, map, to, s, now),
        "openspace" => openspace_neons(painter, map, to, s, now),
        "canteen" => canteen_lamps(painter, map, to, s, now),
        "servers" => servers_dark(painter, map, to, s, now),
        "board" => board_sunset(painter, map, to, s, now),
        _ => {}
    }
}

/// The windows along a floor's outer wall (the top one in the hall, the bottom one elsewhere): their
/// left edge, where no door is.
fn windows(map: &Map) -> Vec<f32> {
    let y = if map.id == "hall" { 0.0 } else { map.h - map.wall };
    let mut out = Vec::new();
    let mut x = 260.0;
    while x < 1360.0 {
        if !map.doors.iter().chain(map.reception.as_ref()).any(|d| d.along == "x" && (d.x - x - 50.0).abs() < d.span / 2.0 + 60.0 && (d.y - y).abs() < 60.0) {
            out.push(x);
        }
        x += 120.0;
    }
    out
}

/// Light coming in by the windows: a slanted shaft from each (`down`: from the top wall), `long`
/// across the floor, `color` (added), shimmering a little.
#[allow(clippy::too_many_arguments)]
fn shafts(painter: &egui::Painter, map: &Map, to: To, now: f64, down: bool, long: f32, slant: f32, color: (f32, f32, f32)) {
    let (y0, dir) = if down { (map.wall, 1.0) } else { (map.h - map.wall, -1.0) };
    for (k, x) in windows(map).into_iter().enumerate() {
        let shimmer = 0.8 + 0.2 * ((now * 0.7 + k as f64 * 1.3).sin() as f32);
        let mut mesh = egui::Mesh::default();
        let c = |f: f32| Color32::from_rgba_premultiplied((color.0 * f * shimmer) as u8, (color.1 * f * shimmer) as u8, (color.2 * f * shimmer) as u8, 0);
        mesh.colored_vertex(to(x, y0), c(1.0));
        mesh.colored_vertex(to(x + 100.0, y0), c(1.0));
        mesh.colored_vertex(to(x + 100.0 + slant, y0 + dir * long), c(0.0));
        mesh.colored_vertex(to(x - 30.0 + slant, y0 + dir * long), c(0.0));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// The hall, a sunny morning: the sun coming in by the glass front in long shafts, warm over the
/// marble; the fountain's water sparkling.
fn hall_morning(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let all = Rect::from_min_max(to(0.0, 0.0), to(map.w, map.h));
    gradient(painter, Rect::from_min_max(all.min, Pos2::new(all.max.x, to(0.0, 400.0).y)), glow(16, 12, 4), Color32::TRANSPARENT, true);
    shafts(painter, map, to, now, true, 800.0, 380.0, (46.0, 36.0, 16.0));
    for d in map.desks.iter().filter(|d| d.k == "fountain") {
        let c = to(d.x + d.w / 2.0, d.y + d.h / 2.0);
        radial(painter, c, d.w * 0.9 * s, glow(30, 60, 80), Color32::TRANSPARENT);
        for k in 0..10 {
            let a = hash(k, 3) * std::f32::consts::TAU;
            let r = (0.2 + 0.25 * hash(3, k)) * d.w * s;
            let on = ((now * 3.0 + k as f64 * 0.7).sin() as f32).max(0.0);
            painter.circle_filled(c + Vec2::new(a.cos() * r, a.sin() * r * 0.6), 2.0 * s * on, Color32::from_white_alpha((220.0 * on) as u8));
        }
    }
}

/// The open space by day: soft daylight from the windows, a long pendant lamp over each island (its
/// light a soft pool the island's shape), and over the table; one lamp failing, buzzing on and off.
fn openspace_neons(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    shafts(painter, map, to, now, false, 500.0, -160.0, (40.0, 38.0, 30.0));
    for (k, d) in map.desks.iter().filter(|d| d.k == "desk" || d.k == "table" || d.k == "babyfoot").enumerate() {
        let failing = k == 3;
        let on = !failing || ((now * 13.0).sin() + (now * 7.1).sin() * 0.8) > 0.2;
        let c = to(d.x + d.w / 2.0, d.y + d.h / 2.0);
        let pool = Rect::from_center_size(c, Vec2::new(d.w + 120.0, d.h + 120.0) * s);
        if on {
            // A soft rectangle: bright in, fading to its edges.
            let mut mesh = egui::Mesh::default();
            let (inner, outer) = (glow(30, 30, 26), Color32::TRANSPARENT);
            let core = pool.shrink2(Vec2::new(70.0, 60.0) * s);
            for p in [core.left_top(), core.right_top(), core.right_bottom(), core.left_bottom()] {
                mesh.colored_vertex(p, inner);
            }
            for p in [pool.left_top(), pool.right_top(), pool.right_bottom(), pool.left_bottom()] {
                mesh.colored_vertex(p, outer);
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            for e in 0..4u32 {
                let n = (e + 1) % 4;
                mesh.add_triangle(e, n, 4 + e);
                mesh.add_triangle(n, 4 + n, 4 + e);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        // The lamp itself, hanging over the middle: a thin bar, lit or not.
        let bar = Rect::from_center_size(c, Vec2::new(d.w * 0.8, 5.0) * s);
        painter.rect_filled(bar.translate(Vec2::new(4.0, 8.0) * s), 2.0, Color32::from_black_alpha(40));
        painter.rect_filled(bar, 2.5 * s, if on { rgb(250, 248, 236) } else { rgb(90, 92, 100) });
        if on {
            painter.rect_filled(bar.expand(3.0 * s), 4.0 * s, glow(40, 38, 30));
        }
    }
}

/// The canteen: warm lamps hanging over the tables (swaying a little), the kitchen's stoves alight
/// (their flames, their glow), the vending machines glowing.
fn canteen_lamps(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let all = Rect::from_min_max(to(0.0, 0.0), to(map.w, map.h));
    painter.rect_filled(all, 0.0, Color32::from_rgba_unmultiplied(30, 16, 6, 40));
    for (k, d) in map.desks.iter().enumerate() {
        let c = to(d.x + d.w / 2.0, d.y + d.h / 2.0);
        match d.k.as_str() {
            "cantable" => {
                for f in [0.25, 0.75] {
                    let sway = Vec2::new((now as f32 * 0.9 + k as f32 + f).sin() * 6.0, 0.0) * s;
                    let p = to(d.x + d.w * f, d.y + d.h / 2.0) + sway;
                    radial(painter, p, 170.0 * s, glow(60, 40, 16), Color32::TRANSPARENT);
                    // The shade from above: copper, its bulb glowing inside.
                    painter.circle_filled(p + Vec2::new(3.0, 5.0) * s, 15.0 * s, Color32::from_black_alpha(50));
                    radial(painter, p, 15.0 * s, rgb(214, 140, 80), rgb(150, 86, 44));
                    painter.circle_stroke(p, 15.0 * s, Stroke::new(1.5 * s, rgb(240, 180, 110)));
                    radial(painter, p, 7.0 * s, rgb(255, 250, 220), rgb(255, 214, 140));
                }
            }
            // In the kitchen (the wing below): stoves, alight.
            "buffet" if d.y > 1000.0 => {
                for m in 0..4 {
                    let b = to(d.x + d.w * (0.14 + 0.24 * m as f32), d.y + d.h / 2.0);
                    radial(painter, b, 50.0 * s, glow(90, 46, 10), Color32::TRANSPARENT);
                    painter.circle_stroke(b, 12.0 * s, Stroke::new(2.0 * s, rgb(40, 40, 44)));
                    for q in 0..8 {
                        let a = q as f32 / 8.0 * std::f32::consts::TAU;
                        let tall = 5.0 + 4.0 * ((now * 17.0 + q as f64 + m as f64).sin() as f32 * 0.5 + 0.5);
                        painter.line_segment([b + Vec2::angled(a) * 7.0 * s, b + Vec2::angled(a) * (7.0 + tall) * s], Stroke::new(2.5 * s, if q % 2 == 0 { rgb(90, 150, 255) } else { rgb(255, 170, 60) }));
                    }
                }
            }
            "vending" => radial(painter, c, 90.0 * s, if k % 2 == 0 { glow(20, 60, 90) } else { glow(90, 20, 30) }, Color32::TRANSPARENT),
            _ => {}
        }
    }
}

/// The basement: dark, but for the racks' cold blue (their lights in the dark), red beacons turning on
/// the walls.
fn servers_dark(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let all = Rect::from_min_max(to(0.0, 0.0), to(map.w, map.h));
    painter.rect_filled(all, 0.0, Color32::from_rgba_unmultiplied(4, 8, 22, 140));
    for d in map.desks.iter().filter(|d| d.k == "rack") {
        let r = Rect::from_min_max(to(d.x, d.y), to(d.x + d.w, d.y + d.h));
        // The cold aisle each side, blue.
        for side in [-1.0, 1.0] {
            let band = if d.h > d.w { Rect::from_center_size(r.center() + Vec2::new(side * (r.width() / 2.0 + 30.0 * s), 0.0), Vec2::new(60.0 * s, r.height())) } else { Rect::from_center_size(r.center() + Vec2::new(0.0, side * (r.height() / 2.0 + 30.0 * s)), Vec2::new(r.width(), 60.0 * s)) };
            let (a, b) = (glow(10, 34, 70), Color32::TRANSPARENT);
            if d.h > d.w {
                gradient(painter, band, if side < 0.0 { b } else { a }, if side < 0.0 { a } else { b }, false);
            } else {
                gradient(painter, band, if side < 0.0 { b } else { a }, if side < 0.0 { a } else { b }, true);
            }
        }
    }
    for d in map.desks.iter().filter(|d| d.k == "ups") {
        radial(painter, to(d.x + d.w / 2.0, d.y + d.h / 2.0), 70.0 * s, glow(20, 70, 30), Color32::TRANSPARENT);
    }
    // Beacons on the walls, their red beams turning.
    let spots = [(map.wall + 6.0, map.h / 2.0), (map.w / 2.0, map.wall + 6.0), (map.w - map.wall - 6.0, map.h - 120.0), (map.w / 2.0, map.h - map.wall - 6.0)];
    for (k, (x, y)) in spots.into_iter().enumerate() {
        let c = to(x, y);
        let a = now as f32 * 2.2 + k as f32 * 1.7;
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c, glow(90, 6, 4));
        for q in 0..=8 {
            mesh.colored_vertex(c + Vec2::angled(a - 0.25 + 0.5 * q as f32 / 8.0) * 420.0 * s, Color32::TRANSPARENT);
        }
        for q in 0..8 {
            mesh.add_triangle(0, q + 1, q + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.circle_filled(c, 8.0 * s, rgb(255, 50, 40));
        radial(painter, c, 40.0 * s, glow(140, 20, 10), Color32::TRANSPARENT);
    }
}

/// The top floor at sunset: the low sun in long orange shafts by the windows, the boardroom's two
/// chandeliers over the table, the CEO's aquarium glowing blue (its fish going round).
fn board_sunset(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let all = Rect::from_min_max(to(0.0, 0.0), to(map.w, map.h));
    gradient(painter, all, Color32::from_rgba_unmultiplied(40, 10, 30, 50), Color32::TRANSPARENT, true);
    gradient(painter, Rect::from_min_max(to(0.0, map.h - 500.0), to(map.w, map.h)), Color32::TRANSPARENT, glow(40, 18, 4), true);
    shafts(painter, map, to, now, false, 900.0, -420.0, (80.0, 38.0, 10.0));
    for d in &map.desks {
        match d.k.as_str() {
            "boardtable" => {
                for f in [0.27, 0.73] {
                    let c = to(d.x + d.w * f, d.y + d.h / 2.0);
                    radial(painter, c, 230.0 * s, glow(70, 54, 22), Color32::TRANSPARENT);
                    // The chandelier from above: its arms and crystals, sparkling.
                    for k in 0..8 {
                        let a = k as f32 / 8.0 * std::f32::consts::TAU + 0.2;
                        let tip = c + Vec2::angled(a) * 34.0 * s;
                        painter.line_segment([c, tip], Stroke::new(2.0 * s, rgb(212, 172, 84)));
                        let on = ((now * 2.5 + k as f64 * 0.9).sin() as f32 * 0.5 + 0.5) * 0.8 + 0.2;
                        painter.circle_filled(tip, 4.0 * s, Color32::from_rgba_unmultiplied(255, 246, 220, (255.0 * on) as u8));
                    }
                    painter.circle_filled(c, 9.0 * s, rgb(240, 214, 140));
                }
            }
            "aquarium" => {
                let r = Rect::from_min_max(to(d.x, d.y), to(d.x + d.w, d.y + d.h));
                radial(painter, r.center(), d.w * 0.8 * s, glow(10, 50, 80), Color32::TRANSPARENT);
                painter.rect_filled(r.translate(Vec2::new(4.0, 6.0) * s), 3.0 * s, Color32::from_black_alpha(80));
                gradient(painter, r, rgb(40, 150, 200), rgb(20, 70, 130), true);
                painter.rect_stroke(r, 3.0 * s, Stroke::new(3.0 * s, rgb(30, 30, 36)), egui::StrokeKind::Inside);
                let clip = painter.with_clip_rect(r.shrink(3.0 * s).intersect(painter.clip_rect()));
                for k in 0..6 {
                    let f = ((now * (0.06 + 0.03 * hash(k, 2) as f64) + hash(k, 1) as f64) % 1.0) as f32;
                    let back = k % 2 == 0;
                    let x = if back { r.min.x + f * r.width() } else { r.max.x - f * r.width() };
                    let y = r.min.y + (0.2 + 0.6 * hash(k, 4)) * r.height() + (now as f32 * 2.0 + k as f32).sin() * 3.0 * s;
                    let dir = if back { 1.0 } else { -1.0 };
                    let color = [rgb(255, 150, 40), rgb(255, 220, 80), rgb(240, 90, 120)][k as usize % 3];
                    poly(&clip, ellipse(Pos2::new(x, y), 14.0 * s, 7.0 * s, 0.0), color);
                    poly(&clip, vec![Pos2::new(x - dir * 6.0 * s, y), Pos2::new(x - dir * 13.0 * s, y - 5.0 * s), Pos2::new(x - dir * 13.0 * s, y + 5.0 * s)], color);
                }
                for k in 0..5 {
                    let f = ((now * 0.5 + k as f64 * 0.21) % 1.0) as f32;
                    clip.circle_stroke(Pos2::new(r.min.x + (0.15 + 0.17 * k as f32) * r.width(), r.max.y - f * r.height()), 2.0 * s, Stroke::new(1.0, Color32::from_white_alpha((160.0 * (1.0 - f)) as u8)));
                }
                // Plants swaying at the bottom.
                for k in 0..4 {
                    let b = Pos2::new(r.min.x + (0.12 + 0.25 * k as f32) * r.width(), r.max.y - 3.0 * s);
                    let sway = (now as f32 * 1.3 + k as f32).sin() * 4.0 * s;
                    clip.line_segment([b, b + Vec2::new(sway, -r.height() * 0.5)], Stroke::new(3.0 * s, rgb(60, 160, 90)));
                }
            }
            _ => {}
        }
    }
}

/// Additive light (added to what's under it, never darkening).
fn glow(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgba_premultiplied(r, g, b, 0)
}

/// The storm's lightning at `now`: how bright its flash (0 to 1), how long since it struck (s), which
/// one it is. One every EVERY seconds, at a moment of its own in them.
pub(super) fn lightning(now: f64) -> (f32, f32, i32) {
    const EVERY: f64 = 9.0;
    let k = (now / EVERY).floor();
    let at = k * EVERY + hash(k as i32, 77) as f64 * 5.0;
    let dt = (now - at) as f32;
    let flash = match dt {
        d if d < 0.0 => 0.0,
        d if d < 0.07 => 1.0,
        d if d < 0.14 => 0.25,
        d if d < 0.22 => 0.85,
        d => (1.0 - (d - 0.22) / 0.5).max(0.0) * 0.6,
    };
    (flash, dt, k as i32)
}

/// The roof at night, under everyone: the dark blue over the concrete, the city's orange glow rising
/// from below the parapet, two searchlights sweeping the terrace from their corners, the giant neon
/// sign along the bottom edge (a letter flickering), the red beacons on the corners.
fn roof_night(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let all = Rect::from_min_max(to(0.0, 0.0), to(map.w, map.h));
    painter.rect_filled(all, 0.0, Color32::from_rgba_unmultiplied(6, 10, 34, 120));
    // The city below: its glow over the edges.
    let edge = 240.0;
    gradient(painter, Rect::from_min_max(to(0.0, map.h - edge), to(map.w, map.h)), Color32::TRANSPARENT, glow(70, 30, 8), true);
    gradient(painter, Rect::from_min_max(to(0.0, 0.0), to(map.w, edge * 0.6)), glow(30, 14, 40), Color32::TRANSPARENT, true);
    gradient(painter, Rect::from_min_max(to(map.w - edge, 0.0), to(map.w, map.h)), Color32::TRANSPARENT, glow(50, 24, 10), false);

    // The searchlights: a beam on the ground, its spot at the end, the lamp in its corner.
    let t = now as f32;
    for (x, y, base, sweep, speed) in [(map.wall + 24.0, map.h - map.wall - 24.0, -0.62, 0.5, 0.33), (map.w - map.wall - 24.0, map.wall + 24.0, 2.55, 0.45, 0.27)] {
        let o = to(x, y);
        let aim = base + sweep * (t * speed + x * 0.01).sin();
        let len = 1500.0 * s;
        let half = 0.085;
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(o, glow(84, 80, 60));
        let n = 12;
        for k in 0..=n {
            let a = aim - half + 2.0 * half * k as f32 / n as f32;
            mesh.colored_vertex(o + Vec2::angled(a) * len, Color32::TRANSPARENT);
        }
        for k in 0..n {
            mesh.add_triangle(0, k + 1, k + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
        let spot = o + Vec2::angled(aim) * len * 0.62;
        poly(painter, ellipse(spot, 260.0 * s, 150.0 * s, aim), glow(26, 25, 18));
        radial(painter, spot, 90.0 * s, glow(60, 58, 42), Color32::TRANSPARENT);
        painter.circle_filled(o + Vec2::new(4.0, 6.0) * s, 22.0 * s, Color32::from_black_alpha(90));
        painter.circle_filled(o, 22.0 * s, rgb(44, 46, 52));
        painter.circle_stroke(o, 22.0 * s, Stroke::new(2.0 * s, rgb(90, 94, 102)));
        let lens = o + Vec2::angled(aim) * 10.0 * s;
        painter.circle_filled(lens, 11.0 * s, rgb(255, 250, 220));
        radial(painter, lens, 46.0 * s, glow(120, 116, 90), Color32::TRANSPARENT);
    }

    // The neon sign, its letters standing along the bottom parapet, their pink on the concrete.
    let word: Vec<char> = "LUNDI MATIN".chars().collect();
    let (from, step, y) = (300.0, 94.0, map.h - map.wall - 50.0);
    let rail = Stroke::new(5.0 * s, rgb(40, 42, 48));
    painter.line_segment([to(from - 50.0, y + 24.0), to(from + step * (word.len() as f32 - 1.0) + 50.0, y + 24.0)], rail);
    let window = (now / 3.0).floor();
    let flickering = (hash(window as i32, 5) * word.len() as f32) as usize;
    let off = (now % 3.0) < 0.9 && (now * 7.3).fract() < 0.45;
    for (k, ch) in word.iter().enumerate() {
        if *ch == ' ' {
            continue;
        }
        let c = to(from + step * k as f32, y);
        painter.line_segment([c + Vec2::new(-14.0, 24.0) * s, c + Vec2::new(-14.0, 6.0) * s], Stroke::new(3.0 * s, rgb(60, 62, 70)));
        painter.line_segment([c + Vec2::new(14.0, 24.0) * s, c + Vec2::new(14.0, 6.0) * s], Stroke::new(3.0 * s, rgb(60, 62, 70)));
        let lit = !(k == flickering && off);
        let font = FontId::proportional(78.0 * s);
        if lit {
            radial(painter, c - Vec2::new(0.0, 20.0 * s), 150.0 * s, glow(70, 10, 40), Color32::TRANSPARENT);
            radial(painter, c, 70.0 * s, glow(110, 20, 64), Color32::TRANSPARENT);
            // The tube: thick and pink, its white-hot core.
            for k in 0..12 {
                let d = Vec2::angled(k as f32 / 12.0 * std::f32::consts::TAU) * 3.5 * s;
                painter.text(c + d, Align2::CENTER_CENTER, ch, font.clone(), Color32::from_rgba_unmultiplied(255, 40, 140, 90));
            }
            for d in [Vec2::new(-1.2, 0.0), Vec2::new(1.2, 0.0)] {
                painter.text(c + d * s, Align2::CENTER_CENTER, ch, font.clone(), rgb(255, 120, 190));
            }
            painter.text(c, Align2::CENTER_CENTER, ch, font, rgb(255, 228, 244));
        } else {
            painter.text(c, Align2::CENTER_CENTER, ch, font, rgb(70, 30, 50));
        }
    }

    // The red beacons on the corners, all blinking together.
    let blink = (now * 1.2).fract() < 0.25;
    for d in map.desks.iter().filter(|d| d.k == "skylight") {
        radial(painter, to(d.x + d.w / 2.0, d.y + d.h / 2.0), d.w.max(d.h) * s, glow(80, 56, 24), Color32::TRANSPARENT);
    }
    let mut beacons = vec![(map.wall, map.wall), (map.w - map.wall, map.wall), (map.wall, map.h - map.wall), (map.w - map.wall, map.h - map.wall)];
    for v in map.walls.iter().filter(|w| w.k == "void") {
        beacons.extend([(v.x, v.y), (v.x + v.w, v.y), (v.x, v.y + v.h), (v.x + v.w, v.y + v.h)]);
    }
    for (x, y) in beacons {
        let c = to(x, y);
        painter.circle_filled(c, 6.0 * s, if blink { rgb(255, 60, 50) } else { rgb(90, 24, 22) });
        if blink {
            radial(painter, c, 60.0 * s, glow(120, 20, 14), Color32::TRANSPARENT);
        }
    }
}

/// The air over everyone, per floor: the dust in the hall's sun, papers blown about the open space,
/// the kitchen's steam, the basement's cold mist; the storm on the roof.
pub(super) fn paint_air(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64, field: Rect) {
    let t = now as f32;
    match map.id.as_str() {
        "roof" => paint_storm(painter, map, to, s, now, field),
        "hall" => {
            for k in 0..90 {
                let x = (hash(k, 1) * map.w + t * (4.0 + 6.0 * hash(k, 2))).rem_euclid(map.w);
                let y = hash(k, 3) * 900.0 + (t * 0.4 + k as f32).sin() * 20.0;
                let on = ((t * 0.8 + k as f32).sin() * 0.5 + 0.5) * 0.8;
                painter.circle_filled(to(x, y), 1.6 * s.max(0.6), Color32::from_rgba_unmultiplied(255, 236, 190, (120.0 * on) as u8));
            }
        }
        "openspace" => {
            // Sheets of paper blown by the broken window's draught, end over end.
            for k in 0..8 {
                let f = (hash(k, 21) + t * (0.04 + hash(k, 22) * 0.03)).fract();
                let x = map.w - f * map.w;
                let y = 200.0 + hash(k, 23) * (map.h - 400.0) + (t * 1.5 + k as f32).sin() * 60.0;
                let c = to(x, y);
                let a = t * (1.5 + hash(k, 24) * 2.0);
                let (u, v) = (Vec2::angled(a), Vec2::angled(a + 1.57));
                let flip = (t * 3.0 + k as f32).sin().abs().max(0.25);
                poly(painter, vec![c - u * 9.0 * s - v * 12.0 * s * flip, c + u * 9.0 * s - v * 12.0 * s * flip, c + u * 9.0 * s + v * 12.0 * s * flip, c - u * 9.0 * s + v * 12.0 * s * flip], Color32::from_white_alpha(190));
            }
        }
        "canteen" => {
            for d in map.desks.iter().filter(|d| d.k == "buffet" && d.y > 1000.0) {
                for k in 0..10 {
                    let f = ((t * 0.35 + hash(k, d.x as i32) * 1.0) % 1.0).fract();
                    let x = d.x + hash(k, 5) * d.w + (t + k as f32).sin() * 14.0;
                    let y = d.y + d.h / 2.0 - f * 260.0;
                    painter.circle_filled(to(x, y), (14.0 + 30.0 * f) * s, Color32::from_white_alpha((50.0 * (1.0 - f)) as u8));
                }
            }
        }
        "servers" => {
            for k in 0..26 {
                let x = (hash(k, 31) * map.w + t * (10.0 + 10.0 * hash(k, 32))).rem_euclid(map.w + 400.0) - 200.0;
                let y = hash(k, 33) * map.h;
                let c = to(x, y);
                poly(painter, ellipse(c, (260.0 + 200.0 * hash(k, 34)) * s, (60.0 + 40.0 * hash(k, 35)) * s, 0.0), Color32::from_rgba_unmultiplied(150, 190, 230, 14));
            }
        }
        _ => {}
    }
}

/// The storm over the roof, over everyone: the rain slanting across the screen and splashing, the
/// leaves blown by, the lightning striking the antenna and its white flash over it all.
fn paint_storm(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64, field: Rect) {
    let t = now as f32;
    let wind = Vec2::new(-0.32, 1.0);
    for k in 0..320 {
        let speed = 0.9 + hash(k, 2) * 0.6;
        let f = (hash(k, 3) + t * speed * 0.9).fract();
        let x = field.min.x + (hash(k, 7) * 1.2 * field.width() - f * 0.32 * field.height()).rem_euclid(field.width());
        let p = Pos2::new(x, field.min.y + f * field.height());
        let long = (14.0 + hash(k, 4) * 14.0) * s.max(0.6);
        painter.line_segment([p, p + wind.normalized() * long], Stroke::new(1.0, Color32::from_rgba_unmultiplied(190, 210, 255, (40.0 + 50.0 * hash(k, 5)) as u8)));
    }
    // The splashes on the ground: a ring opening, then gone.
    for k in 0..70 {
        let period = 0.5 + hash(k, 11) * 0.4;
        let f = ((t + hash(k, 12) * 3.0) / period).fract();
        let round = ((t + hash(k, 12) * 3.0) / period).floor() as i32;
        let p = Pos2::new(field.min.x + hash(k, round) * field.width(), field.min.y + hash(round, k) * field.height());
        painter.circle_stroke(p, (2.0 + f * 7.0) * s.max(0.6), Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 220, 255, (80.0 * (1.0 - f)) as u8)));
    }
    // Leaves and a sheet of paper, blown across.
    for k in 0..9 {
        let f = (hash(k, 21) + t * (0.05 + hash(k, 22) * 0.05)).fract();
        let p = Pos2::new(field.max.x + 40.0 - f * (field.width() + 80.0), field.min.y + hash(k, 23) * field.height() + (t * 2.0 + k as f32).sin() * 30.0 * s);
        let a = t * (2.0 + hash(k, 24) * 3.0);
        let (u, v) = (Vec2::angled(a), Vec2::angled(a + 1.6));
        if k == 0 {
            poly(painter, vec![p - u * 9.0 * s - v * 12.0 * s, p + u * 9.0 * s - v * 12.0 * s, p + u * 9.0 * s + v * 12.0 * s, p - u * 9.0 * s + v * 12.0 * s], Color32::from_white_alpha(150));
        } else {
            poly(painter, vec![p + u * 8.0 * s, p + v * 3.0 * s, p - u * 8.0 * s, p - v * 3.0 * s], rgb(120, 100, 50));
        }
    }

    // The lightning: a bolt down onto the antenna, its sparks, the flash.
    let (flash, dt, n) = lightning(now);
    if let Some(rod) = map.desks.iter().find(|d| d.k == "antenna")
        && (0.0..0.3).contains(&dt)
    {
        let end = to(rod.x + rod.w / 2.0, rod.y + rod.h / 2.0);
        let start = end + Vec2::new(-260.0 + hash(n, 1) * 520.0, -900.0) * s;
        let mut pts = vec![start];
        let steps = 12;
        for m in 1..steps {
            let along = start.lerp(end, m as f32 / steps as f32);
            pts.push(along + Vec2::new((hash(n, m + 2) - 0.5) * 90.0 * s, (hash(m, n) - 0.5) * 30.0 * s));
        }
        pts.push(end);
        let fade = 1.0 - dt / 0.3;
        let branch: Vec<Pos2> = (0..5).map(|m| pts[5] + Vec2::new((m as f32 * 40.0 + (hash(n, m + 40) - 0.5) * 50.0) * if hash(n, 9) > 0.5 { 1.0 } else { -1.0 }, m as f32 * 45.0) * s).collect();
        for line in [&pts, &branch] {
            painter.add(egui::Shape::line(line.to_vec(), Stroke::new(16.0 * s, glow((90.0 * fade) as u8, (100.0 * fade) as u8, (180.0 * fade) as u8))));
            painter.add(egui::Shape::line(line.to_vec(), Stroke::new(3.5 * s, Color32::from_white_alpha((255.0 * fade) as u8))));
        }
        radial(painter, end, 140.0 * s, glow((200.0 * fade) as u8, (200.0 * fade) as u8, (255.0 * fade) as u8), Color32::TRANSPARENT);
        for m in 0..10 {
            let a = hash(n, m + 60) * std::f32::consts::TAU;
            let d = (20.0 + dt * 300.0 * hash(m, n + 3)) * s;
            painter.circle_filled(end + Vec2::angled(a) * d, 2.5 * s, Color32::from_rgba_unmultiplied(255, 240, 160, (255.0 * fade) as u8));
        }
    }
    if flash > 0.0 {
        let v = (150.0 * flash) as u8;
        painter.rect_filled(field, 0.0, glow(v, v, v.saturating_add(20)));
    }
}

/// Over everyone: the shade on the edges of the floor.
pub(super) fn paint_shade(painter: &egui::Painter, field: Rect, roof: bool) {
    let edge = field.width().min(field.height()) * 0.16;
    let dark = Color32::from_black_alpha(if roof { 70 } else { 95 });
    gradient(painter, Rect::from_min_size(field.min, Vec2::new(field.width(), edge)), dark, Color32::TRANSPARENT, true);
    gradient(painter, Rect::from_min_max(Pos2::new(field.min.x, field.max.y - edge), field.max), Color32::TRANSPARENT, dark, true);
    gradient(painter, Rect::from_min_size(field.min, Vec2::new(edge, field.height())), dark, Color32::TRANSPARENT, false);
    gradient(painter, Rect::from_min_max(Pos2::new(field.max.x - edge, field.min.y), field.max), Color32::TRANSPARENT, dark, false);
}

/// The hall: big tiles of veined marble, the red carpet from the entrance to the fountain.
fn ground_marble(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    let tile = 100.0;
    for i in 0..(map.w / tile).ceil() as i32 {
        for j in 0..(map.h / tile).ceil() as i32 {
            let v = hash(i, j);
            let base = if (i + j) % 2 == 0 { rgb(188, 182, 170) } else { rgb(170, 164, 154) };
            let r = Rect::from_min_size(to(i as f32 * tile, j as f32 * tile), Vec2::splat(tile * s + 0.5));
            painter.rect_filled(r, 0.0, base.lerp_to_gamma(Color32::WHITE, v * 0.12));
            // A vein across the tile.
            let a = to(i as f32 * tile + v * tile, j as f32 * tile);
            let b = to(i as f32 * tile + hash(j, i) * tile, (j + 1) as f32 * tile);
            let mid = a.lerp(b, 0.5) + Vec2::new((v - 0.5) * 30.0 * s, 0.0);
            let vein = Stroke::new(1.0 * s.max(0.6), Color32::from_black_alpha(22));
            painter.line_segment([a, mid], vein);
            painter.line_segment([mid, b], vein);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_black_alpha(30)), egui::StrokeKind::Inside);
        }
    }
    // The big hall (its rooms on the right): the server closet's concrete floor, the corridor's
    // carpet; the red carpet down from the revolving door to the fountain.
    let big = map.w > 1600.0;
    if big {
        let closet = Rect::from_min_max(to(1720.0, map.wall), to(map.w - map.wall, 500.0));
        painter.rect_filled(closet, 0.0, rgb(118, 120, 122));
        for k in 0..((map.w - 1720.0) / 120.0) as i32 {
            let x = 1720.0 + k as f32 * 120.0;
            painter.line_segment([to(x, map.wall), to(x, 500.0)], Stroke::new(1.0, Color32::from_black_alpha(30)));
        }
        let corridor = Rect::from_min_max(to(1720.0, 520.0), to(map.w - map.wall, 900.0));
        painter.rect_filled(corridor, 0.0, rgb(70, 66, 74));
    }
    let carpet = if big { Rect::from_min_max(to(760.0, map.wall), to(840.0, 580.0)) } else { Rect::from_min_max(to(900.0, 462.0), to(map.w - map.wall, 538.0)) };
    painter.rect_filled(carpet.translate(Vec2::new(0.0, 3.0 * s)), 0.0, Color32::from_black_alpha(50));
    painter.rect_filled(carpet, 0.0, rgb(128, 26, 38));
    painter.rect_stroke(carpet.shrink(5.0 * s), 0.0, Stroke::new(2.0 * s, rgb(212, 172, 84)), egui::StrokeKind::Inside);
    // The company's star, inlaid by the entrance.
    let c = if big { to(1250.0, 1180.0) } else { to(1250.0, 860.0) };
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let long = if k % 2 == 0 { 70.0 } else { 40.0 } * s;
        poly(painter, vec![c, c + Vec2::angled(a - 0.18) * long * 0.4, c + Vec2::angled(a) * long, c + Vec2::angled(a + 0.18) * long * 0.4], if k % 2 == 0 { rgb(120, 112, 100) } else { rgb(150, 142, 128) });
    }
}

/// The open space: carpet tiles laid in turns (the stripes one way, then the other).
fn ground_carpet(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    let tile = 64.0;
    for i in 0..(map.w / tile).ceil() as i32 {
        for j in 0..(map.h / tile).ceil() as i32 {
            let v = hash(i, j);
            let r = Rect::from_min_size(to(i as f32 * tile, j as f32 * tile), Vec2::splat(tile * s + 0.5));
            painter.rect_filled(r, 0.0, rgb(44, 50, 66).lerp_to_gamma(rgb(52, 60, 78), v * 0.6));
            let stripe = Stroke::new(1.0, Color32::from_white_alpha(7));
            for k in 1..6 {
                let f = k as f32 / 6.0;
                if (i + j) % 2 == 0 {
                    painter.line_segment([r.left_top() + Vec2::new(r.width() * f, 0.0), r.left_bottom() + Vec2::new(r.width() * f, 0.0)], stripe);
                } else {
                    painter.line_segment([r.left_top() + Vec2::new(0.0, r.height() * f), r.right_top() + Vec2::new(0.0, r.height() * f)], stripe);
                }
            }
        }
    }
    // The worn way down the middle aisle.
    // (In the room from before: the floor's wing has its own walls.)
    let right = map.w.min(1600.0) - 60.0;
    gradient(painter, Rect::from_min_max(to(60.0, 560.0), to(right, 600.0)), Color32::TRANSPARENT, Color32::from_white_alpha(8), true);
    gradient(painter, Rect::from_min_max(to(60.0, 600.0), to(right, 640.0)), Color32::from_white_alpha(8), Color32::TRANSPARENT, true);
}

/// The island's colors in the open space: its rug, its chairs (each island its own).
fn island_colors(d: &Desk) -> (Color32, Color32) {
    let k = ((d.x / 420.0) as usize + (d.y / 520.0) as usize * 3) % 4;
    [(rgb(72, 140, 142), rgb(240, 140, 60)), (rgb(204, 160, 70), rgb(50, 110, 150)), (rgb(206, 110, 92), rgb(60, 130, 120)), (rgb(104, 120, 176), rgb(236, 190, 70))][k]
}

/// The open space: light carpet tiles (their weave turning a quarter from one to the next), a colored
/// rug under each island, the light oak floor of the middle aisle.
fn ground_office(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    let tile = 60.0;
    for i in 0..(map.w / tile).ceil() as i32 {
        for j in 0..(map.h / tile).ceil() as i32 {
            let v = hash(i, j);
            let r = Rect::from_min_size(to(i as f32 * tile, j as f32 * tile), Vec2::splat(tile * s + 0.5));
            painter.rect_filled(r, 0.0, rgb(150, 148, 142).lerp_to_gamma(rgb(166, 163, 156), v));
            let weave = Stroke::new(1.0, Color32::from_black_alpha(14));
            for k in 1..5 {
                let f = k as f32 / 5.0;
                if (i + j) % 2 == 0 {
                    painter.vline(r.min.x + r.width() * f, r.y_range(), weave);
                } else {
                    painter.hline(r.x_range(), r.min.y + r.height() * f, weave);
                }
            }
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_black_alpha(18)), egui::StrokeKind::Inside);
        }
    }
    // The aisle across the room: light oak boards.
    let right = map.w.min(1600.0) - map.wall;
    let aisle = Rect::from_min_max(to(map.wall, 560.0), to(right, 640.0));
    painter.rect_filled(aisle, 0.0, rgb(196, 160, 112));
    let mut x = map.wall;
    let mut k = 0;
    while x < right {
        let len = 90.0 + hash(k, 7) * 80.0;
        for (row, y) in [(0, 560.0), (1, 600.0)] {
            let off = if row == 1 { 50.0 } else { 0.0 };
            let a = (x + off).min(right);
            let b = (x + off + len).min(right);
            let plank = Rect::from_min_max(to(a, y), to(b, y + 40.0));
            painter.rect_filled(plank, 0.0, rgb(200, 164, 116).lerp_to_gamma(rgb(214, 180, 130), hash(k, row)));
            painter.rect_stroke(plank, 0.0, Stroke::new(1.0, Color32::from_black_alpha(30)), egui::StrokeKind::Inside);
        }
        x += len;
        k += 1;
    }
    painter.hline(aisle.x_range(), aisle.min.y, Stroke::new(2.0 * s, rgb(120, 96, 66)));
    painter.hline(aisle.x_range(), aisle.max.y, Stroke::new(2.0 * s, rgb(120, 96, 66)));
    // The islands' rugs.
    for d in map.desks.iter().filter(|d| d.k == "desk" && d.x < 1600.0) {
        let rug = Rect::from_min_max(to(d.x - 40.0, d.y - 50.0), to(d.x + d.w + 40.0, d.y + d.h + 50.0));
        let (color, _) = island_colors(d);
        painter.rect_filled(rug, 10.0 * s, color);
        painter.rect_stroke(rug.shrink(6.0 * s), 6.0 * s, Stroke::new(2.0 * s, color.lerp_to_gamma(Color32::WHITE, 0.3)), egui::StrokeKind::Inside);
    }
}

/// The canteen: a linoleum checkerboard, cream and green, its trodden way to the self-service.
fn ground_lino(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    let tile = 80.0;
    for i in 0..(map.w / tile).ceil() as i32 {
        for j in 0..(map.h / tile).ceil() as i32 {
            let v = hash(i, j);
            let base = if (i + j) % 2 == 0 { rgb(222, 214, 190) } else { rgb(120, 158, 132) };
            let r = Rect::from_min_size(to(i as f32 * tile, j as f32 * tile), Vec2::splat(tile * s + 0.5));
            painter.rect_filled(r, 0.0, base.lerp_to_gamma(Color32::BLACK, v * 0.08));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_black_alpha(25)), egui::StrokeKind::Inside);
            // A scuff now and then.
            if v > 0.82 {
                let c = r.center() + Vec2::new((hash(j, i) - 0.5) * 30.0, 0.0) * s;
                painter.line_segment([c, c + Vec2::new(14.0, 3.0) * s], Stroke::new(2.0 * s, Color32::from_black_alpha(30)));
            }
        }
    }
    // A spilt tray of peas, a puddle of soup.
    painter.add(egui::Shape::ellipse_filled(to(1000.0, 300.0), Vec2::new(26.0, 16.0) * s, rgb(196, 150, 70).gamma_multiply(0.7)));
    for k in 0..7 {
        painter.circle_filled(to(560.0 + hash(k, 3) * 60.0, 820.0 + hash(3, k) * 30.0), 3.0 * s, rgb(110, 170, 70));
    }
}

/// The server room: a raised floor of grey tiles, some perforated (the cold air), cables under,
/// the blue glow of the racks.
fn ground_raised(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let tile = 60.0;
    for i in 0..(map.w / tile).ceil() as i32 {
        for j in 0..(map.h / tile).ceil() as i32 {
            let v = hash(i, j);
            let r = Rect::from_min_size(to(i as f32 * tile, j as f32 * tile), Vec2::splat(tile * s + 0.5));
            painter.rect_filled(r, 0.0, rgb(58, 62, 70).lerp_to_gamma(rgb(70, 74, 84), v));
            painter.rect_stroke(r.shrink(1.0), 1.0, Stroke::new(1.0, Color32::from_black_alpha(70)), egui::StrokeKind::Inside);
            if (i * 7 + j * 3) % 5 == 0 {
                // Perforated: a grid of holes.
                for a in 0..4 {
                    for b in 0..4 {
                        painter.circle_filled(r.min + Vec2::new(10.0 + a as f32 * 13.0, 10.0 + b as f32 * 13.0) * s, 1.6 * s, Color32::from_black_alpha(140));
                    }
                }
            }
        }
    }
    // The racks' blue light on the floor, breathing.
    let breath = 0.7 + 0.3 * (now * 1.3).sin() as f32;
    for d in map.desks.iter().filter(|d| d.k == "rack") {
        radial(painter, to(d.x + d.w / 2.0, d.y + d.h / 2.0), (d.w.max(d.h) * 0.9) * s, Color32::from_rgba_unmultiplied(60, 140, 255, (30.0 * breath) as u8), Color32::TRANSPARENT);
    }
}

/// The boardroom: a parquet of walnut planks, laid staggered.
fn ground_parquet(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    let (pw, ph) = (180.0, 28.0);
    for j in 0..(map.h / ph).ceil() as i32 {
        let shift = if j % 2 == 0 { 0.0 } else { pw / 2.0 };
        for i in -1..(map.w / pw).ceil() as i32 + 1 {
            let v = hash(i, j);
            let x = i as f32 * pw + shift;
            let r = Rect::from_min_size(to(x, j as f32 * ph), Vec2::new(pw, ph) * s);
            painter.rect_filled(r, 0.0, rgb(96, 62, 40).lerp_to_gamma(rgb(122, 82, 52), v));
            painter.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, Color32::from_black_alpha(70)));
            painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, Color32::from_black_alpha(55)));
            // The grain.
            let y = r.min.y + r.height() * (0.3 + 0.4 * hash(j, i));
            painter.line_segment([Pos2::new(r.min.x + 10.0 * s, y), Pos2::new(r.max.x - 20.0 * s, y + 2.0 * s)], Stroke::new(1.0, Color32::from_black_alpha(18)));
        }
    }
    // The rug under the table, its border.
    if let Some(table) = map.desks.iter().find(|d| d.k == "boardtable") {
        let rug = Rect::from_min_max(to(table.x - 90.0, table.y - 90.0), to(table.x + table.w + 90.0, table.y + table.h + 90.0));
        painter.rect_filled(rug.translate(Vec2::new(0.0, 3.0 * s)), 4.0 * s, Color32::from_black_alpha(60));
        painter.rect_filled(rug, 4.0 * s, rgb(92, 26, 36));
        painter.rect_stroke(rug.shrink(10.0 * s), 2.0 * s, Stroke::new(3.0 * s, rgb(176, 136, 70)), egui::StrokeKind::Inside);
        painter.rect_stroke(rug.shrink(18.0 * s), 2.0 * s, Stroke::new(1.0 * s, rgb(176, 136, 70).gamma_multiply(0.6)), egui::StrokeKind::Inside);
    }
}

/// The roof: concrete slabs, gravel, puddles; the pad the helicopter stands on.
fn ground_roof(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let slab = 200.0;
    for i in 0..(map.w / slab).ceil() as i32 {
        for j in 0..(map.h / slab).ceil() as i32 {
            let r = Rect::from_min_size(to(i as f32 * slab, j as f32 * slab), Vec2::splat(slab * s + 0.5));
            painter.rect_filled(r, 0.0, rgb(68, 74, 86).lerp_to_gamma(rgb(84, 90, 102), hash(i, j)));
            painter.rect_stroke(r, 0.0, Stroke::new(1.5 * s, Color32::from_black_alpha(60)), egui::StrokeKind::Inside);
            for k in 0..14 {
                let p = to(i as f32 * slab + hash(i * 31 + k, j) * slab, j as f32 * slab + hash(j * 17 + k, i) * slab);
                painter.circle_filled(p, (1.0 + hash(k, i + j) * 1.6) * s, if k % 3 == 0 { Color32::from_white_alpha(22) } else { Color32::from_black_alpha(40) });
            }
        }
    }
    // Puddles, the sky in them.
    for (x, y, w, h) in [(560.0, 820.0, 150.0, 50.0), (1460.0, 380.0, 90.0, 140.0), (180.0, 520.0, 70.0, 40.0)] {
        poly(painter, ellipse(to(x, y), w * s, h * s, 0.2), rgb(70, 94, 120).gamma_multiply(0.75));
        let shine = ((now * 0.6).sin() as f32 * 0.5 + 0.5) * 0.2 + lightning(now).0 * 3.0;
        poly(painter, ellipse(to(x - w * 0.15, y - h * 0.15), w * 0.5 * s, h * 0.3 * s, 0.2), Color32::from_white_alpha((20.0 + 30.0 * shine).min(255.0) as u8));
    }
    // The pad: a dark disc, its yellow ring, the H.
    if let Some(heli) = map.desks.iter().find(|d| d.k == "heli") {
        let c = to(heli.x + heli.w / 2.0, heli.y + heli.h / 2.0);
        let r = 240.0 * s;
        painter.circle_filled(c, r, rgb(46, 50, 56));
        painter.circle_stroke(c, r - 14.0 * s, Stroke::new(8.0 * s, rgb(240, 196, 48)));
        // Its lights, a chase running round.
        let head = (now * 14.0) as i32 % 24;
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            let p = c + Vec2::angled(a) * (r - 2.0 * s);
            let behind = (head - k).rem_euclid(24);
            if behind < 4 {
                radial(painter, p, (26.0 - behind as f32 * 5.0) * s, glow(150, 140, 90), Color32::TRANSPARENT);
            }
            painter.circle_filled(p, 3.5 * s, if behind < 4 { rgb(255, 250, 220) } else if k % 2 == 0 { rgb(160, 150, 120) } else { rgb(200, 70, 50) });
        }
        let (hw, hh, bar) = (90.0 * s, 150.0 * s, 26.0 * s);
        let white = Color32::from_white_alpha(200);
        painter.rect_filled(Rect::from_center_size(c - Vec2::new(hw / 2.0, 0.0), Vec2::new(bar, hh)), 0.0, white);
        painter.rect_filled(Rect::from_center_size(c + Vec2::new(hw / 2.0, 0.0), Vec2::new(bar, hh)), 0.0, white);
        painter.rect_filled(Rect::from_center_size(c, Vec2::new(hw, bar)), 0.0, white);
    }
}

/// The walls in the floor's material, their shade on the ground, the windows.
/// The rooms' grounds over the floor's: parquet, a red carpet, kitchen tiles, concrete, a corridor's
/// runner, the CEO's parquet and rug, the terrace's gravel.
fn rooms(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    for (n, g) in map.rooms.iter().enumerate() {
        let r = Rect::from_min_max(to(g.x, g.y), to(g.x + g.w, g.y + g.h));
        let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
        let seed = n as i32 * 97 + g.x as i32;
        match g.s.as_str() {
            "parquet" | "rug" => {
                // Planks: rows of varying lengths, each its shade.
                let row = 36.0;
                for j in 0..(g.h / row).ceil() as i32 {
                    let mut x = -hash(j, seed) * 160.0;
                    let mut k = 0;
                    while x < g.w {
                        let len = 140.0 + hash(j, k + seed) * 120.0;
                        let v = hash(k, j + seed);
                        let plank = Rect::from_min_max(to(g.x + x.max(0.0), g.y + j as f32 * row), to(g.x + (x + len).min(g.w), g.y + (j + 1) as f32 * row));
                        clip.rect_filled(plank, 0.0, rgb(138, 92, 56).lerp_to_gamma(rgb(170, 118, 72), v));
                        clip.rect_stroke(plank, 0.0, Stroke::new(1.0, Color32::from_black_alpha(45)), egui::StrokeKind::Inside);
                        x += len;
                        k += 1;
                    }
                }
                if g.s == "rug" {
                    // The CEO's rug: deep red, a gold border, a pattern.
                    let rug = Rect::from_center_size(r.center(), r.size() * Vec2::new(0.6, 0.62));
                    clip.rect_filled(rug.translate(Vec2::splat(4.0 * s)), 6.0 * s, Color32::from_black_alpha(60));
                    clip.rect_filled(rug, 6.0 * s, rgb(118, 22, 34));
                    clip.rect_stroke(rug.shrink(8.0 * s), 4.0 * s, Stroke::new(3.0 * s, rgb(214, 172, 84)), egui::StrokeKind::Inside);
                    clip.rect_stroke(rug.shrink(18.0 * s), 3.0 * s, Stroke::new(1.0 * s, rgb(214, 172, 84).gamma_multiply(0.6)), egui::StrokeKind::Inside);
                    for k in 0..6 {
                        let c = rug.center() + Vec2::angled(k as f32 * std::f32::consts::FRAC_PI_3) * rug.height() * 0.22;
                        clip.circle_stroke(c, 18.0 * s, Stroke::new(1.5 * s, rgb(214, 172, 84).gamma_multiply(0.5)));
                    }
                    clip.circle_stroke(rug.center(), 40.0 * s, Stroke::new(2.0 * s, rgb(214, 172, 84).gamma_multiply(0.7)));
                }
            }
            "redcarpet" => {
                clip.rect_filled(r, 0.0, rgb(92, 30, 40));
                // A woven pattern of small diamonds.
                let step = 48.0;
                for i in 0..(g.w / step) as i32 + 1 {
                    for j in 0..(g.h / step) as i32 + 1 {
                        let c = to(g.x + (i as f32 + 0.5) * step, g.y + (j as f32 + 0.5) * step);
                        poly(&clip, vec![c - Vec2::new(0.0, 7.0 * s), c + Vec2::new(7.0 * s, 0.0), c + Vec2::new(0.0, 7.0 * s), c - Vec2::new(7.0 * s, 0.0)], Color32::from_white_alpha(10));
                    }
                }
            }
            "tiles" => {
                let tile = 40.0;
                for i in 0..(g.w / tile).ceil() as i32 {
                    for j in 0..(g.h / tile).ceil() as i32 {
                        let t = Rect::from_min_size(to(g.x + i as f32 * tile, g.y + j as f32 * tile), Vec2::splat(tile * s + 0.5));
                        let white = (i + j) % 2 == 0;
                        let base = if white { rgb(232, 232, 226) } else { rgb(52, 56, 62) };
                        clip.rect_filled(t, 0.0, base.lerp_to_gamma(Color32::from_rgb(160, 150, 130), hash(i, j + seed) * 0.08));
                    }
                }
                // A greasy shine near the stoves.
                radial(&clip, to(g.x + g.w * 0.4, g.y + 120.0), 260.0 * s, Color32::from_rgba_unmultiplied(255, 220, 150, 18), Color32::TRANSPARENT);
            }
            "concrete" => {
                clip.rect_filled(r, 0.0, rgb(112, 114, 116));
                for k in 0..((g.w * g.h) / 9000.0) as i32 {
                    let p = to(g.x + hash(k, seed) * g.w, g.y + hash(seed, k) * g.h);
                    clip.circle_filled(p, (1.0 + hash(k, k + seed) * 2.0) * s, Color32::from_black_alpha(30));
                }
                // Cracks, and the seams of the slabs.
                for k in 0..4 {
                    let a = to(g.x + hash(k, 7 + seed) * g.w, g.y + hash(7 + seed, k) * g.h);
                    let mut p = a;
                    let mut pts = vec![p];
                    for m in 0..5 {
                        p += Vec2::angled(hash(k, m + seed) * std::f32::consts::TAU) * 30.0 * s;
                        pts.push(p);
                    }
                    clip.add(egui::Shape::line(pts, Stroke::new(1.2 * s, Color32::from_black_alpha(60))));
                }
                let slab = 200.0;
                for i in 1..(g.w / slab) as i32 + 1 {
                    clip.line_segment([to(g.x + i as f32 * slab, g.y), to(g.x + i as f32 * slab, g.y + g.h)], Stroke::new(1.0, Color32::from_black_alpha(35)));
                }
            }
            "corridor" => {
                clip.rect_filled(r, 0.0, rgb(58, 56, 66));
                // The runner along it, the lights over it.
                let along_x = g.w > g.h;
                let runner = if along_x { Rect::from_min_max(to(g.x, g.y + g.h * 0.3), to(g.x + g.w, g.y + g.h * 0.7)) } else { Rect::from_min_max(to(g.x + g.w * 0.3, g.y), to(g.x + g.w * 0.7, g.y + g.h)) };
                clip.rect_filled(runner, 0.0, rgb(44, 64, 92));
                clip.rect_stroke(runner.shrink(4.0 * s), 0.0, Stroke::new(1.0 * s, rgb(150, 170, 200).gamma_multiply(0.4)), egui::StrokeKind::Inside);
                let n = ((if along_x { g.w } else { g.h }) / 220.0) as i32;
                for k in 0..n.max(1) {
                    let c = if along_x { to(g.x + (k as f32 + 0.5) * g.w / n.max(1) as f32, g.y + g.h / 2.0) } else { to(g.x + g.w / 2.0, g.y + (k as f32 + 0.5) * g.h / n.max(1) as f32) };
                    radial(&clip, c, 120.0 * s, Color32::from_rgba_unmultiplied(255, 240, 210, 26), Color32::TRANSPARENT);
                }
            }
            "catwalk" => {
                // A metal grating over the void (the city through it), its railings yellow and black.
                clip.rect_filled(r, 0.0, rgb(20, 22, 34));
                let step = 10.0 * s;
                let mut x = r.min.x;
                while x < r.max.x {
                    clip.vline(x, r.y_range(), Stroke::new(2.0 * s, rgb(96, 100, 110)));
                    x += step;
                }
                let mut y = r.min.y;
                while y < r.max.y {
                    clip.hline(r.x_range(), y, Stroke::new(1.0 * s, rgb(76, 80, 90)));
                    y += step * 2.0;
                }
                for edge in [r.min.y + 3.0 * s, r.max.y - 3.0 * s] {
                    clip.hline(r.x_range(), edge, Stroke::new(6.0 * s, rgb(40, 40, 44)));
                    let mut x = r.min.x;
                    let mut k = 0;
                    while x < r.max.x {
                        if k % 2 == 0 {
                            clip.line_segment([Pos2::new(x, edge), Pos2::new((x + 14.0 * s).min(r.max.x), edge)], Stroke::new(4.0 * s, rgb(230, 190, 40)));
                        }
                        x += 14.0 * s;
                        k += 1;
                    }
                }
            }
            "gravel" => {
                clip.rect_filled(r, 0.0, rgb(150, 146, 136));
                for k in 0..((g.w * g.h) / 1400.0) as i32 {
                    let p = to(g.x + hash(k, seed) * g.w, g.y + hash(seed + 3, k) * g.h);
                    let v = hash(k, k + 11);
                    clip.circle_filled(p, (1.2 + v * 2.2) * s, rgb(110, 106, 98).lerp_to_gamma(rgb(196, 192, 182), v));
                }
            }
            _ => {}
        }
    }
}

/// Life on the floor: papers, coffee stains, post-its, cables (always at the same places on a floor);
/// leaves and puddles on the roof.
fn decals(painter: &egui::Painter, map: &Map, to: To, s: f32) {
    let seed = map.id.bytes().fold(7i32, |h, b| h.wrapping_mul(31).wrapping_add(b as i32));
    let n = (map.w * map.h / 55000.0) as i32;
    for k in 0..n {
        let x = map.wall + 30.0 + hash(k, seed) * (map.w - 2.0 * map.wall - 60.0);
        let y = map.wall + 30.0 + hash(seed, k) * (map.h - 2.0 * map.wall - 60.0);
        // Not on a wall.
        if map.walls.iter().any(|d| x > d.x - 20.0 && x < d.x + d.w + 20.0 && y > d.y - 20.0 && y < d.y + d.h + 20.0) {
            continue;
        }
        let c = to(x, y);
        let a = hash(k, k + seed) * std::f32::consts::TAU;
        let kind = (hash(k + 5, seed) * 5.0) as i32;
        if map.id == "roof" {
            if kind < 3 {
                // A leaf.
                poly(painter, vec![c + Vec2::angled(a) * 8.0 * s, c + Vec2::angled(a + 1.6) * 3.0 * s, c - Vec2::angled(a) * 8.0 * s, c + Vec2::angled(a - 1.6) * 3.0 * s], rgb(120, 104, 52).gamma_multiply(0.85));
            } else {
                painter.circle_filled(c, (14.0 + hash(k, 3) * 16.0) * s, Color32::from_rgba_unmultiplied(70, 90, 110, 50));
            }
            continue;
        }
        match kind {
            0 | 1 => {
                // A sheet of paper, its lines.
                let (u, v) = (Vec2::angled(a), Vec2::angled(a + std::f32::consts::FRAC_PI_2));
                let corner = |cx: f32, cy: f32| c + u * cx * s + v * cy * s;
                poly(painter, vec![corner(-9.0, -12.0), corner(9.0, -12.0), corner(9.0, 12.0), corner(-9.0, 12.0)], Color32::from_white_alpha(170));
                for l in 0..4 {
                    painter.line_segment([corner(-6.0, -7.0 + l as f32 * 5.0), corner(6.0, -7.0 + l as f32 * 5.0)], Stroke::new(0.8 * s, Color32::from_black_alpha(40)));
                }
            }
            2 => {
                // A coffee stain.
                painter.circle_stroke(c, 9.0 * s, Stroke::new(2.0 * s, Color32::from_rgba_unmultiplied(92, 56, 30, 70)));
                painter.circle_filled(c, 8.0 * s, Color32::from_rgba_unmultiplied(92, 56, 30, 25));
            }
            3 => {
                // A post-it.
                let color = [rgb(255, 230, 90), rgb(255, 150, 190), rgb(140, 220, 255)][(hash(k, 9) * 3.0) as usize % 3];
                let (u, v) = (Vec2::angled(a), Vec2::angled(a + std::f32::consts::FRAC_PI_2));
                poly(painter, vec![c - u * 6.0 * s - v * 6.0 * s, c + u * 6.0 * s - v * 6.0 * s, c + u * 6.0 * s + v * 6.0 * s, c - u * 6.0 * s + v * 6.0 * s], color.gamma_multiply(0.9));
            }
            _ => {
                // A cable, loose.
                let mut p = c;
                let mut pts = vec![p];
                for m in 0..6 {
                    p += Vec2::angled(a + (hash(k, m) - 0.5) * 1.6) * 14.0 * s;
                    pts.push(p);
                }
                painter.add(egui::Shape::line(pts, Stroke::new(2.0 * s, Color32::from_rgb(28, 30, 34))));
            }
        }
    }
}

/// The colors of a floor's walls: the wall, its top edge.
fn wall_colors(id: &str) -> (Color32, Color32) {
    match id {
        "hall" => (rgb(132, 124, 112), rgb(170, 162, 148)),
        "board" => (rgb(66, 42, 30), rgb(96, 64, 44)),
        "roof" => (rgb(150, 150, 146), rgb(186, 186, 182)),
        "canteen" => (rgb(186, 170, 140), rgb(214, 200, 172)),
        "servers" => (rgb(34, 38, 46), rgb(52, 58, 70)),
        _ => (rgb(56, 63, 80), rgb(78, 88, 108)),
    }
}

/// The void between the roof's two towers: the city far below (its streets, the cars' lights going
/// along them, the lit windows of lower roofs), a haze rising, the towers' edges lit.
fn abyss(painter: &egui::Painter, r: Rect, s: f32, now: f64) {
    let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    gradient(&clip, r, rgb(10, 12, 26), rgb(18, 16, 34), false);
    // Lower buildings' roofs, their windows.
    let block = 70.0 * s;
    let seed = (r.min.y / s) as i32;
    let (nx, ny) = ((r.width() / block).ceil() as i32 + 1, (r.height() / block).ceil() as i32 + 1);
    for i in 0..nx {
        for j in 0..ny {
            let v = hash(i + 3, j + seed);
            let b = Rect::from_min_size(r.min + Vec2::new(i as f32 * block + 8.0 * s, j as f32 * block + 8.0 * s), Vec2::splat(block - 22.0 * s));
            clip.rect_filled(b, 2.0, rgb(22, 24, 40).lerp_to_gamma(rgb(34, 34, 54), v));
            for k in 0..6 {
                if hash(k + i * 7, j + seed) > 0.55 {
                    let w = b.min + Vec2::new(hash(k, i + j) * b.width(), hash(i + j, k) * b.height());
                    clip.rect_filled(Rect::from_min_size(w, Vec2::splat(3.0 * s)), 0.0, if hash(k, i) > 0.7 { rgb(140, 200, 255) } else { rgb(255, 210, 130) });
                }
            }
        }
    }
    // The streets between, the cars' lights moving along (white one way, red the other).
    let t = now as f32;
    for i in 0..nx {
        let x = r.min.x + i as f32 * block + 1.0 * s;
        clip.line_segment([Pos2::new(x, r.min.y), Pos2::new(x, r.max.y)], Stroke::new(5.0 * s, rgb(30, 28, 36)));
        for k in 0..3 {
            let f = (hash(i, k + 40) + t * (0.05 + hash(k, i) * 0.05)).fract();
            let (y, color) = if k % 2 == 0 { (r.min.y + f * r.height(), rgb(255, 240, 200)) } else { (r.max.y - f * r.height(), rgb(255, 60, 50)) };
            clip.circle_filled(Pos2::new(x + if k % 2 == 0 { -1.2 } else { 1.2 } * s, y), 1.5 * s, color);
        }
    }
    // The haze rising, the city's glow in it; the towers' edges catching it.
    radial(&clip, r.center(), r.width().max(r.height()) * 0.7, Color32::from_rgba_premultiplied(40, 18, 30, 0), Color32::TRANSPARENT);
    gradient(&clip, Rect::from_min_size(r.min, Vec2::new(26.0 * s, r.height())), Color32::from_black_alpha(160), Color32::TRANSPARENT, false);
    gradient(&clip, Rect::from_min_max(Pos2::new(r.max.x - 26.0 * s, r.min.y), r.max), Color32::TRANSPARENT, Color32::from_black_alpha(160), false);
    painter.vline(r.min.x, r.y_range(), Stroke::new(3.0 * s, rgb(150, 150, 146)));
    painter.vline(r.max.x, r.y_range(), Stroke::new(3.0 * s, rgb(150, 150, 146)));
}

/// The walls inside the floor (rooms, corridors): as the outer ones, a shade at their foot.
fn inner_walls(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    let (wall, top) = wall_colors(&map.id);
    for d in &map.walls {
        let r = Rect::from_min_max(to(d.x, d.y), to(d.x + d.w, d.y + d.h));
        // No floor there (an L-shaped floor): the dark outside, a wall on its edge.
        if d.k == "void" && (map.id == "roof" || map.id == "board") {
            abyss(painter, r, s, now);
            continue;
        }
        if d.k == "void" {
            painter.rect_filled(r, 0.0, rgb(14, 15, 20));
            painter.rect_stroke(r, 0.0, Stroke::new(map.wall * s, wall), egui::StrokeKind::Inside);
            continue;
        }
        // Their shadow on the ground, below and to the right.
        painter.rect_filled(r.translate(Vec2::new(4.0 * s, 6.0 * s)), 0.0, Color32::from_black_alpha(70));
        painter.rect_filled(r, 0.0, wall);
        painter.rect_stroke(r, 0.0, Stroke::new(2.0 * s, top), egui::StrokeKind::Inside);
    }
}

fn walls(painter: &egui::Painter, map: &Map, to: To, s: f32, field: Rect) {
    let (wall, top) = wall_colors(&map.id);
    let w = map.wall * s;
    let inner = field.shrink(w);
    let shade = 26.0 * s;
    gradient(painter, Rect::from_min_size(inner.min, Vec2::new(inner.width(), shade)), Color32::from_black_alpha(90), Color32::TRANSPARENT, true);
    gradient(painter, Rect::from_min_size(inner.min, Vec2::new(shade, inner.height())), Color32::from_black_alpha(90), Color32::TRANSPARENT, false);
    gradient(painter, Rect::from_min_max(Pos2::new(inner.max.x - shade, inner.min.y), inner.max), Color32::TRANSPARENT, Color32::from_black_alpha(60), false);
    for r in [
        Rect::from_min_max(field.min, Pos2::new(field.max.x, inner.min.y)),
        Rect::from_min_max(Pos2::new(field.min.x, inner.max.y), field.max),
        Rect::from_min_max(field.min, Pos2::new(inner.min.x, field.max.y)),
        Rect::from_min_max(Pos2::new(inner.max.x, field.min.y), field.max),
    ] {
        painter.rect_filled(r, 0.0, wall);
    }
    painter.rect_stroke(inner, 0.0, Stroke::new(2.0 * s, top), egui::StrokeKind::Outside);
    if map.id == "roof" {
        // The parapet's yellow and black edge.
        let n = (map.w / 40.0) as i32;
        for k in 0..n {
            if k % 2 == 0 {
                let x = k as f32 * 40.0;
                painter.rect_filled(Rect::from_min_max(to(x, map.wall), to(x + 20.0, map.wall + 6.0)), 0.0, rgb(230, 190, 40));
                painter.rect_filled(Rect::from_min_max(to(x, map.h - map.wall - 6.0), to(x + 20.0, map.h - map.wall)), 0.0, rgb(230, 190, 40));
            }
        }
        return;
    }
    // The windows along the bottom wall (the top one in the hall: its glass front), the sky in them.
    let y = if map.id == "hall" { 0.0 } else { map.h - map.wall };
    let mut x = 260.0;
    while x < 1360.0 {
        let skip = map.doors.iter().chain(map.reception.as_ref()).any(|d| d.along == "x" && (d.x - x - 50.0).abs() < d.span / 2.0 + 60.0 && (d.y - y).abs() < 60.0);
        if !skip {
            let r = Rect::from_min_max(to(x, y + 4.0), to(x + 100.0, y + map.wall - 4.0));
            gradient(painter, r, rgb(150, 200, 236), rgb(90, 140, 190), false);
            painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.5 * s, top));
        }
        x += 120.0;
    }
}

/// Where the zombies come in: the right one (the revolving door in the hall, a broken window in the
/// open space, the double door of the boardroom, the fire ladder on the roof), the lift at the top,
/// the stairs at the bottom left.
fn doors(painter: &egui::Painter, map: &Map, to: To, s: f32, now: f64) {
    for (k, door) in map.doors.iter().enumerate() {
        let (w, h) = if door.along == "y" { (map.wall, door.span) } else { (door.span, map.wall) };
        let c = to(door.x.clamp(map.wall / 2.0, map.w - map.wall / 2.0), door.y.clamp(map.wall / 2.0, map.h - map.wall / 2.0));
        let r = Rect::from_center_size(c, Vec2::new(w, h) * s);
        match (k, map.id.as_str()) {
            (0, "hall") => {
                painter.rect_filled(r, 2.0, rgb(120, 150, 170));
                let hub = c - Vec2::new(30.0 * s, 0.0);
                painter.circle_filled(hub, 52.0 * s, Color32::from_rgba_unmultiplied(160, 200, 220, 60));
                painter.circle_stroke(hub, 52.0 * s, Stroke::new(2.0 * s, rgb(200, 200, 196)));
                for q in 0..4 {
                    let a = now as f32 * 0.8 + q as f32 * std::f32::consts::FRAC_PI_2;
                    painter.line_segment([hub, hub + Vec2::angled(a) * 50.0 * s], Stroke::new(3.0 * s, rgb(210, 220, 226)));
                }
            }
            (0, "board") => {
                painter.rect_filled(r, 2.0, rgb(110, 72, 44));
                painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(2.0 * s, rgb(60, 36, 22)));
                for d in [-1.0, 1.0] {
                    painter.circle_filled(r.center() + Vec2::new(0.0, d * 14.0 * s), 3.0 * s, rgb(212, 172, 84));
                }
            }
            (0, "roof") => {
                painter.rect_filled(r, 0.0, rgb(70, 74, 80));
                let n = (door.span / 24.0) as i32;
                for q in 0..=n {
                    let y = r.min.y + r.height() * q as f32 / n as f32;
                    painter.line_segment([Pos2::new(r.min.x, y), Pos2::new(r.max.x, y)], Stroke::new(2.0 * s, rgb(190, 60, 50)));
                }
            }
            (0, _) => {
                // The broken window: shards in the frame.
                gradient(painter, r, rgb(120, 170, 210), rgb(70, 110, 150), true);
                for q in 0..7 {
                    let y = r.min.y + r.height() * hash(q, 3);
                    poly(painter, vec![Pos2::new(r.min.x, y), Pos2::new(r.max.x, y + 6.0 * s), Pos2::new(r.min.x - 14.0 * s * hash(q, 5), y + 14.0 * s)], Color32::from_white_alpha(90));
                }
            }
            (1, "roof") => {
                // The stairwell's hut.
                let hut = Rect::from_center_size(c + Vec2::new(0.0, 30.0 * s), Vec2::new(door.span + 40.0, 70.0) * s);
                painter.rect_filled(hut.translate(Vec2::new(4.0, 6.0) * s), 4.0 * s, Color32::from_black_alpha(70));
                painter.rect_filled(hut, 4.0 * s, rgb(124, 126, 128));
                painter.rect_filled(Rect::from_center_size(hut.center_bottom(), Vec2::new(door.span * 0.6, 10.0) * s), 1.0, rgb(60, 64, 70));
            }
            (1, _) => {
                // The lift: brushed steel doors, the arrow lit.
                gradient(painter, r, rgb(190, 196, 204), rgb(130, 136, 146), false);
                painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(2.0 * s, rgb(70, 74, 82)));
                let lit = ((now * 2.0).sin() > 0.0) as u8;
                painter.circle_filled(r.center_bottom() + Vec2::new(0.0, 12.0 * s), 4.0 * s, if lit == 1 { rgb(255, 120, 60) } else { rgb(120, 60, 40) });
            }
            _ => {
                // The stairs: steps going down.
                painter.rect_filled(r, 0.0, rgb(70, 74, 82));
                let n = 6;
                for q in 0..n {
                    let x = r.min.x + r.width() * q as f32 / n as f32;
                    painter.line_segment([Pos2::new(x, r.min.y), Pos2::new(x, r.max.y)], Stroke::new(1.5 * s, Color32::from_white_alpha(40)));
                }
                painter.rect_filled(Rect::from_min_size(r.left_top() - Vec2::new(0.0, 4.0 * s), Vec2::new(r.width(), 4.0 * s)), 0.0, rgb(60, 200, 110).gamma_multiply(0.7));
            }
        }
    }
}

/// An office chair from above: the seat, the back toward `back` (unit vector).
fn chair(painter: &egui::Painter, c: Pos2, s: f32, back: Vec2, color: Color32) {
    painter.circle_filled(c + Vec2::new(2.0, 3.0) * s, 13.0 * s, Color32::from_black_alpha(60));
    painter.circle_filled(c, 12.0 * s, color);
    let side = Vec2::new(-back.y, back.x);
    painter.line_segment([c + back * 13.0 * s - side * 10.0 * s, c + back * 13.0 * s + side * 10.0 * s], Stroke::new(6.0 * s, color.lerp_to_gamma(Color32::BLACK, 0.35)));
}

/// A plant in its pot: leaves around, swaying a little.
fn plant(painter: &egui::Painter, c: Pos2, r: f32, s: f32, now: f64, pot: Color32) {
    painter.circle_filled(c + Vec2::new(3.0, 5.0) * s, r * 0.75, Color32::from_black_alpha(70));
    painter.circle_filled(c, r * 0.62, pot);
    painter.circle_filled(c, r * 0.5, rgb(70, 52, 38));
    for k in 0..9 {
        let a = k as f32 * 0.7 + (now * 0.8 + k as f64).sin() as f32 * 0.05;
        let tip = c + Vec2::angled(a) * r * (0.85 + 0.25 * hash(k, 1));
        poly(painter, ellipse(c.lerp(tip, 0.55), r * 0.95, r * 0.32, a), if k % 2 == 0 { rgb(60, 140, 70) } else { rgb(86, 170, 84) });
    }
}

/// One piece of furniture, as what it is.
fn furniture(painter: &egui::Painter, d: &Desk, to: To, s: f32, now: f64, boss: bool) {
    let r = rect_of(to, d);
    let shadow = |radius: f32| painter.rect_filled(r.translate(Vec2::new(5.0, 8.0) * s), radius, Color32::from_black_alpha(70));
    match d.k.as_str() {
        "desk" => {
            // An island of four white desks: their chairs outside (the island's color), two screens
            // each, the partition between them with its plants.
            let (_, seat) = island_colors(d);
            let (dw, dh) = ((d.w - 10.0) / 2.0, (d.h - 10.0) / 2.0);
            for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let out = if j == 0.0 { -1.0 } else { 1.0 };
                let cx = d.x + i * (dw + 10.0) + dw / 2.0;
                let cy = if j == 0.0 { d.y - 18.0 } else { d.y + d.h + 18.0 };
                chair(painter, to(cx, cy), s, Vec2::new(0.0, out), seat);
            }
            shadow(4.0 * s);
            for (k, (i, j)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
                let desk = Rect::from_min_size(to(d.x + i * (dw + 10.0), d.y + j * (dh + 10.0)), Vec2::new(dw, dh) * s);
                gradient(painter, desk, rgb(246, 246, 242), rgb(222, 222, 216), true);
                painter.rect_stroke(desk, 3.0 * s, Stroke::new(1.0, Color32::from_black_alpha(40)), egui::StrokeKind::Inside);
                let toward = if j == 0.0 { -1.0 } else { 1.0 };
                let flicker = 0.8 + 0.2 * ((now * 2.0 + k as f64 * 1.7 + f64::from(d.x)).sin() as f32 * 0.5 + 0.5);
                let tint = [rgb(90, 170, 255), rgb(120, 230, 160), rgb(255, 200, 110), rgb(200, 140, 255)][(k + d.x as usize) % 4];
                // Two screens, angled a little, toward the chair.
                for side in [-1.0, 1.0] {
                    let at = desk.center() + Vec2::new(side * 22.0, -toward * dh * 0.22) * s;
                    let screen = Rect::from_center_size(at, Vec2::new(38.0, 6.0) * s);
                    painter.rect_filled(screen, 2.0 * s, rgb(30, 32, 40));
                    painter.rect_filled(screen.shrink(1.2 * s).translate(Vec2::new(0.0, toward * 1.2 * s)), 1.0, tint.gamma_multiply(flicker));
                    radial(painter, at + Vec2::new(0.0, toward * 12.0 * s), 24.0 * s, tint.gamma_multiply(0.14 * flicker), Color32::TRANSPARENT);
                }
                painter.rect_filled(Rect::from_center_size(desk.center() + Vec2::new(0.0, toward * dh * 0.14) * s, Vec2::new(34.0, 8.0) * s), 1.5 * s, rgb(60, 62, 70));
                painter.circle_filled(desk.center() + Vec2::new(dw * 0.38, toward * dh * 0.12) * s, 5.0 * s, rgb(250, 250, 248));
                painter.circle_filled(desk.center() + Vec2::new(dw * 0.38, toward * dh * 0.12) * s, 3.0 * s, rgb(90, 56, 30));
            }
            let mid = Rect::from_center_size(to(d.x + d.w / 2.0, d.y + d.h / 2.0), Vec2::new(d.w - 6.0, 8.0) * s);
            painter.rect_filled(mid, 3.0 * s, rgb(70, 90, 80));
            for k in 0..5 {
                let c = mid.left_center() + Vec2::new(mid.width() * (0.1 + 0.2 * k as f32), 0.0);
                for q in 0..5 {
                    let a = q as f32 * 1.26 + k as f32 + (now as f32 * 0.8 + k as f32).sin() * 0.08;
                    poly(painter, ellipse(c + Vec2::angled(a) * 5.0 * s, 9.0 * s, 4.0 * s, a), rgb(70, 150, 80).lerp_to_gamma(rgb(110, 190, 100), hash(k, q)));
                }
            }
        }
        "table" => {
            for k in 0..5 {
                let x = d.x + 20.0 + k as f32 * (d.w - 40.0) / 4.0;
                chair(painter, to(x, d.y - 14.0), s, Vec2::new(0.0, -1.0), rgb(70, 76, 90));
                chair(painter, to(x, d.y + d.h + 14.0), s, Vec2::new(0.0, 1.0), rgb(70, 76, 90));
            }
            shadow(d.h / 2.0 * s);
            painter.rect_filled(r, d.h / 2.0 * s, rgb(146, 112, 80));
            painter.rect_filled(r.shrink(6.0 * s), d.h / 2.0 * s, rgb(164, 128, 92));
            painter.rect_filled(Rect::from_center_size(r.center(), Vec2::new(30.0, 20.0) * s), 2.0 * s, rgb(50, 54, 62));
        }
        "counter" => {
            shadow(10.0 * s);
            painter.rect_filled(r, 10.0 * s, rgb(108, 70, 46));
            let top = Rect::from_min_size(r.min, Vec2::new(r.width(), r.height() * 0.42));
            gradient(painter, top, rgb(240, 236, 228), rgb(206, 200, 190), true);
            painter.rect_filled(Rect::from_center_size(r.center() + Vec2::new(-40.0, 8.0) * s, Vec2::new(40.0, 8.0) * s), 1.0, rgb(30, 34, 44));
            painter.circle_filled(r.center() + Vec2::new(50.0, -8.0) * s, 6.0 * s, rgb(222, 180, 80));
            painter.circle_filled(r.center() + Vec2::new(50.0, -8.0) * s, 2.0 * s, rgb(255, 230, 160));
            // A vase of flowers on the corner.
            let vase = r.left_top() + Vec2::new(20.0, 14.0) * s;
            for k in 0..5 {
                painter.circle_filled(vase + Vec2::angled(k as f32 * 1.26) * 6.0 * s, 4.0 * s, [rgb(240, 100, 140), rgb(255, 210, 90)][k % 2]);
            }
            painter.circle_filled(vase, 3.0 * s, rgb(255, 240, 200));
        }
        "sofa" => {
            shadow(12.0 * s);
            painter.rect_filled(r, 12.0 * s, rgb(104, 60, 38));
            let back = Rect::from_min_size(r.min, Vec2::new(r.width(), r.height() * 0.32));
            painter.rect_filled(back, 10.0 * s, rgb(84, 46, 28));
            let n = 3;
            for k in 0..n {
                let cw = (r.width() - 20.0 * s) / n as f32;
                let cushion = Rect::from_min_size(Pos2::new(r.min.x + 10.0 * s + k as f32 * cw, back.max.y + 2.0 * s), Vec2::new(cw - 4.0 * s, r.height() * 0.56));
                gradient(painter, cushion, rgb(140, 84, 54), rgb(116, 68, 44), true);
            }
        }
        "lowtable" => {
            shadow(4.0 * s);
            painter.rect_filled(r, 4.0 * s, Color32::from_rgba_unmultiplied(170, 214, 230, 120));
            painter.rect_stroke(r, 4.0 * s, Stroke::new(1.5 * s, Color32::from_white_alpha(150)), egui::StrokeKind::Inside);
            painter.rect_filled(Rect::from_center_size(r.center() + Vec2::new(-14.0, -4.0) * s, Vec2::new(26.0, 18.0) * s), 1.0, rgb(220, 60, 60));
            painter.rect_filled(Rect::from_center_size(r.center() + Vec2::new(14.0, 6.0) * s, Vec2::new(24.0, 17.0) * s), 1.0, rgb(240, 240, 232));
        }
        "plant" => plant(painter, r.center(), r.width().min(r.height()) * 0.7, s, now, rgb(222, 218, 210)),
        "fountain" => {
            let c = r.center();
            poly(painter, ellipse(c + Vec2::new(4.0, 7.0) * s, r.width(), r.height(), 0.0), Color32::from_black_alpha(70));
            poly(painter, ellipse(c, r.width(), r.height(), 0.0), rgb(206, 200, 190));
            poly(painter, ellipse(c, r.width() - 18.0 * s, r.height() - 18.0 * s, 0.0), rgb(52, 120, 168));
            for k in 0..3 {
                let f = ((now * 0.5 + k as f64 / 3.0) % 1.0) as f32;
                let ring = ellipse(c, (r.width() - 24.0 * s) * f, (r.height() - 24.0 * s) * f, 0.0);
                painter.add(egui::Shape::closed_line(ring, Stroke::new(1.5 * s, Color32::from_white_alpha((90.0 * (1.0 - f)) as u8))));
            }
            painter.circle_filled(c, 12.0 * s, rgb(220, 214, 204));
            painter.circle_filled(c, 6.0 * s, Color32::from_white_alpha(200));
        }
        "gates" => {
            let n = 5;
            for k in 0..n {
                let x = r.min.x + r.width() * k as f32 / (n - 1) as f32;
                let post = Rect::from_center_size(Pos2::new(x, r.center().y), Vec2::new(12.0, r.height() / s) * s);
                painter.rect_filled(post.translate(Vec2::new(3.0, 5.0) * s), 3.0 * s, Color32::from_black_alpha(60));
                gradient(painter, post, rgb(200, 204, 210), rgb(140, 146, 156), false);
                painter.circle_filled(post.center_top() + Vec2::new(0.0, 6.0 * s), 2.5 * s, if (k + (now as usize)).is_multiple_of(3) { rgb(255, 80, 70) } else { rgb(80, 230, 120) });
                if k + 1 < n {
                    let glass = Rect::from_min_max(Pos2::new(x + 6.0 * s, r.center().y - 3.0 * s), Pos2::new(x + r.width() / (n - 1) as f32 * 0.45, r.center().y + 3.0 * s));
                    painter.rect_filled(glass, 1.0, Color32::from_rgba_unmultiplied(170, 220, 240, 110));
                }
            }
        }
        "boardtable" => {
            // The chairs all around, then the table, glossy, the glasses and the papers on it.
            let n = 7;
            for k in 0..n {
                let x = d.x + 40.0 + k as f32 * (d.w - 80.0) / (n - 1) as f32;
                chair(painter, to(x, d.y - 18.0), s, Vec2::new(0.0, -1.0), rgb(26, 26, 30));
                chair(painter, to(x, d.y + d.h + 18.0), s, Vec2::new(0.0, 1.0), rgb(26, 26, 30));
            }
            chair(painter, to(d.x - 20.0, d.y + d.h / 2.0), s, Vec2::new(-1.0, 0.0), rgb(110, 20, 30));
            chair(painter, to(d.x + d.w + 20.0, d.y + d.h / 2.0), s, Vec2::new(1.0, 0.0), rgb(26, 26, 30));
            shadow(30.0 * s);
            painter.rect_filled(r, 30.0 * s, rgb(58, 34, 22));
            gradient(painter, Rect::from_min_size(r.min + Vec2::new(30.0, 16.0) * s, Vec2::new(r.width() - 60.0 * s, r.height() * 0.3)), Color32::from_white_alpha(28), Color32::TRANSPARENT, true);
            painter.rect_stroke(r, 30.0 * s, Stroke::new(2.0 * s, rgb(120, 80, 50)), egui::StrokeKind::Inside);
            for k in 0..n {
                let x = d.x + 40.0 + k as f32 * (d.w - 80.0) / (n - 1) as f32;
                for (y, dy) in [(d.y + 22.0, 1.0), (d.y + d.h - 22.0, -1.0)] {
                    painter.rect_filled(Rect::from_center_size(to(x - 8.0, y), Vec2::new(18.0, 24.0) * s), 0.0, Color32::from_white_alpha(210));
                    painter.circle_filled(to(x + 14.0, y + dy * 4.0), 4.0 * s, Color32::from_rgba_unmultiplied(200, 230, 255, 140));
                }
            }
            for x in [d.x + d.w * 0.33, d.x + d.w * 0.66] {
                painter.circle_filled(to(x, d.y + d.h / 2.0), 9.0 * s, Color32::from_rgba_unmultiplied(190, 220, 250, 150));
                painter.circle_stroke(to(x, d.y + d.h / 2.0), 9.0 * s, Stroke::new(1.5 * s, Color32::from_white_alpha(160)));
            }
        }
        "flipchart" => {
            shadow(2.0 * s);
            for (a, b) in [(r.left_bottom(), r.center_top()), (r.right_bottom(), r.center_top())] {
                painter.line_segment([a, b], Stroke::new(2.5 * s, rgb(60, 60, 66)));
            }
            let sheet = r.shrink2(Vec2::new(2.0, 6.0) * s);
            painter.rect_filled(sheet, 1.0, rgb(248, 248, 244));
            let points: Vec<Pos2> = (0..5).map(|k| Pos2::new(sheet.min.x + sheet.width() * k as f32 / 4.0, sheet.max.y - sheet.height() * (0.2 + 0.6 * k as f32 / 4.0 * (if k == 3 { 0.4 } else { 1.0 })))).collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.5 * s, rgb(220, 50, 50))));
        }
        "credenza" => {
            shadow(4.0 * s);
            painter.rect_filled(r, 4.0 * s, rgb(52, 32, 22));
            painter.rect_filled(r.shrink(4.0 * s), 3.0 * s, rgb(78, 50, 34));
            // Its books, a trophy, a bottle.
            for k in 0..6 {
                let book = Rect::from_min_size(r.min + Vec2::new(10.0 + k as f32 * 9.0, 10.0) * s, Vec2::new(7.0, 30.0) * s);
                painter.rect_filled(book, 1.0, [rgb(150, 40, 40), rgb(40, 80, 130), rgb(200, 170, 90)][k % 3]);
            }
            painter.circle_filled(r.center() + Vec2::new(30.0, 0.0) * s, 9.0 * s, rgb(222, 180, 70));
            painter.circle_filled(r.center() + Vec2::new(30.0, 0.0) * s, 4.0 * s, rgb(255, 230, 150));
            painter.circle_filled(r.right_center() - Vec2::new(26.0, 0.0) * s, 7.0 * s, rgb(70, 110, 60));
        }
        "heli" => helicopter(painter, d, to, s, now, boss),
        "cantable" => {
            // A long canteen table, its stools both sides, trays and plates.
            let n = ((d.w / 70.0) as i32).max(2);
            for k in 0..n {
                let x = d.x + 35.0 + k as f32 * (d.w - 70.0) / (n - 1).max(1) as f32;
                for (y, out) in [(d.y - 14.0, -1.0), (d.y + d.h + 14.0, 1.0)] {
                    painter.circle_filled(to(x, y) + Vec2::new(2.0, 3.0) * s, 10.0 * s, Color32::from_black_alpha(60));
                    painter.circle_filled(to(x, y), 10.0 * s, rgb(220, 90, 60));
                    painter.circle_filled(to(x, y) + Vec2::new(0.0, out * 2.0) * s, 6.0 * s, rgb(240, 120, 86));
                }
            }
            shadow(4.0 * s);
            painter.rect_filled(r, 4.0 * s, rgb(210, 206, 196));
            painter.rect_stroke(r, 4.0 * s, Stroke::new(1.5 * s, rgb(150, 146, 136)), egui::StrokeKind::Inside);
            for k in 0..n {
                let x = d.x + 35.0 + k as f32 * (d.w - 70.0) / (n - 1).max(1) as f32;
                for (y, top) in [(d.y + d.h * 0.27, true), (d.y + d.h * 0.73, false)] {
                    if hash(k, i32::from(top)) < 0.2 {
                        continue;
                    }
                    let tray = Rect::from_center_size(to(x, y), Vec2::new(46.0, 30.0) * s);
                    painter.rect_filled(tray, 3.0 * s, [rgb(70, 110, 160), rgb(160, 70, 70), rgb(90, 140, 90)][(k as usize + usize::from(top)) % 3]);
                    painter.circle_filled(tray.center() - Vec2::new(6.0, 0.0) * s, 9.0 * s, rgb(248, 248, 244));
                    painter.circle_filled(tray.center() - Vec2::new(6.0, 0.0) * s, 5.0 * s, [rgb(200, 150, 60), rgb(130, 170, 80), rgb(180, 80, 60)][k as usize % 3]);
                    painter.circle_filled(tray.center() + Vec2::new(14.0, -6.0) * s, 4.0 * s, rgb(240, 240, 240));
                }
            }
        }
        "buffet" => {
            // The self-service: its glass, the dishes under it, the trays' rail.
            shadow(6.0 * s);
            gradient(painter, r, rgb(190, 196, 204), rgb(150, 156, 166), true);
            let glass = r.shrink(6.0 * s);
            painter.rect_filled(glass, 3.0 * s, rgb(200, 230, 240).gamma_multiply(0.35));
            let n = ((glass.width() / (40.0 * s)) as i32).max(1);
            for k in 0..n {
                let c = Pos2::new(glass.min.x + glass.width() * (k as f32 + 0.5) / n as f32, glass.center().y);
                painter.rect_filled(Rect::from_center_size(c, Vec2::new(30.0, glass.height() / s * 0.6) * s), 2.0 * s, rgb(90, 96, 104));
                painter.rect_filled(Rect::from_center_size(c, Vec2::new(24.0, glass.height() / s * 0.45) * s), 2.0 * s, [rgb(220, 170, 70), rgb(120, 170, 80), rgb(200, 90, 60), rgb(240, 230, 200)][k as usize % 4]);
            }
            painter.line_segment([r.left_bottom() + Vec2::new(0.0, 5.0 * s), r.right_bottom() + Vec2::new(0.0, 5.0 * s)], Stroke::new(3.0 * s, rgb(170, 176, 186)));
        }
        "vending" => {
            // A vending machine: lit glass, rows of snacks, the slot.
            shadow(4.0 * s);
            painter.rect_filled(r, 4.0 * s, rgb(170, 36, 40));
            let glass = Rect::from_min_max(r.min + Vec2::splat(5.0 * s), Pos2::new(r.max.x - r.width() * 0.3, r.max.y - 5.0 * s));
            painter.rect_filled(glass, 2.0 * s, rgb(210, 230, 240));
            for row in 0..4 {
                for col in 0..3 {
                    let c = glass.min + Vec2::new(glass.width() * (col as f32 + 0.5) / 3.0, glass.height() * (row as f32 + 0.5) / 4.0);
                    painter.rect_filled(Rect::from_center_size(c, Vec2::new(glass.width() / 4.0, glass.height() / 6.0)), 1.0, [rgb(240, 200, 60), rgb(60, 120, 220), rgb(230, 90, 60), rgb(120, 200, 90)][(row + col) % 4]);
                }
            }
            radial(painter, glass.center(), glass.width() * 0.9, Color32::from_rgba_unmultiplied(220, 240, 255, 30), Color32::TRANSPARENT);
            painter.rect_filled(Rect::from_center_size(Pos2::new(r.max.x - r.width() * 0.15, r.center().y), Vec2::new(r.width() * 0.12, r.height() * 0.3)), 1.0, rgb(40, 40, 46));
        }
        "rack" => {
            // A row of server racks: black, their lights blinking, the cables on top.
            shadow(3.0 * s);
            painter.rect_filled(r, 3.0 * s, rgb(22, 24, 30));
            let vertical = d.h > d.w;
            let n = ((if vertical { d.h } else { d.w } / 50.0) as i32).max(1);
            for k in 0..n {
                let f0 = k as f32 / n as f32;
                let f1 = (k + 1) as f32 / n as f32;
                let cab = if vertical {
                    Rect::from_min_max(Pos2::new(r.min.x, r.min.y + r.height() * f0), Pos2::new(r.max.x, r.min.y + r.height() * f1))
                } else {
                    Rect::from_min_max(Pos2::new(r.min.x + r.width() * f0, r.min.y), Pos2::new(r.min.x + r.width() * f1, r.max.y))
                }
                .shrink(2.0 * s);
                painter.rect_filled(cab, 2.0 * s, rgb(36, 40, 50));
                painter.rect_stroke(cab, 2.0 * s, Stroke::new(1.0, rgb(60, 66, 80)), egui::StrokeKind::Inside);
                for l in 0..6 {
                    let at = cab.min + Vec2::new(cab.width() * (0.2 + 0.12 * (l % 6) as f32), cab.height() * (0.25 + 0.5 * hash(k, l)));
                    let on = (now * (2.0 + f64::from(hash(l, k)) * 5.0) + f64::from(k + l)).sin() > 0.0;
                    let color = [rgb(80, 230, 120), rgb(80, 160, 255), rgb(255, 180, 60)][(l + k).rem_euclid(3) as usize];
                    painter.circle_filled(at, 1.8 * s, if on { color } else { color.gamma_multiply(0.25) });
                }
            }
        }
        "ups" => {
            // An inverter: grey, its little screen, its warning stripes.
            shadow(4.0 * s);
            gradient(painter, r, rgb(150, 154, 160), rgb(110, 114, 120), true);
            let screen = Rect::from_center_size(r.center() - Vec2::new(0.0, r.height() * 0.15), Vec2::new(r.width() * 0.5, r.height() * 0.2));
            painter.rect_filled(screen, 1.0, rgb(30, 60, 40));
            painter.text(screen.center(), egui::Align2::CENTER_CENTER, "100%", egui::FontId::monospace(screen.height() * 0.7), rgb(110, 240, 140));
            let band = Rect::from_min_max(Pos2::new(r.min.x, r.max.y - r.height() * 0.2), r.max);
            painter.rect_filled(band, 0.0, rgb(240, 196, 40));
            for k in 0..6 {
                let x = band.min.x + band.width() * k as f32 / 6.0;
                painter.line_segment([Pos2::new(x, band.max.y), Pos2::new(x + band.height(), band.min.y)], Stroke::new(3.0 * s, rgb(30, 30, 30)));
            }
        }
        "crate" => {
            // Cardboard boxes, taped, one open.
            shadow(2.0 * s);
            painter.rect_filled(r, 2.0 * s, rgb(176, 132, 84));
            painter.rect_stroke(r, 2.0 * s, Stroke::new(1.0, rgb(130, 94, 56)), egui::StrokeKind::Inside);
            painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(5.0 * s, rgb(206, 176, 120)));
            painter.line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, rgb(130, 94, 56)));
        }
        "hvac" => {
            shadow(4.0 * s);
            gradient(painter, r, rgb(180, 186, 192), rgb(130, 136, 144), true);
            painter.rect_stroke(r, 4.0 * s, Stroke::new(1.5 * s, rgb(90, 94, 100)), egui::StrokeKind::Inside);
            for k in 0..2 {
                let c = r.left_center() + Vec2::new(r.width() * (0.28 + 0.44 * k as f32), 0.0);
                let fr = r.height() * 0.36;
                painter.circle_filled(c, fr, rgb(50, 54, 60));
                for b in 0..4 {
                    let a = now as f32 * 7.0 + b as f32 * std::f32::consts::FRAC_PI_2 + k as f32;
                    poly(painter, ellipse(c + Vec2::angled(a) * fr * 0.45, fr * 0.8, fr * 0.3, a), rgb(150, 156, 164));
                }
                painter.circle_stroke(c, fr, Stroke::new(2.0 * s, rgb(100, 104, 110)));
            }
        }
        "tank" => {
            let c = r.center();
            let tr = r.width().min(r.height()) / 2.0;
            painter.circle_filled(c + Vec2::new(5.0, 8.0) * s, tr, Color32::from_black_alpha(70));
            radial(painter, c, tr, rgb(170, 176, 182), rgb(110, 116, 124));
            for k in 1..4 {
                painter.circle_stroke(c, tr * k as f32 / 4.0, Stroke::new(1.0, Color32::from_black_alpha(40)));
            }
            painter.circle_filled(c, tr * 0.18, rgb(80, 84, 92));
        }
        "winch" => {
            // The lift's winch: its drum, the cables going down, the motor.
            shadow(5.0 * s);
            painter.rect_filled(r, 4.0 * s, rgb(70, 74, 82));
            painter.rect_stroke(r, 4.0 * s, Stroke::new(1.5 * s, rgb(110, 114, 122)), egui::StrokeKind::Inside);
            let drum = Rect::from_min_max(r.min + Vec2::new(10.0, 14.0) * s, Pos2::new(r.center().x + 10.0 * s, r.max.y - 14.0 * s));
            gradient(painter, drum, rgb(150, 120, 60), rgb(90, 70, 36), true);
            let turn = (now * 1.5).fract() as f32;
            for k in 0..5 {
                let y = drum.min.y + drum.height() * ((k as f32 + turn) / 5.0);
                painter.hline(drum.x_range(), y, Stroke::new(1.5 * s, Color32::from_black_alpha(90)));
            }
            painter.circle_filled(Pos2::new(r.max.x - 26.0 * s, r.center().y), 18.0 * s, rgb(40, 110, 70));
            painter.circle_stroke(Pos2::new(r.max.x - 26.0 * s, r.center().y), 18.0 * s, Stroke::new(2.0 * s, rgb(70, 160, 100)));
        }
        "skylight" => {
            // The glass roof over the CEO's office: the light coming up, his rug below, frames.
            shadow(4.0 * s);
            gradient(painter, r, rgb(255, 214, 140), rgb(200, 140, 80), true);
            let rug = Rect::from_center_size(r.center(), r.size() * 0.55);
            painter.rect_filled(rug, 3.0 * s, rgb(140, 40, 46));
            painter.rect_stroke(rug.shrink(4.0 * s), 2.0 * s, Stroke::new(1.5 * s, rgb(230, 190, 100)), egui::StrokeKind::Inside);
            // Something moving down there.
            let f = (now * 0.3).fract() as f32;
            painter.circle_filled(Pos2::new(rug.min.x + rug.width() * f, rug.center().y + (now * 2.0).sin() as f32 * 6.0 * s), 9.0 * s, Color32::from_black_alpha(120));
            for i in 1..4 {
                let x = r.min.x + r.width() * i as f32 / 4.0;
                painter.vline(x, r.y_range(), Stroke::new(3.0 * s, rgb(60, 64, 72)));
            }
            for j in 1..3 {
                let y = r.min.y + r.height() * j as f32 / 3.0;
                painter.hline(r.x_range(), y, Stroke::new(3.0 * s, rgb(60, 64, 72)));
            }
            painter.rect_stroke(r, 0.0, Stroke::new(6.0 * s, rgb(80, 84, 92)), egui::StrokeKind::Inside);
            poly(painter, vec![r.min + Vec2::new(10.0, 10.0) * s, r.min + Vec2::new(60.0, 10.0) * s, r.min + Vec2::new(20.0, 60.0) * s], Color32::from_white_alpha(60));
        }
        "antenna" => {
            let c = r.center();
            painter.rect_stroke(r, 0.0, Stroke::new(2.0 * s, rgb(170, 170, 176)), egui::StrokeKind::Inside);
            painter.line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.5 * s, rgb(170, 170, 176)));
            painter.line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.5 * s, rgb(170, 170, 176)));
            let blink = (now * 1.5).fract() < 0.3;
            radial(painter, c, 30.0 * s, if blink { Color32::from_rgba_unmultiplied(255, 60, 50, 110) } else { Color32::TRANSPARENT }, Color32::TRANSPARENT);
            painter.circle_filled(c, 4.0 * s, if blink { rgb(255, 70, 60) } else { rgb(120, 40, 36) });
        }
        // Drawn with the floor's lights (board_sunset), glowing.
        "aquarium" => {}
        "babyfoot" => {
            // The table football: its green pitch, its rods and little players, the ball rolling.
            shadow(6.0 * s);
            painter.rect_filled(r, 6.0 * s, rgb(110, 70, 40));
            let pitch = r.shrink(8.0 * s);
            painter.rect_filled(pitch, 2.0 * s, rgb(50, 140, 70));
            painter.rect_stroke(pitch.shrink(3.0 * s), 0.0, Stroke::new(1.0, Color32::from_white_alpha(140)), egui::StrokeKind::Inside);
            painter.vline(pitch.center().x, pitch.y_range(), Stroke::new(1.0, Color32::from_white_alpha(140)));
            for k in 0..8 {
                let x = pitch.min.x + pitch.width() * (k as f32 + 0.5) / 8.0;
                painter.vline(x, Rect::from_min_max(r.min - Vec2::new(0.0, 10.0 * s), r.max + Vec2::new(0.0, 10.0 * s)).y_range(), Stroke::new(2.0 * s, rgb(190, 194, 200)));
                let team = if [0, 1, 3, 5].contains(&k) { rgb(220, 50, 50) } else { rgb(40, 90, 210) };
                let slide = (now as f32 * 2.0 + k as f32).sin() * 6.0 * s;
                for q in 0..3 {
                    painter.circle_filled(Pos2::new(x, pitch.min.y + pitch.height() * (0.2 + 0.3 * q as f32) + slide), 4.0 * s, team);
                }
            }
            let ball = pitch.center() + Vec2::new((now as f32 * 1.7).sin() * pitch.width() * 0.4, (now as f32 * 2.3).cos() * pitch.height() * 0.3);
            painter.circle_filled(ball, 3.0 * s, Color32::WHITE);
        }
        "beanbag" => {
            let c = r.center();
            painter.circle_filled(c + Vec2::new(3.0, 5.0) * s, r.width() / 2.0, Color32::from_black_alpha(70));
            let color = if d.x < 800.0 { rgb(230, 120, 60) } else { rgb(70, 150, 200) };
            radial(painter, c, r.width() / 2.0, color.lerp_to_gamma(Color32::WHITE, 0.2), color.lerp_to_gamma(Color32::BLACK, 0.25));
            painter.circle_filled(c + Vec2::new(-4.0, -4.0) * s, r.width() * 0.18, Color32::from_white_alpha(40));
        }
        "printer" => {
            // The printer: grey, its tray of paper, a sheet coming out now and then, its green light.
            shadow(4.0 * s);
            gradient(painter, r, rgb(220, 222, 226), rgb(170, 174, 180), true);
            painter.rect_stroke(r, 4.0 * s, Stroke::new(1.0, rgb(130, 134, 140)), egui::StrokeKind::Inside);
            let tray = Rect::from_min_size(r.min + Vec2::new(10.0, 8.0) * s, Vec2::new(r.width() - 20.0 * s, 14.0 * s));
            painter.rect_filled(tray, 1.0, rgb(60, 62, 70));
            let out = (now * 0.4).fract() as f32;
            if out < 0.5 {
                let sheet = Rect::from_min_size(Pos2::new(tray.min.x + 4.0 * s, r.max.y - 6.0 * s), Vec2::new(tray.width() - 8.0 * s, 26.0 * s * (out * 2.0)));
                painter.rect_filled(sheet, 0.0, Color32::WHITE);
            }
            painter.circle_filled(r.right_top() + Vec2::new(-8.0, 8.0) * s, 2.5 * s, if (now * 2.0).fract() < 0.5 { rgb(80, 230, 120) } else { rgb(40, 110, 60) });
        }
        "shelf" => {
            // A bookcase against the wall: rows of colored books, a binder or two lying.
            shadow(3.0 * s);
            painter.rect_filled(r, 2.0 * s, rgb(120, 84, 54));
            let inner = r.shrink(3.0 * s);
            let mut x = inner.min.x;
            let mut k = 0;
            while x < inner.max.x - 4.0 * s {
                let w = (6.0 + hash(k, d.x as i32) * 8.0) * s;
                let color = [rgb(190, 60, 60), rgb(50, 100, 170), rgb(220, 180, 70), rgb(60, 140, 90), rgb(230, 230, 220), rgb(120, 70, 150)][(hash(d.x as i32, k) * 6.0) as usize % 6];
                painter.rect_filled(Rect::from_min_max(Pos2::new(x, inner.min.y), Pos2::new((x + w).min(inner.max.x), inner.max.y)), 0.0, color);
                painter.vline(x, inner.y_range(), Stroke::new(1.0, Color32::from_black_alpha(60)));
                x += w;
                k += 1;
            }
        }
        "cooler" => {
            // The water fountain: its blue bottle, bubbles going up.
            let c = r.center();
            painter.rect_filled(r.translate(Vec2::new(3.0, 5.0) * s), 4.0 * s, Color32::from_black_alpha(70));
            painter.rect_filled(r, 4.0 * s, rgb(230, 232, 236));
            painter.circle_filled(c, r.width() * 0.36, rgb(90, 170, 230));
            painter.circle_stroke(c, r.width() * 0.36, Stroke::new(1.5 * s, rgb(160, 210, 250)));
            let f = (now * 0.8).fract() as f32;
            painter.circle_filled(c + Vec2::new(3.0 * s, r.width() * 0.25 * (1.0 - 2.0 * f)), 2.0 * s, Color32::from_white_alpha(200));
        }
        _ => {
            shadow(6.0 * s);
            painter.rect_filled(r, 6.0 * s, rgb(150, 120, 90));
        }
    }
}

/// The CEO's helicopter, from above: its black body with a gold line, the cockpit, the tail, the
/// skids, the rotor (turning fast when the executive committee is there).
fn helicopter(painter: &egui::Painter, d: &Desk, to: To, s: f32, now: f64, boss: bool) {
    let body_c = to(d.x + d.w * 0.36, d.y + d.h / 2.0);
    let (bw, bh) = (d.w * 0.52 * s, d.h * 0.62 * s);
    for dy in [-1.0, 1.0] {
        let y = body_c.y + dy * bh * 0.62;
        painter.line_segment([Pos2::new(body_c.x - bw * 0.45, y), Pos2::new(body_c.x + bw * 0.45, y)], Stroke::new(4.0 * s, rgb(40, 42, 46)));
    }
    poly(painter, ellipse(body_c + Vec2::new(6.0, 10.0) * s, bw, bh, 0.0), Color32::from_black_alpha(80));
    // The tail boom and its rotor.
    let tail_end = to(d.x + d.w, d.y + d.h / 2.0);
    painter.line_segment([body_c, tail_end], Stroke::new(12.0 * s, rgb(28, 30, 36)));
    painter.line_segment([tail_end - Vec2::new(0.0, 22.0 * s), tail_end + Vec2::new(0.0, 22.0 * s)], Stroke::new(4.0 * s, rgb(80, 84, 92)));
    poly(painter, ellipse(body_c, bw, bh, 0.0), rgb(24, 26, 32));
    painter.add(egui::Shape::closed_line(ellipse(body_c, bw * 0.9, bh * 0.82, 0.0), Stroke::new(2.0 * s, rgb(212, 172, 84))));
    // The cockpit's glass.
    poly(painter, ellipse(body_c - Vec2::new(bw * 0.28, 0.0), bw * 0.42, bh * 0.66, 0.0), rgb(90, 140, 180));
    poly(painter, ellipse(body_c - Vec2::new(bw * 0.34, bh * 0.12), bw * 0.16, bh * 0.2, 0.3), Color32::from_white_alpha(90));
    painter.text(body_c + Vec2::new(bw * 0.2, 0.0), Align2::CENTER_CENTER, "PDG", FontId::proportional(13.0 * s.max(0.6)), rgb(212, 172, 84));
    // The rotor: four long blades, blurred into a disc when it turns fast.
    let spin = if boss { 22.0 } else { 0.35 };
    let hub = body_c;
    let len = d.w * 0.62 * s;
    if boss {
        painter.circle_filled(hub, len, Color32::from_white_alpha(18));
    }
    for k in 0..4 {
        let a = now as f32 * spin + k as f32 * std::f32::consts::FRAC_PI_2;
        let tip = hub + Vec2::angled(a) * len;
        painter.line_segment([hub + Vec2::new(4.0, 8.0) * s, tip + Vec2::new(4.0, 8.0) * s], Stroke::new(7.0 * s, Color32::from_black_alpha(50)));
        painter.line_segment([hub, tip], Stroke::new(6.0 * s, if boss { Color32::from_rgba_unmultiplied(60, 62, 70, 120) } else { rgb(60, 62, 70) }));
    }
    painter.circle_filled(hub, 8.0 * s, rgb(120, 124, 132));
}

/// The coffee machine, shining, glowing during the break.
pub(super) fn coffee_machine(painter: &egui::Painter, c: Pos2, s: f32, now: f64, wave_on: bool) {
    let glow = if wave_on { 0.1 } else { 0.22 + 0.12 * (now * 3.0).sin() as f32 };
    radial(painter, c, 90.0 * s, Color32::from_rgba_unmultiplied(255, 181, 71, (glow * 255.0) as u8), Color32::TRANSPARENT);
    let machine = Rect::from_center_size(c, Vec2::new(60.0, 72.0) * s);
    painter.rect_filled(machine.translate(Vec2::new(4.0, 7.0) * s), 6.0 * s, Color32::from_black_alpha(80));
    gradient(painter, machine, rgb(78, 56, 42), rgb(44, 30, 22), false);
    painter.rect_stroke(machine, 6.0 * s, Stroke::new(1.5 * s, rgb(120, 90, 66)), egui::StrokeKind::Inside);
    let screen = Rect::from_min_size(machine.min + Vec2::new(10.0, 10.0) * s, Vec2::new(40.0, 14.0) * s);
    painter.rect_filled(screen, 3.0 * s, rgb(255, 181, 71));
    painter.text(screen.center(), Align2::CENTER_CENTER, "CAFÉ", FontId::monospace(10.0 * s.max(0.6)), rgb(60, 42, 32));
    // A cup under the spout, its steam.
    let cup = machine.center_bottom() - Vec2::new(0.0, 16.0 * s);
    painter.rect_filled(Rect::from_center_size(cup, Vec2::new(14.0, 12.0) * s), 2.0 * s, rgb(240, 236, 226));
    for k in 0..3 {
        let f = ((now * 0.7 + k as f64 / 3.0) % 1.0) as f32;
        painter.circle_filled(cup - Vec2::new((f * 9.0).sin() * 3.0, 8.0 + f * 26.0) * s, (3.0 + f * 3.0) * s, Color32::from_white_alpha((70.0 * (1.0 - f)) as u8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_stays_between_0_and_1() {
        for i in -20..20 {
            for j in -20..20 {
                let v = hash(i, j);
                assert!((0.0..=1.0).contains(&v));
                assert_eq!(v, hash(i, j));
            }
        }
    }

    #[test]
    fn every_floor_draws() {
        // In a frame: the neon sign's letters need the fonts.
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx();
            let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
            let desk = |k: &str| Desk { x: 600.0, y: 300.0, w: 300.0, h: 140.0, k: k.into() };
            for id in ["hall", "openspace", "canteen", "servers", "board", "roof", "?"] {
                let map = Map {
                    id: id.into(),
                    w: 1600.0,
                    h: 1000.0,
                    wall: 20.0,
                    desks: ["desk", "table", "counter", "sofa", "lowtable", "plant", "fountain", "gates", "boardtable", "flipchart", "credenza", "heli", "hvac", "tank", "antenna", "cantable", "buffet", "vending", "rack", "ups", "crate", "winch", "skylight", "aquarium", "babyfoot", "beanbag", "printer", "shelf", "cooler", "?"].iter().map(|k| desk(k)).collect(),
                    walls: vec![Desk { x: 700.0, y: 20.0, w: 200.0, h: 300.0, k: "void".into() }, Desk { x: 300.0, y: 500.0, w: 20.0, h: 300.0, k: "wall".into() }],
                    rooms: ["parquet", "rug", "concrete", "corridor", "gravel", "catwalk", "?"].iter().map(|k| crate::zombie::Ground { x: 100.0, y: 100.0, w: 400.0, h: 200.0, s: (*k).into() }).collect(),
                    ..Default::default()
                };
                let to = |x: f32, y: f32| Pos2::new(x * 0.5, y * 0.5);
                let field = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
                for now in [1.0, 4.03, 9.1] {
                    paint_floor(&painter, &map, &to, 0.5, now, false);
                    paint_shade(&painter, field, id == "roof");
                    paint_air(&painter, &map, &to, 0.5, now, field);
                }
            }
        });
        out.textures_delta.clear();
    }
}
