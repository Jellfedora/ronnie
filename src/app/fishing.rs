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
use crate::fishing::{Reel, Rarity, When, FISH};
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

enum Phase {
    Idle,
    /// The cast charging: its power goes up and down while held.
    Charging { power: f32, rising: bool },
    Waiting { cast: f32, bite_in: f32 },
    Bite { cast: f32, left: f32 },
    Reeling { cast: f32, reel: Reel, size: u32, shiny: bool },
    Caught { fish: usize, size: u32, shiny: bool, new: bool, record: bool, at: f64 },
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
    acc: f32,
    rng: u64,
    pub(super) pond: crate::pond::Pond,
    pub(super) floor: crate::floor::GameFloor,
}

impl Default for Fishing {
    fn default() -> Self {
        Self { phase: Phase::Idle, cards: false, zoom: None, message: None, held: false, acc: 0.0, rng: 0, pond: Default::default(), floor: crate::floor::GameFloor::new("peche", false) }
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
        let mut caught = None;
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
                    let cast = power.max(0.05);
                    let wait = 2.5 + (super::game::random(&mut self.fishing.rng) % 7000) as f32 / 1000.0;
                    self.fishing.phase = Phase::Waiting { cast, bite_in: wait };
                }
            }
            Phase::Waiting { cast, bite_in } => {
                *bite_in -= dt;
                if pressed {
                    // Reeled in empty.
                    self.fishing.phase = Phase::Idle;
                } else if *bite_in <= 0.0 {
                    self.fishing.phase = Phase::Bite { cast: *cast, left: BITE_TIME };
                }
            }
            Phase::Bite { cast, left } => {
                *left -= dt;
                if pressed {
                    let cast = *cast;
                    let (fish, size) = crate::fishing::pick(&mut self.fishing.rng, hour(), cast);
                    let seed = super::game::random(&mut self.fishing.rng);
                    let shiny = super::game::random(&mut self.fishing.rng).is_multiple_of(SHINY_ONE_IN);
                    self.fishing.phase = Phase::Reeling { cast, reel: Reel::new(fish, seed), size, shiny };
                    self.fishing.acc = 0.0;
                } else if *left <= 0.0 {
                    self.fishing.phase = Phase::Idle;
                    self.fishing.message = Some((t.fishing_late.to_owned(), now + 3.0));
                }
            }
            Phase::Reeling { reel, size, shiny, .. } => {
                self.fishing.acc += dt;
                while self.fishing.acc >= STEP {
                    self.fishing.acc -= STEP;
                    match reel.step(STEP, held) {
                        Some(true) => {
                            caught = Some((reel.fish, *size, *shiny));
                            break;
                        }
                        Some(false) => {
                            self.fishing.phase = Phase::Idle;
                            self.fishing.message = Some((t.fishing_lost.to_owned(), now + 3.0));
                            break;
                        }
                        None => {}
                    }
                }
            }
            Phase::Caught { at, .. } => {
                // Stays until put away (a press, once the hand let go of the reel): time to read it.
                if pressed && now - *at > 1.2 {
                    self.fishing.phase = Phase::Idle;
                }
            }
        }
        if let Some((fish, size, shiny)) = caught {
            self.fishing_hired(fish, size, shiny, now);
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
            Phase::Waiting { .. } => t.fishing_wait.to_owned(),
            Phase::Bite { .. } => t.fishing_bite.to_owned(),
            Phase::Reeling { .. } => t.fishing_help.to_owned(),
            Phase::Caught { .. } => t.fishing_put_away.to_owned(),
        };
        let color = if matches!(self.fishing.phase, Phase::Bite { .. }) { theme.accent } else { theme.text_muted };
        ui.painter().text(Pos2::new(field.center().x, field.max.y + 18.0), Align2::CENTER_CENTER, hint, FontId::proportional(13.0), color);
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
        self.fishing.phase = Phase::Caught { fish, size, shiny, new, record, at: now };
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
            self.paint_fisher(&painter, at, scale, f.spot, &f.name, shirt, false, f.state, cast, bend, now);
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
        let (line, cast) = match &self.fishing.phase {
            Phase::Charging { .. } => (Line::Idle, 0.0),
            _ => self.fishing.line(),
        };
        self.paint_fisher(&painter, at, scale, me, name, theme.accent, true, line, cast, matches!(self.fishing.phase, Phase::Reeling { .. }), now);

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
            let track = r(x - 22.0, 70.0, x + 22.0, 430.0);
            let panel = track.expand(10.0 * scale);
            painter.rect(Rect::from_min_max(panel.min, Pos2::new(panel.max.x + 26.0 * scale, panel.max.y)), 10.0, theme.chrome_bg.gamma_multiply(0.95), Stroke::new(1.5, hex(0x7a6a48)), egui::StrokeKind::Inside);
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
        if let Phase::Caught { fish, size, shiny, new, record, at: when } = &self.fishing.phase {
            let k = ((now - when) / 0.35).min(1.0) as f32;
            painter.rect_filled(field, 0.0, Color32::from_black_alpha((110.0 * k) as u8));
            let h = (H - 90.0) * scale;
            let card = Rect::from_center_size(field.center() + Vec2::new(0.0, 14.0 * scale + (1.0 - k) * 40.0), Vec2::new(h * 0.714, h));
            let entry = self.config.settings.fish_cards.get(FISH[*fish].id);
            paint_card(&painter, card, *fish, entry, *shiny, t, theme, now);
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

    /// A fisher on the bank at `spot`: a figure with a cap in `shirt`, its rod, its line and float
    /// where `line` is; their name under them.
    #[allow(clippy::too_many_arguments)]
    fn paint_fisher(&self, painter: &egui::Painter, at: impl Fn(f32, f32) -> Pos2, scale: f32, spot: usize, name: &str, shirt: Color32, me: bool, line: Line, cast: f32, bend: bool, now: f64) {
        let theme = &self.theme;
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
        let tip = if !in_water {
            at(x + 30.0, FEET - 120.0)
        } else if bend {
            at(x + 44.0, FEET - 86.0 + (tm * 18.0).sin() * 3.0)
        } else {
            at(x + 46.0, FEET - 110.0)
        };
        let mid = if bend { at(x + 34.0, FEET - 92.0) } else { Pos2::new((hand.x + tip.x) / 2.0, (hand.y + tip.y) / 2.0) };
        painter.add(egui::Shape::line(vec![hand, mid, tip], stroke(2.0, hex(0x6a4a2a))));
        if in_water {
            let mut float = float_at(spot, cast);
            let mut dip = 0.0;
            match line {
                Line::Wait => float.y += (tm * 2.2 + spot as f32).sin() * 1.5,
                Line::Bite => dip = 5.0 + (tm * 22.0).sin() * 3.0,
                Line::Reel => float.y += (tm * 25.0).sin() * 3.0,
                Line::Idle => {}
            }
            let fp = at(float.x, float.y + dip);
            // A slack line waiting, taut in a fight.
            let sag = if bend { 0.0 } else { 16.0 };
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
        // The name, under them.
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
    painter.text(Pos2::new(inner.min.x + pad, inner.min.y + 13.0 * k), Align2::LEFT_CENTER, name, font(12.5), ink);
    if known {
        let hp = format!("{} {}", t.fishing_motivation, 30 + f.difficulty as u32);
        painter.text(Pos2::new(inner.max.x - pad, inner.min.y + 13.0 * k), Align2::RIGHT_CENTER, hp, font(9.0), hex(0xd0453a));
    }

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

const LOOKS: [Look; 16] = [
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
