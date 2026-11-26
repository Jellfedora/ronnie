//! Open Space, the army and the raids: the troops (recruited in the recruiting rooms, housed in the
//! break rooms, upgraded in the meeting room), and a raid on an open space: the troops dropped on
//! its edges go by themselves for their favourite target, around the firewalls or through them; the
//! defenses shoot, the traps go off. Stars at 50 % destroyed, at the boss's office down, at 100 %.
//!
//! The raid runs in fixed steps (`STEP`), the same each time for the same drops: what's shown is drawn
//! from it (`app::openspace_raid`). The campaign: 50 rival companies, their open spaces made from
//! their number, harder and harder.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use serde::{Deserialize, Serialize};

use crate::openspace_cards::{Action, Rarity};
use crate::openspace::{self as os, Building, Group, Kind, OpenSpace, Res, Tx, Why, GRID, HQ_MAX};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Troop {
    Intern,
    Dev,
    Tester,
    Devops,
    Sales,
    Data,
    Designer,
    Drone,
    Consultant,
    /// The hero: the lead dev (one, from the lead dev's office; not recruited, see `HERO_MAX`).
    Lead,
}

pub const TROOPS: [Troop; 9] = [Troop::Intern, Troop::Dev, Troop::Tester, Troop::Devops, Troop::Sales, Troop::Data, Troop::Designer, Troop::Drone, Troop::Consultant];

/// What a troop goes for first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefers {
    Any,
    Defense,
    Resource,
    Wall,
}

pub struct Stats {
    pub housing: u32,
    pub hp: f32,
    /// Damage per second (healing, for the designer).
    pub dps: f32,
    /// Its reach, in tiles.
    pub range: f32,
    /// Tiles per second.
    pub speed: f32,
    pub prefers: Prefers,
    /// Its hits hurt around, this far.
    pub splash: f32,
    pub air: bool,
    pub heals: bool,
    /// Its training (seconds).
    pub train: u32,
    /// Its training's cost at level 1 (coffee).
    pub cost: u64,
    /// The recruiting room's level that unlocks it.
    pub unlock: u8,
    pub max_level: u8,
}

/// The raid's length (seconds) and its step.
pub const TIME_LIMIT: f32 = 180.0;
pub const STEP: f32 = 0.05;
/// The campaign's levels.
pub const LEVELS: u32 = 50;

/// A break room's room for the army, by level.
pub fn lounge_room(level: u8) -> u32 {
    [20, 30, 35, 40, 45, 50, 55, 60, 65][(level.clamp(1, 9) - 1) as usize]
}

fn scale(level: u8) -> f32 {
    1.0 + 0.2 * (level.max(1) - 1) as f32
}

impl Troop {
    pub fn name(self) -> Tx {
        match self {
            Troop::Intern => ("Stagiaire", "Intern"),
            Troop::Dev => ("Dev", "Dev"),
            Troop::Tester => ("Testeur", "Tester"),
            Troop::Devops => ("DevOps", "DevOps"),
            Troop::Sales => ("Commercial", "Salesperson"),
            Troop::Data => ("Data scientist", "Data scientist"),
            Troop::Designer => ("Designer", "Designer"),
            Troop::Drone => ("Drone livreur", "Delivery drone"),
            Troop::Consultant => ("Consultant", "Consultant"),
            Troop::Lead => ("Lead Dev", "Lead dev"),
        }
    }

    pub fn about(self) -> Tx {
        match self {
            Troop::Intern => ("Pas cher, rapide à former, en masse. Fragile, frappe de près, attaque ce qui est le plus proche.", "Cheap, quick to train, in numbers. Fragile, hits up close, attacks whatever is nearest."),
            Troop::Dev => ("Tire de loin avec son laptop. Fragile : à garder derrière les autres.", "Shoots from afar with their laptop. Fragile: keep them behind the others."),
            Troop::Tester => ("Fonce sur les pare-feux et les casse (×40 de dégâts sur les murs). Ne sert à rien d'autre.", "Rushes the firewalls and breaks them (×40 damage on walls). Good for nothing else."),
            Troop::Devops => ("Vise d'abord les ressources (machines, serveurs, frigos, coffres), qu'il casse deux fois plus vite.", "Goes for resources first (machines, servers, fridges, safes), which they break twice as fast."),
            Troop::Sales => ("Très costaud, vise les défenses en premier. Encaisse pendant que les autres frappent.", "Very tough, goes for defenses first. Soaks up hits while the others strike."),
            Troop::Data => ("Lance des requêtes qui touchent toute une zone. Fragile mais dévastateur.", "Fires queries that hit a whole area. Fragile but devastating."),
            Troop::Designer => ("Vole au-dessus du champ de bataille et soigne les troupes autour. N'attaque pas.", "Flies over the battlefield and heals the troops around. Doesn't attack."),
            Troop::Drone => ("Vole par-dessus les murs vers les défenses et largue des colis explosifs.", "Flies over the walls to the defenses and drops exploding parcels."),
            Troop::Consultant => ("Hors de prix, très lent, mais il démolit tout sur son passage.", "Overpriced, very slow, but wrecks everything in their way."),
            Troop::Lead => ("Ton héros : costaud, frappe fort et ne prend pas de place. Une fois posé, « Mode crunch » le soigne et fait foncer les troupes autour. S'il tombe, il se repose avant le prochain raid.", "Your hero: tough, hits hard and takes no room. Once dropped, « Crunch mode » heals them and makes the troops around rush. If they fall, they rest before the next raid."),
        }
    }

    pub fn stats(self) -> Stats {
        let s = |housing, hp, dps, range, speed, prefers, splash, air, heals, train, cost, unlock, max_level| Stats { housing, hp, dps, range, speed, prefers, splash, air, heals, train, cost, unlock, max_level };
        match self {
            Troop::Intern => s(1, 45.0, 9.0, 0.6, 2.0, Prefers::Any, 0.0, false, false, 20, 25, 1, 8),
            Troop::Dev => s(1, 30.0, 8.0, 3.5, 2.0, Prefers::Any, 0.0, false, false, 25, 50, 2, 8),
            Troop::Tester => s(2, 25.0, 12.0, 0.6, 2.4, Prefers::Wall, 0.0, false, false, 60, 400, 3, 7),
            Troop::Devops => s(1, 55.0, 12.0, 0.6, 2.8, Prefers::Resource, 0.0, false, false, 30, 80, 4, 7),
            Troop::Sales => s(5, 450.0, 12.0, 0.8, 1.3, Prefers::Defense, 0.0, false, false, 120, 500, 5, 8),
            Troop::Data => s(4, 80.0, 45.0, 3.0, 1.5, Prefers::Any, 1.0, false, false, 300, 1_500, 6, 7),
            Troop::Designer => s(14, 500.0, 35.0, 4.0, 2.0, Prefers::Any, 2.0, true, true, 600, 5_000, 7, 6),
            Troop::Drone => s(5, 260.0, 40.0, 0.8, 1.4, Prefers::Defense, 1.2, true, false, 300, 3_000, 8, 6),
            Troop::Consultant => s(20, 3_200.0, 280.0, 0.9, 1.1, Prefers::Any, 0.6, false, false, 900, 25_000, 9, 5),
            Troop::Lead => s(0, 1_100.0, 75.0, 0.9, 1.9, Prefers::Any, 0.0, false, false, 0, 0, 0, HERO_MAX),
        }
    }

    pub fn hp(self, level: u8) -> f32 {
        self.stats().hp * scale(level)
    }

    pub fn dps(self, level: u8) -> f32 {
        self.stats().dps * scale(level)
    }

    /// Its training's cost at `level` (coffee).
    pub fn cost(self, level: u8) -> u64 {
        (self.stats().cost as f64 * (1.0 + 0.35 * (level.max(1) - 1) as f64)).round() as u64
    }

    /// Getting it to `to` in the meeting room: its cost (coffee), its time (seconds).
    pub fn research(self, to: u8) -> (u64, u32) {
        let k = 2u64.pow(to.saturating_sub(2) as u32);
        let cost = self.stats().cost.max(40) * 60 * k;
        let secs = (1_800 * k as u32).min(259_200);
        (cost, secs)
    }
}

impl OpenSpace {
    /// The highest built level of `kind` (0: none).
    pub fn top_level(&self, kind: Kind) -> u8 {
        self.buildings.iter().filter(|b| b.kind == kind && b.level >= 1).map(|b| b.level).max().unwrap_or(0)
    }

    pub fn troop_level(&self, t: Troop) -> u8 {
        self.troop_levels.iter().find(|(k, _)| *k == t).map_or(1, |(_, l)| *l)
    }

    /// Room for the army: the break rooms built.
    pub fn housing_room(&self) -> u32 {
        self.buildings.iter().filter(|b| b.kind == Kind::Lounge && b.level >= 1).map(|b| lounge_room(b.level)).sum()
    }

    /// The room taken: the army and the troops being trained.
    pub fn housing_used(&self) -> u32 {
        self.army.iter().map(|(t, n)| t.stats().housing * n).sum::<u32>() + self.queue.iter().map(|t| t.stats().housing).sum::<u32>()
    }

    pub fn army_count(&self, t: Troop) -> u32 {
        self.army.iter().find(|(k, _)| *k == t).map_or(0, |(_, n)| *n)
    }

    pub fn army_size(&self) -> u32 {
        self.army.iter().map(|(_, n)| n).sum()
    }

    /// How long one `t` takes to train: shared by the recruiting rooms.
    pub fn train_secs(&self, t: Troop) -> i64 {
        let rooms = self.buildings.iter().filter(|b| b.kind == Kind::Recruit && b.level >= 1).count().max(1) as i64;
        (t.stats().train as i64 / rooms).max(1)
    }

    pub fn can_train(&self, t: Troop) -> Result<(), Why> {
        let st = t.stats();
        if self.top_level(Kind::Recruit) < st.unlock {
            return Err(Why::NeedRecruit(st.unlock));
        }
        if self.housing_used() + st.housing > self.housing_room() {
            return Err(Why::Room);
        }
        let price = t.cost(self.troop_level(t));
        if self.coffee < price as f64 {
            return Err(Why::Short(Res::Coffee, price - self.coffee as u64));
        }
        Ok(())
    }

    /// One `t` more to train.
    pub fn train(&mut self, t: Troop, now: i64) -> Result<(), Why> {
        self.can_train(t)?;
        self.coffee -= t.cost(self.troop_level(t)) as f64;
        if self.queue.is_empty() {
            self.queue_until = now + self.train_secs(t);
        }
        self.queue.push(t);
        self.actions += 1;
        Ok(())
    }

    /// The last `t` waiting to be trained taken off, its coffee back.
    pub fn untrain(&mut self, t: Troop, now: i64) -> bool {
        let Some(i) = self.queue.iter().rposition(|k| *k == t) else { return false };
        self.queue.remove(i);
        let cap = self.capacity(Res::Coffee);
        self.coffee = (self.coffee + t.cost(self.troop_level(t)) as f64).min(cap.max(self.coffee));
        if i == 0 {
            self.queue_until = self.queue.first().map_or(0, |n| now + self.train_secs(*n));
        }
        self.actions += 1;
        true
    }

    /// One `t` of the army sent on a raid.
    pub fn take_troop(&mut self, t: Troop) -> bool {
        let Some(slot) = self.army.iter_mut().find(|(k, n)| *k == t && *n > 0) else { return false };
        slot.1 -= 1;
        self.army.retain(|(_, n)| *n > 0);
        self.actions += 1;
        true
    }

    fn add_troop(&mut self, t: Troop) {
        match self.army.iter_mut().find(|(k, _)| *k == t) {
            Some(slot) => slot.1 += 1,
            None => self.army.push((t, 1)),
        }
    }

    /// The highest level `t` can reach now (the meeting room's level, plus one).
    pub fn troop_cap(&self, t: Troop) -> u8 {
        (self.top_level(Kind::Meeting) + 1).min(t.stats().max_level)
    }

    pub fn can_research(&self, t: Troop) -> Result<(), Why> {
        if self.top_level(Kind::Meeting) == 0 {
            return Err(Why::NeedHq(Kind::Meeting.unlocked_at()));
        }
        if self.research.is_some() {
            return Err(Why::Busy);
        }
        if self.top_level(Kind::Recruit) < t.stats().unlock {
            return Err(Why::NeedRecruit(t.stats().unlock));
        }
        let level = self.troop_level(t);
        if level >= self.troop_cap(t) {
            return Err(Why::MaxLevel);
        }
        let (price, _) = t.research(level + 1);
        if self.coffee < price as f64 {
            return Err(Why::Short(Res::Coffee, price - self.coffee as u64));
        }
        Ok(())
    }

    pub fn start_research(&mut self, t: Troop, now: i64) -> Result<(), Why> {
        self.can_research(t)?;
        let (price, secs) = t.research(self.troop_level(t) + 1);
        self.coffee -= price as f64;
        self.research = Some((t, now + secs as i64, secs));
        self.actions += 1;
        Ok(())
    }

    /// The training and the research run to `now`.
    pub(crate) fn run_army(&mut self, now: i64) {
        if let Some((t, until, secs)) = self.research
            && until <= now
        {
            let level = self.troop_level(t) + 1;
            match self.troop_levels.iter_mut().find(|(k, _)| *k == t) {
                Some(slot) => slot.1 = level,
                None => self.troop_levels.push((t, level)),
            }
            self.research = None;
            self.xp += os::xp_for(secs);
        }
        if let Some((until, secs)) = self.hero_training
            && until <= now
        {
            self.hero_level = self.hero_level() + 1;
            self.hero_training = None;
            self.xp += os::xp_for(secs);
        }
        while let Some(&t) = self.queue.first() {
            if self.queue_until > now {
                break;
            }
            let done_at = self.queue_until;
            self.queue.remove(0);
            self.add_troop(t);
            self.trained += 1;
            if let Some(&next) = self.queue.first() {
                self.queue_until = done_at + self.train_secs(next);
            }
        }
    }

    /// Whether the campaign's `level` (from 1) can be played.
    pub fn level_open(&self, level: u32) -> bool {
        level == 1 || self.campaign.get(level as usize - 2).is_some_and(|&s| s > 0)
    }

    pub fn stars(&self, level: u32) -> u8 {
        self.campaign.get(level as usize - 1).copied().unwrap_or(0)
    }

    /// A raid on the campaign's `level` over: its loot (as much as there's room for), its stars kept,
    /// tickets for a first 3 stars, and the troops still standing (`back`) home in the army.
    pub fn finish_raid(&mut self, level: u32, stars: u8, destroyed: f32, back: &[Troop]) -> Outcome {
        let back: Vec<Troop> = back.iter().copied().filter(|t| *t != Troop::Lead).collect();
        for &t in &back {
            self.add_troop(t);
        }
        let (coffee, budget) = reward(level);
        let share = destroyed.clamp(0.0, 1.0) as f64 * self.loot_boost();
        let room = |s: &Self, r| (s.capacity(r) - s.amount(r)).max(0.0);
        let got_coffee = (coffee as f64 * share).floor().min(room(self, Res::Coffee));
        let got_budget = (budget as f64 * share).floor().min(room(self, Res::Budget));
        self.coffee += got_coffee;
        self.budget += got_budget;
        let i = level as usize - 1;
        if self.campaign.len() <= i {
            self.campaign.resize(i + 1, 0);
        }
        let before = self.campaign[i];
        let tickets = if stars == 3 && before < 3 { 3 + level / 10 } else { 0 };
        self.tickets += tickets;
        self.campaign[i] = before.max(stars);
        self.xp += 5 + 10 * stars as u64;
        // A card for a win; a rare or better for a first 3 stars.
        let mut cards = Vec::new();
        if stars > 0 {
            cards.push(self.draw_card(Rarity::Common));
        }
        if stars == 3 && before < 3 {
            cards.push(self.draw_card(Rarity::Rare));
        }
        self.raids += 1;
        if stars > 0 {
            self.wins += 1;
        }
        self.actions += 1;
        Outcome { coffee: got_coffee, budget: got_budget, tickets, stars, new_stars: stars.saturating_sub(before), cards, back: back.len() as u32 }
    }

    /// A raid in the world over (a player, a bot, the boss, a quest): the troops still standing home,
    /// the seniority. Its loot and trophies come from floor after (`gain`).
    pub fn finish_world(&mut self, stars: u8, back: &[Troop]) -> Outcome {
        let back: Vec<Troop> = back.iter().copied().filter(|t| *t != Troop::Lead).collect();
        for &t in &back {
            self.add_troop(t);
        }
        self.xp += 5 + 10 * stars as u64;
        self.raids += 1;
        if stars > 0 {
            self.wins += 1;
        }
        self.actions += 1;
        Outcome { coffee: 0.0, budget: 0.0, tickets: 0, stars, new_stars: 0, cards: Vec::new(), back: back.len() as u32 }
    }

    /// Loot brought from the world (the cards' loot bonus on top), as much as there's room for: what
    /// was kept.
    pub fn gain(&mut self, coffee: f64, budget: f64, tickets: u32) -> (f64, f64) {
        let boost = self.loot_boost();
        let room = |s: &Self, r| (s.capacity(r) - s.amount(r)).max(0.0);
        let c = (coffee.max(0.0) * boost).floor().min(room(self, Res::Coffee));
        let b = (budget.max(0.0) * boost).floor().min(room(self, Res::Budget));
        self.coffee += c;
        self.budget += b;
        self.tickets += tickets;
        self.actions += 1;
        (c, b)
    }

    /// Loot taken by a raid on this open space (what's there at most).
    pub fn lose(&mut self, coffee: f64, budget: f64) {
        self.coffee = (self.coffee - coffee.max(0.0)).max(0.0);
        self.budget = (self.budget - budget.max(0.0)).max(0.0);
        self.actions += 1;
    }

    /// Troops given (a guild, a quest): straight into the army.
    pub fn give_troops(&mut self, list: &[(Troop, u32)]) {
        for &(t, n) in list.iter().filter(|(t, _)| *t != Troop::Lead) {
            for _ in 0..n {
                self.add_troop(t);
            }
        }
        self.actions += 1;
    }

    /// Troops given away (to a guild mate): false when the army doesn't have them.
    pub fn give_away(&mut self, list: &[(Troop, u32)]) -> bool {
        if list.iter().any(|&(t, n)| self.army_count(t) < n) {
            return false;
        }
        for &(t, n) in list {
            if let Some(slot) = self.army.iter_mut().find(|(k, _)| *k == t) {
                slot.1 -= n;
            }
        }
        self.army.retain(|(_, n)| *n > 0);
        self.actions += 1;
        true
    }
}

/// The hero's highest level, and how many the lead dev's office allows by its level.
pub const HERO_MAX: u8 = 20;
pub const HERO_PER_OFFICE: u8 = 5;
/// « Crunch mode »: how far around the hero the troops rush.
pub const CRUNCH_RADIUS: f32 = 3.5;

/// The hero's training to `to`: its cost (coffee), its time (seconds).
pub fn hero_training(to: u8) -> (u64, u32) {
    let to = to as u64;
    (15_000 * to * to, (3_600 * to as u32).min(259_200))
}

/// How long a fallen hero rests, at `level` (seconds).
pub fn hero_rest_secs(level: u8) -> i64 {
    600 + 120 * level as i64
}

/// « Crunch mode » at `level`: the share of their hit points the hero gets back, how long the rush lasts.
pub fn crunch_power(level: u8) -> (f32, f32) {
    (0.4, 5.0 + 0.25 * level as f32)
}

/// Where the hero is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hero {
    /// No lead dev's office yet.
    None,
    /// Ready for a raid.
    Ready,
    /// Training to the next level, until then.
    Training(i64),
    /// Fell in a raid, resting until then.
    Resting(i64),
}

impl OpenSpace {
    pub fn hero_level(&self) -> u8 {
        self.hero_level.max(1)
    }

    /// The hero's highest level for now (the lead dev's office's level).
    pub fn hero_cap(&self) -> u8 {
        (self.top_level(Kind::Heroes) * HERO_PER_OFFICE).min(HERO_MAX)
    }

    pub fn hero(&self, now: i64) -> Hero {
        if self.top_level(Kind::Heroes) == 0 {
            Hero::None
        } else if let Some((until, _)) = self.hero_training {
            Hero::Training(until)
        } else if self.hero_rest > now {
            Hero::Resting(self.hero_rest)
        } else {
            Hero::Ready
        }
    }

    pub fn can_train_hero(&self) -> Result<(), Why> {
        if self.top_level(Kind::Heroes) == 0 {
            return Err(Why::NeedHq(Kind::Heroes.unlocked_at()));
        }
        if self.hero_training.is_some() {
            return Err(Why::Busy);
        }
        let level = self.hero_level();
        if level >= self.hero_cap() {
            return Err(Why::MaxLevel);
        }
        let (price, _) = hero_training(level + 1);
        if self.coffee < price as f64 {
            return Err(Why::Short(Res::Coffee, price - self.coffee as u64));
        }
        Ok(())
    }

    /// The hero trained to the next level (away from raids meanwhile).
    pub fn train_hero(&mut self, now: i64) -> Result<(), Why> {
        self.can_train_hero()?;
        let (price, secs) = hero_training(self.hero_level() + 1);
        self.coffee -= price as f64;
        self.hero_training = Some((now + secs as i64, secs));
        self.actions += 1;
        Ok(())
    }

    /// The hero fell in a raid: resting for a while.
    pub fn hero_fell(&mut self, now: i64) {
        self.hero_rest = now + hero_rest_secs(self.hero_level());
        self.actions += 1;
    }

    /// The hero's rest cut short for tickets: its cost.
    pub fn wake_hero_cost(&self, now: i64) -> u32 {
        os::finish_cost(self.hero_rest - now)
    }

    pub fn wake_hero(&mut self, now: i64) -> Result<(), Why> {
        let price = self.wake_hero_cost(now);
        if self.tickets < price {
            return Err(Why::Tickets(price - self.tickets));
        }
        self.tickets -= price;
        self.hero_rest = 0;
        self.actions += 1;
        Ok(())
    }
}

/// What a raid brought.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub coffee: f64,
    pub budget: f64,
    pub tickets: u32,
    pub stars: u8,
    pub new_stars: u8,
    /// The cards found (their numbers).
    pub cards: Vec<usize>,
    /// The troops back home alive (the hero aside).
    pub back: u32,
}

/// The loot of the campaign's `level` all destroyed (coffee, budget).
pub fn reward(level: u32) -> (u64, u64) {
    let v = (250.0 * (level as f64).powf(1.75)).round() as u64;
    (v, v)
}

/// The rival company of the campaign's `level`.
pub fn rival_name(level: u32) -> String {
    const FIRST: [&str; 10] = ["Kiwi", "Pixel", "Nuage", "Turbo", "Hexa", "Octo", "Néon", "Quanta", "Zénith", "Atlas"];
    const LAST: [&str; 5] = ["Labs", "Studio", "Consulting", "Scale", "Corp"];
    let i = (level.max(1) - 1) as usize;
    format!("{} {}", FIRST[i % 10], LAST[(i / 10).min(4)])
}

/// The campaign's chapters, ten levels each.
pub const CHAPTERS: [Tx; 5] = [("La startup du coin", "The startup next door"), ("L'agence web", "The web agency"), ("L'ESN", "The IT consultancy"), ("La scale-up", "The scale-up"), ("MegaCorp", "MegaCorp")];

fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

/// The open space of the campaign's `level`: its boss's office level grows every five levels, its
/// buildings near the top of what that level allows, a ring of firewalls around its core.
pub fn rival_base(level: u32) -> Vec<Building> {
    let level = level.clamp(1, LEVELS);
    let hq = (1 + (level - 1) / 5).min(HQ_MAX as u32) as u8;
    let mut rng = 0x9E37_79B9_7F4A_7C15u64 ^ (level as u64 * 7_919);
    let mut s = OpenSpace::new(0, 1);
    s.buildings.retain(|b| b.kind == Kind::Hq);
    s.buildings[0].level = hq;
    let c = GRID / 2;
    // A little under the top, more often early in each band of five.
    let step_in = (level - 1) % 5;
    let level_of = |kind: Kind, rng: &mut u64| -> u8 {
        let top = kind.allowed(hq).1.max(1);
        let less = if step_in < 2 { (xorshift(rng) % 2) as u8 } else { 0 };
        top.saturating_sub(less).max(1)
    };
    let count_of = |kind: Kind, share: f64| -> u32 {
        let n = kind.allowed(hq).0 as f64;
        (n * share * (0.6 + 0.1 * step_in as f64)).ceil().min(n) as u32
    };
    let place = |s: &mut OpenSpace, rng: &mut u64, kind: Kind, rmin: i32, rmax: i32| {
        let lvl = level_of(kind, rng);
        for _ in 0..120 {
            let dx = (xorshift(rng) % (2 * rmax as u64 + 1)) as i32 - rmax;
            let dy = (xorshift(rng) % (2 * rmax as u64 + 1)) as i32 - rmax;
            if dx.abs().max(dy.abs()) < rmin {
                continue;
            }
            let (x, y) = (c + dx - kind.size() / 2, c + dy - kind.size() / 2);
            if s.fits(kind, x, y, None) {
                let id = s.next_id;
                s.next_id += 1;
                s.buildings.push(Building { id, kind, x, y, level: lvl, job: None, stored: 0.0 });
                return;
            }
        }
    };
    // The core: storage and defenses, then the traps.
    for kind in [Kind::Fridge, Kind::Safe] {
        for _ in 0..count_of(kind, 1.0) {
            place(&mut s, &mut rng, kind, 3, 6);
        }
    }
    for kind in [Kind::Review, Kind::Linter, Kind::Alarm, Kind::Antivirus, Kind::Honeypot, Kind::Cluster, Kind::Board] {
        for _ in 0..count_of(kind, 1.0) {
            place(&mut s, &mut rng, kind, 3, 8);
        }
    }
    for kind in [Kind::Bug, Kind::Printer, Kind::Aircon] {
        for _ in 0..count_of(kind, 1.0) {
            place(&mut s, &mut rng, kind, 3, 9);
        }
    }
    // The ring of firewalls, as far as there are firewalls (spread out when too few).
    // Rings of firewalls, as far as there are firewalls: the inner one first (spread out when too few).
    let walls = count_of(Kind::Wall, 1.0) as usize;
    let lvl = level_of(Kind::Wall, &mut rng);
    let mut placed = 0;
    for r in [10, 13, 7, 16] {
        if placed >= walls {
            break;
        }
        let mut ring: Vec<(i32, i32)> = Vec::new();
        for k in -r..=r {
            ring.extend([(c + k, c - r), (c + k, c + r)]);
            if k > -r && k < r {
                ring.extend([(c - r, c + k), (c + r, c + k)]);
            }
        }
        ring.sort_by(|a, b| {
            let angle = |p: &(i32, i32)| ((p.1 - c) as f32).atan2((p.0 - c) as f32);
            angle(a).partial_cmp(&angle(b)).unwrap_or(Ordering::Equal)
        });
        let every = (ring.len() as f32 / (walls - placed) as f32).max(1.0);
        let mut k = 0.0f32;
        while (k as usize) < ring.len() && placed < walls {
            let (x, y) = ring[k as usize];
            if s.fits(Kind::Wall, x, y, None) {
                let id = s.next_id;
                s.next_id += 1;
                s.buildings.push(Building { id, kind: Kind::Wall, x, y, level: lvl, job: None, stored: 0.0 });
                placed += 1;
            }
            k += every;
        }
    }
    // Outside: the machines, the army's rooms, the interns.
    for kind in [Kind::Coffee, Kind::Server] {
        for _ in 0..count_of(kind, 0.8) {
            place(&mut s, &mut rng, kind, 11, 16);
        }
    }
    for kind in [Kind::Recruit, Kind::Lounge, Kind::Interns, Kind::Meeting, Kind::Training, Kind::Heroes] {
        for _ in 0..count_of(kind, 0.6) {
            place(&mut s, &mut rng, kind, 11, 17);
        }
    }
    s.buildings
}

/// A defense's way of shooting.
#[derive(Clone, Copy)]
pub struct Tower {
    pub range: f32,
    pub min: f32,
    pub dps: f32,
    /// Seconds between shots.
    pub period: f32,
    pub splash: f32,
    pub ground: bool,
    pub air: bool,
    pub hidden: bool,
}

pub fn tower(kind: Kind) -> Option<Tower> {
    let t = |range, min, dps, period, splash, ground, air, hidden| Some(Tower { range, min, dps, period, splash, ground, air, hidden });
    match kind {
        Kind::Review => t(4.5, 0.0, 16.0, 0.8, 0.0, true, false, false),
        Kind::Linter => t(7.5, 0.0, 11.0, 1.0, 0.0, true, true, false),
        Kind::Alarm => t(9.0, 3.0, 8.0, 5.0, 1.5, true, false, false),
        Kind::Antivirus => t(9.0, 0.0, 90.0, 1.0, 0.6, false, true, false),
        Kind::Honeypot => t(4.0, 0.0, 40.0, 0.6, 0.0, true, true, true),
        Kind::Cluster => t(11.0, 0.0, 55.0, 0.15, 0.0, true, true, false),
        Kind::Board => t(6.0, 0.0, 30.0, 0.15, 0.0, true, true, false),
        _ => None,
    }
}

fn tower_scale(level: u8) -> f32 {
    1.0 + 0.28 * (level.max(1) - 1) as f32
}

/// A building in a raid.
#[derive(Clone, Debug)]
pub struct Def {
    pub id: u32,
    pub kind: Kind,
    pub x: i32,
    pub y: i32,
    pub level: u8,
    pub hp: f32,
    pub max: f32,
    cool: f32,
    /// Not shooting (a code freeze) for that long.
    pub frozen: f32,
    /// The troop it shoots at (its id).
    aim: Option<u32>,
    /// The board's beam warming up on the same target.
    ramp: f32,
    /// A hidden defense seen, a trap gone off.
    pub revealed: bool,
}

impl Def {
    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }

    pub fn center(&self) -> (f32, f32) {
        let h = self.kind.size() as f32 / 2.0;
        (self.x as f32 + h, self.y as f32 + h)
    }

    pub fn is_trap(&self) -> bool {
        self.kind.group() == Group::Traps
    }

    /// Counts in the share destroyed.
    pub fn counts(&self) -> bool {
        self.kind != Kind::Wall && !self.is_trap()
    }

    /// From (x, y) to its edge.
    fn reach(&self, x: f32, y: f32) -> f32 {
        let s = self.kind.size() as f32;
        let dx = (self.x as f32 - x).max(x - (self.x as f32 + s)).max(0.0);
        let dy = (self.y as f32 - y).max(y - (self.y as f32 + s)).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }

    fn resource(&self) -> bool {
        matches!(self.kind, Kind::Coffee | Kind::Server | Kind::Fridge | Kind::Safe)
    }

    fn defense(&self) -> bool {
        self.kind.group() == Group::Defense && self.kind != Kind::Wall
    }
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub id: u32,
    pub troop: Troop,
    pub level: u8,
    pub x: f32,
    pub y: f32,
    pub hp: f32,
    pub max: f32,
    /// The building it goes for.
    pub target: Option<usize>,
    /// The firewall in its way, being broken.
    pub wall: Option<usize>,
    cool: f32,
    pub stun: f32,
    pub slow: f32,
    /// Hitting (for its animation).
    pub hitting: bool,
    /// Where it looks.
    pub dir: (f32, f32),
    /// Faster and harder hitting (a double espresso) for that long.
    pub rush: f32,
}

/// What shows for a moment.
#[derive(Clone, Debug)]
pub enum FxKind {
    /// A shot from a building (its index) to a point (in the air or not).
    Shot { from: usize, to: (f32, f32), air: bool },
    /// A troop's shot, from a point to another.
    Throw { from: (f32, f32), to: (f32, f32) },
    Boom { at: (f32, f32), r: f32 },
    Heal { at: (f32, f32) },
    Zap { at: (f32, f32) },
    Freeze { at: (f32, f32), r: f32 },
}

#[derive(Clone, Debug)]
pub struct Fx {
    pub kind: FxKind,
    pub at: f32,
}

pub struct Battle {
    pub defs: Vec<Def>,
    pub units: Vec<Unit>,
    pub fx: Vec<Fx>,
    pub time: f32,
    /// The troops still to drop: each kind, its level, how many.
    pub left: Vec<(Troop, u8, u32)>,
    pub started: bool,
    next_unit: u32,
    /// Each tile: the building on it (its index), if any.
    grid: Vec<Option<usize>>,
    /// Where troops can be dropped.
    pub drop_ok: Vec<bool>,
    fields: HashMap<usize, Vec<f32>>,
    /// The cards' bonus to each troop: hit points, damage (times).
    pub boosts: Vec<(Troop, f32, f32)>,
    /// The action cards brought: each, its level, whether it was played.
    pub cards: Vec<(Action, u8, bool)>,
    /// The hero brought (to drop like a troop), and whether « Crunch mode » was used.
    pub had_hero: bool,
    pub crunched: bool,
}

const INF: f32 = f32::INFINITY;

fn idx(x: i32, y: i32) -> Option<usize> {
    (x >= 0 && y >= 0 && x < GRID && y < GRID).then(|| (y * GRID + x) as usize)
}

#[derive(PartialEq)]
struct Node(f32, usize);
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal).then(o.1.cmp(&self.1))
    }
}

impl Battle {
    pub fn new(base: &[Building], army: Vec<(Troop, u8, u32)>) -> Self {
        let had_hero = army.iter().any(|(t, _, n)| *t == Troop::Lead && *n > 0);
        let defs: Vec<Def> = base
            .iter()
            .filter(|b| b.level >= 1 && !matches!(b.kind.group(), Group::Decor | Group::Obstacle))
            .map(|b| {
                let max = b.kind.hp(b.level) as f32;
                Def { id: b.id, kind: b.kind, x: b.x, y: b.y, level: b.level, hp: max, max, cool: 0.5, aim: None, ramp: 0.0, revealed: false, frozen: 0.0 }
            })
            .collect();
        let mut grid = vec![None; (GRID * GRID) as usize];
        let mut drop_ok = vec![true; (GRID * GRID) as usize];
        for (i, d) in defs.iter().enumerate() {
            let s = d.kind.size();
            for x in d.x..d.x + s {
                for y in d.y..d.y + s {
                    if let Some(k) = idx(x, y) {
                        grid[k] = Some(i);
                    }
                }
            }
            // Not dropped right next to a building (the traps are hidden: not them).
            if !d.is_trap() {
                for x in d.x - 1..d.x + s + 1 {
                    for y in d.y - 1..d.y + s + 1 {
                        if let Some(k) = idx(x, y) {
                            drop_ok[k] = false;
                        }
                    }
                }
            }
        }
        Battle { defs, units: Vec::new(), fx: Vec::new(), time: 0.0, left: army.into_iter().filter(|(_, _, n)| *n > 0).collect(), started: false, next_unit: 1, grid, drop_ok, fields: HashMap::new(), boosts: Vec::new(), cards: Vec::new(), had_hero, crunched: false }
    }

    pub fn can_drop(&self, x: f32, y: f32) -> bool {
        idx(x.floor() as i32, y.floor() as i32).is_some_and(|k| self.drop_ok[k])
    }

    /// Drops one `t` at (x, y): whether it could.
    pub fn drop(&mut self, t: Troop, x: f32, y: f32) -> bool {
        if self.over() || !self.can_drop(x, y) {
            return false;
        }
        let Some(slot) = self.left.iter_mut().find(|(k, _, n)| *k == t && *n > 0) else { return false };
        slot.2 -= 1;
        let level = slot.1;
        let max = t.hp(level) * self.boost(t).0;
        self.units.push(Unit { id: self.next_unit, troop: t, level, x, y, hp: max, max, target: None, wall: None, cool: 0.3, stun: 0.0, slow: 0.0, hitting: false, dir: (1.0, 0.0), rush: 0.0 });
        self.next_unit += 1;
        self.started = true;
        true
    }

    /// The cards' bonus to troop `t`: hit points, damage (times).
    pub fn boost(&self, t: Troop) -> (f32, f32) {
        self.boosts.iter().find(|(k, _, _)| *k == t).map_or((1.0, 1.0), |&(_, hp, dps)| (hp, dps))
    }

    /// Plays action card `i` (of `cards`) at (x, y): whether it could.
    pub fn play(&mut self, i: usize, x: f32, y: f32) -> bool {
        if self.over() || !(0.0..GRID as f32).contains(&x) || !(0.0..GRID as f32).contains(&y) {
            return false;
        }
        let Some(&(action, level, false)) = self.cards.get(i) else { return false };
        self.cards[i].2 = true;
        let (r, p) = (action.radius(), action.power(level));
        let now = self.time;
        let near = |d: &Def| d.alive() && d.reach(x, y) <= r;
        match action {
            Action::Reinforce => {
                let level = self.left.iter().find(|(t, _, _)| *t == Troop::Intern).map_or(1, |l| l.1).max(1);
                let max = Troop::Intern.hp(level) * self.boost(Troop::Intern).0;
                for k in 0..p as u32 {
                    let a = k as f32 * 2.4;
                    let (ux, uy) = (x + a.cos() * 0.6, y + a.sin() * 0.6);
                    self.units.push(Unit { id: self.next_unit, troop: Troop::Intern, level, x: ux, y: uy, hp: max, max, target: None, wall: None, cool: 0.3, stun: 0.0, slow: 0.0, hitting: false, dir: (1.0, 0.0), rush: 0.0 });
                    self.next_unit += 1;
                }
                self.fx.push(Fx { kind: FxKind::Heal { at: (x, y) }, at: now });
            }
            Action::Hotfix | Action::Friday => {
                let hits: Vec<usize> = self.defs.iter().enumerate().filter(|(_, d)| near(d) && !d.is_trap()).map(|(i, _)| i).collect();
                for i in hits {
                    self.damage_def(i, p);
                }
                self.fx.push(Fx { kind: FxKind::Boom { at: (x, y), r }, at: now });
                if action == Action::Friday {
                    self.fx.push(Fx { kind: FxKind::Boom { at: (x, y), r: r * 0.6 }, at: now + 0.25 });
                }
            }
            Action::Standup => {
                for u in self.units.iter_mut().filter(|u| (u.x - x).hypot(u.y - y) <= r) {
                    u.hp = (u.hp + p).min(u.max);
                    u.stun = 0.0;
                    u.slow = 0.0;
                }
                for k in 0..5 {
                    let a = k as f32 * 1.26;
                    self.fx.push(Fx { kind: FxKind::Heal { at: (x + a.cos() * r * 0.5, y + a.sin() * r * 0.5) }, at: now + k as f32 * 0.08 });
                }
            }
            Action::Rush => {
                for u in self.units.iter_mut().filter(|u| (u.x - x).hypot(u.y - y) <= r) {
                    u.rush = u.rush.max(p);
                }
                self.fx.push(Fx { kind: FxKind::Zap { at: (x, y) }, at: now });
            }
            Action::Freeze => {
                for d in self.defs.iter_mut().filter(|d| near(d)) {
                    d.frozen = d.frozen.max(p);
                }
                self.fx.push(Fx { kind: FxKind::Freeze { at: (x, y), r }, at: now });
            }
        }
        self.started = true;
        true
    }

    /// The hero on the field, standing.
    pub fn hero(&self) -> Option<&Unit> {
        self.units.iter().find(|u| u.troop == Troop::Lead && u.hp > 0.0)
    }

    /// « Crunch mode », once a raid: the hero gets some hit points back, they and the troops around rush.
    pub fn crunch(&mut self) -> bool {
        if self.crunched || self.over() {
            return false;
        }
        let Some(h) = self.units.iter().position(|u| u.troop == Troop::Lead && u.hp > 0.0) else { return false };
        let (heal, secs) = crunch_power(self.units[h].level);
        let (x, y) = (self.units[h].x, self.units[h].y);
        self.units[h].hp = (self.units[h].hp + self.units[h].max * heal).min(self.units[h].max);
        self.units[h].stun = 0.0;
        self.units[h].slow = 0.0;
        for u in self.units.iter_mut().filter(|u| (u.x - x).hypot(u.y - y) <= CRUNCH_RADIUS) {
            u.rush = u.rush.max(secs);
        }
        self.fx.push(Fx { kind: FxKind::Zap { at: (x, y) }, at: self.time });
        self.fx.push(Fx { kind: FxKind::Heal { at: (x, y) }, at: self.time + 0.1 });
        self.crunched = true;
        true
    }

    /// The hero dropped and fallen.
    pub fn hero_fell(&self) -> bool {
        self.had_hero && !self.left.iter().any(|(t, _, n)| *t == Troop::Lead && *n > 0) && self.hero().is_none()
    }

    /// The troops dropped and still standing: they come back to the army after the raid.
    pub fn survivors(&self) -> Vec<Troop> {
        self.units.iter().filter(|u| u.hp > 0.0 && u.troop != Troop::Lead).map(|u| u.troop).collect()
    }

    pub fn troops_left(&self) -> u32 {
        self.left.iter().map(|(_, _, n)| n).sum()
    }

    /// The share destroyed (0 to 1).
    pub fn destroyed(&self) -> f32 {
        let all = self.defs.iter().filter(|d| d.counts()).count();
        if all == 0 {
            return 1.0;
        }
        self.defs.iter().filter(|d| d.counts() && !d.alive()).count() as f32 / all as f32
    }

    pub fn stars(&self) -> u8 {
        let d = self.destroyed();
        let hq_down = self.defs.iter().any(|b| b.kind == Kind::Hq && !b.alive());
        (d >= 0.5) as u8 + hq_down as u8 + (d >= 1.0) as u8
    }

    pub fn over(&self) -> bool {
        self.time >= TIME_LIMIT || self.destroyed() >= 1.0 || (self.started && self.units.is_empty() && self.troops_left() == 0)
    }

    /// The building that `t` goes for, from (x, y).
    fn pick(&self, t: Troop, x: f32, y: f32) -> Option<usize> {
        let near = |want: &dyn Fn(&Def) -> bool| {
            self.defs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.alive() && want(d))
                .min_by(|(_, a), (_, b)| {
                    let (ax, ay) = a.center();
                    let (bx, by) = b.center();
                    ((ax - x).powi(2) + (ay - y).powi(2)).partial_cmp(&((bx - x).powi(2) + (by - y).powi(2))).unwrap_or(Ordering::Equal)
                })
                .map(|(i, _)| i)
        };
        let any = |d: &Def| d.counts() && (d.kind != Kind::Honeypot || d.revealed);
        let first = match t.stats().prefers {
            Prefers::Any => None,
            Prefers::Defense => near(&|d: &Def| d.defense() && any(d)),
            Prefers::Resource => near(&|d: &Def| d.resource()),
            Prefers::Wall => near(&|d: &Def| d.kind == Kind::Wall),
        };
        first.or_else(|| near(&any))
    }

    /// The way to building `target` from every tile: the firewalls cost (to break), the other buildings
    /// can't be crossed.
    fn field(&mut self, target: usize) -> &Vec<f32> {
        if !self.fields.contains_key(&target) {
            let n = (GRID * GRID) as usize;
            let mut dist = vec![INF; n];
            let mut heap = BinaryHeap::new();
            let d = &self.defs[target];
            let s = d.kind.size();
            for x in d.x..d.x + s {
                for y in d.y..d.y + s {
                    if let Some(k) = idx(x, y) {
                        dist[k] = 0.0;
                        heap.push(Node(0.0, k));
                    }
                }
            }
            let cost = |k: usize| -> f32 {
                match self.grid[k] {
                    Some(i) if self.defs[i].alive() && !self.defs[i].is_trap() => {
                        if self.defs[i].kind == Kind::Wall {
                            4.0 + self.defs[i].hp / 60.0
                        } else {
                            INF
                        }
                    }
                    _ => 1.0,
                }
            };
            while let Some(Node(dk, k)) = heap.pop() {
                if dk > dist[k] {
                    continue;
                }
                let (x, y) = ((k as i32) % GRID, (k as i32) / GRID);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                    let Some(j) = idx(x + dx, y + dy) else { continue };
                    // Moving out of a tile: the cost of the tile left (a wall to get through).
                    let c = if dist[k] == 0.0 { 1.0 } else { cost(k) };
                    if c == INF {
                        continue;
                    }
                    let nd = dk + c * if dx != 0 && dy != 0 { 1.414 } else { 1.0 };
                    if nd < dist[j] {
                        dist[j] = nd;
                        heap.push(Node(nd, j));
                    }
                }
            }
            self.fields.insert(target, dist);
        }
        &self.fields[&target]
    }

    fn damage_def(&mut self, i: usize, dmg: f32) {
        let d = &mut self.defs[i];
        if !d.alive() {
            return;
        }
        d.hp -= dmg;
        if d.hp <= 0.0 {
            d.hp = 0.0;
            // A wall down opens new ways; a building down too.
            self.fields.clear();
            let at = d.center();
            self.fx.push(Fx { kind: FxKind::Boom { at, r: d.kind.size() as f32 * 0.6 }, at: self.time });
        }
    }

    /// One step of the raid.
    pub fn step(&mut self) {
        if self.over() {
            return;
        }
        self.time += STEP;
        let now = self.time;
        self.fx.retain(|f| now - f.at < 1.5);

        // The traps under the troops' feet.
        for i in 0..self.defs.len() {
            let d = &self.defs[i];
            if !d.is_trap() || d.revealed || !d.alive() {
                continue;
            }
            let (cx, cy) = d.center();
            let trigger = match d.kind {
                Kind::Aircon => 1.5,
                _ => 0.9,
            };
            let hit = self.units.iter().any(|u| !u.troop.stats().air && (u.x - cx).hypot(u.y - cy) < trigger);
            if !hit {
                continue;
            }
            let k = tower_scale(d.level);
            let kind = d.kind;
            self.defs[i].revealed = true;
            match kind {
                Kind::Bug => {
                    for u in self.units.iter_mut().filter(|u| !u.troop.stats().air && (u.x - cx).hypot(u.y - cy) < 1.6) {
                        u.hp -= 60.0 * k;
                    }
                    self.fx.push(Fx { kind: FxKind::Boom { at: (cx, cy), r: 1.6 }, at: now });
                }
                Kind::Printer => {
                    for u in self.units.iter_mut().filter(|u| !u.troop.stats().air && (u.x - cx).hypot(u.y - cy) < 1.2) {
                        u.stun = u.stun.max(3.0 + k);
                    }
                    self.fx.push(Fx { kind: FxKind::Zap { at: (cx, cy) }, at: now });
                }
                _ => {
                    for u in self.units.iter_mut().filter(|u| (u.x - cx).hypot(u.y - cy) < 3.0) {
                        u.slow = u.slow.max(5.0 + k);
                    }
                    self.fx.push(Fx { kind: FxKind::Freeze { at: (cx, cy), r: 3.0 }, at: now });
                }
            }
        }

        // The defenses.
        for i in 0..self.defs.len() {
            let (kind, level, alive) = (self.defs[i].kind, self.defs[i].level, self.defs[i].alive());
            let Some(tw) = tower(kind) else { continue };
            if !alive {
                continue;
            }
            if self.defs[i].frozen > 0.0 {
                self.defs[i].frozen -= STEP;
                continue;
            }
            let (cx, cy) = self.defs[i].center();
            let in_reach = |u: &Unit| {
                let air = u.troop.stats().air;
                let d = (u.x - cx).hypot(u.y - cy);
                u.hp > 0.0 && ((air && tw.air) || (!air && tw.ground)) && d <= tw.range && d >= tw.min
            };
            if tw.hidden && !self.defs[i].revealed {
                if self.units.iter().any(|u| (u.x - cx).hypot(u.y - cy) <= tw.range) {
                    self.defs[i].revealed = true;
                    self.fx.push(Fx { kind: FxKind::Zap { at: (cx, cy) }, at: now });
                }
                continue;
            }
            let keep = self.defs[i].aim.and_then(|id| self.units.iter().find(|u| u.id == id)).is_some_and(in_reach);
            if !keep {
                let near = self
                    .units
                    .iter()
                    .filter(|u| in_reach(u))
                    .min_by(|a, b| (a.x - cx).hypot(a.y - cy).partial_cmp(&(b.x - cx).hypot(b.y - cy)).unwrap_or(Ordering::Equal))
                    .map(|u| u.id);
                if near != self.defs[i].aim {
                    self.defs[i].ramp = 0.0;
                }
                self.defs[i].aim = near;
            }
            self.defs[i].cool -= STEP;
            let Some(aim) = self.defs[i].aim else { continue };
            if kind == Kind::Board {
                self.defs[i].ramp += STEP;
            }
            if self.defs[i].cool > 0.0 {
                continue;
            }
            self.defs[i].cool = tw.period;
            let ramp = if kind == Kind::Board { (1.0 + self.defs[i].ramp * 1.5).min(8.0) } else { 1.0 };
            let dmg = tw.dps * tw.period * tower_scale(level) * ramp;
            let Some(target) = self.units.iter().find(|u| u.id == aim) else { continue };
            let (tx_, ty_, air) = (target.x, target.y, target.troop.stats().air);
            if tw.splash > 0.0 {
                for u in self.units.iter_mut().filter(|u| u.troop.stats().air == air && (u.x - tx_).hypot(u.y - ty_) <= tw.splash) {
                    u.hp -= dmg;
                }
                self.fx.push(Fx { kind: FxKind::Boom { at: (tx_, ty_), r: tw.splash }, at: now + if kind == Kind::Alarm { 0.6 } else { 0.2 } });
            } else if let Some(u) = self.units.iter_mut().find(|u| u.id == aim) {
                u.hp -= dmg;
            }
            // The cluster and the board fire non-stop: not every shot drawn.
            if !matches!(kind, Kind::Cluster | Kind::Board) || (now * 20.0) as i32 % 3 == 0 {
                self.fx.push(Fx { kind: FxKind::Shot { from: i, to: (tx_, ty_), air }, at: now });
            }
        }

        // The troops.
        for u in 0..self.units.len() {
            if self.units[u].hp <= 0.0 {
                continue;
            }
            let st = self.units[u].troop.stats();
            self.units[u].hitting = false;
            if self.units[u].stun > 0.0 {
                self.units[u].stun -= STEP;
                continue;
            }
            let slow = if self.units[u].slow > 0.0 {
                self.units[u].slow -= STEP;
                0.5
            } else {
                1.0
            };
            let rush = if self.units[u].rush > 0.0 {
                self.units[u].rush -= STEP;
                2.0
            } else {
                1.0
            };
            let speed = st.speed * slow * rush * STEP;
            let (x, y) = (self.units[u].x, self.units[u].y);

            if st.heals {
                // To the most hurt troop around, healing everyone near it.
                let hurt = self
                    .units
                    .iter()
                    .enumerate()
                    .filter(|(j, o)| *j != u && o.hp > 0.0 && o.hp < o.max && !o.troop.stats().heals)
                    .min_by(|(_, a), (_, b)| (a.hp / a.max).partial_cmp(&(b.hp / b.max)).unwrap_or(Ordering::Equal))
                    .map(|(j, o)| (j, o.x, o.y));
                let follow = hurt.map(|(_, tx_, ty_)| (tx_, ty_)).or_else(|| self.units.iter().filter(|o| o.hp > 0.0 && !o.troop.stats().heals).map(|o| (o.x, o.y)).next());
                if let Some((tx_, ty_)) = follow {
                    let d = (tx_ - x).hypot(ty_ - y);
                    if d > st.range * 0.6 {
                        self.move_toward(u, tx_, ty_, speed);
                    }
                    if hurt.is_some() && d <= st.range {
                        let heal = self.units[u].troop.dps(self.units[u].level) * self.boost(self.units[u].troop).1 * STEP;
                        for o in self.units.iter_mut().filter(|o| o.hp > 0.0 && (o.x - tx_).hypot(o.y - ty_) <= st.splash) {
                            o.hp = (o.hp + heal).min(o.max);
                        }
                        if (now * 20.0) as i32 % 20 == 0 {
                            self.fx.push(Fx { kind: FxKind::Heal { at: (tx_, ty_) }, at: now });
                        }
                    }
                }
                continue;
            }

            // Its target: kept while standing.
            let valid = self.units[u].target.is_some_and(|t| self.defs[t].alive());
            if !valid {
                self.units[u].target = self.pick(self.units[u].troop, x, y);
                self.units[u].wall = None;
            }
            let Some(target) = self.units[u].target else { continue };
            // A firewall in the way being broken.
            if let Some(w) = self.units[u].wall
                && !self.defs[w].alive()
            {
                self.units[u].wall = None;
            }
            let hit = match self.units[u].wall {
                Some(w) => Some(w),
                None => (self.defs[target].reach(x, y) <= st.range + 0.15).then_some(target),
            };
            if let Some(h) = hit {
                self.units[u].hitting = true;
                let (hx, hy) = self.defs[h].center();
                let d = ((hx - x), (hy - y));
                let len = d.0.hypot(d.1).max(0.01);
                self.units[u].dir = (d.0 / len, d.1 / len);
                self.units[u].cool -= STEP;
                if self.units[u].cool <= 0.0 {
                    self.units[u].cool = 1.0;
                    let mut dmg = self.units[u].troop.dps(self.units[u].level) * self.boost(self.units[u].troop).1 * if self.units[u].rush > 0.0 { 1.6 } else { 1.0 };
                    if self.defs[h].kind == Kind::Wall && st.prefers == Prefers::Wall {
                        dmg *= 40.0;
                    }
                    if st.prefers == Prefers::Resource && self.defs[h].resource() {
                        dmg *= 2.0;
                    }
                    if st.splash > 0.0 {
                        let hits: Vec<usize> = self.defs.iter().enumerate().filter(|(_, d)| d.alive() && !d.is_trap() && d.reach(hx, hy) <= st.splash).map(|(i, _)| i).collect();
                        for i in hits {
                            self.damage_def(i, dmg);
                        }
                        self.fx.push(Fx { kind: FxKind::Boom { at: (hx, hy), r: st.splash }, at: now + 0.2 });
                    } else {
                        self.damage_def(h, dmg);
                    }
                    if st.range > 1.5 || st.air {
                        self.fx.push(Fx { kind: FxKind::Throw { from: (x, y), to: (hx, hy) }, at: now });
                    }
                }
                continue;
            }
            // On its way: straight when flying, else the cheapest way (a wall on it: broken).
            if st.air {
                let (tx_, ty_) = self.defs[target].center();
                self.move_toward(u, tx_, ty_, speed);
                continue;
            }
            let (tx0, ty0) = (x.floor() as i32, y.floor() as i32);
            let field = self.field(target);
            let here = idx(tx0, ty0).map_or(INF, |k| field[k]);
            let mut best = (here, None);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                if let Some(k) = idx(tx0 + dx, ty0 + dy)
                    && field[k] < best.0
                {
                    best = (field[k], Some((tx0 + dx, ty0 + dy, k)));
                }
            }
            match best.1 {
                Some((bx, by, k)) => {
                    if let Some(i) = self.grid[k]
                        && self.defs[i].alive()
                        && self.defs[i].kind == Kind::Wall
                        && i != target
                    {
                        self.units[u].wall = Some(i);
                    } else {
                        self.move_toward(u, bx as f32 + 0.5, by as f32 + 0.5, speed);
                    }
                }
                None => {
                    let (tx_, ty_) = self.defs[target].center();
                    self.move_toward(u, tx_, ty_, speed);
                }
            }
        }
        self.units.retain(|u| u.hp > 0.0);
    }

    fn move_toward(&mut self, u: usize, tx_: f32, ty_: f32, speed: f32) {
        let unit = &mut self.units[u];
        let (dx, dy) = (tx_ - unit.x, ty_ - unit.y);
        let d = dx.hypot(dy);
        if d < 1e-4 {
            return;
        }
        let k = (speed / d).min(1.0);
        unit.x += dx * k;
        unit.y += dy * k;
        unit.dir = (dx / d, dy / d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_000_000;

    fn camp() -> OpenSpace {
        let mut s = OpenSpace::new(T0, 3);
        s.buildings.retain(|b| !b.kind.is_obstacle());
        s.coffee = 1_000.0;
        s.budget = 1_000.0;
        s.build(Kind::Interns, 14, 19, T0).unwrap();
        s.build(Kind::Recruit, 10, 10, T0).unwrap();
        s.run(T0 + 100);
        s.build(Kind::Lounge, 10, 26, T0 + 100).unwrap();
        s.run(T0 + 600);
        s
    }

    #[test]
    fn troops_train_in_real_time_within_the_room() {
        let mut s = camp();
        assert_eq!(s.housing_room(), 20);
        assert_eq!(s.can_train(Troop::Dev), Err(Why::NeedRecruit(2)));
        for _ in 0..5 {
            s.train(Troop::Intern, T0 + 600).unwrap();
        }
        assert_eq!(s.housing_used(), 5);
        s.run(T0 + 600 + 20 * 3);
        assert_eq!(s.army_count(Troop::Intern), 3);
        s.run(T0 + 600 + 20 * 5);
        assert_eq!(s.army_count(Troop::Intern), 5);
        assert!(s.queue.is_empty());
        s.coffee = 1_000.0;
        for _ in 0..15 {
            s.train(Troop::Intern, T0 + 700).unwrap();
        }
        assert_eq!(s.can_train(Troop::Intern), Err(Why::Room));
        assert!(s.untrain(Troop::Intern, T0 + 700));
        assert!(s.take_troop(Troop::Intern));
        assert_eq!(s.army_count(Troop::Intern), 4);
    }

    #[test]
    fn rival_bases_grow_and_fit() {
        let first = rival_base(1);
        let last = rival_base(LEVELS);
        assert!(first.iter().any(|b| b.kind == Kind::Hq && b.level == 1));
        assert!(last.iter().any(|b| b.kind == Kind::Hq && b.level == HQ_MAX));
        assert!(last.len() > first.len() * 3);
        assert!(last.iter().filter(|b| b.kind == Kind::Wall).count() > 100);
        for level in 1..=LEVELS {
            let base = rival_base(level);
            let mut s = OpenSpace::new(0, 1);
            s.buildings.clear();
            for b in &base {
                assert!(s.fits(b.kind, b.x, b.y, None), "level {level}: {:?} overlaps", b.kind);
                s.buildings.push(b.clone());
            }
        }
        assert_eq!(rival_base(7), rival_base(7), "the same each time");
    }

    fn run(b: &mut Battle) {
        while !b.over() {
            b.step();
        }
    }

    #[test]
    fn the_hero_trains_with_the_office_and_rests_after_falling() {
        let mut s = camp();
        assert_eq!(s.hero(T0), Hero::None);
        let id = 9_999;
        s.buildings.push(Building { id, kind: Kind::Heroes, x: 20, y: 10, level: 1, job: None, stored: 0.0 });
        assert_eq!((s.hero(T0), s.hero_level(), s.hero_cap()), (Hero::Ready, 1, HERO_PER_OFFICE));
        s.coffee = 0.0;
        assert!(matches!(s.can_train_hero(), Err(Why::Short(Res::Coffee, _))));
        s.coffee = 1e9;
        s.train_hero(T0).unwrap();
        let (_, secs) = hero_training(2);
        assert!(matches!(s.hero(T0 + 1), Hero::Training(_)));
        assert_eq!(s.train_hero(T0 + 1), Err(Why::Busy));
        s.run_army(T0 + secs as i64);
        assert_eq!((s.hero(T0 + secs as i64), s.hero_level()), (Hero::Ready, 2));
        // Up to the office's cap.
        s.hero_level = HERO_PER_OFFICE;
        assert_eq!(s.can_train_hero(), Err(Why::MaxLevel));
        // Fallen: resting, then back; or woken for tickets.
        s.hero_fell(T0);
        assert!(matches!(s.hero(T0 + 1), Hero::Resting(_)));
        assert_eq!(s.hero(T0 + hero_rest_secs(s.hero_level())), Hero::Ready);
        s.tickets = 100;
        s.wake_hero(T0 + 1).unwrap();
        assert_eq!(s.hero(T0 + 1), Hero::Ready);
        assert!(s.tickets < 100);
    }

    #[test]
    fn the_hero_crunches_once_and_can_fall() {
        let mut b = Battle::new(&rival_base(30), vec![(Troop::Intern, 1, 3), (Troop::Lead, 5, 1)]);
        assert!(b.had_hero && b.hero().is_none() && !b.hero_fell());
        assert!(!b.crunch(), "no crunch before the hero is dropped");
        let spot = (0..GRID).flat_map(|x| (0..GRID).map(move |y| (x as f32 + 0.5, y as f32 + 0.5))).find(|&(x, y)| b.can_drop(x, y)).unwrap();
        assert!(b.drop(Troop::Lead, spot.0, spot.1));
        assert!(!b.drop(Troop::Lead, spot.0, spot.1), "one hero");
        assert!(b.drop(Troop::Intern, spot.0, spot.1));
        let max = b.hero().unwrap().max;
        assert_eq!(max, Troop::Lead.hp(5));
        b.units[0].hp = max * 0.3;
        assert!(b.crunch());
        let h = b.hero().unwrap();
        assert!((h.hp - max * 0.7).abs() < 1e-3);
        assert!(h.rush > 0.0 && b.units[1].rush > 0.0, "the troops around rush too");
        assert!(!b.crunch(), "once a raid");
        b.units[0].hp = 0.0;
        b.step();
        assert!(b.hero_fell());
    }

    #[test]
    fn a_few_interns_take_the_first_company() {
        let mut b = Battle::new(&rival_base(1), vec![(Troop::Intern, 1, 20)]);
        for k in 0..20 {
            assert!(b.drop(Troop::Intern, 2.0 + (k % 5) as f32 * 0.3, 2.0));
        }
        assert!(!b.drop(Troop::Intern, 2.0, 2.0), "none left");
        run(&mut b);
        assert!(b.stars() >= 2, "stars {} destroyed {}", b.stars(), b.destroyed());
    }

    #[test]
    fn troops_cant_be_dropped_on_the_base() {
        let base = rival_base(1);
        let hq = base.iter().find(|b| b.kind == Kind::Hq).unwrap();
        let b = Battle::new(&base, vec![(Troop::Intern, 1, 1)]);
        assert!(!b.can_drop(hq.x as f32 + 1.0, hq.y as f32 + 1.0));
        assert!(!b.can_drop(hq.x as f32 - 0.5, hq.y as f32 + 1.0), "nor right next to it");
        assert!(b.can_drop(1.5, 1.5));
    }

    #[test]
    fn walls_are_broken_or_gone_around() {
        // A boss's office walled in on all sides: interns break through.
        let mut base = vec![Building { id: 1, kind: Kind::Hq, x: 18, y: 18, level: 1, job: None, stored: 0.0 }];
        let mut id = 2;
        for k in 17..=22 {
            for (x, y) in [(k, 17), (k, 22), (17, k), (22, k)] {
                if !base.iter().any(|b| b.x == x && b.y == y) {
                    base.push(Building { id, kind: Kind::Wall, x, y, level: 1, job: None, stored: 0.0 });
                    id += 1;
                }
            }
        }
        let mut b = Battle::new(&base, vec![(Troop::Intern, 1, 10)]);
        for _ in 0..10 {
            b.drop(Troop::Intern, 5.0, 5.0);
        }
        run(&mut b);
        assert_eq!(b.stars(), 3, "the office down behind its walls");
        assert!(b.defs.iter().any(|d| d.kind == Kind::Wall && !d.alive()), "a wall broken on the way");
    }

    #[test]
    fn a_defense_holds_a_lone_intern() {
        let base = vec![Building { id: 1, kind: Kind::Hq, x: 18, y: 18, level: 1, job: None, stored: 0.0 }, Building { id: 2, kind: Kind::Review, x: 14, y: 14, level: 3, job: None, stored: 0.0 }];
        let mut b = Battle::new(&base, vec![(Troop::Intern, 1, 1)]);
        b.drop(Troop::Intern, 10.0, 10.0);
        run(&mut b);
        assert_eq!(b.stars(), 0);
        assert!(b.units.is_empty());
    }

    #[test]
    fn a_raid_brings_loot_and_stars_once() {
        let mut s = camp();
        assert!(s.level_open(1) && !s.level_open(2));
        let o = s.finish_raid(1, 3, 1.0, &[]);
        assert_eq!(o.new_stars, 3);
        assert!(o.tickets > 0 && o.coffee > 0.0);
        assert!(s.level_open(2));
        let again = s.finish_raid(1, 3, 1.0, &[]);
        assert_eq!((again.new_stars, again.tickets), (0, 0));
        assert_eq!(s.stars(1), 3);
    }

    #[test]
    fn the_world_pays_and_takes() {
        let mut s = camp();
        s.coffee = 0.0;
        s.budget = 0.0;
        let tickets = s.tickets;
        // Loot as much as there's room for.
        let room = s.capacity(Res::Coffee);
        let (c, b) = s.gain(1e12, 50.0, 2);
        assert_eq!((c, b, s.coffee, s.tickets), (room, 50.0, room, tickets + 2));
        // A raid suffered: never below zero.
        s.lose(1e12, 20.0);
        assert_eq!((s.coffee, s.budget), (0.0, 30.0));
        // A world raid: the survivors home, no loot until floor says.
        s.add_troop(Troop::Intern);
        assert!(s.take_troop(Troop::Intern));
        let o = s.finish_world(2, &[Troop::Intern]);
        assert_eq!((o.back, o.coffee, o.cards.len()), (1, 0.0, 0));
        // Troops given and given away.
        let had = s.army_count(Troop::Dev);
        s.give_troops(&[(Troop::Dev, 3), (Troop::Lead, 1)]);
        assert_eq!(s.army_count(Troop::Dev), had + 3);
        assert_eq!(s.army_count(Troop::Lead), 0);
        assert!(!s.give_away(&[(Troop::Dev, had + 4)]));
        assert!(s.give_away(&[(Troop::Dev, had + 3)]));
        assert_eq!(s.army_count(Troop::Dev), 0);
    }

    #[test]
    fn survivors_come_back_to_the_army() {
        let mut s = camp();
        s.add_troop(Troop::Intern);
        s.add_troop(Troop::Intern);
        let had = s.army_count(Troop::Intern);
        assert!(s.take_troop(Troop::Intern) && s.take_troop(Troop::Intern));
        // One fell on the way, one still standing; the hero never joins the army.
        let o = s.finish_raid(1, 0, 0.0, &[Troop::Intern, Troop::Lead]);
        assert_eq!(o.back, 1);
        assert_eq!(s.army_count(Troop::Intern), had - 1);
        assert_eq!(s.army_count(Troop::Lead), 0);
    }

    #[test]
    fn research_levels_troops_up() {
        let mut s = camp();
        assert!(matches!(s.can_research(Troop::Intern), Err(Why::NeedHq(_))));
        let id = s.next_id;
        s.next_id += 1;
        s.buildings.push(Building { id, kind: Kind::Meeting, x: 26, y: 26, level: 1, job: None, stored: 0.0 });
        s.coffee = 1e5;
        s.start_research(Troop::Intern, T0).unwrap();
        assert_eq!(s.can_research(Troop::Dev), Err(Why::Busy));
        s.run(T0 + 3_600);
        assert_eq!(s.troop_level(Troop::Intern), 2);
        assert_eq!(s.can_research(Troop::Intern), Err(Why::MaxLevel));
    }
}
