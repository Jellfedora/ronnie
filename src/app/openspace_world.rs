//! Open Space, the world on screen (its rules and floor's link in `crate::openspace_world`): the
//! business district every player shares, drawn in isometric like the open space (each company a
//! tower as high as its boss's office, the player's own ringed in gold, a shield as a blue dome, the
//! day's quests as markers, the audit firm's tower in the centre during the « Surprise audit »); a
//! company clicked in a card at the bottom (attack, invite); the guild, the ranking and the quests in
//! the side panel; the raids suffered while away in a card at the opening.

use super::openspace_raid::{Fight, Paid, Target};
use super::*;
use crate::config::FloorAccount;
use crate::openspace::{self as os, tx, OpenSpace};
use crate::openspace_battle::{self as war, Troop};
use crate::openspace_world::{self as w, Answer, Ask, World, WorldLink};
use std::collections::HashSet;

/// The world asked again this often while shown (seconds); the guild while its panel is open.
const REFRESH: f64 = 30.0;
/// Troops given at once, and a day (floor's limits).
const GIVE_MAX: u32 = 10;
const GIVE_DAY: u32 = 40;

/// What's clicked on the map.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Player(String),
    Bot(String),
    Quest(String),
    Boss,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Side {
    #[default]
    None,
    Guild,
    Board,
    Quests,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum GuildTab {
    #[default]
    Members,
    Quests,
    War,
    Gifts,
}

#[derive(Clone, Copy)]
struct Cam {
    zoom: f32,
    pan: Vec2,
}

impl Default for Cam {
    fn default() -> Self {
        Self { zoom: 0.7, pan: Vec2::ZERO }
    }
}

impl WorldPage {
    /// The world asked again at once (the page just opened).
    pub fn asked_world_reset(&mut self) {
        self.asked_world = None;
        self.centred = false;
    }
}

#[derive(Default)]
pub(super) struct WorldPage {
    /// Shown instead of the open space.
    pub open: bool,
    link: WorldLink,
    world: Option<World>,
    asked_world: Option<f64>,
    asked_guild: Option<f64>,
    cam: Cam,
    centred: bool,
    pick: Option<Pick>,
    side: Side,
    guild_tab: GuildTab,
    board: Option<w::Board>,
    board_guilds: bool,
    guilds: Option<Vec<w::GuildRow>>,
    /// None: not known yet; Some(None): no guild.
    guild: Option<Option<w::Guild>>,
    /// The base being fetched to raid it.
    waiting: Option<String>,
    /// The raids suffered already taken off the open space.
    applied: HashSet<i64>,
    /// The raids suffered, to show.
    report: Vec<w::Defense>,
    said: Option<(String, f64)>,
    new_name: String,
    new_tag: String,
    new_closed: bool,
    leaving: bool,
    give_to: Option<String>,
    give: Vec<(Troop, u32)>,
    /// floor didn't answer the last time.
    unreachable: bool,
    /// No open space on floor yet (the progress not synced).
    no_base: bool,
}

/// The lots on screen: a lot is `2 hw` wide, `2 hh` high.
#[derive(Clone, Copy)]
struct Map {
    origin: Pos2,
    hw: f32,
    hh: f32,
}

impl Map {
    fn new(rect: Rect, cam: &Cam, size: i32) -> Self {
        let hw = 44.0 * cam.zoom;
        let hh = hw * 0.5;
        Self { origin: rect.center() + cam.pan - Vec2::new(0.0, size as f32 * hh), hw, hh }
    }
    fn at(&self, gx: f32, gy: f32) -> Pos2 {
        self.origin + Vec2::new((gx - gy) * self.hw, (gx + gy) * self.hh)
    }
    fn up(&self, p: Pos2, z: f32) -> Pos2 {
        p - Vec2::new(0.0, z * self.k())
    }
    fn back(&self, p: Pos2) -> (f32, f32) {
        let d = p - self.origin;
        let (a, b) = (d.x / self.hw, d.y / self.hh);
        ((a + b) / 2.0, (b - a) / 2.0)
    }
    fn k(&self) -> f32 {
        self.hw / 44.0
    }
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| (v as f32 * f).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

fn lot_hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 1000) as f32 / 1000.0
}

/// A box from (x0, y0) to (x1, y1), from `z0` to `z1` high.
#[allow(clippy::too_many_arguments)]
fn block(painter: &egui::Painter, m: &Map, x0: f32, y0: f32, x1: f32, y1: f32, z0: f32, z1: f32, color: Color32) {
    let (n, e, s, wp) = (m.at(x0, y0), m.at(x1, y0), m.at(x1, y1), m.at(x0, y1));
    let edge = Stroke::new(0.8, shade(color, 0.45));
    painter.add(egui::Shape::convex_polygon(vec![m.up(wp, z0), m.up(s, z0), m.up(s, z1), m.up(wp, z1)], shade(color, 0.8), edge));
    painter.add(egui::Shape::convex_polygon(vec![m.up(s, z0), m.up(e, z0), m.up(e, z1), m.up(s, z1)], shade(color, 0.62), edge));
    painter.add(egui::Shape::convex_polygon(vec![m.up(n, z1), m.up(e, z1), m.up(s, z1), m.up(wp, z1)], shade(color, 1.08), edge));
}

/// A company's tower on lot (x, y): as high as its boss's office, its windows lit at night.
#[allow(clippy::too_many_arguments)]
fn tower(painter: &egui::Painter, m: &Map, x: i32, y: i32, hq: u8, color: Color32, flag: Option<Color32>, dim: bool, night: f32) -> f32 {
    let (x, y) = (x as f32, y as f32);
    let h = 16.0 + hq.max(1) as f32 * 8.0;
    let body = if dim { shade(color, 0.55) } else { color };
    // The forecourt, then the tower in two steps.
    block(painter, m, x + 0.12, y + 0.12, x + 0.88, y + 0.88, 0.0, 2.5, Color32::from_rgb(150, 150, 142));
    block(painter, m, x + 0.2, y + 0.2, x + 0.8, y + 0.8, 2.5, h * 0.62, body);
    block(painter, m, x + 0.28, y + 0.28, x + 0.72, y + 0.72, h * 0.62, h, shade(body, 1.12));
    // The windows: rows on the two faces.
    let lit = Color32::from_rgba_unmultiplied(255, 226, 140, (60.0 + 170.0 * night) as u8);
    let floors = (hq as usize).clamp(1, 10);
    for f in 0..floors {
        let z = 6.0 + f as f32 * (h * 0.62 - 9.0) / floors as f32;
        for k in 0..3 {
            let u = 0.26 + k as f32 * 0.18;
            let on = lot_hash(x as i32 * 7 + f as i32, y as i32 * 3 + k) < 0.55 + 0.3 * night;
            let c = if on { lit } else { Color32::from_black_alpha(60) };
            let a = m.up(m.at(x + 0.2 + u * 0.6, y + 0.8), z);
            painter.rect_filled(Rect::from_center_size(a, Vec2::new(3.0, 3.5) * m.k()), 0.0, c);
            let b = m.up(m.at(x + 0.8, y + 0.8 - u * 0.6), z);
            painter.rect_filled(Rect::from_center_size(b, Vec2::new(3.0, 3.5) * m.k()), 0.0, shade(c, 0.85));
        }
    }
    // The guild's flag on the roof.
    if let Some(f) = flag {
        let top = m.up(m.at(x + 0.5, y + 0.5), h);
        painter.line_segment([top, top - Vec2::new(0.0, 14.0 * m.k())], Stroke::new(1.5 * m.k(), Color32::from_gray(60)));
        let p = top - Vec2::new(0.0, 14.0 * m.k());
        painter.add(egui::Shape::convex_polygon(vec![p, p + Vec2::new(10.0, 3.0) * m.k(), p + Vec2::new(0.0, 6.0) * m.k()], f, Stroke::NONE));
    }
    h
}

/// A dome over a lot: its shield.
fn dome(painter: &egui::Painter, m: &Map, x: i32, y: i32, time: f64) {
    let c = m.at(x as f32 + 0.5, y as f32 + 0.5);
    let (rx, ry) = (m.hw * 0.62, m.hh * 1.7);
    let pts: Vec<Pos2> = (0..=24).map(|k| {
        let a = std::f32::consts::PI * k as f32 / 24.0;
        c + Vec2::new(-rx * a.cos(), -ry * a.sin() * 1.4)
    }).collect();
    let pulse = (time * 2.0).sin() as f32 * 0.5 + 0.5;
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgba_unmultiplied(120, 200, 255, (120.0 + 80.0 * pulse) as u8))));
    painter.add(egui::Shape::ellipse_filled(c - Vec2::new(0.0, ry * 0.55), Vec2::new(rx, ry * 0.9), Color32::from_rgba_unmultiplied(120, 200, 255, 28)));
}

/// A label over the map: a dark pill.
fn tag(painter: &egui::Painter, at: Pos2, text: &str, color: Color32, size: f32) -> Rect {
    let g = painter.layout_no_wrap(text.to_owned(), FontId::proportional(size), color);
    let r = Rect::from_center_size(at, g.size() + Vec2::new(10.0, 4.0));
    painter.rect_filled(r, 6.0, Color32::from_black_alpha(160));
    painter.galley(r.min + Vec2::new(5.0, 2.0), g, color);
    r
}

fn quest_icon(kind: &str) -> (&'static str, Color32) {
    match kind {
        "convoy" => ("☕", Color32::from_rgb(190, 120, 60)),
        "abandoned" => ("💼", Color32::from_rgb(80, 170, 100)),
        "hackers" => ("💀", Color32::from_rgb(170, 70, 200)),
        _ => ("🧑", Color32::from_rgb(70, 140, 220)),
    }
}

fn now_ms() -> i64 {
    super::openspace::unix_now() * 1000
}

/// What's left before `until` (unix ms), as text.
fn left(until: i64, fr: bool) -> String {
    os::duration_text((until - now_ms()) / 1000, fr)
}

fn stars(n: u8) -> String {
    (0..3).map(|k| if k < n { '★' } else { '☆' }).collect()
}

fn guild_label(g: &Option<w::GuildBrief>) -> String {
    g.as_ref().map(|g| format!("[{}] ", g.tag)).unwrap_or_default()
}

impl App {
    /// The account the world is played with (a pseudo on the floor in use).
    fn world_account(&self) -> Option<FloorAccount> {
        let url = crate::floor::url()?;
        self.config.settings.floor.clone().filter(|a| a.url == url && a.name.is_some())
    }

    fn world_send(&mut self, ctx: &egui::Context, ask: Ask) {
        if let Some(account) = self.world_account() {
            self.openspace.world.link.ask(ctx, &account, ask);
        }
    }

    /// Each frame Open Space is shown (its raids too): floor's answers taken, a world raid just over
    /// sent to be counted.
    pub(super) fn world_poll(&mut self, ctx: &egui::Context, s: &mut OpenSpace, time: f64) {
        let fr = self.os_fr();
        // A world raid over: counted by floor.
        let mut send = None;
        let signed = self.world_account().is_some();
        if let Some(f) = self.openspace.fight.as_mut()
            && f.result.is_some()
            && !f.sent
            && f.target != Target::Campaign
        {
            f.sent = true;
            let (stars, destroyed) = (f.b.stars(), f.b.destroyed());
            send = Some(match &f.target {
                Target::Quest { id, .. } => Ask::Quest { id: id.clone(), stars },
                Target::World { id, .. } => Ask::Attack { target: id.clone(), stars, destroyed },
                Target::Campaign => unreachable!(),
            });
            if !signed {
                f.paid = Some(Err(tx(("Pas connecté à floor : ce raid n'a pas compté.", "Not signed in to floor: this raid didn't count."), fr).into()));
                send = None;
            }
        }
        if let Some(ask) = send {
            self.world_send(ctx, ask);
        }
        let answers = self.openspace.world.link.take();
        for (ask, answer) in answers {
            self.world_answer(ctx, s, ask, answer, time, fr);
        }
    }

    fn world_answer(&mut self, ctx: &egui::Context, s: &mut OpenSpace, ask: Ask, answer: Answer, time: f64, fr: bool) {
        let say = |page: &mut WorldPage, text: String| page.said = Some((text, time));
        match answer {
            Answer::World(world) => {
                let page = &mut self.openspace.world;
                page.unreachable = false;
                page.no_base = false;
                // The raids suffered: their loot off the open space, once.
                let fresh: Vec<w::Defense> = world.defenses.iter().filter(|d| !page.applied.contains(&d.id)).cloned().collect();
                for d in &fresh {
                    s.lose(d.coffee, d.budget);
                    page.applied.insert(d.id);
                }
                if !fresh.is_empty() {
                    let ids = fresh.iter().map(|d| d.id).collect();
                    page.report.extend(fresh);
                    self.world_send(ctx, Ask::Seen(ids));
                }
                self.openspace.world.world = Some(*world);
            }
            Answer::Base(base) => {
                let page = &mut self.openspace.world;
                if page.waiting.as_deref() != Some(base.id.as_str()) {
                    return;
                }
                page.waiting = None;
                let level = base.level.unwrap_or_else(|| (base.hq.unwrap_or(1) as u32 - 1) * 5 + 3).clamp(1, war::LEVELS);
                let buildings = base.buildings.clone().filter(|b| !b.is_empty()).unwrap_or_else(|| war::rival_base(level));
                let unix = super::openspace::unix_now();
                self.openspace.fight = Some(Fight::on(&buildings, level, Target::World { id: base.id.clone(), name: base.name.clone() }, s, unix));
                page.pick = None;
                page.side = Side::None;
            }
            Answer::Struck(st) => {
                let (coffee, budget) = s.gain(st.coffee, st.budget, st.tickets);
                let paid = Paid { coffee, budget, tickets: st.tickets, trophies: Some((st.trophies, st.total)), troops: Vec::new() };
                match self.openspace.fight.as_mut() {
                    Some(f) if f.sent && f.paid.is_none() => f.paid = Some(Ok(paid)),
                    _ => say(&mut self.openspace.world, format!("{}{} 🏆", if st.trophies >= 0 { "+" } else { "" }, st.trophies)),
                }
                self.world_send(ctx, Ask::World);
            }
            Answer::Quest { id, reward } => {
                let (coffee, budget) = s.gain(reward.coffee, reward.budget, reward.tickets);
                let troops = w::troops(&reward.troops);
                s.give_troops(&troops);
                let paid = Paid { coffee, budget, tickets: reward.tickets, trophies: None, troops: troops.clone() };
                match self.openspace.fight.as_mut() {
                    Some(f) if matches!(&f.target, Target::Quest { id: q, .. } if *q == id) && f.paid.is_none() => f.paid = Some(Ok(paid)),
                    _ => {
                        let list: Vec<String> = troops.iter().map(|(t, n)| format!("{n} {}", tx(t.name(), fr))).collect();
                        let text = format!("{} {}", tx(("Quête faite :", "Quest done:"), fr), if list.is_empty() { format!("+{} 🎫", reward.tickets) } else { format!("+ {}", list.join(", ")) });
                        self.openspace.world.said = Some((text, time));
                    }
                }
                self.world_send(ctx, Ask::World);
            }
            Answer::Tickets(n) => {
                s.gain(0.0, 0.0, n);
                say(&mut self.openspace.world, format!("+{n} 🎫"));
                self.world_send(ctx, if ask == Ask::EventClaim { Ask::World } else { Ask::Guild });
            }
            Answer::Board(b) => self.openspace.world.board = Some(b),
            Answer::Guilds(list) => self.openspace.world.guilds = Some(list),
            Answer::Guild(g) => {
                let page = &mut self.openspace.world;
                let had = page.guild.as_ref().map(|g| g.as_ref().map(|g| g.guild.id.clone()));
                page.guild = Some(g.map(|g| *g));
                if let Ask::Donate { .. } = ask {
                    say(page, tx(("Troupes envoyées, merci pour la guilde !", "Troops sent, thanks from the guild!"), fr).into());
                }
                if let Ask::Invite(_) = ask {
                    say(page, tx(("Invitation envoyée", "Invitation sent"), fr).into());
                }
                let now = page.guild.as_ref().map(|g| g.as_ref().map(|g| g.guild.id.clone()));
                // In a guild now (or another): the map shows its flag.
                if had.is_some() && had != now || matches!(ask, Ask::Create { .. } | Ask::Join(_)) {
                    self.world_send(ctx, Ask::World);
                }
            }
            Answer::Troops(list) => {
                s.give_troops(&list);
                let text: Vec<String> = list.iter().map(|(t, n)| format!("{n} {}", tx(t.name(), fr))).collect();
                if !text.is_empty() {
                    say(&mut self.openspace.world, format!("{} {}", tx(("Renforts reçus :", "Reinforcements in:"), fr), text.join(", ")));
                }
                self.world_send(ctx, Ask::Guild);
            }
            Answer::Done => match ask {
                Ask::Leave => {
                    self.openspace.world.guild = Some(None);
                    self.openspace.world.leaving = false;
                    self.world_send(ctx, Ask::Guilds);
                    self.world_send(ctx, Ask::World);
                }
                Ask::Decline(_) => self.world_send(ctx, Ask::World),
                _ => {}
            },
            Answer::Failed { ask, message, code } => {
                let page = &mut self.openspace.world;
                let text = if message.is_empty() { tx(("floor ne répond pas : réessaie dans un moment.", "floor isn't answering: try again in a while."), fr).to_owned() } else { message };
                if ask == Ask::World && code.is_none() && text.contains("floor") {
                    page.unreachable = true;
                }
                match &ask {
                    Ask::World if code.as_deref() == Some("no-base") => page.no_base = true,
                    Ask::Attack { .. } | Ask::Quest { .. } => match self.openspace.fight.as_mut() {
                        Some(f) if f.sent && f.paid.is_none() => f.paid = Some(Err(text)),
                        _ => say(page, text),
                    },
                    Ask::Base(_) => {
                        page.waiting = None;
                        say(page, text);
                        self.world_send(ctx, Ask::World);
                    }
                    // Given away but refused: back to the army.
                    Ask::Donate { troops, .. } => {
                        s.give_troops(troops);
                        say(page, text);
                    }
                    Ask::Guild if code.as_deref() == Some("no-guild") => page.guild = Some(None),
                    _ => say(page, text),
                }
            }
        }
    }

    /// The world's page: the map, its cards and panels.
    pub(super) fn world_page(&mut self, ui: &mut Ui, rect: Rect, s: &mut OpenSpace) {
        let t = self.t();
        let fr = self.os_fr();
        let theme = self.theme.clone();
        let time = ui.input(|i| i.time);
        let ctx = ui.ctx().clone();
        let night = super::openspace_scene::night();
        ui.painter_at(rect).rect_filled(rect, 0.0, Color32::from_rgb(34, 40, 52).lerp_to_gamma(Color32::from_rgb(14, 16, 26), night));

        let Some(_account) = self.world_account() else {
            self.world_back_button(ui, rect, fr, &theme);
            self.openspace_back(ui, rect, t);
            let r = Rect::from_center_size(rect.center(), Vec2::new(460.0, 150.0));
            super::openspace::panel(ui, r, "world-signin", &theme, |ui| {
                ui.label(egui::RichText::new(tx(("Le monde se joue en ligne", "The world is played online"), fr)).size(18.0).strong().color(theme.text));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tx(("Connecte-toi avec ton compte Ronnie (le tchat, les classements) pour y poser ton open space, attaquer tes collègues et rejoindre une guilde.", "Sign in with your Ronnie account (the chat, the rankings) to put your open space on it, attack your colleagues and join a guild."), fr)).size(13.0).color(theme.text_muted));
            });
            return;
        };

        // Asked again now and then.
        let page = &mut self.openspace.world;
        if page.asked_world.is_none_or(|at| time - at > REFRESH) {
            page.asked_world = Some(time);
            self.world_send(&ctx, Ask::World);
        }
        let page = &mut self.openspace.world;
        if page.side == Side::Guild && page.asked_guild.is_none_or(|at| time - at > REFRESH) {
            page.asked_guild = Some(time);
            self.world_send(&ctx, Ask::Guild);
            self.world_send(&ctx, Ask::Guilds);
        }

        let page = &mut self.openspace.world;
        let Some(world) = page.world.clone() else {
            let text = if page.no_base {
                tx(("Ton open space n'est pas encore arrivé sur floor : attends la prochaine synchronisation (une minute), puis reviens.", "Your open space hasn't reached floor yet: wait for the next sync (a minute), then come back."), fr)
            } else if page.unreachable {
                tx(("floor ne répond pas : le monde revient dès qu'il répond.", "floor isn't answering: the world comes back as soon as it does."), fr)
            } else {
                tx(("Connexion au monde…", "Joining the world…"), fr)
            };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(15.0), theme.text_muted);
            self.world_back_button(ui, rect, fr, &theme);
            self.openspace_back(ui, rect, t);
            return;
        };
        let size = world.size;
        let me = world.me.clone();
        let mine_guild = me.guild.as_ref().map(|g| g.id.clone());
        let nowms = now_ms();

        // The camera: centred on the player's open space the first time.
        let map_resp = ui.interact(rect, ui.id().with("os-world"), Sense::click_and_drag());
        if !page.centred {
            page.centred = true;
            let m = Map::new(rect, &page.cam, size);
            page.cam.pan -= m.at(me.x as f32 + 0.5, me.y as f32 + 0.5) - rect.center();
        }
        if let Some(p) = map_resp.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.0018).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let old = page.cam.zoom;
                let new = (old * factor).clamp(0.3, 2.0);
                let d = p - (rect.center() + page.cam.pan);
                page.cam.pan += d - d * (new / old);
                page.cam.zoom = new;
            }
        }
        if map_resp.dragged() {
            page.cam.pan += map_resp.drag_delta();
        }
        let m = Map::new(rect, &page.cam, size);
        let painter = ui.painter_at(rect);

        // What's on each lot.
        let boss_at = world.boss.as_ref().map(|b| (b.x, b.y));
        let at_lot = |x: i32, y: i32| -> Option<Pick> {
            if let Some((bx, by)) = boss_at
                && (x - bx).abs() <= 1
                && (y - by).abs() <= 1
            {
                return Some(Pick::Boss);
            }
            if let Some(p) = world.players.iter().find(|p| p.x == x && p.y == y) {
                return Some(Pick::Player(p.id.clone()));
            }
            if let Some(b) = world.bots.iter().find(|b| b.x == x && b.y == y) {
                return Some(Pick::Bot(b.id.clone()));
            }
            world.quests.iter().find(|q| q.x == x && q.y == y && !q.done).map(|q| Pick::Quest(q.id.clone()))
        };
        let hover = map_resp.hover_pos().map(|p| m.back(p)).filter(|(x, y)| *x >= 0.0 && *y >= 0.0 && *x < size as f32 && *y < size as f32).and_then(|(x, y)| at_lot(x.floor() as i32, y.floor() as i32));
        if map_resp.clicked() {
            page.pick = hover.clone();
        }
        if map_resp.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            page.pick = None;
        }
        if hover.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // The district: the asphalt, each lot (grass, a forecourt, the plaza in the middle), trees.
        let n = size as f32;
        painter.add(egui::Shape::convex_polygon(vec![m.at(-0.3, -0.3), m.at(n + 0.3, -0.3), m.at(n + 0.3, n + 0.3), m.at(-0.3, n + 0.3)], Color32::from_rgb(62, 66, 74), Stroke::new(2.0, Color32::from_rgb(40, 42, 48))));
        let occupied: HashSet<(i32, i32)> = world.players.iter().map(|p| (p.x, p.y)).chain(world.bots.iter().map(|b| (b.x, b.y))).collect();
        for y in 0..size {
            for x in 0..size {
                let (fx, fy) = (x as f32, y as f32);
                let central = boss_at.is_some_and(|(bx, by)| (x - bx).abs() <= 1 && (y - by).abs() <= 1) || ((x - size / 2).abs() <= 1 && (y - size / 2).abs() <= 1);
                let h = lot_hash(x, y);
                let color = if central {
                    Color32::from_rgb(176, 168, 150)
                } else if occupied.contains(&(x, y)) {
                    Color32::from_rgb(132, 136, 128)
                } else {
                    Color32::from_rgb(78, 128 + (h * 30.0) as u8, 70).lerp_to_gamma(Color32::from_rgb(30, 50, 34), night * 0.6)
                };
                let i = 0.08;
                painter.add(egui::Shape::convex_polygon(vec![m.at(fx + i, fy + i), m.at(fx + 1.0 - i, fy + i), m.at(fx + 1.0 - i, fy + 1.0 - i), m.at(fx + i, fy + 1.0 - i)], color, Stroke::NONE));
                if hover == at_lot(x, y) && hover.is_some() || page.pick.is_some() && page.pick == at_lot(x, y) && !central {
                    painter.add(egui::Shape::closed_line(vec![m.at(fx + i, fy + i), m.at(fx + 1.0 - i, fy + i), m.at(fx + 1.0 - i, fy + 1.0 - i), m.at(fx + i, fy + 1.0 - i)], Stroke::new(2.0, theme.accent)));
                }
            }
        }
        // Back to front: the trees, the towers, the quests.
        let mut order: Vec<(i32, u8, usize)> = Vec::new();
        for y in 0..size {
            for x in 0..size {
                if !occupied.contains(&(x, y)) && lot_hash(x, y) < 0.3 && !((x - size / 2).abs() <= 1 && (y - size / 2).abs() <= 1) && !world.quests.iter().any(|q| q.x == x && q.y == y && !q.done) {
                    order.push((x + y, 0, (y * size + x) as usize));
                }
            }
        }
        for (k, p) in world.players.iter().enumerate() {
            order.push((p.x + p.y, 1, k));
        }
        for (k, b) in world.bots.iter().enumerate() {
            order.push((b.x + b.y, 2, k));
        }
        for (k, q) in world.quests.iter().enumerate().filter(|(_, q)| !q.done) {
            order.push((q.x + q.y, 3, k));
        }
        if let Some(b) = &world.boss {
            order.push((b.x + b.y + 2, 4, 0));
        }
        order.sort();
        let show_labels = page.cam.zoom > 0.55;
        let mut labels: Vec<(Pos2, String, Color32)> = Vec::new();
        for (_, what, k) in order {
            match what {
                0 => {
                    let (x, y) = ((k as i32) % size, (k as i32) / size);
                    let c = m.at(x as f32 + 0.5, y as f32 + 0.5);
                    painter.line_segment([c, m.up(c, 10.0)], Stroke::new(2.5 * m.k(), Color32::from_rgb(100, 70, 40)));
                    painter.circle_filled(m.up(c, 14.0), 9.0 * m.k(), Color32::from_rgb(52, 120 + (lot_hash(y, x) * 40.0) as u8, 60).lerp_to_gamma(Color32::from_rgb(20, 46, 28), night * 0.6));
                }
                1 => {
                    let p = &world.players[k];
                    let color = if p.me { Color32::from_rgb(230, 180, 70) } else { p.guild.as_ref().map_or(Color32::from_rgb(110, 150, 200), |g| rgb(g.color).lerp_to_gamma(Color32::WHITE, 0.25)) };
                    let h = tower(&painter, &m, p.x, p.y, p.hq, color, p.guild.as_ref().map(|g| rgb(g.color)), false, night);
                    if p.me {
                        let c = m.at(p.x as f32 + 0.5, p.y as f32 + 0.5);
                        let pulse = (time * 3.0).sin() as f32 * 0.5 + 0.5;
                        painter.add(egui::Shape::ellipse_stroke(c, Vec2::new(m.hw * (0.75 + 0.06 * pulse), m.hh * (0.75 + 0.06 * pulse)), Stroke::new(2.5, Color32::from_rgb(255, 196, 64))));
                    }
                    if p.shield_until > nowms {
                        dome(&painter, &m, p.x, p.y, time);
                    }
                    if show_labels || p.me || page.pick == Some(Pick::Player(p.id.clone())) {
                        let name = if p.me { tx(("Toi", "You"), fr).to_owned() } else { p.name.clone().unwrap_or_default() };
                        labels.push((m.up(m.at(p.x as f32 + 0.5, p.y as f32 + 0.5), h + 22.0), format!("{}{name}  🏆{}", guild_label(&p.guild), p.trophies), if p.me { Color32::from_rgb(255, 210, 110) } else { Color32::WHITE }));
                    }
                }
                2 => {
                    let b = &world.bots[k];
                    let rebuilding = b.ready_at > nowms;
                    let h = tower(&painter, &m, b.x, b.y, b.hq, Color32::from_rgb(150, 156, 170), Some(rgb(b.guild.color)), rebuilding, night);
                    if rebuilding {
                        painter.text(m.up(m.at(b.x as f32 + 0.5, b.y as f32 + 0.5), h + 8.0), Align2::CENTER_CENTER, "🔧", FontId::proportional(13.0 * m.k().max(0.7)), Color32::WHITE);
                    }
                    if show_labels && page.cam.zoom > 0.85 || page.pick == Some(Pick::Bot(b.id.clone())) {
                        labels.push((m.up(m.at(b.x as f32 + 0.5, b.y as f32 + 0.5), h + 22.0), format!("{} 🏆{}", b.name, b.trophies), Color32::from_gray(215)));
                    }
                }
                3 => {
                    let q = &world.quests[k];
                    let (icon, color) = quest_icon(&q.kind);
                    let bob = ((time * 3.0 + k as f64).sin() as f32 * 0.5 + 0.5) * 6.0;
                    let c = m.up(m.at(q.x as f32 + 0.5, q.y as f32 + 0.5), 18.0 + bob);
                    painter.add(egui::Shape::ellipse_filled(m.at(q.x as f32 + 0.5, q.y as f32 + 0.5), Vec2::new(m.hw * 0.3, m.hh * 0.3), Color32::from_black_alpha(70)));
                    painter.circle_filled(c, 13.0 * m.k().max(0.7), color);
                    painter.circle_stroke(c, 13.0 * m.k().max(0.7), Stroke::new(2.0, Color32::WHITE));
                    painter.text(c, Align2::CENTER_CENTER, icon, FontId::proportional(13.0 * m.k().max(0.7)), Color32::WHITE);
                    painter.text(c - Vec2::new(0.0, 20.0 * m.k().max(0.7)), Align2::CENTER_CENTER, "!", FontId::proportional(16.0 * m.k().max(0.7)), Color32::from_rgb(255, 210, 60));
                }
                _ => {
                    let b = world.boss.as_ref().expect("boss");
                    let (x0, y0) = (b.x as f32 - 1.0, b.y as f32 - 1.0);
                    let down = b.hp <= 0;
                    let body = if down { Color32::from_rgb(80, 80, 80) } else { Color32::from_rgb(120, 30, 40) };
                    block(&painter, &m, x0 + 0.3, y0 + 0.3, x0 + 2.7, y0 + 2.7, 0.0, 6.0, Color32::from_rgb(90, 90, 96));
                    block(&painter, &m, x0 + 0.6, y0 + 0.6, x0 + 2.4, y0 + 2.4, 6.0, if down { 30.0 } else { 120.0 }, body);
                    if !down {
                        block(&painter, &m, x0 + 0.95, y0 + 0.95, x0 + 2.05, y0 + 2.05, 120.0, 150.0, shade(body, 1.2));
                    }
                    let top = m.up(m.at(b.x as f32 + 0.5, b.y as f32 + 0.5), if down { 46.0 } else { 172.0 });
                    let bar = Rect::from_center_size(top, Vec2::new(120.0, 10.0));
                    painter.rect_filled(bar, 4.0, Color32::from_black_alpha(180));
                    let f = (b.hp as f32 / b.max.max(1) as f32).clamp(0.0, 1.0);
                    painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * f, bar.height())), 4.0, Color32::from_rgb(220, 60, 60));
                    labels.push((top - Vec2::new(0.0, 16.0), format!("{}  {} / {}", tx(("Cabinet d'audit", "Audit firm"), fr), b.hp, b.max), Color32::from_rgb(255, 160, 160)));
                }
            }
        }
        for (at, text, color) in labels {
            tag(&painter, at, &text, color, 11.5 * page.cam.zoom.clamp(0.8, 1.2));
        }

        // The top: back, the name, the trophies, the event.
        self.openspace_back(ui, rect, t);
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 30.0), Align2::CENTER_CENTER, tx(("Le monde", "The world"), fr), 28.0, theme.accent, 1.0);
        self.openspace_hud(ui, rect, s, fr, &theme, time);
        let page = &mut self.openspace.world;
        let gold = Color32::from_rgb(255, 196, 64);
        let mut chip_x = rect.min.x + 16.0;
        let chip_y = rect.min.y + 58.0;
        let mut chip = |ui: &mut Ui, text: String, color: Color32, hover: &str| {
            let g = ui.painter().layout_no_wrap(text, FontId::proportional(14.0), color);
            let r = Rect::from_min_size(Pos2::new(chip_x, chip_y), g.size() + Vec2::new(18.0, 10.0));
            ui.painter().rect_filled(r, 8.0, Color32::from_black_alpha(150));
            ui.painter().galley(r.min + Vec2::new(9.0, 5.0), g, color);
            if !hover.is_empty() {
                ui.interact(r, ui.id().with(("os-chip", chip_x as i32)), Sense::hover()).on_hover_text(hover);
            }
            chip_x = r.max.x + 8.0;
        };
        chip(ui, format!("🏆 {}", me.trophies), gold, tx(("Tes trophées : gagnés en raid dans le monde, perdus quand tu rates ou quand on te bat. Le classement les compte.", "Your trophies: won raiding in the world, lost when you fail or when you're beaten. The ranking counts them."), fr));
        if me.shield_until > nowms {
            chip(ui, format!("🛡 {}", left(me.shield_until, fr)), Color32::from_rgb(140, 210, 255), tx(("Ton bouclier : personne ne peut t'attaquer. Il tombe si tu attaques un joueur.", "Your shield: nobody can attack you. It drops if you attack a player."), fr));
        }
        if let Some(g) = &me.guild {
            chip(ui, format!("[{}] {}", g.tag, g.name), rgb(g.color).lerp_to_gamma(Color32::WHITE, 0.4), "");
        }
        // The event: under way, or the next one.
        let (title, text, when) = match &world.event {
            Some(e) => {
                let (n, d) = w::event_text(&e.id);
                (format!("✨ {}", tx(n, fr)), tx(d, fr).to_owned(), format!("{} {}", tx(("encore", "for"), fr), left(e.ends, fr)))
            }
            None => {
                let (n, d) = w::event_text(&world.next_event.id);
                (format!("{} {}", tx(("Bientôt :", "Coming:"), fr), tx(n, fr)), tx(d, fr).to_owned(), format!("{} {}", tx(("dans", "in"), fr), left(world.next_event.starts, fr)))
            }
        };
        let g = ui.painter().layout_no_wrap(format!("{title} · {when}"), FontId::proportional(13.5), if world.event.is_some() { Color32::from_rgb(255, 220, 120) } else { Color32::from_white_alpha(200) });
        let er = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 66.0), g.size() + Vec2::new(22.0, 10.0));
        ui.painter().rect_filled(er, 10.0, if world.event.is_some() { Color32::from_rgba_unmultiplied(120, 60, 20, 200) } else { Color32::from_black_alpha(150) });
        ui.painter().galley(er.min + Vec2::new(11.0, 5.0), g, Color32::WHITE);
        if ui.interact(er, ui.id().with("os-event"), Sense::click()).on_hover_text(text).clicked() {
            page.side = if page.side == Side::Quests { Side::None } else { Side::Quests };
        }

        // The bottom: back home, the guild, the ranking, the quests.
        let home = Rect::from_min_size(Pos2::new(rect.max.x - 210.0, rect.max.y - 58.0), Vec2::new(196.0, 42.0));
        if ui.put(home, egui::Button::new(egui::RichText::new(format!("🏢  {}", tx(("Mon open space", "My open space"), fr))).size(15.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(10.0)).clicked() {
            page.open = false;
            page.pick = None;
        }
        let mut x = home.min.x;
        let invites = !world.invites.is_empty();
        let gifts = page.guild.as_ref().and_then(|g| g.as_ref()).is_some_and(|g| g.gift.is_some() || !g.donations.is_empty() || g.quests.iter().any(|q| q.complete && !q.claimed) || g.last_war.as_ref().is_some_and(|l| !l.claimed && l.reward.unwrap_or(0) > 0));
        let quests_open = world.quests.iter().filter(|q| !q.done).count();
        let buttons: [(Side, String, bool); 3] = [
            (Side::Quests, format!("📜  {} {quests_open}", tx(("Quêtes", "Quests"), fr)), world.boss.as_ref().is_some_and(|b| b.hp <= 0 && b.hit && !b.claimed)),
            (Side::Board, format!("🏆  {}", tx(("Classement", "Ranking"), fr)), false),
            (Side::Guild, format!("🛡  {}", tx(("Guilde", "Guild"), fr)), invites || gifts),
        ];
        let mut ask_board = false;
        for (side, label, dot) in buttons {
            let r = Rect::from_min_size(Pos2::new(x - 140.0, home.min.y), Vec2::new(130.0, 42.0));
            x = r.min.x;
            if ui.put(r, egui::Button::new(egui::RichText::new(label).size(14.0).strong()).corner_radius(10.0)).clicked() {
                page.side = if page.side == side { Side::None } else { side };
                page.pick = None;
                match page.side {
                    Side::Board => ask_board = true,
                    Side::Guild => page.asked_guild = None,
                    _ => {}
                }
            }
            if dot {
                let pulse = (time * 4.0).sin() as f32 * 0.5 + 0.5;
                ui.painter().circle_filled(r.right_top() + Vec2::new(-4.0, 4.0), 6.0 + pulse * 1.5, Color32::from_rgb(220, 60, 60));
            }
        }

        if ask_board {
            self.world_send(&ctx, Ask::Board);
        }
        // The side panel.
        let page = &mut self.openspace.world;
        let side_rect = Rect::from_min_max(Pos2::new(rect.max.x - 400.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 14.0, rect.max.y - 70.0));
        let mut asks: Vec<Ask> = Vec::new();
        let mut go: Option<Pick> = None;
        match page.side {
            Side::Quests => {
                super::openspace::panel(ui, side_rect, "world-quests", &theme, |ui| quests_ui(ui, &world, fr, &theme, &mut go, &mut asks));
            }
            Side::Board => {
                super::openspace::panel(ui, side_rect, "world-board", &theme, |ui| board_ui(ui, page, fr, &theme));
            }
            Side::Guild => {
                super::openspace::panel(ui, side_rect, "world-guild", &theme, |ui| guild_ui(ui, page, &world, s, fr, &theme, &mut asks));
            }
            Side::None => {}
        }
        if let Some(p) = go {
            let lot = match &p {
                Pick::Quest(id) => world.quests.iter().find(|q| &q.id == id).map(|q| (q.x, q.y)),
                Pick::Boss => world.boss.as_ref().map(|b| (b.x, b.y)),
                Pick::Player(id) => world.players.iter().find(|q| &q.id == id).map(|q| (q.x, q.y)),
                Pick::Bot(id) => world.bots.iter().find(|q| &q.id == id).map(|q| (q.x, q.y)),
            };
            if let Some((x, y)) = lot {
                let m = Map::new(rect, &page.cam, size);
                page.cam.pan -= m.at(x as f32 + 0.5, y as f32 + 0.5) - rect.center();
            }
            page.pick = Some(p);
            page.side = Side::None;
        }

        // The company clicked.
        if let Some(pick) = page.pick.clone() {
            let card = Rect::from_center_size(Pos2::new(rect.center().x, rect.max.y - 110.0), Vec2::new(440.0_f32.min(rect.width() - 40.0), 150.0));
            let card = card.translate(Vec2::new(0.0, -40.0));
            let mut close = false;
            super::openspace::panel(ui, card, "world-pick", &theme, |ui| {
                close = pick_ui(ui, &pick, &world, &mine_guild, s, fr, &theme, &mut asks);
            });
            if close {
                page.pick = None;
            }
        }

        // The raids suffered while away.
        if !page.report.is_empty() {
            let mut ok = false;
            egui::Window::new("os-defenses")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .frame(Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(2.0, theme.ansi[1])).corner_radius(16.0).inner_margin(20.0))
                .show(&ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(egui::RichText::new(tx(("On t'a attaqué !", "You were attacked!"), fr)).size(19.0).strong().color(theme.text));
                    ui.add_space(6.0);
                    for d in &page.report {
                        let who = d.attacker.clone().unwrap_or_else(|| "?".into());
                        ui.label(egui::RichText::new(format!("{who}  {}  {:.0} %", stars(d.stars), d.destroyed * 100.0)).size(14.0).color(theme.text));
                        ui.label(egui::RichText::new(format!("−{}  −{}  ·  {}{} 🏆", os::amount_text(d.coffee, os::Res::Coffee, fr), os::amount_text(d.budget, os::Res::Budget, fr), if d.trophies >= 0 { "+" } else { "" }, d.trophies)).size(12.5).color(theme.text_muted));
                        ui.add_space(4.0);
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(tx(("Améliore tes défenses et tes pare-feu, et un bouclier te protège un moment après une défaite.", "Upgrade your defenses and firewalls; a shield protects you for a while after a defeat."), fr)).size(12.0).color(theme.text_muted));
                    ui.add_space(8.0);
                    ok = ui.add(egui::Button::new(egui::RichText::new(tx(("Compris", "Got it"), fr)).size(14.0).color(theme.bg)).fill(theme.accent).corner_radius(8.0)).clicked();
                });
            if ok {
                page.report.clear();
            }
        }

        // Waiting for a base, a message.
        if page.waiting.is_some() {
            let r = Rect::from_center_size(rect.center(), Vec2::new(260.0, 44.0));
            ui.painter().rect_filled(r, 12.0, Color32::from_black_alpha(190));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, tx(("Repérage de la cible…", "Scouting the target…"), fr), FontId::proportional(14.0), Color32::WHITE);
        }
        if let Some((text, at)) = &page.said {
            let age = time - at;
            if age > 3.5 {
                page.said = None;
            } else {
                let a = (1.0 - ((age - 2.9) / 0.6).max(0.0)) as f32;
                let g = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(14.0), Color32::WHITE.gamma_multiply(a));
                let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 112.0), g.size() + Vec2::new(28.0, 16.0));
                ui.painter().rect_filled(r, 10.0, Color32::from_rgba_unmultiplied(40, 40, 50, (220.0 * a) as u8));
                ui.painter().galley(r.min + Vec2::new(14.0, 8.0), g, Color32::WHITE);
            }
        }

        // What the cards and panels asked.
        let unix = super::openspace::unix_now();
        for ask in asks {
            match &ask {
                Ask::Donate { troops, .. } if !s.give_away(troops) => {
                    self.openspace.world.said = Some((tx(("Tu n'as pas ces troupes", "You don't have these troops"), fr).into(), time));
                    continue;
                }
                Ask::Donate { .. } => self.openspace.world.give.clear(),
                // A base asked to raid it; a quest raided straight away.
                Ask::Base(id) => {
                    if s.army.is_empty() {
                        self.openspace.world.said = Some((tx(("Ton armée est vide : recrute des troupes avant d'attaquer.", "Your army is empty: recruit troops before attacking."), fr).into(), time));
                        continue;
                    }
                    self.openspace.world.waiting = Some(id.clone());
                }
                Ask::Quest { id, .. } => {
                    if let Some(q) = world.quests.iter().find(|q| &q.id == id).filter(|q| q.raid) {
                        if s.army.is_empty() {
                            self.openspace.world.said = Some((tx(("Ton armée est vide : recrute des troupes avant d'attaquer.", "Your army is empty: recruit troops before attacking."), fr).into(), time));
                            continue;
                        }
                        let name = tx(w::quest_text(&q.kind).0, fr).to_owned();
                        self.openspace.fight = Some(Fight::on(&war::rival_base(q.level), q.level, Target::Quest { id: q.id.clone(), name }, s, unix));
                        self.openspace.world.pick = None;
                        continue;
                    }
                }
                _ => {}
            }
            self.world_send(&ctx, ask);
        }
    }

    /// Before the account: back to the open space.
    fn world_back_button(&mut self, ui: &mut Ui, rect: Rect, fr: bool, theme: &Theme) {
        let home = Rect::from_min_size(Pos2::new(rect.max.x - 210.0, rect.max.y - 58.0), Vec2::new(196.0, 42.0));
        if ui.put(home, egui::Button::new(egui::RichText::new(format!("🏢  {}", tx(("Mon open space", "My open space"), fr))).size(15.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(10.0)).clicked() {
            self.openspace.world.open = false;
        }
    }
}

fn heading(ui: &mut Ui, text: &str, theme: &Theme) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).size(12.5).strong().color(theme.text_muted));
}

fn action(ui: &mut Ui, enabled: bool, text: impl Into<String>, color: Color32, theme: &Theme) -> bool {
    let b = egui::Button::new(egui::RichText::new(text.into()).size(13.5).strong().color(if enabled { Color32::WHITE } else { theme.text_muted })).fill(if enabled { color } else { theme.tab_hover }).corner_radius(8.0);
    ui.add_enabled(enabled, b).clicked()
}

/// The card of what's clicked: true to close it.
#[allow(clippy::too_many_arguments)]
fn pick_ui(ui: &mut Ui, pick: &Pick, world: &World, mine_guild: &Option<String>, s: &OpenSpace, fr: bool, theme: &Theme, asks: &mut Vec<Ask>) -> bool {
    let nowms = now_ms();
    let red = Color32::from_rgb(190, 60, 50);
    let mut close = false;
    let title = |ui: &mut Ui, text: String, close: &mut bool| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(text).size(16.0).strong().color(theme.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    *close = true;
                }
            });
        });
    };
    let empty = s.army.is_empty();
    match pick {
        Pick::Player(id) => {
            let Some(p) = world.players.iter().find(|p| &p.id == id) else { return true };
            title(ui, format!("{}{}", guild_label(&p.guild), if p.me { tx(("Ton open space", "Your open space"), fr).to_owned() } else { p.name.clone().unwrap_or_default() }), &mut close);
            ui.label(egui::RichText::new(format!("{} {} · 🏆 {}", tx(("Bureau du boss niveau", "Boss's office level"), fr), p.hq, p.trophies)).size(13.0).color(theme.text_muted));
            if p.me {
                ui.label(egui::RichText::new(tx(("Les autres joueurs te voient ici. Attaque-les pour gagner des trophées !", "The other players see you here. Attack them to win trophies!"), fr)).size(12.5).color(theme.text_muted));
                return close;
            }
            let same = p.guild.as_ref().is_some_and(|g| Some(&g.id) == mine_guild.as_ref());
            let shielded = p.shield_until > nowms;
            let small = (p.hq as i32) < world.me.hq as i32 - 2;
            let why = if same {
                Some(tx(("Dans ta guilde", "In your guild"), fr).to_owned())
            } else if shielded {
                Some(format!("🛡 {} {}", tx(("Bouclier encore", "Shield for"), fr), left(p.shield_until, fr)))
            } else if small {
                Some(tx(("Trop petit pour toi (bureau du boss 3 niveaux sous le tien)", "Too small for you (boss's office 3 levels below yours)"), fr).to_owned())
            } else if empty {
                Some(tx(("Ton armée est vide", "Your army is empty"), fr).to_owned())
            } else {
                None
            };
            if let Some(why) = &why {
                ui.label(egui::RichText::new(why).size(12.5).color(theme.ansi[3]));
            } else if world.me.shield_until > nowms {
                ui.label(egui::RichText::new(tx(("Attaquer un joueur fait tomber ton bouclier.", "Attacking a player drops your shield."), fr)).size(12.0).color(theme.text_muted));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if action(ui, why.is_none(), format!("⚔  {}", tx(("Attaquer", "Attack"), fr)), red, theme) {
                    asks.push(Ask::Base(p.id.clone()));
                }
                let invitable = mine_guild.is_some() && !same;
                if invitable && action(ui, true, format!("✉  {}", tx(("Inviter dans ma guilde", "Invite to my guild"), fr)), theme.accent, theme) {
                    asks.push(Ask::Invite(p.id.clone()));
                }
            });
        }
        Pick::Bot(id) => {
            let Some(b) = world.bots.iter().find(|b| &b.id == id) else { return true };
            title(ui, format!("[{}] {}", b.guild.tag, b.name), &mut close);
            let (coffee, budget) = war::reward(b.level);
            ui.label(egui::RichText::new(format!("{} {} · 🏆 {} · 🤖", tx(("Bureau du boss niveau", "Boss's office level"), fr), b.hq, b.trophies)).size(13.0).color(theme.text_muted));
            ui.label(egui::RichText::new(format!("{} {} · {}", tx(("Butin jusqu'à", "Loot up to"), fr), os::amount_text(coffee as f64 * 0.5, os::Res::Coffee, fr), os::amount_text(budget as f64 * 0.5, os::Res::Budget, fr))).size(12.5).color(theme.text_muted));
            let why = if b.ready_at > nowms {
                Some(format!("🔧 {} {}", tx(("Se reconstruit encore", "Rebuilding for"), fr), left(b.ready_at, fr)))
            } else if empty {
                Some(tx(("Ton armée est vide", "Your army is empty"), fr).to_owned())
            } else {
                None
            };
            if let Some(why) = &why {
                ui.label(egui::RichText::new(why).size(12.5).color(theme.ansi[3]));
            }
            ui.add_space(6.0);
            if action(ui, why.is_none(), format!("⚔  {}", tx(("Attaquer", "Attack"), fr)), red, theme) {
                asks.push(Ask::Base(b.id.clone()));
            }
        }
        Pick::Quest(id) => {
            let Some(q) = world.quests.iter().find(|q| &q.id == id) else { return true };
            let (name, text) = w::quest_text(&q.kind);
            title(ui, format!("{} {}", quest_icon(&q.kind).0, tx(name, fr)), &mut close);
            ui.label(egui::RichText::new(tx(text, fr)).size(12.5).color(theme.text_muted));
            ui.label(egui::RichText::new(format!("{} {}", tx(("Récompense :", "Reward:"), fr), reward_text(&q.reward, fr))).size(13.0).color(theme.text));
            ui.add_space(6.0);
            if q.done {
                ui.label(egui::RichText::new(tx(("✔ Faite", "✔ Done"), fr)).color(theme.ansi[2]));
            } else if q.raid {
                if action(ui, !empty, format!("⚔  {}", tx(("Y aller", "Go"), fr)), red, theme) {
                    asks.push(Ask::Quest { id: q.id.clone(), stars: 0 });
                }
            } else if action(ui, true, format!("🧭  {}", tx(("Aller les chercher", "Go and get them"), fr)), theme.accent, theme) {
                asks.push(Ask::Quest { id: q.id.clone(), stars: 0 });
                close = true;
            }
        }
        Pick::Boss => {
            let Some(b) = &world.boss else { return true };
            title(ui, format!("🏛 {}", tx(("Cabinet d'audit", "Audit firm"), fr)), &mut close);
            ui.label(egui::RichText::new(tx(w::event_text("audit").1, fr)).size(12.5).color(theme.text_muted));
            ui.label(egui::RichText::new(format!("{} {} / {}", tx(("Résistance", "Resistance"), fr), b.hp, b.max)).size(13.0).color(theme.text));
            ui.add_space(6.0);
            if b.hp <= 0 {
                if b.hit && !b.claimed {
                    if action(ui, true, format!("🎫  {} {}", tx(("Prendre ta part :", "Take your share:"), fr), b.reward), theme.accent, theme) {
                        asks.push(Ask::EventClaim);
                    }
                } else {
                    ui.label(egui::RichText::new(if b.claimed { tx(("✔ Récompense prise", "✔ Reward taken"), fr) } else { tx(("Tombé sans toi cette fois.", "Brought down without you this time."), fr) }).color(theme.text_muted));
                }
            } else if b.ready_at > nowms {
                ui.label(egui::RichText::new(format!("{} {}", tx(("Ton prochain raid dans", "Your next raid in"), fr), left(b.ready_at, fr))).color(theme.ansi[3]));
            } else if action(ui, !empty, format!("⚔  {}", tx(("Attaquer", "Attack"), fr)), red, theme) {
                asks.push(Ask::Base("boss".into()));
            }
        }
    }
    close
}

fn reward_text(r: &w::Reward, fr: bool) -> String {
    let mut parts = Vec::new();
    if r.coffee >= 1.0 {
        parts.push(os::amount_text(r.coffee, os::Res::Coffee, fr));
    }
    if r.budget >= 1.0 {
        parts.push(os::amount_text(r.budget, os::Res::Budget, fr));
    }
    if r.tickets > 0 {
        parts.push(format!("{} 🎫", r.tickets));
    }
    for (t, n) in w::troops(&r.troops) {
        parts.push(format!("{n} {}", tx(t.name(), fr)));
    }
    parts.join(" · ")
}

/// The quests panel: the event, the day's quests.
fn quests_ui(ui: &mut Ui, world: &World, fr: bool, theme: &Theme, go: &mut Option<Pick>, asks: &mut Vec<Ask>) {
    ui.label(egui::RichText::new(tx(("Quêtes et événements", "Quests and events"), fr)).size(17.0).strong().color(theme.text));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        heading(ui, tx(("ÉVÉNEMENT", "EVENT"), fr), theme);
        match &world.event {
            Some(e) => {
                let (n, d) = w::event_text(&e.id);
                ui.label(egui::RichText::new(format!("✨ {}", tx(n, fr))).size(15.0).strong().color(Color32::from_rgb(255, 210, 110)));
                ui.label(egui::RichText::new(tx(d, fr)).size(12.5).color(theme.text));
                ui.label(egui::RichText::new(format!("{} {}", tx(("Fin dans", "Ends in"), fr), left(e.ends, fr))).size(12.0).color(theme.text_muted));
                if let Some(b) = &world.boss {
                    let f = (b.hp as f32 / b.max.max(1) as f32).clamp(0.0, 1.0);
                    ui.add(egui::ProgressBar::new(f).text(format!("{} / {}", b.hp, b.max)).fill(Color32::from_rgb(200, 60, 60)));
                    ui.horizontal(|ui| {
                        if ui.button(tx(("Voir le cabinet d'audit", "See the audit firm"), fr)).clicked() {
                            *go = Some(Pick::Boss);
                        }
                        if b.hp <= 0 && b.hit && !b.claimed && ui.button(format!("🎫 +{}", b.reward)).clicked() {
                            asks.push(Ask::EventClaim);
                        }
                    });
                }
            }
            None => {
                let (n, d) = w::event_text(&world.next_event.id);
                ui.label(egui::RichText::new(format!("{} {} {}", tx(n, fr), tx(("dans", "in"), fr), left(world.next_event.starts, fr))).size(14.0).strong().color(theme.text));
                ui.label(egui::RichText::new(tx(d, fr)).size(12.5).color(theme.text_muted));
            }
        }
        heading(ui, tx(("QUÊTES DU JOUR", "TODAY'S QUESTS"), fr), theme);
        ui.label(egui::RichText::new(tx(("Trois nouvelles chaque jour, sur la carte (les « ! »).", "Three new ones each day, on the map (the « ! »)."), fr)).size(12.0).color(theme.text_muted));
        for q in &world.quests {
            ui.add_space(4.0);
            let (name, text) = w::quest_text(&q.kind);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{} {}", quest_icon(&q.kind).0, tx(name, fr))).size(14.0).strong().color(if q.done { theme.text_muted } else { theme.text }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if q.done {
                        ui.label(egui::RichText::new("✔").color(theme.ansi[2]));
                    } else if ui.button(tx(("Voir", "Show"), fr)).clicked() {
                        *go = Some(Pick::Quest(q.id.clone()));
                    }
                });
            });
            ui.label(egui::RichText::new(tx(text, fr)).size(12.0).color(theme.text_muted));
            ui.label(egui::RichText::new(reward_text(&q.reward, fr)).size(12.0).color(theme.text));
        }
        if let Some(q) = world.quests.first() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("{} {}", tx(("Nouvelles quêtes dans", "New quests in"), fr), left(q.ends, fr))).size(12.0).color(theme.text_muted));
        }
    });
}

/// The ranking: the players, the guilds.
fn board_ui(ui: &mut Ui, page: &mut WorldPage, fr: bool, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tx(("Classement", "Ranking"), fr)).size(17.0).strong().color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.selectable_value(&mut page.board_guilds, true, tx(("Guildes", "Guilds"), fr));
            ui.selectable_value(&mut page.board_guilds, false, tx(("Joueurs", "Players"), fr));
        });
    });
    let Some(board) = &page.board else {
        ui.spinner();
        return;
    };
    let gold = Color32::from_rgb(255, 196, 64);
    let medal = |rank: u32| match rank {
        1 => Color32::from_rgb(255, 196, 64),
        2 => Color32::from_rgb(200, 205, 215),
        3 => Color32::from_rgb(205, 127, 50),
        _ => theme.text_muted,
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if page.board_guilds {
            if board.guilds.is_empty() {
                ui.label(egui::RichText::new(tx(("Pas encore de guilde : crée la première !", "No guild yet: make the first one!"), fr)).color(theme.text_muted));
            }
            for g in &board.guilds {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(r.shrink2(Vec2::new(0.0, 1.5)), 6.0, if g.mine { theme.accent.gamma_multiply(0.25) } else { theme.tab_hover.gamma_multiply(0.5) });
                p.text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, format!("{}", g.rank), FontId::proportional(14.0), medal(g.rank));
                p.rect_filled(Rect::from_center_size(Pos2::new(r.min.x + 42.0, r.center().y), Vec2::new(10.0, 18.0)), 2.0, rgb(g.color));
                p.text(Pos2::new(r.min.x + 54.0, r.center().y), Align2::LEFT_CENTER, format!("[{}] {}", g.tag, g.name), FontId::proportional(13.0), theme.text);
                p.text(Pos2::new(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, format!("🏆 {}", g.trophies), FontId::proportional(13.0), gold);
                p.text(Pos2::new(r.max.x - 90.0, r.center().y), Align2::RIGHT_CENTER, format!("👥 {}", g.members), FontId::proportional(12.0), theme.text_muted);
            }
            return;
        }
        let row = |ui: &mut Ui, p: &w::BoardPlayer| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
            let pt = ui.painter();
            pt.rect_filled(r.shrink2(Vec2::new(0.0, 1.5)), 6.0, if p.me { theme.accent.gamma_multiply(0.25) } else { theme.tab_hover.gamma_multiply(0.5) });
            pt.text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, format!("{}", p.rank), FontId::proportional(14.0), medal(p.rank));
            pt.text(Pos2::new(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, format!("{}{}", guild_label(&p.guild), p.name.clone().unwrap_or_default()), FontId::proportional(13.0), theme.text);
            pt.text(Pos2::new(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, format!("🏆 {}", p.trophies), FontId::proportional(13.0), gold);
            pt.text(Pos2::new(r.max.x - 90.0, r.center().y), Align2::RIGHT_CENTER, format!("{} {}", tx(("QG", "HQ"), fr), p.hq), FontId::proportional(12.0), theme.text_muted);
        };
        for p in &board.players {
            row(ui, p);
        }
        if let Some(me) = board.me.as_ref().filter(|m| !board.players.iter().any(|p| p.id == m.id)) {
            ui.label("…");
            row(ui, me);
        }
    });
}

/// The guild: none (invitations, join, make one), or its members, quests, war and troops given.
#[allow(clippy::too_many_arguments)]
fn guild_ui(ui: &mut Ui, page: &mut WorldPage, world: &World, s: &OpenSpace, fr: bool, theme: &Theme, asks: &mut Vec<Ask>) {
    let gold = Color32::from_rgb(255, 196, 64);
    let Some(known) = page.guild.clone() else {
        ui.label(egui::RichText::new(tx(("Guilde", "Guild"), fr)).size(17.0).strong().color(theme.text));
        ui.spinner();
        return;
    };
    let Some(g) = known else {
        ui.label(egui::RichText::new(tx(("Guilde", "Guild"), fr)).size(17.0).strong().color(theme.text));
        ui.label(egui::RichText::new(tx(("Rejoins une guilde avec tes collègues : des renforts en troupes, des quêtes chaque semaine et une guerre contre une autre guilde. Les places libres sont prises par des bots.", "Join a guild with your colleagues: troops as reinforcements, quests each week and a war against another guild. The free seats are taken by bots."), fr)).size(12.5).color(theme.text_muted));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if !world.invites.is_empty() {
                heading(ui, tx(("INVITATIONS", "INVITATIONS"), fr), theme);
                for i in &world.invites {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("[{}] {} · 👥 {}", i.guild.tag, i.guild.name, i.guild.members)).size(13.5).color(theme.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(tx(("Refuser", "Decline"), fr)).clicked() {
                                asks.push(Ask::Decline(i.guild.id.clone()));
                            }
                            if ui.add(egui::Button::new(egui::RichText::new(tx(("Rejoindre", "Join"), fr)).color(theme.bg)).fill(theme.accent)).clicked() {
                                asks.push(Ask::Join(i.guild.id.clone()));
                            }
                        });
                    });
                    if let Some(from) = &i.from {
                        ui.label(egui::RichText::new(format!("{} {from}", tx(("Invité par", "Invited by"), fr))).size(11.5).color(theme.text_muted));
                    }
                }
            }
            heading(ui, tx(("CRÉER UNE GUILDE", "MAKE A GUILD"), fr), theme);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut page.new_name).hint_text(tx(("Nom (3 à 24)", "Name (3 to 24)"), fr)).desired_width(200.0).char_limit(24));
                ui.add(egui::TextEdit::singleline(&mut page.new_tag).hint_text(tx(("Sigle", "Tag"), fr)).desired_width(60.0).char_limit(4));
            });
            page.new_tag = page.new_tag.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            ui.checkbox(&mut page.new_closed, tx(("Sur invitation seulement", "By invitation only"), fr));
            let ok = page.new_name.trim().chars().count() >= 3 && (2..=4).contains(&page.new_tag.len());
            if ui.add_enabled(ok, egui::Button::new(egui::RichText::new(tx(("Créer", "Make it"), fr)).color(theme.bg)).fill(theme.accent)).clicked() {
                asks.push(Ask::Create { name: page.new_name.trim().to_owned(), tag: page.new_tag.clone(), open: !page.new_closed });
            }
            heading(ui, tx(("REJOINDRE UNE GUILDE", "JOIN A GUILD"), fr), theme);
            match &page.guilds {
                None => {
                    ui.spinner();
                }
                Some(list) if list.is_empty() => {
                    ui.label(egui::RichText::new(tx(("Aucune guilde pour l'instant.", "No guild yet."), fr)).color(theme.text_muted));
                }
                Some(list) => {
                    for row in list {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("[{}] {}", row.tag, row.name)).size(13.5).color(theme.text));
                            ui.label(egui::RichText::new(format!("👥 {} · 🏆 {}", row.members, row.trophies)).size(12.0).color(theme.text_muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if row.open || row.invited {
                                    if ui.button(tx(("Rejoindre", "Join"), fr)).clicked() {
                                        asks.push(Ask::Join(row.id.clone()));
                                    }
                                } else {
                                    ui.label(egui::RichText::new(tx(("🔒 sur invitation", "🔒 invitation only"), fr)).size(11.5).color(theme.text_muted));
                                }
                            });
                        });
                    }
                }
            }
        });
        return;
    };

    // Its head.
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 22.0), Sense::hover());
        ui.painter().rect_filled(r, 3.0, rgb(g.guild.color));
        ui.label(egui::RichText::new(format!("[{}] {}", g.guild.tag, g.guild.name)).size(17.0).strong().color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("🏆 {}", g.guild.trophies)).size(13.0).color(gold));
        });
    });
    ui.horizontal(|ui| {
        if g.guild.leader {
            let mut open = g.guild.open;
            if ui.checkbox(&mut open, tx(("Ouverte à tous", "Open to all"), fr)).changed() {
                asks.push(Ask::Open(open));
            }
        } else {
            ui.label(egui::RichText::new(if g.guild.open { tx(("Ouverte à tous", "Open to all"), fr) } else { tx(("Sur invitation", "By invitation"), fr) }).size(12.0).color(theme.text_muted));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if page.leaving {
                if ui.add(egui::Button::new(egui::RichText::new(tx(("Oui, quitter", "Yes, leave"), fr)).color(Color32::WHITE)).fill(Color32::from_rgb(170, 50, 50))).clicked() {
                    asks.push(Ask::Leave);
                }
                if ui.button(tx(("Non", "No"), fr)).clicked() {
                    page.leaving = false;
                }
            } else if ui.button(tx(("Quitter", "Leave"), fr)).clicked() {
                page.leaving = true;
            }
        });
    });
    let gifts = g.gift.is_some() || !g.donations.is_empty();
    let to_claim = g.quests.iter().any(|q| q.complete && !q.claimed);
    ui.horizontal(|ui| {
        for (tab, label, dot) in [(GuildTab::Members, tx(("Membres", "Members"), fr), false), (GuildTab::Quests, tx(("Quêtes", "Quests"), fr), to_claim), (GuildTab::War, tx(("Guerre", "War"), fr), g.last_war.as_ref().is_some_and(|l| !l.claimed && l.reward.unwrap_or(0) > 0)), (GuildTab::Gifts, tx(("Renforts", "Reinforcements"), fr), gifts)] {
            let text = if dot { format!("{label} •") } else { label.to_owned() };
            ui.selectable_value(&mut page.guild_tab, tab, text);
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match page.guild_tab {
        GuildTab::Members => {
            for m in &g.members {
                ui.horizontal(|ui| {
                    let badge = if m.bot { "🤖 " } else if m.role == "leader" { "👑 " } else { "" };
                    ui.label(egui::RichText::new(format!("{badge}{}", m.name.clone().unwrap_or_default())).size(13.5).color(if m.me { gold } else { theme.text }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if g.guild.leader && !m.bot && !m.me && ui.small_button(tx(("Exclure", "Remove"), fr)).clicked() {
                            asks.push(Ask::Kick(m.id.clone()));
                        }
                        ui.label(egui::RichText::new(format!("{} {} · 🏆 {}", tx(("QG", "HQ"), fr), m.hq, m.trophies)).size(12.0).color(theme.text_muted));
                    });
                });
            }
            heading(ui, tx(("INVITER DES JOUEURS", "INVITE PLAYERS"), fr), theme);
            let others: Vec<&w::Player> = world.players.iter().filter(|p| !p.me && p.guild.as_ref().is_none_or(|x| x.id != g.guild.id)).collect();
            if others.is_empty() {
                ui.label(egui::RichText::new(tx(("Personne d'autre dans le monde pour l'instant.", "Nobody else in the world yet."), fr)).size(12.0).color(theme.text_muted));
            }
            for p in others {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{}{}", guild_label(&p.guild), p.name.clone().unwrap_or_default())).size(13.0).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if g.invites.iter().any(|i| i.id == p.id) {
                            ui.label(egui::RichText::new(tx(("invité", "invited"), fr)).size(11.5).color(theme.text_muted));
                        } else if ui.small_button(tx(("Inviter", "Invite"), fr)).clicked() {
                            asks.push(Ask::Invite(p.id.clone()));
                        }
                    });
                });
            }
        }
        GuildTab::Quests => {
            ui.label(egui::RichText::new(tx(("Chaque semaine, trois objectifs pour toute la guilde (les bots aident aussi). Chacun prend sa récompense.", "Each week, three goals for the whole guild (the bots help too). Everyone takes their reward."), fr)).size(12.0).color(theme.text_muted));
            if world.event.as_ref().is_some_and(|e| e.id == "seminaire") {
                ui.label(egui::RichText::new(tx(("✨ Séminaire d'équipe : ce que tu fais compte double !", "✨ Team off-site: what you do counts double!"), fr)).size(12.5).color(gold));
            }
            for q in &g.quests {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tx(w::guild_quest_text(&q.id), fr).replace("{n}", &q.target.to_string())).size(13.5).color(theme.text));
                ui.add(egui::ProgressBar::new(q.done as f32 / q.target.max(1) as f32).text(format!("{} / {}", q.done, q.target)).fill(if q.complete { theme.ansi[2] } else { theme.accent }));
                if q.claimed {
                    ui.label(egui::RichText::new(tx(("✔ Récompense prise", "✔ Reward taken"), fr)).size(12.0).color(theme.text_muted));
                } else if q.complete && ui.add(egui::Button::new(egui::RichText::new(format!("🎫 +{}", q.reward)).color(theme.bg)).fill(theme.accent)).clicked() {
                    asks.push(Ask::Claim(q.claim.clone()));
                }
            }
        }
        GuildTab::War => {
            let war_ui = |ui: &mut Ui, war: &w::War| {
                let total = (war.us.score + war.them.score).max(1) as f32;
                for (side, ours) in [(&war.us, true), (&war.them, false)] {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("[{}] {}{}", side.tag, side.name, if side.bot { " 🤖" } else { "" })).size(13.5).strong().color(if ours { gold } else { theme.text }));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("★ {}", side.score)).size(14.0).strong().color(theme.text));
                        });
                    });
                    ui.add(egui::ProgressBar::new(side.score as f32 / total).fill(if ours { theme.accent } else { Color32::from_rgb(190, 60, 50) }).desired_height(8.0));
                }
            };
            ui.label(egui::RichText::new(format!("{} — {} {}", tx(("Guerre de la semaine", "This week's war"), fr), tx(("fin dans", "ends in"), fr), left(g.war.ends, fr))).size(14.0).strong().color(theme.text));
            ui.label(egui::RichText::new(tx(("Chaque étoile gagnée en raid dans le monde par un membre compte pour la guilde.", "Every star a member wins raiding in the world counts for the guild."), fr)).size(12.0).color(theme.text_muted));
            ui.add_space(4.0);
            war_ui(ui, &g.war);
            if let Some(last) = &g.last_war {
                heading(ui, tx(("LA SEMAINE DERNIÈRE", "LAST WEEK"), fr), theme);
                let result = match last.result.as_deref() {
                    Some("win") => tx(("Victoire !", "Victory!"), fr),
                    Some("loss") => tx(("Défaite", "Defeat"), fr),
                    _ => tx(("Égalité", "Draw"), fr),
                };
                ui.label(egui::RichText::new(result).size(15.0).strong().color(if last.result.as_deref() == Some("win") { gold } else { theme.text }));
                war_ui(ui, last);
                if let (Some(claim), Some(reward)) = (&last.claim, last.reward) {
                    if last.claimed {
                        ui.label(egui::RichText::new(tx(("✔ Récompense prise", "✔ Reward taken"), fr)).size(12.0).color(theme.text_muted));
                    } else if ui.add(egui::Button::new(egui::RichText::new(format!("🎫 +{reward}")).color(theme.bg)).fill(theme.accent)).clicked() {
                        asks.push(Ask::Claim(claim.clone()));
                    }
                }
            }
        }
        GuildTab::Gifts => {
            heading(ui, tx(("REÇUS", "RECEIVED"), fr), theme);
            let mut any = false;
            for d in &g.donations {
                any = true;
                let list: Vec<String> = w::troops(&d.troops).iter().map(|(t, n)| format!("{n} {}", tx(t.name(), fr))).collect();
                ui.label(egui::RichText::new(format!("{} : {}", d.from.clone().unwrap_or_default(), list.join(", "))).size(13.0).color(theme.text));
            }
            if let Some(gift) = &g.gift {
                any = true;
                let list: Vec<String> = w::troops(&gift.troops).iter().map(|(t, n)| format!("{n} {}", tx(t.name(), fr))).collect();
                ui.label(egui::RichText::new(format!("🤖 {} : {}", gift.from, list.join(", "))).size(13.0).color(theme.text));
            }
            if any {
                if ui.add(egui::Button::new(egui::RichText::new(tx(("Faire entrer les renforts", "Let the reinforcements in"), fr)).color(theme.bg)).fill(theme.accent)).clicked() {
                    asks.push(Ask::TakeDonations);
                }
            } else {
                ui.label(egui::RichText::new(format!("{} {}", tx(("Rien pour l'instant. Prochains renforts des bots dans", "Nothing yet. The bots' next reinforcements in"), fr), left(g.next_gift, fr))).size(12.0).color(theme.text_muted));
            }
            heading(ui, tx(("DONNER DES TROUPES", "GIVE TROOPS"), fr), theme);
            let mates: Vec<&w::Member> = g.members.iter().filter(|m| !m.me).collect();
            if page.give_to.as_ref().is_none_or(|id| !mates.iter().any(|m| &m.id == id)) {
                page.give_to = mates.iter().find(|m| !m.bot).or(mates.first()).map(|m| m.id.clone());
            }
            let name_of = |id: &Option<String>| mates.iter().find(|m| Some(&m.id) == id.as_ref()).map(|m| format!("{}{}", if m.bot { "🤖 " } else { "" }, m.name.clone().unwrap_or_default())).unwrap_or_default();
            egui::ComboBox::from_id_salt("os-give-to").selected_text(name_of(&page.give_to)).show_ui(ui, |ui| {
                for m in &mates {
                    ui.selectable_value(&mut page.give_to, Some(m.id.clone()), format!("{}{}", if m.bot { "🤖 " } else { "" }, m.name.clone().unwrap_or_default()));
                }
            });
            let left_today = GIVE_DAY.saturating_sub(g.donated_today);
            let total: u32 = page.give.iter().map(|(_, n)| n).sum();
            for (t, have) in s.army.clone() {
                let n = page.give.iter().find(|(k, _)| *k == t).map_or(0, |(_, n)| *n);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::hover());
                    super::openspace_raid::portrait(ui.painter(), t, r, 0.0);
                    ui.label(egui::RichText::new(format!("{} ({have})", tx(t.name(), fr))).size(13.0).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let more = n < have && total < GIVE_MAX.min(left_today);
                        if ui.add_enabled(more, egui::Button::new("+")).clicked() {
                            match page.give.iter_mut().find(|(k, _)| *k == t) {
                                Some(slot) => slot.1 += 1,
                                None => page.give.push((t, 1)),
                            }
                        }
                        ui.label(egui::RichText::new(n.to_string()).size(13.0).strong());
                        if ui.add_enabled(n > 0, egui::Button::new("−")).clicked()
                            && let Some(slot) = page.give.iter_mut().find(|(k, _)| *k == t)
                        {
                            slot.1 -= 1;
                        }
                    });
                });
            }
            page.give.retain(|(_, n)| *n > 0);
            ui.label(egui::RichText::new(format!("{} {GIVE_MAX} {} · {} {left_today} {}", tx(("Jusqu'à", "Up to"), fr), tx(("à la fois", "at once"), fr), tx(("encore", "still"), fr), tx(("aujourd'hui", "today"), fr))).size(11.5).color(theme.text_muted));
            if let Some(to) = page.give_to.clone()
                && ui.add_enabled(total > 0, egui::Button::new(egui::RichText::new(format!("🎁 {} {total}", tx(("Donner", "Give"), fr))).color(theme.bg)).fill(theme.accent)).clicked()
            {
                asks.push(Ask::Donate { to, troops: page.give.clone() });
            }
        }
    });
}
