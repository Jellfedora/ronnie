//! Ronnie Corp's office, drawn: the floor seen from above, its back wall seen from the front (the
//! windows on the sky of the hour, the board of the contracts, the TV of the figures). Each one of the
//! team at their desk, typing, their screen of their task's color; now and then off for a break (the
//! coffee, the foosball, the nap room…) and back. Who's hired comes in by the door, who leaves goes out
//! by it. The money of a contract, a level gained, an outage, a new stage: shown where they happen.
//!
//! The people's comings and goings are the page's alone (not the company's: nothing of it is saved).
//! Everything is in coordinates of the floor (0 to 1 both ways), so that the window can be resized.

use std::collections::HashMap;

use super::*;
use crate::tycoon::{self as corp, money, tx, Corp, Employee, NewsKind, Task};

/// Where a click on the scene leads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Link {
    Team,
    Contracts,
    Products,
    Research,
    Premises,
}

#[derive(Default)]
pub(super) struct Scene {
    walkers: HashMap<u32, Walker>,
    /// Those gone, on their way out.
    ghosts: Vec<Walker>,
    floats: Vec<Float>,
    last_news: Option<corp::News>,
    confetti: f64,
    pub(super) selected: Option<u32>,
    /// Where the board of the contracts was drawn (the tutorial points at it).
    pub(super) board: Option<Rect>,
    /// The people put at their desks (the first time, nobody comes in by the door).
    seeded: bool,
    office: u8,
    rng: u64,
}

struct Walker {
    id: u32,
    pos: Vec2,
    path: Vec<Vec2>,
    /// Heading for (or at) their desk; else on a break.
    home: bool,
    wait_until: f64,
    next_break: f64,
    look: Look,
    bubble: Option<(&'static str, f64)>,
}

#[derive(Clone, Copy)]
struct Look {
    skin: Color32,
    hair: Color32,
    shirt: Color32,
}

struct Float {
    text: String,
    color: Color32,
    pos: Vec2,
    born: f64,
}

const SKINS: [u32; 6] = [0xf1c27d, 0xe0ac69, 0xc68642, 0x8d5524, 0xffdbac, 0xd1a27a];
const HAIRS: [u32; 7] = [0x2c1b10, 0x4a2c17, 0x8b5a2b, 0xd8b26e, 0x1a1a1a, 0xa33b20, 0x6b6b6b];

fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn job_color(job: &str, theme: &Theme) -> Color32 {
    match job {
        "founder" => theme.accent,
        "front" => rgb(0x4f8ef7),
        "back" => rgb(0x8e5cf7),
        "design" => rgb(0xf06ba8),
        "mobile" => rgb(0x2ec4b6),
        "qa" => rgb(0xf2c94c),
        "sales" => rgb(0xf2994a),
        "fullstack" => rgb(0x5b6ee1),
        "data" => rgb(0x27ae60),
        "devops" => rgb(0xb5523b),
        "lead" => rgb(0x34495e),
        "hr" => rgb(0xff7f6e),
        "office" => rgb(0x8d6e63),
        "assistant" => rgb(0x26a69a),
        "po" => rgb(0xec407a),
        _ => rgb(0x222222),
    }
}

fn task_color(task: Task, theme: &Theme) -> Color32 {
    match task {
        Task::Contracts => theme.accent,
        Task::Products => rgb(0x3ddc84),
        Task::Research => rgb(0xb388ff),
        Task::Refactor => rgb(0xffa040),
    }
}

fn look_of(e: &Employee, theme: &Theme) -> Look {
    let h = (e.id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    Look { skin: rgb(SKINS[(h % SKINS.len() as u64) as usize]), hair: rgb(HAIRS[((h >> 8) % HAIRS.len() as u64) as usize]), shirt: job_color(&e.job, theme) }
}

/// The floor's plan: the desks, the aisles, the lounge, the door.
struct Plan {
    floor: Rect,
    /// Each desk's seat (where its person sits), and its desk's rect, in floor units.
    seats: Vec<Vec2>,
    desk: Vec2,
    rows_aisle: Vec<f32>,
    /// The corridor between the desks and the lounge.
    spine: f32,
    door: Vec2,
    lounge: Rect,
}

impl Plan {
    fn new(floor: Rect, desks: usize) -> Self {
        let work = Rect::from_min_max(Pos2::new(0.04, 0.08), Pos2::new(0.63, 0.86));
        let (aw, ah) = (work.width() * floor.width(), work.height() * floor.height());
        let n = desks.max(1);
        let cols = ((n as f32 * aw / ah / 1.3).sqrt().ceil() as usize).clamp(1, n);
        let rows = n.div_ceil(cols);
        // Not giant in the garage.
        let cw = (aw / cols as f32).min(150.0);
        let ch = (ah / rows as f32).min(120.0);
        let (gw, gh) = (cw * cols as f32, ch * rows as f32);
        let x0 = work.min.x * floor.width() + (aw - gw) / 2.0;
        let y0 = work.min.y * floor.height() + (ah - gh) / 2.0;
        let mut seats = Vec::new();
        let mut rows_aisle = Vec::new();
        for r in 0..rows {
            rows_aisle.push((y0 + (r as f32 + 0.93) * ch) / floor.height());
            for c in 0..cols {
                if seats.len() < n {
                    seats.push(Vec2::new((x0 + (c as f32 + 0.5) * cw) / floor.width(), (y0 + (r as f32 + 0.62) * ch) / floor.height()));
                }
            }
        }
        let desk = Vec2::new(cw * 0.72 / floor.width(), ch * 0.3 / floor.height());
        Self { floor, seats, desk, rows_aisle, spine: 0.685, door: Vec2::new(0.685, 1.04), lounge: Rect::from_min_max(Pos2::new(0.73, 0.05), Pos2::new(0.985, 0.88)) }
    }

    fn at(&self, p: Vec2) -> Pos2 {
        self.floor.min + Vec2::new(p.x * self.floor.width(), p.y * self.floor.height())
    }

    fn back(&self, p: Pos2) -> Vec2 {
        Vec2::new((p.x - self.floor.min.x) / self.floor.width(), (p.y - self.floor.min.y) / self.floor.height())
    }

    /// The scale of things: a person's radius.
    fn person(&self) -> f32 {
        (self.desk.y * self.floor.height() * 0.55).clamp(5.0, 13.0)
    }

    fn aisle(&self, p: Vec2) -> f32 {
        if p.x >= self.spine - 0.01 {
            return p.y.min(0.97);
        }
        self.rows_aisle.iter().copied().min_by(|a, b| (a - p.y).abs().total_cmp(&(b - p.y).abs())).unwrap_or(0.95)
    }

    /// From `a` to `b` by the aisles and the corridor.
    fn route(&self, a: Vec2, b: Vec2) -> Vec<Vec2> {
        let (ya, yb) = (self.aisle(a), if b.y > 1.0 { 1.0 } else { self.aisle(b) });
        vec![Vec2::new(a.x, ya), Vec2::new(self.spine, ya), Vec2::new(self.spine, yb), Vec2::new(b.x, yb), b]
    }
}

impl Scene {
    fn rand(&mut self) -> f32 {
        if self.rng == 0 {
            self.rng = 0x1234_5678_9abc_def1;
        }
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    fn float(&mut self, text: String, color: Color32, pos: Vec2, now: f64) {
        self.floats.push(Float { text, color, pos, born: now });
    }
}

/// The hour of day (local), as a fraction of a day.
fn day_hour() -> f32 {
    use chrono::Timelike;
    let n = chrono::Local::now();
    n.hour() as f32 + n.minute() as f32 / 60.0
}

fn sky(hour: f32) -> (Color32, Color32) {
    let night = (rgb(0x0b1430), rgb(0x1d2b55));
    let day = (rgb(0x6fb7ff), rgb(0xcfe8ff));
    let dusk = (rgb(0x3b2a6b), rgb(0xf29e6b));
    match hour {
        h if !(6.0..21.0).contains(&h) => night,
        h if h < 8.0 => dusk,
        h if h >= 19.0 => dusk,
        _ => day,
    }
}

impl App {
    /// The office drawn in `rect`; where a click asks to go.
    pub(super) fn tycoon_scene(&mut self, ui: &mut Ui, rect: Rect, c: &mut Corp, fr: bool, theme: &Theme) -> Option<Link> {
        let now = ui.input(|i| i.time);
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        ui.ctx().request_repaint();
        let painter = ui.painter_at(rect);
        let mut link = None;

        let wall_h = (rect.height() * 0.2).clamp(60.0, 130.0);
        let wall = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + wall_h));
        let floor = Rect::from_min_max(Pos2::new(rect.min.x, wall.max.y), rect.max);
        let desks = c.office().capacity;
        let plan = Plan::new(floor, desks);
        let scene = &mut self.tycoon.scene;
        if scene.office != c.office {
            // Moved: everyone at their new desk.
            scene.office = c.office;
            scene.walkers.clear();
            scene.ghosts.clear();
            scene.seeded = false;
        }

        // What happened since the last frame.
        let fresh: Vec<corp::News> = match &scene.last_news {
            Some(last) => match c.news.iter().rposition(|n| n == last) {
                Some(k) => c.news[k + 1..].to_vec(),
                None => c.news.iter().rev().take(3).rev().cloned().collect(),
            },
            None => Vec::new(),
        };
        scene.last_news = c.news.last().cloned();

        // The people: one walker each, those at the office (the others work from home).
        let present: Vec<&Employee> = c.staff.iter().take(plan.seats.len()).collect();
        let ids: Vec<u32> = present.iter().map(|e| e.id).collect();
        let gone: Vec<u32> = scene.walkers.keys().copied().filter(|id| !c.staff.iter().any(|e| e.id == *id)).collect();
        for id in gone {
            if let Some(mut w) = scene.walkers.remove(&id) {
                w.path = plan.route(w.pos, plan.door);
                w.bubble = Some(("…", now + 3.0));
                scene.ghosts.push(w);
            }
        }
        // Moved to working from home: they leave too, quietly.
        let home_now: Vec<u32> = scene.walkers.keys().copied().filter(|id| !ids.contains(id)).collect();
        for id in home_now {
            if let Some(mut w) = scene.walkers.remove(&id) {
                w.path = plan.route(w.pos, plan.door);
                scene.ghosts.push(w);
            }
        }
        let seeded = scene.seeded;
        for (k, e) in present.iter().enumerate() {
            let seat = plan.seats[k];
            let next_break = now + 10.0 + 50.0 * scene.rand() as f64;
            scene.walkers.entry(e.id).or_insert_with(|| {
                let (pos, path) = if seeded { (plan.door, plan.route(plan.door, seat)) } else { (seat, Vec::new()) };
                Walker { id: e.id, pos, path, home: true, wait_until: 0.0, next_break, look: look_of(e, theme), bubble: None }
            });
        }
        scene.seeded = true;

        for n in &fresh {
            let center = Vec2::new(0.35, 0.45);
            match n.kind {
                NewsKind::Delivered | NewsKind::Late => scene.float(format!("+{}", money(n.n, fr)), if n.kind == NewsKind::Late { theme.ansi[3] } else { theme.ansi[2] }, Vec2::new(0.18, 0.04), now),
                NewsKind::LevelUp => {
                    let who = c.staff.iter().find(|e| if n.who.is_empty() { e.founder() } else { e.name == n.who }).map(|e| e.id);
                    let pos = who.and_then(|id| scene.walkers.get(&id)).map_or(center, |w| w.pos - Vec2::new(0.0, 0.05));
                    scene.float(format!("{} {} !", tx(("Niv.", "Lvl"), fr), n.n), theme.accent, pos, now);
                }
                NewsKind::Hired => scene.float(format!("{} {} !", tx(("Bienvenue", "Welcome"), fr), n.who), theme.text, plan.door - Vec2::new(0.0, 0.08), now),
                NewsKind::Quit | NewsKind::Fired | NewsKind::Unpaid => scene.float(format!("{} {}", tx(("Au revoir", "Bye"), fr), n.who), theme.text_muted, plan.door - Vec2::new(0.0, 0.08), now),
                NewsKind::Outage => scene.float(tx(("PANNE !", "OUTAGE!"), fr).to_owned(), theme.ansi[1], Vec2::new(0.6, 0.04), now),
                NewsKind::Tech => scene.float(format!("★ {}", tx(corp::TECHS[(n.n as usize).min(corp::TECHS.len() - 1)].name, fr)), rgb(0xb388ff), center, now),
                NewsKind::Launched | NewsKind::Version => scene.float(corp::news_text(n, fr), rgb(0x3ddc84), center, now),
                NewsKind::Stage => {
                    scene.confetti = now;
                    scene.float(corp::news_text(n, fr), theme.accent, center, now);
                }
                NewsKind::Event if n.cash.abs() >= 1.0 => scene.float(format!("{}{}", if n.cash > 0.0 { "+" } else { "" }, money(n.cash, fr)), if n.cash > 0.0 { theme.ansi[2] } else { theme.ansi[1] }, Vec2::new(0.35, 0.3), now),
                NewsKind::Perk => scene.float(format!("✦ {}", tx(corp::PERKS[(n.n as usize).min(corp::PERKS.len() - 1)].name, fr)), theme.accent, Vec2::new(0.86, 0.5), now),
                _ => {}
            }
        }

        // --- The wall, seen from the front.
        let hour = day_hour();
        let night = !(7.0..20.0).contains(&hour);
        let tier = c.office;
        let wall_color = match tier {
            0 => rgb(0x9a8f84),
            1 => rgb(0xe8dcc8),
            2 | 3 => rgb(0xdfe3e8),
            _ => rgb(0xcfd8dc),
        };
        painter.rect_filled(wall, 0.0, if theme.dark { wall_color.gamma_multiply(0.75) } else { wall_color });
        painter.line_segment([Pos2::new(wall.min.x, wall.max.y), Pos2::new(wall.max.x, wall.max.y)], Stroke::new(3.0, rgb(0x5d534a)));
        let (sky_top, sky_low) = sky(hour);
        let paint_window = |r: Rect| {
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(r.left_top(), sky_top);
            mesh.colored_vertex(r.right_top(), sky_top);
            mesh.colored_vertex(r.right_bottom(), sky_low);
            mesh.colored_vertex(r.left_bottom(), sky_low);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
            // Outside: houses low, then a city, higher with the floors.
            let k = (r.min.x * 7.3) as u32;
            let n = 5;
            for i in 0..n {
                let bw = r.width() / n as f32;
                let seed = (k + i * 31) % 17;
                let tall = match tier {
                    0 | 1 => 0.18 + seed as f32 / 17.0 * 0.15,
                    2 | 3 => 0.3 + seed as f32 / 17.0 * 0.35,
                    4 | 5 => 0.45 + seed as f32 / 17.0 * 0.45,
                    _ => 0.15 + seed as f32 / 17.0 * 0.2,
                };
                let b = Rect::from_min_max(Pos2::new(r.min.x + i as f32 * bw + 1.0, r.max.y - r.height() * tall), Pos2::new(r.min.x + (i + 1) as f32 * bw - 1.0, r.max.y));
                let color = if night { rgb(0x1b2238) } else { rgb(0x7f8fa6) };
                painter.rect_filled(b, 1.0, color);
                if night {
                    for j in 0..3 {
                        if !(seed + j).is_multiple_of(3) {
                            painter.rect_filled(Rect::from_center_size(Pos2::new(b.center().x + (j as f32 - 1.0) * 3.0, b.min.y + 4.0 + j as f32 * 4.0), Vec2::splat(1.6)), 0.0, rgb(0xffe08a));
                        }
                    }
                }
            }
            painter.rect_stroke(r, 2.0, Stroke::new(2.5, rgb(0xf5f5f5)), egui::StrokeKind::Middle);
            painter.line_segment([Pos2::new(r.center().x, r.min.y), Pos2::new(r.center().x, r.max.y)], Stroke::new(1.5, rgb(0xf5f5f5)));
        };
        // Laid out from the left: the board, the TV, the sign; then the windows (the garage's door)
        // and the clock, each in its own room.
        let wy = wall.min.y + wall_h * 0.16;
        let wh = wall_h * 0.62;
        let clock_room = if tier > 0 { 46.0 } else { 12.0 };
        let right_end = wall.max.x - clock_room;
        let mut x = wall.min.x + 14.0;
        let board = Rect::from_min_size(Pos2::new(x, wy), Vec2::new((wall.width() * 0.2).clamp(90.0, 220.0), wh));
        x = board.max.x + 16.0;
        let tv_w = (wall.width() * 0.14).clamp(80.0, 170.0);
        let show_tv = x + tv_w + 16.0 + 150.0 + 120.0 < right_end;
        let tv = Rect::from_min_size(Pos2::new(x, wy + wh * 0.06), Vec2::new(tv_w, wh * 0.82));
        if show_tv {
            x = tv.max.x + 16.0;
        }
        // The sign: the company's name, lit, and its stage under it.
        let stage = tx(corp::STAGES[c.stage as usize].name, fr).to_uppercase();
        let title_size = (wh * 0.3).clamp(11.0, 18.0);
        let stage_galley = painter.layout_no_wrap(stage, FontId::proportional((title_size * 0.62).max(9.0)), rgb(0xffe082));
        let sign_w = (title_size * 7.6).max(stage_galley.size().x) + 28.0;
        let sign = Rect::from_min_size(Pos2::new(x, wy + wh * 0.1), Vec2::new(sign_w, wh * 0.8));
        x = sign.max.x + 16.0;
        let windows = Rect::from_min_max(Pos2::new(x, wy), Pos2::new(right_end - 8.0, wy + wh));
        if windows.width() > 40.0 {
            if tier == 0 {
                // A small window, and the garage's door.
                let win_w = (windows.width() * 0.22).min(70.0);
                paint_window(Rect::from_min_size(windows.min, Vec2::new(win_w, wh * 0.7)));
                let door = Rect::from_min_max(Pos2::new(windows.min.x + win_w + 14.0, wy - 4.0), Pos2::new(windows.max.x, wall.max.y));
                painter.rect_filled(door, 2.0, rgb(0xb8b2a7));
                let mut y = door.min.y + 6.0;
                while y < door.max.y {
                    painter.line_segment([Pos2::new(door.min.x, y), Pos2::new(door.max.x, y)], Stroke::new(1.0, rgb(0x8a847a)));
                    y += 9.0;
                }
                painter.rect_filled(Rect::from_center_size(Pos2::new(door.center().x, door.max.y - 8.0), Vec2::new(26.0, 4.0)), 2.0, rgb(0x5d534a));
            } else {
                let n = ((windows.width() / 70.0) as usize).clamp(1, 2 + tier as usize);
                let gap = 10.0;
                let w = (windows.width() - gap * (n - 1) as f32) / n as f32;
                for k in 0..n {
                    paint_window(Rect::from_min_size(Pos2::new(windows.min.x + k as f32 * (w + gap), wy), Vec2::new(w, wh)));
                }
            }
        }
        let glow = if night { 1.0 } else { 0.55 };
        painter.rect_filled(sign.expand(3.0), 8.0, theme.accent.gamma_multiply(0.25 * glow));
        painter.rect_filled(sign, 6.0, rgb(0x1c1c22));
        painter.rect_stroke(sign, 6.0, Stroke::new(1.5, theme.accent.gamma_multiply(0.6 + 0.4 * glow)), egui::StrokeKind::Inside);
        for corner in [sign.left_top() + Vec2::new(5.0, 5.0), sign.right_top() + Vec2::new(-5.0, 5.0), sign.left_bottom() + Vec2::new(5.0, -5.0), sign.right_bottom() + Vec2::new(-5.0, -5.0)] {
            painter.circle_filled(corner, 1.5, rgb(0x777777));
        }
        let has_stage = sign.height() > title_size * 2.2;
        let title_y = if has_stage { sign.center().y - title_size * 0.38 } else { sign.center().y };
        paint_metal(&painter, Pos2::new(sign.center().x, title_y), Align2::CENTER_CENTER, "RONNIE CORP", title_size, theme.accent, 1.0);
        if has_stage {
            let size = stage_galley.size();
            painter.galley(Pos2::new(sign.center().x - size.x / 2.0, sign.center().y + title_size * 0.42), stage_galley, rgb(0xffe082));
        }

        // The board of the contracts: a post-it each, filled as it gets done.
        scene.board = Some(board);
        painter.rect_filled(board, 3.0, rgb(0xfafafa));
        painter.rect_stroke(board, 3.0, Stroke::new(2.0, rgb(0x9e9e9e)), egui::StrokeKind::Middle);
        let notes = c.contracts.len().min(8);
        let note = Vec2::new((board.width() - 12.0) / 4.0 - 4.0, (board.height() - 12.0) / 2.0 - 4.0);
        for (k, ct) in c.contracts.iter().take(notes).enumerate() {
            let r = Rect::from_min_size(board.min + Vec2::new(6.0 + (k % 4) as f32 * (note.x + 4.0), 6.0 + (k / 4) as f32 * (note.y + 4.0)), note);
            let color = if ct.left < 0.0 { rgb(0xff8a80) } else { [rgb(0xfff59d), rgb(0xa5d6a7), rgb(0x90caf9), rgb(0xf48fb1)][k % 4] };
            painter.rect_filled(r, 1.0, color);
            let done = ct.progress() as f32;
            painter.rect_filled(Rect::from_min_size(Pos2::new(r.min.x + 2.0, r.max.y - 5.0), Vec2::new((r.width() - 4.0) * done, 3.0)), 0.0, rgb(0x2e7d32));
            for l in 0..2 {
                painter.line_segment([r.min + Vec2::new(3.0, 5.0 + l as f32 * 4.0), r.min + Vec2::new(r.width() * (0.5 + 0.3 * l as f32), 5.0 + l as f32 * 4.0)], Stroke::new(1.0, rgb(0x555555)));
            }
        }
        if c.contracts.is_empty() {
            painter.text(board.center(), Align2::CENTER_CENTER, tx(("Aucun contrat", "No contract"), fr), FontId::proportional(11.0), rgb(0x777777));
        }
        let board_resp = ui.interact(board, ui.id().with("tc-board"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx(("Les contrats", "The contracts"), fr));
        if board_resp.clicked() {
            link = Some(Link::Contracts);
        }

        // The TV: the cash's curve, the products.
        if show_tv {
            painter.rect_filled(tv.expand(3.0), 4.0, rgb(0x212121));
            painter.rect_filled(tv, 2.0, rgb(0x0d1b2a));
            let down = c.products.iter().any(|p| p.down > 0.0);
            if down && (now * 3.0) as i64 % 2 == 0 {
                painter.rect_filled(tv, 2.0, theme.ansi[1].gamma_multiply(0.4));
            }
            if c.history.len() >= 2 {
                let (lo, hi) = c.history.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                let span = (hi - lo).max(1.0);
                let g = tv.shrink(5.0);
                let pts: Vec<Pos2> = c.history.iter().enumerate().map(|(k, &v)| Pos2::new(g.min.x + g.width() * k as f32 / (c.history.len() - 1) as f32, g.max.y - g.height() * 0.6 * (v - lo) / span)).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(1.5, rgb(0x3ddc84))));
            }
            painter.text(tv.min + Vec2::new(4.0, 3.0), Align2::LEFT_TOP, money(c.cash, fr), FontId::monospace(10.0), rgb(0xe0f7fa));
            for (k, p) in c.products.iter().take(2).enumerate() {
                let color = if p.down > 0.0 { theme.ansi[1] } else { rgb(0x80deea) };
                painter.text(Pos2::new(tv.min.x + 4.0, tv.min.y + 15.0 + k as f32 * 11.0), Align2::LEFT_TOP, format!("{} {}", tx(p.kind().name, fr), if p.version == 0 { "…".to_owned() } else { format!("{:.0}", p.users) }), FontId::monospace(9.0), color);
            }
            let tv_resp = ui.interact(tv, ui.id().with("tc-tv"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx(("Les produits", "The products"), fr));
            if tv_resp.clicked() {
                link = Some(Link::Products);
            }
        }
        // The clock.
        if tier > 0 {
            let clock = Pos2::new(wall.max.x - 24.0, wy + wh * 0.4);
            painter.circle_filled(clock, 13.0, rgb(0xfafafa));
            painter.circle_stroke(clock, 13.0, Stroke::new(2.0, rgb(0x424242)));
            let ang = |f: f32| f * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let (h, m) = (hour % 12.0 / 12.0, hour.fract());
            painter.line_segment([clock, clock + Vec2::angled(ang(h)) * 7.0], Stroke::new(2.0, rgb(0x212121)));
            painter.line_segment([clock, clock + Vec2::angled(ang(m)) * 10.0], Stroke::new(1.2, rgb(0x212121)));
        }

        // --- The floor.
        let (floor_color, line_color) = match tier {
            0 => (rgb(0x8e8e8e), rgb(0x7d7d7d)),
            1 => (rgb(0xc19a6b), rgb(0xa98257)),
            2 => (rgb(0x8fa3b0), rgb(0x8296a3)),
            3 => (rgb(0x7b8a9a), rgb(0x6f7e8d)),
            4 => (rgb(0x9aa5b1), rgb(0x8b97a3)),
            5 => (rgb(0xd7ccc8), rgb(0xc9bdb8)),
            _ => (rgb(0xeceff1), rgb(0xdde3e6)),
        };
        let floor_color = if theme.dark { floor_color.gamma_multiply(0.7) } else { floor_color };
        painter.rect_filled(floor, 0.0, floor_color);
        let step = if tier == 1 { 14.0 } else { 36.0 };
        let mut y = floor.min.y + step;
        while y < floor.max.y {
            painter.line_segment([Pos2::new(floor.min.x, y), Pos2::new(floor.max.x, y)], Stroke::new(1.0, line_color));
            y += step;
        }
        if tier != 1 {
            let mut x = floor.min.x + step;
            while x < floor.max.x {
                painter.line_segment([Pos2::new(x, floor.min.y), Pos2::new(x, floor.max.y)], Stroke::new(1.0, line_color));
                x += step;
            }
        }
        if tier == 0 {
            painter.circle_filled(plan.at(Vec2::new(0.6, 0.7)), 18.0, rgb(0x6f6f6f).gamma_multiply(0.6));
        }
        // The lounge: its own floor behind a glass partition, a place per slot of the premises.
        let lounge_px = Rect::from_min_max(plan.at(plan.lounge.min.to_vec2()), plan.at(plan.lounge.max.to_vec2()));
        let rug = if theme.dark { rgb(0x6d5a4a) } else { rgb(0xc8a27a) };
        painter.rect_filled(lounge_px.translate(Vec2::new(2.0, 3.0)), 10.0, Color32::from_black_alpha(35));
        painter.rect_filled(lounge_px, 10.0, rug);
        let mut y = lounge_px.min.y + 10.0;
        while y < lounge_px.max.y - 4.0 {
            painter.line_segment([Pos2::new(lounge_px.min.x + 4.0, y), Pos2::new(lounge_px.max.x - 4.0, y)], Stroke::new(1.0, rug.gamma_multiply(0.85)));
            y += 10.0;
        }
        // The glass partition, open at the bottom (the way in).
        let glass_x = lounge_px.min.x - 5.0;
        let glass_end = lounge_px.min.y + lounge_px.height() * 0.78;
        painter.line_segment([Pos2::new(glass_x, lounge_px.min.y), Pos2::new(glass_x, glass_end)], Stroke::new(4.0, rgb(0xb3e5fc).gamma_multiply(0.55)));
        let mut py = lounge_px.min.y;
        while py <= glass_end {
            painter.circle_filled(Pos2::new(glass_x, py), 2.5, rgb(0x90a4ae));
            py += (glass_end - lounge_px.min.y) / 4.0;
        }
        let slots = c.office().slots;
        painter.text(Pos2::new(lounge_px.min.x + 8.0, lounge_px.min.y + 5.0), Align2::LEFT_TOP, format!("{} · {}/{}", tx(("Coin détente", "Lounge"), fr), c.perks.len(), slots), FontId::proportional(10.5), rgb(0xfff8e1));
        let mut spots: Vec<Vec2> = Vec::new();
        let any_down = c.products.iter().any(|p| p.down > 0.0);
        let cols = if lounge_px.width() > 230.0 { 3 } else if lounge_px.width() > 120.0 { 2 } else { 1 };
        let rows = slots.div_ceil(cols).max(1);
        let area = Rect::from_min_max(lounge_px.min + Vec2::new(6.0, 20.0), lounge_px.max - Vec2::new(6.0, 6.0));
        let cell = Vec2::new(area.width() / cols as f32, (area.height() / rows as f32).min(area.width() / cols as f32 * 1.1));
        for k in 0..slots {
            let at = area.min + Vec2::new((k % cols) as f32 * cell.x, (k / cols) as f32 * cell.y);
            let slot = Rect::from_min_size(at, cell).shrink(4.0);
            let label_h = if slot.height() > 46.0 { 12.0 } else { 0.0 };
            let item = Rect::from_min_max(slot.min, slot.max - Vec2::new(0.0, label_h));
            match c.perks.get(k).and_then(|id| corp::perk(id)) {
                Some(p) => {
                    painter.add(egui::Shape::ellipse_filled(Pos2::new(item.center().x, item.max.y - 2.0), Vec2::new(item.width() * 0.42, 4.0), Color32::from_black_alpha(45)));
                    paint_perk(&painter, p.id, item.shrink(2.0), now, any_down, theme);
                    if label_h > 0.0 {
                        let galley = painter.layout(tx(p.name, fr).to_owned(), FontId::proportional(9.5), rgb(0xfff8e1), slot.width());
                        painter.galley(Pos2::new(slot.center().x - galley.size().x / 2.0, slot.max.y - label_h + 1.0), galley, rgb(0xfff8e1));
                    }
                    if !matches!(p.id, "servers" | "plants" | "screens" | "fiber" | "chairs") {
                        spots.push(plan.back(Pos2::new(item.center().x, item.max.y - 3.0)));
                    }
                    let resp = ui.interact(slot, ui.id().with(("tc-perk", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{}\n{}", tx(p.name, fr), tx(p.about, fr)));
                    if resp.clicked() {
                        link = Some(if matches!(p.id, "servers" | "library") { Link::Research } else { Link::Premises });
                    }
                }
                None => {
                    let hot = ui.rect_contains_pointer(slot);
                    let stroke = Stroke::new(1.2, rgb(0xfff8e1).gamma_multiply(if hot { 0.9 } else { 0.45 }));
                    let dash = 5.0;
                    for (a, b) in [(slot.left_top(), slot.right_top()), (slot.right_top(), slot.right_bottom()), (slot.right_bottom(), slot.left_bottom()), (slot.left_bottom(), slot.left_top())] {
                        let len = a.distance(b);
                        let mut d = 0.0;
                        while d < len {
                            let e = (d + dash).min(len);
                            painter.line_segment([a + (b - a) * (d / len), a + (b - a) * (e / len)], stroke);
                            d += dash * 2.0;
                        }
                    }
                    painter.text(slot.center(), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), stroke.color);
                    let resp = ui.interact(slot, ui.id().with(("tc-slot", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx(("Emplacement libre : installer un aménagement", "Free slot: install a fitting"), fr));
                    if resp.clicked() {
                        link = Some(Link::Premises);
                    }
                }
            }
        }
        let door_px = Rect::from_center_size(Pos2::new(plan.at(Vec2::new(plan.spine, 0.0)).x, floor.max.y - 3.0), Vec2::new(48.0, 6.0));
        painter.rect_filled(door_px, 1.0, rgb(0x5d4037));
        let door_resp = ui.interact(door_px.expand2(Vec2::new(4.0, 14.0)), ui.id().with("tc-door"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx(("Les locaux", "Premises"), fr));
        if door_resp.clicked() {
            link = Some(Link::Premises);
        }
        if night {
            painter.rect_filled(floor, 0.0, Color32::from_black_alpha(45));
            painter.rect_filled(wall, 0.0, Color32::from_black_alpha(35));
        }
        // Somewhere to stand on a break, even with nothing: by a window, by the door.
        spots.push(Vec2::new(0.5, 0.04));
        spots.push(Vec2::new(plan.spine + 0.04, 0.95));

        // --- The desks.
        let r = plan.person();
        let screens = c.has_perk("screens");
        let chair_color = if c.has_perk("chairs") { rgb(0x1e88e5) } else { rgb(0x424242) };
        let desk_color = match tier {
            0 => rgb(0x8d6e63),
            1 | 2 => rgb(0xbcaaa4),
            _ => rgb(0xf5f5f5),
        };
        let mut hover_empty = None;
        for (k, seat) in plan.seats.iter().enumerate() {
            let p = plan.at(*seat);
            let d = Vec2::new(plan.desk.x * floor.width(), plan.desk.y * floor.height());
            let desk = Rect::from_center_size(p - Vec2::new(0.0, d.y * 0.95 + r * 0.4), d);
            painter.rect_filled(desk.translate(Vec2::new(2.0, 2.0)), 3.0, Color32::from_black_alpha(40));
            painter.rect_filled(desk, 3.0, desk_color);
            painter.rect_stroke(desk, 3.0, Stroke::new(1.0, desk_color.gamma_multiply(0.7)), egui::StrokeKind::Inside);
            painter.circle_filled(p + Vec2::new(0.0, r * 0.4), r * 0.95, chair_color);
            let worker = present.get(k);
            let at_desk = worker.and_then(|e| scene.walkers.get(&e.id)).is_some_and(|w| w.home && w.path.is_empty());
            let n_mon = if screens { 2 } else { 1 };
            let mw = (d.x * 0.32).min(34.0);
            for m in 0..n_mon {
                let off = if n_mon == 2 { (m as f32 - 0.5) * (mw + 3.0) } else { 0.0 };
                let mon = Rect::from_center_size(Pos2::new(desk.center().x + off, desk.min.y + d.y * 0.32), Vec2::new(mw, d.y * 0.38));
                painter.rect_filled(mon, 1.5, rgb(0x212121));
                if let (Some(e), true) = (worker, at_desk) {
                    let glow = task_color(e.task, theme);
                    painter.rect_filled(mon.shrink(1.5), 1.0, glow.gamma_multiply(0.35));
                    // The code going by.
                    let rows = 3;
                    for l in 0..rows {
                        let phase = (now * 2.0 + e.id as f64 * 0.7 + l as f64 * 1.3 + m as f64) % 4.0;
                        let len = (0.3 + 0.6 * ((phase * 1.7).sin().abs())) as f32;
                        let y = mon.min.y + 2.5 + l as f32 * (mon.height() - 4.0) / rows as f32;
                        painter.line_segment([Pos2::new(mon.min.x + 2.5, y), Pos2::new(mon.min.x + 2.5 + (mon.width() - 5.0) * len, y)], Stroke::new(1.0, glow));
                    }
                }
            }
            // A mug, here and there.
            if (k * 7) % 3 == 0 {
                painter.circle_filled(Pos2::new(desk.max.x - 6.0, desk.center().y + 2.0), 2.6, rgb(0xeeeeee));
            }
            if worker.is_none() {
                let hot = ui.rect_contains_pointer(desk.union(Rect::from_center_size(p, Vec2::splat(r * 2.0))));
                if hot {
                    hover_empty = Some(desk);
                }
            }
        }
        if let Some(desk) = hover_empty {
            painter.text(desk.center(), Align2::CENTER_CENTER, "+", FontId::proportional(18.0), theme.accent);
            let resp = ui.interact(desk, ui.id().with("tc-empty"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx(("Place libre : embaucher", "Free seat: hire"), fr));
            if resp.clicked() {
                link = Some(Link::Team);
            }
        }

        // --- The people move.
        let speed = (r * 9.0).max(55.0);
        let crunch = c.policy.hours == 2;
        let breakers = scene.walkers.values().filter(|w| !w.home).count();
        let room_for_break = breakers * 3 < scene.walkers.len().max(1);
        let morale: HashMap<u32, f64> = c.staff.iter().map(|e| (e.id, e.morale)).collect();
        let seat_of: HashMap<u32, Vec2> = present.iter().enumerate().map(|(k, e)| (e.id, plan.seats[k])).collect();
        let mut rolls: Vec<f32> = (0..scene.walkers.len() * 3 + 2).map(|_| scene.rand()).collect();
        let mut started_break = false;
        for w in scene.walkers.values_mut() {
            let Some(seat) = seat_of.get(&w.id).copied() else { continue };
            step_walker(w, &plan, speed, dt);
            if !w.path.is_empty() {
                continue;
            }
            let roll = rolls.pop().unwrap_or(0.5);
            let pick = rolls.pop().unwrap_or(0.5);
            let m = morale.get(&w.id).copied().unwrap_or(60.0);
            if w.home {
                // Moved desks (someone left before them): to the new one.
                if (w.pos - seat).length() > 0.005 {
                    w.path = plan.route(w.pos, seat);
                    continue;
                }
                if now >= w.next_break && room_for_break && !started_break && !crunch {
                    let spot = spots[((pick * spots.len() as f32) as usize).min(spots.len() - 1)];
                    w.path = plan.route(w.pos, spot);
                    w.home = false;
                    w.wait_until = 0.0;
                    started_break = true;
                } else if now >= w.next_break {
                    w.next_break = now + 15.0 + 30.0 * roll as f64;
                }
                // Now and then a word of their mood.
                if w.bubble.is_none_or(|(_, until)| now > until + 6.0) && roll < 0.004 {
                    let word = if m < 25.0 { "#@!" } else if m < 45.0 { "…" } else if crunch && m < 60.0 { "zzz" } else if m > 85.0 { "♥" } else { "" };
                    if !word.is_empty() {
                        w.bubble = Some((word, now + 2.5));
                    }
                }
            } else if w.wait_until == 0.0 {
                w.wait_until = now + 4.0 + 6.0 * roll as f64;
            } else if now >= w.wait_until {
                w.path = plan.route(w.pos, seat);
                w.home = true;
                // Low morale: more breaks.
                let gap = if m < 35.0 { 15.0 } else { 30.0 };
                w.next_break = now + gap + 50.0 * pick as f64;
            }
        }
        for g in &mut scene.ghosts {
            step_walker(g, &plan, speed, dt);
        }
        scene.ghosts.retain(|g| !g.path.is_empty());

        // Drawn from the back to the front.
        let event_on = c.event.is_some();
        let mut order: Vec<(&Walker, bool)> = scene.walkers.values().map(|w| (w, false)).chain(scene.ghosts.iter().map(|w| (w, true))).collect();
        order.sort_by(|a, b| a.0.pos.y.total_cmp(&b.0.pos.y));
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut hovered: Option<u32> = None;
        for (w, ghost) in &order {
            let p = plan.at(w.pos);
            let seated = w.home && w.path.is_empty();
            let walking = !w.path.is_empty();
            let founder = c.staff.iter().any(|e| e.id == w.id && e.founder());
            paint_person(&painter, p, r, w.look, seated, walking, founder, now + w.id as f64, if *ghost { 0.6 } else { 1.0 });
            if scene.selected == Some(w.id) {
                painter.circle_stroke(p, r * 1.6, Stroke::new(2.0, theme.accent));
            }
            let bubble = if founder && event_on { Some(("!", now + 1.0)) } else { w.bubble };
            if let Some((word, until)) = bubble.filter(|(_, until)| now < *until) {
                let _ = until;
                let bp = p - Vec2::new(-r * 0.8, r * 2.4);
                let pulse = if word == "!" { 1.0 + 0.15 * (now * 6.0).sin() as f32 } else { 1.0 };
                let galley = painter.layout_no_wrap(word.to_owned(), FontId::proportional(11.0 * pulse), if word == "!" { rgb(0xffffff) } else { rgb(0x212121) });
                let br = Rect::from_center_size(bp, galley.size() + Vec2::new(10.0, 6.0));
                painter.rect_filled(br, 7.0, if word == "!" { theme.ansi[1] } else { rgb(0xffffff) });
                painter.galley(br.center() - galley.size() / 2.0, galley, rgb(0x212121));
            }
            if !ghost && pointer.is_some_and(|q| q.distance(p) < r * 1.3) {
                hovered = Some(w.id);
            }
        }
        if let Some(id) = hovered
            && let Some(e) = c.staff.iter().find(|e| e.id == id)
        {
            let w = &scene.walkers[&id];
            let area = Rect::from_center_size(plan.at(w.pos), Vec2::splat(r * 2.6));
            let name = if e.founder() { tx(("Toi", "You"), fr).to_owned() } else { e.name.clone() };
            let task = corp::TASKS.iter().find(|(t, _)| *t == e.task).map_or("", |(_, l)| tx(*l, fr));
            let resp = ui.interact(area, ui.id().with(("tc-person", id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!(
                "{name}\n{} · {} {}\n{} {:.0} %{}",
                tx(e.job().name, fr),
                tx(("niv.", "lvl"), fr),
                e.level,
                tx(("Moral", "Morale"), fr),
                e.morale,
                if e.makes() { format!("\n{task}") } else { String::new() }
            ));
            if resp.clicked() {
                scene.selected = if scene.selected == Some(id) { None } else { Some(id) };
            }
        }

        // Those working from home.
        let remote = c.staff.len().saturating_sub(plan.seats.len());
        if remote > 0 {
            painter.text(Pos2::new(floor.min.x + 10.0, floor.max.y - 8.0), Align2::LEFT_BOTTOM, format!("⌂ {remote} {}", tx(("en télétravail", "working from home"), fr)), FontId::proportional(12.0), theme.text);
        }

        // What happened, rising.
        scene.floats.retain(|f| now - f.born < 2.6);
        for (k, f) in scene.floats.iter().enumerate() {
            let age = (now - f.born) as f32;
            let p = plan.at(f.pos) - Vec2::new(0.0, age * 22.0 + k as f32 * 2.0);
            let alpha = (1.0 - age / 2.6).clamp(0.0, 1.0);
            let galley = painter.layout_no_wrap(f.text.clone(), FontId::proportional(14.0), f.color.gamma_multiply(alpha));
            let at = Pos2::new((p.x - galley.size().x / 2.0).clamp(rect.min.x + 4.0, rect.max.x - galley.size().x - 4.0), p.y.max(rect.min.y + 2.0));
            painter.galley(at + Vec2::new(1.0, 1.0), painter.layout_no_wrap(f.text.clone(), FontId::proportional(14.0), Color32::from_black_alpha((alpha * 160.0) as u8)), Color32::BLACK);
            painter.galley(at, galley, f.color);
        }
        // A new stage: confetti.
        let since = (now - scene.confetti) as f32;
        if scene.confetti > 0.0 && since < 4.0 {
            for k in 0..80 {
                let x = rect.min.x + rect.width() * ((k * 37 % 100) as f32 / 100.0) + (since * 2.0 + k as f32).sin() * 12.0;
                let y = rect.min.y + (since * (90.0 + (k * 13 % 60) as f32)) - (k * 23 % 120) as f32;
                if y > rect.min.y && y < rect.max.y {
                    let color = [theme.accent, theme.ansi[2], theme.ansi[3], theme.ansi[5], theme.ansi[4]][k % 5];
                    painter.rect_filled(Rect::from_center_size(Pos2::new(x, y), Vec2::new(4.0, 6.0)), 1.0, color);
                }
            }
        }
        painter.rect_stroke(rect, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        if !c.staff.iter().any(|e| Some(e.id) == scene.selected) {
            scene.selected = None;
        }
        link
    }
}

fn step_walker(w: &mut Walker, plan: &Plan, speed: f32, dt: f32) {
    let Some(target) = w.path.first().copied() else { return };
    let (p, q) = (plan.at(w.pos), plan.at(target));
    let d = q - p;
    let step = speed * dt;
    if d.length() <= step {
        w.pos = target;
        w.path.remove(0);
    } else {
        w.pos = plan.back(p + d.normalized() * step);
    }
}

/// A person seen from above: shoulders, head, hair; typing at their desk, walking elsewhere.
#[allow(clippy::too_many_arguments)]
fn paint_person(painter: &egui::Painter, p: Pos2, r: f32, look: Look, seated: bool, walking: bool, founder: bool, t: f64, alpha: f32) {
    let a = |c: Color32| c.gamma_multiply(alpha);
    painter.add(egui::Shape::ellipse_filled(p + Vec2::new(0.0, r * 0.9), Vec2::new(r * 1.1, r * 0.4), Color32::from_black_alpha((50.0 * alpha) as u8)));
    if walking {
        let swing = (t * 10.0).sin() as f32 * r * 0.35;
        painter.circle_filled(p + Vec2::new(-r * 0.35, r * 0.7 + swing), r * 0.25, a(rgb(0x37474f)));
        painter.circle_filled(p + Vec2::new(r * 0.35, r * 0.7 - swing), r * 0.25, a(rgb(0x37474f)));
    }
    let bob = if walking { (t * 10.0).sin().abs() as f32 * r * 0.08 } else { 0.0 };
    painter.add(egui::Shape::ellipse_filled(p + Vec2::new(0.0, r * 0.25 - bob), Vec2::new(r * 1.05, r * 0.62), a(look.shirt)));
    if seated {
        // Typing.
        for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            let tap = ((t * 9.0 + k as f64 * 1.6).sin().max(0.0)) as f32 * r * 0.18;
            painter.circle_filled(p + Vec2::new(side * r * 0.55, -r * 0.75 - tap), r * 0.22, a(look.skin));
        }
    }
    let head = p - Vec2::new(0.0, r * 0.15 + bob);
    painter.circle_filled(head, r * 0.62, a(look.skin));
    painter.circle_filled(head - Vec2::new(0.0, r * 0.14), r * 0.52, a(look.hair));
    if founder {
        let c = head - Vec2::new(0.0, r * 0.95);
        let w = r * 0.5;
        let crown = vec![c + Vec2::new(-w, w * 0.5), c + Vec2::new(-w, -w * 0.3), c + Vec2::new(-w * 0.5, w * 0.1), c + Vec2::new(0.0, -w * 0.5), c + Vec2::new(w * 0.5, w * 0.1), c + Vec2::new(w, -w * 0.3), c + Vec2::new(w, w * 0.5)];
        painter.add(egui::Shape::convex_polygon(crown, a(rgb(0xffc107)), Stroke::NONE));
    }
}

/// A fitting of the lounge, in `r` (a bit of a front view, for each one to be recognised).
fn paint_perk(painter: &egui::Painter, id: &str, r: Rect, t: f64, down: bool, theme: &Theme) {
    let (w, h) = (r.width(), r.height());
    let s = w.min(h);
    // (-1, -1) the top left of `r`, (1, 1) its bottom right.
    let at = |x: f32, y: f32| Pos2::new(r.center().x + x * w / 2.0, r.center().y + y * h / 2.0);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let line = |a: Pos2, b: Pos2, width: f32, color: Color32| painter.line_segment([a, b], Stroke::new(width, color));
    let steam = |p: Pos2, k: f64| {
        for j in 0..2 {
            let phase = ((t * 0.9 + k + j as f64 * 0.5) % 1.0) as f32;
            let q = p - Vec2::new(0.0, phase * s * 0.25);
            painter.circle_filled(q + Vec2::new((phase * 9.0).sin() * 2.0, 0.0), 1.5 + phase * 2.0, Color32::from_white_alpha(((1.0 - phase) * 140.0) as u8));
        }
    };
    match id {
        "plants" => {
            for (x, big) in [(-0.45f32, 1.0f32), (0.45, 0.75)] {
                let pot_top = 0.25 + (1.0 - big) * 0.3;
                let pot = vec![at(x - 0.3 * big, pot_top), at(x + 0.3 * big, pot_top), at(x + 0.22 * big, 0.9), at(x - 0.22 * big, 0.9)];
                painter.add(egui::Shape::convex_polygon(pot, rgb(0xc1603a), Stroke::new(1.0, rgb(0x8d3f22))));
                painter.rect_filled(rect(x - 0.34 * big, pot_top - 0.08, x + 0.34 * big, pot_top + 0.04), 2.0, rgb(0xd77a52));
                let sway = (t as f32 * 0.8 + x).sin() * 0.03;
                for (k, (dx, dy, rad)) in [(-0.2, -0.25, 0.22), (0.18, -0.35, 0.24), (0.0, -0.6, 0.26), (-0.15, -0.85, 0.18), (0.22, -0.75, 0.2)].into_iter().enumerate() {
                    let leaf = at(x + (dx + sway) * big, pot_top - 0.1 + (dy - 0.05) * big);
                    painter.circle_filled(leaf, rad * big * s * 0.5, [rgb(0x2e7d32), rgb(0x388e3c), rgb(0x43a047)][k % 3]);
                    painter.circle_filled(leaf - Vec2::splat(rad * big * s * 0.15), rad * big * s * 0.16, rgb(0x81c784));
                }
            }
        }
        "coffee" => {
            painter.rect_filled(rect(-1.0, 0.35, 1.0, 0.95), 3.0, rgb(0x8d6e63));
            painter.rect_filled(rect(-1.0, 0.35, 1.0, 0.45), 2.0, rgb(0xa1887f));
            let machine = rect(-0.55, -0.85, 0.25, 0.38);
            painter.rect_filled(machine, 5.0, rgb(0x37474f));
            painter.rect_filled(rect(-0.45, -0.75, 0.15, -0.45), 2.0, rgb(0x263238));
            let blink = (t * 2.0) as i64 % 2 == 0;
            painter.rect_filled(rect(-0.4, -0.7, 0.1, -0.5), 1.0, if blink { rgb(0x4fc3f7) } else { rgb(0x29b6f6) });
            painter.rect_filled(rect(-0.25, -0.3, -0.05, -0.15), 1.0, rgb(0x90a4ae));
            painter.circle_filled(at(0.15, -0.75), 2.0, rgb(0xff5252));
            let cup = rect(-0.27, 0.05, -0.03, 0.32);
            painter.rect_filled(cup, 2.0, rgb(0xfafafa));
            painter.rect_filled(rect(-0.25, 0.05, -0.05, 0.1), 0.0, rgb(0x6d4c41));
            steam(at(-0.15, 0.0), 0.0);
            for (k, x) in [0.45f32, 0.75].into_iter().enumerate() {
                painter.rect_filled(rect(x - 0.1, 0.15, x + 0.1, 0.38), 2.0, [rgb(0xef5350), rgb(0x42a5f5)][k]);
                painter.circle_stroke(at(x + 0.14, 0.26), s * 0.04, Stroke::new(1.2, [rgb(0xef5350), rgb(0x42a5f5)][k]));
            }
        }
        "babyfoot" => {
            let frame = rect(-0.85, -0.7, 0.85, 0.7);
            painter.rect_filled(frame, 4.0, rgb(0x6d4c41));
            let pitch = frame.shrink(s * 0.06);
            painter.rect_filled(pitch, 2.0, rgb(0x2e7d32));
            line(Pos2::new(pitch.center().x, pitch.min.y), Pos2::new(pitch.center().x, pitch.max.y), 1.0, rgb(0xe8f5e9));
            painter.circle_stroke(pitch.center(), s * 0.1, Stroke::new(1.0, rgb(0xe8f5e9)));
            for side in [-1.0f32, 1.0] {
                painter.rect_filled(Rect::from_center_size(Pos2::new(if side < 0.0 { pitch.min.x } else { pitch.max.x }, pitch.center().y), Vec2::new(3.0, pitch.height() * 0.35)), 0.0, rgb(0xfafafa));
            }
            for k in 0..4 {
                let x = pitch.min.x + pitch.width() * (k as f32 + 0.5) / 4.0;
                let color = if k % 2 == 0 { rgb(0xe53935) } else { rgb(0x1e88e5) };
                line(Pos2::new(x, frame.min.y - s * 0.12), Pos2::new(x, frame.max.y + s * 0.12), 1.4, rgb(0xcfd8dc));
                painter.circle_filled(Pos2::new(x, frame.max.y + s * 0.14), s * 0.045, rgb(0x212121));
                let shift = ((t * 2.5 + k as f64).sin() as f32) * pitch.height() * 0.12;
                for j in 0..3 {
                    let y = pitch.min.y + pitch.height() * (j as f32 + 0.5) / 3.0 + shift;
                    painter.rect_filled(Rect::from_center_size(Pos2::new(x, y), Vec2::new(s * 0.07, s * 0.1)), 1.0, color);
                }
            }
            let bx = pitch.center().x + (t * 1.7).sin() as f32 * pitch.width() * 0.42;
            let by = pitch.center().y + (t * 2.3).cos() as f32 * pitch.height() * 0.35;
            painter.circle_filled(Pos2::new(bx, by), s * 0.035, rgb(0xffffff));
        }
        "screens" => {
            line(at(0.0, 0.3), at(0.0, 0.85), 3.0, rgb(0x455a64));
            painter.rect_filled(rect(-0.4, 0.8, 0.4, 0.92), 2.0, rgb(0x455a64));
            for k in 0..4 {
                let (cx, cy) = (if k % 2 == 0 { -0.45 } else { 0.45 }, if k < 2 { -0.45 } else { 0.08 });
                let scr = rect(cx - 0.42, cy - 0.25, cx + 0.42, cy + 0.25);
                painter.rect_filled(scr, 2.0, rgb(0x212121));
                let inner = scr.shrink(2.0);
                painter.rect_filled(inner, 1.0, rgb(0x0d2137));
                for b in 0..5 {
                    let v = (((t * 1.3 + b as f64 * 0.9 + k as f64 * 2.1).sin() * 0.5 + 0.5) as f32).max(0.15);
                    let bw = inner.width() / 5.0;
                    painter.rect_filled(Rect::from_min_max(Pos2::new(inner.min.x + b as f32 * bw + 1.0, inner.max.y - inner.height() * v * 0.85), Pos2::new(inner.min.x + (b + 1) as f32 * bw - 1.0, inner.max.y - 1.0)), 0.0, [rgb(0x4fc3f7), rgb(0x81c784), rgb(0xffb74d), rgb(0xba68c8)][k]);
                }
            }
        }
        "chairs" => {
            for (k, x) in [-0.45f32, 0.45].into_iter().enumerate() {
                let c = at(x, 0.1);
                for j in 0..5 {
                    let ang = j as f32 * std::f32::consts::TAU / 5.0 + 0.3;
                    let foot = c + Vec2::angled(ang) * s * 0.3;
                    line(c, foot, 2.0, rgb(0x424242));
                    painter.circle_filled(foot, 2.0, rgb(0x212121));
                }
                let color = if k == 0 { rgb(0x1e88e5) } else { rgb(0x43a047) };
                painter.circle_filled(c, s * 0.2, color);
                painter.rect_filled(Rect::from_center_size(c - Vec2::new(0.0, s * 0.3), Vec2::new(s * 0.36, s * 0.14)), 4.0, color.gamma_multiply(0.8));
                line(c - Vec2::new(s * 0.22, 0.0), c - Vec2::new(s * 0.22, s * 0.12), 2.0, rgb(0x616161));
                line(c + Vec2::new(s * 0.22, 0.0), c + Vec2::new(s * 0.22, -s * 0.12), 2.0, rgb(0x616161));
            }
        }
        "fiber" => {
            let cab = rect(-0.55, -0.9, 0.55, 0.9);
            painter.rect_filled(cab, 3.0, rgb(0x263238));
            for row in 0..5 {
                let y = -0.7 + row as f32 * 0.33;
                painter.rect_filled(rect(-0.45, y - 0.1, 0.45, y + 0.1), 1.0, rgb(0x37474f));
                for port in 0..6 {
                    let x = -0.38 + port as f32 * 0.15;
                    let on = ((t * 5.0) as i64 + row + port * 3) % 5 != 0;
                    painter.circle_filled(at(x, y), 1.4, if on { rgb(0x00e676) } else { rgb(0x1b5e20) });
                }
            }
            for (k, color) in [rgb(0xffeb3b), rgb(0x29b6f6), rgb(0xff7043)].into_iter().enumerate() {
                let y = -0.6 + k as f32 * 0.5;
                line(at(0.55, y), at(0.85, y + 0.2), 2.0, color);
                line(at(0.85, y + 0.2), at(0.85, 0.95), 2.0, color);
            }
        }
        "library" => {
            let frame = rect(-0.9, -0.95, 0.9, 0.9);
            painter.rect_filled(frame, 2.0, rgb(0x5d4037));
            let colors = [0xe57373, 0x64b5f6, 0x81c784, 0xffd54f, 0xba68c8, 0x4db6ac, 0xff8a65, 0x90a4ae];
            for shelf in 0..3 {
                let y0 = -0.88 + shelf as f32 * 0.6;
                let y1 = y0 + 0.52;
                painter.rect_filled(rect(-0.82, y0, 0.82, y1), 0.0, rgb(0x3e2723));
                let n = 8;
                for b in 0..n {
                    let seed = (shelf * 5 + b * 3) % 7;
                    let bh = 0.55 + seed as f32 * 0.06;
                    let x0 = -0.8 + b as f32 * (1.6 / n as f32);
                    if seed == 6 {
                        continue;
                    }
                    painter.rect_filled(rect(x0 + 0.01, y1 - (y1 - y0) * bh, x0 + 1.6 / n as f32 - 0.02, y1), 0.5, rgb(colors[(shelf * 3 + b) % colors.len()]));
                }
                painter.rect_filled(rect(-0.86, y1, 0.86, y1 + 0.07), 0.0, rgb(0x795548));
            }
        }
        "meeting" => {
            painter.rect_filled(rect(-0.5, -0.98, 0.5, -0.72), 2.0, rgb(0x212121));
            painter.rect_filled(rect(-0.46, -0.94, 0.46, -0.76), 1.0, rgb(0x1565c0));
            let table = Rect::from_center_size(at(0.0, 0.1), Vec2::new(w * 0.62, h * 0.5));
            for k in 0..6 {
                let ang = k as f32 * std::f32::consts::TAU / 6.0 + 0.5;
                painter.circle_filled(table.center() + Vec2::new(ang.cos() * w * 0.4, ang.sin() * h * 0.34), s * 0.08, rgb(0x546e7a));
            }
            painter.add(egui::Shape::ellipse_filled(table.center(), table.size() / 2.0, rgb(0xd7ccc8)));
            painter.add(egui::Shape::ellipse_stroke(table.center(), table.size() / 2.0, Stroke::new(1.5, rgb(0xa1887f))));
            painter.rect_filled(Rect::from_center_size(table.center() - Vec2::new(w * 0.12, 0.0), Vec2::new(s * 0.2, s * 0.13)), 1.0, rgb(0x90a4ae));
            painter.circle_filled(table.center() + Vec2::new(w * 0.15, 0.0), s * 0.05, rgb(0xfafafa));
        }
        "nap" => {
            let sofa = rect(-0.95, -0.35, 0.95, 0.55);
            painter.rect_filled(sofa, 8.0, rgb(0x5e35b1));
            painter.rect_filled(rect(-0.95, -0.35, 0.95, -0.1), 6.0, rgb(0x4527a0));
            for k in 0..3 {
                let x = -0.62 + k as f32 * 0.62;
                painter.rect_filled(rect(x - 0.28, -0.05, x + 0.28, 0.45), 5.0, rgb(0x7e57c2));
            }
            painter.rect_filled(rect(-0.75, 0.0, -0.35, 0.25), 6.0, rgb(0xf8bbd0));
            painter.rect_filled(rect(0.1, 0.05, 0.85, 0.45), 4.0, rgb(0x80cbc4).gamma_multiply(0.9));
            for k in 0..2 {
                let z = ((t * 0.6 + k as f64 * 0.5) % 1.0) as f32;
                painter.text(at(0.5 + z * 0.3, -0.5 - z * 0.5), Align2::CENTER_CENTER, "z", FontId::proportional(9.0 + z * 6.0), Color32::from_white_alpha(((1.0 - z) * 230.0) as u8));
            }
        }
        "servers" => {
            for k in 0..3 {
                let x0 = -0.95 + k as f32 * 0.65;
                let rack = rect(x0, -0.95, x0 + 0.55, 0.92);
                painter.rect_filled(rack, 2.0, rgb(0x212b30));
                painter.rect_stroke(rack, 2.0, Stroke::new(1.0, rgb(0x455a64)), egui::StrokeKind::Inside);
                for row in 0..7 {
                    let y = rack.min.y + 4.0 + row as f32 * (rack.height() - 8.0) / 7.0;
                    painter.rect_filled(Rect::from_min_size(Pos2::new(rack.min.x + 2.0, y), Vec2::new(rack.width() - 4.0, (rack.height() - 8.0) / 7.0 - 2.0)), 1.0, rgb(0x2f3d44));
                    let blink = ((t * 7.0) as i64 + row + k as i64 * 3) % 4 != 0;
                    let color = if down { if (t * 4.0) as i64 % 2 == 0 { rgb(0xff1744) } else { rgb(0x4a0000) } } else if blink { rgb(0x00e676) } else { rgb(0x1b5e20) };
                    painter.circle_filled(Pos2::new(rack.min.x + 5.0, y + 2.5), 1.3, color);
                    painter.circle_filled(Pos2::new(rack.min.x + 9.0, y + 2.5), 1.3, if blink { rgb(0x40c4ff) } else { rgb(0x01579b) });
                }
            }
        }
        "gym" => {
            let mill = rect(-0.95, -0.9, -0.15, 0.9);
            painter.rect_filled(mill, 4.0, rgb(0x37474f));
            let belt = mill.shrink(s * 0.06);
            painter.rect_filled(belt, 2.0, rgb(0x212121));
            let offset = ((t * 40.0) % 9.0) as f32;
            let mut y = belt.min.y + offset;
            while y < belt.max.y {
                line(Pos2::new(belt.min.x + 1.0, y), Pos2::new(belt.max.x - 1.0, y), 1.0, rgb(0x424242));
                y += 9.0;
            }
            painter.rect_filled(rect(-0.95, -0.98, -0.15, -0.82), 2.0, rgb(0x90a4ae));
            painter.rect_filled(rect(0.05, -0.2, 0.35, 0.8), 3.0, rgb(0xd32f2f));
            for (k, y) in [-0.7f32, -0.45, 0.85].into_iter().enumerate() {
                let c = at(0.7, y);
                line(c - Vec2::new(s * 0.14, 0.0), c + Vec2::new(s * 0.14, 0.0), 2.0, rgb(0x9e9e9e));
                for side in [-1.0f32, 1.0] {
                    painter.circle_filled(c + Vec2::new(side * s * 0.14, 0.0), s * (0.05 + 0.015 * k as f32), rgb(0x212121));
                }
            }
        }
        "chef" => {
            painter.rect_filled(rect(-1.0, -0.2, 1.0, 0.9), 3.0, rgb(0xb0bec5));
            painter.rect_filled(rect(-1.0, -0.2, 1.0, -0.05), 2.0, rgb(0xeceff1));
            let stove = rect(-0.9, 0.0, 0.1, 0.8);
            painter.rect_filled(stove, 2.0, rgb(0x263238));
            for (k, (x, y)) in [(-0.65f32, 0.2f32), (-0.15, 0.2), (-0.65, 0.6), (-0.15, 0.6)].into_iter().enumerate() {
                let p = at(x, y);
                painter.circle_stroke(p, s * 0.09, Stroke::new(1.2, rgb(0x546e7a)));
                if k == 0 || k == 3 {
                    let flick = 0.8 + 0.2 * ((t * 12.0 + k as f64).sin() as f32);
                    painter.circle_filled(p, s * 0.07 * flick, rgb(0xff7043).gamma_multiply(0.85));
                    painter.circle_filled(p, s * 0.035, rgb(0xffca28));
                }
            }
            painter.circle_filled(at(-0.65, 0.2), s * 0.11, rgb(0x78909c));
            steam(at(-0.65, 0.1), 0.0);
            steam(at(-0.15, 0.5), 0.4);
            let board = rect(0.25, 0.1, 0.9, 0.6);
            painter.rect_filled(board, 2.0, rgb(0xd7a86e));
            for (k, color) in [rgb(0xff7043), rgb(0x66bb6a), rgb(0xffee58)].into_iter().enumerate() {
                painter.circle_filled(board.center() + Vec2::new((k as f32 - 1.0) * s * 0.11, 0.0), s * 0.05, color);
            }
            let hat = at(0.6, -0.6);
            painter.circle_filled(hat, s * 0.12, rgb(0xfafafa));
            painter.circle_filled(hat + Vec2::new(-s * 0.08, s * 0.03), s * 0.08, rgb(0xfafafa));
            painter.circle_filled(hat + Vec2::new(s * 0.08, s * 0.03), s * 0.08, rgb(0xfafafa));
            painter.rect_filled(Rect::from_center_size(hat + Vec2::new(0.0, s * 0.13), Vec2::new(s * 0.22, s * 0.07)), 1.0, rgb(0xeeeeee));
        }
        "arcade" => {
            for k in 0..2 {
                let x0 = -0.95 + k as f32 * 1.0;
                let cab = rect(x0, -0.95, x0 + 0.85, 0.9);
                let body = if k == 0 { rgb(0x3949ab) } else { rgb(0xd81b60) };
                painter.rect_filled(cab, 3.0, body);
                painter.rect_filled(Rect::from_min_size(cab.min + Vec2::new(2.0, 2.0), Vec2::new(cab.width() - 4.0, cab.height() * 0.13)), 1.0, rgb(0xffeb3b));
                let screen = Rect::from_min_size(cab.min + Vec2::new(4.0, cab.height() * 0.2), Vec2::new(cab.width() - 8.0, cab.height() * 0.38));
                painter.rect_filled(screen, 1.0, rgb(0x050510));
                let hue = ((t * 0.3 + k as f64 * 0.5) % 1.0) as f32;
                let color = Color32::from(egui::ecolor::Hsva::new(hue, 0.8, 1.0, 1.0));
                let px = screen.width() / 8.0;
                let dx = ((t * 1.5).sin() as f32) * px;
                for (i, row) in [0b0010_0100u8, 0b0111_1110, 0b1101_1011, 0b1111_1111, 0b0101_1010].into_iter().enumerate() {
                    for j in 0..8 {
                        if row & (1 << (7 - j)) != 0 {
                            painter.rect_filled(Rect::from_min_size(Pos2::new(screen.min.x + j as f32 * px + dx * 0.5, screen.min.y + 3.0 + i as f32 * px * 0.8), Vec2::new(px * 0.9, px * 0.7)), 0.0, color);
                        }
                    }
                }
                let panel = Rect::from_min_size(Pos2::new(cab.min.x, screen.max.y + 3.0), Vec2::new(cab.width(), cab.height() * 0.16));
                painter.rect_filled(panel, 1.0, body.gamma_multiply(0.7));
                painter.circle_filled(panel.left_center() + Vec2::new(panel.width() * 0.28, 0.0), s * 0.04, rgb(0x212121));
                line(panel.left_center() + Vec2::new(panel.width() * 0.28, 0.0), panel.left_center() + Vec2::new(panel.width() * 0.28, -s * 0.07), 2.0, rgb(0x212121));
                painter.circle_filled(panel.left_center() + Vec2::new(panel.width() * 0.6, 0.0), s * 0.035, rgb(0xff1744));
                painter.circle_filled(panel.left_center() + Vec2::new(panel.width() * 0.78, 0.0), s * 0.035, rgb(0x00e676));
            }
        }
        "rooftop" => {
            painter.rect_filled(rect(-1.0, -1.0, 1.0, 1.0), 4.0, rgb(0xbcaaa4));
            let mut y = r.min.y + 4.0;
            while y < r.max.y {
                line(Pos2::new(r.min.x, y), Pos2::new(r.max.x, y), 1.0, rgb(0xa1887f));
                y += 7.0;
            }
            painter.rect_filled(rect(-0.85, 0.1, -0.15, 0.85), 3.0, rgb(0x4fc3f7));
            painter.rect_filled(rect(-0.85, 0.1, -0.15, 0.3), 3.0, rgb(0x0288d1));
            painter.rect_filled(rect(0.15, 0.25, 0.85, 0.85), 3.0, rgb(0xffb74d));
            let shade = at(0.4, -0.35);
            let rad = s * 0.42;
            for k in 0..8 {
                let a0 = k as f32 * std::f32::consts::TAU / 8.0;
                let a1 = a0 + std::f32::consts::TAU / 8.0;
                let tri = vec![shade, shade + Vec2::angled(a0) * rad, shade + Vec2::angled(a1) * rad];
                painter.add(egui::Shape::convex_polygon(tri, if k % 2 == 0 { rgb(0xffffff) } else { rgb(0xe53935) }, Stroke::NONE));
            }
            painter.circle_filled(shade, 2.5, rgb(0x5d4037));
            painter.circle_filled(at(-0.75, -0.75), s * 0.12, rgb(0x43a047));
        }
        _ => {
            painter.rect_filled(r.shrink(6.0), 4.0, theme.tab_hover);
        }
    }
}
