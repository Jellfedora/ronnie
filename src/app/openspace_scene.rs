//! Open Space, drawn: the open space seen from above at an angle (isometric), on a slab of floor
//! (carpet, the parquet of its edge), each building in volume (its faces lit from the left, its
//! shadow on the floor), its look growing with its level (taller, a silver then a golden trim).
//! The coffee machines steam, the servers blink, the boss's office lights up at night (the hour of
//! the computer), the works show their scaffolding and what's left.
//!
//! Everything is drawn in code, in grid coordinates (`Iso`: the grid to the screen and back), the
//! camera (`View`) moved by a drag and zoomed by the wheel. What moves only on screen (the floating
//! texts, the sparks of a level gained) is the scene's alone: nothing of it is saved.

use std::collections::HashMap;

use super::*;
use crate::openspace::{self as os, Building, Kind, OpenSpace, Res, GRID};
use crate::openspace_battle::{Battle, FxKind, Troop};

/// The camera.
#[derive(Clone, Copy)]
pub(super) struct View {
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 0.8, pan: Vec2::ZERO }
    }
}

/// From the grid to the screen (and back): a tile is `2 hw` wide, `2 hh` high.
#[derive(Clone, Copy)]
pub(super) struct Iso {
    origin: Pos2,
    hw: f32,
    hh: f32,
}

impl Iso {
    pub fn new(rect: Rect, view: &View) -> Self {
        let hw = 30.0 * view.zoom;
        let hh = hw * 0.5;
        let origin = rect.center() + view.pan - Vec2::new(0.0, GRID as f32 * hh);
        Self { origin, hw, hh }
    }

    /// A point of the floor.
    pub fn at(&self, gx: f32, gy: f32) -> Pos2 {
        self.origin + Vec2::new((gx - gy) * self.hw, (gx + gy) * self.hh)
    }

    /// `z` above `p` (`z` in pixels at zoom 1).
    pub fn up(&self, p: Pos2, z: f32) -> Pos2 {
        p - Vec2::new(0.0, z * self.k())
    }

    /// The point of the floor under `p`.
    pub fn back(&self, p: Pos2) -> (f32, f32) {
        let d = p - self.origin;
        let (a, b) = (d.x / self.hw, d.y / self.hh);
        ((a + b) / 2.0, (b - a) / 2.0)
    }

    /// The scale (1 at zoom 1).
    pub fn k(&self) -> f32 {
        self.hw / 30.0
    }
}

/// A text rising from a building.
struct Float {
    text: String,
    color: Color32,
    at: (f32, f32),
    since: f64,
}

/// A burst of sparks (a level gained).
struct Burst {
    at: (f32, f32),
    since: f64,
    gold: bool,
}

#[derive(Default)]
pub(super) struct Scene {
    floats: Vec<Float>,
    bursts: Vec<Burst>,
    /// Each building's level when last drawn (a level gained: sparks).
    levels: HashMap<u32, u8>,
}

impl Scene {
    pub fn float(&mut self, text: String, color: Color32, at: (f32, f32), now: f64) {
        self.floats.push(Float { text, color, at, since: now });
    }
}

/// What the page asks the scene to show over the open space.
pub(super) struct Overlay {
    pub selected: Option<u32>,
    /// A building being placed: its kind, its top corner, whether it can go there.
    pub ghost: Option<(Kind, i32, i32, bool)>,
    /// A building being moved: its id, where it would go, whether it can.
    pub moving: Option<(u32, i32, i32, bool)>,
    pub hover: Option<u32>,
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// `c` darkened (or lit, past 1) by `f`, its alpha kept.
fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| (v as f32 * f).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (c.a() as f32 * a.clamp(0.0, 1.0)) as u8)
}

fn quad(painter: &egui::Painter, pts: [Pos2; 4], fill: Color32, edge: Color32) {
    painter.add(egui::Shape::convex_polygon(pts.to_vec(), fill, Stroke::new(0.8, edge)));
}

/// A box on the floor from (x0, y0) to (x1, y1), from `z0` to `z1` high: its two front faces (lit
/// from the left) and its top.
#[allow(clippy::too_many_arguments)]
fn block(painter: &egui::Painter, iso: &Iso, x0: f32, y0: f32, x1: f32, y1: f32, z0: f32, z1: f32, color: Color32) {
    let (n, e, s, w) = (iso.at(x0, y0), iso.at(x1, y0), iso.at(x1, y1), iso.at(x0, y1));
    let edge = alpha(shade(color, 0.45), 0.6);
    quad(painter, [iso.up(w, z0), iso.up(s, z0), iso.up(s, z1), iso.up(w, z1)], shade(color, 0.8), edge);
    quad(painter, [iso.up(s, z0), iso.up(e, z0), iso.up(e, z1), iso.up(s, z1)], shade(color, 0.62), edge);
    quad(painter, [iso.up(n, z1), iso.up(e, z1), iso.up(s, z1), iso.up(w, z1)], shade(color, 1.08), edge);
}

/// A point on a box's left face (`u` along it, `v` up it, 0 to 1).
#[allow(clippy::too_many_arguments)]
fn left_face(iso: &Iso, x0: f32, y1: f32, x1: f32, z0: f32, z1: f32, u: f32, v: f32) -> Pos2 {
    let p = iso.at(x0 + (x1 - x0) * u, y1);
    iso.up(p, z0 + (z1 - z0) * v)
}

/// A point on a box's right face (`u` along it from its front corner, `v` up it).
#[allow(clippy::too_many_arguments)]
fn right_face(iso: &Iso, x1: f32, y1: f32, y0: f32, z0: f32, z1: f32, u: f32, v: f32) -> Pos2 {
    let p = iso.at(x1, y1 + (y0 - y1) * u);
    iso.up(p, z0 + (z1 - z0) * v)
}

/// A window (or a screen, a door) on a face: from (u0, v0) to (u1, v1).
fn pane(painter: &egui::Painter, f: impl Fn(f32, f32) -> Pos2, u0: f32, v0: f32, u1: f32, v1: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![f(u0, v0), f(u1, v0), f(u1, v1), f(u0, v1)], color, Stroke::NONE));
}

/// A standing cylinder at (cx, cy), `r` tiles wide.
#[allow(clippy::too_many_arguments)]
fn cylinder(painter: &egui::Painter, iso: &Iso, cx: f32, cy: f32, r: f32, z0: f32, z1: f32, color: Color32) {
    let c = iso.at(cx, cy);
    let radius = Vec2::new(r * iso.hw * std::f32::consts::SQRT_2, r * iso.hh * std::f32::consts::SQRT_2);
    let (b, t) = (iso.up(c, z0), iso.up(c, z1));
    painter.add(egui::Shape::ellipse_filled(b, radius, shade(color, 0.7)));
    painter.rect_filled(Rect::from_min_max(Pos2::new(t.x - radius.x, t.y), Pos2::new(b.x + radius.x, b.y)), 0.0, shade(color, 0.75));
    // Lit on its left half.
    painter.rect_filled(Rect::from_min_max(Pos2::new(t.x - radius.x, t.y), Pos2::new(t.x - radius.x * 0.2, b.y)), 0.0, shade(color, 0.88));
    painter.add(egui::Shape::ellipse_filled(t, radius, shade(color, 1.1)));
}

/// The floor's diamond of tile (x, y), `inset` tiles in.
fn diamond(iso: &Iso, x: f32, y: f32, w: f32, inset: f32) -> Vec<Pos2> {
    vec![iso.at(x + inset, y + inset), iso.at(x + w - inset, y + inset), iso.at(x + w - inset, y + w - inset), iso.at(x + inset, y + w - inset)]
}

/// The colors of a level: its trim (none, silver, gold).
fn trim(level: u8) -> Option<Color32> {
    match level {
        0..=3 => None,
        4..=7 => Some(rgb(0xd8dde6)),
        _ => Some(rgb(0xf2c14e)),
    }
}

/// How high a building stands (pixels at zoom 1), for drawing and picking.
pub(super) fn height(kind: Kind, level: u8) -> f32 {
    let l = level.max(1) as f32;
    match kind {
        Kind::Hq => 62.0 + 4.0 * l,
        Kind::Coffee => 26.0 + 1.5 * l,
        Kind::Server => 48.0 + 2.0 * l,
        Kind::Fridge => 40.0 + 2.0 * l,
        Kind::Safe => 30.0 + 2.0 * l,
        Kind::Interns => 22.0,
        Kind::Recruit => 34.0 + 2.0 * l,
        Kind::Lounge => 16.0,
        Kind::Meeting => 30.0 + 1.5 * l,
        Kind::Wall => 14.0 + 1.6 * l,
        Kind::Linter => 50.0 + 2.5 * l,
        Kind::Review => 30.0 + 1.5 * l,
        Kind::Alarm => 32.0 + 2.0 * l,
        Kind::Antivirus => 36.0 + 2.0 * l,
        Kind::Honeypot => 26.0 + 1.5 * l,
        Kind::Cluster => 46.0 + 4.0 * l,
        Kind::Board => 52.0 + 6.0 * l,
        Kind::Bug | Kind::Printer | Kind::Aircon => 4.0,
        Kind::Training => 38.0 + 2.0 * l,
        Kind::Heroes => 44.0 + 3.0 * l,
        Kind::Plant => 26.0,
        Kind::Fountain => 30.0,
        Kind::Foosball => 16.0,
        Kind::Sofa => 14.0,
        Kind::Arcade => 34.0,
        Kind::Statue => 48.0,
        Kind::Boxes => 18.0,
        Kind::OldPc => 16.0,
        Kind::DeadPlant => 18.0,
        Kind::Chairs => 14.0,
    }
}

/// The outline of a building on screen (its foot and its height): to pick it.
pub(super) fn outline(iso: &Iso, b: &Building) -> Vec<Pos2> {
    let s = b.kind.size() as f32;
    let (x, y) = (b.x as f32, b.y as f32);
    let z = height(b.kind, b.level) + 14.0;
    let (n, e, so, w) = (iso.at(x, y), iso.at(x + s, y), iso.at(x + s, y + s), iso.at(x, y + s));
    vec![iso.up(n, z), iso.up(e, z), e, so, w, iso.up(w, z)]
}

pub(super) fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether a machine has enough to show its bubble.
pub(super) fn ripe(b: &Building) -> bool {
    b.kind.makes().is_some() && b.level >= 1 && b.stored >= (os::production(b.level).1 * 0.02).max(10.0)
}

/// Where a machine's bubble is, on screen.
pub(super) fn bubble_at(iso: &Iso, b: &Building, now: f64) -> Pos2 {
    let s = b.kind.size() as f32;
    let top = iso.up(iso.at(b.x as f32 + s / 2.0, b.y as f32 + s / 2.0), height(b.kind, b.level) + 26.0);
    top + Vec2::new(0.0, ((now * 2.4 + b.id as f64).sin() as f32) * 3.0 * iso.k())
}

/// A coffee cup, `r` wide.
pub(super) fn paint_cup(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let body = Rect::from_center_size(c + Vec2::new(-0.15 * r, 0.1 * r), Vec2::new(1.1 * r, 1.0 * r));
    painter.rect_filled(body, egui::CornerRadius { nw: 0, ne: 0, sw: (0.35 * r) as u8, se: (0.35 * r) as u8 }, color);
    painter.circle_stroke(Pos2::new(body.max.x + 0.05 * r, body.center().y), 0.25 * r, Stroke::new(0.18 * r, color));
    for k in [-0.3, 0.05] {
        let x = c.x + k * r;
        painter.line_segment([Pos2::new(x, c.y - 0.55 * r), Pos2::new(x + 0.08 * r, c.y - 0.85 * r)], Stroke::new(0.12 * r, alpha(color, 0.7)));
    }
}

/// A banknote, `r` wide.
pub(super) fn paint_bill(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let note = Rect::from_center_size(c, Vec2::new(1.5 * r, 0.85 * r));
    painter.rect_filled(note, 0.12 * r, color);
    painter.rect_stroke(note.shrink(0.12 * r), 0.08 * r, Stroke::new(0.08 * r, shade(color, 0.6)), egui::StrokeKind::Inside);
    painter.circle_filled(c, 0.2 * r, shade(color, 0.6));
}

/// A ticket, `r` wide.
pub(super) fn paint_ticket(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let t = Rect::from_center_size(c, Vec2::new(1.4 * r, 0.8 * r));
    painter.rect_filled(t, 0.12 * r, color);
    for side in [t.min.x, t.max.x] {
        painter.circle_filled(Pos2::new(side, c.y), 0.16 * r, Color32::from_black_alpha(160));
    }
    painter.line_segment([Pos2::new(c.x + 0.2 * r, t.min.y + 0.12 * r), Pos2::new(c.x + 0.2 * r, t.max.y - 0.12 * r)], Stroke::new(0.06 * r, shade(color, 0.6)));
}

pub(super) fn res_color(r: Res) -> Color32 {
    match r {
        Res::Coffee => rgb(0xc98a4b),
        Res::Budget => rgb(0x5cc98a),
    }
}

pub(super) fn paint_res(painter: &egui::Painter, r: Res, c: Pos2, size: f32) {
    match r {
        Res::Coffee => paint_cup(painter, c, size, res_color(r)),
        Res::Budget => paint_bill(painter, c, size, res_color(r)),
    }
}

/// A small hash, for what looks random but stays put (the lights of a server).
fn hash(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65_535.0
}

/// The night's share (0 by day, 1 deep in the night), by the computer's hour.
pub(super) fn night() -> f32 {
    use chrono::Timelike;
    let now = chrono::Local::now();
    let h = now.hour() as f32 + now.minute() as f32 / 60.0;
    if (8.0..18.0).contains(&h) {
        0.0
    } else if (6.0..8.0).contains(&h) {
        (8.0 - h) / 2.0
    } else if (18.0..20.0).contains(&h) {
        (h - 18.0) / 2.0
    } else {
        1.0
    }
}

/// The floor: a slab, its carpet in squares, its edge in parquet.
fn paint_floor(painter: &egui::Painter, iso: &Iso, dark: bool) {
    let g = GRID as f32;
    let slab = if dark { rgb(0x2a2d38) } else { rgb(0x8d8fa0) };
    let depth = 18.0;
    let (e, s, w) = (iso.at(g, 0.0), iso.at(g, g), iso.at(0.0, g));
    quad(painter, [w, s, iso.up(s, -depth), iso.up(w, -depth)], shade(slab, 0.85), Color32::TRANSPARENT);
    quad(painter, [s, e, iso.up(e, -depth), iso.up(s, -depth)], shade(slab, 0.65), Color32::TRANSPARENT);
    let (carpet_a, carpet_b, parquet_a, parquet_b) = if dark { (rgb(0x3b4256), rgb(0x373e51), rgb(0x6b4f36), rgb(0x634a33)) } else { (rgb(0xa9b4c8), rgb(0xa2adc2), rgb(0xc49a6c), rgb(0xbb9265)) };
    let mut mesh = egui::Mesh::default();
    for x in 0..GRID {
        for y in 0..GRID {
            let edge = x == 0 || y == 0 || x == GRID - 1 || y == GRID - 1;
            let checker = (x / 2 + y / 2) % 2 == 0;
            let color = match (edge, checker) {
                (true, true) => parquet_a,
                (true, false) => parquet_b,
                (false, true) => carpet_a,
                (false, false) => carpet_b,
            };
            let base = mesh.vertices.len() as u32;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                mesh.colored_vertex(iso.at(x as f32 + dx, y as f32 + dy), color);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    // The grid's lines, faint, every tile.
    let line = Stroke::new(0.6, Color32::from_black_alpha(if dark { 28 } else { 18 }));
    for k in 1..GRID {
        let k = k as f32;
        painter.line_segment([iso.at(k, 1.0), iso.at(k, g - 1.0)], line);
        painter.line_segment([iso.at(1.0, k), iso.at(g - 1.0, k)], line);
    }
}

/// A building, whole: its shadow, its plinth, its body by kind, its trim, its works.
#[allow(clippy::too_many_arguments)]
fn paint_building(painter: &egui::Painter, iso: &Iso, b: &Building, walls: &std::collections::HashSet<(i32, i32)>, now: f64, unix: i64, night: f32, ghost: f32) {
    let kind = b.kind;
    let s = kind.size() as f32;
    let (x, y) = (b.x as f32, b.y as f32);
    let level = b.level.max(1);
    let a = |c: Color32| if ghost < 1.0 { alpha(c, ghost) } else { c };
    let building = b.level == 0;
    // Being built the first time: it rises with the work.
    let rise = if building { b.progress(unix).unwrap_or(0.0).clamp(0.08, 1.0) as f32 } else { 1.0 };
    let h = height(kind, level) * rise;

    // The shadow, toward the bottom right.
    if kind != Kind::Wall && kind.group() != os::Group::Traps {
        let sh = vec![iso.at(x + 0.15, y + 0.1), iso.at(x + s + 0.55, y + 0.35), iso.at(x + s + 0.55, y + s + 0.35), iso.at(x + 0.15, y + s + 0.1)];
        painter.add(egui::Shape::convex_polygon(sh, Color32::from_black_alpha((50.0 * ghost) as u8), Stroke::NONE));
    }

    let m = if kind == Kind::Wall { 0.0 } else { 0.12 };
    let (x0, y0, x1, y1) = (x + m, y + m, x + s - m, y + s - m);
    match kind {
        Kind::Hq => {
            block(painter, iso, x0, y0, x1, y1, 0.0, 6.0, a(rgb(0x6d7486)));
            let (bx0, by0, bx1, by1) = (x0 + 0.3, y0 + 0.3, x1 - 0.3, y1 - 0.3);
            let glass = a(rgb(0x5a86c7));
            block(painter, iso, bx0, by0, bx1, by1, 6.0, h, glass);
            // The windows' rows, lit at night.
            let floors = (2 + level / 2) as usize;
            for f in 0..floors {
                let v0 = 0.12 + f as f32 * 0.82 / floors as f32;
                let v1 = v0 + 0.82 / floors as f32 * 0.55;
                for k in 0..4 {
                    let u0 = 0.08 + k as f32 * 0.23;
                    let lit = night > 0.3 && hash(b.id + f as u32, k) > 0.35;
                    let c = if lit { rgb(0xffd27a) } else { rgb(0x9cc3f0) };
                    pane(painter, |u, v| left_face(iso, bx0, by1, bx1, 6.0, h, u, v), u0, v0, u0 + 0.16, v1, a(alpha(c, 0.85)));
                    pane(painter, |u, v| right_face(iso, bx1, by1, by0, 6.0, h, u, v), u0, v0, u0 + 0.16, v1, a(alpha(shade(c, 0.85), 0.85)));
                }
            }
            // The roof: a sign and the boss's star.
            let top = iso.up(iso.at((bx0 + bx1) / 2.0, (by0 + by1) / 2.0), h);
            block(painter, iso, bx0 + 0.6, by0 + 0.6, bx1 - 0.6, by1 - 0.6, h, h + 5.0, a(rgb(0x4b5163)));
            let star_c = trim(level).unwrap_or(rgb(0xe8eef8));
            painter.text(iso.up(top, 14.0), Align2::CENTER_CENTER, "★", FontId::proportional(16.0 * iso.k()), a(star_c));
        }
        Kind::Coffee => {
            let body = a(rgb(0x7a3d2a));
            block(painter, iso, x0 + 0.3, y0 + 0.3, x1 - 0.3, y1 - 0.3, 0.0, h, body);
            pane(painter, |u, v| left_face(iso, x0 + 0.3, y1 - 0.3, x1 - 0.3, 0.0, h, u, v), 0.25, 0.15, 0.75, 0.45, a(rgb(0x2b1a14)));
            pane(painter, |u, v| left_face(iso, x0 + 0.3, y1 - 0.3, x1 - 0.3, 0.0, h, u, v), 0.3, 0.6, 0.7, 0.85, a(rgb(0xe8c27a)));
            // The cup on top, and its steam.
            let c = iso.at((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cylinder(painter, iso, (x0 + x1) / 2.0, (y0 + y1) / 2.0, 0.35, h, h + 10.0, a(rgb(0xf2ede4)));
            if b.producing() {
                for k in 0..3 {
                    let phase = ((now * 0.45 + k as f64 / 3.0 + b.id as f64 * 0.13) % 1.0) as f32;
                    let p = iso.up(c, h + 14.0 + phase * 26.0) + Vec2::new((phase * 6.0 + k as f32).sin() * 4.0 * iso.k(), 0.0);
                    painter.circle_filled(p, (3.0 + phase * 5.0) * iso.k(), Color32::from_white_alpha(((1.0 - phase) * 70.0 * ghost) as u8));
                }
            }
        }
        Kind::Server => {
            let rack = a(rgb(0x23262f));
            for k in 0..2 {
                let ox = x0 + 0.2 + k as f32 * 1.35;
                block(painter, iso, ox, y0 + 0.3, ox + 1.1, y1 - 0.3, 0.0, h, rack);
                for row in 0..6 {
                    let v = 0.1 + row as f32 * 0.14;
                    for led in 0..3 {
                        let on = ((now * (0.8 + hash(b.id, row * 7 + led) as f64 * 3.0) + hash(row, led) as f64 * 10.0) % 1.0) > 0.45;
                        let c = if !b.producing() { rgb(0x553333) } else if on { if led == 0 { rgb(0x50fa7b) } else { rgb(0x56b6ff) } } else { rgb(0x1c3a2a) };
                        let u = 0.2 + led as f32 * 0.22;
                        pane(painter, |u, v| left_face(iso, ox, y1 - 0.3, ox + 1.1, 0.0, h, u, v), u, v, u + 0.12, v + 0.05, a(c));
                    }
                    pane(painter, |u, v| right_face(iso, ox + 1.1, y1 - 0.3, y0 + 0.3, 0.0, h, u, v), 0.1, v, 0.9, v + 0.04, a(rgb(0x3a3f4c)));
                }
            }
        }
        Kind::Fridge => {
            let body = a(rgb(0xe9edf2));
            block(painter, iso, x0 + 0.4, y0 + 0.4, x1 - 0.4, y1 - 0.4, 0.0, h, body);
            let f = |u, v| left_face(iso, x0 + 0.4, y1 - 0.4, x1 - 0.4, 0.0, h, u, v);
            painter.line_segment([f(0.0, 0.62), f(1.0, 0.62)], Stroke::new(1.2 * iso.k(), a(rgb(0xb8c0cc))));
            painter.line_segment([f(0.85, 0.7), f(0.85, 0.9)], Stroke::new(2.0 * iso.k(), a(rgb(0x8f98a6))));
            painter.line_segment([f(0.85, 0.2), f(0.85, 0.5)], Stroke::new(2.0 * iso.k(), a(rgb(0x8f98a6))));
            // A post-it, brown with coffee.
            pane(painter, f, 0.2, 0.3, 0.4, 0.45, a(rgb(0xf7d774)));
        }
        Kind::Safe => {
            let body = a(rgb(0x5d6577));
            block(painter, iso, x0 + 0.3, y0 + 0.3, x1 - 0.3, y1 - 0.3, 0.0, h, body);
            let f = |u, v| right_face(iso, x1 - 0.3, y1 - 0.3, y0 + 0.3, 0.0, h, u, v);
            let dial = f(0.5, 0.55);
            painter.circle_filled(dial, 6.0 * iso.k(), a(rgb(0xc9ced8)));
            painter.circle_stroke(dial, 6.0 * iso.k(), Stroke::new(1.0, a(rgb(0x3a3f4c))));
            painter.line_segment([dial, dial + Vec2::angled(now as f32 * 0.3) * 5.0 * iso.k()], Stroke::new(1.4 * iso.k(), a(rgb(0x3a3f4c))));
        }
        Kind::Interns => {
            // A desk, a screen, an intern.
            let wood = a(rgb(0xb98552));
            block(painter, iso, x0 + 0.1, y0 + 0.3, x1 - 0.1, y1 - 0.5, 0.0, 12.0, wood);
            block(painter, iso, x0 + 0.5, y0 + 0.4, x1 - 0.5, y0 + 0.55, 12.0, 24.0, a(rgb(0x2b2f3a)));
            pane(painter, |u, v| left_face(iso, x0 + 0.5, y0 + 0.55, x1 - 0.5, 12.0, 24.0, u, v), 0.08, 0.15, 0.92, 0.9, a(rgb(0x5ab0ff)));
            let seat = iso.at((x0 + x1) / 2.0, y1 - 0.15);
            let bob = ((now * 3.0 + b.id as f64).sin() as f32) * 1.0;
            painter.circle_filled(iso.up(seat, 12.0 + bob), 5.5 * iso.k(), a(rgb(0x3f7de0)));
            painter.circle_filled(iso.up(seat, 22.0 + bob), 4.2 * iso.k(), a(rgb(0xf0c9a0)));
            // Their hard hat.
            painter.circle_filled(iso.up(seat, 25.0 + bob), 3.6 * iso.k(), a(rgb(0xf2c14e)));
        }
        Kind::Recruit => {
            let body = a(rgb(0x2f8f83));
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 0.0, h, body);
            pane(painter, |u, v| left_face(iso, x0 + 0.2, y1 - 0.2, x1 - 0.2, 0.0, h, u, v), 0.38, 0.0, 0.62, 0.6, a(rgb(0x1b4f48)));
            // A flag on the roof.
            let mast = iso.up(iso.at(x0 + 0.8, y0 + 0.8), h);
            let top = mast - Vec2::new(0.0, 22.0 * iso.k());
            painter.line_segment([mast, top], Stroke::new(1.5 * iso.k(), a(rgb(0xdddddd))));
            let wave = (now as f32 * 4.0).sin() * 2.0 * iso.k();
            painter.add(egui::Shape::convex_polygon(vec![top, top + Vec2::new(14.0 * iso.k(), 4.0 * iso.k() + wave), top + Vec2::new(0.0, 9.0 * iso.k())], a(rgb(0xff6b5e)), Stroke::NONE));
        }
        Kind::Lounge => {
            // A rug, sofas, a plant, a table football.
            block(painter, iso, x0, y0, x1, y1, 0.0, 2.0, a(rgb(0x8e5bb5)));
            block(painter, iso, x0 + 0.3, y0 + 0.3, x0 + 0.9, y1 - 0.3, 2.0, 12.0, a(rgb(0xe07a5f)));
            block(painter, iso, x0 + 0.9, y0 + 0.3, x1 - 0.3, y0 + 0.9, 2.0, 12.0, a(rgb(0xe07a5f)));
            block(painter, iso, x0 + 1.7, y0 + 1.7, x1 - 0.8, y1 - 0.8, 2.0, 10.0, a(rgb(0x2f6b3a)));
            cylinder(painter, iso, x1 - 0.5, y1 - 0.5, 0.25, 2.0, 10.0, a(rgb(0x9a6b45)));
            let leaf = iso.up(iso.at(x1 - 0.5, y1 - 0.5), 18.0);
            painter.circle_filled(leaf, 7.0 * iso.k(), a(rgb(0x3fa34d)));
            painter.circle_filled(leaf + Vec2::new(-3.0, -4.0) * iso.k(), 5.0 * iso.k(), a(rgb(0x52c062)));
        }
        Kind::Meeting => {
            let glass = a(alpha(rgb(0x8fd3e8), 0.55));
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 0.0, 3.0, a(rgb(0x6d7486)));
            // The table and the chairs inside, then the glass.
            block(painter, iso, x0 + 0.8, y0 + 0.9, x1 - 0.8, y1 - 0.9, 3.0, 13.0, a(rgb(0xf2f2f2)));
            for (cx, cy) in [(x0 + 0.55, y0 + 1.5), (x1 - 0.55, y0 + 1.5), (x0 + 1.5, y0 + 0.55), (x0 + 1.5, y1 - 0.55)] {
                cylinder(painter, iso, cx, cy, 0.18, 3.0, 10.0, a(rgb(0x30343f)));
            }
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 3.0, h, glass);
        }
        Kind::Wall => {
            let c = match b.level {
                0..=1 => rgb(0xd9cfb8),
                2..=3 => rgb(0xa9adb6),
                4..=5 => rgb(0x6f7686),
                6..=7 => rgb(0x3f6fb5),
                8..=9 => rgb(0x2f3442),
                _ => rgb(0xf2c14e),
            };
            let c = a(c);
            block(painter, iso, x + 0.25, y + 0.25, x + 0.75, y + 0.75, 0.0, h, c);
            // Joined to the next ones (the ones behind are drawn by them).
            if walls.contains(&(b.x + 1, b.y)) {
                block(painter, iso, x + 0.75, y + 0.32, x + 1.25, y + 0.68, 0.0, h * 0.82, c);
            }
            if walls.contains(&(b.x, b.y + 1)) {
                block(painter, iso, x + 0.32, y + 0.75, x + 0.68, y + 1.25, 0.0, h * 0.82, c);
            }
        }
        Kind::Linter => {
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 0.0, 10.0, a(rgb(0x5d6577)));
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cylinder(painter, iso, cx, cy, 0.5, 10.0, h - 12.0, a(rgb(0x8a93a6)));
            block(painter, iso, cx - 0.55, cy - 0.55, cx + 0.55, cy + 0.55, h - 12.0, h, a(rgb(0x6b4fbb)));
            // Its barrel, looking around.
            let head = iso.up(iso.at(cx, cy), h - 5.0);
            let dir = Vec2::angled((now * 0.4 + b.id as f64) as f32);
            painter.line_segment([head, head + Vec2::new(dir.x, dir.y * 0.5) * 16.0 * iso.k()], Stroke::new(3.0 * iso.k(), a(rgb(0x2b2f3a))));
            painter.circle_filled(iso.up(iso.at(cx, cy), h + 3.0), 2.5 * iso.k(), a(if (now * 2.0) % 1.0 > 0.5 { rgb(0x50fa7b) } else { rgb(0x2a7a44) }));
        }
        Kind::Review => {
            block(painter, iso, x0 + 0.2, y0 + 0.5, x1 - 0.2, y1 - 0.2, 0.0, 14.0, a(rgb(0x4a4f5e)));
            // The big screen, the diff on it.
            let (sx0, sx1, sy) = (x0 + 0.3, x1 - 0.3, y0 + 0.5);
            block(painter, iso, sx0, sy - 0.15, sx1, sy, 14.0, h, a(rgb(0x1e2129)));
            let f = |u, v| left_face(iso, sx0, sy, sx1, 14.0, h, u, v);
            pane(painter, f, 0.06, 0.1, 0.94, 0.9, a(rgb(0x272b35)));
            for line in 0..5 {
                let v = 0.2 + line as f32 * 0.13;
                let red = hash(b.id, line) > 0.5;
                let w = 0.3 + hash(line, b.id) * 0.5;
                pane(painter, f, 0.12, v, 0.12 + w, v + 0.06, a(if red { rgb(0xff6b6b) } else { rgb(0x50fa7b) }));
            }
        }
        Kind::Alarm => {
            block(painter, iso, x0 + 0.3, y0 + 0.3, x1 - 0.3, y1 - 0.3, 0.0, h * 0.5, a(rgb(0x8a2f2f)));
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cylinder(painter, iso, cx, cy, 0.55, h * 0.5, h * 0.8, a(rgb(0xd33b3b)));
            let dome = iso.up(iso.at(cx, cy), h * 0.8);
            painter.add(egui::Shape::ellipse_filled(dome, Vec2::new(16.0, 9.0) * iso.k(), a(rgb(0xe04848))));
            let flash = ((now * 1.6) % 1.0) < 0.15;
            painter.circle_filled(iso.up(dome, 6.0), 4.0 * iso.k(), a(if flash { rgb(0xfff2a8) } else { rgb(0xffb3b3) }));
            if flash {
                painter.circle_filled(iso.up(dome, 6.0), 14.0 * iso.k(), Color32::from_rgba_unmultiplied(255, 120, 120, 40));
            }
        }
        Kind::Antivirus => {
            // A shield on a mast, a radar turning.
            block(painter, iso, x0 + 0.3, y0 + 0.3, x1 - 0.3, y1 - 0.3, 0.0, 12.0, a(rgb(0x2d6a4f)));
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cylinder(painter, iso, cx, cy, 0.3, 12.0, h - 10.0, a(rgb(0x95a3b8)));
            let top = iso.up(iso.at(cx, cy), h - 6.0);
            let turn = (now * 1.5 + b.id as f64) as f32;
            painter.add(egui::Shape::ellipse_filled(top, Vec2::new(16.0 * turn.cos().abs().max(0.25), 10.0) * iso.k(), a(rgb(0x52b788))));
            painter.add(egui::Shape::ellipse_stroke(top, Vec2::new(16.0 * turn.cos().abs().max(0.25), 10.0) * iso.k(), Stroke::new(1.5, a(rgb(0x1b4332)))));
            painter.text(top, Align2::CENTER_CENTER, "✚", FontId::proportional(11.0 * iso.k()), a(Color32::WHITE));
        }
        Kind::Honeypot => {
            // A forgotten server, a jar of honey on top, sparks now and then.
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 0.0, h - 8.0, a(rgb(0x6b5b3e)));
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cylinder(painter, iso, cx, cy, 0.3, h - 8.0, h + 2.0, a(rgb(0xf4a261)));
            if (now * 0.7 + b.id as f64) % 1.0 < 0.12 {
                let p = iso.up(iso.at(cx, cy), h + 8.0);
                for k in 0..4 {
                    let d = Vec2::angled(k as f32 * 1.6 + now as f32 * 9.0) * 9.0 * iso.k();
                    painter.line_segment([p, p + d], Stroke::new(1.5 * iso.k(), a(rgb(0x9bf6ff))));
                }
            }
        }
        Kind::Cluster => {
            // A rack of servers with a multi-barrel on top, spinning.
            block(painter, iso, x0 + 0.1, y0 + 0.1, x1 - 0.1, y1 - 0.1, 0.0, h * 0.6, a(rgb(0x1d1f27)));
            for k in 0..4 {
                let v = 0.15 + k as f32 * 0.2;
                pane(painter, |u, v| left_face(iso, x0 + 0.1, y1 - 0.1, x1 - 0.1, 0.0, h * 0.6, u, v), 0.1, v, 0.9, v + 0.06, a(if (now * 3.0 + k as f64) % 1.0 > 0.5 { rgb(0x4cc9f0) } else { rgb(0x23395b) }));
            }
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            block(painter, iso, cx - 0.6, cy - 0.6, cx + 0.6, cy + 0.6, h * 0.6, h * 0.85, a(rgb(0x3a0ca3)));
            let head = iso.up(iso.at(cx, cy), h * 0.95);
            let dir = Vec2::angled((now * 0.6 + b.id as f64) as f32);
            for k in -1..=1 {
                let off = Vec2::new(-dir.y, dir.x * 0.5) * k as f32 * 3.0 * iso.k();
                painter.line_segment([head + off, head + off + Vec2::new(dir.x, dir.y * 0.5) * 22.0 * iso.k()], Stroke::new(2.2 * iso.k(), a(rgb(0x14161c))));
            }
            painter.circle_filled(head, 6.0 * iso.k(), a(rgb(0x7209b7)));
        }
        Kind::Board => {
            // A glass tower, the board's table at its top, a red beam ready.
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 0.0, 10.0, a(rgb(0x2b2d42)));
            block(painter, iso, cx - 0.7, cy - 0.7, cx + 0.7, cy + 0.7, 10.0, h - 10.0, a(rgb(0x8d99ae)));
            for k in 0..5 {
                let v = 0.1 + k as f32 * 0.17;
                pane(painter, |u, v| left_face(iso, cx - 0.7, cy + 0.7, cx + 0.7, 10.0, h - 10.0, u, v), 0.15, v, 0.85, v + 0.09, a(rgb(0x2b2d42)));
            }
            cylinder(painter, iso, cx, cy, 0.8, h - 10.0, h, a(rgb(0xd90429)));
            let glow = ((now * 2.0).sin() as f32 * 0.5 + 0.5) * 60.0;
            painter.circle_filled(iso.up(iso.at(cx, cy), h + 4.0), 8.0 * iso.k(), Color32::from_rgba_unmultiplied(255, 60, 60, (80.0 + glow) as u8));
        }
        Kind::Bug | Kind::Printer | Kind::Aircon => {
            // Traps: hidden from attackers, a discreet mark for their owner.
            let c = match kind {
                Kind::Bug => rgb(0x9d4edd),
                Kind::Printer => rgb(0xe9ecef),
                _ => rgb(0x90e0ef),
            };
            block(painter, iso, x + 0.2, y + 0.2, x + 0.8, y + 0.8, 0.0, 4.0, a(alpha(c, 0.75)));
            let p = iso.up(iso.at(x + 0.5, y + 0.5), 6.0);
            let icon = match kind {
                Kind::Bug => "🐞",
                Kind::Printer => "⎙",
                _ => "❄",
            };
            painter.text(p, Align2::CENTER_CENTER, icon, FontId::proportional(11.0 * iso.k()), a(shade(c, 0.5)));
        }
        Kind::Training => {
            // A classroom: a whiteboard, a projector's beam.
            block(painter, iso, x0 + 0.2, y0 + 0.2, x1 - 0.2, y1 - 0.2, 0.0, h, a(rgb(0x4361ee)));
            let f = |u, v| left_face(iso, x0 + 0.2, y1 - 0.2, x1 - 0.2, 0.0, h, u, v);
            pane(painter, f, 0.15, 0.35, 0.85, 0.85, a(rgb(0xf8f9fa)));
            for k in 0..3 {
                let v = 0.5 + k as f32 * 0.1;
                pane(painter, f, 0.25, v, 0.25 + 0.15 * (k + 2) as f32, v + 0.04, a(rgb(0x4361ee)));
            }
            let top = iso.up(iso.at((x0 + x1) / 2.0, (y0 + y1) / 2.0), h);
            painter.text(top - Vec2::new(0.0, 8.0 * iso.k()), Align2::CENTER_CENTER, "✦", FontId::proportional(14.0 * iso.k()), a(rgb(0xffd166)));
        }
        Kind::Heroes => {
            // A corner office with a gold door, a crown on the roof.
            block(painter, iso, x0 + 0.1, y0 + 0.1, x1 - 0.1, y1 - 0.1, 0.0, h, a(rgb(0x7f5539)));
            pane(painter, |u, v| left_face(iso, x0 + 0.1, y1 - 0.1, x1 - 0.1, 0.0, h, u, v), 0.38, 0.0, 0.62, 0.55, a(rgb(0xe9c46a)));
            pane(painter, |u, v| right_face(iso, x1 - 0.1, y1 - 0.1, y0 + 0.1, 0.0, h, u, v), 0.2, 0.5, 0.8, 0.8, a(rgb(0xffe8a3)));
            let top = iso.up(iso.at((x0 + x1) / 2.0, (y0 + y1) / 2.0), h + 10.0);
            painter.text(top, Align2::CENTER_CENTER, "♛", FontId::proportional(18.0 * iso.k()), a(rgb(0xf2c14e)));
        }
        Kind::Plant | Kind::DeadPlant => {
            let dead = kind == Kind::DeadPlant;
            cylinder(painter, iso, x + 0.5, y + 0.5, 0.25, 0.0, 9.0, a(if dead { rgb(0x6c584c) } else { rgb(0xbc6c25) }));
            let leaf = iso.up(iso.at(x + 0.5, y + 0.5), 17.0);
            let (l1, l2) = if dead { (rgb(0x9c8b6e), rgb(0x7f7059)) } else { (rgb(0x2d6a4f), rgb(0x52b788)) };
            painter.circle_filled(leaf, 8.0 * iso.k(), a(l1));
            painter.circle_filled(leaf + Vec2::new(-3.0, -5.0) * iso.k(), 5.5 * iso.k(), a(l2));
            painter.circle_filled(leaf + Vec2::new(4.0, -3.0) * iso.k(), 4.5 * iso.k(), a(l2));
        }
        Kind::Fountain => {
            cylinder(painter, iso, x + 0.5, y + 0.5, 0.3, 0.0, 18.0, a(rgb(0xe9ecef)));
            cylinder(painter, iso, x + 0.5, y + 0.5, 0.22, 18.0, 30.0, a(alpha(rgb(0x48cae4), 0.85)));
            let bubble = ((now * 0.8 + b.id as f64) % 1.0) as f32;
            painter.circle_filled(iso.up(iso.at(x + 0.5, y + 0.5), 19.0 + bubble * 9.0), 1.5 * iso.k(), a(Color32::WHITE));
        }
        Kind::Foosball => {
            block(painter, iso, x + 0.2, y + 0.4, x + 1.8, y + 1.6, 0.0, 12.0, a(rgb(0x6f4518)));
            block(painter, iso, x + 0.3, y + 0.5, x + 1.7, y + 1.5, 12.0, 13.0, a(rgb(0x2d6a4f)));
            for k in 0..4 {
                let gx = x + 0.45 + k as f32 * 0.35;
                painter.line_segment([iso.up(iso.at(gx, y + 0.3), 15.0), iso.up(iso.at(gx, y + 1.7), 15.0)], Stroke::new(1.2 * iso.k(), a(rgb(0xadb5bd))));
            }
            let ball = ((now * 1.3).sin() as f32 * 0.5 + 0.5) * 1.0;
            painter.circle_filled(iso.up(iso.at(x + 0.5 + ball, y + 1.0), 14.0), 1.8 * iso.k(), a(Color32::WHITE));
        }
        Kind::Sofa | Kind::Chairs => {
            let c = if kind == Kind::Sofa { rgb(0x264653) } else { rgb(0x6c757d) };
            if kind == Kind::Sofa {
                block(painter, iso, x + 0.2, y + 0.4, x + 1.8, y + 1.6, 0.0, 7.0, a(c));
                block(painter, iso, x + 0.2, y + 0.4, x + 1.8, y + 0.75, 7.0, 14.0, a(shade(c, 1.2)));
                for k in 0..2 {
                    block(painter, iso, x + 0.3 + k as f32 * 0.75, y + 0.8, x + 0.95 + k as f32 * 0.75, y + 1.5, 7.0, 9.0, a(shade(c, 1.4)));
                }
            } else {
                // Tipped over, a leg missing.
                block(painter, iso, x + 0.3, y + 0.3, x + 0.9, y + 0.9, 0.0, 3.0, a(c));
                block(painter, iso, x + 0.3, y + 0.3, x + 0.45, y + 0.9, 3.0, 10.0, a(shade(c, 1.2)));
                block(painter, iso, x + 1.0, y + 1.0, x + 1.7, y + 1.6, 0.0, 6.0, a(shade(c, 0.85)));
                block(painter, iso, x + 1.0, y + 1.0, x + 1.7, y + 1.15, 6.0, 14.0, a(shade(c, 1.1)));
            }
        }
        Kind::Arcade => {
            block(painter, iso, x + 0.15, y + 0.2, x + 0.85, y + 0.8, 0.0, h, a(rgb(0x3c096c)));
            let f = |u, v| left_face(iso, x + 0.15, y + 0.8, x + 0.85, 0.0, h, u, v);
            let hue = ((now * 0.8) % 1.0) as f32;
            pane(painter, f, 0.12, 0.55, 0.88, 0.88, a(Color32::from_rgb((80.0 + 170.0 * hue) as u8, 60, (255.0 - 150.0 * hue) as u8)));
            pane(painter, f, 0.2, 0.32, 0.8, 0.45, a(rgb(0xff006e)));
        }
        Kind::Statue => {
            block(painter, iso, x + 0.2, y + 0.2, x + 1.8, y + 1.8, 0.0, 12.0, a(rgb(0xadb5bd)));
            let c = iso.at(x + 1.0, y + 1.0);
            let gold = a(rgb(0xe9c46a));
            cylinder(painter, iso, x + 1.0, y + 1.0, 0.3, 12.0, 34.0, gold);
            painter.circle_filled(iso.up(c, 40.0), 6.5 * iso.k(), gold);
            // An arm raised, pointing to the future.
            painter.line_segment([iso.up(c, 30.0), iso.up(c, 30.0) + Vec2::new(10.0, -12.0) * iso.k()], Stroke::new(3.0 * iso.k(), gold));
        }
        Kind::Boxes => {
            block(painter, iso, x + 0.2, y + 0.2, x + 1.0, y + 1.0, 0.0, 10.0, a(rgb(0xc8a165)));
            block(painter, iso, x + 1.0, y + 0.9, x + 1.8, y + 1.7, 0.0, 12.0, a(rgb(0xb98b4f)));
            block(painter, iso, x + 0.3, y + 0.3, x + 0.9, y + 0.9, 10.0, 18.0, a(rgb(0xd4b483)));
            let f = |u, v| left_face(iso, x + 1.0, y + 1.7, x + 1.8, 0.0, 12.0, u, v);
            pane(painter, f, 0.45, 0.0, 0.55, 1.0, a(rgb(0x8d6e4b)));
        }
        Kind::OldPc => {
            block(painter, iso, x + 0.15, y + 0.3, x + 0.85, y + 0.9, 0.0, 6.0, a(rgb(0xd6ccc2)));
            block(painter, iso, x + 0.2, y + 0.15, x + 0.8, y + 0.55, 6.0, 16.0, a(rgb(0xe3d5ca)));
            pane(painter, |u, v| left_face(iso, x + 0.2, y + 0.55, x + 0.8, 6.0, 16.0, u, v), 0.15, 0.2, 0.85, 0.85, a(rgb(0x1b263b)));
            // Dust.
            painter.circle_filled(iso.up(iso.at(x + 0.7, y + 0.7), 4.0), 2.0 * iso.k(), a(alpha(rgb(0x8d8d8d), 0.6)));
        }
    }

    // The level's trim, on the top edges.
    if let Some(t) = trim(level)
        && !matches!(kind.group(), os::Group::Traps | os::Group::Decor | os::Group::Obstacle)
        && !matches!(kind, Kind::Wall | Kind::Lounge | Kind::Interns)
    {
        let z = h;
        let (n, e, so, w) = (iso.at(x0, y0), iso.at(x1, y0), iso.at(x1, y1), iso.at(x0, y1));
        let st = Stroke::new(1.6 * iso.k(), a(t));
        painter.line_segment([iso.up(w, z), iso.up(so, z)], st);
        painter.line_segment([iso.up(so, z), iso.up(e, z)], st);
        let _ = n;
    }

    // Works: scaffolding, a bar and the time left.
    if let Some(job) = &b.job {
        let full = height(kind, b.level + 1);
        let st = Stroke::new(1.2 * iso.k(), a(rgb(0xf2a03d)));
        for z in [full * 0.33, full * 0.66, full] {
            let pts = diamond(iso, x, y, s, 0.05);
            let lifted: Vec<Pos2> = pts.iter().map(|p| iso.up(*p, z)).collect();
            painter.add(egui::Shape::closed_line(lifted, st));
        }
        for (gx, gy) in [(x + 0.05, y + s - 0.05), (x + s - 0.05, y + s - 0.05), (x + s - 0.05, y + 0.05)] {
            let p = iso.at(gx, gy);
            painter.line_segment([p, iso.up(p, full)], st);
        }
        let frac = b.progress(unix).unwrap_or(0.0) as f32;
        let top = iso.up(iso.at(x + s / 2.0, y + s / 2.0), full + 18.0);
        let w = (40.0 + 10.0 * s) * iso.k().max(0.7);
        let bar = Rect::from_center_size(top, Vec2::new(w, 7.0));
        painter.rect_filled(bar.expand(1.5), 4.0, Color32::from_black_alpha(160));
        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * frac, bar.height())), 3.0, rgb(0x50c878));
        painter.text(bar.center_top() - Vec2::new(0.0, 3.0), Align2::CENTER_BOTTOM, os::duration_text(job.until - unix, true).replace(" min", "m"), FontId::proportional(11.0), Color32::WHITE);
    }
}

/// The open space drawn in `rect`, the camera `view`; returns its `Iso` (to pick what's under the
/// pointer).
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(painter: &egui::Painter, rect: Rect, view: &View, s: &OpenSpace, overlay: &Overlay, scene: &mut Scene, theme: &Theme, now: f64, unix: i64) -> Iso {
    let iso = Iso::new(rect, view);
    let night = night();
    // The room around: a gradient, darker at night.
    let (top, bottom) = if theme.dark { (rgb(0x1b1e27), rgb(0x10121a)) } else { (rgb(0xdfe6f1), rgb(0xc3cdde)) };
    let mut bg = egui::Mesh::default();
    bg.colored_vertex(rect.left_top(), top);
    bg.colored_vertex(rect.right_top(), top);
    bg.colored_vertex(rect.right_bottom(), bottom);
    bg.colored_vertex(rect.left_bottom(), bottom);
    bg.add_triangle(0, 1, 2);
    bg.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(bg));
    paint_floor(painter, &iso, theme.dark);

    // The tiles a building being placed or moved would take.
    let footprint = |kind: Kind, x: i32, y: i32, ok: bool| {
        let pts = diamond(&iso, x as f32, y as f32, kind.size() as f32, 0.0);
        let c = if ok { Color32::from_rgba_unmultiplied(80, 220, 120, 90) } else { Color32::from_rgba_unmultiplied(240, 80, 80, 100) };
        painter.add(egui::Shape::convex_polygon(pts, c, Stroke::new(1.5, if ok { rgb(0x50dc78) } else { rgb(0xf05050) })));
    };
    if let Some((id, x, y, ok)) = overlay.moving
        && let Some(b) = s.get(id)
    {
        footprint(b.kind, x, y, ok);
    }
    if let Some((kind, x, y, ok)) = overlay.ghost {
        footprint(kind, x, y, ok);
    }
    // The selected one, outlined on the floor.
    if let Some(b) = overlay.selected.and_then(|id| s.get(id)) {
        let pulse = 0.6 + 0.4 * ((now * 3.0).sin() as f32 * 0.5 + 0.5);
        let pts = diamond(&iso, b.x as f32, b.y as f32, b.kind.size() as f32, -0.1);
        painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, Color32::from_white_alpha((200.0 * pulse) as u8))));
    }

    // Back to front.
    let walls: std::collections::HashSet<(i32, i32)> = s.buildings.iter().filter(|b| b.kind == Kind::Wall).map(|b| (b.x, b.y)).collect();
    let mut order: Vec<&Building> = s.buildings.iter().collect();
    order.sort_by_key(|b| (b.x + b.y + b.kind.size(), b.x));
    for b in order {
        let moved;
        let b = match overlay.moving {
            Some((id, x, y, _)) if id == b.id => {
                moved = Building { x, y, ..b.clone() };
                &moved
            }
            _ => b,
        };
        let ghost = if overlay.moving.is_some_and(|(id, ..)| id == b.id) { 0.7 } else { 1.0 };
        paint_building(painter, &iso, b, &walls, now, unix, night, ghost);
        if overlay.hover == Some(b.id) && overlay.ghost.is_none() {
            painter.add(egui::Shape::convex_polygon(outline(&iso, b), Color32::from_white_alpha(14), Stroke::NONE));
        }
    }
    // The building being placed, see-through.
    if let Some((kind, x, y, _)) = overlay.ghost {
        let b = Building { id: u32::MAX, kind, x, y, level: 1, job: None, stored: 0.0 };
        paint_building(painter, &iso, &b, &walls, now, unix, night, 0.55);
    }

    // The bubbles over the machines with something to collect.
    for b in s.buildings.iter().filter(|b| ripe(b)) {
        let Some(r) = b.kind.makes() else { continue };
        let p = bubble_at(&iso, b, now);
        let full = b.stored >= os::production(b.level).1 - 0.5;
        let rad = 13.0 * iso.k().clamp(0.7, 1.3);
        painter.circle_filled(p + Vec2::new(0.0, 2.0), rad, Color32::from_black_alpha(70));
        painter.circle_filled(p, rad, if full { rgb(0xfff1c9) } else { Color32::WHITE });
        painter.circle_stroke(p, rad, Stroke::new(1.5, res_color(r)));
        paint_res(painter, r, p, rad * 0.95);
    }

    // The levels gained: sparks, and a text.
    for b in &s.buildings {
        let was = scene.levels.insert(b.id, b.level);
        if let Some(was) = was
            && b.level > was
        {
            let s2 = b.kind.size() as f32 / 2.0;
            let at = (b.x as f32 + s2, b.y as f32 + s2);
            scene.bursts.push(Burst { at, since: now, gold: b.level >= 8 });
            scene.floats.push(Float { text: format!("★ {}", b.level), color: rgb(0xffd27a), at, since: now });
        }
    }
    scene.bursts.retain(|bu| now - bu.since < 1.2);
    for bu in &scene.bursts {
        let t = ((now - bu.since) / 1.2) as f32;
        let c = iso.up(iso.at(bu.at.0, bu.at.1), 30.0);
        for k in 0..14 {
            let angle = k as f32 / 14.0 * std::f32::consts::TAU + hash(k, 7);
            let d = (20.0 + 40.0 * t) * iso.k();
            let p = c + Vec2::angled(angle) * d + Vec2::new(0.0, 30.0 * t * t * iso.k());
            painter.circle_filled(p, 2.5 * (1.0 - t) * iso.k() + 0.5, alpha(if bu.gold { rgb(0xffd27a) } else { rgb(0xffffff) }, 1.0 - t));
        }
    }
    scene.floats.retain(|f| now - f.since < 1.6);
    for f in &scene.floats {
        let t = ((now - f.since) / 1.6) as f32;
        let p = iso.up(iso.at(f.at.0, f.at.1), 40.0 + 40.0 * t);
        let c = alpha(f.color, 1.0 - t * t);
        painter.text(p + Vec2::new(1.0, 1.0), Align2::CENTER_CENTER, &f.text, FontId::proportional(15.0), alpha(Color32::BLACK, (1.0 - t * t) * 0.6));
        painter.text(p, Align2::CENTER_CENTER, &f.text, FontId::proportional(15.0), c);
    }

    // The night falling on the room.
    if night > 0.0 {
        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(10, 16, 48, (night * 70.0) as u8));
    }
    iso
}

/// A building alone, small (the shop's cards): on a tile of floor, centered at `c`.
pub(super) fn paint_mini(painter: &egui::Painter, kind: Kind, c: Pos2, size: f32, now: f64) {
    let s = kind.size() as f32;
    let zoom = size / (60.0 * s.max(2.0)) * 1.4;
    let view = View { zoom, pan: Vec2::ZERO };
    let iso = Iso::new(Rect::from_center_size(c, Vec2::splat(size)), &view);
    // Centered on the building's middle, a little low (it stands up).
    let mid = iso.at(GRID as f32 / 2.0, GRID as f32 / 2.0);
    let want = c + Vec2::new(0.0, size * 0.18);
    let iso = Iso { origin: iso.origin + (want - mid), ..iso };
    let g = GRID as f32 / 2.0 - s / 2.0;
    let base = diamond(&iso, g - 0.3, g - 0.3, s + 0.6, 0.0);
    painter.add(egui::Shape::convex_polygon(base, Color32::from_black_alpha(40), Stroke::NONE));
    let b = Building { id: 7, kind, x: g as i32, y: g as i32, level: 1, job: None, stored: 0.0 };
    let walls = std::collections::HashSet::new();
    // Off the grid's integer tiles when s is odd: drawn from its float corner.
    let iso = Iso { origin: iso.origin + (iso.at(g, g) - iso.at(g.floor(), g.floor())), ..iso };
    paint_building(painter, &iso, &b, &walls, now, 0, 0.0, 1.0);
}

/// A troop's colors: its shirt, its accent.
pub(super) fn troop_colors(t: Troop) -> (Color32, Color32) {
    match t {
        Troop::Intern => (rgb(0x8ecae6), rgb(0xf2c14e)),
        Troop::Dev => (rgb(0x343a40), rgb(0x56b6ff)),
        Troop::Tester => (rgb(0xf77f00), rgb(0x6c757d)),
        Troop::Devops => (rgb(0x2a9d8f), rgb(0xe9c46a)),
        Troop::Sales => (rgb(0x1d3557), rgb(0xe63946)),
        Troop::Data => (rgb(0x7b2cbf), rgb(0xc77dff)),
        Troop::Designer => (rgb(0xff70a6), rgb(0xffffff)),
        Troop::Drone => (rgb(0x495057), rgb(0xff006e)),
        Troop::Consultant => (rgb(0x111111), rgb(0xd4af37)),
    }
}

/// A troop, standing at `p` (its feet), `k` its scale; `time` its walk.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_troop(painter: &egui::Painter, t: Troop, p: Pos2, k: f32, time: f32, moving: bool, hitting: bool, dir: (f32, f32)) {
    let (shirt, accent) = troop_colors(t);
    let size = match t {
        Troop::Sales => 1.3,
        Troop::Consultant => 1.7,
        Troop::Intern => 0.85,
        _ => 1.0,
    } * k;
    let st = t.stats();
    if t == Troop::Drone {
        let up = p - Vec2::new(0.0, 22.0 * k);
        painter.add(egui::Shape::ellipse_filled(p, Vec2::new(7.0, 3.5) * k, Color32::from_black_alpha(60)));
        painter.line_segment([up - Vec2::new(9.0, 3.0) * k, up + Vec2::new(9.0, 3.0) * k], Stroke::new(1.6 * k, shirt));
        painter.line_segment([up - Vec2::new(9.0, -3.0) * k, up + Vec2::new(9.0, -3.0) * k], Stroke::new(1.6 * k, shirt));
        for (dx, dy) in [(-9.0, -3.0), (9.0, 3.0), (-9.0, 3.0), (9.0, -3.0)] {
            let spin = (time * 40.0).sin().abs() * 4.0 + 1.0;
            painter.add(egui::Shape::ellipse_filled(up + Vec2::new(dx, dy) * k, Vec2::new(spin, 1.2) * k, Color32::from_white_alpha(120)));
        }
        painter.rect_filled(Rect::from_center_size(up + Vec2::new(0.0, 4.0 * k), Vec2::new(7.0, 6.0) * k), 1.0, rgb(0xc8a165));
        painter.circle_filled(up, 3.0 * k, accent);
        return;
    }
    let lift = if st.air { 18.0 * k } else { 0.0 };
    let bob = if moving { (time * 12.0).sin().abs() * 2.0 * k } else { 0.0 };
    painter.add(egui::Shape::ellipse_filled(p, Vec2::new(6.0, 3.0) * size, Color32::from_black_alpha(70)));
    let feet = p - Vec2::new(0.0, lift + bob);
    let swing = if moving { (time * 12.0).sin() * 2.0 * size } else { 0.0 };
    for side in [-1.0f32, 1.0] {
        painter.line_segment([feet + Vec2::new(side * 2.0 * size, -6.0 * size), feet + Vec2::new(side * 2.0 * size + side * swing, 0.0)], Stroke::new(2.2 * size, rgb(0x2b2d42)));
    }
    let body = Rect::from_center_size(feet - Vec2::new(0.0, 11.0 * size), Vec2::new(10.0, 11.0) * size);
    painter.rect_filled(body, 3.0 * size, shirt);
    let head = feet - Vec2::new(0.0, 20.0 * size);
    painter.circle_filled(head, 4.6 * size, rgb(0xf1c27d));
    let arm = body.center() + Vec2::new(dir.0, dir.1 * 0.5) * 7.0 * size;
    let jab = if hitting { ((time * 10.0).sin() * 0.5 + 0.5) * 3.0 * size } else { 0.0 };
    painter.line_segment([body.center(), arm + Vec2::new(dir.0, dir.1 * 0.5) * jab], Stroke::new(2.0 * size, shirt));
    match t {
        Troop::Intern => {
            painter.circle_filled(head - Vec2::new(0.0, 3.0 * size), 3.6 * size, accent);
        }
        Troop::Dev => {
            painter.rect_filled(Rect::from_center_size(arm, Vec2::new(7.0, 4.5) * size), 1.0, accent);
        }
        Troop::Tester => {
            painter.line_segment([arm, arm - Vec2::new(0.0, 8.0 * size)], Stroke::new(1.6 * size, rgb(0x8d6e4b)));
            painter.rect_filled(Rect::from_center_size(arm - Vec2::new(0.0, 8.0 * size), Vec2::new(6.0, 3.0) * size), 1.0, accent);
        }
        Troop::Devops => {
            painter.circle_stroke(arm, 2.6 * size, Stroke::new(1.5 * size, accent));
        }
        Troop::Sales => {
            painter.line_segment([body.center_top(), body.center_top() + Vec2::new(0.0, 6.0 * size)], Stroke::new(1.8 * size, accent));
        }
        Troop::Data => {
            painter.circle_filled(arm, 3.0 * size, accent);
        }
        Troop::Designer => {
            for side in [-1.0f32, 1.0] {
                let flap = (time * 10.0).sin() * 3.0;
                painter.add(egui::Shape::convex_polygon(vec![body.center(), body.center() + Vec2::new(side * 12.0, -6.0 - flap) * size, body.center() + Vec2::new(side * 9.0, 3.0) * size], alpha(accent, 0.8), Stroke::NONE));
            }
        }
        Troop::Consultant => {
            painter.rect_filled(Rect::from_center_size(arm + Vec2::new(0.0, 3.0 * size), Vec2::new(7.0, 5.0) * size), 1.0, accent);
        }
        Troop::Drone => {}
    }
}

enum Item<'a> {
    Def(&'a crate::openspace_battle::Def),
    Unit(&'a crate::openspace_battle::Unit),
}

/// A raid drawn: the rival open space (what's down as rubble, the hidden ones unseen), the troops,
/// the shots; the tiles where nothing can be dropped, tinted while picking.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_battle(painter: &egui::Painter, rect: Rect, view: &View, b: &Battle, theme: &Theme, now: f64, show_zone: bool, hover: Option<(f32, f32)>) -> Iso {
    let iso = Iso::new(rect, view);
    let (top, bottom) = if theme.dark { (rgb(0x1d1b26), rgb(0x0f0e16)) } else { (rgb(0xe8e0f0), rgb(0xcac0dc)) };
    let mut bg = egui::Mesh::default();
    bg.colored_vertex(rect.left_top(), top);
    bg.colored_vertex(rect.right_top(), top);
    bg.colored_vertex(rect.right_bottom(), bottom);
    bg.colored_vertex(rect.left_bottom(), bottom);
    bg.add_triangle(0, 1, 2);
    bg.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(bg));
    paint_floor(painter, &iso, theme.dark);
    if show_zone {
        let mut mesh = egui::Mesh::default();
        for x in 0..GRID {
            for y in 0..GRID {
                if b.can_drop(x as f32 + 0.5, y as f32 + 0.5) {
                    continue;
                }
                let base = mesh.vertices.len() as u32;
                for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                    mesh.colored_vertex(iso.at(x as f32 + dx, y as f32 + dy), Color32::from_rgba_unmultiplied(230, 60, 60, 38));
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    if let Some((x, y)) = hover {
        let ok = b.can_drop(x, y);
        painter.circle_stroke(iso.at(x, y), 10.0 * iso.k(), Stroke::new(2.0, if ok { rgb(0x50dc78) } else { rgb(0xf05050) }));
    }
    let time = b.time;
    let walls: std::collections::HashSet<(i32, i32)> = b.defs.iter().filter(|d| d.kind == Kind::Wall && d.alive()).map(|d| (d.x, d.y)).collect();
    // The buildings and the troops, back to front.
    let mut items: Vec<(f32, Item)> = Vec::new();
    for d in &b.defs {
        let s = d.kind.size() as f32;
        items.push(((d.x + d.y) as f32 + s * 1.2, Item::Def(d)));
    }
    for u in &b.units {
        items.push((u.x + u.y + 0.5 + if u.troop.stats().air { 6.0 } else { 0.0 }, Item::Unit(u)));
    }
    items.sort_by(|a, c| a.0.partial_cmp(&c.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, item) in &items {
        match item {
            Item::Def(d) => {
                let s = d.kind.size() as f32;
                if !d.alive() {
                    if d.kind != Kind::Wall && !d.is_trap() {
                        // Rubble.
                        for k in 0..(s as u32 * 3) {
                            let (hx, hy) = (hash(d.id, k), hash(k, d.id));
                            let (gx, gy) = (d.x as f32 + 0.2 + hx * (s - 0.6), d.y as f32 + 0.2 + hy * (s - 0.6));
                            block(painter, &iso, gx, gy, gx + 0.35, gy + 0.35, 0.0, 3.0 + hx * 5.0, rgb(0x5c5f6b));
                        }
                    }
                    continue;
                }
                if (d.is_trap() || d.kind == Kind::Honeypot) && !d.revealed {
                    continue;
                }
                let bld = Building { id: d.id, kind: d.kind, x: d.x, y: d.y, level: d.level, job: None, stored: 0.0 };
                paint_building(painter, &iso, &bld, &walls, now, 0, 0.0, 1.0);
                if d.hp < d.max && d.kind != Kind::Wall {
                    let top = iso.up(iso.at(d.x as f32 + s / 2.0, d.y as f32 + s / 2.0), height(d.kind, d.level) + 12.0);
                    let bar = Rect::from_center_size(top, Vec2::new(14.0 + 8.0 * s, 4.0));
                    painter.rect_filled(bar.expand(1.0), 2.0, Color32::from_black_alpha(160));
                    painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * d.hp / d.max, 4.0)), 2.0, rgb(0xf3722c));
                }
            }
            Item::Unit(u) => {
                let p = iso.at(u.x, u.y);
                paint_troop(painter, u.troop, p, iso.k().clamp(0.6, 1.6), time + u.id as f32 * 0.37, !u.hitting && u.stun <= 0.0, u.hitting, u.dir);
                if u.stun > 0.0 {
                    painter.text(p - Vec2::new(0.0, 30.0 * iso.k()), Align2::CENTER_CENTER, "✶", FontId::proportional(12.0), rgb(0xffd166));
                }
                if u.hp < u.max {
                    let air = if u.troop.stats().air { 18.0 } else { 0.0 };
                    let bar = Rect::from_center_size(p - Vec2::new(0.0, (30.0 + air) * iso.k()), Vec2::new(14.0, 3.0));
                    painter.rect_filled(bar.expand(1.0), 1.5, Color32::from_black_alpha(160));
                    painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * (u.hp / u.max).max(0.0), 3.0)), 1.5, rgb(0x57cc99));
                }
            }
        }
    }
    // The shots and the blasts.
    for f in &b.fx {
        let age = time - f.at;
        if age < 0.0 {
            continue;
        }
        match &f.kind {
            FxKind::Shot { from, to, air } => {
                let d = &b.defs[*from];
                let s = d.kind.size() as f32 / 2.0;
                let a = iso.up(iso.at(d.x as f32 + s, d.y as f32 + s), height(d.kind, d.level) * 0.85);
                let target = iso.up(iso.at(to.0, to.1), if *air { 28.0 } else { 12.0 });
                let fly = if d.kind == Kind::Alarm { 0.6 } else { 0.18 };
                if age > fly {
                    continue;
                }
                let t = age / fly;
                let mut p = a + (target - a) * t;
                if d.kind == Kind::Alarm {
                    p.y -= (t * std::f32::consts::PI).sin() * 60.0 * iso.k();
                }
                let c = match d.kind {
                    Kind::Board => rgb(0xff2a2a),
                    Kind::Cluster | Kind::Antivirus => rgb(0x9bf6ff),
                    Kind::Honeypot => rgb(0xbde0fe),
                    Kind::Alarm => rgb(0xff5d5d),
                    _ => rgb(0xfff3b0),
                };
                if matches!(d.kind, Kind::Board | Kind::Honeypot) {
                    painter.line_segment([a, target], Stroke::new(2.5 * iso.k(), alpha(c, 1.0 - t)));
                } else {
                    painter.circle_filled(p, 3.0 * iso.k(), c);
                    painter.line_segment([p, p - (target - a).normalized() * 8.0 * iso.k()], Stroke::new(1.5 * iso.k(), alpha(c, 0.6)));
                }
            }
            FxKind::Throw { from, to } => {
                if age > 0.25 {
                    continue;
                }
                let t = age / 0.25;
                let (a, z) = (iso.up(iso.at(from.0, from.1), 14.0), iso.up(iso.at(to.0, to.1), 14.0));
                painter.circle_filled(a + (z - a) * t, 2.5 * iso.k(), rgb(0x56b6ff));
            }
            FxKind::Boom { at, r } => {
                if age > 0.5 {
                    continue;
                }
                let t = age / 0.5;
                let c = iso.at(at.0, at.1);
                let rad = Vec2::new(r * iso.hw * 1.2, r * iso.hh * 1.2) * (0.4 + t);
                painter.add(egui::Shape::ellipse_filled(c, rad, Color32::from_rgba_unmultiplied(255, 160, 60, (160.0 * (1.0 - t)) as u8)));
                painter.add(egui::Shape::ellipse_filled(c, rad * 0.5, Color32::from_rgba_unmultiplied(255, 240, 180, (200.0 * (1.0 - t)) as u8)));
            }
            FxKind::Heal { at } => {
                if age > 0.8 {
                    continue;
                }
                let t = age / 0.8;
                let c = iso.at(at.0, at.1);
                painter.add(egui::Shape::ellipse_stroke(c, Vec2::new(2.0 * iso.hw, iso.hh * 2.0) * (0.5 + t * 0.5), Stroke::new(2.0, Color32::from_rgba_unmultiplied(110, 230, 140, (200.0 * (1.0 - t)) as u8))));
                painter.text(iso.up(c, 20.0 + 20.0 * t), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), Color32::from_rgba_unmultiplied(110, 230, 140, (255.0 * (1.0 - t)) as u8));
            }
            FxKind::Zap { at } => {
                if age > 0.4 {
                    continue;
                }
                let c = iso.up(iso.at(at.0, at.1), 10.0);
                for k in 0..6 {
                    let d = Vec2::angled(k as f32 + time * 30.0) * 16.0 * iso.k();
                    painter.line_segment([c, c + d], Stroke::new(1.5, rgb(0x9bf6ff)));
                }
            }
            FxKind::Freeze { at, r } => {
                if age > 1.2 {
                    continue;
                }
                let t = age / 1.2;
                let c = iso.at(at.0, at.1);
                painter.add(egui::Shape::ellipse_filled(c, Vec2::new(r * iso.hw * 1.4, r * iso.hh * 1.4), Color32::from_rgba_unmultiplied(160, 220, 255, (90.0 * (1.0 - t)) as u8)));
            }
        }
    }
    iso
}
