//! Ronnie.io, in the games: the world floor runs for everyone (see blob.rs), drawn around the player.
//! The pointer steers, Space splits, W ejects some mass, E lays a mine; watching between two lives, with the skin
//! picked and the players online to invite.
//!
//! floor's frames come 20 times a second, a little late: the player's own cells are drawn where they
//! are going (toward the pointer, by floor's delay), the others carried on at their speed, both eased
//! toward floor's places.

use std::collections::HashMap;
use std::time::Instant;

use super::motion::{out_back, out_cubic};
use super::*;
use crate::blob::{FX_BOUNTY, FX_GHOST, FX_MAGNET, FX_PAUSED, FX_SHIELD, FX_SPEED, GOLD};

/// A cell as drawn.
#[derive(Clone, Copy)]
struct Shown {
    pos: Pos2,
    radius: f32,
}

#[derive(Default)]
pub(super) struct Blob {
    online: crate::blob::Online,
    shown: HashMap<u32, Shown>,
    /// floor's places at the last frame, and the cells' speeds from the two last frames.
    places: HashMap<u32, Pos2>,
    speeds: HashMap<u32, Vec2>,
    /// The last frame taken (when it came).
    frame_at: Option<Instant>,
    /// The camera: the center, and the height of the world shown (eased).
    cam: Option<(Pos2, f32)>,
    last: Option<f64>,
    /// When the pointer was last sent, and where; where it points now (in the world).
    aimed: Option<(f64, Pos2)>,
    aim: Option<Pos2>,
    /// When the page was last drawn: out of sight a moment, the connection closes.
    seen: f64,
    /// When the last life ended.
    died_at: f64,
    /// A bonus just taken, since then (it pops in the middle).
    flash: Option<(String, f64)>,
    sound: Option<Sound>,
    /// The invitation (sent) already followed to the game.
    followed: Option<String>,
    /// The chat, with two players or more connected.
    chat: super::chat::Chat,
}

/// A cell's radius, from its mass (as floor's).
fn radius(mass: f32) -> f32 {
    4.0 + mass.sqrt() * 6.0
}

/// A cell's speed, from its mass (as floor's).
fn speed(mass: f32) -> f32 {
    420.0 * mass.powf(-0.24)
}

/// The players' colors, from the theme.
fn hue(theme: &Theme, k: u8) -> Color32 {
    const SLOTS: [usize; 12] = [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14];
    theme.ansi[SLOTS[k as usize % SLOTS.len()]]
}

const PINK: Color32 = Color32::from_rgb(255, 105, 180);
const GOLDEN: Color32 = Color32::from_rgb(255, 200, 40);
const BOSS: Color32 = Color32::from_rgb(150, 20, 30);
const HOLE: Color32 = Color32::from_rgb(150, 90, 230);
const METEOR: Color32 = Color32::from_rgb(255, 110, 40);
/// The night: how dark, and how far around the player's cells it stays light (world units).
const NIGHT_DARK: f32 = 0.94;
const NIGHT_SIGHT: f32 = 280.0;

/// A cell's color: its player's, or its skin's own.
fn cell_color(theme: &Theme, k: u8, skin: &str) -> Color32 {
    match skin {
        "ronnie" => PINK,
        "boss" => BOSS,
        _ => hue(theme, k),
    }
}

/// "2 min 13 s".
fn duration(seconds: u32) -> String {
    match seconds {
        s if s < 60 => format!("{s} s"),
        s => format!("{} min {:02} s", s / 60, s % 60),
    }
}

fn skin_name(t: &Strings, id: &str) -> &'static str {
    match id {
        "stripes" => t.blob_skin_stripes,
        "checker" => t.blob_skin_checker,
        "flames" => t.blob_skin_flames,
        "lightning" => t.blob_skin_lightning,
        "skull" => t.blob_skin_skull,
        "galaxy" => t.blob_skin_galaxy,
        "crown" => t.blob_skin_crown,
        "ronnie" => t.blob_skin_ronnie,
        _ => t.blob_skin_plain,
    }
}

fn bonus_name(t: &Strings, kind: &str) -> &'static str {
    match kind {
        "speed" => t.blob_fx_speed,
        "magnet" => t.blob_fx_magnet,
        "ghost" => t.blob_fx_ghost,
        "mine" => t.blob_fx_mine,
        _ => t.blob_fx_shield,
    }
}

/// An event's banner: its words and color (None: an event from a newer floor).
fn event_look(kind: &str, t: &Strings, theme: &Theme) -> Option<(&'static str, Color32)> {
    Some(match kind {
        "rain" => (t.blob_rain, GOLDEN),
        "boss" => (t.blob_boss, theme.ansi[1]),
        "zone" => (t.blob_zone, theme.ansi[1]),
        "hole" => (t.blob_hole, HOLE),
        "rush" => (t.blob_rush, bonus_color(bonus_kind("speed"), theme)),
        "night" => (t.blob_night, theme.ansi[4]),
        "meteors" => (t.blob_meteors, METEOR),
        "hill" => (t.blob_hill, theme.ansi[3]),
        "feast" => (t.blob_feast, theme.ansi[2]),
        _ => return None,
    })
}

/// A bonus's number on the wire, from its name.
fn bonus_kind(kind: &str) -> u8 {
    match kind {
        "speed" => 0,
        "magnet" => 1,
        "ghost" => 3,
        "mine" => 4,
        _ => 2,
    }
}

/// Who ate: a player's name, or the black hole.
fn eater<'a>(t: &Strings, by: &'a str) -> &'a str {
    if by == "hole" { t.blob_hole_name } else { by }
}

/// The game's little sounds, made on the spot: a gulp (the player ate someone), a rising chime (a bonus),
/// sparkles (Ronnie).
struct Sound {
    _device: rodio::MixerDeviceSink,
    mixer: rodio::mixer::Mixer,
}

#[derive(Clone, Copy)]
enum Sfx {
    Gulp,
    Bonus,
    Sparkle,
}

impl Sound {
    fn open() -> Option<Self> {
        let mut device = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| crate::log::error(&format!("Ronnie.io sound: {e}"))).ok()?;
        device.log_on_drop(false);
        let mixer = device.mixer().clone();
        Some(Self { _device: device, mixer })
    }

    fn play(&self, sfx: Sfx) {
        const RATE: u32 = 44_100;
        // Notes: start and end frequencies, length (seconds), loudness.
        let notes: &[(f32, f32, f32, f32)] = match sfx {
            Sfx::Gulp => &[(520.0, 160.0, 0.16, 0.5)],
            Sfx::Bonus => &[(660.0, 660.0, 0.07, 0.3), (880.0, 880.0, 0.07, 0.3), (1320.0, 1320.0, 0.12, 0.3)],
            Sfx::Sparkle => &[(1568.0, 1568.0, 0.06, 0.22), (2093.0, 2093.0, 0.06, 0.22), (2637.0, 2637.0, 0.06, 0.22), (3136.0, 3136.0, 0.18, 0.2)],
        };
        let mut samples = Vec::new();
        let mut phase = 0.0_f32;
        for &(from, to, length, loud) in notes {
            let n = (length * RATE as f32) as usize;
            for k in 0..n {
                let p = k as f32 / n as f32;
                let f = from + (to - from) * p;
                phase += f / RATE as f32 * std::f32::consts::TAU;
                // A quick attack, a soft end.
                let envelope = (p * 30.0).min(1.0) * (1.0 - p).powf(1.5);
                samples.push(phase.sin() * envelope * loud * 0.5);
            }
        }
        let (Some(channels), Some(rate)) = (std::num::NonZero::new(1), std::num::NonZero::new(RATE)) else { return };
        self.mixer.add(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
    }
}

impl App {
    /// Ronnie.io, on the home page.
    pub(super) fn open_blob(&mut self) {
        self.open_game();
        self.home_which = super::four::HomeGame::Blob;
    }

    /// At each frame: the page out of sight for a moment, the connection closes (a game under way goes
    /// on pause, floor keeps it 30 min; otherwise floor takes the player out); an invitation to Ronnie.io accepted by the other, to the game.
    pub(super) fn blob_frame(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.blob.online.connected() && now - self.blob.seen > 0.5 {
            if self.blob.online.alive() {
                self.blob.online.suspend();
            } else {
                self.blob.online.disconnect();
            }
            self.blob.shown.clear();
            self.blob.cam = None;
        }
        let accepted = self.versus.sent.as_ref().filter(|s| s.status == "accepted" && s.game.as_deref() == Some("ronnie-io")).map(|s| s.id.clone());
        if let Some(id) = accepted.filter(|id| self.blob.followed.as_ref() != Some(id)) {
            self.blob.followed = Some(id);
            self.open_blob();
        }
    }

    fn blob_sound(&mut self, sfx: Sfx) {
        if self.config.settings.game_sound == Some(false) {
            return;
        }
        if self.blob.sound.is_none() {
            self.blob.sound = Sound::open();
        }
        if let Some(sound) = &self.blob.sound {
            sound.play(sfx);
        }
    }

    pub(super) fn blob_page(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        let dt = self.blob.last.map_or(0.0, |l| (now - l) as f32).min(0.1);
        self.blob.last = Some(now);
        self.blob.seen = now;
        ui.ctx().request_repaint();
        let aspect = rect.width() / rect.height().max(1.0);
        let account = self.config.settings.floor.clone().filter(|_| crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some());
        // Connected while the page shows, with a pseudo.
        if let Some(account) = &account
            && self.blob.online.status == crate::blob::Status::Off
        {
            self.blob.online.connect(ui.ctx(), account, aspect);
        }
        let news = self.blob.online.poll();
        if let Some(death) = &news.death {
            self.blob.died_at = now;
            if death.best > self.config.settings.blob_best {
                self.config.settings.blob_best = death.best;
                self.save_config();
            }
        }
        if news.ate > 0 {
            self.blob_sound(Sfx::Gulp);
        }
        if let Some(kind) = news.bonus {
            self.blob_sound(Sfx::Bonus);
            self.blob.flash = Some((kind, now));
        }
        if news.ronnie {
            self.blob_sound(Sfx::Sparkle);
        }
        if news.badge {
            self.blob_sound(Sfx::Sparkle);
            self.config.settings.blob_skin = "ronnie".to_owned();
            self.save_config();
            self.toasts.push(super::Toast { ok: true, title: t.blob_badge.to_owned(), body: t.blob_badge_body.to_owned(), tab: self.active, at: Instant::now() });
        }
        // A goal reached: its skin announced (Ronnie's has its own toast).
        for id in news.unlocked.iter().filter(|id| *id != "ronnie") {
            self.toasts.push(super::Toast { ok: true, title: t.blob_unlocked.replace("{n}", skin_name(t, id)), body: t.blob_unlocked_body.to_owned(), tab: self.active, at: Instant::now() });
        }
        let alive = self.blob.online.alive();
        self.fold_for_game(alive);

        self.blob_world(ui, rect, dt, now, &theme);
        // The leaderboard, the kill feed, the events: playing or watching.
        self.blob_leaders(ui, rect, &theme, t);
        self.blob_feed(ui, rect, alive, &theme, t);
        self.blob_banner(ui, rect, now, &theme, t);
        let paused = self.blob.online.paused();
        // The chat: with someone else connected (or once something was said).
        let chat = self.blob.online.board.as_ref().is_some_and(|b| b.online >= 2) || !self.blob.online.chat.is_empty();
        if alive && !paused {
            self.blob_controls(ui, rect, now, aspect, chat);
        }
        if alive {
            self.blob_hud(ui, rect, now, &theme, t);
        }
        if paused {
            self.blob_paused(ui, rect, &theme, t);
        }
        let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
        if !alive && ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).fill(theme.chrome_bg.gamma_multiply(0.8)).corner_radius(6.0)).clicked() {
            self.home_game = false;
            self.blob.online.disconnect();
        }
        let invites = if alive { None } else { self.blob_card(ui, rect, now, account.is_some(), &theme, t) };
        if chat {
            self.blob_chat(ui, rect, alive, invites, &theme, t);
        }
    }

    /// The messages top left; the field while watching, or opened by Enter while playing (it closes
    /// once sent, back to the game). Between two lives, under the players to invite (`invites`) when
    /// there is room, not hidden behind them.
    fn blob_chat(&mut self, ui: &mut Ui, rect: Rect, alive: bool, invites: Option<Rect>, theme: &Theme, t: &Strings) {
        let mut area = Rect::from_min_size(Pos2::new(rect.min.x + 14.0, rect.min.y + 56.0), Vec2::new((rect.width() * 0.4).clamp(220.0, 340.0), 210.0));
        if let Some(invites) = invites {
            let top = invites.max.y + 12.0;
            let room = rect.max.y - 14.0 - top;
            if room >= 100.0 {
                area = Rect::from_min_size(Pos2::new(invites.min.x, top), Vec2::new(invites.width(), room.min(210.0)));
            }
        }
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref());
        let lines: Vec<super::chat::Line> = self.blob.online.chat.iter().map(|s| super::chat::Line { from: &s.from, text: &s.text, mine: pseudo == Some(s.from.as_str()), age: Some(s.at.elapsed().as_secs_f32()) }).collect();
        let sent = self.blob.chat.ui(ui, area, &lines, !alive, !alive, "blob", theme, t);
        if let Some(text) = sent {
            self.blob.online.say(&text);
        }
        if alive && !self.blob.chat.typing {
            ui.painter().text(Pos2::new(area.min.x + 2.0, area.max.y + 10.0), Align2::LEFT_CENTER, t.chat_open, FontId::proportional(11.5), theme.text_muted.gamma_multiply(0.7));
        }
    }

    /// The pointer (where the cells go), Space, W, E, P and Escape (pause), Enter (the chat, `chat`:
    /// when there is one). Not while typing in the chat.
    fn blob_controls(&mut self, ui: &mut Ui, rect: Rect, now: f64, aspect: f32, chat: bool) {
        let Some((cam, height)) = self.blob.cam else { return };
        let scale = rect.height() / height;
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()).filter(|p| rect.contains(*p)) {
            let world = cam + (p - rect.center()) / scale;
            self.blob.aim = Some(world);
            // Often enough to follow it, not more (and again now and then: the cells move under it).
            let due = self.blob.aimed.is_none_or(|(at, last)| now - at > 0.2 || (now - at > 0.03 && (last - world).length() > 2.0));
            if due {
                self.blob.online.aim(world.x, world.y, aspect);
                self.blob.aimed = Some((now, world));
            }
        }
        if self.blob.chat.typing {
            return;
        }
        if chat && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            self.blob.chat.open();
        }
        let (split, eject, pause, mine) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                i.key_pressed(egui::Key::W),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) | i.consume_key(egui::Modifiers::NONE, egui::Key::P),
                i.consume_key(egui::Modifiers::NONE, egui::Key::E),
            )
        });
        if mine {
            self.blob.online.mine();
        }
        if pause {
            self.blob.online.pause();
        }
        if split {
            self.blob.online.split();
        }
        if eject {
            self.blob.online.eject();
        }
        ui.interact(rect, ui.id().with("blob-world"), Sense::hover()).on_hover_cursor(egui::CursorIcon::Crosshair);
    }

    /// The game on pause: what it means, "Resume" (Enter, P, Escape), and leave the game (the mass counts).
    fn blob_paused(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) {
        let width = 380.0_f32.min(rect.width() - 32.0);
        let card = Rect::from_center_size(rect.center(), Vec2::new(width, 190.0));
        let painter = ui.painter().clone();
        painter.rect_filled(card.translate(Vec2::new(0.0, 6.0)), 16.0, Color32::from_black_alpha(60));
        painter.rect_filled(card, 16.0, theme.chrome_bg.gamma_multiply(0.94));
        painter.rect_stroke(card, 16.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.4)), egui::StrokeKind::Inside);
        let mid = card.center().x;
        paint_metal(&painter, Pos2::new(mid, card.min.y + 40.0), Align2::CENTER_CENTER, t.blob_paused, 30.0, theme.accent, 1.0);
        let body = painter.layout(t.blob_paused_body.to_owned(), FontId::proportional(13.0), theme.text, width - 48.0);
        painter.galley(Pos2::new(mid - body.size().x / 2.0, card.min.y + 70.0), body, theme.text);
        let y = card.max.y - 34.0;
        let w = ((width - 48.0 - 12.0) / 2.0).min(160.0);
        let resume_at = Rect::from_center_size(Pos2::new(mid + w / 2.0 + 6.0, y), Vec2::new(w, 36.0));
        let quit_at = Rect::from_center_size(Pos2::new(mid - w / 2.0 - 6.0, y), Vec2::new(w, 36.0));
        let resume = egui::Button::new(egui::RichText::new(t.blob_resume).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
        let quit = egui::Button::new(egui::RichText::new(t.blob_quit).size(13.0)).corner_radius(8.0);
        let typing = self.blob.chat.typing;
        let key = !typing && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) | i.consume_key(egui::Modifiers::NONE, egui::Key::P) | i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if ui.put(resume_at, resume).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() || key {
            self.blob.online.resume();
            self.blob.aimed = None;
        }
        if ui.put(quit_at, quit).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.blob.online.leave();
        }
    }

    /// The world around the camera: its grid, the pellets, the bonuses, the cells (the smaller under,
    /// with their skins and effects), the viruses.
    fn blob_world(&mut self, ui: &mut Ui, rect: Rect, dt: f32, now: f64, theme: &Theme) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.bg);
        let blob = &mut self.blob;
        let Some(frame) = blob.online.frame.as_ref() else {
            blob.shown.clear();
            return;
        };
        // A new frame: the cells' speeds, from where they were.
        if blob.frame_at != Some(frame.at) {
            let gap = blob.frame_at.map_or(0.05, |at| frame.at.duration_since(at).as_secs_f32()).max(0.01);
            let mut places = HashMap::with_capacity(frame.cells.len());
            let mut speeds = HashMap::with_capacity(frame.cells.len());
            for cell in &frame.cells {
                let at = Pos2::new(cell.x, cell.y);
                if let Some(before) = blob.places.get(&cell.id) {
                    let v = (at - *before) / gap;
                    speeds.insert(cell.id, if v.length() > 1500.0 { Vec2::ZERO } else { v });
                }
                places.insert(cell.id, at);
            }
            blob.places = places;
            blob.speeds = speeds;
            blob.frame_at = Some(frame.at);
        }
        // How old floor's places are: the player's cells are drawn ahead by floor's delay, toward the
        // pointer; the others carried on at their speed.
        let age = frame.at.elapsed().as_secs_f32().min(0.15);
        let lead = (blob.online.rtt / 2.0 + age).min(0.25);
        let mut shown = HashMap::with_capacity(frame.cells.len());
        for cell in &frame.cells {
            let place = Pos2::new(cell.x, cell.y);
            let r = radius(cell.mass);
            let before = blob.shown.get(&cell.id).copied();
            let mine = Some(cell.owner) == frame.me;
            let ahead = match (mine, blob.aim, before) {
                (true, Some(aim), Some(s)) => {
                    let to = aim - s.pos;
                    let d = to.length();
                    let fast = if cell.fx & FX_SPEED != 0 { 1.5 } else { 1.0 };
                    if d > 0.5 { to / d * speed(cell.mass) * fast * (d / r.max(30.0)).min(1.0) * lead } else { Vec2::ZERO }
                }
                _ => blob.speeds.get(&cell.id).copied().unwrap_or(Vec2::ZERO) * age,
            };
            let target = Shown { pos: place + ahead, radius: r };
            // A new cell from its player's nearest one (a split flies out of it).
            let from = before.or_else(|| {
                let siblings = frame.cells.iter().filter(|c| c.owner == cell.owner && c.id != cell.id);
                siblings.filter_map(|c| blob.shown.get(&c.id)).min_by(|a, b| a.pos.distance(place).total_cmp(&b.pos.distance(place))).map(|s| Shown { pos: s.pos, radius: r })
            });
            let ease = 1.0 - (-dt * if mine { 20.0 } else { 16.0 }).exp();
            let s = match from {
                Some(s) => Shown { pos: s.pos + (target.pos - s.pos) * ease, radius: s.radius + (target.radius - s.radius) * ease },
                None => target,
            };
            shown.insert(cell.id, s);
        }
        blob.shown = shown;

        // The camera on the player's cells (as drawn), or where floor looks.
        let mine: Vec<(Pos2, f32)> = frame.cells.iter().filter(|c| Some(c.owner) == frame.me).filter_map(|c| blob.shown.get(&c.id).map(|s| (s.pos, c.mass))).collect();
        let total: f32 = mine.iter().map(|(_, m)| m).sum();
        let target = if total > 0.0 { Pos2::ZERO + mine.iter().fold(Vec2::ZERO, |acc, (p, m)| acc + p.to_vec2() * *m) / total } else { Pos2::new(frame.x, frame.y) };
        let (cam, height) = match blob.cam {
            Some((cam, height)) => {
                let k = 1.0 - (-dt * 6.0).exp();
                (cam + (target - cam) * if total > 0.0 { 1.0 - (-dt * 18.0).exp() } else { k }, height + (frame.height - height) * k)
            }
            None => (target, frame.height),
        };
        blob.cam = Some((cam, height));
        let scale = rect.height() / height;
        let to_screen = |p: Pos2| rect.center() + (p - cam) * scale;

        // Outside the world, darker; the grid inside.
        let size = blob.online.size.max(1.0);
        let world = Rect::from_min_max(to_screen(Pos2::ZERO), to_screen(Pos2::new(size, size)));
        painter.rect_filled(rect, 0.0, super::sidebar::lerp_color(theme.bg, Color32::BLACK, 0.3));
        painter.rect_filled(world, 0.0, theme.bg);
        let grid = theme.text.gamma_multiply(0.05);
        let step = 50.0;
        let view = Rect::from_center_size(cam, rect.size() / scale).intersect(Rect::from_min_max(Pos2::ZERO, Pos2::new(size, size)));
        let mut x = (view.min.x / step).ceil() * step;
        while x <= view.max.x {
            let sx = to_screen(Pos2::new(x, 0.0)).x;
            painter.line_segment([Pos2::new(sx, world.min.y.max(rect.min.y)), Pos2::new(sx, world.max.y.min(rect.max.y))], Stroke::new(1.0, grid));
            x += step;
        }
        let mut y = (view.min.y / step).ceil() * step;
        while y <= view.max.y {
            let sy = to_screen(Pos2::new(0.0, y)).y;
            painter.line_segment([Pos2::new(world.min.x.max(rect.min.x), sy), Pos2::new(world.max.x.min(rect.max.x), sy)], Stroke::new(1.0, grid));
            y += step;
        }
        painter.rect_stroke(world, 0.0, Stroke::new(2.0, theme.accent.gamma_multiply(0.4)), egui::StrokeKind::Outside);

        let event = frame.event.as_ref().map(|(kind, _)| kind.as_str());
        // The feast: the pellets worth more, bigger.
        let feast = if event == Some("feast") { 1.6 } else { 1.0 };
        let seen = view.expand(30.0);
        for &(x, y, k) in blob.online.pellets.values() {
            if !seen.contains(Pos2::new(x, y)) {
                continue;
            }
            let at = to_screen(Pos2::new(x, y));
            if k == GOLD {
                let twinkle = 0.75 + 0.25 * ((now * 6.0 + f64::from(x)).sin() as f32);
                painter.circle_filled(at, (8.0 * scale).max(2.5), GOLDEN.gamma_multiply(twinkle));
                painter.circle_filled(at + Vec2::new(-2.0, -2.0) * scale, (2.5 * scale).max(1.0), Color32::WHITE.gamma_multiply(0.7));
            } else {
                painter.circle_filled(at, (5.0 * scale * feast).max(1.5), hue(theme, k));
            }
        }
        for &(x, y, k) in &frame.ejected {
            let r = radius(12.0) * 0.7 * scale;
            paint_jelly(&painter, to_screen(Pos2::new(x, y)), r, hue(theme, k), now, 0, 0.0);
        }
        for &(x, y, kind) in &frame.bonuses {
            paint_bonus(&painter, to_screen(Pos2::new(x, y)), (18.0 * scale).max(7.0), kind, now, theme);
        }
        for &(x, y, k) in &frame.mines {
            paint_mine(&painter, to_screen(Pos2::new(x, y)), (16.0 * scale).max(6.0), hue(theme, k), now, theme);
        }
        if let Some((x, y)) = frame.hole {
            paint_hole(&painter, to_screen(Pos2::new(x, y)), scale, now);
        }
        // The hill: a golden circle on the ground, its edge turning.
        if let Some((x, y, r)) = frame.hill {
            let at = to_screen(Pos2::new(x, y));
            let r = r * scale;
            let gold = theme.ansi[3];
            painter.circle_filled(at, r, gold.gamma_multiply(0.08 + 0.03 * ((now * 2.0).sin() as f32)));
            let turn = (now * 0.4) as f32;
            for k in 0..24 {
                let a = turn + k as f32 / 24.0 * std::f32::consts::TAU;
                let dir = Vec2::angled(a);
                painter.line_segment([at + dir * r, at + Vec2::angled(a + 0.13) * r], Stroke::new(3.0, gold.gamma_multiply(0.75)));
            }
        }

        // The cells, the smaller first; their skins and effects, their names on them, the masses of the
        // player's.
        let mut cells: Vec<_> = frame.cells.iter().filter_map(|c| blob.shown.get(&c.id).map(|s| (c, *s))).collect();
        cells.sort_by(|a, b| a.0.mass.total_cmp(&b.0.mass));
        // Each player's largest cell (the last one, smaller first): the name above it when it no longer fits inside.
        let largest: std::collections::HashMap<_, _> = cells.iter().map(|(c, _)| (c.owner, c.id)).collect();
        // The rush: everyone fast, everyone with the speed's trail.
        let rush = event == Some("rush");
        for (cell, s) in cells {
            let at = to_screen(s.pos);
            let r = s.radius * scale;
            if !rect.expand(r + 200.0 * scale).contains(at) {
                continue;
            }
            let (name, k, skin) = blob.online.names.get(&cell.owner).map_or(("", 0, "plain"), |(n, k, s)| (n.as_str(), *k, s.as_str()));
            // On pause: faded, out of the game for now; a ghost, see-through.
            let faded = if cell.fx & FX_PAUSED != 0 { 0.4 } else if cell.fx & FX_GHOST != 0 { 0.5 + 0.1 * ((now * 4.0).sin() as f32) } else { 1.0 };
            let color = cell_color(theme, k, skin).gamma_multiply(faded);
            if cell.fx & FX_MAGNET != 0 {
                let pull = (s.radius + 120.0 + s.radius * 0.3) * scale;
                paint_ring(&painter, at, pull, theme.ansi[4].gamma_multiply(0.35), now);
            }
            if cell.fx & FX_SPEED != 0 || rush {
                // Ghosts behind it.
                let back = blob.speeds.get(&cell.id).copied().unwrap_or(Vec2::ZERO) * scale;
                for k in 1..=3 {
                    painter.circle_filled(at - back * 0.04 * k as f32, r * (1.0 - 0.08 * k as f32), color.gamma_multiply(0.18 / k as f32));
                }
            }
            paint_jelly(&painter, at, r, color, now, cell.id, 0.025);
            paint_skin(&painter, at, r, color, skin, now, cell.id);
            if cell.fx & FX_SHIELD != 0 {
                let pulse = 0.6 + 0.4 * ((now * 5.0).sin() as f32);
                painter.circle_filled(at, r + 6.0, theme.ansi[6].gamma_multiply(0.10));
                painter.circle_stroke(at, r + 6.0, Stroke::new(2.5, theme.ansi[6].gamma_multiply(0.8 * pulse)));
            }
            if cell.fx & FX_BOUNTY != 0 {
                paint_crosshair(&painter, at, r + 10.0, GOLDEN, now);
            }
            if !name.is_empty() {
                // Readable however far the view is zoomed out.
                let size = (r * 0.36).clamp(11.0, 42.0);
                let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(size), Color32::WHITE);
                let inside = r > 14.0 && galley.size().x < r * 2.2;
                if inside {
                    let p = at - galley.size() / 2.0;
                    painter.galley(p + Vec2::new(1.0, 1.0), painter.layout_no_wrap(name.to_owned(), FontId::proportional(size), Color32::from_black_alpha(160)), Color32::BLACK);
                    painter.galley(p, galley, Color32::WHITE);
                } else if largest.get(&cell.owner) == Some(&cell.id) {
                    // Too small for it: above the cell.
                    let font = FontId::proportional(11.5);
                    let p = at - Vec2::new(0.0, r + 9.0);
                    painter.text(p + Vec2::new(1.0, 1.0), Align2::CENTER_CENTER, name, font.clone(), Color32::from_black_alpha(170));
                    painter.text(p, Align2::CENTER_CENTER, name, font, Color32::WHITE.gamma_multiply(0.92 * faded));
                }
                if inside && Some(cell.owner) == frame.me && r > 26.0 {
                    painter.text(at + Vec2::new(0.0, size * 0.85), Align2::CENTER_CENTER, format!("{}", cell.mass.round()), FontId::proportional(size * 0.55), Color32::WHITE.gamma_multiply(0.85));
                }
            }
        }

        // The viruses over the cells (the small ones hide under them).
        let virus = theme.ansi[2];
        for &(x, y) in &frame.viruses {
            let at = to_screen(Pos2::new(x, y));
            let r = radius(100.0) * scale;
            if rect.expand(r * 1.2).contains(at) {
                paint_virus(&painter, at, r, virus, now);
            }
        }

        // The zone: outside it darkened and red, its edge pulsing.
        if let Some((x, y, r)) = frame.zone {
            let at = to_screen(Pos2::new(x, y));
            let r = r * scale;
            // A ring wide enough to cover the screen around the circle.
            let far = rect.size().length() + (at - rect.center()).length();
            let width = far.max(r) + 10.0;
            painter.circle_stroke(at, r + width / 2.0, Stroke::new(width, theme.ansi[1].gamma_multiply(0.16)));
            let pulse = 0.6 + 0.4 * ((now * 4.0).sin() as f32);
            painter.circle_stroke(at, r, Stroke::new(3.0, theme.ansi[1].gamma_multiply(0.8 * pulse)));
        }

        // The night: dark but around the player's cells (around the middle while watching), coming
        // and going softly.
        let night = ui.ctx().animate_bool_with_time(ui.id().with("blob-night"), event == Some("night"), 1.5);
        if night > 0.0 {
            let (around, sight) = match frame.cells.iter().filter(|c| Some(c.owner) == frame.me).filter_map(|c| blob.shown.get(&c.id)).map(|s| (s.pos, s.radius)).collect::<Vec<_>>() {
                mine if !mine.is_empty() => {
                    let reach = mine.iter().map(|(p, r)| p.distance(cam) + r).fold(0.0, f32::max);
                    (to_screen(cam), (reach + NIGHT_SIGHT) * scale)
                }
                _ => (rect.center(), NIGHT_SIGHT * 1.5 * scale),
            };
            paint_night(&painter, rect, around, sight, NIGHT_DARK * night);
        }

        // The meteors: a circle on the ground filling up until they hit, then the blast.
        let late = frame.at.elapsed().as_secs_f32();
        for &(x, y, r, left) in &frame.meteors {
            let at = to_screen(Pos2::new(x, y));
            let r = r * scale;
            let left = left - late;
            if left > 0.0 {
                let k = (1.0 - left / 2.0).clamp(0.0, 1.0);
                painter.circle_filled(at, r, METEOR.gamma_multiply(0.08 + 0.17 * k));
                painter.circle_filled(at, r * k, METEOR.gamma_multiply(0.22));
                let blink = if left < 0.6 { 0.5 + 0.5 * ((now * 20.0).sin() as f32) } else { 1.0 };
                painter.circle_stroke(at, r, Stroke::new(2.5, METEOR.gamma_multiply(0.9 * blink)));
                // The rock coming down, from the top right.
                let rock = at + Vec2::new(1.0, -1.6) * left * 220.0 * scale.max(0.4);
                painter.line_segment([rock, rock + Vec2::new(1.0, -1.6) * 40.0 * scale.max(0.4)], Stroke::new(4.0, METEOR.gamma_multiply(0.5)));
                painter.circle_filled(rock, (12.0 * scale).max(4.0), METEOR);
            } else {
                let age = (-left / 0.6).clamp(0.0, 1.0);
                painter.circle_filled(at, r * (0.8 + 0.5 * age), METEOR.gamma_multiply(0.7 * (1.0 - age)));
                painter.circle_filled(at, r * 0.5 * (1.0 - age), Color32::from_rgb(255, 230, 160).gamma_multiply(1.0 - age));
            }
        }
    }

    /// The players in the world, the largest first (top right), as many as fit in the height; the
    /// player's place below when it is further down. The players who aren't bots in a color of their own.
    fn blob_leaders(&self, ui: &Ui, rect: Rect, theme: &Theme, t: &Strings) {
        let Some(board) = self.blob.online.board.as_ref().filter(|_| self.blob.online.frame.is_some()) else { return };
        let painter = ui.painter_at(rect);
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).unwrap_or(t.game_you);
        let mut lines: Vec<(u32, &str, u32, bool, bool)> = board.leaders.iter().enumerate().map(|(k, (name, mass, me, bot))| (k as u32 + 1, name.as_str(), *mass, *me, *bot)).collect();
        // The player's line when floor sent only the first ones.
        if let Some((rank, mass)) = board.me.filter(|(rank, _)| *rank as usize > board.leaders.len()) {
            lines.push((rank, pseudo, mass, true, false));
        }
        // Too many for the height: the first ones, then the player's line if it is further down.
        let room = (((rect.height() - 28.0 - 40.0 - 8.0) / 19.0).floor() as usize).max(3);
        if lines.len() > room {
            let mine = lines.iter().position(|l| l.3).filter(|&i| i >= room);
            let keep = if mine.is_some() { room - 1 } else { room };
            let mine = mine.map(|i| lines[i]);
            lines.truncate(keep);
            lines.extend(mine);
        }
        let gap = lines.windows(2).any(|w| w[1].0 > w[0].0 + 1);
        let w = 190.0;
        let h = 40.0 + lines.len() as f32 * 19.0 + if gap { 8.0 } else { 0.0 };
        let area = Rect::from_min_size(Pos2::new(rect.max.x - w - 14.0, rect.min.y + 14.0), Vec2::new(w, h));
        painter.rect_filled(area, 10.0, theme.chrome_bg.gamma_multiply(0.82));
        painter.text(Pos2::new(area.min.x + 12.0, area.min.y + 16.0), Align2::LEFT_CENTER, t.blob_leaders, FontId::proportional(13.0), theme.text);
        painter.text(Pos2::new(area.max.x - 12.0, area.min.y + 16.0), Align2::RIGHT_CENTER, t.four_online.replace("{n}", &board.online.to_string()), FontId::proportional(11.0), theme.text_muted);
        let mut y = area.min.y + 40.0;
        let mut last = 0;
        for (rank, name, mass, mine, bot) in lines {
            if rank > last + 1 {
                // Lines left out: a gap, then the player's line.
                painter.text(Pos2::new(area.center().x, y - 6.0), Align2::CENTER_CENTER, "···", FontId::proportional(11.0), theme.text_muted);
                y += 8.0;
            }
            last = rank;
            let color = if mine {
                theme.accent
            } else if name == "Ronnie" {
                PINK
            } else if bot {
                theme.text_muted
            } else {
                theme.ansi[6]
            };
            // A bot: a small badge after its name.
            let badge = if bot { 30.0 } else { 0.0 };
            let mut job = egui::text::LayoutJob::simple_singleline(format!("{rank}. {name}"), FontId::proportional(12.5), color);
            job.wrap = egui::text::TextWrapping::truncate_at_width(w - 70.0 - badge);
            let galley = painter.layout_job(job);
            let end = area.min.x + 12.0 + galley.size().x;
            painter.galley(Pos2::new(area.min.x + 12.0, y - 8.0), galley, color);
            if bot {
                let pill = Rect::from_min_size(Pos2::new(end + 5.0, y - 6.5), Vec2::new(25.0, 13.0));
                painter.rect_filled(pill, 4.0, theme.text_muted.gamma_multiply(0.18));
                painter.text(pill.center(), Align2::CENTER_CENTER, "BOT", FontId::proportional(8.5), theme.text_muted);
            }
            painter.text(Pos2::new(area.max.x - 12.0, y), Align2::RIGHT_CENTER, mass.to_string(), FontId::monospace(11.5), color);
            y += 19.0;
        }
    }

    /// Who ate whom, bottom left (above the mass while playing), the last ones fading.
    fn blob_feed(&self, ui: &Ui, rect: Rect, alive: bool, theme: &Theme, t: &Strings) {
        const LIFE: f32 = 6.0;
        let painter = ui.painter_at(rect);
        let me = self.blob.online.frame.as_ref().and_then(|f| f.me);
        let mut y = rect.max.y - if alive { 14.0 + 30.0 + 44.0 } else { 20.0 };
        for kill in self.blob.online.feed.iter().rev() {
            let age = kill.at.elapsed().as_secs_f32();
            if age > LIFE {
                continue;
            }
            let alpha = ((LIFE - age) / 0.8).min(1.0) * out_cubic(age / 0.25);
            // Ronnie and the boss eaten: in their colors, the line tinted.
            let special = match kill.special.as_deref() {
                Some("ronnie") => Some(PINK),
                Some("boss") => Some(theme.ansi[1]),
                Some("bounty") => Some(GOLDEN),
                Some("hole") => Some(HOLE),
                _ => None,
            };
            let name_color = |id: u32| if Some(id) == me { theme.accent } else { theme.text };
            let template = t.blob_ate;
            let (before, rest) = template.split_once("{a}").unwrap_or(("", template));
            let (middle, after) = rest.split_once("{b}").unwrap_or((rest, ""));
            let font = FontId::proportional(12.5);
            let mut job = egui::text::LayoutJob::default();
            let muted = egui::TextFormat { font_id: font.clone(), color: theme.text_muted.gamma_multiply(alpha), ..Default::default() };
            job.append(before, 0.0, muted.clone());
            let by_color = if kill.by.0 == "hole" { HOLE } else { name_color(kill.by.1) };
            job.append(eater(t, &kill.by.0), 0.0, egui::TextFormat { font_id: font.clone(), color: by_color.gamma_multiply(alpha), ..Default::default() });
            job.append(middle, 0.0, muted.clone());
            job.append(&kill.victim.0, 0.0, egui::TextFormat { font_id: font.clone(), color: special.unwrap_or_else(|| name_color(kill.victim.1)).gamma_multiply(alpha), ..Default::default() });
            job.append(after, 0.0, muted);
            let galley = painter.layout_job(job);
            let area = Rect::from_min_size(Pos2::new(rect.min.x + 14.0, y - 24.0), Vec2::new(galley.size().x + 20.0, 24.0));
            painter.rect_filled(area, 6.0, theme.chrome_bg.gamma_multiply(0.75 * alpha));
            if let Some(color) = special {
                painter.rect_stroke(area, 6.0, Stroke::new(1.0, color.gamma_multiply(0.7 * alpha)), egui::StrokeKind::Inside);
            }
            painter.galley(Pos2::new(area.min.x + 10.0, area.center().y - galley.size().y / 2.0), galley, theme.text);
            y -= 28.0;
        }
    }

    /// The event going on, at the top: announced big as it starts, then with its seconds left; Ronnie
    /// appearing; the connection coming back.
    fn blob_banner(&self, ui: &Ui, rect: Rect, now: f64, theme: &Theme, t: &Strings) {
        let painter = ui.painter_at(rect);
        let online = &self.blob.online;
        let at = Pos2::new(rect.center().x, rect.min.y + 34.0);
        if let crate::blob::Status::Reconnecting(_) = online.status {
            let dots = ".".repeat(1 + (now * 2.0) as usize % 3);
            let text = format!("{}{dots}", t.blob_reconnecting.trim_end_matches('…'));
            banner(&painter, at, &text, theme.ansi[3], theme);
            return;
        }
        let Some(frame) = &online.frame else { return };
        // Just started (5 s): big, in the metal letters; a bounty, and the player who held on.
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref());
        if let Some(notice) = online.notices.iter().rev().find(|n| (n.on || n.held) && n.at.elapsed().as_secs_f32() < 5.0) {
            let (text, color) = match notice.kind.as_str() {
                "bounty" if !notice.on => (t.blob_bounty_held.replace("{n}", &notice.name), GOLDEN),
                "bounty" if pseudo == Some(notice.name.as_str()) => (t.blob_bounty_you.to_owned(), GOLDEN),
                "bounty" => (t.blob_bounty.replace("{n}", &notice.name), GOLDEN),
                "ronnie" => (t.blob_ronnie.to_owned(), PINK),
                kind => match event_look(kind, t, theme) {
                    Some((text, color)) => (text.to_owned(), color),
                    // An event from a newer floor.
                    None => return,
                },
            };
            let age = notice.at.elapsed().as_secs_f32();
            let pop = out_back((age / 0.5).min(1.0));
            let alpha = ((5.0 - age) / 0.6).min(1.0);
            let size = 22.0 + 10.0 * pop;
            paint_metal(&painter, at + Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, &text, size, color, alpha);
            return;
        }
        if let Some((kind, left)) = &frame.event {
            let Some((text, color)) = event_look(kind, t, theme) else { return };
            banner(&painter, at, &format!("{text}  {left} s"), color, theme);
        }
    }

    /// The mass and the keys (bottom left), the effects going on above them, the bonus just taken (in
    /// the middle), the map (bottom right).
    fn blob_hud(&self, ui: &Ui, rect: Rect, now: f64, theme: &Theme, t: &Strings) {
        let painter = ui.painter_at(rect);
        let panel = theme.chrome_bg.gamma_multiply(0.82);
        let Some(frame) = &self.blob.online.frame else { return };
        let mine: Vec<_> = frame.cells.iter().filter(|c| Some(c.owner) == frame.me).collect();
        let mass: f32 = mine.iter().map(|c| c.mass).sum();
        let text = format!("{}    {}", t.blob_mass.replace("{n}", &format!("{}", mass.round())), t.blob_hint);
        let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), theme.text);
        let area = Rect::from_min_size(Pos2::new(rect.min.x + 14.0, rect.max.y - 14.0 - 30.0), Vec2::new(galley.size().x + 24.0, 30.0));
        painter.rect_filled(area, 8.0, panel);
        painter.galley(Pos2::new(area.min.x + 12.0, area.center().y - galley.size().y / 2.0), galley, theme.text);
        // The effects: a chip each, its time running out.
        if let Some(fx) = frame.fx {
            let mut x = area.min.x;
            let timed = [("speed", 6.0), ("magnet", 8.0), ("shield", 4.0), ("ghost", 7.0)].into_iter().enumerate().filter(|(k, _)| fx[*k] > 0.0);
            let chips = timed.map(|(k, (kind, total))| (bonus_kind(kind), format!("{}  {:.1} s", bonus_name(t, kind), fx[k]), Some(fx[k] / total)));
            // The mines carried: how many, no time.
            let mines = (frame.carried > 0).then(|| (4, format!("{} ×{}  ·  E", t.blob_fx_mine, frame.carried), None));
            for (kind, label, left) in chips.chain(mines) {
                let galley = painter.layout_no_wrap(label, FontId::proportional(12.0), theme.text);
                let chip = Rect::from_min_size(Pos2::new(x, area.min.y - 36.0), Vec2::new(galley.size().x + 40.0, 28.0));
                painter.rect_filled(chip, 8.0, panel);
                let color = bonus_color(kind, theme);
                if let Some(left) = left {
                    let bar = Rect::from_min_size(Pos2::new(chip.min.x + 6.0, chip.max.y - 5.0), Vec2::new((chip.width() - 12.0) * left.min(1.0), 2.5));
                    painter.rect_filled(bar, 1.0, color);
                }
                paint_bonus_icon(&painter, Pos2::new(chip.min.x + 16.0, chip.center().y - 1.0), 7.0, kind, color);
                painter.galley(Pos2::new(chip.min.x + 30.0, chip.center().y - galley.size().y / 2.0 - 1.0), galley, theme.text);
                x = chip.max.x + 8.0;
            }
        }
        if let Some((kind, at)) = &self.blob.flash {
            let age = (now - at) as f32;
            if age < 1.2 {
                let pop = out_back((age / 0.35).min(1.0));
                let alpha = ((1.2 - age) / 0.4).min(1.0);
                paint_metal(&painter, rect.center() - Vec2::new(0.0, rect.height() * 0.22), Align2::CENTER_CENTER, &format!("{} !", bonus_name(t, kind)), 20.0 + 12.0 * pop, bonus_color(bonus_kind(kind), theme), alpha);
            }
        }
        // The map: where the player is in the world.
        let size = self.blob.online.size.max(1.0);
        let map = Rect::from_min_size(Pos2::new(rect.max.x - 14.0 - 120.0, rect.max.y - 14.0 - 120.0), Vec2::splat(120.0));
        painter.rect_filled(map, 8.0, panel);
        painter.rect_stroke(map, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        if let Some((x, y, r)) = frame.hill {
            painter.circle_stroke(map.min + Vec2::new(x, y) / size * map.width(), (r / size * map.width()).max(4.0), Stroke::new(1.5, theme.ansi[3]));
        }
        // The night: the map goes out.
        let night = frame.event.as_ref().is_some_and(|(kind, _)| kind == "night");
        if let Some((cam, _)) = self.blob.cam.filter(|_| !night) {
            let at = map.min + cam.to_vec2() / size * map.width();
            painter.circle_filled(at, 4.0, theme.accent);
            painter.circle_stroke(at, 7.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.5)));
        }
    }

    /// Between two lives, over the world watched: the title and how to play, or how the last life
    /// ended; the skins; play (Enter). The records below, the players online to invite on the left.
    /// Returns where the players to invite are.
    fn blob_card(&mut self, ui: &mut Ui, rect: Rect, now: f64, has_pseudo: bool, theme: &Theme, t: &Strings) -> Option<Rect> {
        use crate::blob::Status;
        let status = self.blob.online.status.clone();
        let death = self.blob.online.death.clone();
        let pop = if death.is_some() { out_cubic(((now - self.blob.died_at) / 0.4) as f32) } else { 1.0 };
        let ready = has_pseudo && matches!(status, Status::Ready | Status::Reconnecting(_));
        let card_h = if ready { 320.0 } else { 250.0 };
        // The records below the card, as many lines as fit; the two centered together.
        let records = self.blob.online.records.clone().filter(|_| ready);
        let width = 420.0_f32.min(rect.width() - 32.0);
        let room = ((rect.height() - card_h - 12.0 - 60.0 - 40.0) / 19.0).floor().max(0.0) as usize;
        let panel_h = records.as_ref().map_or(0.0, |r| 40.0 + r.leaders.len().clamp(1, room.max(1)) as f32 * 19.0 + if r.me.is_some_and(|(rank, _)| rank as usize > r.leaders.len().min(room)) { 27.0 } else { 0.0 } + 8.0);
        let total = card_h + if records.is_some() { 12.0 + panel_h } else { 0.0 };
        let top = rect.center().y - total / 2.0 + (1.0 - pop) * 30.0;
        let card = Rect::from_min_size(Pos2::new(rect.center().x - width / 2.0, top), Vec2::new(width, card_h));
        let painter = ui.painter().clone();
        let painter = &painter;
        if let Some(records) = &records {
            let panel = Rect::from_min_size(Pos2::new(card.min.x, card.max.y + 12.0), Vec2::new(width, panel_h));
            paint_records(painter, panel, records, room, pop, theme, t);
        }
        let invites = (ready && card.min.x - 16.0 - 230.0 > rect.min.x + 16.0).then(|| Rect::from_min_size(Pos2::new(card.min.x - 16.0 - 230.0, card.min.y), Vec2::new(230.0, card_h)));
        if let Some(area) = invites {
            self.blob_invites(ui, area, pop, theme, t);
        }
        painter.rect_filled(card.translate(Vec2::new(0.0, 6.0)), 16.0, Color32::from_black_alpha((60.0 * pop) as u8));
        painter.rect_filled(card, 16.0, theme.chrome_bg.gamma_multiply(0.94 * pop));
        painter.rect_stroke(card, 16.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.4 * pop)), egui::StrokeKind::Inside);
        let mid = card.center().x;
        let mut y = card.min.y + 44.0;
        match &death {
            Some(d) => {
                let title = if d.expired { t.blob_expired.to_owned() } else { t.blob_eaten.replace("{n}", eater(t, &d.by)) };
                paint_metal(painter, Pos2::new(mid, y), Align2::CENTER_CENTER, &title, 30.0, if d.by == "Ronnie" { PINK } else { theme.accent }, pop);
                y += 44.0;
                let stats = t.blob_stats.replace("{m}", &d.best.to_string()).replace("{t}", &duration(d.time)).replace("{k}", &d.kills.to_string());
                painter.text(Pos2::new(mid, y), Align2::CENTER_CENTER, stats, FontId::proportional(14.0), theme.text);
                y += 26.0;
                if d.record {
                    painter.text(Pos2::new(mid, y), Align2::CENTER_CENTER, t.blob_record, FontId::proportional(14.0), theme.ansi[2]);
                }
            }
            None => {
                paint_metal(painter, Pos2::new(mid, y), Align2::CENTER_CENTER, t.blob_name, 38.0, theme.accent, 1.0);
                y += 44.0;
                painter.text(Pos2::new(mid, y), Align2::CENTER_CENTER, t.blob_tagline, FontId::proportional(14.0), theme.text);
                y += 24.0;
                // The keys, cut between two of them when the card is too narrow; each line centered.
                let font = FontId::proportional(12.0);
                let fits = |text: &str| painter.layout_no_wrap(text.to_owned(), font.clone(), theme.text_muted).size().x <= width - 40.0;
                let mut lines: Vec<String> = Vec::new();
                for key in t.blob_help.split("  ·  ") {
                    match lines.last_mut() {
                        Some(line) if fits(&format!("{line}  ·  {key}")) => *line = format!("{line}  ·  {key}"),
                        _ => lines.push(key.to_owned()),
                    }
                }
                for line in lines {
                    painter.text(Pos2::new(mid, y), Align2::CENTER_CENTER, line, font.clone(), theme.text_muted);
                    y += 18.0;
                }
                y += 4.0;
                let best = self.config.settings.blob_best;
                if best > 0 {
                    painter.text(Pos2::new(mid, y), Align2::CENTER_CENTER, t.blob_best.replace("{n}", &best.to_string()), FontId::proportional(12.0), theme.text_muted);
                }
            }
        }
        let button = Rect::from_center_size(Pos2::new(mid, card.max.y - 36.0), Vec2::new(170.0, 36.0));
        let line = Pos2::new(mid, card.max.y - 36.0);
        if !has_pseudo {
            if ui.put(button, egui::Button::new(egui::RichText::new(t.pseudo_pick).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0)).clicked() {
                self.ask_pseudo();
            }
            painter.text(line - Vec2::new(0.0, 34.0), Align2::CENTER_CENTER, t.four_need_pseudo, FontId::proportional(12.0), theme.text_muted);
            return invites;
        }
        match status {
            Status::Off | Status::Connecting => {
                painter.text(line, Align2::CENTER_CENTER, t.four_connecting, FontId::proportional(13.0), theme.text_muted);
            }
            Status::Failed(why) => {
                painter.text(line - Vec2::new(0.0, 34.0), Align2::CENTER_CENTER, why.as_deref().unwrap_or(t.four_offline), FontId::proportional(12.5), theme.ansi[1]);
                if ui.put(button, egui::Button::new(egui::RichText::new(t.blob_retry).size(14.0)).corner_radius(8.0)).clicked() {
                    self.blob.online.status = Status::Off;
                }
            }
            Status::Ready | Status::Reconnecting(_) => {
                self.blob_skins(ui, Pos2::new(mid, card.max.y - 98.0), pop, theme, t);
                let label = if death.is_some() { t.four_again } else { t.four_play };
                let play = egui::Button::new(egui::RichText::new(label).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
                // Enter sends a message while typing in the chat.
                let enter = !self.blob.chat.typing && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                // A short pause after a death: the Space pressed to flee doesn't replay at once.
                let ready = now - self.blob.died_at > 0.6 && status == Status::Ready;
                if (ui.put(button, play).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() || enter) && ready {
                    let skin = self.config.settings.blob_skin.clone();
                    self.blob.online.join(&skin);
                    self.blob.aimed = None;
                    self.blob.aim = None;
                }
            }
        }
        invites
    }

    /// The skins in a row, centered on `at`: the one picked ringed, the locked ones dimmed with a lock
    /// (what unlocks them in the tooltip).
    fn blob_skins(&mut self, ui: &mut Ui, at: Pos2, alpha: f32, theme: &Theme, t: &Strings) {
        let skins = self.blob.online.skins.clone();
        if skins.is_empty() {
            return;
        }
        let gap = 42.0;
        let picked = self.config.settings.blob_skin.clone();
        let x0 = at.x - gap * (skins.len() as f32 - 1.0) / 2.0;
        let painter = ui.painter().clone();
        let color = hue(theme, 3);
        for (k, crate::blob::Skin(id, goal, need, ok, progress)) in skins.iter().enumerate() {
            let c = Pos2::new(x0 + gap * k as f32, at.y);
            let spot = Rect::from_center_size(c, Vec2::splat(36.0));
            let resp = ui.interact(spot, ui.id().with(("blob-skin", k)), Sense::click());
            let r = if resp.hovered() && *ok { 16.5 } else { 15.0 };
            let fill = cell_color(theme, 3, id).gamma_multiply(if *ok { alpha } else { 0.3 * alpha });
            paint_jelly(&painter, c, r, if id == "ronnie" { fill } else { color.gamma_multiply(if *ok { alpha } else { 0.3 * alpha }) }, 0.0, 0, 0.0);
            if *ok {
                paint_skin(&painter, c, r, fill, id, ui.input(|i| i.time), 0);
            } else {
                paint_lock(&painter, c, theme.text.gamma_multiply(0.8 * alpha));
            }
            if *id == picked && *ok {
                painter.circle_stroke(c, r + 4.0, Stroke::new(2.0, theme.accent.gamma_multiply(alpha)));
            }
            // Its goal, and where the player is.
            let goal = match goal.as_str() {
                "games" if *need == 1 => t.blob_goal_game.to_owned(),
                "games" => format!("{} ({progress}/{need})", t.blob_goal_games.replace("{n}", &need.to_string())),
                "best" => format!("{} ({progress}/{need})", t.blob_goal_best.replace("{n}", &need.to_string())),
                "kills" => format!("{} ({progress}/{need})", t.blob_goal_kills.replace("{n}", &need.to_string())),
                _ => t.blob_skin_ronnie_how.to_owned(),
            };
            let tip = if *ok { skin_name(t, id).to_owned() } else { format!("{}\n{goal}", skin_name(t, id)) };
            let resp = resp.on_hover_text(tip);
            if *ok && resp.clicked() {
                self.config.settings.blob_skin = id.clone();
                self.save_config();
            }
            if *ok {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        }
    }

    /// The players online (on floor), to invite to Ronnie.io; the invitation sent.
    fn blob_invites(&mut self, ui: &mut Ui, area: Rect, alpha: f32, theme: &Theme, t: &Strings) {
        let painter = ui.painter().clone();
        painter.rect_filled(area, 16.0, theme.chrome_bg.gamma_multiply(0.94 * alpha));
        painter.rect_stroke(area, 16.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(alpha)), egui::StrokeKind::Inside);
        painter.text(Pos2::new(area.min.x + 18.0, area.min.y + 22.0), Align2::LEFT_CENTER, t.blob_invite_title, FontId::proportional(14.0), theme.text.gamma_multiply(alpha));
        let inner = Rect::from_min_max(Pos2::new(area.min.x + 14.0, area.min.y + 42.0), Pos2::new(area.max.x - 14.0, area.max.y - 12.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        let live = &self.versus;
        let mut invite = None;
        if live.online.is_empty() {
            child.label(egui::RichText::new(t.four_nobody).size(12.5).color(theme.text_muted));
        }
        let pending = live.sent.as_ref().filter(|s| s.status == "pending");
        egui::ScrollArea::vertical().max_height(inner.height() - 40.0).auto_shrink([false, true]).show(&mut child, |ui| {
            for player in &live.online {
                ui.horizontal(|ui| {
                    ui.set_min_height(28.0);
                    ui.label(egui::RichText::new("●").size(10.0).color(theme.ansi[2]));
                    ui.label(egui::RichText::new(&player.name).size(13.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(pending.is_none(), egui::Button::new(egui::RichText::new(t.four_invite).size(12.0)).corner_radius(6.0)).clicked() {
                            invite = Some(player.name.clone());
                        }
                    });
                });
            }
        });
        if let Some(sent) = pending {
            child.add_space(4.0);
            child.label(egui::RichText::new(t.four_sent.replace("{n}", sent.to.as_deref().unwrap_or(t.game_anonymous))).size(12.0).color(theme.text_muted));
        }
        if let Some(name) = invite {
            self.versus.invite_to(&name, "ronnie-io");
        }
    }
}

/// A pill at the top: an event and its seconds left, the connection coming back.
fn banner(painter: &egui::Painter, at: Pos2, text: &str, color: Color32, theme: &Theme) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(15.0), color);
    let area = Rect::from_center_size(at, galley.size() + Vec2::new(32.0, 14.0));
    painter.rect_filled(area, area.height() / 2.0, theme.chrome_bg.gamma_multiply(0.88));
    painter.rect_stroke(area, area.height() / 2.0, Stroke::new(1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    painter.galley(area.center() - galley.size() / 2.0, galley, color);
}

/// The records, in a panel: the first ones (`room` lines at most), the player's place below them.
fn paint_records(painter: &egui::Painter, panel: Rect, records: &crate::blob::Records, room: usize, alpha: f32, theme: &Theme, t: &Strings) {
    painter.rect_filled(panel, 16.0, theme.chrome_bg.gamma_multiply(0.94 * alpha));
    painter.rect_stroke(panel, 16.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    painter.text(Pos2::new(panel.min.x + 18.0, panel.min.y + 20.0), Align2::LEFT_CENTER, t.blob_records, FontId::proportional(14.0), theme.text.gamma_multiply(alpha));
    let mut y = panel.min.y + 44.0;
    if records.leaders.is_empty() {
        painter.text(Pos2::new(panel.min.x + 18.0, y), Align2::LEFT_CENTER, t.blob_no_records, FontId::proportional(12.5), theme.text_muted.gamma_multiply(alpha));
        return;
    }
    let shown = records.leaders.len().min(room.max(1));
    let mine = records.me.filter(|(rank, _)| *rank as usize > shown).map(|(rank, mass)| (rank, None, mass, true));
    let lines = records.leaders.iter().take(shown).enumerate().map(|(k, (name, mass, me))| (k as u32 + 1, Some(name.as_str()), *mass, *me));
    for (rank, name, mass, me) in lines.chain(mine) {
        if rank as usize > shown {
            painter.text(Pos2::new(panel.center().x, y - 4.0), Align2::CENTER_CENTER, "···", FontId::proportional(11.0), theme.text_muted.gamma_multiply(alpha));
            y += 8.0;
        }
        let color = if me { theme.accent } else { theme.text_muted }.gamma_multiply(alpha);
        // The first three in metal colors.
        let medal = [Color32::from_rgb(232, 190, 60), Color32::from_rgb(190, 196, 204), Color32::from_rgb(196, 128, 70)].get(rank as usize - 1).copied();
        painter.text(Pos2::new(panel.min.x + 30.0, y), Align2::RIGHT_CENTER, format!("{rank}."), FontId::proportional(12.5), medal.map_or(color, |m| m.gamma_multiply(alpha)));
        let name = name.unwrap_or(t.game_you).to_owned();
        let mut job = egui::text::LayoutJob::simple_singleline(name, FontId::proportional(12.5), if me { color } else { theme.text.gamma_multiply(alpha) });
        job.wrap = egui::text::TextWrapping::truncate_at_width(panel.width() - 110.0);
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(panel.min.x + 38.0, y - galley.size().y / 2.0), galley, color);
        painter.text(Pos2::new(panel.max.x - 18.0, y), Align2::RIGHT_CENTER, mass.to_string(), FontId::monospace(12.0), color);
        y += 19.0;
    }
}

/// A cell: a disc whose edge wobbles a little (`wobble`: how much), a darker rim, a shine.
fn paint_jelly(painter: &egui::Painter, at: Pos2, r: f32, color: Color32, now: f64, seed: u32, wobble: f32) {
    if r < 1.0 {
        return;
    }
    let steps = (r * 0.8).clamp(16.0, 72.0) as usize;
    let phase = (seed % 97) as f32;
    let edge = |k: usize, r: f32| {
        let a = k as f32 / steps as f32 * std::f32::consts::TAU;
        let w = 1.0 + wobble * ((a * 5.0 + now as f32 * 2.3 + phase).sin() * 0.6 + (a * 3.0 - now as f32 * 1.7 + phase * 0.5).sin() * 0.4);
        at + Vec2::new(a.cos(), a.sin()) * r * w
    };
    let rim = super::sidebar::lerp_color(color, Color32::BLACK.gamma_multiply(color.a() as f32 / 255.0), 0.3);
    let border = (r * 0.07).clamp(1.5, 8.0);
    painter.add(fan(at, (0..steps).map(|k| edge(k, r)), rim));
    painter.add(fan(at, (0..steps).map(|k| edge(k, r - border)), color));
    if r > 8.0 {
        painter.circle_filled(at + Vec2::new(-0.35, -0.4) * r, r * 0.14, Color32::WHITE.gamma_multiply(0.18 * color.a() as f32 / 255.0));
    }
}

/// A skin's drawing over a cell (`r`: its radius on screen): stripes, a checkerboard, flames, a bolt,
/// a skull; Ronnie's sparkles, the boss's horns.
fn paint_skin(painter: &egui::Painter, at: Pos2, r: f32, color: Color32, skin: &str, now: f64, seed: u32) {
    if r < 6.0 {
        return;
    }
    let inner = r * 0.86;
    let dark = super::sidebar::lerp_color(color, Color32::BLACK, 0.35);
    let light = super::sidebar::lerp_color(color, Color32::WHITE, 0.45);
    let t = now as f32;
    match skin {
        "stripes" => {
            // Diagonal chords, turning slowly.
            let a = 0.7 + t * 0.15 + seed as f32;
            let (along, across) = (Vec2::new(a.cos(), a.sin()), Vec2::new(-a.sin(), a.cos()));
            let w = inner / 3.5;
            let mut o = -inner + w;
            while o < inner {
                let half = (inner * inner - o * o).max(0.0).sqrt() - w * 0.3;
                if half > 0.0 {
                    let c = at + across * o;
                    painter.line_segment([c - along * half, c + along * half], Stroke::new(w * 0.9, dark.gamma_multiply(0.7)));
                }
                o += w * 2.0;
            }
        }
        "checker" => {
            let s = inner / 2.6;
            let n = (inner / s).ceil() as i32;
            for i in -n..n {
                for j in -n..n {
                    if (i + j) % 2 != 0 {
                        continue;
                    }
                    let c = at + Vec2::new((i as f32 + 0.5) * s, (j as f32 + 0.5) * s);
                    if (c - at).length() + s * 0.72 < inner {
                        painter.rect_filled(Rect::from_center_size(c, Vec2::splat(s)), s * 0.15, dark.gamma_multiply(0.75));
                    }
                }
            }
        }
        "flames" => {
            // Tongues rising from the bottom, flickering.
            let colors = [Color32::from_rgb(255, 80, 20), Color32::from_rgb(255, 170, 30), Color32::from_rgb(255, 230, 120)];
            for (layer, flame) in colors.iter().enumerate() {
                let scale = 1.0 - layer as f32 * 0.25;
                for k in 0..5 {
                    let x = (k as f32 - 2.0) / 2.6;
                    let base_y = (1.0 - x * x).max(0.0).sqrt() * inner * 0.9;
                    let flick = 0.75 + 0.25 * (t * 9.0 + k as f32 * 1.7 + seed as f32).sin();
                    let h = inner * (0.9 - x.abs() * 0.5) * flick * scale;
                    let w = inner * 0.26 * scale;
                    let bottom = at + Vec2::new(x * inner, base_y);
                    let tip = bottom + Vec2::new((t * 5.0 + k as f32).sin() * w * 0.4, -h);
                    painter.add(egui::Shape::convex_polygon(vec![bottom - Vec2::new(w, 0.0), tip, bottom + Vec2::new(w, 0.0)], flame.gamma_multiply(0.9), Stroke::NONE));
                }
            }
        }
        "lightning" => {
            let s = inner;
            let bolt = [(-0.15, -0.85), (0.3, -0.85), (0.05, -0.15), (0.4, -0.15), (-0.25, 0.85), (-0.05, 0.1), (-0.4, 0.1)];
            let pts: Vec<Pos2> = bolt.iter().map(|&(x, y)| at + Vec2::new(x, y) * s).collect();
            let glow = 0.6 + 0.4 * ((t * 7.0 + seed as f32).sin() * 0.5 + 0.5);
            let yellow = Color32::from_rgb(255, 225, 60);
            // Two convex halves of the bolt.
            painter.add(egui::Shape::convex_polygon(vec![pts[0], pts[1], pts[2], pts[3], pts[6]], yellow.gamma_multiply(glow), Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![pts[2], pts[3], pts[4], pts[5], pts[6]], yellow.gamma_multiply(glow), Stroke::NONE));
            let mut outline = pts.clone();
            outline.push(pts[0]);
            painter.add(egui::Shape::line(outline, Stroke::new((r * 0.03).max(1.0), Color32::WHITE.gamma_multiply(0.7))));
        }
        "skull" => {
            let s = inner * 0.62;
            let bone = Color32::from_rgb(240, 236, 225);
            let hole = Color32::from_rgb(30, 25, 30);
            painter.circle_filled(at + Vec2::new(0.0, -s * 0.15), s, bone);
            painter.rect_filled(Rect::from_center_size(at + Vec2::new(0.0, s * 0.75), Vec2::new(s * 1.1, s * 0.6)), s * 0.15, bone);
            for side in [-1.0, 1.0] {
                painter.circle_filled(at + Vec2::new(side * s * 0.4, -s * 0.15), s * 0.27, hole);
            }
            painter.add(egui::Shape::convex_polygon(vec![at + Vec2::new(0.0, s * 0.18), at + Vec2::new(-s * 0.13, s * 0.42), at + Vec2::new(s * 0.13, s * 0.42)], hole, Stroke::NONE));
            for k in -1..=1 {
                let x = k as f32 * s * 0.3;
                painter.line_segment([at + Vec2::new(x, s * 0.55), at + Vec2::new(x, s * 1.0)], Stroke::new((s * 0.06).max(1.0), hole));
            }
        }
        "ronnie" => {
            // Sparkles around, turning; a heart in the middle.
            for k in 0..6 {
                let a = t * 0.8 + k as f32 / 6.0 * std::f32::consts::TAU;
                let d = inner * (0.62 + 0.08 * (t * 3.0 + k as f32).sin());
                let size = r * (0.09 + 0.04 * (t * 5.0 + k as f32 * 2.0).sin());
                paint_star(painter, at + Vec2::new(a.cos(), a.sin()) * d, size, Color32::WHITE.gamma_multiply(0.85));
            }
            let s = inner * 0.32;
            let c = at + Vec2::new(0.0, -s * 0.1);
            painter.circle_filled(c + Vec2::new(-s * 0.5, 0.0), s * 0.55, light);
            painter.circle_filled(c + Vec2::new(s * 0.5, 0.0), s * 0.55, light);
            painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-s * 1.02, s * 0.15), c + Vec2::new(s * 1.02, s * 0.15), c + Vec2::new(0.0, s * 1.25)], light, Stroke::NONE));
        }
        "galaxy" => {
            // A night sky: dark, a spiral of stars turning.
            painter.circle_filled(at, inner, Color32::from_rgb(25, 12, 50).gamma_multiply(0.85));
            painter.circle_filled(at, inner * 0.55, Color32::from_rgb(90, 40, 140).gamma_multiply(0.35));
            for k in 0..14 {
                let f = k as f32 / 14.0;
                let a = t * 0.5 + f * std::f32::consts::TAU * 1.6 + seed as f32;
                let d = inner * (0.15 + 0.75 * f);
                let twinkle = 0.5 + 0.5 * (t * 4.0 + k as f32 * 1.3).sin();
                paint_star(painter, at + Vec2::new(a.cos(), a.sin()) * d, r * (0.04 + 0.05 * twinkle), Color32::WHITE.gamma_multiply(0.5 + 0.5 * twinkle));
            }
        }
        "crown" => {
            // A golden crown on top.
            let gold = Color32::from_rgb(245, 195, 50);
            let w = r * 0.62;
            let base_y = at.y - r * 0.55;
            let band = Rect::from_min_max(Pos2::new(at.x - w, base_y - r * 0.12), Pos2::new(at.x + w, base_y + r * 0.08));
            for k in 0..3 {
                let x = at.x + (k as f32 - 1.0) * w * 0.8;
                let h = if k == 1 { r * 0.55 } else { r * 0.42 };
                painter.add(egui::Shape::convex_polygon(vec![Pos2::new(x - w * 0.42, band.min.y), Pos2::new(x, band.min.y - h), Pos2::new(x + w * 0.42, band.min.y)], gold, Stroke::NONE));
                painter.circle_filled(Pos2::new(x, band.min.y - h), r * 0.06, Color32::from_rgb(230, 40, 60));
            }
            painter.rect_filled(band, r * 0.03, gold);
        }
        "boss" => {
            // Horns over the top, angry eyes.
            let horn = Color32::from_rgb(235, 225, 200);
            for side in [-1.0, 1.0] {
                let base = at + Vec2::new(side * r * 0.45, -r * 0.8);
                painter.add(egui::Shape::convex_polygon(vec![base - Vec2::new(r * 0.18, 0.0), base + Vec2::new(side * r * 0.35, -r * 0.55), base + Vec2::new(r * 0.18, 0.0)], horn, Stroke::NONE));
                let eye = at + Vec2::new(side * r * 0.3, -r * 0.15);
                painter.circle_filled(eye, r * 0.12, Color32::from_rgb(255, 220, 60));
                painter.line_segment([eye + Vec2::new(-side * r * 0.18, -r * 0.2), eye + Vec2::new(side * r * 0.14, -r * 0.08)], Stroke::new(r * 0.06, Color32::BLACK));
            }
        }
        _ => {}
    }
}

/// A four-pointed star.
fn paint_star(painter: &egui::Painter, at: Pos2, size: f32, color: Color32) {
    let thin = size * 0.28;
    painter.add(egui::Shape::convex_polygon(vec![at + Vec2::new(0.0, -size), at + Vec2::new(thin, 0.0), at + Vec2::new(0.0, size), at + Vec2::new(-thin, 0.0)], color, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![at + Vec2::new(-size, 0.0), at + Vec2::new(0.0, -thin), at + Vec2::new(size, 0.0), at + Vec2::new(0.0, thin)], color, Stroke::NONE));
}

/// A padlock, for a skin still locked.
fn paint_lock(painter: &egui::Painter, at: Pos2, color: Color32) {
    painter.rect_filled(Rect::from_center_size(at + Vec2::new(0.0, 2.5), Vec2::new(11.0, 8.5)), 2.0, color);
    let shackle: Vec<Pos2> = (0..=12).map(|k| {
        let a = std::f32::consts::PI * (1.0 + k as f32 / 12.0);
        at + Vec2::new(0.0, -2.0) + Vec2::new(a.cos(), a.sin()) * 3.6
    }).collect();
    painter.add(egui::Shape::line(shackle, Stroke::new(1.6, color)));
}

/// A dashed ring turning slowly: the magnet's reach.
fn paint_ring(painter: &egui::Painter, at: Pos2, r: f32, color: Color32, now: f64) {
    let steps = 64;
    let turn = now as f32 * 0.6;
    let points: Vec<Pos2> = (0..=steps).map(|k| {
        let a = k as f32 / steps as f32 * std::f32::consts::TAU + turn;
        at + Vec2::new(a.cos(), a.sin()) * r
    }).collect();
    painter.extend(egui::Shape::dashed_line(&points, Stroke::new(1.5, color), 8.0, 7.0));
}

fn bonus_color(kind: u8, theme: &Theme) -> Color32 {
    match kind {
        0 => theme.ansi[3],
        1 => theme.ansi[4],
        3 => theme.ansi[5],
        4 => theme.ansi[1],
        _ => theme.ansi[6],
    }
}

/// A bonus on the map: a glowing token with its icon, breathing.
fn paint_bonus(painter: &egui::Painter, at: Pos2, r: f32, kind: u8, now: f64, theme: &Theme) {
    let color = bonus_color(kind, theme);
    let breath = 1.0 + 0.08 * ((now * 4.0).sin() as f32);
    painter.circle_filled(at, r * 1.6 * breath, color.gamma_multiply(0.12));
    painter.circle_filled(at, r * breath, theme.chrome_bg);
    painter.circle_stroke(at, r * breath, Stroke::new((r * 0.14).max(1.5), color));
    paint_bonus_icon(painter, at, r * 0.55, kind, color);
}

/// A bonus's icon: two chevrons (speed), a horseshoe magnet, a shield, a ghost, a mine.
fn paint_bonus_icon(painter: &egui::Painter, at: Pos2, s: f32, kind: u8, color: Color32) {
    let stroke = Stroke::new((s * 0.28).max(1.3), color);
    match kind {
        0 => {
            for dx in [-0.45, 0.35] {
                let x = at.x + dx * s;
                painter.add(egui::Shape::line(vec![Pos2::new(x - s * 0.3, at.y - s * 0.6), Pos2::new(x + s * 0.3, at.y), Pos2::new(x - s * 0.3, at.y + s * 0.6)], stroke));
            }
        }
        1 => {
            let arc: Vec<Pos2> = (0..=12).map(|k| {
                let a = std::f32::consts::PI * (k as f32 / 12.0);
                at + Vec2::new(0.0, s * 0.05) + Vec2::new(a.cos(), a.sin()) * s * 0.6
            }).collect();
            painter.add(egui::Shape::line(arc, stroke));
            for side in [-1.0, 1.0] {
                painter.line_segment([at + Vec2::new(side * s * 0.6, s * 0.05), at + Vec2::new(side * s * 0.6, -s * 0.6)], stroke);
                painter.line_segment([at + Vec2::new(side * s * 0.6, -s * 0.4), at + Vec2::new(side * s * 0.6, -s * 0.65)], Stroke::new(stroke.width, Color32::WHITE));
            }
        }
        3 => {
            // A dome on top, three waves at the bottom, two eyes.
            let mut outline: Vec<Pos2> = (0..=12).map(|k| {
                let a = std::f32::consts::PI * (1.0 + k as f32 / 12.0);
                at + Vec2::new(0.0, -s * 0.1) + Vec2::new(a.cos(), a.sin()) * s * 0.6
            }).collect();
            for k in 0..=6 {
                let x = 0.6 - 1.2 * k as f32 / 6.0;
                let y = if k % 2 == 0 { 0.65 } else { 0.4 };
                outline.push(at + Vec2::new(x * s, y * s));
            }
            outline.push(outline[0]);
            painter.add(egui::Shape::line(outline, stroke));
            for side in [-1.0, 1.0] {
                painter.circle_filled(at + Vec2::new(side * s * 0.22, -s * 0.12), s * 0.11, color);
            }
        }
        4 => {
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU;
                let d = Vec2::new(a.cos(), a.sin());
                painter.line_segment([at + d * s * 0.3, at + d * s * 0.75], stroke);
            }
            painter.circle_filled(at, s * 0.42, color);
        }
        _ => {
            let shield = vec![at + Vec2::new(-s * 0.6, -s * 0.6), at + Vec2::new(s * 0.6, -s * 0.6), at + Vec2::new(s * 0.6, s * 0.05), at + Vec2::new(0.0, s * 0.7), at + Vec2::new(-s * 0.6, s * 0.05)];
            painter.add(egui::Shape::convex_polygon(shield, color.gamma_multiply(0.35), stroke));
        }
    }
}

/// A mine laid: a dark spiked ball, its light blinking in its player's color.
fn paint_mine(painter: &egui::Painter, at: Pos2, r: f32, color: Color32, now: f64, theme: &Theme) {
    let body = super::sidebar::lerp_color(theme.text_muted, Color32::BLACK, 0.5);
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU + 0.2;
        let d = Vec2::new(a.cos(), a.sin());
        painter.line_segment([at, at + d * r * 1.35], Stroke::new((r * 0.22).max(1.5), body));
    }
    painter.circle_filled(at, r, body);
    let blink = if (now * 3.0).fract() < 0.5 { 1.0 } else { 0.35 };
    painter.circle_filled(at, r * 0.32, color.gamma_multiply(blink));
}

/// The black hole: a dark core, a glowing ring and arms turning around it.
/// Dark over `rect` but within `sight` of `around`, the edge soft.
fn paint_night(painter: &egui::Painter, rect: Rect, around: Pos2, sight: f32, dark: f32) {
    let color = |a: f32| Color32::from_rgba_unmultiplied(3, 4, 12, (255.0 * a) as u8);
    // The soft edge: rings darker and darker, from 70 % of the sight to past it.
    let steps = 10;
    let (from, to) = (sight * 0.7, sight * 1.15);
    let w = (to - from) / steps as f32;
    for k in 0..steps {
        let a = dark * ((k as f32 + 1.0) / steps as f32).powf(1.5);
        painter.circle_stroke(around, from + w * (k as f32 + 0.5), Stroke::new(w + 0.5, color(a)));
    }
    // Beyond: a ring wide enough to cover the screen.
    let far = rect.size().length() + (around - rect.center()).length();
    painter.circle_stroke(around, to + far / 2.0, Stroke::new(far, color(dark)));
}

fn paint_hole(painter: &egui::Painter, at: Pos2, scale: f32, now: f64) {
    let core = 70.0 * scale;
    let t = now as f32;
    painter.circle_filled(at, core * 5.0, HOLE.gamma_multiply(0.05));
    painter.circle_filled(at, core * 2.6, HOLE.gamma_multiply(0.08));
    // Arms of dust spiraling in.
    for arm in 0..4 {
        let points: Vec<Pos2> = (0..=24).map(|k| {
            let f = k as f32 / 24.0;
            let a = t * 1.2 + arm as f32 / 4.0 * std::f32::consts::TAU + f * 3.0;
            at + Vec2::new(a.cos(), a.sin()) * core * (1.0 + f * 3.5)
        }).collect();
        painter.add(egui::Shape::line(points, Stroke::new((core * 0.12).max(1.5), HOLE.gamma_multiply(0.35))));
    }
    painter.circle_stroke(at, core * 1.15, Stroke::new((core * 0.18).max(2.0), HOLE.gamma_multiply(0.7 + 0.2 * (t * 3.0).sin())));
    painter.circle_filled(at, core, Color32::from_rgb(8, 4, 14));
}

/// The crosshair on a head with a bounty: four ticks turning, and a dashed ring.
fn paint_crosshair(painter: &egui::Painter, at: Pos2, r: f32, color: Color32, now: f64) {
    paint_ring(painter, at, r, color.gamma_multiply(0.8), now);
    let turn = now as f32 * 0.8;
    for k in 0..4 {
        let a = turn + k as f32 / 4.0 * std::f32::consts::TAU;
        let d = Vec2::new(a.cos(), a.sin());
        painter.line_segment([at + d * (r - 6.0), at + d * (r + 10.0)], Stroke::new(2.5, color));
    }
}

/// A virus: a green disc with spikes, turning slowly.
fn paint_virus(painter: &egui::Painter, at: Pos2, r: f32, color: Color32, now: f64) {
    let spikes = 22;
    let turn = now as f32 * 0.15;
    let point = |k: usize, r: f32| {
        let a = k as f32 / (spikes * 2) as f32 * std::f32::consts::TAU + turn;
        let r = if k.is_multiple_of(2) { r * 1.1 } else { r * 0.96 };
        at + Vec2::new(a.cos(), a.sin()) * r
    };
    let rim = super::sidebar::lerp_color(color, Color32::BLACK, 0.35);
    painter.add(fan(at, (0..spikes * 2).map(|k| point(k, r)), rim));
    painter.add(fan(at, (0..spikes * 2).map(|k| point(k, r - (r * 0.08).max(2.0))), color.gamma_multiply(0.9)));
}

/// A shape around `center` (convex or star-shaped), as a fan of triangles.
fn fan(center: Pos2, edge: impl Iterator<Item = Pos2>, color: Color32) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    for p in edge {
        mesh.colored_vertex(p, color);
    }
    let n = mesh.vertices.len() as u32 - 1;
    for k in 0..n {
        mesh.add_triangle(0, 1 + k, 1 + (k + 1) % n);
    }
    egui::Shape::mesh(mesh)
}

/// A big cell eating a small one: the game's icon.
pub(super) fn paint_blob_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    painter.circle_filled(c + Vec2::new(-1.5, 0.5) * scale, 6.5 * scale, hue(theme, 3));
    painter.circle_filled(c + Vec2::new(5.5, -4.5) * scale, 2.6 * scale, PINK);
    painter.circle_filled(c + Vec2::new(-3.6, -2.0) * scale, 1.4 * scale, Color32::WHITE.gamma_multiply(0.35));
}
