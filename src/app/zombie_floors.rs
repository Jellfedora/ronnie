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
        _ => ground_carpet(painter, map, to, s),
    }
    // The lights of the ceiling on the ground (the sun, on the roof).
    if map.id != "roof" {
        for i in 0..4 {
            for j in 0..2 {
                let c = to(200.0 + i as f32 * 400.0, 250.0 + j as f32 * 500.0);
                radial(painter, c, 230.0 * s, Color32::from_rgba_unmultiplied(255, 244, 214, 26), Color32::TRANSPARENT);
            }
        }
    }
    walls(painter, map, to, s, field);
    doors(painter, map, to, s, now);
    for d in &map.desks {
        furniture(painter, d, to, s, now, boss);
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
    // The red carpet, gold edged, from the revolving door to the fountain.
    let carpet = Rect::from_min_max(to(900.0, 462.0), to(map.w - map.wall, 538.0));
    painter.rect_filled(carpet.translate(Vec2::new(0.0, 3.0 * s)), 0.0, Color32::from_black_alpha(50));
    painter.rect_filled(carpet, 0.0, rgb(128, 26, 38));
    painter.rect_stroke(carpet.shrink(5.0 * s), 0.0, Stroke::new(2.0 * s, rgb(212, 172, 84)), egui::StrokeKind::Inside);
    // The company's star, inlaid by the entrance.
    let c = to(1250.0, 860.0);
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
    gradient(painter, Rect::from_min_max(to(60.0, 560.0), to(map.w - 60.0, 600.0)), Color32::TRANSPARENT, Color32::from_white_alpha(8), true);
    gradient(painter, Rect::from_min_max(to(60.0, 600.0), to(map.w - 60.0, 640.0)), Color32::from_white_alpha(8), Color32::TRANSPARENT, true);
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
            painter.rect_filled(r, 0.0, rgb(84, 88, 94).lerp_to_gamma(rgb(98, 102, 106), hash(i, j)));
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
        let shine = ((now * 0.6).sin() as f32 * 0.5 + 0.5) * 0.2;
        poly(painter, ellipse(to(x - w * 0.15, y - h * 0.15), w * 0.5 * s, h * 0.3 * s, 0.2), Color32::from_white_alpha((20.0 + 30.0 * shine) as u8));
    }
    // The pad: a dark disc, its yellow ring, the H.
    if let Some(heli) = map.desks.iter().find(|d| d.k == "heli") {
        let c = to(heli.x + heli.w / 2.0, heli.y + heli.h / 2.0);
        let r = 240.0 * s;
        painter.circle_filled(c, r, rgb(46, 50, 56));
        painter.circle_stroke(c, r - 14.0 * s, Stroke::new(8.0 * s, rgb(240, 196, 48)));
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            painter.circle_filled(c + Vec2::angled(a) * (r - 2.0 * s), 3.0 * s, if k % 2 == 0 { rgb(255, 240, 200) } else { rgb(255, 90, 60) });
        }
        let (hw, hh, bar) = (90.0 * s, 150.0 * s, 26.0 * s);
        let white = Color32::from_white_alpha(200);
        painter.rect_filled(Rect::from_center_size(c - Vec2::new(hw / 2.0, 0.0), Vec2::new(bar, hh)), 0.0, white);
        painter.rect_filled(Rect::from_center_size(c + Vec2::new(hw / 2.0, 0.0), Vec2::new(bar, hh)), 0.0, white);
        painter.rect_filled(Rect::from_center_size(c, Vec2::new(hw, bar)), 0.0, white);
    }
}

/// The walls in the floor's material, their shade on the ground, the windows.
fn walls(painter: &egui::Painter, map: &Map, to: To, s: f32, field: Rect) {
    let (wall, top) = match map.id.as_str() {
        "hall" => (rgb(132, 124, 112), rgb(170, 162, 148)),
        "board" => (rgb(66, 42, 30), rgb(96, 64, 44)),
        "roof" => (rgb(150, 150, 146), rgb(186, 186, 182)),
        _ => (rgb(56, 63, 80), rgb(78, 88, 108)),
    };
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
            // An island of four desks: their chairs outside, the partition between them.
            let (dw, dh) = ((d.w - 10.0) / 2.0, (d.h - 10.0) / 2.0);
            for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let out = if j == 0.0 { -1.0 } else { 1.0 };
                let cx = d.x + i * (dw + 10.0) + dw / 2.0;
                let cy = if j == 0.0 { d.y - 16.0 } else { d.y + d.h + 16.0 };
                chair(painter, to(cx, cy), s, Vec2::new(0.0, out), rgb(40, 44, 54));
            }
            shadow(6.0 * s);
            for (k, (i, j)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
                let desk = Rect::from_min_size(to(d.x + i * (dw + 10.0), d.y + j * (dh + 10.0)), Vec2::new(dw, dh) * s);
                gradient(painter, desk, rgb(214, 194, 158), rgb(186, 164, 128), true);
                painter.rect_stroke(desk, 4.0 * s, Stroke::new(1.0, Color32::from_black_alpha(50)), egui::StrokeKind::Inside);
                // The screen toward its chair, glowing; the keyboard, a mug, papers.
                let toward = if j == 0.0 { -1.0 } else { 1.0 };
                let screen = Rect::from_center_size(desk.center() - Vec2::new(0.0, toward * dh * 0.22) * s, Vec2::new(46.0, 8.0) * s);
                painter.rect_filled(screen, 2.0 * s, rgb(24, 28, 38));
                let flicker = 0.75 + 0.25 * ((now * 3.0 + k as f64 * 1.7 + f64::from(d.x)).sin() as f32 * 0.5 + 0.5);
                let glow = [rgb(90, 170, 255), rgb(120, 230, 160), rgb(255, 200, 110), rgb(200, 140, 255)][(k + d.x as usize) % 4];
                painter.rect_filled(screen.shrink(1.5 * s).translate(Vec2::new(0.0, toward * 1.5 * s)), 1.0, glow.gamma_multiply(flicker * 0.8));
                radial(painter, screen.center() + Vec2::new(0.0, toward * 14.0 * s), 26.0 * s, glow.gamma_multiply(0.18 * flicker), Color32::TRANSPARENT);
                painter.rect_filled(Rect::from_center_size(desk.center() + Vec2::new(0.0, toward * dh * 0.12) * s, Vec2::new(36.0, 9.0) * s), 1.5 * s, rgb(232, 232, 236));
                painter.circle_filled(desk.center() + Vec2::new(dw * 0.36, toward * dh * 0.1) * s, 5.0 * s, rgb(236, 230, 216));
                painter.circle_filled(desk.center() + Vec2::new(dw * 0.36, toward * dh * 0.1) * s, 3.0 * s, rgb(90, 56, 30));
                let paper = Rect::from_center_size(desk.center() + Vec2::new(-dw * 0.34, toward * dh * 0.08) * s, Vec2::new(16.0, 20.0) * s);
                painter.rect_filled(paper, 0.0, Color32::from_white_alpha(220));
            }
            let mid = Rect::from_center_size(to(d.x + d.w / 2.0, d.y + d.h / 2.0), Vec2::new(d.w - 6.0, 6.0) * s);
            painter.rect_filled(mid, 2.0 * s, rgb(92, 104, 128));
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
        "antenna" => {
            let c = r.center();
            painter.rect_stroke(r, 0.0, Stroke::new(2.0 * s, rgb(170, 170, 176)), egui::StrokeKind::Inside);
            painter.line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.5 * s, rgb(170, 170, 176)));
            painter.line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.5 * s, rgb(170, 170, 176)));
            let blink = (now * 1.5).fract() < 0.3;
            radial(painter, c, 30.0 * s, if blink { Color32::from_rgba_unmultiplied(255, 60, 50, 110) } else { Color32::TRANSPARENT }, Color32::TRANSPARENT);
            painter.circle_filled(c, 4.0 * s, if blink { rgb(255, 70, 60) } else { rgb(120, 40, 36) });
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
        let ctx = egui::Context::default();
        let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
        let desk = |k: &str| Desk { x: 600.0, y: 300.0, w: 300.0, h: 140.0, k: k.into() };
        for id in ["hall", "openspace", "board", "roof", "?"] {
            let map = Map {
                id: id.into(),
                w: 1600.0,
                h: 1000.0,
                wall: 20.0,
                desks: ["desk", "table", "counter", "sofa", "lowtable", "plant", "fountain", "gates", "boardtable", "flipchart", "credenza", "heli", "hvac", "tank", "antenna", "?"].iter().map(|k| desk(k)).filter(|d| d.k != "heli").collect(),
                ..Default::default()
            };
            let to = |x: f32, y: f32| Pos2::new(x * 0.5, y * 0.5);
            // Text needs the fonts: the helicopter's letters are left out here.
            paint_floor(&painter, &map, &to, 0.5, 1.0, false);
            paint_shade(&painter, Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0)), id == "roof");
        }
    }
}
