//! La pêche aux talents (its page in `app::fishing`, the pond shared through `crate::pond`): the
//! player is a recruiter, the fish are candidates (the intern, the developer, the accountant... the
//! five-legged sheep), each one biting at its hours; then the fight once one has bitten. The fish darts up and down a track, the
//! player keeps a bar on it (held: it rises, let go: it falls); on it, the catch fills, off it, it
//! empties. Full: caught; empty: it got away.

/// How a fish moves on the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Behavior {
    /// Goes to and fro, calmly.
    Smooth,
    /// Darts far and fast.
    Dart,
    /// Keeps to the bottom.
    Sinker,
    /// Keeps to the top.
    Floater,
    /// All of these by turns.
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    VeryRare,
    Legendary,
}

impl Rarity {
    fn weight(self) -> f32 {
        match self {
            Rarity::Common => 100.0,
            Rarity::Uncommon => 40.0,
            Rarity::Rare => 12.0,
            Rarity::VeryRare => 4.0,
            Rarity::Legendary => 0.8,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// When a fish bites (the machine's clock).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    Any,
    /// 5:00 to 12:00.
    Morning,
    /// 12:00 to 18:00.
    Afternoon,
    /// 18:00 to 5:00.
    Evening,
    /// 5:00 to 18:00.
    Day,
}

impl When {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The time of day an hour is in: 0 the morning, 1 the afternoon, 2 the evening (and the night).
pub fn time_of_day(hour: u32) -> usize {
    match hour {
        5..=11 => 0,
        12..=17 => 1,
        _ => 2,
    }
}

impl When {
    pub fn bites(self, hour: u32) -> bool {
        let tod = time_of_day(hour);
        match self {
            When::Any => true,
            When::Morning => tod == 0,
            When::Afternoon => tod == 1,
            When::Evening => tod == 2,
            When::Day => tod < 2,
        }
    }
}

/// A fish of the pond. Its name in `i18n` (`fishing_names`, in this order).
#[derive(Clone, Copy, Debug)]
pub struct Fish {
    /// What floor knows it by (floor's src/pond/fish.js: the same ids and sizes).
    pub id: &'static str,
    pub min_cm: u32,
    pub max_cm: u32,
    pub rarity: Rarity,
    /// 0 to 100: how hard it fights.
    pub difficulty: f32,
    pub behavior: Behavior,
    pub when: When,
}

const fn fish(id: &'static str, min_cm: u32, max_cm: u32, rarity: Rarity, difficulty: f32, behavior: Behavior, when: When) -> Fish {
    Fish { id, min_cm, max_cm, rarity, difficulty, behavior, when }
}

use Behavior::*;
use Rarity::*;

pub const FISH: [Fish; 16] = [
    fish("stagiaire", 10, 30, Common, 15.0, Smooth, When::Any),
    fish("alternant", 8, 20, Common, 22.0, Dart, When::Day),
    fish("developpeur", 15, 45, Common, 30.0, Mixed, When::Any),
    fish("commercial", 12, 35, Common, 20.0, Floater, When::Afternoon),
    fish("comptable", 20, 60, Uncommon, 35.0, Sinker, When::Morning),
    fish("chef-de-projet", 30, 70, Uncommon, 30.0, Smooth, When::Evening),
    fish("admin-sys", 30, 90, Uncommon, 45.0, Sinker, When::Any),
    fish("designer", 20, 60, Uncommon, 52.0, Dart, When::Morning),
    fish("support", 25, 60, Uncommon, 50.0, Mixed, When::Afternoon),
    fish("product-owner", 30, 80, Rare, 55.0, Floater, When::Afternoon),
    fish("juriste", 40, 90, Rare, 62.0, Dart, When::Evening),
    fish("devops", 40, 120, Rare, 70.0, Mixed, When::Evening),
    fish("directeur", 50, 120, Rare, 72.0, Dart, When::Any),
    fish("senior-legacy", 80, 200, VeryRare, 80.0, Smooth, When::Morning),
    fish("pdg", 80, 250, VeryRare, 85.0, Sinker, When::Evening),
    fish("mouton-5-pattes", 60, 100, Legendary, 95.0, Mixed, When::Any),
];

/// The fish known by `id`.
pub fn by_id(id: &str) -> Option<usize> {
    FISH.iter().position(|f| f.id == id)
}

/// A number in [0, 1) from the generator.
fn unit(rng: &mut u64) -> f32 {
    // xorshift64
    if *rng == 0 {
        *rng = 0x9e37_79b9_7f4a_7c15;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    (*rng >> 40) as f32 / (1u64 << 24) as f32
}

/// The fish that bites at `hour`, a cast thrown with `power` (0 to 1: farther, the rarer and the
/// bigger a bit more often), and its size in centimeters.
pub fn pick(rng: &mut u64, hour: u32, power: f32) -> (usize, u32) {
    let power = power.clamp(0.0, 1.0);
    let weight = |f: &Fish| if f.rarity == Common { f.rarity.weight() } else { f.rarity.weight() * (1.0 + power * 0.6) };
    let biting: Vec<usize> = (0..FISH.len()).filter(|&k| FISH[k].when.bites(hour)).collect();
    let total: f32 = biting.iter().map(|&k| weight(&FISH[k])).sum();
    let mut roll = unit(rng) * total;
    let mut which = biting[0];
    for &k in &biting {
        roll -= weight(&FISH[k]);
        if roll < 0.0 {
            which = k;
            break;
        }
    }
    let f = &FISH[which];
    // Most of them small, a big one now and then; a long cast pushes up a little.
    let t = (unit(rng).powf(1.8) * (1.0 - power * 0.25) + power * 0.25 * unit(rng)).clamp(0.0, 1.0);
    let size = f.min_cm + ((f.max_cm - f.min_cm) as f32 * t).round() as u32;
    (which, size.clamp(f.min_cm, f.max_cm))
}

/// Where the catch starts.
pub const START: f32 = 0.3;

/// The fight with a fish that has bitten. Positions on the track from 0 (the bottom) to 1 (the top).
#[derive(Clone, Debug)]
pub struct Reel {
    pub fish: usize,
    /// The fish's middle.
    pub fish_y: f32,
    fish_v: f32,
    target: f32,
    /// Seconds before it picks where to go next.
    timer: f32,
    /// The mood a mixed fish is in.
    mood: Behavior,
    /// The bar's bottom, and its height.
    pub bar_y: f32,
    bar_v: f32,
    pub bar_h: f32,
    /// 0 to 1: full, caught.
    pub progress: f32,
    /// The fish was on the bar all along (never left it).
    pub perfect: bool,
    /// Seconds since the bar was last held: left alone at the bottom, it hires nobody.
    idle: f32,
    rng: u64,
}

impl Reel {
    pub fn new(fish: usize, seed: u64) -> Self {
        let d = FISH[fish].difficulty / 100.0;
        let bar_h = 0.30 - d * 0.10;
        Self { fish, fish_y: 0.25, fish_v: 0.0, target: 0.25, timer: 0.6, mood: Smooth, bar_y: 0.0, bar_v: 0.0, bar_h, progress: START, perfect: true, idle: 0.0, rng: seed | 1 }
    }

    /// The fish is on the bar.
    pub fn on_bar(&self) -> bool {
        (self.bar_y..=self.bar_y + self.bar_h).contains(&self.fish_y)
    }

    /// `dt` seconds more, the bar `held` (it rises) or not. Some(true): caught; Some(false): got away.
    pub fn step(&mut self, dt: f32, held: bool) -> Option<bool> {
        let f = &FISH[self.fish];
        let d = f.difficulty / 100.0;

        // The fish: now and then where to go next, by its kind.
        self.timer -= dt;
        if self.timer <= 0.0 {
            if f.behavior == Mixed {
                self.mood = [Smooth, Dart, Sinker, Floater][(unit(&mut self.rng) * 4.0) as usize % 4];
            } else {
                self.mood = f.behavior;
            }
            let r = unit(&mut self.rng);
            let reach = match self.mood {
                Dart => 0.35 + 0.45 * d,
                _ => 0.2 + 0.3 * d,
            };
            let mut target = self.fish_y + (r * 2.0 - 1.0) * reach;
            match self.mood {
                // Low, but off the very bottom (where the bar rests: left alone, it would win).
                Sinker => target = 0.2 + target * 0.5,
                Floater => target = target * 0.7 + 0.3,
                _ => {}
            }
            self.target = target.clamp(0.03, 0.97);
            let pause = match self.mood {
                Dart => 0.35 + unit(&mut self.rng) * 0.6,
                _ => 0.6 + unit(&mut self.rng) * 1.0,
            };
            self.timer = pause * (1.35 - d * 0.75);
        }
        let pull = match self.mood {
            Dart => 14.0 + 26.0 * d,
            _ => 6.0 + 16.0 * d,
        };
        let damp = 2.0 * pull.sqrt();
        self.fish_v += ((self.target - self.fish_y) * pull - self.fish_v * damp) * dt;
        self.fish_y += self.fish_v * dt;
        if !(0.0..=1.0).contains(&self.fish_y) {
            self.fish_y = self.fish_y.clamp(0.0, 1.0);
            self.fish_v = 0.0;
        }

        // The bar: lifted while held, falling else, bouncing a little off the bottom.
        self.bar_v += if held { 2.4 } else { -2.0 } * dt;
        self.bar_v = self.bar_v.clamp(-1.6, 1.6);
        self.bar_y += self.bar_v * dt;
        let top = 1.0 - self.bar_h;
        if self.bar_y < 0.0 {
            self.bar_y = 0.0;
            self.bar_v = if self.bar_v < -0.3 { -self.bar_v * 0.35 } else { 0.0 };
        } else if self.bar_y > top {
            self.bar_y = top;
            self.bar_v = self.bar_v.min(0.0);
        }

        // The catch: filling only with a hand on the bar.
        self.idle = if held { 0.0 } else { self.idle + dt };
        if self.on_bar() {
            if self.idle < 2.0 {
                self.progress += 0.17 * dt;
            }
        } else {
            self.progress -= (0.11 + 0.05 * d) * dt;
            self.perfect = false;
        }
        if self.progress >= 1.0 {
            self.progress = 1.0;
            Some(true)
        } else if self.progress <= 0.0 {
            self.progress = 0.0;
            Some(false)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_and_the_hours() {
        let mut rng = 42;
        let mut seen = [false; FISH.len()];
        for k in 0..20000 {
            let hour = [9, 15, 21][k % 3];
            let (f, size) = pick(&mut rng, hour, (k % 7) as f32 / 6.0);
            assert!(FISH[f].when.bites(hour), "{} at {hour}", FISH[f].id);
            assert!((FISH[f].min_cm..=FISH[f].max_cm).contains(&size));
            seen[f] = true;
        }
        // Each one turns up in the end, the golden one too.
        assert!(seen.iter().all(|&s| s), "{seen:?}");
        // Only in the evening.
        assert!(!When::Evening.bites(10) && When::Evening.bites(23) && When::Evening.bites(3));
        assert_eq!(by_id("directeur"), Some(12));
    }

    /// Following the fish with the bar catches it; doing nothing loses it.
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn followed_caught_left_lost() {
        for fish in 0..FISH.len() {
            let mut reel = Reel::new(fish, 7 + fish as u64);
            let mut end = None;
            for _ in 0..120 * 60 {
                let held = reel.bar_y + reel.bar_h / 2.0 < reel.fish_y;
                end = reel.step(1.0 / 120.0, held);
                if end.is_some() {
                    break;
                }
            }
            if FISH[fish].difficulty < 40.0 {
                assert_eq!(end, Some(true), "{}", FISH[fish].id);
            }
            let mut reel = Reel::new(fish, 3);
            reel.fish_y = 0.9;
            let mut end = None;
            for _ in 0..120 * 60 {
                end = reel.step(1.0 / 120.0, false);
                if end.is_some() {
                    break;
                }
            }
            assert_eq!(end, Some(false), "{}", FISH[fish].id);
        }
    }
}

#[cfg(test)]
mod rates {
    use super::*;

    #[test]
    #[allow(clippy::needless_range_loop)]
    #[ignore = "prints how often a simple player catches each one"]
    fn print_rates() {
        for fish in 0..FISH.len() {
            let mut won = 0;
            let mut secs = 0.0;
            for seed in 0..200u64 {
                let mut reel = Reel::new(fish, seed * 7919 + 1);
                // Holds when the fish is above the bar's middle, reacting 0.15 s late.
                let mut seen = vec![reel.fish_y; 18];
                for k in 0..120 * 90 {
                    seen.push(reel.fish_y);
                    let late = seen[seen.len() - 18];
                    let held = reel.bar_y + reel.bar_h / 2.0 < late;
                    if let Some(w) = reel.step(1.0 / 120.0, held) {
                        if w {
                            won += 1;
                            secs += k as f32 / 120.0;
                        }
                        break;
                    }
                }
            }
            println!("{:16} d={:3} {:3}%  {:.1}s", FISH[fish].id, FISH[fish].difficulty, won / 2, secs / won.max(1) as f32);
        }
    }
}
