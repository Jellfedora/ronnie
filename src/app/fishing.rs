//! La pêche aux talents, in the games (its rules in `crate::fishing`, the pond shared through
//! `crate::pond`): the player is a recruiter at the edge of a pond full of candidates. Space (or the
//! mouse) held then let go casts, a bite is hooked with Space, then the candidate is kept in the bar
//! until hired. Each one hired is a card in the binder (« le vivier »), from common to legendary, a
//! shiny one now and then. The colleagues fishing at the same time stand on the bank too.
//!
//! During working hours, `crate::playtime::FISH_CATCHES` candidates a half-day; the pond stays open
//! to watch the others and leaf through the binder.

use super::four::HomeGame;
use super::*;
use crate::fishing::{Bait, Lure, Reel, Rarity, When, FISH};
use crate::pond::Line;

/// The world moves in steps this long, whatever the frame rate.
const STEP: f32 = 1.0 / 120.0;
/// The scene's size, in its own units.
const W: f32 = 960.0;
const H: f32 = 540.0;
/// Where the water starts and ends, and where the fishers stand.
const WATER_TOP: f32 = 205.0;
const BANK: f32 = 440.0;
const FEET: f32 = 488.0;
const SPOTS: usize = 6;
/// One catch in so many is a shiny card.
const SHINY_ONE_IN: u64 = 25;
/// Seconds to hook a bite.
const BITE_TIME: f32 = 1.1;
/// How long another one's catch shows over their head (ms).
const SHOWN_MS: f64 = 6000.0;
/// How far the aim turns either way from straight out (radians).
const AIM_MAX: f32 = 1.15;
/// How near the bubbles a float must land to be in them.
const HOT_R: f32 = 48.0;
/// A nibble: the float tapped this long (hooking then scares the candidate off).
const NIBBLE: f32 = 0.32;
/// The candidate leaping out of the water before its card shows.
const LEAP: f64 = 0.75;
/// A rival's counter-offer, in seconds.
const OFFER: f32 = 3.5;

/// Who is on the line: picked as the float lands (its shadow comes to it), with its size, whether
/// shiny, and the seed of its fight.
#[derive(Clone, Copy)]
struct Hook {
    fish: usize,
    size: u32,
    shiny: bool,
    seed: u64,
    /// Landed in the bubbles.
    hot: bool,
}

enum Phase {
    Idle,
    /// The cast charging: its power goes up and down while held.
    Charging { power: f32, rising: bool },
    /// The float in the water: the bite at `bite_in` seconds after the cast, nibbles before (their
    /// times, negative: none), `elapsed` since the cast.
    Waiting { cast: f32, hook: Hook, bite_in: f32, nibbles: [f32; 3], elapsed: f32, splashed: bool },
    Bite { cast: f32, hook: Hook, left: f32 },
    /// The fight; a rival's counter-offer at `offer_at` seconds into it (negative: none).
    Reeling { cast: f32, reel: Reel, hook: Hook, t: f32, offer_at: f32 },
    Caught { fish: usize, size: u32, shiny: bool, new: bool, record: bool, bait: Option<Bait>, at: f64 },
}

pub(super) struct Fishing {
    phase: Phase,
    /// The binder shown rather than the pond.
    cards: bool,
    /// A card of the binder shown big.
    zoom: Option<usize>,
    /// A word under the scene (the candidate lost...), until then.
    message: Option<(String, f64)>,
    held: bool,
    /// When the line was let go (the cast's swing and the float's flight drawn from it).
    cast_at: f64,
    /// Where the player aims (radians from straight out, toward the right positive), and where the
    /// float went (the scene's units).
    aim: f32,
    float: Pos2,
    sound: Option<super::blob::Sound>,
    acc: f32,
    rng: u64,
    pub(super) pond: crate::pond::Pond,
    pub(super) floor: crate::floor::GameFloor,
}

impl Default for Fishing {
    fn default() -> Self {
        Self { phase: Phase::Idle, cards: false, zoom: None, message: None, held: false, cast_at: 0.0, aim: 0.0, float: Pos2::ZERO, sound: None, acc: 0.0, rng: 0, pond: Default::default(), floor: crate::floor::GameFloor::new("peche", false) }
    }
}

impl Fishing {
    fn line(&self) -> (Line, f32) {
        match &self.phase {
            Phase::Waiting { cast, .. } => (Line::Wait, *cast),
            Phase::Bite { cast, .. } => (Line::Bite, *cast),
            Phase::Reeling { cast, .. } => (Line::Reel, *cast),
            _ => (Line::Idle, 0.0),
        }
    }

    fn fighting(&self) -> bool {
        matches!(self.phase, Phase::Reeling { .. })
    }

    /// The page left: the line comes out of the water, the player leaves the pond.
    pub(super) fn leave(&mut self, account: Option<&crate::config::FloorAccount>) {
        if !matches!(self.phase, Phase::Caught { .. }) {
            self.phase = Phase::Idle;
        }
        self.held = false;
        self.pond.leave(account);
    }
}

/// A candidate's card text: its name, its attack, the attack's damage, its flavor text.
pub(super) struct CardText<'a> {
    pub name: &'a str,
    attack: &'a str,
    damage: &'a str,
    flavor: &'a str,
}

pub(super) fn card_text(t: &Strings, fish: usize) -> CardText<'static> {
    let line = t.fishing_cards.lines().nth(fish).unwrap_or_default();
    let mut parts = line.split('|');
    let mut next = || parts.next().unwrap_or_default();
    CardText { name: next(), attack: next(), damage: next(), flavor: next() }
}

/// The chime of a hire: longer the rarer, sparkles when shiny.
fn hired_sound(r: Rarity, shiny: bool) -> &'static [(f32, f32, f32, f32)] {
    match (r, shiny) {
        (_, true) => &[(784.0, 784.0, 0.08, 0.3), (988.0, 988.0, 0.08, 0.3), (1175.0, 1175.0, 0.08, 0.3), (1568.0, 1568.0, 0.06, 0.22), (2093.0, 2093.0, 0.06, 0.22), (2637.0, 2637.0, 0.2, 0.2)],
        (Rarity::Common, _) => &[(660.0, 660.0, 0.08, 0.3), (880.0, 880.0, 0.14, 0.3)],
        (Rarity::Uncommon, _) => &[(660.0, 660.0, 0.08, 0.3), (880.0, 880.0, 0.08, 0.3), (1100.0, 1100.0, 0.16, 0.3)],
        (Rarity::Rare, _) => &[(523.0, 523.0, 0.08, 0.3), (659.0, 659.0, 0.08, 0.3), (784.0, 784.0, 0.08, 0.3), (1047.0, 1047.0, 0.22, 0.3)],
        (Rarity::VeryRare, _) => &[(523.0, 523.0, 0.07, 0.3), (659.0, 659.0, 0.07, 0.3), (784.0, 784.0, 0.07, 0.3), (1047.0, 1047.0, 0.1, 0.3), (1319.0, 1319.0, 0.28, 0.3)],
        (Rarity::Legendary, _) => &[(392.0, 392.0, 0.12, 0.35), (523.0, 523.0, 0.12, 0.35), (659.0, 659.0, 0.12, 0.35), (784.0, 784.0, 0.3, 0.35), (659.0, 659.0, 0.1, 0.3), (1047.0, 1047.0, 0.45, 0.35)],
    }
}

fn rarity_color(r: Rarity) -> Color32 {
    match r {
        Rarity::Common => Color32::from_rgb(0x9a, 0xa3, 0xad),
        Rarity::Uncommon => Color32::from_rgb(0x5f, 0xb8, 0x6a),
        Rarity::Rare => Color32::from_rgb(0x4f, 0x8f, 0xe8),
        Rarity::VeryRare => Color32::from_rgb(0xa8, 0x6b, 0xe8),
        Rarity::Legendary => Color32::from_rgb(0xf2, 0xb8, 0x32),
    }
}

fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// A rect filled from `top` to `bottom`.
fn gradient(painter: &egui::Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.left_bottom(), bottom);
    mesh.colored_vertex(r.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

/// The hour now (the machine's clock).
fn hour() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

/// The sky's top and bottom, the water, at `hour`; whether it's night (stars).
fn scene_colors(hour: u32) -> (Color32, Color32, Color32, bool) {
    match hour {
        5..=11 => (hex(0x8ec3e6), hex(0xf6d2b4), hex(0x4f8fae), false),
        12..=17 => (hex(0x4f9fde), hex(0xc4e6f6), hex(0x3a7ea2), false),
        18..=20 => (hex(0x2a2a5c), hex(0xe08a6a), hex(0x2c4f6e), false),
        _ => (hex(0x0a0f26), hex(0x1f2c52), hex(0x172c45), true),
    }
}

/// Where fisher `spot` stands.
fn spot_x(spot: usize) -> f32 {
    W * (spot as f32 + 0.5) / SPOTS as f32
}

/// The rod: its length (from the hand), its angle from upright at rest, pulled back over the
/// shoulder, thrown forward, and waiting.
const ROD: f32 = 78.0;
const ROD_REST: f32 = 0.21;
const ROD_BACK: f32 = -0.62;
const ROD_FORWARD: f32 = 0.98;
const ROD_WAIT: f32 = 0.45;
/// The forward swing's time, and when in it the line goes (the float flying from the tip).
const THROW: f32 = 0.2;
const LET_GO: f32 = 0.11;
/// The float's flight: longer the further it goes.
const FLIGHT_BASE: f32 = 0.45;
const FLIGHT_PER_CAST: f32 = 0.5;

/// How the player's rod moves: still, pulled back as the cast charges, or thrown `t` s ago.
#[derive(Clone, Copy)]
enum Swing {
    Still,
    Back(f32),
    Thrown { from: f32, t: f32 },
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k.clamp(0.0, 1.0)).powi(3)
}

/// The rod `t` s after a cast of `from`: whipped forward from where it was pulled back, then
/// springing to rest in front, and how much its middle lags behind its tip.
fn rod_angle(from: f32, t: f32) -> (f32, f32) {
    let back = ROD_REST - (ROD_REST - ROD_BACK) * ease_out(from);
    if t < THROW {
        let k = ease_out(t / THROW);
        (back + (ROD_FORWARD - back) * k, 0.22 * (1.0 - k))
    } else {
        let s = t - THROW;
        let wobble = (-s * 5.0).exp() * (s * 15.0).cos();
        (ROD_WAIT + (ROD_FORWARD - ROD_WAIT) * wobble, 0.1 * wobble)
    }
}

/// Where the player's line leaves from on the water's edge.
fn cast_origin(spot: usize) -> Pos2 {
    Pos2::new(spot_x(spot) + 26.0, BANK - 6.0)
}

/// Where a cast of `power` aimed at `aim` lands (kept on the water).
fn landing(origin: Pos2, aim: f32, power: f32) -> Pos2 {
    let d = 40.0 + power * 200.0;
    Pos2::new((origin.x + aim.sin() * d).clamp(20.0, W - 20.0), (origin.y - aim.cos() * d).clamp(WATER_TOP + 16.0, BANK - 22.0))
}

/// The clock, in seconds (the same for everyone at the pond: the fairs, the bubbles).
fn unix_now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

/// The bubbles on the water now: where, and how strong.
fn hotspots(unix: f64) -> [(Pos2, f32); 2] {
    crate::fishing::hotspots(unix).map(|(u, v, k)| (Pos2::new(u * W, WATER_TOP + 20.0 + v * (BANK - WATER_TOP - 50.0)), k))
}

fn in_bubbles(unix: f64, p: Pos2) -> bool {
    hotspots(unix).iter().any(|(at, k)| *k > 0.3 && at.distance(p) < HOT_R)
}

/// A rect from two corners in the scene's units.
fn r_of(at: &impl Fn(f32, f32) -> Pos2, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(at(x0, y0), at(x1, y1))
}

/// Where a line's float is, cast so far.
fn float_at(spot: usize, cast: f32) -> Pos2 {
    Pos2::new(spot_x(spot) + 26.0 + cast * 18.0, BANK - 28.0 - cast * (BANK - WATER_TOP - 60.0))
}

impl App {
    pub(super) fn open_fishing(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Fishing;
    }

    /// The candidates still to hire this half-day (None: free).
    fn fish_left(&mut self) -> Option<(u32, crate::playtime::Period)> {
        let p = self.work_period()?;
        Some((crate::playtime::FISH_CATCHES.saturating_sub(self.config.settings.game_usage.fish), p))
    }

    pub(super) fn fishing_page(&mut self, ui: &mut Ui, rect: Rect) {
        let fighting = self.fishing.fighting();
        self.fold_for_game(fighting);
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        ui.ctx().request_repaint();

        // The pond on floor: where the line is, the others.
        self.fishing.pond.poll(now);
        let (line, cast) = self.fishing.line();
        let account = self.config.settings.floor.clone();
        self.fishing.pond.tick(ui.ctx(), now, account.as_ref(), line, cast);
        self.fishing.floor.poll();
        self.fishing.floor.sync(ui.ctx(), now, Some(60.0), account.as_ref());

        if !fighting {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.home_game = false;
                return;
            }
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 40.0), Align2::CENTER_CENTER, t.fishing_name, 34.0, theme.accent, 1.0);

        // The two tabs: the pond, the binder.
        let have = self.config.settings.fish_cards.len();
        let tabs = [(false, t.fishing_tab_pond.to_owned()), (true, t.fishing_tab_cards.replace("{n}", &have.to_string()).replace("{max}", &FISH.len().to_string()))];
        let mut x = rect.center().x - 160.0;
        for (cards, label) in tabs {
            let r = Rect::from_min_size(Pos2::new(x, rect.min.y + 66.0), Vec2::new(150.0, 28.0));
            let on = self.fishing.cards == cards;
            let b = egui::Button::new(egui::RichText::new(label).size(13.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.chrome_bg }).corner_radius(14.0);
            if ui.put(r, b).clicked() && !fighting {
                self.fishing.cards = cards;
                self.fishing.zoom = None;
            }
            x += 170.0;
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 106.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
        if self.fishing.cards {
            self.fishing_binder(ui, body, now);
        } else {
            self.fishing_pond(ui, body, now);
        }
        // What it unlocks (the theme, the sheep, the badge, the frame), out of a fight.
        if !self.fishing.fighting() && self.fishing.zoom.is_none() {
            self.unlocks_pill(ui, rect, HomeGame::Fishing);
        }
    }

    /// The pond: the scene, the line, the fight, the card just won.
    fn fishing_pond(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let room = Vec2::new(body.width(), body.height() - 34.0);
        let scale = (room.x / W).min(room.y / H).max(0.2);
        let field = Rect::from_min_size(Pos2::new(body.center().x - W * scale / 2.0, body.min.y), Vec2::new(W, H) * scale);

        // Held: Space, or the mouse on the scene (only when nothing else has the keyboard).
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let resp = ui.interact(field, ui.id().with("fishing-field"), Sense::click_and_drag());
        let held = ui.input(|i| free && (i.key_down(Key::Space) || i.key_down(Key::Enter))) || resp.is_pointer_button_down_on();
        let pressed = held && !self.fishing.held;
        let released = !held && self.fishing.held;
        self.fishing.held = held;

        let left = self.fish_left();
        let dt = ui.input(|i| i.stable_dt).min(0.1);

        // The aim: the pointer over the scene, or ← →, until the line goes.
        let me = self.fishing.pond.view.as_ref().and_then(|v| v.you).unwrap_or(2).min(SPOTS - 1);
        let origin = cast_origin(me);
        if matches!(self.fishing.phase, Phase::Idle | Phase::Charging { .. }) {
            if let Some(p) = ui.input(|i| i.pointer.latest_pos()).filter(|p| field.contains(*p)) {
                let p = ((p - field.min) / scale).to_pos2();
                if p.y < origin.y - 4.0 {
                    self.fishing.aim = (p.x - origin.x).atan2(origin.y - p.y).clamp(-AIM_MAX, AIM_MAX);
                }
            }
            let turn = ui.input(|i| free as i32 * (i.key_down(Key::ArrowRight) as i32 - i.key_down(Key::ArrowLeft) as i32));
            self.fishing.aim = (self.fishing.aim + turn as f32 * dt * 1.1).clamp(-AIM_MAX, AIM_MAX);
        }

        let mut caught = None;
        let mut thrown = None;
        let mut sfx: Option<&'static [(f32, f32, f32, f32)]> = None;
        match &mut self.fishing.phase {
            Phase::Idle => {
                if pressed {
                    if left.as_ref().is_some_and(|(n, _)| *n == 0) {
                        // The half-day's quota hired: the pond only to watch.
                    } else {
                        self.fishing.phase = Phase::Charging { power: 0.0, rising: true };
                        self.fishing.message = None;
                    }
                }
            }
            Phase::Charging { power, rising } => {
                *power += if *rising { 1.0 } else { -1.0 } * dt * 0.9;
                if *power >= 1.0 {
                    *power = 1.0;
                    *rising = false;
                } else if *power <= 0.0 {
                    *power = 0.0;
                    *rising = true;
                }
                if released {
                    thrown = Some(power.max(0.05));
                }
            }
            Phase::Waiting { cast, hook, bite_in, nibbles, elapsed, splashed } => {
                *elapsed += dt;
                if !*splashed && *elapsed >= LET_GO + FLIGHT_BASE + FLIGHT_PER_CAST * *cast {
                    *splashed = true;
                    sfx = Some(&[(420.0, 110.0, 0.14, 0.35)]);
                }
                let nibbling = nibbles.iter().any(|&n| n >= 0.0 && *elapsed >= n && *elapsed < n + NIBBLE);
                if nibbles.iter().any(|&n| n >= 0.0 && *elapsed >= n && *elapsed - dt < n) {
                    sfx = Some(&[(1250.0, 1100.0, 0.035, 0.12)]);
                }
                if pressed {
                    // Reeled in empty; on a nibble, it took fright.
                    if nibbling {
                        self.fishing.message = Some((t.fishing_spooked.to_owned(), now + 3.0));
                        sfx = Some(&[(380.0, 200.0, 0.2, 0.3)]);
                    }
                    self.fishing.phase = Phase::Idle;
                } else if *elapsed >= *bite_in {
                    self.fishing.phase = Phase::Bite { cast: *cast, hook: *hook, left: BITE_TIME };
                    sfx = Some(&[(900.0, 1300.0, 0.06, 0.3), (900.0, 1300.0, 0.06, 0.3)]);
                }
            }
            Phase::Bite { cast, hook, left } => {
                *left -= dt;
                if pressed {
                    let (cast, hook) = (*cast, *hook);
                    let r = super::game::random(&mut self.fishing.rng);
                    let rare = FISH[hook.fish].rarity.index() >= Rarity::Rare.index();
                    let offer_at = if r % 100 < if rare { 35 } else { 12 } { 2.0 + (r >> 8) as f32 % 4000.0 / 1000.0 } else { -1.0 };
                    self.fishing.phase = Phase::Reeling { cast, reel: Reel::new(hook.fish, hook.seed, cast), hook, t: 0.0, offer_at };
                    self.fishing.acc = 0.0;
                } else if *left <= 0.0 {
                    self.fishing.phase = Phase::Idle;
                    self.fishing.message = Some((t.fishing_late.to_owned(), now + 3.0));
                }
            }
            Phase::Reeling { reel, hook, t: fight, offer_at, .. } => {
                *fight += dt;
                if *offer_at >= 0.0 && *fight >= *offer_at {
                    *offer_at = -1.0;
                    reel.offer = OFFER;
                    sfx = Some(&[(220.0, 220.0, 0.14, 0.4), (175.0, 175.0, 0.22, 0.4)]);
                }
                self.fishing.acc += dt;
                while self.fishing.acc >= STEP {
                    self.fishing.acc -= STEP;
                    match reel.step(STEP, held) {
                        Some(true) => {
                            caught = Some((reel.fish, hook.size, hook.shiny));
                            break;
                        }
                        Some(false) => {
                            self.fishing.phase = Phase::Idle;
                            self.fishing.message = Some((t.fishing_lost.to_owned(), now + 3.0));
                            sfx = Some(&[(300.0, 140.0, 0.3, 0.35)]);
                            break;
                        }
                        None => {}
                    }
                }
            }
            Phase::Caught { at, .. } => {
                // Stays until put away (a press, once the hand let go of the reel): time to read it.
                if pressed && now - *at > LEAP + 0.6 {
                    self.fishing.phase = Phase::Idle;
                }
            }
        }
        if let Some(cast) = thrown {
            self.fishing_cast(origin, cast, now);
            sfx = Some(&[(260.0, 820.0, 0.13, 0.12)]);
        }
        if let Some((fish, size, shiny)) = caught {
            self.fishing_hired(fish, size, shiny, now);
            sfx = Some(hired_sound(FISH[fish].rarity, shiny));
        }
        if let Some(notes) = sfx {
            self.fishing_sound(notes);
        }

        self.paint_pond(ui, field, scale, now);

        // Under the scene: what to do, or what just happened.
        let hint = match &self.fishing.phase {
            _ if self.fishing.message.as_ref().is_some_and(|(_, until)| now < *until) => self.fishing.message.as_ref().map(|(m, _)| m.clone()).unwrap_or_default(),
            Phase::Idle => match &left {
                Some((0, p)) => (if p.morning { t.fishing_done_morning } else { t.fishing_done_afternoon }).replace("{n}", &crate::playtime::FISH_CATCHES.to_string()).replace("{h}", &self.hour_text(p.end)),
                _ => t.fishing_cast.to_owned(),
            },
            Phase::Charging { .. } => t.fishing_cast.to_owned(),
            Phase::Waiting { hook, .. } if hook.hot => t.fishing_wait_hot.to_owned(),
            Phase::Waiting { .. } => t.fishing_wait.to_owned(),
            Phase::Bite { .. } => t.fishing_bite.to_owned(),
            Phase::Reeling { reel, .. } if reel.offer > 0.0 => t.fishing_offer.to_owned(),
            Phase::Reeling { .. } => t.fishing_help.to_owned(),
            Phase::Caught { .. } => t.fishing_put_away.to_owned(),
        };
        let alarm = matches!(&self.fishing.phase, Phase::Bite { .. }) || matches!(&self.fishing.phase, Phase::Reeling { reel, .. } if reel.offer > 0.0);
        let color = if alarm { theme.accent } else { theme.text_muted };
        ui.painter().text(Pos2::new(field.center().x, field.max.y + 18.0), Align2::CENTER_CENTER, hint, FontId::proportional(13.0), color);

        // The baits won: one put on the hook (out of a cast), used one a cast.
        let idle = matches!(self.fishing.phase, Phase::Idle);
        let mut y = field.min.y + 62.0 * scale;
        let mut picked = None;
        for bait in Bait::ALL {
            let n = self.config.settings.fish_baits.get(bait.id()).copied().unwrap_or(0);
            if n == 0 {
                continue;
            }
            let on = self.config.settings.fish_bait.as_deref() == Some(bait.id());
            let name = t.fishing_baits.split('|').nth(bait.index()).unwrap_or_default();
            let effect = t.fishing_bait_effects.split('|').nth(bait.index()).unwrap_or_default();
            let text = egui::RichText::new(format!("{}  {name}  ×{n}", ["🎫", "🏠", "💶"][bait.index()])).size(12.0).color(if on { theme.bg } else { theme.text });
            let b = egui::Button::new(text).fill(if on { theme.accent } else { theme.chrome_bg.gamma_multiply(0.9) }).corner_radius(12.0);
            let r = Rect::from_min_size(Pos2::new(field.min.x + 12.0 * scale, y), Vec2::new(176.0, 24.0));
            if ui.put(r, b).on_hover_text(effect).clicked() && idle {
                picked = Some(if on { None } else { Some(bait.id().to_owned()) });
            }
            y += 28.0;
        }
        if let Some(bait) = picked {
            self.config.settings.fish_bait = bait;
            self.save_config();
        }
    }

    /// The line let go with `power`: where it lands, who will bite there (with the bait used, in the
    /// bubbles or not), when, and the nibbles before.
    fn fishing_cast(&mut self, origin: Pos2, power: f32, now: f64) {
        let float = landing(origin, self.fishing.aim, power);
        self.fishing.float = float;
        let unix = unix_now();
        let hot = in_bubbles(unix, float);
        let bait = self.fishing_take_bait();
        let rng = &mut self.fishing.rng;
        let (fish, size) = crate::fishing::pick(rng, hour(), power, Lure { bait, hot });
        let seed = super::game::random(rng);
        let shiny = super::game::random(rng).is_multiple_of(SHINY_ONE_IN);
        // Patience: 7 to 18 s, sooner with tickets resto, in the bubbles, at a job fair.
        let mut wait = 7.0 + (super::game::random(rng) % 11_000) as f32 / 1000.0;
        if bait == Some(Bait::Tickets) {
            wait *= 0.65;
        }
        if hot {
            wait *= 0.6;
        }
        if crate::fishing::job_fair(unix as i64).is_some() {
            wait *= 0.6;
        }
        let landed = LET_GO + FLIGHT_BASE + FLIGHT_PER_CAST * power;
        let bite_in = landed + wait.max(3.5);
        // Up to three nibbles before, none in the last second.
        let mut nibbles = [-1.0f32; 3];
        let room = bite_in - landed - 2.2;
        let count = if room > 0.5 { super::game::random(rng) % 4 } else { 0 } as usize;
        for n in nibbles.iter_mut().take(count) {
            *n = landed + 1.0 + (super::game::random(rng) % 1000) as f32 / 1000.0 * room;
        }
        self.fishing.phase = Phase::Waiting { cast: power, hook: Hook { fish, size, shiny, seed, hot }, bite_in, nibbles, elapsed: 0.0, splashed: false };
        self.fishing.cast_at = now;
        self.fishing.message = None;
    }

    /// The bait on the hook, one used (none left: taken off).
    fn fishing_take_bait(&mut self) -> Option<Bait> {
        let s = &mut self.config.settings;
        let id = s.fish_bait.clone()?;
        let bait = Bait::by_id(&id);
        let left = s.fish_baits.get_mut(&id).map(|n| {
            *n = n.saturating_sub(1);
            *n
        });
        if left.unwrap_or(0) == 0 {
            s.fish_baits.remove(&id);
            s.fish_bait = None;
        }
        self.save_config();
        bait.filter(|_| left.is_some())
    }

    /// A little sound, unless the games are muted.
    fn fishing_sound(&mut self, notes: &[(f32, f32, f32, f32)]) {
        if self.config.settings.game_sound == Some(false) {
            return;
        }
        if self.fishing.sound.is_none() {
            self.fishing.sound = super::blob::Sound::open();
        }
        if let Some(sound) = &self.fishing.sound {
            sound.tones(notes);
        }
    }

    /// A candidate hired: its card in the binder, counted for the half-day, told to the pond and to
    /// floor's board.
    fn fishing_hired(&mut self, fish: usize, size: u32, shiny: bool, now: f64) {
        let id = FISH[fish].id;
        let all = FISH.len() as u32;
        let tally = |cards: &std::collections::BTreeMap<String, crate::config::FishCard>| (cards.len() as u32, cards.values().filter(|c| c.shiny > 0).count() as u32, cards.values().map(|c| c.count).sum::<u32>());
        let (species_before, shiny_before, hires_before) = tally(&self.config.settings.fish_cards);
        let card = self.config.settings.fish_cards.entry(id.to_owned()).or_default();
        let new = card.count == 0;
        let record = !new && size > card.best_cm;
        card.count += 1;
        card.best_cm = card.best_cm.max(size);
        if shiny {
            card.shiny += 1;
        }
        if new {
            card.first_at = chrono::Utc::now().timestamp();
        }
        // A bait won: with each new card, else one hire in three.
        let r = super::game::random(&mut self.fishing.rng);
        let bait = (new || r.is_multiple_of(3)).then(|| Bait::ALL[(r >> 8) as usize % Bait::ALL.len()]);
        if let Some(b) = bait {
            *self.config.settings.fish_baits.entry(b.id().to_owned()).or_default() += 1;
        }
        self.fishing.phase = Phase::Caught { fish, size, shiny, new, record, bait, at: now };
        self.count_fish();
        self.save_config();
        self.fishing.pond.caught(id, size, shiny);
        let cards = &self.config.settings.fish_cards;
        let (species, shinies, catches) = tally(cards);
        let biggest = cards.values().map(|c| c.best_cm).max().unwrap_or(size);
        let points = crate::fishing::points(cards);

        // What it unlocks: said.
        let t = self.t();
        let mut said = vec![];
        if species_before < all && species >= all {
            said.push((t.office_unlocked.to_owned(), t.office_unlocked_body.to_owned()));
        }
        if new && FISH[fish].rarity == Rarity::Legendary {
            said.push((t.sheep_unlocked.to_owned(), t.sheep_unlocked_body.to_owned()));
        }
        if hires_before < crate::fishing::HUNTER && catches >= crate::fishing::HUNTER {
            said.push((t.hunter_unlocked.to_owned(), t.hunter_unlocked_body.to_owned()));
        }
        if shiny_before < all && shinies >= all {
            said.push((t.frame_unlocked.replace("{n}", super::frames::frame_name(t, "holo")), t.frame_unlocked_body.to_owned()));
        }
        for (title, body) in said {
            self.push_toast(super::Toast { ok: true, title, body, tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
        }

        if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.fishing.floor.finished(crate::floor::GameRound { score: points, details: serde_json::json!({ "points": points, "species": species, "shiny": shinies, "catches": catches, "biggest": biggest }) });
            self.fishing.floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
        }
    }

    /// The scene: the sky at this hour, the far bank, the water, the fishers on the near bank (the
    /// player and the colleagues at the pond), the lines; the fight, the card just won.
    fn paint_pond(&self, ui: &Ui, field: Rect, scale: f32, now: f64) {
        let t = self.t();
        let theme = &self.theme;
        let painter = ui.painter_at(field);
        let at = |x: f32, y: f32| field.min + Vec2::new(x, y) * scale;
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
        let hour = hour();
        let (sky_top, sky_low, water, night) = scene_colors(hour);
        let tm = now as f32;

        gradient(&painter, r(0.0, 0.0, W, WATER_TOP), sky_top, sky_low);
        if night {
            for k in 0..60u32 {
                let h = k.wrapping_mul(2_654_435_761);
                let p = at((h % 960) as f32, ((h >> 12) % 170) as f32);
                let twinkle = 0.4 + 0.6 * ((tm * 1.5 + k as f32).sin() * 0.5 + 0.5);
                painter.circle_filled(p, 1.1 * scale, Color32::WHITE.gamma_multiply(twinkle));
            }
            painter.circle_filled(at(800.0, 60.0), 22.0 * scale, hex(0xf3f0d8));
            painter.circle_filled(at(810.0, 54.0), 20.0 * scale, sky_top);
        } else {
            let (sx, sy, sun) = match hour {
                5..=11 => (180.0, 120.0, hex(0xffe3a0)),
                12..=17 => (700.0, 60.0, hex(0xfff3c4)),
                _ => (480.0, 190.0, hex(0xff9a5a)),
            };
            painter.circle_filled(at(sx, sy), 34.0 * scale, sun.gamma_multiply(0.35));
            painter.circle_filled(at(sx, sy), 24.0 * scale, sun);
            // Clouds drifting.
            for k in 0..4 {
                let x = ((k as f32 * 290.0 + tm * (6.0 + k as f32 * 2.0)) % (W + 200.0)) - 100.0;
                let y = 40.0 + k as f32 * 28.0;
                for (dx, dy, rr) in [(0.0, 0.0, 18.0), (20.0, -8.0, 22.0), (44.0, 0.0, 17.0), (22.0, 6.0, 18.0)] {
                    painter.circle_filled(at(x + dx, y + dy), rr * scale, Color32::WHITE.gamma_multiply(0.55));
                }
            }
        }
        // The far bank: hills and trees, dark against the sky.
        let far = lerp(hex(0x2f5a3a), sky_top, if night { 0.55 } else { 0.25 });
        let mut hills = vec![at(0.0, WATER_TOP)];
        for k in 0..=24 {
            let x = k as f32 * W / 24.0;
            let y = WATER_TOP - 18.0 - 14.0 * ((x * 0.011).sin() + 0.6 * (x * 0.027 + 1.3).sin());
            hills.push(at(x, y));
        }
        hills.push(at(W, WATER_TOP));
        painter.add(egui::Shape::convex_polygon(vec![at(0.0, WATER_TOP), at(W, WATER_TOP), at(W, WATER_TOP - 30.0), at(0.0, WATER_TOP - 30.0)], far, Stroke::NONE));
        painter.add(egui::Shape::line(hills.clone(), Stroke::new(1.0 * scale, far)));
        for k in 0..24 {
            let x = k as f32 * W / 24.0 + 20.0;
            let y = WATER_TOP - 18.0 - 14.0 * ((x * 0.011).sin() + 0.6 * (x * 0.027 + 1.3).sin());
            painter.add(egui::Shape::convex_polygon(vec![at(x - 22.0, WATER_TOP), at(x + 22.0, WATER_TOP), at(x + 22.0, y), at(x - 22.0, y)], far, Stroke::NONE));
        }
        for k in 0..14u32 {
            let h = k.wrapping_mul(2_246_822_519);
            let x = (h % 940) as f32 + 10.0;
            let tall = 30.0 + (h >> 10) as f32 % 34.0;
            let base = WATER_TOP - 22.0;
            let tree = lerp(far, hex(0x10261a), 0.35);
            painter.add(egui::Shape::convex_polygon(vec![at(x, base - tall), at(x + 12.0, base), at(x - 12.0, base)], tree, Stroke::NONE));
        }

        // The water, its ripples moving, lily pads, reeds.
        gradient(&painter, r(0.0, WATER_TOP, W, BANK), lerp(water, sky_low, 0.25), lerp(water, Color32::BLACK, 0.25));
        for k in 0..26u32 {
            let h = k.wrapping_mul(2_654_435_761);
            let y = WATER_TOP + 12.0 + (h % 210) as f32;
            let x = ((h >> 8) % 960) as f32 + (tm * 0.35 + k as f32).sin() * 18.0;
            let w = 14.0 + (h >> 20) as f32 % 30.0;
            painter.line_segment([at(x - w, y), at(x + w, y)], Stroke::new(1.2 * scale, Color32::WHITE.gamma_multiply(0.12 + 0.08 * (tm * 0.8 + k as f32).sin())));
        }
        for (x, y, rr) in [(90.0, 300.0, 16.0), (130.0, 318.0, 11.0), (840.0, 270.0, 14.0), (612.0, 250.0, 10.0)] {
            painter.circle_filled(at(x, y), rr * scale, hex(0x3f8a4a));
            painter.add(egui::Shape::convex_polygon(vec![at(x, y), at(x + rr, y - 3.0), at(x + rr, y + 3.0)], lerp(water, Color32::BLACK, 0.1), Stroke::NONE));
            if x < 100.0 {
                painter.circle_filled(at(x - 4.0, y - 3.0), 4.0 * scale, hex(0xf2a5c0));
            }
        }
        // The bubbles where a shoal of candidates is (the same for everyone at the pond).
        let unix = unix_now();
        for (k, (spot, strength)) in hotspots(unix).into_iter().enumerate() {
            if strength <= 0.0 {
                continue;
            }
            for b in 0..9 {
                let phase = ((tm * 0.7 + b as f32 * 0.37 + k as f32 * 0.5) % 1.0 + 1.0) % 1.0;
                let h = (b as u32 + 1).wrapping_mul(2_654_435_761);
                let dx = ((h % 100) as f32 - 50.0) * 0.6 + (tm * 2.0 + b as f32).sin() * 3.0;
                let dy = ((h >> 8) % 40) as f32 - 20.0 - phase * 10.0;
                let alpha = strength * (1.0 - phase) * 0.8;
                painter.circle_stroke(at(spot.x + dx, spot.y + dy * 0.5), (1.5 + phase * 2.5) * scale, Stroke::new(1.0, Color32::WHITE.gamma_multiply(alpha)));
            }
            painter.add(egui::Shape::ellipse_stroke(at(spot.x, spot.y + 4.0), Vec2::new(HOT_R * 0.7, HOT_R * 0.22) * scale, Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.12 * strength))));
        }
        // A job fair: bunting over the far bank, and how long it lasts.
        if let Some(left) = crate::fishing::job_fair(unix as i64) {
            let colors = [0xe0453a, 0xf2b832, 0x4f8fe8, 0x5fd06a, 0xa86be8];
            let rope: Vec<Pos2> = (0..=24).map(|k| at(k as f32 * W / 24.0, WATER_TOP - 52.0 + ((k as f32 / 4.0 * std::f32::consts::PI).sin()).abs() * 10.0)).collect();
            painter.add(egui::Shape::line(rope.clone(), Stroke::new(1.0 * scale, Color32::WHITE.gamma_multiply(0.7))));
            for (k, w) in rope.windows(2).enumerate() {
                let sway = (tm * 2.0 + k as f32).sin() * 1.5 * scale;
                let mid = Pos2::new((w[0].x + w[1].x) / 2.0 + sway, (w[0].y + w[1].y) / 2.0 + 12.0 * scale);
                painter.add(egui::Shape::convex_polygon(vec![w[0], w[1], mid], hex(colors[k % colors.len()]), Stroke::NONE));
            }
            let text = t.fishing_fair.replace("{n}", &((left + 59) / 60).to_string());
            let galley = painter.layout_no_wrap(text, FontId::proportional(13.0 * scale.max(0.75)), Color32::WHITE);
            let banner = Rect::from_center_size(at(W / 2.0, 40.0), galley.size() + Vec2::new(22.0, 10.0));
            painter.rect_filled(banner, 12.0, hex(0xe0453a).gamma_multiply(0.9));
            painter.galley(banner.center() - galley.size() / 2.0, galley, Color32::WHITE);
        }

        // The near bank.
        gradient(&painter, r(0.0, BANK, W, H), hex(0x5f9a4a), hex(0x3d6b32));
        painter.line_segment([at(0.0, BANK), at(W, BANK)], Stroke::new(3.0 * scale, hex(0x7a6a48)));
        for k in 0..30u32 {
            let h = k.wrapping_mul(2_246_822_519);
            let x = (h % 960) as f32;
            let sway = (tm * 1.3 + k as f32).sin() * 2.0;
            for d in [-3.0, 0.0, 3.0] {
                painter.line_segment([at(x + d, BANK + 4.0), at(x + d * 1.6 + sway, BANK - 14.0 - (h >> 9) as f32 % 10.0)], Stroke::new(1.4 * scale, hex(0x4c7d38)));
            }
        }
        if night {
            painter.rect_filled(field, 0.0, Color32::from_black_alpha(40));
        }

        // The fishers: the colleagues as floor last said, the player as they are now.
        let view = self.fishing.pond.view.as_ref();
        let me = view.and_then(|v| v.you).unwrap_or(2).min(SPOTS - 1);
        let mut rows = vec![];
        if let Some(v) = view {
            for f in v.fishers.iter().filter(|f| !f.me && f.spot < SPOTS) {
                rows.push(f);
            }
        }
        for f in &rows {
            let shirt = theme.ansi[1 + (f.name.bytes().map(u32::from).sum::<u32>() as usize % 6)];
            let (cast, bend) = (f.cast, f.state == Line::Reel);
            self.paint_fisher(&painter, at, scale, f.spot, &f.name, shirt, false, f.state, bend, Swing::Still, float_at(f.spot, cast), now);
            // Their catch, a moment over their head.
            if let Some(c) = f.catch.as_ref().filter(|c| c.ago + (now - self.fishing.pond.at) * 1000.0 < SHOWN_MS)
                && let Some(k) = crate::fishing::by_id(&c.fish)
            {
                {
                    let name = card_text(t, k).name;
                    let text = format!("{name} · {} cm{}", c.size, if c.shiny { " ✦" } else { "" });
                    let x = spot_x(f.spot);
                    let galley = painter.layout_no_wrap(text, FontId::proportional(11.5 * scale.max(0.7)), theme.text);
                    let bubble = Rect::from_center_size(at(x, FEET - 118.0), galley.size() + Vec2::new(16.0, 8.0));
                    painter.rect(bubble, 8.0, theme.chrome_bg.gamma_multiply(0.95), Stroke::new(1.5, rarity_color(FISH[k].rarity)), egui::StrokeKind::Inside);
                    painter.galley(bubble.center() - galley.size() / 2.0, galley, theme.text);
                    paint_candidate(&painter, at(x + 20.0, FEET - 82.0), 34.0 * scale, k, None, now);
                }
            }
        }
        let name = crate::floor::pseudo(self.config.settings.floor.as_ref()).unwrap_or(t.game_you);
        let line = self.fishing.line().0;
        let swing = match &self.fishing.phase {
            Phase::Charging { power, .. } => Swing::Back(*power),
            Phase::Waiting { cast, .. } if now - self.fishing.cast_at < 3.0 => Swing::Thrown { from: *cast, t: (now - self.fishing.cast_at) as f32 },
            _ => Swing::Still,
        };
        // Under the float: the candidate coming (its shadow), nibbling, biting; splashing in the fight.
        self.paint_under_float(&painter, at, scale, now);
        // A nibble taps the float under for a moment.
        let dip = match &self.fishing.phase {
            Phase::Waiting { nibbles, elapsed, .. } => nibbles.iter().filter(|&&n| n >= 0.0 && *elapsed >= n && *elapsed < n + NIBBLE).map(|&n| ((elapsed - n) / NIBBLE * std::f32::consts::PI).sin() * 5.0).sum(),
            _ => 0.0,
        };
        self.paint_fisher(&painter, at, scale, me, name, theme.accent, true, line, matches!(self.fishing.phase, Phase::Reeling { .. }), swing, self.fishing.float + Vec2::new(0.0, dip), now);
        self.paint_aim(&painter, at, scale, me, now);

        // The cast charging: a gauge beside the player.
        if let Phase::Charging { power, .. } = &self.fishing.phase {
            let x = spot_x(me) - 46.0;
            let gauge = r(x - 7.0, FEET - 110.0, x + 7.0, FEET - 10.0);
            painter.rect(gauge, 4.0, theme.chrome_bg, Stroke::new(1.0, theme.text_muted), egui::StrokeKind::Inside);
            let full = Rect::from_min_max(Pos2::new(gauge.min.x, gauge.max.y - gauge.height() * power), gauge.max).shrink(2.0);
            painter.rect_filled(full, 3.0, lerp(hex(0xe0b040), hex(0x5fd06a), *power));
        }

        // Top left: the half-day's hires; top right, the pond's news.
        let hud = FontId::proportional(13.0 * scale.max(0.75));
        let mut y = 14.0;
        if let Some((n, _)) = self.fish_left_peek() {
            painter.text(at(14.0, y), Align2::LEFT_TOP, t.fishing_left.replace("{n}", &(crate::playtime::FISH_CATCHES - n).to_string()).replace("{max}", &crate::playtime::FISH_CATCHES.to_string()), hud.clone(), Color32::WHITE);
            y += 22.0;
        }
        let others = rows.len();
        let alone = if view.is_none() { Some(t.fishing_offline) } else if others == 0 { Some(t.fishing_alone) } else { None };
        if let Some(alone) = alone {
            painter.text(at(14.0, y), Align2::LEFT_TOP, alone, FontId::proportional(12.0 * scale.max(0.75)), Color32::WHITE.gamma_multiply(0.75));
        }
        if let Some(v) = view {
            // The last minute's, the latest first.
            for (k, n) in v.feed.iter().filter(|n| n.ago < 60_000.0).take(3).enumerate() {
                let Some(f) = crate::fishing::by_id(&n.fish) else { continue };
                let mut text = t.fishing_news.replace("{name}", &n.name).replace("{fish}", card_text(t, f).name).replace("{size}", &n.size.to_string());
                if n.shiny {
                    text.push_str(" ✦");
                }
                painter.text(at(W - 14.0, 14.0 + k as f32 * 20.0), Align2::RIGHT_TOP, text, FontId::proportional(12.0 * scale.max(0.75)), Color32::WHITE.gamma_multiply(1.0 - k as f32 * 0.25));
            }
        }

        // The fight: the track beside the player, the bar, the candidate, how far the hire is.
        if let Phase::Reeling { reel, .. } = &self.fishing.phase {
            let side = if me < SPOTS / 2 { 1.0 } else { -1.0 };
            let x = spot_x(me) + side * 150.0;
            let track = r(x - 22.0, 104.0, x + 22.0, 420.0);
            let panel = track.expand(10.0 * scale);
            let rarity = FISH[reel.fish].rarity;
            let rival = reel.offer > 0.0;
            let edge = if rival && (tm * 6.0).sin() > 0.0 { hex(0xe0453a) } else { rarity_color(rarity) };
            painter.rect(Rect::from_min_max(panel.min, Pos2::new(panel.max.x + 26.0 * scale, panel.max.y)), 10.0, theme.chrome_bg.gamma_multiply(0.95), Stroke::new(2.0, edge), egui::StrokeKind::Inside);
            // Who it is, how rare, how it moves.
            let top = Pos2::new(panel.center().x + 13.0 * scale, panel.min.y - 8.0 * scale);
            let kind = format!("{} · {}", t.fishing_rarities.split('|').nth(rarity.index()).unwrap_or_default(), t.fishing_moods.split('|').nth(FISH[reel.fish].behavior as usize).unwrap_or_default());
            painter.text(top - Vec2::new(0.0, 16.0 * scale.max(0.75)), Align2::CENTER_BOTTOM, card_text(t, reel.fish).name, FontId::proportional(15.0 * scale.max(0.75)), Color32::WHITE);
            painter.text(top, Align2::CENTER_BOTTOM, kind, FontId::proportional(11.5 * scale.max(0.75)), rarity_color(rarity));
            if rival {
                painter.text(Pos2::new(panel.center().x + 13.0 * scale, panel.max.y + 6.0 * scale), Align2::CENTER_TOP, t.fishing_offer_tag, FontId::proportional(13.0 * scale.max(0.75)), hex(0xff6a5a));
            }
            painter.rect_filled(track, 4.0, lerp(water, Color32::BLACK, 0.35));
            let y_of = |v: f32| track.max.y - v * track.height();
            let bar = Rect::from_min_max(Pos2::new(track.min.x + 2.0, y_of(reel.bar_y + reel.bar_h)), Pos2::new(track.max.x - 2.0, y_of(reel.bar_y)));
            let on = reel.on_bar();
            painter.rect(bar, 4.0, hex(0x5fd06a).gamma_multiply(if on { 0.75 } else { 0.45 }), Stroke::new(1.5, hex(0x8ff09a)), egui::StrokeKind::Inside);
            let shake = if on { 0.0 } else { (tm * 40.0).sin() * 2.0 };
            paint_candidate(&painter, Pos2::new(track.center().x + shake * scale, y_of(reel.fish_y)), 34.0 * scale, reel.fish, None, now);
            let gauge = Rect::from_min_max(Pos2::new(track.max.x + 12.0 * scale, track.min.y), Pos2::new(track.max.x + 24.0 * scale, track.max.y));
            painter.rect_filled(gauge, 3.0, Color32::from_black_alpha(80));
            let full = Rect::from_min_max(Pos2::new(gauge.min.x, gauge.max.y - gauge.height() * reel.progress), gauge.max);
            painter.rect_filled(full, 3.0, lerp(hex(0xe05a4a), hex(0x5fd06a), reel.progress));
        }

        // Hired: its card, big, in the middle.
        if let Phase::Caught { fish, size, shiny, new, record, bait, at: when } = &self.fishing.phase {
            let age = now - when;
            let h = (H - 90.0) * scale;
            let centre = field.center() + Vec2::new(0.0, 14.0 * scale);
            if age < LEAP {
                // Out of the water in an arc, growing, toward where its card shows.
                let u = (age / LEAP) as f32;
                let from = at(self.fishing.float.x, self.fishing.float.y);
                let p = from + (centre - from) * u - Vec2::new(0.0, 160.0 * scale * 4.0 * u * (1.0 - u));
                if u < 0.6 {
                    for k in 0..8 {
                        let a = k as f32 / 8.0 * std::f32::consts::PI;
                        let d = u * 70.0 * scale;
                        painter.circle_filled(from + Vec2::new(-a.cos() * d, -a.sin() * d * 1.2 + u * u * 120.0 * scale), 2.0 * scale, Color32::WHITE.gamma_multiply(1.0 - u / 0.6));
                    }
                    painter.add(egui::Shape::ellipse_stroke(from, Vec2::new(8.0 + u * 60.0, 3.0 + u * 20.0) * scale, Stroke::new(1.5, Color32::WHITE.gamma_multiply(1.0 - u / 0.6))));
                }
                paint_candidate(&painter, p, (40.0 + 80.0 * u) * scale, *fish, None, now);
                return;
            }
            let age = age - LEAP;
            let k = (age / 0.3).min(1.0) as f32;
            painter.rect_filled(field, 0.0, Color32::from_black_alpha((110.0 * k) as u8));
            let card = Rect::from_center_size(centre, Vec2::new(h * 0.714, h));
            let rarity = FISH[*fish].rarity;
            // The rarer ones show their back first, then turn over; the others pop up.
            let turn = if rarity.index() >= Rarity::Rare.index() { 0.32 } else { 0.0 };
            if age < turn {
                let squeeze = (1.0 - age / turn) as f32;
                let back = Rect::from_center_size(centre, Vec2::new(card.width() * squeeze, card.height()));
                painter.rect_filled(back, 10.0, rarity_color(rarity));
                painter.rect_filled(back.shrink(6.0 * scale), 6.0, lerp(rarity_color(rarity), Color32::BLACK, 0.45));
                if squeeze > 0.3 {
                    painter.text(back.center(), Align2::CENTER_CENTER, "?", FontId::proportional(90.0 * scale * squeeze), rarity_color(rarity));
                }
                return;
            }
            let pop = ((age - turn) / 0.18).min(1.0) as f32;
            let card = Rect::from_center_size(centre, card.size() * (0.86 + 0.14 * ease_out(pop)));
            let entry = self.config.settings.fish_cards.get(FISH[*fish].id);
            paint_card(&painter, card, *fish, entry, *shiny, t, theme, now);
            if pop < 1.0 {
                painter.rect_filled(card, 10.0, Color32::WHITE.gamma_multiply(0.7 * (1.0 - pop)));
            }
            if let Some(b) = bait {
                let name = t.fishing_baits.split('|').nth(b.index()).unwrap_or_default();
                painter.text(Pos2::new(card.center().x, card.max.y + 30.0 * scale), Align2::CENTER_TOP, t.fishing_bait_won.replace("{bait}", name), FontId::proportional(13.0 * scale.max(0.7)), hex(0xffd23a));
            }
            let mut title = t.fishing_caught.to_owned();
            for (on, word) in [(*new, t.fishing_new_card), (*record, t.fishing_record), (*shiny, t.fishing_shiny)] {
                if on {
                    title = format!("{title}  {word}");
                }
            }
            paint_metal(&painter, Pos2::new(card.center().x, card.min.y - 22.0 * scale), Align2::CENTER_CENTER, &title, 26.0 * scale.max(0.6), theme.accent, 1.0);
            painter.text(Pos2::new(card.center().x, card.max.y + 10.0 * scale), Align2::CENTER_TOP, t.fishing_size.replace("{n}", &size.to_string()), FontId::proportional(14.0 * scale.max(0.7)), Color32::WHITE);
        }
    }

    /// The half-day's hires left, without counting anything (for the painting).
    fn fish_left_peek(&self) -> Option<(u32, ())> {
        if !crate::config::OFFICIAL {
            return None;
        }
        let p = crate::playtime::period(chrono::Local::now().naive_local())?;
        let usage = &self.config.settings.game_usage;
        let used = if usage.period == p.key { usage.fish } else { 0 };
        Some((crate::playtime::FISH_CATCHES.saturating_sub(used), ()))
    }

    /// Under the player's float: the candidate who will bite, a shadow coming in the last seconds,
    /// tapping it on a nibble, pulling it under at the bite; splashing in the fight.
    fn paint_under_float(&self, painter: &egui::Painter, at: impl Fn(f32, f32) -> Pos2, scale: f32, now: f64) {
        let float = self.fishing.float;
        let tm = now as f32;
        let shadow = |p: Pos2, fish: usize, alpha: f32| {
            let len = 22.0 + FISH[fish].rarity.index() as f32 * 9.0;
            paint_candidate(painter, at(p.x, p.y + 6.0), len * scale, fish, Some(Color32::from_black_alpha((alpha * 255.0) as u8)), now);
        };
        match &self.fishing.phase {
            Phase::Waiting { hook, bite_in, nibbles, elapsed, .. } => {
                let nibbling = nibbles.iter().any(|&n| n >= 0.0 && *elapsed >= n && *elapsed < n + NIBBLE);
                let coming = elapsed - (bite_in - 4.5);
                // From the left (it faces right), a little below, nearer and darker.
                let side = if hook.seed % 2 == 0 { 0.25 } else { -0.25 };
                let k = (coming / 4.5).clamp(0.0, 1.0);
                let near = nibbles.iter().any(|&n| n >= 0.0 && *elapsed >= n - 0.6 && *elapsed < n + NIBBLE + 0.4);
                if nibbling || near {
                    let d = if nibbling { 8.0 } else { 18.0 };
                    shadow(float + Vec2::new(-d - 10.0, 2.0 + (tm * 9.0).sin()), hook.fish, 0.28);
                } else if coming > 0.0 {
                    let d = (1.0 - k) * 110.0 + 14.0;
                    shadow(float + Vec2::new(-d - 10.0, d * side + (tm * 3.0).sin() * 2.0), hook.fish, 0.12 + 0.22 * k);
                }
                if nibbling {
                    painter.circle_stroke(at(float.x, float.y + 2.0), 9.0 * scale, Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.6)));
                }
            }
            Phase::Bite { hook, .. } => shadow(float + Vec2::new(-14.0, 3.0), hook.fish, 0.4),
            Phase::Reeling { .. } => {
                // Thrashing: rings and drops.
                let beat = (tm * 2.5) % 1.0;
                painter.add(egui::Shape::ellipse_stroke(at(float.x, float.y + 3.0), Vec2::new(8.0 + beat * 26.0, 3.0 + beat * 9.0) * scale, Stroke::new(1.3, Color32::WHITE.gamma_multiply(0.8 * (1.0 - beat)))));
                for k in 0..4 {
                    let a = k as f32 * 1.7 + tm * 7.0;
                    let up = ((tm * 4.0 + k as f32 * 0.25) % 1.0) * 1.0;
                    painter.circle_filled(at(float.x + a.cos() * 10.0, float.y - up * 16.0 + up * up * 16.0), 1.5 * scale, Color32::WHITE.gamma_multiply(0.8 * (1.0 - up)));
                }
            }
            _ => {}
        }
        // A rival firm's boat, coming for the candidate during its counter-offer.
        if let Phase::Reeling { reel, .. } = &self.fishing.phase
            && reel.offer > 0.0
        {
            let k = 1.0 - reel.offer / OFFER;
            let dir = if float.x > W / 2.0 { -1.0 } else { 1.0 };
            let x = float.x - dir * (260.0 * (1.0 - k.min(0.6) / 0.6) + 60.0);
            let y = float.y - 8.0 + (tm * 3.0).sin() * 1.5;
            painter.add(egui::Shape::convex_polygon(vec![at(x - 26.0, y), at(x + 26.0, y), at(x + 18.0, y + 10.0), at(x - 18.0, y + 10.0)], hex(0x8a2a2a), Stroke::NONE));
            painter.rect_filled(r_of(&at, x - 4.0, y - 22.0, x + 4.0, y), 2.0, hex(0x2a2a35));
            painter.circle_filled(at(x, y - 26.0), 5.0 * scale, hex(0xf0c8a0));
            painter.text(at(x + dir * 14.0, y - 30.0), Align2::CENTER_CENTER, "€€", FontId::proportional(14.0 * scale.max(0.7)), hex(0xffd23a));
            painter.add(egui::Shape::line(vec![at(x - dir * 30.0, y + 6.0), at(x - dir * 60.0, y + 4.0)], Stroke::new(1.2 * scale, Color32::WHITE.gamma_multiply(0.5))));
        }
    }

    /// The aim: out of a cast a few dots where the line would go; charging, the arc to where the
    /// float would land, ringed green over the bubbles.
    fn paint_aim(&self, painter: &egui::Painter, at: impl Fn(f32, f32) -> Pos2, scale: f32, me: usize, now: f64) {
        let origin = cast_origin(me);
        let aim = self.fishing.aim;
        match &self.fishing.phase {
            Phase::Idle if self.fishing.message.is_none() || now > self.fishing.message.as_ref().map_or(0.0, |m| m.1) => {
                for k in 1..=5 {
                    let d = 18.0 + k as f32 * 16.0;
                    painter.circle_filled(at(origin.x + aim.sin() * d, origin.y - aim.cos() * d), 2.0 * scale, Color32::WHITE.gamma_multiply(0.55 - k as f32 * 0.08));
                }
            }
            Phase::Charging { power, .. } => {
                let end = landing(origin, aim, *power);
                let high = 40.0 + 90.0 * power;
                for k in 1..14 {
                    let u = k as f32 / 14.0;
                    let p = origin + (end - origin) * u - Vec2::new(0.0, 4.0 * high * u * (1.0 - u));
                    painter.circle_filled(at(p.x, p.y), 1.8 * scale, Color32::WHITE.gamma_multiply(0.65));
                }
                let hot = in_bubbles(unix_now(), end);
                let color = if hot { hex(0x5fd06a) } else { Color32::WHITE };
                painter.add(egui::Shape::ellipse_stroke(at(end.x, end.y), Vec2::new(12.0, 4.5) * scale, Stroke::new(1.6, color.gamma_multiply(0.85))));
            }
            _ => {}
        }
    }

    /// A fisher on the bank at `spot`: a figure with a cap in `shirt`, its rod, its line and float
    /// where `line` is; their name under them.
    #[allow(clippy::too_many_arguments)]
    fn paint_fisher(&self, painter: &egui::Painter, at: impl Fn(f32, f32) -> Pos2, scale: f32, spot: usize, name: &str, shirt: Color32, me: bool, line: Line, bend: bool, swing: Swing, float_pos: Pos2, now: f64) {
        let x = spot_x(spot);
        let tm = now as f32;
        let skin = hex(0xf0c8a0);
        let dark = hex(0x2a2a35);
        let stroke = |w: f32, c: Color32| Stroke::new(w * scale, c);
        // Legs, body, arm, head, cap.
        painter.line_segment([at(x - 5.0, FEET - 26.0), at(x - 7.0, FEET)], stroke(4.0, dark));
        painter.line_segment([at(x + 5.0, FEET - 26.0), at(x + 7.0, FEET)], stroke(4.0, dark));
        painter.rect_filled(Rect::from_min_max(at(x - 10.0, FEET - 56.0), at(x + 10.0, FEET - 24.0)), 5.0 * scale, shirt);
        painter.circle_filled(at(x, FEET - 66.0), 9.0 * scale, skin);
        painter.add(egui::Shape::convex_polygon(vec![at(x - 9.0, FEET - 68.0), at(x - 7.0, FEET - 77.0), at(x + 7.0, FEET - 77.0), at(x + 9.0, FEET - 68.0)], shirt, Stroke::NONE));
        painter.line_segment([at(x + 6.0, FEET - 70.0), at(x + 14.0, FEET - 68.0)], stroke(2.0, shirt));
        let hand = at(x + 14.0, FEET - 44.0);
        painter.line_segment([at(x + 6.0, FEET - 50.0), hand], stroke(3.0, shirt));
        // The rod: upright out of the water, bent toward the float in a fight.
        let in_water = line != Line::Idle;
        // The rod swung: drawn by its angle (from upright, toward the water) about the hand, its
        // middle lagging behind the tip as it whips.
        let rod_at = |angle: f32, lag: f32| {
            let (hx, hy) = (x + 14.0, FEET - 44.0);
            let tip = at(hx + ROD * angle.sin(), hy - ROD * angle.cos());
            let mid = at(hx + ROD * 0.5 * (angle + lag).sin(), hy - ROD * 0.5 * (angle + lag).cos());
            (tip, mid)
        };
        let swung = match swing {
            Swing::Back(power) => Some(rod_at(ROD_REST - (ROD_REST - ROD_BACK) * ease_out(power), -0.06 * power)),
            Swing::Thrown { from, t } => {
                let (angle, lag) = rod_angle(from, t);
                Some(rod_at(angle, lag))
            }
            Swing::Still => None,
        };
        let (tip, mid) = if let Some(swung) = swung {
            swung
        } else {
            let tip = if !in_water {
                rod_at(ROD_REST, 0.0).0
            } else if bend {
                at(x + 44.0, FEET - 86.0 + (tm * 18.0).sin() * 3.0)
            } else {
                rod_at(ROD_WAIT, 0.0).0
            };
            (tip, if bend { at(x + 34.0, FEET - 92.0) } else { Pos2::new((hand.x + tip.x) / 2.0, (hand.y + tip.y) / 2.0) })
        };
        painter.add(egui::Shape::line(vec![hand, mid, tip], stroke(2.0, hex(0x6a4a2a))));
        // Pulled back: the float dangling under the tip.
        if let Swing::Back(_) = swing {
            let fp = tip + Vec2::new(0.0, 10.0 * scale);
            painter.line_segment([tip, fp], stroke(0.8, Color32::WHITE.gamma_multiply(0.7)));
            painter.circle_filled(fp, 4.0 * scale, hex(0xe0453a));
        }
        // Thrown: the float flying in an arc from the tip to where it lands, then a splash.
        if let (Swing::Thrown { from, t }, Line::Wait) = (swing, line) {
            let flight = FLIGHT_BASE + FLIGHT_PER_CAST * from;
            let landed = t - LET_GO - flight;
            if landed < 0.0 {
                let fp = if t < LET_GO {
                    tip + Vec2::new(0.0, 10.0 * scale)
                } else {
                    let (angle, _) = rod_angle(from, LET_GO);
                    let start = Pos2::new(x + 14.0 + ROD * angle.sin(), FEET - 44.0 - ROD * angle.cos() + 10.0);
                    let end = float_pos;
                    let u = ((t - LET_GO) / flight).clamp(0.0, 1.0);
                    let high = 40.0 + 90.0 * from;
                    at(start.x + (end.x - start.x) * u, start.y + (end.y - start.y) * u - 4.0 * high * u * (1.0 - u))
                };
                let middle = Pos2::new((tip.x + fp.x) / 2.0, (tip.y + fp.y) / 2.0 + 3.0 * scale);
                painter.add(egui::Shape::line(vec![tip, middle, fp], stroke(0.8, Color32::WHITE.gamma_multiply(0.7))));
                painter.circle_filled(fp, 4.0 * scale, hex(0xe0453a));
                painter.circle_filled(fp - Vec2::new(0.0, 3.0 * scale), 2.5 * scale, Color32::WHITE);
                return self.paint_fisher_name(painter, at, scale, x, name, me);
            }
            // The splash: rings spreading, drops thrown up and falling back.
            let float = float_pos;
            if landed < 0.9 {
                for (k, delay) in [0.0f32, 0.18].into_iter().enumerate() {
                    let a = landed - delay;
                    if a > 0.0 {
                        let fade = (1.0 - a / 0.72).max(0.0);
                        let radius = 5.0 + a * 34.0 - k as f32 * 2.0;
                        painter.add(egui::Shape::ellipse_stroke(at(float.x, float.y + 2.0), Vec2::new(radius, radius * 0.35) * scale, Stroke::new(1.3, Color32::WHITE.gamma_multiply(0.8 * fade))));
                    }
                }
                if landed < 0.45 {
                    for k in 0..7 {
                        let dx = (k as f32 - 3.0) * 5.0;
                        let up = 70.0 + (k * 37 % 5) as f32 * 14.0;
                        let (px, py) = (float.x + dx * landed * 6.0, float.y - up * landed + 260.0 * landed * landed);
                        painter.circle_filled(at(px, py), 1.6 * scale, Color32::WHITE.gamma_multiply(0.85 * (1.0 - landed / 0.45)));
                    }
                }
            }
        }
        if in_water {
            let mut float = float_pos;
            // Just landed: it sinks and bobs back up, less and less.
            if let Swing::Thrown { from, t } = swing {
                let landed = t - LET_GO - FLIGHT_BASE - FLIGHT_PER_CAST * from;
                if landed >= 0.0 {
                    float.y += (landed * 13.0).sin() * 5.0 * (-landed * 3.5).exp();
                }
            }
            let mut dip = 0.0;
            match line {
                Line::Wait => float.y += (tm * 2.2 + spot as f32).sin() * 1.5,
                Line::Bite => dip = 5.0 + (tm * 22.0).sin() * 3.0,
                Line::Reel => float.y += (tm * 25.0).sin() * 3.0,
                Line::Idle => {}
            }
            let fp = at(float.x, float.y + dip);
            // A slack line waiting (slackening as the float settles), taut in a fight.
            let settled = match swing {
                Swing::Thrown { from, t } => ((t - LET_GO - FLIGHT_BASE - FLIGHT_PER_CAST * from) / 0.6).clamp(0.0, 1.0),
                _ => 1.0,
            };
            let sag = if bend { 0.0 } else { 3.0 + 13.0 * settled };
            let middle = Pos2::new((tip.x + fp.x) / 2.0, (tip.y + fp.y) / 2.0 + sag * scale);
            painter.add(egui::Shape::line(vec![tip, middle, fp], stroke(0.8, Color32::WHITE.gamma_multiply(0.7))));
            painter.circle_filled(fp, 4.0 * scale, hex(0xe0453a));
            painter.circle_filled(fp - Vec2::new(0.0, 3.0 * scale), 2.5 * scale, Color32::WHITE);
            if line != Line::Wait {
                let ring = ((tm * 3.0) % 1.0) * 14.0;
                painter.circle_stroke(at(float.x, float.y + 2.0), (6.0 + ring) * scale, Stroke::new(1.0, Color32::WHITE.gamma_multiply(1.0 - ring / 14.0)));
            }
            if line == Line::Bite {
                paint_metal(painter, at(x, FEET - 100.0), Align2::CENTER_CENTER, "!", 30.0 * scale.max(0.6), hex(0xffd23a), 1.0);
            }
        }
        self.paint_fisher_name(painter, at, scale, x, name, me);
    }

    /// A fisher's name, under them.
    fn paint_fisher_name(&self, painter: &egui::Painter, at: impl Fn(f32, f32) -> Pos2, scale: f32, x: f32, name: &str, me: bool) {
        let theme = &self.theme;
        let color = if me { theme.accent } else { Color32::WHITE };
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(12.0 * scale.max(0.75)), color);
        let tag = Rect::from_center_size(at(x, FEET + 22.0), galley.size() + Vec2::new(12.0, 4.0));
        painter.rect_filled(tag, 6.0, Color32::from_black_alpha(110));
        painter.galley(tag.center() - galley.size() / 2.0, galley, color);
    }

    /// The binder: every candidate's card (a shadow of those not caught yet), and floor's board of the
    /// most cards. A card clicked shows big.
    fn fishing_binder(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let wide = body.width() >= 980.0;
        let grid_w = if wide { body.width() - 320.0 } else { body.width() };
        let card = Vec2::new(176.0, 246.0);
        let gap = 16.0;
        let cols = (((grid_w + gap) / (card.x + gap)).floor() as usize).max(1);
        let rows = FISH.len().div_ceil(cols);
        let block_w = cols as f32 * (card.x + gap) - gap;
        let head = 34.0;
        let content_h = head + rows as f32 * (card.y + gap);
        let have = self.config.settings.fish_cards.len();
        let shiny: u32 = self.config.settings.fish_cards.values().map(|c| c.shiny).sum();

        let mut clicked = None;
        let grid_rect = Rect::from_min_size(body.min, Vec2::new(grid_w, body.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid_rect));
        egui::ScrollArea::vertical().id_salt("fishing-binder").auto_shrink([false; 2]).show(&mut child, |ui| {
            let (area, _) = ui.allocate_exact_size(Vec2::new(grid_w, content_h), Sense::hover());
            let painter = ui.painter().clone();
            let x0 = area.center().x - block_w / 2.0;
            let mut summary = t.fishing_collection.replace("{n}", &have.to_string()).replace("{max}", &FISH.len().to_string());
            if shiny > 0 {
                summary = format!("{summary}  ·  ✦ {shiny}");
            }
            painter.text(Pos2::new(x0, area.min.y + 10.0), Align2::LEFT_CENTER, summary, FontId::proportional(14.0), theme.text);
            for (k, f) in FISH.iter().enumerate() {
                let (row, col) = (k / cols, k % cols);
                let r = Rect::from_min_size(Pos2::new(x0 + col as f32 * (card.x + gap), area.min.y + head + row as f32 * (card.y + gap)), card);
                let entry = self.config.settings.fish_cards.get(f.id);
                let resp = ui.interact(r, ui.id().with(("fish-card", k)), Sense::click());
                let lift = if resp.hovered() { Vec2::new(0.0, -3.0) } else { Vec2::ZERO };
                paint_card(&painter, r.translate(lift), k, entry, entry.is_some_and(|c| c.shiny > 0), t, &theme, now);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    clicked = Some(k);
                }
            }
        });
        if let Some(k) = clicked {
            self.fishing.zoom = Some(k);
        }

        // floor's board, beside.
        if wide {
            let board = Rect::from_min_max(Pos2::new(body.max.x - 290.0, body.min.y + 4.0), body.max);
            let best = (have > 0).then(|| t.fishing_collection.replace("{n}", &have.to_string()).replace("{max}", &FISH.len().to_string()));
            let note = |e: &crate::floor::GameEntry| e.details.get("catches").and_then(serde_json::Value::as_u64).map(|n| t.fishing_board_note.replace("{n}", &n.to_string()));
            let entries = self.fishing.floor.board().map(<[_]>::to_vec);
            if self.game_board_noted(ui, board, &theme, entries.as_deref(), best, |s| format!("{s} pts"), &note) {
                self.ask_pseudo();
            }
        }

        // A card big, over the page; a click anywhere puts it back.
        if let Some(k) = self.fishing.zoom {
            let painter = ui.painter().clone();
            painter.rect_filled(body.expand(16.0), 0.0, Color32::from_black_alpha(150));
            let h = (body.height() - 20.0).min(560.0);
            let r = Rect::from_center_size(body.center(), Vec2::new(h * 0.714, h));
            let entry = self.config.settings.fish_cards.get(FISH[k].id);
            paint_card(&painter, r, k, entry, entry.is_some_and(|c| c.shiny > 0), t, &theme, now);
            let away = ui.interact(body.expand(16.0), ui.id().with("fish-zoom"), Sense::click());
            if away.clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                self.fishing.zoom = None;
            }
        }
    }
}

/// A rarity's mark: a circle, a diamond, a star, two stars, a burst.
fn paint_rarity(painter: &egui::Painter, c: Pos2, s: f32, r: Rarity) {
    let color = rarity_color(r);
    let star = |c: Pos2, s: f32| {
        let pts: Vec<Pos2> = (0..10).map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
            let rr = if k % 2 == 0 { s } else { s * 0.45 };
            c + Vec2::new(a.cos(), a.sin()) * rr
        }).collect();
        // Not convex: a fan from the middle.
        for k in 0..10 {
            painter.add(egui::Shape::convex_polygon(vec![c, pts[k], pts[(k + 1) % 10]], color, Stroke::NONE));
        }
    };
    match r {
        Rarity::Common => {
            painter.circle_filled(c, s * 0.6, color);
        }
        Rarity::Uncommon => {
            painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, -s * 0.8), c + Vec2::new(s * 0.7, 0.0), c + Vec2::new(0.0, s * 0.8), c + Vec2::new(-s * 0.7, 0.0)], color, Stroke::NONE));
        }
        Rarity::Rare => star(c, s),
        Rarity::VeryRare => {
            star(c - Vec2::new(s * 0.75, 0.0), s * 0.8);
            star(c + Vec2::new(s * 0.75, 0.0), s * 0.8);
        }
        Rarity::Legendary => {
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                painter.line_segment([c, c + Vec2::new(a.cos(), a.sin()) * s * 1.1], Stroke::new(s * 0.18, color));
            }
            star(c, s * 0.8);
        }
    }
}

/// A candidate's card, in `r` (about 5 by 7): its frame in its rarity's color, its name and
/// motivation, its picture in its water, its rarity and hours, its attack, its flavor text, its number
/// and what was caught of it. Not caught yet: a shadow and question marks. Shiny: a rainbow sheen.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_card(painter: &egui::Painter, r: Rect, fish: usize, entry: Option<&crate::config::FishCard>, shiny: bool, t: &Strings, theme: &Theme, now: f64) {
    let f = &FISH[fish];
    let k = r.width() / 176.0;
    let known = entry.is_some();
    let frame = if known { rarity_color(f.rarity) } else { theme.tab_hover };
    let text = card_text(t, fish);
    let paper = if theme.dark { hex(0x1e2230) } else { hex(0xf7f3e8) };
    let ink = if theme.dark { hex(0xe8e6df) } else { hex(0x2a2a30) };
    let muted = ink.gamma_multiply(0.65);
    painter.rect_filled(r, 10.0 * k, frame);
    let inner = r.shrink(6.0 * k);
    painter.rect_filled(inner, 6.0 * k, paper);
    let font = |s: f32| FontId::proportional(s * k);
    let pad = 8.0 * k;

    // The name, the motivation.
    let name = if known { text.name } else { t.fishing_unknown };
    // The motivation on the right; the name made smaller until it fits beside it.
    let mut room = inner.width() - 2.0 * pad;
    if known {
        let hp = format!("{} {}", t.fishing_motivation, 30 + f.difficulty as u32);
        let hp = painter.text(Pos2::new(inner.max.x - pad, inner.min.y + 13.0 * k), Align2::RIGHT_CENTER, hp, font(9.0), hex(0xd0453a));
        room -= hp.width() + 6.0 * k;
    }
    let mut size = 12.5;
    while size > 8.0 && painter.layout_no_wrap(name.to_owned(), font(size), ink).size().x > room {
        size -= 0.5;
    }
    painter.text(Pos2::new(inner.min.x + pad, inner.min.y + 13.0 * k), Align2::LEFT_CENTER, name, font(size), ink);

    // The picture, in the water of its hours.
    let pic = Rect::from_min_max(Pos2::new(inner.min.x + pad, inner.min.y + 26.0 * k), Pos2::new(inner.max.x - pad, inner.min.y + 112.0 * k));
    let (top, low) = match f.when {
        When::Morning => (hex(0xf6d2b4), hex(0x4f8fae)),
        When::Afternoon => (hex(0xc4e6f6), hex(0x3a7ea2)),
        When::Evening => (hex(0xe08a6a), hex(0x1f3550)),
        When::Day => (hex(0xd8ecf6), hex(0x4f8fae)),
        When::Any => (hex(0xb8d8e8), hex(0x2f6a8a)),
    };
    gradient(painter, pic, if known { top } else { hex(0x3a3d48) }, if known { low } else { hex(0x22242c) });
    painter.rect_stroke(pic, 0.0, Stroke::new(1.5 * k, frame.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    let length = pic.width() * (0.42 + 0.4 * ((f.max_cm as f32).ln() - 3.0) / 2.6).clamp(0.4, 0.8);
    let bob = ((now * 1.6 + fish as f64).sin() * 2.0) as f32 * k;
    paint_candidate(&painter.with_clip_rect(pic), pic.center() + Vec2::new(0.0, bob), length, fish, (!known).then_some(Color32::from_black_alpha(220)), now);

    // Its rarity and hours.
    let rarities: Vec<&str> = t.fishing_rarities.split('|').collect();
    let whens: Vec<&str> = t.fishing_when.split('|').collect();
    let y = pic.max.y + 11.0 * k;
    let line = format!("{} · {}", rarities.get(f.rarity.index()).copied().unwrap_or_default(), whens.get(f.when.index()).copied().unwrap_or_default());
    painter.text(Pos2::new(inner.min.x + pad, y), Align2::LEFT_CENTER, line, font(9.0), muted);
    paint_rarity(painter, Pos2::new(inner.max.x - pad - 7.0 * k, y), 5.5 * k, f.rarity);

    let mut y = y + 12.0 * k;
    if known {
        // The attack.
        painter.line_segment([Pos2::new(inner.min.x + pad, y), Pos2::new(inner.max.x - pad, y)], Stroke::new(0.6 * k, muted));
        y += 11.0 * k;
        painter.text(Pos2::new(inner.min.x + pad, y), Align2::LEFT_CENTER, text.attack, font(10.0), ink);
        painter.text(Pos2::new(inner.max.x - pad, y), Align2::RIGHT_CENTER, text.damage, font(12.0), ink);
        y += 10.0 * k;
        // The flavor text.
        let galley = painter.layout(text.flavor.to_owned(), FontId::proportional(8.6 * k), muted, inner.width() - 2.0 * pad);
        painter.galley(Pos2::new(inner.min.x + pad, y), galley, muted);
    } else {
        let hint = t.fishing_unknown_hint.replace("{when}", whens.get(f.when.index()).copied().unwrap_or_default());
        let galley = painter.layout(hint, FontId::proportional(9.5 * k), muted, inner.width() - 2.0 * pad);
        painter.galley(Pos2::new(inner.min.x + pad, y + 8.0 * k), galley, muted);
    }

    // At the bottom: its number, what was caught of it.
    let bottom = inner.max.y - 9.0 * k;
    painter.text(Pos2::new(inner.min.x + pad, bottom), Align2::LEFT_CENTER, format!("#{:03}/{:03}", fish + 1, FISH.len()), FontId::monospace(8.5 * k), muted);
    if let Some(c) = entry {
        let mut stats = format!("{}  ·  {}", t.fishing_count.replace("{n}", &c.count.to_string()), t.fishing_best.replace("{n}", &c.best_cm.to_string()));
        if c.shiny > 0 {
            stats = format!("✦{}  {stats}", c.shiny);
        }
        painter.text(Pos2::new(inner.max.x - pad, bottom), Align2::RIGHT_CENTER, stats, font(8.5), muted);
    }

    // Shiny: a rainbow sheen sweeping across.
    if shiny && known {
        let clip = painter.with_clip_rect(r);
        let sweep = ((now * 0.35) % 1.6) as f32 - 0.3;
        let colors = [hex(0xff6a6a), hex(0xffd36a), hex(0x8cff8a), hex(0x6ad8ff), hex(0xb38cff)];
        for (n, c) in colors.iter().enumerate() {
            let x = r.min.x + (sweep + n as f32 * 0.06) * r.width() * 1.4;
            let band = vec![Pos2::new(x, r.min.y), Pos2::new(x + 14.0 * k, r.min.y), Pos2::new(x + 14.0 * k - r.height() * 0.5, r.max.y), Pos2::new(x - r.height() * 0.5, r.max.y)];
            clip.add(egui::Shape::convex_polygon(band, c.gamma_multiply(0.22), Stroke::NONE));
        }
        painter.rect_stroke(r, 10.0 * k, Stroke::new(1.5 * k, Color32::WHITE.gamma_multiply(0.6 + 0.4 * ((now * 3.0).sin() as f32 * 0.5 + 0.5))), egui::StrokeKind::Inside);
    }
}

/// What a candidate looks like: a fish of some kind, dressed for its job.
#[derive(Clone, Copy, PartialEq)]
enum Wear {
    Badge,
    Cap,
    Glasses,
    Tie,
    Clipboard,
    Beret,
    Headset,
    PostIt,
    BowTie,
    Beard,
    TopHat,
    Wool,
    /// Hair in a bun.
    Bun,
    Pearls,
    Mug,
    Phone,
}

#[derive(Clone, Copy, PartialEq)]
enum Pattern {
    Plain,
    Bars,
    Spots,
    Patches,
    Plates,
}

struct Look {
    body: u32,
    belly: u32,
    fins: u32,
    /// Height over length.
    aspect: f32,
    pattern: Pattern,
    whiskers: bool,
    wear: &'static [Wear],
}

const LOOKS: [Look; 36] = [
    Look { body: 0xb8c4cc, belly: 0xeef2f4, fins: 0xd8504a, aspect: 0.42, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Badge] },
    Look { body: 0xd2dce6, belly: 0xf4f8fa, fins: 0xa8b8c8, aspect: 0.32, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Cap] },
    Look { body: 0xa6b84c, belly: 0xe8e2b0, fins: 0xe88a3a, aspect: 0.42, pattern: Pattern::Bars, whiskers: false, wear: &[Wear::Glasses, Wear::Headset] },
    Look { body: 0xdcaa48, belly: 0xf6e2a8, fins: 0xd8504a, aspect: 0.4, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Tie] },
    Look { body: 0x6f7d3a, belly: 0xc8c890, fins: 0x56602a, aspect: 0.4, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Glasses] },
    Look { body: 0xb08a4f, belly: 0xe2cfa0, fins: 0x8a6a3a, aspect: 0.52, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Clipboard] },
    Look { body: 0x8a6a3a, belly: 0xd8c08a, fins: 0x6a4a2a, aspect: 0.4, pattern: Pattern::Plain, whiskers: true, wear: &[Wear::Glasses, Wear::Beard] },
    Look { body: 0xa78a5a, belly: 0xefe2c4, fins: 0x8a6a3a, aspect: 0.34, pattern: Pattern::Spots, whiskers: false, wear: &[Wear::Beret] },
    Look { body: 0x5e8a46, belly: 0xdfe8b8, fins: 0x4a6a36, aspect: 0.4, pattern: Pattern::Bars, whiskers: false, wear: &[Wear::Headset] },
    Look { body: 0xf6f2ea, belly: 0xffffff, fins: 0xf0a060, aspect: 0.4, pattern: Pattern::Patches, whiskers: false, wear: &[Wear::PostIt] },
    Look { body: 0x7d8a7a, belly: 0xdcdfd4, fins: 0x5a6658, aspect: 0.3, pattern: Pattern::Bars, whiskers: false, wear: &[Wear::BowTie, Wear::Glasses] },
    Look { body: 0x4c5232, belly: 0xa8a878, fins: 0x3a4026, aspect: 0.14, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Cap] },
    Look { body: 0x4f6b3a, belly: 0xd8e0b0, fins: 0x6a7a3a, aspect: 0.24, pattern: Pattern::Spots, whiskers: false, wear: &[Wear::Tie] },
    Look { body: 0x7a7f86, belly: 0xd0d4d8, fins: 0x5a5f66, aspect: 0.22, pattern: Pattern::Plates, whiskers: true, wear: &[Wear::Glasses, Wear::Beard] },
    Look { body: 0x3d3a36, belly: 0x8a8478, fins: 0x2a2826, aspect: 0.3, pattern: Pattern::Plain, whiskers: true, wear: &[Wear::TopHat] },
    Look { body: 0xd88a9a, belly: 0xf6dde2, fins: 0xb05a6a, aspect: 0.4, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Bun, Wear::Pearls] },
    Look { body: 0xc0a0d8, belly: 0xf0e6f8, fins: 0x8a6aa8, aspect: 0.4, pattern: Pattern::Spots, whiskers: false, wear: &[Wear::Bun, Wear::Glasses] },
    Look { body: 0x9ad0e8, belly: 0xeef8fc, fins: 0x5a9ab8, aspect: 0.3, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Cap, Wear::Phone] },
    Look { body: 0xc8b070, belly: 0xf2e8c8, fins: 0x9a8040, aspect: 0.42, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Glasses, Wear::Clipboard] },
    Look { body: 0xf08ab8, belly: 0xfde0ee, fins: 0xc05088, aspect: 0.38, pattern: Pattern::Patches, whiskers: false, wear: &[Wear::Phone] },
    Look { body: 0x8a8a7a, belly: 0xd8d8cc, fins: 0x6a6a5a, aspect: 0.36, pattern: Pattern::Plates, whiskers: false, wear: &[Wear::Beard, Wear::Mug] },
    Look { body: 0x4a6aa8, belly: 0xc8d6ee, fins: 0x2a4a88, aspect: 0.44, pattern: Pattern::Bars, whiskers: false, wear: &[Wear::Cap] },
    Look { body: 0xe08a3a, belly: 0xf8dcc0, fins: 0xb05a20, aspect: 0.42, pattern: Pattern::Spots, whiskers: false, wear: &[Wear::Headset, Wear::Pearls] },
    Look { body: 0x6ac0a0, belly: 0xd8f2e8, fins: 0x3a8a70, aspect: 0.38, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::PostIt, Wear::Glasses] },
    Look { body: 0xa0b8e0, belly: 0xe6eef8, fins: 0x6a88c0, aspect: 0.36, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Glasses, Wear::Clipboard] },
    Look { body: 0x3ad0c0, belly: 0xd0f6f2, fins: 0x1a9a8a, aspect: 0.3, pattern: Pattern::Bars, whiskers: false, wear: &[Wear::Cap, Wear::Phone] },
    Look { body: 0xd04a3a, belly: 0xf6d0c8, fins: 0x9a2a20, aspect: 0.44, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Beard, Wear::Badge] },
    Look { body: 0x8a5ab0, belly: 0xe6d6f2, fins: 0x5a3a80, aspect: 0.4, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Bun, Wear::Clipboard] },
    Look { body: 0x5a9ae0, belly: 0xd6e8fa, fins: 0x2a6ab0, aspect: 0.36, pattern: Pattern::Spots, whiskers: false, wear: &[Wear::Glasses, Wear::Mug] },
    Look { body: 0x5ab070, belly: 0xd6f0dc, fins: 0x2a8040, aspect: 0.34, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Headset, Wear::Mug] },
    Look { body: 0x3a4a6a, belly: 0xb8c4d8, fins: 0x2a3450, aspect: 0.3, pattern: Pattern::Bars, whiskers: false, wear: &[Wear::Tie, Wear::Glasses] },
    Look { body: 0x2a3a2a, belly: 0xa8b8a8, fins: 0x1a2a1a, aspect: 0.32, pattern: Pattern::Plates, whiskers: false, wear: &[Wear::Glasses, Wear::Badge] },
    Look { body: 0x6a6ab0, belly: 0xd6d6f2, fins: 0x4a4a90, aspect: 0.26, pattern: Pattern::Patches, whiskers: false, wear: &[Wear::Beard, Wear::Glasses] },
    Look { body: 0xc87a9a, belly: 0xf2dce6, fins: 0x9a4a6a, aspect: 0.44, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Bun, Wear::Glasses, Wear::Pearls] },
    Look { body: 0xe8c070, belly: 0xfaeccc, fins: 0xc09040, aspect: 0.28, pattern: Pattern::Spots, whiskers: false, wear: &[Wear::Beard, Wear::Phone] },
    Look { body: 0xf2c94c, belly: 0xfff0b0, fins: 0xe8a020, aspect: 0.42, pattern: Pattern::Plain, whiskers: false, wear: &[Wear::Wool] },
];

/// A candidate, facing right, its middle at `c`, `len` long. `shadow`: all in that color (not caught
/// yet).
pub(super) fn paint_candidate(painter: &egui::Painter, c: Pos2, len: f32, fish: usize, shadow: Option<Color32>, now: f64) {
    let look = &LOOKS[fish.min(LOOKS.len() - 1)];
    let col = |v: u32| shadow.unwrap_or(hex(v));
    let (body, belly, fins) = (col(look.body), col(look.belly), col(look.fins));
    let half = len / 2.0;
    let h = len * look.aspect / 2.0;
    let at = |x: f32, y: f32| c + Vec2::new(x * half, y * h);
    let wag = ((now * 6.0 + fish as f64).sin() * 0.12) as f32;
    let u = len / 60.0;

    // The tail, the fins.
    painter.add(egui::Shape::convex_polygon(vec![at(-0.82, 0.0), at(-1.15, -0.9 + wag), at(-1.05, 0.0), at(-1.15, 0.9 + wag)], fins, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![at(-0.3, -0.8), at(0.0, -1.35), at(0.3, -0.85)], fins, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![at(-0.2, 0.8), at(0.0, 1.2), at(0.2, 0.85)], fins, Stroke::NONE));
    // The body: an egg, pointed at the tail.
    let pts: Vec<Pos2> = (0..32).map(|k| {
        let a = k as f32 / 32.0 * std::f32::consts::TAU;
        let x = a.cos();
        let squeeze = if x < 0.0 { 1.0 - 0.35 * x * x } else { 1.0 };
        at(x * 0.95, a.sin() * squeeze)
    }).collect();
    painter.add(egui::Shape::convex_polygon(pts, body, Stroke::NONE));
    if shadow.is_none() {
        // The belly, lighter.
        let low: Vec<Pos2> = (0..=16).map(|k| {
            let a = k as f32 / 16.0 * std::f32::consts::PI;
            at(a.cos() * 0.8, 0.25 + a.sin() * 0.62)
        }).collect();
        painter.add(egui::Shape::convex_polygon(low, belly, Stroke::NONE));
        match look.pattern {
            Pattern::Bars => {
                for k in 0..5 {
                    let x = -0.55 + k as f32 * 0.25;
                    painter.line_segment([at(x, -0.85), at(x - 0.04, 0.25)], Stroke::new(2.6 * u, hex(look.body).gamma_multiply(0.55)));
                }
            }
            Pattern::Spots => {
                for (k, (x, y)) in [(-0.5, -0.3), (-0.2, -0.5), (0.1, -0.2), (-0.35, 0.05), (0.3, -0.5), (0.0, 0.1), (-0.65, -0.05)].into_iter().enumerate() {
                    let color = if k % 3 == 0 { hex(0xd8504a) } else { hex(0x2a2a20) };
                    painter.circle_filled(at(x, y), 1.4 * u, color);
                }
            }
            Pattern::Patches => {
                for (x, y, r, v) in [(-0.35, -0.3, 0.32, 0xf07a2a), (0.25, -0.45, 0.22, 0xe0453a), (-0.05, 0.2, 0.2, 0x2a2a30)] {
                    painter.circle_filled(at(x, y), r * h * 1.4, hex(v));
                }
            }
            Pattern::Plates => {
                for k in 0..6 {
                    let x = -0.7 + k as f32 * 0.25;
                    painter.add(egui::Shape::convex_polygon(vec![at(x - 0.06, -0.55), at(x, -0.85), at(x + 0.06, -0.55)], hex(0xd0d4d8), Stroke::NONE));
                }
            }
            Pattern::Plain => {}
        }
        // The gill, the eye, the mouth.
        painter.add(egui::Shape::line(vec![at(0.42, -0.55), at(0.36, 0.0), at(0.42, 0.5)], Stroke::new(1.0 * u, hex(look.body).gamma_multiply(0.6))));
    }
    let eye = at(0.62, -0.25);
    if shadow.is_none() {
        painter.circle_filled(eye, 2.6 * u + 1.0, Color32::WHITE);
        painter.circle_filled(eye + Vec2::new(0.6 * u, 0.0), 1.4 * u + 0.6, hex(0x1a1a20));
        painter.line_segment([at(0.86, 0.22), at(0.95, 0.15)], Stroke::new(1.0 * u, hex(0x3a2a2a)));
    }
    if look.whiskers {
        for (dy, end) in [(0.2, 0.9), (0.3, 1.3)] {
            painter.add(egui::Shape::line(vec![at(0.9, dy), at(1.1, dy + 0.3), at(1.0, end)], Stroke::new(1.0 * u, if shadow.is_some() { body } else { hex(0x2a2826) })));
        }
    }
    // What it wears.
    let ink = |v: u32| shadow.unwrap_or(hex(v));
    for wear in look.wear {
        match wear {
            Wear::Bun => {
                let top = at(0.35, -1.0);
                painter.circle_filled(top + Vec2::new(0.0, -4.0) * u, 5.0 * u, ink(0x5a3a22));
                painter.line_segment([top + Vec2::new(-4.0, -1.0) * u, top + Vec2::new(4.0, -1.0) * u], Stroke::new(1.6 * u, ink(0xe0453a)));
            }
            Wear::Pearls => {
                for k in 0..7 {
                    let a = k as f32 / 6.0;
                    painter.circle_filled(at(0.12 + a * 0.5, 0.42 + (a * std::f32::consts::PI).sin() * 0.3), 1.5 * u, ink(0xfaf6ee));
                }
            }
            Wear::Mug => {
                let m = Rect::from_center_size(at(1.05, 0.55), Vec2::new(7.0, 8.0) * u);
                painter.circle_stroke(Pos2::new(m.max.x, m.center().y), 2.4 * u, Stroke::new(1.2 * u, ink(0xf2f2f2)));
                painter.rect_filled(m, 1.2 * u, ink(0xf2f2f2));
                painter.rect_filled(Rect::from_min_size(m.min + Vec2::new(0.0, 3.0 * u), Vec2::new(m.width(), 2.0 * u)), 0.0, ink(0xd0453a));
                if shadow.is_none() {
                    let steam = ((now * 3.0).sin() * 1.2) as f32;
                    painter.add(egui::Shape::line(vec![m.center_top() + Vec2::new(0.0, -1.0) * u, m.center_top() + Vec2::new(1.5 + steam, -4.0) * u, m.center_top() + Vec2::new(-0.5 - steam, -7.0) * u], Stroke::new(0.8 * u, Color32::WHITE.gamma_multiply(0.6))));
                }
            }
            Wear::Phone => {
                let ph = Rect::from_center_size(at(0.95, 0.35), Vec2::new(5.5, 9.5) * u);
                painter.rect_filled(ph, 1.4 * u, ink(0x1a1a22));
                painter.rect_filled(ph.shrink(1.0 * u), 0.8 * u, ink(0x6ac0f0));
            }
            Wear::Badge => {
                painter.add(egui::Shape::line(vec![at(0.45, -0.9), at(0.3, 0.2), at(0.55, -0.9)], Stroke::new(1.0 * u, ink(0x3a6ae0))));
                let card = Rect::from_center_size(at(0.38, 0.4), Vec2::new(7.0, 9.0) * u);
                painter.rect_filled(card, 1.0 * u, ink(0xffffff));
                painter.rect_filled(Rect::from_min_size(card.min, Vec2::new(card.width(), 2.5 * u)), 1.0 * u, ink(0x3a6ae0));
            }
            Wear::Cap => {
                let top = at(0.5, -0.95);
                painter.add(egui::Shape::convex_polygon((0..=10).map(|k| top + Vec2::angled(std::f32::consts::PI + k as f32 / 10.0 * std::f32::consts::PI) * 7.0 * u).collect(), ink(0xe0453a), Stroke::NONE));
                painter.add(egui::Shape::convex_polygon(vec![top + Vec2::new(4.0, 0.0) * u, top + Vec2::new(14.0, 1.5) * u, top + Vec2::new(4.0, 2.5) * u], ink(0xb03028), Stroke::NONE));
            }
            Wear::Glasses => {
                let stroke = Stroke::new(1.2 * u, ink(0x1a1a20));
                painter.circle_stroke(eye, 4.2 * u, stroke);
                painter.line_segment([eye + Vec2::new(4.2 * u, 0.0), eye + Vec2::new(8.0 * u, 1.0 * u)], stroke);
                painter.line_segment([eye - Vec2::new(4.2 * u, 0.0), eye - Vec2::new(9.0 * u, 1.5 * u)], stroke);
            }
            Wear::Tie => {
                let knot = at(0.35, 0.55);
                painter.add(egui::Shape::convex_polygon(vec![knot + Vec2::new(-2.0, -2.0) * u, knot + Vec2::new(2.0, -2.0) * u, knot + Vec2::new(1.5, 2.0) * u, knot + Vec2::new(-1.5, 2.0) * u], ink(0xc0283a), Stroke::NONE));
                painter.add(egui::Shape::convex_polygon(vec![knot + Vec2::new(-1.5, 2.0) * u, knot + Vec2::new(1.5, 2.0) * u, knot + Vec2::new(3.0, 12.0) * u, knot + Vec2::new(0.0, 15.0) * u, knot + Vec2::new(-3.0, 12.0) * u], ink(0xe0384a), Stroke::NONE));
            }
            Wear::Clipboard => {
                let b = Rect::from_center_size(at(0.05, 0.75), Vec2::new(12.0, 15.0) * u);
                painter.rect_filled(b, 1.0 * u, ink(0x9a6a3a));
                painter.rect_filled(b.shrink(1.6 * u), 0.0, ink(0xffffff));
                for k in 0..3 {
                    let y = b.min.y + (5.0 + k as f32 * 3.0) * u;
                    painter.line_segment([Pos2::new(b.min.x + 3.0 * u, y), Pos2::new(b.max.x - 3.0 * u, y)], Stroke::new(0.8 * u, ink(0x5a5a68)));
                }
            }
            Wear::Beret => {
                let top = at(0.45, -0.95);
                painter.add(egui::Shape::convex_polygon((0..16).map(|k| top + Vec2::new((k as f32 / 16.0 * std::f32::consts::TAU).cos() * 9.0, (k as f32 / 16.0 * std::f32::consts::TAU).sin() * 3.5) * u).collect(), ink(0x2a2a40), Stroke::NONE));
                painter.circle_filled(top + Vec2::new(0.0, -3.5) * u, 1.4 * u, ink(0x2a2a40));
            }
            Wear::Headset => {
                let around = (0..=12).map(|k| at(0.55, -0.1) + Vec2::angled(std::f32::consts::PI * (1.0 + k as f32 / 12.0)) * (h + 2.0 * u)).collect::<Vec<_>>();
                painter.add(egui::Shape::line(around, Stroke::new(1.6 * u, ink(0x2a2a30))));
                painter.circle_filled(at(0.55, -0.1) + Vec2::new(h + 2.0 * u, 0.0), 2.6 * u, ink(0x2a2a30));
                painter.line_segment([at(0.55, -0.1) + Vec2::new(h + 2.0 * u, 0.0), at(1.0, 0.4)], Stroke::new(1.0 * u, ink(0x2a2a30)));
                painter.circle_filled(at(1.0, 0.4), 1.6 * u, ink(0x2a2a30));
            }
            Wear::PostIt => {
                for (x, y, v, tilt) in [(-0.3, -0.2, 0xffe45a, 0.2f32), (0.05, 0.1, 0xff8ac0, -0.15), (-0.55, 0.2, 0x8ae0ff, 0.1)] {
                    let p = at(x, y);
                    let s = 5.0 * u;
                    let rot = |dx: f32, dy: f32| p + Vec2::new(dx * tilt.cos() - dy * tilt.sin(), dx * tilt.sin() + dy * tilt.cos()) * s;
                    painter.add(egui::Shape::convex_polygon(vec![rot(-1.0, -1.0), rot(1.0, -1.0), rot(1.0, 1.0), rot(-1.0, 1.0)], ink(v), Stroke::NONE));
                }
            }
            Wear::BowTie => {
                let k = at(0.4, 0.6);
                painter.add(egui::Shape::convex_polygon(vec![k, k + Vec2::new(-6.0, -3.5) * u, k + Vec2::new(-6.0, 3.5) * u], ink(0x2a2a40), Stroke::NONE));
                painter.add(egui::Shape::convex_polygon(vec![k, k + Vec2::new(6.0, -3.5) * u, k + Vec2::new(6.0, 3.5) * u], ink(0x2a2a40), Stroke::NONE));
                painter.circle_filled(k, 1.6 * u, ink(0x4a4a60));
            }
            Wear::Beard => {
                for k in 0..5 {
                    let x = 0.62 + k as f32 * 0.07;
                    painter.add(egui::Shape::convex_polygon(vec![at(x - 0.05, 0.35), at(x + 0.05, 0.35), at(x - 0.02, 0.35) + Vec2::new(0.0, 9.0 * u)], ink(0xe8e8e8), Stroke::NONE));
                }
            }
            Wear::TopHat => {
                let base = at(0.45, -0.92);
                painter.rect_filled(Rect::from_center_size(base, Vec2::new(18.0, 2.5) * u), 1.0 * u, ink(0x111114));
                let crown = Rect::from_min_max(base + Vec2::new(-6.0, -15.0) * u, base + Vec2::new(6.0, 0.0) * u);
                painter.rect_filled(crown, 1.0 * u, ink(0x111114));
                painter.rect_filled(Rect::from_min_max(Pos2::new(crown.min.x, crown.max.y - 4.0 * u), Pos2::new(crown.max.x, crown.max.y - 2.0 * u)), 0.0, ink(0xc0283a));
            }
            Wear::Wool => {
                // A sheep's fleece on its back, and its five legs.
                for k in 0..6 {
                    painter.circle_filled(at(-0.55 + k as f32 * 0.22, -0.85), 5.0 * u, ink(0xfafaf2));
                }
                for k in 0..5 {
                    let x = -0.5 + k as f32 * 0.25;
                    let step = ((now * 8.0 + k as f64).sin() * 2.0) as f32 * u;
                    painter.line_segment([at(x, 0.8), at(x, 0.8) + Vec2::new(step, 9.0 * u)], Stroke::new(2.2 * u, ink(0x3a3030)));
                }
            }
        }
    }
}

/// The game's icon: a float on its ripples, a hook's line over it.
pub(super) fn paint_fishing_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    painter.line_segment([at(-9.0, -10.0), at(0.0, -3.0)], Stroke::new(0.9 * s, theme.text_muted));
    for (r, a) in [(9.0, 0.35), (6.0, 0.6)] {
        painter.add(egui::Shape::line((0..=12).map(|k| at(0.0, 4.0) + Vec2::new((k as f32 / 12.0 * std::f32::consts::PI).cos() * r, (k as f32 / 12.0 * std::f32::consts::PI).sin() * r * 0.35)).collect(), Stroke::new(1.0 * s, theme.text_muted.gamma_multiply(a))));
    }
    painter.circle_filled(at(0.0, 1.0), 4.0 * s, theme.accent);
    painter.circle_filled(at(0.0, -2.5), 2.3 * s, theme.text);
}
