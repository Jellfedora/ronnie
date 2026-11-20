//! Mise en prod du vendredi: the rules (drawn in `app::friday`). It's Friday 17:42, the deploy just
//! went out: hold production until Monday 9:00. Incidents grow on the servers (memory leaks) or come
//! along the cables (bugs, a slow query, waves of traffic); the player spends a budget on fixes
//! (hotfix, rollback, cache, someone on call, coffee) while the team, tiring through the night, does
//! what it can. Between the waves, a card out of three. The satisfaction at 0 %: called in on Monday.
//!
//! The world is 960 × 540 (the room seen from a little above: the back wall at the top, the floor
//! below). Its clock runs in game minutes, `PACE` a second at normal speed.

/// The world's size, and where the floor starts (the back wall above).
pub const W: f32 = 960.0;
pub const H: f32 = 540.0;
pub const FLOOR_Y: f32 = 178.0;
/// Friday 17:42, in minutes from Friday 00:00.
pub const START: u32 = 17 * 60 + 42;
/// Monday 9:00, in game minutes from the start.
pub const END: f32 = (3 * 1440 + 9 * 60 - START) as f32;
/// Game minutes a second, at normal speed.
pub const PACE: f32 = 8.0;
/// Where the traffic comes from (Internet), the phone's desk, the coffee machine, the couch.
pub const NET: (f32, f32) = (60.0, 300.0);
pub const PHONE: (f32, f32) = (120.0, 205.0);
pub const COFFEE: (f32, f32) = (905.0, 395.0);
pub const COUCH: (f32, f32) = (860.0, 470.0);
/// The tutorial's steps.
pub const TUTORIAL_STEPS: usize = 7;
/// The budget's ceiling.
pub const BUDGET_MAX: f32 = 200.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// Three servers, gentle.
    Startup,
    /// Five servers.
    Scaleup,
    /// Five servers, traffic all night (unlocked once a weekend survived).
    BlackFriday,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Startup, Level::Scaleup, Level::BlackFriday];

    fn roles(self) -> &'static [Role] {
        match self {
            Level::Startup => &[Role::Lb, Role::Api, Role::Db],
            _ => &[Role::Lb, Role::Api, Role::Db, Role::Front, Role::Cache],
        }
    }

    /// How much trouble comes, compared with the scale-up.
    fn trouble(self) -> f32 {
        match self {
            Level::Startup => 0.9,
            Level::Scaleup => 1.0,
            Level::BlackFriday => 1.25,
        }
    }

    /// The score's multiplier.
    fn multiplier(self) -> f32 {
        match self {
            Level::Startup => 1.0,
            Level::Scaleup => 1.5,
            Level::BlackFriday => 2.2,
        }
    }
}

/// What a server does (its name, its place in the room).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Lb,
    Api,
    Db,
    Front,
    Cache,
}

impl Role {
    /// Its place: the middle of its foot.
    fn at(self) -> (f32, f32) {
        match self {
            Role::Lb => (230.0, 300.0),
            Role::Api => (470.0, 290.0),
            Role::Db => (720.0, 300.0),
            Role::Front => (330.0, 455.0),
            Role::Cache => (600.0, 460.0),
        }
    }
}

/// A cable's end: Internet, or a server (its index).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Node {
    Net,
    Server(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: Node,
    pub to: usize,
}

#[derive(Clone, Debug)]
pub struct Server {
    pub role: Role,
    pub x: f32,
    pub y: f32,
    /// 0 to 100.
    pub hp: f32,
    /// The memory leak under it, 0 (none) to 1.
    pub leak: f32,
    /// The bugs sitting on it.
    pub bugs: u8,
    /// A cache on it, wearing off (0: none).
    pub cache: f32,
    pub down: bool,
    /// Just rolled back: it flashes (seconds).
    pub flash: f32,
    /// Behind a feature flag: game minutes left without damage.
    pub flag: f32,
    /// Rewritten in Rust: it no longer falls.
    pub rust: bool,
}

impl Server {
    /// Something on it to fix.
    pub fn troubled(&self) -> bool {
        self.leak > 0.05 || self.bugs > 0
    }
}

/// A bug crawling along a cable (`t`: 0 at its start, 1 at the server).
#[derive(Clone, Copy, Debug)]
pub struct Crawler {
    pub edge: usize,
    pub t: f32,
}

/// A packet along a cable; a wave's ones hit the server they reach.
#[derive(Clone, Copy, Debug)]
pub struct Packet {
    pub edge: usize,
    pub t: f32,
    pub wave: bool,
}

/// Someone of the team.
#[derive(Clone, Debug)]
pub struct Member {
    pub name: &'static str,
    /// Their colors (an index in the drawing's palette).
    pub look: u8,
    pub x: f32,
    pub y: f32,
    pub tx: f32,
    pub ty: f32,
    /// 0 to 1: at 0 they fall asleep where they are.
    pub energy: f32,
    /// The server they were sent to.
    pub job: Option<usize>,
    /// Their fix under way (seconds).
    pub work: f32,
    pub asleep: bool,
    pub phoning: bool,
    pub walking: bool,
    /// Away (rewriting in Rust): game minutes left.
    pub away: f32,
    /// How fast they fix (1: as anyone).
    pub skill: f32,
    /// The old senior: asleep from 23:00 to 7:00 whatever.
    pub senior: bool,
    /// The intern: sometimes makes it worse.
    pub intern: bool,
}

impl Member {
    fn new(name: &'static str, look: u8, x: f32, y: f32) -> Self {
        Self { name, look, x, y, tx: x, ty: y, energy: 1.0, job: None, work: 0.0, asleep: false, phoning: false, walking: false, away: 0.0, skill: 1.0, senior: false, intern: false }
    }

    pub fn free(&self) -> bool {
        !self.asleep && self.away <= 0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Hotfix,
    Rollback,
    Cache,
    Oncall,
    Coffee,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Hotfix, Tool::Rollback, Tool::Cache, Tool::Oncall, Tool::Coffee];
}

/// A card picked between two waves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    /// Tom rewrites a server in Rust: it no longer falls, Tom is away 4 h.
    Rust,
    /// The worst server behind a feature flag 2 h; satisfaction −5 %.
    Flag,
    /// One more on the team, who sometimes makes it worse.
    Intern,
    /// Everyone's energy full; budget −20.
    Pizza,
    /// The incidents seen early: the team goes sooner, the leaks grow slower.
    Monitoring,
    /// Fixes three times as fast, asleep from 23:00 to 7:00.
    Senior,
    /// Budget +80.
    Funding,
    /// The bugs crawl slower, and fewer come.
    Firewall,
    /// The load balancer heals itself and takes half the waves.
    Autoscale,
    /// The hotfixes never make a new bug.
    Docs,
    /// The coffee cheaper and stronger.
    Espresso,
    /// The team tires slower and sleeps better.
    Nap,
}

impl Card {
    pub const ALL: [Card; 12] = [Card::Rust, Card::Flag, Card::Intern, Card::Pizza, Card::Monitoring, Card::Senior, Card::Funding, Card::Firewall, Card::Autoscale, Card::Docs, Card::Espresso, Card::Nap];

    /// 0 common, 1 rare, 2 epic.
    pub fn rarity(self) -> u8 {
        match self {
            Card::Pizza | Card::Intern | Card::Funding | Card::Espresso => 0,
            Card::Flag | Card::Monitoring | Card::Firewall | Card::Nap | Card::Docs => 1,
            Card::Rust | Card::Senior | Card::Autoscale => 2,
        }
    }

    /// Taken again and again (the others once).
    fn repeats(self) -> bool {
        matches!(self, Card::Pizza | Card::Funding | Card::Flag)
    }
}

/// When the weekend pauses for a card (game minutes): Saturday 0:00, 12:00, 22:00, Sunday 9:00, 18:00.
pub const PICKS: [f32; 5] = [at_minute(1, 0), at_minute(1, 12 * 60), at_minute(1, 22 * 60), at_minute(2, 9 * 60), at_minute(2, 18 * 60)];

/// Game minutes from the start to `day` (0 Friday) at `minute` of it.
const fn at_minute(day: u32, minute: u32) -> f32 {
    (day * 1440 + minute - START) as f32
}

/// What happens that the page says in a band at the top.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum News {
    Peak,
    Phone,
    Boss,
    Host,
    Npm,
    Snail,
    /// A server fell (its index).
    Down(usize),
    /// Someone fell asleep (their index).
    Asleep(usize),
    /// The client calmed down.
    Calmed,
    Started,
}

/// A word floating up from where something happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    Fixed,
    Relapse,
    Rollback,
    Cached,
    SnailGone,
    Calmed,
    Energy,
    NoBudget,
    NobodyFree,
    /// Someone goes (their index).
    OnIt(usize),
    WellDone,
}

#[derive(Clone, Copy, Debug)]
pub struct Float {
    pub x: f32,
    pub y: f32,
    pub t: f32,
    pub note: Note,
}

/// A spark: its color is a kind (0 gold, 1 red, 2 ice, 3 coffee).
#[derive(Clone, Copy, Debug)]
pub struct Spark {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub t: f32,
    pub kind: u8,
}

/// What is explained the first time it happens, the game waiting (a bit each, kept by the app).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tip {
    Peak,
    Phone,
    Boss,
    Snail,
    Host,
    Down,
    Npm,
    Cards,
    Sleep,
}

impl Tip {
    pub fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// The weekend's figures, for the post-mortem.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub fixed: u32,
    pub downs: u32,
    pub coffees: u32,
    /// The servers up, summed over the game minutes (a fraction of them each minute).
    pub up: f32,
    /// The team's game minutes asleep.
    pub sleep: f32,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub level: Level,
    /// Game minutes since Friday 17:42.
    pub min: f32,
    pub budget: f32,
    /// 0 to 100.
    pub sat: f32,
    pub servers: Vec<Server>,
    pub edges: Vec<Edge>,
    pub crawlers: Vec<Crawler>,
    pub packets: Vec<Packet>,
    pub team: Vec<Member>,
    /// The slow query, on the cable into the database (its index).
    pub snail: Option<usize>,
    /// Game minutes before it leaves by itself.
    pub snail_left: f32,
    /// The phone ringing (seconds).
    pub phone: Option<f32>,
    /// The DSI walking across (where).
    pub boss: Option<f32>,
    /// The host's outage (seconds left).
    pub blackout: f32,
    pub news: Option<(News, f32)>,
    /// Three cards to pick from, the game waiting.
    pub pick: Option<[Card; 3]>,
    pub cards: Vec<Card>,
    pub picks_done: usize,
    /// Some(true): survived; Some(false): called in.
    pub over: Option<bool>,
    pub stats: Stats,
    /// What is explained now (the game waiting), and what was already.
    pub tip: Option<Tip>,
    pub tips_seen: u32,
    /// The tutorial's step, when it is one; how long its "well done" stays.
    pub tutorial: Option<usize>,
    pub tutorial_wait: f32,
    pub floats: Vec<Float>,
    pub sparks: Vec<Spark>,
    /// The weekend's events already come (bits).
    done: u32,
    /// The phone's next ring (game minutes).
    next_ring: f32,
    rng: u64,
}

impl Game {
    pub fn new(level: Level, seed: u64, tips_seen: u32) -> Self {
        let servers: Vec<Server> = level.roles().iter().map(|&role| {
            let (x, y) = role.at();
            Server { role, x, y, hp: 100.0, leak: 0.0, bugs: 0, cache: 0.0, down: false, flash: 0.0, flag: 0.0, rust: false }
        }).collect();
        let index = |r: Role| servers.iter().position(|s| s.role == r);
        let mut edges = Vec::new();
        let pairs = [(None, Role::Lb), (Some(Role::Lb), Role::Api), (Some(Role::Lb), Role::Front), (Some(Role::Api), Role::Db), (Some(Role::Api), Role::Cache), (Some(Role::Front), Role::Api)];
        for (from, to) in pairs {
            let Some(to) = index(to) else { continue };
            let from = match from {
                None => Node::Net,
                Some(r) => match index(r) {
                    Some(i) => Node::Server(i),
                    None => continue,
                },
            };
            edges.push(Edge { from, to });
        }
        let team = vec![Member::new("Léa", 0, 820.0, 250.0), Member::new("Tom", 1, 770.0, 420.0), Member::new("Sam", 2, 150.0, 440.0)];
        Self {
            level,
            min: 0.0,
            budget: 60.0,
            sat: 100.0,
            servers,
            edges,
            crawlers: Vec::new(),
            packets: Vec::new(),
            team,
            snail: None,
            snail_left: 0.0,
            phone: None,
            boss: None,
            blackout: 0.0,
            news: None,
            pick: None,
            cards: Vec::new(),
            picks_done: 0,
            over: None,
            stats: Stats::default(),
            tip: None,
            tips_seen,
            tutorial: None,
            tutorial_wait: 0.0,
            floats: Vec::new(),
            sparks: Vec::new(),
            done: 0,
            next_ring: 18.0 * 60.0,
            rng: seed | 1,
        }
    }

    /// The tutorial: the scale-up's room, the clock stopped, a full budget.
    pub fn tutorial(seed: u64) -> Self {
        let mut g = Self::new(Level::Scaleup, seed, u32::MAX);
        g.budget = BUDGET_MAX;
        g.tutorial = Some(0);
        g
    }

    fn rnd(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The day (0 Friday … 3 Monday) and the hour (with its fraction).
    pub fn clock(&self) -> (u32, f32) {
        let total = START as f32 + self.min;
        ((total / 1440.0) as u32, (total % 1440.0) / 60.0)
    }

    pub fn night(&self) -> bool {
        let (_, h) = self.clock();
        !(7.0..22.0).contains(&h)
    }

    /// Waiting for the player (a tip, a card), or finished.
    pub fn waiting(&self) -> bool {
        self.tip.is_some() || self.pick.is_some() || self.over.is_some()
    }

    pub fn cost(&self, tool: Tool) -> f32 {
        match tool {
            Tool::Hotfix => 10.0,
            Tool::Rollback => 40.0,
            Tool::Cache => 30.0,
            Tool::Oncall => 20.0,
            Tool::Coffee if self.cards.contains(&Card::Espresso) => 8.0,
            Tool::Coffee => 15.0,
        }
    }

    /// A cable's point at `t` (0 its start, 1 its end): a curve sagging toward the floor.
    pub fn edge_point(&self, e: usize, t: f32) -> (f32, f32) {
        let edge = self.edges[e];
        let (ax, ay) = match edge.from {
            Node::Net => (NET.0, NET.1 - 8.0),
            Node::Server(i) => (self.servers[i].x + 30.0, self.servers[i].y - 8.0),
        };
        let to = &self.servers[edge.to];
        let (bx, by) = (to.x - 30.0, to.y - 8.0);
        let (mx, my) = ((ax + bx) / 2.0, ay.max(by) + 34.0);
        let u = 1.0 - t;
        (u * u * ax + 2.0 * u * t * mx + t * t * bx, u * u * ay + 2.0 * u * t * my + t * t * by)
    }

    /// The server under a point of the world.
    pub fn server_at(&self, x: f32, y: f32) -> Option<usize> {
        self.servers.iter().position(|s| x > s.x - 46.0 && x < s.x + 52.0 && y > s.y - 130.0 && y < s.y + 34.0)
    }

    fn float(&mut self, x: f32, y: f32, note: Note) {
        self.floats.push(Float { x, y, t: 1.4, note });
    }

    fn spark(&mut self, x: f32, y: f32, kind: u8, n: usize) {
        for _ in 0..n {
            let a = self.rnd() * std::f32::consts::TAU;
            let v = 40.0 + self.rnd() * 90.0;
            let t = 0.6 + self.rnd() * 0.4;
            self.sparks.push(Spark { x, y, vx: a.cos() * v, vy: a.sin() * v - 40.0, t, kind });
        }
    }

    fn tell(&mut self, news: News) {
        self.news = Some((news, 4.0));
    }

    /// Explained once: the game waits until the player has read it.
    fn tip_once(&mut self, tip: Tip) {
        if self.tips_seen & tip.bit() == 0 && self.tutorial.is_none() {
            self.tips_seen |= tip.bit();
            self.tip = Some(tip);
        }
    }

    /// The player read the tip.
    pub fn dismiss_tip(&mut self) {
        self.tip = None;
    }

    /// A tool used where the player clicked (the world's point). Coffee goes anywhere; the others on a
    /// server. Whether something was done.
    pub fn act(&mut self, tool: Tool, x: f32, y: f32) -> bool {
        if self.waiting() {
            return false;
        }
        let cost = self.cost(tool);
        if self.budget < cost {
            self.float(x, y, Note::NoBudget);
            return false;
        }
        if tool == Tool::Coffee {
            self.budget -= cost;
            let boost = if self.cards.contains(&Card::Espresso) { 0.6 } else { 0.4 };
            for m in &mut self.team {
                m.energy = (m.energy + boost).min(1.0);
                m.asleep = false;
            }
            self.stats.coffees += 1;
            self.spark(COFFEE.0, COFFEE.1 - 30.0, 3, 18);
            self.float(COFFEE.0 - 30.0, COFFEE.1 - 60.0, Note::Energy);
            return true;
        }
        let Some(i) = self.server_at(x, y) else { return false };
        let (sx, sy) = (self.servers[i].x, self.servers[i].y);
        match tool {
            Tool::Hotfix => {
                let s = &mut self.servers[i];
                if s.leak > 0.05 {
                    s.leak = (s.leak - 0.45).max(0.0);
                } else if s.bugs > 0 {
                    s.bugs -= 1;
                } else {
                    s.hp = (s.hp + 10.0).min(100.0);
                }
                self.stats.fixed += 1;
                self.spark(sx, sy - 70.0, 0, 14);
                self.float(sx, sy - 140.0, Note::Fixed);
                if !self.cards.contains(&Card::Docs) && self.tutorial.is_none() && self.rnd() < 0.1 {
                    self.servers[i].bugs += 1;
                    self.float(sx, sy - 160.0, Note::Relapse);
                }
            }
            Tool::Rollback => {
                let s = &mut self.servers[i];
                if s.troubled() {
                    self.stats.fixed += 1;
                }
                s.leak = 0.0;
                s.bugs = 0;
                s.cache = 0.0;
                s.down = false;
                s.hp = s.hp.max(60.0);
                s.flash = 1.0;
                self.float(sx, sy - 140.0, Note::Rollback);
            }
            Tool::Cache => {
                self.servers[i].cache = 1.0;
                if self.snail.is_some_and(|e| self.edges[e].to == i) {
                    self.snail = None;
                    self.float(sx, sy - 160.0, Note::SnailGone);
                }
                self.spark(sx, sy - 70.0, 2, 14);
                self.float(sx, sy - 140.0, Note::Cached);
            }
            Tool::Oncall => {
                let pick = (0..self.team.len()).filter(|&k| self.team[k].free() && !self.team[k].phoning).max_by(|&a, &b| self.team[a].energy.total_cmp(&self.team[b].energy));
                let Some(k) = pick else {
                    self.float(sx, sy - 140.0, Note::NobodyFree);
                    return false;
                };
                self.team[k].job = Some(i);
                self.team[k].work = 0.0;
                self.float(sx, sy - 140.0, Note::OnIt(k));
            }
            Tool::Coffee => {}
        }
        self.budget -= cost;
        true
    }

    /// A card picked (its index in the three).
    pub fn choose(&mut self, k: usize) {
        let Some(cards) = self.pick.take() else { return };
        let card = cards[k.min(2)];
        self.cards.push(card);
        match card {
            Card::Rust => {
                // The server worst off; Tom away 4 h.
                if let Some(i) = (0..self.servers.len()).filter(|&i| !self.servers[i].rust).min_by(|&a, &b| self.servers[a].hp.total_cmp(&self.servers[b].hp)) {
                    let s = &mut self.servers[i];
                    s.rust = true;
                    s.down = false;
                    s.hp = s.hp.max(70.0);
                }
                if let Some(tom) = self.team.iter_mut().find(|m| m.name == "Tom") {
                    tom.away = 240.0;
                    tom.job = None;
                    tom.phoning = false;
                }
            }
            Card::Flag => {
                if let Some(i) = (0..self.servers.len()).min_by(|&a, &b| self.servers[a].hp.total_cmp(&self.servers[b].hp)) {
                    self.servers[i].flag = 120.0;
                }
                self.sat = (self.sat - 5.0).max(1.0);
            }
            Card::Intern => {
                let mut m = Member::new("Stagiaire", 3, 60.0, 470.0);
                m.skill = 0.7;
                m.intern = true;
                self.team.push(m);
            }
            Card::Pizza => {
                for m in &mut self.team {
                    m.energy = 1.0;
                    m.asleep = false;
                }
                self.budget = (self.budget - 20.0).max(0.0);
            }
            Card::Senior => {
                let mut m = Member::new("Michel", 4, 60.0, 470.0);
                m.skill = 3.0;
                m.senior = true;
                self.team.push(m);
            }
            Card::Funding => self.budget = (self.budget + 80.0).min(BUDGET_MAX + 80.0),
            Card::Monitoring | Card::Firewall | Card::Autoscale | Card::Docs | Card::Espresso | Card::Nap => {}
        }
    }

    /// Three cards to pick from: different, the ones taken once not again; the rarer, the rarer.
    fn draw_cards(&mut self) -> [Card; 3] {
        let mut out: Vec<Card> = Vec::new();
        let mut tries = 0;
        while out.len() < 3 && tries < 200 {
            tries += 1;
            let roll = self.rnd();
            let rarity = if roll < 0.14 { 2 } else if roll < 0.5 { 1 } else { 0 };
            let pool: Vec<Card> = Card::ALL.into_iter().filter(|c| c.rarity() == rarity && !out.contains(c) && (c.repeats() || !self.cards.contains(c))).collect();
            if pool.is_empty() {
                continue;
            }
            let k = ((self.rnd() * pool.len() as f32) as usize).min(pool.len() - 1);
            out.push(pool[k]);
        }
        // Never short of three: the repeatable ones fill.
        for c in [Card::Pizza, Card::Funding, Card::Flag] {
            if out.len() < 3 && !out.contains(&c) {
                out.push(c);
            }
        }
        [out[0], out[1], out[2]]
    }

    /// The score of the weekend (once over).
    pub fn score(&self) -> u32 {
        let uptime = if self.min > 0.0 { self.stats.up / self.min } else { 1.0 };
        let reached = (self.min / END).min(1.0);
        let base = uptime * 6000.0 * reached + self.sat * 40.0 + self.stats.fixed as f32 * 15.0 + if self.over == Some(true) { 4000.0 } else { 0.0 } - self.stats.downs as f32 * 250.0;
        (base.max(0.0) * self.level.multiplier()) as u32
    }

    /// The share of time the servers were up, 0 to 1.
    pub fn uptime(&self) -> f32 {
        if self.min > 0.0 { (self.stats.up / self.min).clamp(0.0, 1.0) } else { 1.0 }
    }

    /// A step of `dt` seconds, the clock `speed` times faster than normal.
    pub fn step(&mut self, dt: f32, speed: f32) {
        self.fade(dt);
        if self.waiting() {
            return;
        }
        if self.tutorial.is_some() {
            self.tutorial_motion(dt);
            return;
        }
        let gm = dt * PACE * speed;
        self.min += gm;
        let (day, hour) = self.clock();
        let night = self.night();
        let boss = if self.boss.is_some() { 2.0 } else { 1.0 };
        let trouble = self.level.trouble();
        self.budget = (self.budget + gm * 0.2).min(BUDGET_MAX.max(self.budget));

        if self.min >= END {
            self.min = END;
            self.over = Some(self.sat > 0.0);
            return;
        }
        self.events(day, hour);
        if self.waiting() {
            return;
        }

        // New trouble: more as the weekend goes, at night, with the DSI around.
        let rate = (0.004 + self.min / END * 0.006) * boss * trouble * if night { 1.3 } else { 1.0 };
        let leak_growth = if self.cards.contains(&Card::Monitoring) { 0.0032 } else { 0.004 };
        for i in 0..self.servers.len() {
            if !self.servers[i].down && self.servers[i].flag <= 0.0 && self.rnd() < rate * gm * 0.7 {
                self.servers[i].leak = self.servers[i].leak.max(0.08);
            }
            let s = &mut self.servers[i];
            if s.leak > 0.0 && s.flag <= 0.0 {
                s.leak = (s.leak + gm * leak_growth * boss).min(1.0);
            }
        }
        let firewall = self.cards.contains(&Card::Firewall);
        if self.rnd() < rate * gm * 0.8 * if firewall { 0.7 } else { 1.0 } && !self.edges.is_empty() {
            let e = ((self.rnd() * self.edges.len() as f32) as usize).min(self.edges.len() - 1);
            self.crawlers.push(Crawler { edge: e, t: 0.0 });
        }
        if self.snail.is_none() && self.min > 90.0 && self.rnd() < gm * 0.002 * trouble {
            if let Some(e) = self.edges.iter().position(|e| self.servers[e.to].role == Role::Db) {
                self.snail = Some(e);
                self.snail_left = 240.0;
                self.tell(News::Snail);
                self.tip_once(Tip::Snail);
            }
        }
        if self.snail.is_some() {
            self.snail_left -= gm;
            if self.snail_left <= 0.0 {
                self.snail = None;
            }
        }
        // Black Friday: the traffic never stops.
        if self.level == Level::BlackFriday && self.rnd() < gm * 0.003 {
            self.wave(14);
        }

        self.crawl(dt * 0.12 * speed * if firewall { 0.6 } else { 1.0 });
        self.traffic(dt, speed);

        // Damage, servers falling.
        let autoscale = self.cards.contains(&Card::Autoscale);
        let mut fell = Vec::new();
        for (i, s) in self.servers.iter_mut().enumerate() {
            s.flash = (s.flash - dt * 2.0).max(0.0);
            if s.flag > 0.0 {
                s.flag -= gm;
                continue;
            }
            if s.down {
                continue;
            }
            let snail = self.snail.is_some_and(|e| self.edges[e].to == i) && s.cache <= 0.0;
            let mut hurt = (s.leak * 0.9 + s.bugs as f32 * 0.25 + if snail { 0.25 } else { 0.0 }) * boss;
            if s.rust {
                hurt *= 0.3;
            }
            s.hp = (s.hp - hurt * gm * 0.6).min(100.0);
            if autoscale && s.role == Role::Lb {
                s.hp = (s.hp + gm * 0.05).min(100.0);
            }
            // Nothing wrong: it mends slowly.
            if !s.troubled() && !snail {
                s.hp = (s.hp + gm * 0.15).min(100.0);
            }
            if s.cache > 0.0 {
                s.cache = (s.cache - gm * 0.002).max(0.0);
            }
            if s.hp <= 0.0 {
                if s.rust {
                    s.hp = 1.0;
                } else {
                    s.hp = 0.0;
                    s.down = true;
                    fell.push(i);
                }
            }
        }
        for i in fell {
            self.stats.downs += 1;
            let (x, y) = (self.servers[i].x, self.servers[i].y);
            self.spark(x, y - 60.0, 1, 24);
            self.tell(News::Down(i));
            self.tip_once(Tip::Down);
        }

        // Satisfaction: down while servers are, back up slowly when all is well; the phone unanswered.
        let n = self.servers.len() as f32;
        let downs = self.servers.iter().filter(|s| s.down).count() as f32;
        let weak = self.servers.iter().any(|s| s.hp < 30.0);
        self.stats.up += gm * (n - downs) / n;
        let ringing = self.phone.is_some_and(|t| t > 2.0);
        self.sat += gm * if downs > 0.0 { -0.06 * downs } else if weak { -0.01 } else { 0.03 } - if ringing { gm * 0.03 } else { 0.0 };
        self.sat = self.sat.clamp(0.0, 100.0);
        if self.sat <= 0.0 {
            self.over = Some(false);
            return;
        }
        if let Some(t) = &mut self.phone {
            *t += dt;
        }
        self.team_moves(dt, speed, gm);

        // The DSI walks across, then leaves; the outage passes.
        if let Some(x) = &mut self.boss {
            *x += dt * 26.0 * speed.min(3.0);
            if *x > W + 40.0 {
                self.boss = None;
            }
        }
        self.blackout = (self.blackout - dt).max(0.0);
    }

    /// The weekend's events, each once, and the pauses for the cards.
    fn events(&mut self, day: u32, hour: f32) {
        let once = |done: &mut u32, bit: u32, at: f32, min: f32| {
            let due = min >= at && *done & (1 << bit) == 0;
            if due {
                *done |= 1 << bit;
            }
            due
        };
        let min = self.min;
        // The evening peaks: Friday, Saturday, Sunday 20:00.
        for (k, d) in [0, 1, 2].into_iter().enumerate() {
            if once(&mut self.done, k as u32, at_minute(d, 20 * 60), min) {
                self.tell(News::Peak);
                self.wave(if self.level == Level::Startup { 26 } else { 40 });
                self.tip_once(Tip::Peak);
            }
        }
        if once(&mut self.done, 3, at_minute(1, 19 * 60), min) || once(&mut self.done, 4, at_minute(2, 14 * 60), min) {
            self.boss = Some(-40.0);
            self.tell(News::Boss);
            self.tip_once(Tip::Boss);
        }
        if self.level != Level::Startup && once(&mut self.done, 5, at_minute(1, 21 * 60), min) {
            self.blackout = 3.2;
            // A blow, not a fall: the ones up lose a share, never below 5.
            for s in &mut self.servers {
                if !s.rust && !s.down {
                    s.hp = (s.hp - 15.0).max(5.0_f32.min(s.hp));
                }
            }
            self.tell(News::Host);
            self.tip_once(Tip::Host);
        }
        if once(&mut self.done, 6, at_minute(2, 3 * 60), min) {
            for _ in 0..if self.level == Level::Startup { 10 } else { 18 } {
                let e = ((self.rnd() * self.edges.len() as f32) as usize).min(self.edges.len() - 1);
                let t = -self.rnd() * 0.8;
                self.crawlers.push(Crawler { edge: e, t });
            }
            self.tell(News::Npm);
            self.tip_once(Tip::Npm);
        }
        // The client calls now and then, in the day.
        if self.phone.is_none() && min >= self.next_ring && (8.0..21.0).contains(&hour) && day < 3 {
            self.phone = Some(0.0);
            self.next_ring = min + 300.0 + self.rnd() * 300.0;
            self.tell(News::Phone);
            self.tip_once(Tip::Phone);
        }
        // A card at each pause.
        if self.pick.is_none() && self.picks_done < PICKS.len() && min >= PICKS[self.picks_done] {
            self.picks_done += 1;
            let cards = self.draw_cards();
            self.pick = Some(cards);
            self.tip_once(Tip::Cards);
        }
    }

    /// A wave of `n` packets from Internet.
    fn wave(&mut self, n: usize) {
        for i in 0..n {
            self.packets.push(Packet { edge: 0, t: -(i as f32) * 0.04, wave: true });
        }
    }

    /// The bugs move along their cables; at the end, they sit on the server.
    fn crawl(&mut self, by: f32) {
        for b in &mut self.crawlers {
            b.t += by;
        }
        let (edges, servers) = (&self.edges, &mut self.servers);
        self.crawlers.retain(|b| {
            if b.t < 1.0 {
                return true;
            }
            let s = &mut servers[edges[b.edge].to];
            s.bugs = (s.bugs + 1).min(6);
            false
        });
    }

    /// The packets: the ordinary ones, slowed behind the snail; a wave's ones hit the load balancer.
    fn traffic(&mut self, dt: f32, speed: f32) {
        if self.rnd() < dt * 6.0 && !self.edges.is_empty() {
            let e = ((self.rnd() * self.edges.len() as f32) as usize).min(self.edges.len() - 1);
            self.packets.push(Packet { edge: e, t: 0.0, wave: false });
        }
        let autoscale = self.cards.contains(&Card::Autoscale);
        let mut absorbed = false;
        for k in 0..self.packets.len() {
            let p = self.packets[k];
            let slow = self.snail == Some(p.edge) && (0.45..0.55).contains(&p.t);
            let t = p.t + dt * 0.5 * if slow { 0.08 } else { 1.0 } * speed.min(4.0);
            self.packets[k].t = t;
            if p.wave && p.t < 1.0 && t >= 1.0 && self.tutorial.is_none() {
                let s = &mut self.servers[self.edges[p.edge].to];
                if s.cache > 0.0 {
                    s.cache = (s.cache - 0.03).max(0.0);
                    absorbed = true;
                } else if !s.down && s.flag <= 0.0 {
                    s.hp -= if autoscale { 0.4 } else { 0.8 };
                }
            }
        }
        if absorbed && self.rnd() < 0.3 {
            let s = &self.servers[self.edges[0].to];
            let (x, y) = (s.x - 30.0, s.y - 40.0);
            self.spark(x, y, 2, 2);
        }
        self.packets.retain(|p| p.t < 1.0);
    }

    /// The team: to the phone, to their job, to the worst server, to the couch, or asleep.
    fn team_moves(&mut self, dt: f32, speed: f32, gm: f32) {
        let (_, hour) = self.clock();
        let night = self.night();
        let nap = self.cards.contains(&Card::Nap);
        let monitoring = self.cards.contains(&Card::Monitoring);
        // The phone: someone free goes to answer it.
        if self.phone.is_some() && !self.team.iter().any(|m| m.phoning) {
            let k = self.team.iter().position(|m| m.free() && m.job.is_none()).or_else(|| self.team.iter().position(Member::free));
            if let Some(k) = k {
                let m = &mut self.team[k];
                m.phoning = true;
                m.job = None;
                m.work = 0.0;
            }
        }
        for k in 0..self.team.len() {
            let m = &mut self.team[k];
            if m.away > 0.0 {
                m.away -= gm;
                // Off rewriting, at the back: not seen working.
                m.tx = 900.0;
                m.ty = 230.0;
            }
            let senior_sleeps = m.senior && !(7.0..23.0).contains(&hour);
            if m.asleep {
                self.stats.sleep += gm;
                m.energy = (m.energy + gm * if nap { 0.008 } else { 0.004 }).min(1.0);
                if m.energy > 0.7 && !senior_sleeps {
                    m.asleep = false;
                }
                continue;
            }
            if senior_sleeps {
                m.asleep = true;
                m.job = None;
                continue;
            }
            if m.phoning {
                (m.tx, m.ty) = (PHONE.0, PHONE.1);
                if (m.x - m.tx).hypot(m.y - m.ty) < 6.0 {
                    m.work += dt * speed.min(3.0);
                    if m.work > 2.5 {
                        m.phoning = false;
                        m.work = 0.0;
                        self.phone = None;
                        self.floats.push(Float { x: PHONE.0, y: PHONE.1 - 40.0, t: 1.4, note: Note::Calmed });
                        self.news = Some((News::Calmed, 3.0));
                    }
                }
            } else if m.away <= 0.0 {
                let threshold = if monitoring { 0.05 } else { 0.15 };
                // Sent somewhere, else a fallen server first (to restart), else the worst off.
                let need = |s: &Server| if s.down { 10.0 } else { s.leak + s.bugs as f32 };
                let target = m.job.or_else(|| {
                    (0..self.servers.len())
                        .filter(|&i| self.servers[i].down || self.servers[i].leak > threshold || self.servers[i].bugs > 0)
                        .max_by(|&a, &b| need(&self.servers[a]).total_cmp(&need(&self.servers[b])))
                });
                match target {
                    Some(i) => {
                        let s = &mut self.servers[i];
                        (m.tx, m.ty) = (s.x + 58.0 + if m.intern { 10.0 } else { 0.0 }, s.y - 6.0);
                        if (m.x - m.tx).hypot(m.y - m.ty) < 6.0 {
                            m.work += dt * speed * m.skill;
                            // A fallen server: restarted, slowly, low.
                            if s.down && m.work > 4.0 {
                                m.work = 0.0;
                                m.energy -= 0.1;
                                s.down = false;
                                s.hp = 45.0;
                                s.leak = 0.0;
                                s.bugs = 0;
                                s.flash = 1.0;
                                self.stats.fixed += 1;
                            } else if !s.down && m.work > 1.2 {
                                m.work = 0.0;
                                m.energy -= 0.03;
                                if m.intern && (self.rng >> 7).is_multiple_of(4) {
                                    s.bugs = (s.bugs + 1).min(6);
                                } else if s.leak > 0.05 {
                                    s.leak = (s.leak - 0.25).max(0.0);
                                    self.stats.fixed += 1;
                                } else if s.bugs > 0 {
                                    s.bugs -= 1;
                                    self.stats.fixed += 1;
                                } else {
                                    m.job = None;
                                }
                            }
                        }
                    }
                    None => {
                        m.job = None;
                        (m.tx, m.ty) = (COUCH.0 - 80.0 + (k % 4) as f32 * 40.0, COUCH.1 - 50.0);
                    }
                }
            }
            let drain = if night { 0.0009 } else { 0.0004 } * if nap { 0.7 } else { 1.0 };
            m.energy = (m.energy - gm * drain).max(0.0);
            if m.energy <= 0.0 && m.away <= 0.0 {
                m.asleep = true;
                m.job = None;
                m.phoning = false;
                (m.tx, m.ty) = (m.x, m.y);
                self.news = Some((News::Asleep(k), 3.0));
                if self.tips_seen & Tip::Sleep.bit() == 0 {
                    self.tips_seen |= Tip::Sleep.bit();
                    self.tip = Some(Tip::Sleep);
                }
            }
            walk(m, dt, speed);
        }
    }

    /// What only fades: sparks, floating words, the news band, the "well done".
    fn fade(&mut self, dt: f32) {
        for p in &mut self.sparks {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.vy += 220.0 * dt;
            p.t -= dt;
        }
        self.sparks.retain(|p| p.t > 0.0);
        for f in &mut self.floats {
            f.y -= 22.0 * dt;
            f.t -= dt;
        }
        self.floats.retain(|f| f.t > 0.0);
        if let Some((_, t)) = &mut self.news {
            *t -= dt;
            if *t <= 0.0 {
                self.news = None;
            }
        }
    }

    // ---- The tutorial: the clock stopped, one thing shown at a time, each move done once for real.

    /// The tool the step asks for (None: a step to read, a click goes on).
    pub fn tutorial_tool(step: usize) -> Option<Tool> {
        match step {
            2 => Some(Tool::Hotfix),
            3 => Some(Tool::Oncall),
            4 => Some(Tool::Cache),
            5 => Some(Tool::Coffee),
            _ => None,
        }
    }

    /// What the step shows (a rectangle of the world: x, y, width, height; the whole room for the last).
    pub fn tutorial_focus(&self, step: usize) -> (f32, f32, f32, f32) {
        let around = |r: Role| self.servers.iter().find(|s| s.role == r).map_or((0.0, 0.0, W, H), |s| (s.x - 50.0, s.y - 132.0, 112.0, 176.0));
        match step {
            0 => (150.0, 150.0, 640.0, 350.0),
            1 => (594.0, 24.0, 312.0, 124.0),
            2 => around(Role::Api),
            3 => (300.0, 150.0, 260.0, 220.0),
            4 => (20.0, 150.0, 290.0, 220.0),
            5 => (700.0, 200.0, 250.0, 310.0),
            _ => (0.0, 0.0, W, H),
        }
    }

    fn role(&mut self, r: Role) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.role == r)
    }

    /// The step's move is done.
    fn tutorial_done(&self, step: usize) -> bool {
        let get = |r: Role| self.servers.iter().find(|s| s.role == r);
        match step {
            2 => get(Role::Api).is_some_and(|s| s.leak <= 0.16),
            3 => get(Role::Api).is_some_and(|s| s.bugs == 0) && self.crawlers.is_empty(),
            4 => get(Role::Lb).is_some_and(|s| s.cache > 0.0),
            5 => self.team.iter().all(|m| m.energy > 0.5),
            _ => false,
        }
    }

    /// The step set up: the leak, the bugs, the wave, the tired team.
    fn tutorial_setup(&mut self, step: usize) {
        match step {
            2 => {
                if let Some(s) = self.role(Role::Api) {
                    s.leak = 0.6;
                }
            }
            3 => {
                if let Some(s) = self.role(Role::Api) {
                    s.bugs = 2;
                }
                if let Some(e) = self.edges.iter().position(|e| e.from != Node::Net && self.servers[e.to].role == Role::Api) {
                    self.crawlers.push(Crawler { edge: e, t: 0.2 });
                }
            }
            4 => self.wave(24),
            5 => {
                for m in &mut self.team {
                    m.energy = 0.22;
                }
            }
            _ => {}
        }
    }

    /// A click on a step to read: the next one. True when the tutorial is over (the real game to start).
    pub fn tutorial_next(&mut self) -> bool {
        let Some(step) = self.tutorial else { return false };
        if Self::tutorial_tool(step).is_some() {
            return false;
        }
        self.tutorial_advance()
    }

    fn tutorial_advance(&mut self) -> bool {
        let Some(step) = self.tutorial else { return false };
        if step + 1 >= TUTORIAL_STEPS {
            self.tutorial = None;
            return true;
        }
        self.tutorial = Some(step + 1);
        self.tutorial_setup(step + 1);
        false
    }

    /// In the tutorial nothing breaks: things move, the team does what it's sent to, a move done
    /// goes on to the next step after a "well done".
    fn tutorial_motion(&mut self, dt: f32) {
        let Some(step) = self.tutorial else { return };
        if self.tutorial_wait > 0.0 {
            self.tutorial_wait -= dt;
            if self.tutorial_wait <= 0.0 {
                self.tutorial_advance();
            }
        } else if self.tutorial_done(step) {
            self.tutorial_wait = 1.1;
            let (x, y, w, _) = self.tutorial_focus(step);
            self.float(x + w / 2.0, y + 12.0, Note::WellDone);
        }
        self.crawl(dt * 0.3);
        self.traffic(dt, 1.0);
        for s in &mut self.servers {
            s.flash = (s.flash - dt * 2.0).max(0.0);
        }
        for k in 0..self.team.len() {
            let m = &mut self.team[k];
            if let Some(i) = m.job {
                let s = &mut self.servers[i];
                (m.tx, m.ty) = (s.x + 58.0, s.y - 6.0);
                if (m.x - m.tx).hypot(m.y - m.ty) < 6.0 {
                    m.work += dt;
                    if m.work > 1.2 {
                        m.work = 0.0;
                        if s.leak > 0.05 {
                            s.leak = (s.leak - 0.25).max(0.0);
                        } else if s.bugs > 0 {
                            s.bugs -= 1;
                        } else {
                            m.job = None;
                        }
                    }
                }
            }
            walk(m, dt, 1.0);
        }
    }

    /// The tutorial's step is waiting for its move to be done.
    pub fn tutorial_well_done(&self) -> bool {
        self.tutorial_wait > 0.0
    }
}

/// A step toward where they're going (slower when tired).
fn walk(m: &mut Member, dt: f32, speed: f32) {
    let (dx, dy) = (m.tx - m.x, m.ty - m.y);
    let d = dx.hypot(dy);
    let v = 90.0 * speed.min(3.0) * (0.5 + m.energy * 0.5);
    if d > 1.0 {
        let k = (v * dt / d).min(1.0);
        m.x += dx * k;
        m.y += dy * k;
        m.walking = true;
    } else {
        m.walking = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(g: &mut Game, seconds: f32, speed: f32) {
        for _ in 0..(seconds / DT) as usize {
            g.step(DT, speed);
            // A player who reads the tips and takes the first card.
            if g.tip.is_some() {
                g.dismiss_tip();
            }
            if g.pick.is_some() {
                g.choose(0);
            }
            if g.over.is_some() {
                break;
            }
        }
    }

    #[test]
    fn the_rooms_have_their_servers_and_cables() {
        let small = Game::new(Level::Startup, 1, 0);
        assert_eq!(small.servers.len(), 3);
        // Internet → LB, LB → API, API → DB.
        assert_eq!(small.edges.len(), 3);
        assert!(matches!(small.edges[0].from, Node::Net));
        let big = Game::new(Level::Scaleup, 1, 0);
        assert_eq!((big.servers.len(), big.edges.len()), (5, 6));
    }

    #[test]
    fn the_clock_starts_on_friday_evening_and_ends_on_monday() {
        let mut g = Game::new(Level::Scaleup, 1, 0);
        assert_eq!(g.clock().0, 0);
        assert!((g.clock().1 - 17.7).abs() < 0.01);
        g.min = END - 1.0;
        assert_eq!(g.clock().0, 3);
        assert!(g.clock().1 < 9.0);
    }

    #[test]
    fn a_hotfix_dries_a_leak_and_costs_the_budget() {
        let mut g = Game::new(Level::Scaleup, 1, 0);
        g.cards.push(Card::Docs);
        let api = g.servers.iter().position(|s| s.role == Role::Api).unwrap();
        g.servers[api].leak = 0.5;
        let (x, y) = (g.servers[api].x, g.servers[api].y - 60.0);
        assert!(g.act(Tool::Hotfix, x, y));
        assert!((g.servers[api].leak - 0.05).abs() < 0.001);
        assert_eq!(g.budget, 50.0);
        // Nothing under the click, or too poor: nothing done.
        assert!(!g.act(Tool::Hotfix, 5.0, 5.0));
        g.budget = 5.0;
        assert!(!g.act(Tool::Hotfix, x, y));
    }

    #[test]
    fn a_rollback_raises_a_fallen_server() {
        let mut g = Game::new(Level::Scaleup, 1, 0);
        g.budget = 100.0;
        let s = &mut g.servers[0];
        s.down = true;
        s.hp = 0.0;
        s.bugs = 3;
        let (x, y) = (g.servers[0].x, g.servers[0].y - 60.0);
        assert!(g.act(Tool::Rollback, x, y));
        assert!(!g.servers[0].down && g.servers[0].hp >= 60.0 && g.servers[0].bugs == 0);
    }

    #[test]
    fn a_cache_sends_the_snail_away() {
        let mut g = Game::new(Level::Scaleup, 1, 0);
        let db = g.servers.iter().position(|s| s.role == Role::Db).unwrap();
        g.snail = g.edges.iter().position(|e| e.to == db);
        assert!(g.snail.is_some());
        let (x, y) = (g.servers[db].x, g.servers[db].y - 60.0);
        assert!(g.act(Tool::Cache, x, y));
        assert!(g.snail.is_none());
    }

    #[test]
    fn coffee_wakes_the_team() {
        let mut g = Game::new(Level::Scaleup, 1, 0);
        for m in &mut g.team {
            m.energy = 0.0;
            m.asleep = true;
        }
        assert!(g.act(Tool::Coffee, 10.0, 10.0));
        assert!(g.team.iter().all(|m| !m.asleep && m.energy >= 0.4));
    }

    #[test]
    fn the_first_peak_is_explained_and_waits() {
        // Every other tip already read: the peak is the one.
        let mut g = Game::new(Level::Scaleup, 3, u32::MAX & !Tip::Peak.bit());
        while g.tip.is_none() && g.min < 200.0 {
            g.step(DT, 4.0);
        }
        assert_eq!(g.tip, Some(Tip::Peak));
        let min = g.min;
        g.step(DT, 4.0);
        assert_eq!(g.min, min, "the clock waits for the tip to be read");
        g.dismiss_tip();
        g.step(DT, 4.0);
        assert!(g.min > min);
        // Read once: not again.
        assert_ne!(g.tips_seen & Tip::Peak.bit(), 0);
    }

    #[test]
    fn the_weekend_pauses_for_cards() {
        let mut g = Game::new(Level::Scaleup, 5, u32::MAX);
        g.min = PICKS[0] - 0.01;
        g.step(DT, 1.0);
        let cards = g.pick.expect("three cards");
        assert!(cards[0] != cards[1] && cards[1] != cards[2] && cards[0] != cards[2]);
        g.choose(1);
        assert_eq!(g.cards, vec![cards[1]]);
        assert!(g.pick.is_none());
    }

    #[test]
    fn cards_taken_once_dont_come_back() {
        let mut g = Game::new(Level::Scaleup, 9, u32::MAX);
        g.cards = vec![Card::Rust, Card::Senior, Card::Autoscale, Card::Monitoring, Card::Firewall, Card::Docs, Card::Nap, Card::Intern, Card::Espresso];
        for _ in 0..50 {
            let cards = g.draw_cards();
            assert!(cards.iter().all(|c| c.repeats()), "{cards:?}");
        }
    }

    #[test]
    fn the_rust_card_keeps_a_server_up_and_sends_tom_away() {
        let mut g = Game::new(Level::Scaleup, 1, 0);
        g.servers[2].hp = 10.0;
        g.pick = Some([Card::Rust, Card::Pizza, Card::Funding]);
        g.choose(0);
        assert!(g.servers[2].rust);
        assert!(g.team.iter().find(|m| m.name == "Tom").unwrap().away > 0.0);
        g.servers[2].leak = 1.0;
        g.servers[2].bugs = 6;
        g.servers[2].hp = 2.0;
        run(&mut g, 2.0, 4.0);
        assert!(!g.servers[2].down);
    }

    #[test]
    fn with_nobody_minding_it_production_falls_and_the_player_is_called_in() {
        let mut g = Game::new(Level::BlackFriday, 11, u32::MAX);
        // The team asleep for good, no budget spent.
        for m in &mut g.team {
            m.away = f32::MAX;
        }
        run(&mut g, 600.0, 4.0);
        assert_eq!(g.over, Some(false));
        assert!(g.stats.downs > 0);
    }

    #[test]
    fn a_weekend_ends_on_monday_morning() {
        let mut g = Game::new(Level::Startup, 2, u32::MAX);
        // Minded well: the servers kept up by hand.
        for _ in 0..(700.0 / DT) as usize {
            g.step(DT, 8.0);
            for s in &mut g.servers {
                s.hp = 100.0;
                s.down = false;
            }
            g.sat = g.sat.max(50.0);
            if g.pick.is_some() {
                g.choose(0);
            }
            if g.over.is_some() {
                break;
            }
        }
        assert_eq!(g.over, Some(true));
        assert_eq!(g.picks_done, PICKS.len());
        assert!(g.score() > 4000);
    }

    #[test]
    fn the_tutorial_goes_step_by_step() {
        let mut g = Game::tutorial(1);
        // Two steps to read.
        assert!(!g.tutorial_next());
        assert!(!g.tutorial_next());
        assert_eq!(g.tutorial, Some(2));
        // A step asking for a move doesn't go on with a click.
        assert!(!g.tutorial_next());
        assert_eq!(g.tutorial, Some(2));
        let api = g.servers.iter().position(|s| s.role == Role::Api).unwrap();
        let (x, y) = (g.servers[api].x, g.servers[api].y - 60.0);
        assert!(g.servers[api].leak > 0.5);
        g.act(Tool::Hotfix, x, y);
        assert_eq!(g.tutorial, Some(2), "one hotfix isn't enough");
        g.act(Tool::Hotfix, x, y);
        // "Well done", then the next step.
        for _ in 0..120 {
            g.step(DT, 1.0);
        }
        assert_eq!(g.tutorial, Some(3));
        // Someone on call on the API: the bugs gone, on to the wave.
        g.act(Tool::Oncall, x, y);
        for _ in 0..(20.0 / DT) as usize {
            g.step(DT, 1.0);
        }
        assert_eq!(g.tutorial, Some(4));
        let lb = &g.servers[0];
        let (lx, ly) = (lb.x, lb.y - 60.0);
        g.act(Tool::Cache, lx, ly);
        for _ in 0..120 {
            g.step(DT, 1.0);
        }
        assert_eq!(g.tutorial, Some(5));
        g.act(Tool::Coffee, 500.0, 400.0);
        for _ in 0..120 {
            g.step(DT, 1.0);
        }
        assert_eq!(g.tutorial, Some(6));
        // The last one read: over, the real game to start; nothing broke meanwhile.
        assert!(g.tutorial_next());
        assert!(g.servers.iter().all(|s| !s.down && s.hp == 100.0));
        assert_eq!(g.min, 0.0);
    }

    /// A simple player, a decision a second: rollback what fell, hotfix the worst leak, a cache on the
    /// load balancer before the peaks, coffee when the team is spent. .
    fn bot(level: Level, seed: u64, lazy: bool) -> Game {
        let mut g = Game::new(level, seed, u32::MAX);
        let mut clock = 0.0;
        while g.over.is_none() {
            g.step(DT, 1.0);
            if g.tip.is_some() {
                g.dismiss_tip();
            }
            if g.pick.is_some() {
                g.choose(0);
            }
            if std::env::var("FRIDAY_TRACE").is_ok_and(|v| v == format!("{level:?}{seed}{lazy}")) && (g.min / 60.0).fract() < 0.0045 {
                let hp: Vec<String> = g.servers.iter().map(|s| format!("{:.0}{}l{:.1}b{}", s.hp, if s.down { "X" } else { "" }, s.leak, s.bugs)).collect();
                let en: Vec<String> = g.team.iter().map(|m| format!("{:.1}{}", m.energy, if m.asleep { "z" } else if m.phoning { "p" } else { "" })).collect();
                eprintln!("{:>5.0} sat {:>3.0} budget {:>3.0} | {} | {} | crawl {} snail {}", g.min, g.sat, g.budget, hp.join(" "), en.join(" "), g.crawlers.len(), g.snail.is_some());
            }
            clock += DT;
            if clock < if lazy { 3.0 } else { 1.0 } {
                continue;
            }
            clock = 0.0;
            let at = |s: &Server| (s.x, s.y - 60.0);
            if let Some(s) = g.servers.iter().find(|s| s.down) {
                let (x, y) = at(s);
                g.act(Tool::Rollback, x, y);
            } else if g.team.iter().map(|m| m.energy).sum::<f32>() / (g.team.len() as f32) < 0.25 {
                g.act(Tool::Coffee, 10.0, 10.0);
            } else if let Some(s) = g.servers.iter().filter(|s| s.troubled()).max_by(|a, b| (a.leak + a.bugs as f32 * 0.3).total_cmp(&(b.leak + b.bugs as f32 * 0.3))) {
                let (x, y) = at(s);
                g.act(Tool::Hotfix, x, y);
            }
        }
        g
    }

    #[test]
    #[ignore = "prints the balance"]
    fn balance() {
        for level in Level::ALL {
            for lazy in [false, true] {
                let games: Vec<Game> = (1..=8).map(|seed| bot(level, seed, lazy)).collect();
                let survived = games.iter().filter(|g| g.over == Some(true)).count();
                let sat: f32 = games.iter().map(|g| g.sat).sum::<f32>() / 8.0;
                let downs: u32 = games.iter().map(|g| g.stats.downs).sum();
                let score: u32 = games.iter().map(Game::score).sum::<u32>() / 8;
                let reached: f32 = games.iter().map(|g| g.min / END).sum::<f32>() / 8.0;
                println!("{level:?} lazy={lazy}: survived {survived}/8, sat {sat:.0}, downs {downs}, reached {:.0} %, score {score}", reached * 100.0);
            }
        }
    }
}
