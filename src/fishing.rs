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
            Rarity::Rare => 14.0,
            Rarity::VeryRare => 7.0,
            Rarity::Legendary => 4.0,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// What its card is worth in the collection (as much again shiny): floor's src/pond/fish.js.
    pub fn points(self) -> u32 {
        match self {
            Rarity::Common => 1,
            Rarity::Uncommon => 2,
            Rarity::Rare => 5,
            Rarity::VeryRare => 10,
            Rarity::Legendary => 25,
        }
    }
}

/// The five-legged sheep's id (its card unlocks the sheep).
pub const SHEEP: &str = "mouton-5-pattes";

/// Candidates hired in all for the « Chasseur de têtes » badge on floor (src/games/peche.js).
pub const HUNTER: u32 = 100;

/// The collection's points: each card by its rarity, as much again when caught shiny.
pub fn points(cards: &std::collections::BTreeMap<String, crate::config::FishCard>) -> u32 {
    FISH.iter().filter_map(|f| cards.get(f.id).filter(|c| c.count > 0).map(|c| f.rarity.points() * if c.shiny > 0 { 2 } else { 1 })).sum()
}

/// When a fish bites (the machine's clock).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    Any,
    /// 5:00 to 12:00.
    Morning,
    /// 12:00 to 18:00.
    Afternoon,
    /// 16:00 to 5:00 (the end of the working day too).
    Evening,
    /// 5:00 to 18:00.
    Day,
}

impl When {
    pub fn index(self) -> usize {
        self as usize
    }
}

impl When {
    pub fn bites(self, hour: u32) -> bool {
        match self {
            When::Any => true,
            When::Morning => (5..12).contains(&hour),
            When::Afternoon => (12..18).contains(&hour),
            When::Evening => !(5..16).contains(&hour),
            When::Day => (5..18).contains(&hour),
        }
    }
}

/// Hires in a row with no new card: the next one is a card not caught yet (among those biting).
pub const PITY: u32 = 30;

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

pub const FISH: [Fish; 38] = [
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
    // The colleagues one knows by their first name.
    fish("maria", 15, 40, Common, 18.0, Smooth, When::Morning),
    fish("karene", 15, 40, Common, 20.0, Floater, When::Afternoon),
    fish("kevin", 8, 20, Common, 12.0, Dart, When::Day),
    fish("sandrine", 20, 50, Common, 25.0, Sinker, When::Any),
    fish("jessica", 15, 40, Common, 26.0, Floater, When::Day),
    fish("jean-michel", 40, 90, Uncommon, 35.0, Sinker, When::Morning),
    fish("gerard", 30, 80, Uncommon, 40.0, Smooth, When::Any),
    fish("fatou", 25, 60, Uncommon, 38.0, Floater, When::Day),
    fish("jerome", 25, 65, Uncommon, 42.0, Smooth, When::Morning),
    fish("elodie", 20, 55, Uncommon, 46.0, Mixed, When::Afternoon),
    fish("brice", 20, 55, Uncommon, 48.0, Dart, When::Afternoon),
    fish("thierry", 40, 100, Uncommon, 50.0, Sinker, When::Any),
    fish("nathalie", 35, 85, Rare, 60.0, Smooth, When::Morning),
    fish("sophie", 30, 75, Rare, 58.0, Mixed, When::Afternoon),
    fish("lea", 30, 75, Rare, 62.0, Dart, When::Evening),
    fish("bernard", 50, 110, Rare, 66.0, Dart, When::Any),
    fish("patrick", 45, 100, Rare, 68.0, Sinker, When::Evening),
    fish("yannick", 60, 150, VeryRare, 82.0, Mixed, When::Evening),
    fish("monique", 60, 140, VeryRare, 84.0, Smooth, When::Morning),
    fish("clement", 70, 160, VeryRare, 88.0, Dart, When::Any),
    fish("loic", 12, 30, Common, 14.0, Floater, When::Day),
    fish("flash", 50, 110, Legendary, 90.0, Mixed, When::Evening),
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

/// A bait, an offer put on the hook: won with the hires, one used at each cast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bait {
    /// Bites come sooner, the juniors most of all.
    Tickets,
    /// The tech profiles come.
    Remote,
    /// The rarer ones come, and bigger.
    Salary,
}

impl Bait {
    pub const ALL: [Bait; 3] = [Bait::Tickets, Bait::Remote, Bait::Salary];

    /// Its id in the config (and its place in `i18n`'s `fishing_baits`).
    pub fn id(self) -> &'static str {
        match self {
            Bait::Tickets => "tickets-resto",
            Bait::Remote => "teletravail",
            Bait::Salary => "salaire",
        }
    }

    pub fn by_id(id: &str) -> Option<Bait> {
        Self::ALL.into_iter().find(|b| b.id() == id)
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// The profiles working from home draws.
const TECH: [&str; 11] = ["developpeur", "admin-sys", "designer", "devops", "senior-legacy", "elodie", "brice", "sophie", "lea", "patrick", "yannick"];

/// What a cast has for it: its bait, and whether it landed where the bubbles rise.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lure {
    pub bait: Option<Bait>,
    pub hot: bool,
}

/// The fish that bites at `hour`, a cast thrown with `power` (0 to 1: near the bank the commons,
/// far out the rarer ones, a bit bigger), with `lure`; and its size in centimeters.
pub fn pick(rng: &mut u64, hour: u32, power: f32, lure: Lure) -> (usize, u32) {
    pick_among(rng, hour, power, lure, |_| true).unwrap_or((0, FISH[0].min_cm))
}

/// As `pick`, among the fish `wanted` says (None: none of them bites now).
pub fn pick_among(rng: &mut u64, hour: u32, power: f32, lure: Lure, wanted: impl Fn(usize) -> bool) -> Option<(usize, u32)> {
    let power = power.clamp(0.0, 1.0);
    let weight = |f: &Fish| {
        let mut w = f.rarity.weight();
        if f.rarity == Common {
            w *= 1.3 - 0.7 * power;
        } else {
            w *= 0.6 + 1.8 * power;
            if lure.hot {
                w *= 1.8;
            }
        }
        match lure.bait {
            Some(Bait::Tickets) if f.rarity == Common => w * 1.5,
            Some(Bait::Remote) if TECH.contains(&f.id) => w * 3.0,
            Some(Bait::Salary) if f.rarity != Common => w * 2.0,
            _ => w,
        }
    };
    let biting: Vec<usize> = (0..FISH.len()).filter(|&k| FISH[k].when.bites(hour) && wanted(k)).collect();
    if biting.is_empty() {
        return None;
    }
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
    let push = power * 0.25 + if lure.bait == Some(Bait::Salary) { 0.2 } else { 0.0 };
    let t = (unit(rng).powf(1.8) * (1.0 - push) + push * unit(rng)).clamp(0.0, 1.0);
    let size = f.min_cm + ((f.max_cm - f.min_cm) as f32 * t).round() as u32;
    Some((which, size.clamp(f.min_cm, f.max_cm)))
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
    /// Seconds left of a rival's counter-offer: the hire fills slower and empties faster.
    pub offer: f32,
    rng: u64,
}

impl Reel {
    /// `risk`: how far it was hooked (0 to 1), far out the bar a little shorter.
    pub fn new(fish: usize, seed: u64, risk: f32) -> Self {
        let d = FISH[fish].difficulty / 100.0;
        let bar_h = (0.30 - d * 0.10) * (1.0 - 0.1 * risk.clamp(0.0, 1.0));
        Self { fish, fish_y: 0.25, fish_v: 0.0, target: 0.25, timer: 0.6, mood: FISH[fish].behavior, bar_y: 0.0, bar_v: 0.0, bar_h, progress: START, perfect: true, idle: 0.0, offer: 0.0, rng: seed | 1 }
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
                Dart => 0.3 + 0.3 * d,
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
                Dart => 0.5 + unit(&mut self.rng) * 0.7,
                _ => 0.6 + unit(&mut self.rng) * 1.0,
            };
            self.timer = pause * (1.4 - d * 0.6);
        }
        let pull = match self.mood {
            Dart => 11.0 + 14.0 * d,
            _ => 6.0 + 16.0 * d,
        };
        let damp = 2.0 * pull.sqrt();
        self.fish_v += ((self.target - self.fish_y) * pull - self.fish_v * damp) * dt;
        // Never so fast that the bar (which takes time to get going) cannot follow.
        let top_speed = 0.35 + 0.45 * d;
        self.fish_v = self.fish_v.clamp(-top_speed, top_speed);
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
        let rival = self.offer > 0.0;
        self.offer = (self.offer - dt).max(0.0);
        if self.on_bar() {
            if self.idle < 2.0 {
                self.progress += 0.17 * dt * if rival { 0.55 } else { 1.0 };
            }
        } else {
            self.progress -= (0.11 + 0.05 * d) * dt * if rival { 1.6 } else { 1.0 };
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

/// A number from `seed`, the same on every machine (splitmix64).
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d4_9bb1_3311_14eb);
    z ^ (z >> 31)
}

/// Each quarter of an hour, one in three opens with a job fair of 4 minutes: the seconds it still
/// lasts at `unix` (the clock's, so that the colleagues at the pond have the same).
pub fn job_fair(unix: i64) -> Option<i64> {
    let (slot, into) = (unix.div_euclid(900), unix.rem_euclid(900));
    (mix(slot as u64 ^ 0x5a1e).is_multiple_of(3) && into < 240).then_some(240 - into)
}

/// Where the bubbles rise (two places, moving each 75 s, the same for everyone): each one's place
/// on the water (0 to 1 across, 0 to 1 from the far bank to the near), and how strong they are now
/// (rising then fading over its 75 s).
pub fn hotspots(unix: f64) -> [(f32, f32, f32); 2] {
    let slot = (unix / 75.0).floor();
    let into = ((unix - slot * 75.0) / 75.0) as f32;
    let strength = (into * 8.0).min(1.0).min((1.0 - into) * 8.0);
    let at = |k: u64| {
        let h = mix(slot as u64 * 2 + k);
        (0.08 + (h % 1000) as f32 / 1000.0 * 0.84, 0.1 + ((h >> 20) % 1000) as f32 / 1000.0 * 0.6, strength)
    };
    [at(0), at(1)]
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
            let lure = Lure { bait: [None, Some(Bait::Tickets), Some(Bait::Remote), Some(Bait::Salary)][k % 4], hot: k % 5 == 0 };
            let (f, size) = pick(&mut rng, hour, (k % 7) as f32 / 6.0, lure);
            assert!(FISH[f].when.bites(hour), "{} at {hour}", FISH[f].id);
            assert!((FISH[f].min_cm..=FISH[f].max_cm).contains(&size));
            seen[f] = true;
        }
        // Each one turns up in the end, the golden one too.
        assert!(seen.iter().all(|&s| s), "{seen:?}");
        // Only in the evening.
        assert!(!When::Evening.bites(10) && When::Evening.bites(16) && When::Evening.bites(23) && When::Evening.bites(3));
        // Only those not caught yet, when asked.
        assert_eq!(pick_among(&mut rng, 9, 0.5, Lure::default(), |k| k == 0).map(|f| f.0), Some(0));
        assert_eq!(pick_among(&mut rng, 9, 0.5, Lure::default(), |k| FISH[k].when == When::Evening), None);
        assert_eq!(by_id("directeur"), Some(12));
        // The fairs: some quarters of an hour, never more than 4 minutes.
        let fairs = (0..96).filter(|q| job_fair(q * 900 + 10).is_some()).count();
        assert!((15..50).contains(&fairs), "{fairs}");
        assert_eq!(job_fair(900 * 7 + 300), None);
        // As floor counts them: 358 for the whole binder, shiny.
        let all: std::collections::BTreeMap<String, crate::config::FishCard> = FISH.iter().map(|f| (f.id.to_owned(), crate::config::FishCard { count: 1, shiny: 1, ..Default::default() })).collect();
        assert_eq!(points(&all), 358);
    }

    /// Following the fish with the bar catches it; doing nothing loses it.
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn followed_caught_left_lost() {
        for fish in 0..FISH.len() {
            let mut reel = Reel::new(fish, 7 + fish as u64, 0.0);
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
            let mut reel = Reel::new(fish, 3, 0.0);
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
            let mut peak = 0.0f32;
            let mut fast = 0u32;
            let mut ticks = 0u32;
            for seed in 0..200u64 {
                let mut reel = Reel::new(fish, seed * 7919 + 1, 0.0);
                // Holds when the fish is above the bar's middle, reacting 0.15 s late.
                let mut seen = vec![reel.fish_y; 18];
                for k in 0..120 * 90 {
                    seen.push(reel.fish_y);
                    let late = seen[seen.len() - 18];
                    let held = reel.bar_y + reel.bar_h / 2.0 < late;
                    peak = peak.max(reel.fish_v.abs());
                    fast += (reel.fish_v.abs() > 0.8) as u32;
                    ticks += 1;
                    if let Some(w) = reel.step(1.0 / 120.0, held) {
                        if w {
                            won += 1;
                            secs += k as f32 / 120.0;
                        }
                        break;
                    }
                }
            }
            println!("{:16} d={:3} {:3}%  {:.1}s  peak {:.2}/s  >0.8/s {:.0}%", FISH[fish].id, FISH[fish].difficulty, won / 2, secs / won.max(1) as f32, peak, fast as f32 * 100.0 / ticks as f32);
        }
    }
}
