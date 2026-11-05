//! Bug Hunt's world: bugs come down in ranks toward production, marching from side to side and a row
//! lower at each edge, faster as they thin out; they drop exceptions, and from the third wave some
//! leave the ranks to dive at the debugger. The debugger, at the bottom, moves and fires patches; four
//! firewalls shelter it from the exceptions and wear away under them (its own patches go out through
//! them). A bug squashed now and then drops
//! a fix: a triple shot, a rapid fire, a shield. Every fifth wave, a boss alone: the Heisenbug, big,
//! sweeping the top, firing in fans, vanishing to show up elsewhere and sending moths to dive. Units
//! are the field's (y down); the page scales them.

/// The field.
pub const W: f32 = 640.0;
pub const H: f32 = 720.0;
/// The debugger's middle, its half-width.
pub const PLAYER_Y: f32 = 672.0;
pub const PLAYER_HALF: f32 = 22.0;
const PLAYER_SPEED: f32 = 400.0;
const SHOT_SPEED: f32 = 780.0;
const BOLT_SPEED: f32 = 270.0;
const DROP_SPEED: f32 = 150.0;

/// The ranks: their columns, their rows, the room between two bugs.
pub const COLS: usize = 10;
pub const ROWS: usize = 5;
pub const CELL_W: f32 = 48.0;
pub const CELL_H: f32 = 40.0;
/// A bug's half-size.
pub const BUG_HALF: (f32, f32) = (16.0, 13.0);
/// How far the ranks come down at an edge.
const STEP_DOWN: f32 = 10.0;
const MARGIN: f32 = 14.0;

/// A firewall: its cells across, down, and their size.
pub const WALL_COLS: usize = 11;
pub const WALL_ROWS: usize = 6;
pub const WALL_CELL: f32 = 7.0;
/// The firewalls' top.
pub const WALL_Y: f32 = 578.0;

/// How long a fix lasts, in seconds.
const POWER_SECS: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The bottom rows: 10 points.
    Moth,
    /// The middle ones: 20.
    Beetle,
    /// The top one: 30.
    Spider,
}

impl Kind {
    pub fn points(self) -> u32 {
        match self {
            Kind::Moth => 10,
            Kind::Beetle => 20,
            Kind::Spider => 30,
        }
    }

    fn of_row(row: usize) -> Self {
        match row {
            0 => Kind::Spider,
            1 | 2 => Kind::Beetle,
            _ => Kind::Moth,
        }
    }
}

/// A bug out of the ranks: down toward where the debugger was, swaying, then back to its place from
/// the top.
#[derive(Clone, Copy, Debug)]
pub struct Dive {
    t: f32,
    from: (f32, f32),
    target: f32,
    /// Gone off the bottom: coming back to its place from above.
    pub back: bool,
    fired: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Bug {
    pub kind: Kind,
    pub col: usize,
    pub row: usize,
    pub alive: bool,
    /// Its middle.
    pub x: f32,
    pub y: f32,
    pub dive: Option<Dive>,
    /// Sent by the boss: diving once, gone off the bottom (no place in the ranks).
    pub minion: bool,
}

/// Every fifth wave, the Heisenbug: across the top, its fans of exceptions, now and then gone and back
/// elsewhere, moths sent to dive.
#[derive(Clone, Copy, Debug)]
pub struct Boss {
    pub x: f32,
    pub y: f32,
    pub hp: u32,
    pub max_hp: u32,
    /// Which boss (1 at wave 5, 2 at wave 10...).
    pub level: u32,
    vx: f32,
    t: f32,
    fire_in: f32,
    blink_in: f32,
    spawn_in: f32,
    /// Seconds left of its vanishing (patches go through it), from `BLINK`.
    pub blink: f32,
    /// Where it was before vanishing.
    pub from_x: f32,
    /// Seconds left of its flash, just hit.
    pub flash: f32,
}

/// The boss's half-size, how long it vanishes.
pub const BOSS_HALF: (f32, f32) = (54.0, 36.0);
pub const BLINK: f32 = 0.45;

/// A patch fired, going up.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub x: f32,
    pub y: f32,
    vx: f32,
}

/// An exception dropped by a bug, going down.
#[derive(Clone, Copy, Debug)]
pub struct Bolt {
    pub x: f32,
    pub y: f32,
    vx: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Power {
    Triple,
    Rapid,
    Shield,
}

/// A fix falling, to catch.
#[derive(Clone, Copy, Debug)]
pub struct Drop {
    pub x: f32,
    pub y: f32,
    pub power: Power,
}

/// A bit flying off something hit; `kind`: what it was (the page colors it), None: the debugger.
#[derive(Clone, Copy, Debug)]
pub struct Spark {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    /// Seconds left, out of `SPARK_LIFE`.
    pub life: f32,
    pub kind: Option<Kind>,
}

pub const SPARK_LIFE: f32 = 0.6;

#[derive(Clone, Debug)]
pub struct Wall {
    /// Its left, its top.
    pub x: f32,
    pub y: f32,
    pub cells: [[bool; WALL_COLS]; WALL_ROWS],
}

impl Wall {
    fn new(center: f32) -> Self {
        let mut cells = [[true; WALL_COLS]; WALL_ROWS];
        // Its top corners rounded, an arch under it.
        for (r, c) in [(0, 0), (0, 1), (1, 0), (0, WALL_COLS - 1), (0, WALL_COLS - 2), (1, WALL_COLS - 1)] {
            cells[r][c] = false;
        }
        for row in cells.iter_mut().skip(WALL_ROWS - 2) {
            for cell in row.iter_mut().take(7).skip(4) {
                *cell = false;
            }
        }
        Self { x: center - WALL_COLS as f32 * WALL_CELL / 2.0, y: WALL_Y, cells }
    }

    /// The cell at a point, if standing.
    fn cell_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        if x < self.x || y < self.y {
            return None;
        }
        let (c, r) = (((x - self.x) / WALL_CELL) as usize, ((y - self.y) / WALL_CELL) as usize);
        (r < WALL_ROWS && c < WALL_COLS && self.cells[r][c]).then_some((r, c))
    }

    fn rect(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.x + WALL_COLS as f32 * WALL_CELL, self.y + WALL_ROWS as f32 * WALL_CELL)
    }
}

/// The keys this step.
#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    pub left: bool,
    pub right: bool,
    pub fire: bool,
}

#[derive(Clone, Debug)]
pub struct Game {
    /// The debugger's middle.
    pub x: f32,
    pub lives: u32,
    pub score: u32,
    pub wave: u32,
    pub bugs: Vec<Bug>,
    /// The ranks' first place (column 0, row 0), their way (1: right).
    fx: f32,
    fy: f32,
    dir: f32,
    pub shots: Vec<Shot>,
    pub bolts: Vec<Bolt>,
    pub drops: Vec<Drop>,
    pub sparks: Vec<Spark>,
    pub walls: Vec<Wall>,
    cooldown: f32,
    /// Seconds left untouchable (just hit), of triple shot, of rapid fire.
    pub invincible: f32,
    pub triple: f32,
    pub rapid: f32,
    /// A shield taken: the next hit is on it.
    pub shield: bool,
    /// Seconds left of the wave's title (the bugs wait).
    pub banner: f32,
    fire_in: f32,
    dive_in: f32,
    /// How far the bugs' legs and wings are in their beat.
    pub beat: f32,
    pub kills: u32,
    pub boss: Option<Boss>,
    pub over: bool,
    /// Over because the ranks reached production (else: no lives left).
    pub invaded: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            x: W / 2.0,
            lives: 3,
            score: 0,
            wave: 0,
            bugs: Vec::new(),
            fx: 0.0,
            fy: 0.0,
            dir: 1.0,
            shots: Vec::new(),
            bolts: Vec::new(),
            drops: Vec::new(),
            sparks: Vec::new(),
            walls: Vec::new(),
            cooldown: 0.0,
            invincible: 0.0,
            triple: 0.0,
            rapid: 0.0,
            shield: false,
            banner: 0.0,
            fire_in: 0.0,
            dive_in: 0.0,
            beat: 0.0,
            kills: 0,
            boss: None,
            over: false,
            invaded: false,
            rng: seed | 1,
        };
        game.next_wave();
        game
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.random() * n as f32) as usize).min(n.saturating_sub(1))
    }

    /// The next wave: full ranks a bit lower each time, the firewalls rebuilt, a life more every third.
    fn next_wave(&mut self) {
        self.wave += 1;
        if self.wave > 1 && self.wave % 3 == 1 {
            self.lives = (self.lives + 1).min(5);
        }
        self.fx = (W - (COLS - 1) as f32 * CELL_W) / 2.0;
        self.fy = 86.0 + (self.wave - 1).min(5) as f32 * 8.0;
        self.dir = 1.0;
        if self.wave.is_multiple_of(5) {
            // The boss, alone.
            let level = self.wave / 5;
            let hp = 40 + 15 * (level - 1);
            self.bugs.clear();
            self.boss = Some(Boss { x: W / 2.0, y: 150.0, hp, max_hp: hp, level, vx: 110.0 + 15.0 * level as f32, t: 0.0, fire_in: 1.5, blink_in: 5.0, spawn_in: 3.0, blink: 0.0, from_x: W / 2.0, flash: 0.0 });
        } else {
            self.bugs = (0..ROWS).flat_map(|row| (0..COLS).map(move |col| Bug { kind: Kind::of_row(row), col, row, alive: true, x: 0.0, y: 0.0, dive: None, minion: false })).collect();
        }
        self.place_ranks();
        self.walls = (0..4).map(|k| Wall::new(W * (k as f32 + 1.0) / 5.0)).collect();
        self.shots.clear();
        self.bolts.clear();
        self.drops.clear();
        self.banner = 1.6;
        self.fire_in = 1.2;
        self.dive_in = 3.0;
    }

    fn place_ranks(&mut self) {
        for bug in self.bugs.iter_mut().filter(|b| b.dive.is_none()) {
            bug.x = self.fx + bug.col as f32 * CELL_W;
            bug.y = self.fy + bug.row as f32 * CELL_H;
        }
    }

    fn alive(&self) -> usize {
        self.bugs.iter().filter(|b| b.alive).count()
    }

    /// The ranks' speed: faster with each wave, and much faster as they thin out.
    fn march_speed(&self) -> f32 {
        let left = self.alive() as f32 / (COLS * ROWS) as f32;
        (24.0 + 5.0 * (self.wave - 1).min(8) as f32) * (1.0 + 3.0 * (1.0 - left).powi(2))
    }

    /// `dt` seconds more.
    pub fn step(&mut self, dt: f32, input: Input) {
        if self.over {
            self.sparks_step(dt);
            return;
        }
        self.beat += dt * (1.5 + self.march_speed() / 40.0);
        let way = f32::from(u8::from(input.right)) - f32::from(u8::from(input.left));
        self.x = (self.x + way * PLAYER_SPEED * dt).clamp(PLAYER_HALF + 4.0, W - PLAYER_HALF - 4.0);
        for t in [&mut self.cooldown, &mut self.invincible, &mut self.triple, &mut self.rapid] {
            *t = (*t - dt).max(0.0);
        }
        self.fire(input.fire);
        self.shots_step(dt);
        self.sparks_step(dt);
        self.drops_step(dt);
        if self.banner > 0.0 {
            self.banner -= dt;
            return;
        }
        self.march(dt);
        self.dives(dt);
        self.bug_fire(dt);
        self.boss_step(dt);
        self.bolts_step(dt);
        self.collide();
        if !self.over && self.alive() == 0 && self.boss.is_none() {
            self.next_wave();
        }
    }

    fn fire(&mut self, fire: bool) {
        let limit = if self.triple > 0.0 || self.rapid > 0.0 { 9 } else { 2 };
        if !fire || self.cooldown > 0.0 || self.shots.len() >= limit {
            return;
        }
        let y = PLAYER_Y - 18.0;
        if self.triple > 0.0 {
            self.shots.extend([-150.0, 0.0, 150.0].map(|vx| Shot { x: self.x, y, vx }));
        } else {
            self.shots.push(Shot { x: self.x, y, vx: 0.0 });
        }
        self.cooldown = if self.rapid > 0.0 { 0.13 } else { 0.3 };
    }

    fn shots_step(&mut self, dt: f32) {
        for s in &mut self.shots {
            s.y -= SHOT_SPEED * dt;
            s.x += s.vx * dt;
        }
        self.shots.retain(|s| s.y > -20.0 && (-20.0..W + 20.0).contains(&s.x));
    }

    fn bolts_step(&mut self, dt: f32) {
        let speed = BOLT_SPEED + 8.0 * (self.wave - 1).min(10) as f32;
        for b in &mut self.bolts {
            b.y += speed * dt;
            b.x += b.vx * dt;
        }
        self.bolts.retain(|b| b.y < H + 20.0);
    }

    fn drops_step(&mut self, dt: f32) {
        for d in &mut self.drops {
            d.y += DROP_SPEED * dt;
        }
        let x = self.x;
        let (caught, falling): (Vec<Drop>, Vec<Drop>) = self.drops.drain(..).filter(|d| d.y < H + 20.0).partition(|d| (d.x - x).abs() < PLAYER_HALF + 10.0 && (d.y - PLAYER_Y).abs() < 18.0);
        self.drops = falling;
        for d in caught {
            match d.power {
                Power::Triple => self.triple = POWER_SECS,
                Power::Rapid => self.rapid = POWER_SECS,
                Power::Shield => self.shield = true,
            }
            self.score += 50;
        }
    }

    fn sparks_step(&mut self, dt: f32) {
        for s in &mut self.sparks {
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            s.vy += 300.0 * dt;
            s.life -= dt;
        }
        self.sparks.retain(|s| s.life > 0.0);
    }

    fn burst(&mut self, x: f32, y: f32, kind: Option<Kind>, n: usize) {
        for _ in 0..n {
            let a = self.random() * std::f32::consts::TAU;
            let v = 60.0 + self.random() * 160.0;
            let life = SPARK_LIFE * (0.5 + self.random() * 0.5);
            self.sparks.push(Spark { x, y, vx: a.cos() * v, vy: a.sin() * v - 40.0, life, kind });
        }
    }

    /// The ranks from side to side, a row down and back at an edge.
    fn march(&mut self, dt: f32) {
        self.fx += self.dir * self.march_speed() * dt;
        let cols = self.bugs.iter().filter(|b| b.alive).map(|b| b.col);
        let (lo, hi) = cols.fold((usize::MAX, 0), |(lo, hi), c| (lo.min(c), hi.max(c)));
        if lo == usize::MAX {
            return;
        }
        let left = self.fx + lo as f32 * CELL_W - BUG_HALF.0;
        let right = self.fx + hi as f32 * CELL_W + BUG_HALF.0;
        if self.dir > 0.0 && right > W - MARGIN {
            self.fx -= right - (W - MARGIN);
            self.dir = -1.0;
            self.fy += STEP_DOWN;
        } else if self.dir < 0.0 && left < MARGIN {
            self.fx += MARGIN - left;
            self.dir = 1.0;
            self.fy += STEP_DOWN;
        }
        self.place_ranks();
    }

    /// From the third wave, a bug leaves the ranks now and then (two at most) and dives.
    fn dives(&mut self, dt: f32) {
        let player = self.x;
        for k in 0..self.bugs.len() {
            let Some(mut d) = self.bugs[k].dive else { continue };
            d.t += dt;
            let bug = self.bugs[k];
            let (x, y) = if d.back {
                // Back to its place, from above.
                let home = (self.fx + bug.col as f32 * CELL_W, self.fy + bug.row as f32 * CELL_H);
                let (dx, dy) = (home.0 - bug.x, home.1 - bug.y);
                let dist = (dx * dx + dy * dy).sqrt();
                let step = 320.0 * dt;
                if dist <= step {
                    self.bugs[k].dive = None;
                    (home.0, home.1)
                } else {
                    self.bugs[k].dive = Some(d);
                    (bug.x + dx / dist * step, bug.y + dy / dist * step)
                }
            } else {
                let speed = 210.0 + 10.0 * self.wave.min(10) as f32;
                let along = (d.t / 1.8).min(1.0);
                let x = d.from.0 + (d.target - d.from.0) * along + (d.t * 3.2).sin() * 70.0;
                let y = d.from.1 + speed * d.t - 40.0 * (d.t * 4.0).min(std::f32::consts::PI).sin();
                if y > H + 30.0 && bug.minion {
                    self.bugs[k].alive = false;
                    self.bugs[k].dive = None;
                    (x, y)
                } else if y > H + 30.0 {
                    d.back = true;
                    self.bugs[k].dive = Some(d);
                    (x.clamp(20.0, W - 20.0), -30.0)
                } else {
                    // An exception on the way, once, from halfway.
                    if !d.fired && y > 300.0 {
                        d.fired = true;
                        let vx = ((player - x) * 0.4).clamp(-90.0, 90.0);
                        self.bolts.push(Bolt { x, y: y + 12.0, vx });
                    }
                    self.bugs[k].dive = Some(d);
                    (x, y)
                }
            };
            self.bugs[k].x = x;
            self.bugs[k].y = y;
        }
        if self.wave < 3 {
            return;
        }
        self.dive_in -= dt;
        if self.dive_in > 0.0 {
            return;
        }
        self.dive_in = (6.5 - 0.4 * self.wave as f32).max(2.2) + self.random() * 2.0;
        if self.bugs.iter().filter(|b| b.dive.is_some()).count() >= 2 {
            return;
        }
        // The bottom bug of a column.
        let pick: Vec<usize> = (0..COLS).filter_map(|c| self.bottom_of(c)).collect();
        if pick.is_empty() {
            return;
        }
        let k = pick[self.below(pick.len())];
        let bug = self.bugs[k];
        self.bugs[k].dive = Some(Dive { t: 0.0, from: (bug.x, bug.y), target: self.x, back: false, fired: false });
    }

    /// The lowest bug in the ranks of column `c`.
    fn bottom_of(&self, c: usize) -> Option<usize> {
        (0..self.bugs.len()).filter(|&k| self.bugs[k].alive && self.bugs[k].dive.is_none() && self.bugs[k].col == c).max_by_key(|&k| self.bugs[k].row)
    }

    /// An exception from the bottom of a column now and then, more often each wave.
    fn bug_fire(&mut self, dt: f32) {
        self.fire_in -= dt;
        if self.fire_in > 0.0 {
            return;
        }
        self.fire_in = (1.5 - 0.08 * self.wave as f32).max(0.5) * (0.5 + self.random());
        let cols: Vec<usize> = (0..COLS).filter_map(|c| self.bottom_of(c)).collect();
        if cols.is_empty() {
            return;
        }
        // From the third wave, the column over the debugger fires more often.
        let over = cols.iter().copied().min_by_key(|&k| (self.bugs[k].x - self.x).abs() as i32);
        let k = match over {
            Some(k) if self.wave >= 3 && self.random() < 0.4 => k,
            _ => cols[self.below(cols.len())],
        };
        let bug = self.bugs[k];
        self.bolts.push(Bolt { x: bug.x, y: bug.y + BUG_HALF.1, vx: 0.0 });
    }

    fn collide(&mut self) {
        // The patches: on a bug, an exception (they go out through the firewalls).
        let mut k = 0;
        while k < self.shots.len() {
            let s = self.shots[k];
            if let Some(b) = self.bugs.iter().position(|b| b.alive && (b.x - s.x).abs() < BUG_HALF.0 && (b.y - s.y).abs() < BUG_HALF.1 + 6.0) {
                self.shots.swap_remove(k);
                self.squash(b);
                continue;
            }
            if let Some(boss) = self.boss.as_mut().filter(|b| b.blink <= 0.0 && (b.x - s.x).abs() < BOSS_HALF.0 && (b.y - s.y).abs() < BOSS_HALF.1) {
                boss.hp = boss.hp.saturating_sub(1);
                boss.flash = 0.08;
                let (bx, by, dead) = (s.x, s.y, boss.hp == 0);
                self.shots.swap_remove(k);
                self.burst(bx, by, Some(Kind::Spider), 2);
                if dead {
                    self.boss_down();
                }
                continue;
            }
            if let Some(b) = self.bolts.iter().position(|b| (b.x - s.x).abs() < 6.0 && (b.y - s.y).abs() < 12.0) {
                self.shots.swap_remove(k);
                self.bolts.swap_remove(b);
                self.burst(s.x, s.y, None, 4);
                continue;
            }
            k += 1;
        }
        // The exceptions: on a firewall, on the debugger.
        let mut k = 0;
        while k < self.bolts.len() {
            let b = self.bolts[k];
            if self.erode(b.x, b.y + 8.0, 1) {
                self.bolts.swap_remove(k);
                continue;
            }
            if (b.x - self.x).abs() < PLAYER_HALF - 3.0 && (b.y + 8.0 - PLAYER_Y).abs() < 12.0 {
                self.bolts.swap_remove(k);
                self.hit();
                continue;
            }
            k += 1;
        }
        // The bugs: through the firewalls, into the debugger, down to production.
        for k in 0..self.bugs.len() {
            let b = self.bugs[k];
            if !b.alive {
                continue;
            }
            for wall in &mut self.walls {
                let (x0, y0, x1, y1) = wall.rect();
                if b.x + BUG_HALF.0 < x0 || b.x - BUG_HALF.0 > x1 || b.y + BUG_HALF.1 < y0 || b.y - BUG_HALF.1 > y1 {
                    continue;
                }
                for (r, row) in wall.cells.iter_mut().enumerate() {
                    for (c, cell) in row.iter_mut().enumerate() {
                        let (cx, cy) = (wall.x + (c as f32 + 0.5) * WALL_CELL, wall.y + (r as f32 + 0.5) * WALL_CELL);
                        if (cx - b.x).abs() < BUG_HALF.0 && (cy - b.y).abs() < BUG_HALF.1 {
                            *cell = false;
                        }
                    }
                }
            }
            if (b.x - self.x).abs() < BUG_HALF.0 + PLAYER_HALF - 6.0 && (b.y - PLAYER_Y).abs() < BUG_HALF.1 + 8.0 {
                self.bugs[k].alive = false;
                self.burst(b.x, b.y, Some(b.kind), 12);
                self.hit();
            } else if b.dive.is_none() && b.y + BUG_HALF.1 >= PLAYER_Y - 14.0 {
                // The ranks reached production.
                self.lives = 0;
                self.over = true;
                self.invaded = true;
                self.burst(self.x, PLAYER_Y, None, 30);
                return;
            }
        }
    }

    /// The boss across the top, swaying; its fans of exceptions, its vanishing, its moths.
    fn boss_step(&mut self, dt: f32) {
        let Some(mut b) = self.boss else { return };
        b.t += dt;
        b.flash = (b.flash - dt).max(0.0);
        b.blink = (b.blink - dt).max(0.0);
        // Faster as it weakens.
        let rage = 1.0 + (1.0 - b.hp as f32 / b.max_hp as f32) * 0.8;
        b.x += b.vx * rage * dt;
        let edge = BOSS_HALF.0 + 10.0;
        if (b.x < edge && b.vx < 0.0) || (b.x > W - edge && b.vx > 0.0) {
            b.vx = -b.vx;
            b.x = b.x.clamp(edge, W - edge);
        }
        b.y = 150.0 + (b.t * 0.9).sin() * 26.0;
        b.fire_in -= dt * rage;
        if b.fire_in <= 0.0 {
            b.fire_in = (1.7 - 0.12 * b.level as f32).max(0.9);
            let n = 3 + b.level.min(3) as usize;
            for k in 0..n {
                let vx = (k as f32 - (n - 1) as f32 / 2.0) * 70.0;
                self.bolts.push(Bolt { x: b.x, y: b.y + BOSS_HALF.1 - 6.0, vx });
            }
        }
        b.blink_in -= dt;
        if b.blink_in <= 0.0 {
            b.blink_in = 4.0 + self.random() * 3.0;
            b.blink = BLINK;
            b.from_x = b.x;
            b.x = edge + self.random() * (W - 2.0 * edge);
        }
        b.spawn_in -= dt;
        if b.spawn_in <= 0.0 {
            b.spawn_in = (4.0 - 0.3 * b.level as f32).max(2.2);
            for side in [-1.0, 1.0] {
                let (x, y) = (b.x + side * 30.0, b.y + 20.0);
                let dive = Dive { t: 0.0, from: (x, y), target: self.x + side * 40.0, back: false, fired: false };
                self.bugs.push(Bug { kind: Kind::Moth, col: 0, row: 0, alive: true, x, y, dive: Some(dive), minion: true });
            }
        }
        self.boss = Some(b);
    }

    /// The boss down: its points, its moths squashed with it, a fix for sure.
    fn boss_down(&mut self) {
        let Some(b) = self.boss.take() else { return };
        self.score += 500 * b.level;
        self.kills += 1;
        for _ in 0..4 {
            let (dx, dy) = ((self.random() - 0.5) * 80.0, (self.random() - 0.5) * 50.0);
            self.burst(b.x + dx, b.y + dy, Some(Kind::Spider), 14);
        }
        let minions: Vec<(f32, f32)> = self.bugs.iter().filter(|m| m.alive).map(|m| (m.x, m.y)).collect();
        for (x, y) in minions {
            self.burst(x, y, Some(Kind::Moth), 6);
        }
        self.bugs.clear();
        let power = [Power::Triple, Power::Rapid, Power::Shield][self.below(3)];
        self.drops.push(Drop { x: b.x, y: b.y, power });
    }

    /// Bug `k` squashed: its points (twice as many diving), its sparks, maybe a fix.
    fn squash(&mut self, k: usize) {
        let b = self.bugs[k];
        self.bugs[k].alive = false;
        self.bugs[k].dive = None;
        self.kills += 1;
        self.score += b.kind.points() * if b.dive.is_some() { 2 } else { 1 };
        self.burst(b.x, b.y, Some(b.kind), 10);
        if self.random() < 0.07 {
            let power = [Power::Triple, Power::Rapid, Power::Shield][self.below(3)];
            self.drops.push(Drop { x: b.x, y: b.y, power });
        }
    }

    /// A firewall cell at a point knocked out, with the next one the shot goes into (`way`: -1 up, 1
    /// down). Whether there was one.
    fn erode(&mut self, x: f32, y: f32, way: i32) -> bool {
        let spread = self.below(3) as i32 - 1;
        for wall in &mut self.walls {
            let Some((r, c)) = wall.cell_at(x, y) else { continue };
            wall.cells[r][c] = false;
            let (r2, c2) = (r as i32 + way, c as i32 + spread);
            if (0..WALL_ROWS as i32).contains(&r2) && (0..WALL_COLS as i32).contains(&c2) {
                wall.cells[r2 as usize][c2 as usize] = false;
            }
            return true;
        }
        false
    }

    /// The debugger hit: on its shield if it has one, else a life lost (none left: over).
    fn hit(&mut self) {
        if self.invincible > 0.0 {
            return;
        }
        self.burst(self.x, PLAYER_Y, None, 18);
        if self.shield {
            self.shield = false;
            self.invincible = 1.0;
            return;
        }
        self.lives = self.lives.saturating_sub(1);
        self.triple = 0.0;
        self.rapid = 0.0;
        if self.lives == 0 {
            self.over = true;
            return;
        }
        self.invincible = 2.2;
        self.bolts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    fn idle() -> Input {
        Input::default()
    }

    #[test]
    fn a_patch_squashes_a_bug_for_its_points() {
        let mut g = Game::new(5);
        g.banner = 0.0;
        g.fire_in = 100.0;
        let target = g.bugs.iter().position(|b| b.row == ROWS - 1 && b.col == 4).unwrap();
        g.x = g.bugs[target].x;
        g.step(DT, Input { fire: true, ..idle() });
        for _ in 0..120 {
            g.step(DT, idle());
        }
        assert!(!g.bugs[target].alive);
        assert!(g.score >= Kind::Moth.points());
    }

    #[test]
    fn the_ranks_turn_and_come_down_at_an_edge() {
        let mut g = Game::new(9);
        g.banner = 0.0;
        g.fire_in = 1000.0;
        let y = g.fy;
        for _ in 0..120 * 20 {
            g.march(DT);
            if g.dir < 0.0 {
                break;
            }
        }
        assert_eq!(g.dir, -1.0);
        assert_eq!(g.fy, y + STEP_DOWN);
        let right = g.bugs.iter().map(|b| b.x).fold(0.0, f32::max) + BUG_HALF.0;
        assert!(right <= W - MARGIN + 0.01);
    }

    #[test]
    fn an_exception_costs_a_life_then_a_moment_untouchable() {
        let mut g = Game::new(3);
        g.bolts.push(Bolt { x: g.x, y: PLAYER_Y - 8.0, vx: 0.0 });
        g.banner = 0.0;
        g.collide();
        assert_eq!(g.lives, 2);
        assert!(g.invincible > 0.0);
        g.bolts.push(Bolt { x: g.x, y: PLAYER_Y - 8.0, vx: 0.0 });
        g.collide();
        assert_eq!(g.lives, 2);
    }

    #[test]
    fn the_shield_takes_the_hit() {
        let mut g = Game::new(3);
        g.shield = true;
        g.hit();
        assert_eq!((g.lives, g.shield), (3, false));
    }

    #[test]
    fn the_last_life_lost_ends_the_game_but_not_by_production() {
        let mut g = Game::new(3);
        g.lives = 1;
        g.hit();
        assert!(g.over && !g.invaded);
    }

    #[test]
    fn the_last_bug_squashed_brings_the_next_wave() {
        let mut g = Game::new(4);
        for b in g.bugs.iter_mut().skip(1) {
            b.alive = false;
        }
        g.squash(0);
        g.banner = 0.0;
        g.step(DT, idle());
        assert_eq!(g.wave, 2);
        assert_eq!(g.bugs.iter().filter(|b| b.alive).count(), COLS * ROWS);
    }

    #[test]
    fn the_ranks_down_to_production_end_the_game() {
        let mut g = Game::new(4);
        g.banner = 0.0;
        g.fy = PLAYER_Y - (ROWS - 1) as f32 * CELL_H;
        g.place_ranks();
        g.collide();
        assert!(g.over && g.invaded);
    }

    #[test]
    fn every_fifth_wave_the_boss_then_its_points() {
        let mut g = Game::new(8);
        while g.wave < 4 {
            g.bugs.clear();
            g.banner = 0.0;
            g.step(DT, idle());
        }
        assert!(g.boss.is_none());
        g.bugs.clear();
        g.banner = 0.0;
        g.step(DT, idle());
        assert_eq!(g.wave, 5);
        let boss = g.boss.expect("a boss at wave 5");
        assert!(g.bugs.is_empty());
        // Its moths come and go; hit until down.
        g.banner = 0.0;
        g.invincible = 1000.0;
        let before = g.score;
        for _ in 0..boss.max_hp {
            let b = g.boss.unwrap();
            g.boss.as_mut().unwrap().blink = 0.0;
            g.shots.push(Shot { x: b.x, y: b.y, vx: 0.0 });
            g.collide();
        }
        assert!(g.boss.is_none());
        assert!(g.score >= before + 500);
        g.step(DT, idle());
        assert_eq!(g.wave, 6);
    }

    /// Whole games, firing at random: they end, and nothing goes out of the field.
    #[test]
    fn games_played_at_random_end() {
        for seed in 1..12 {
            let mut g = Game::new(seed * 7919);
            let mut k = 0u32;
            while !g.over && k < 120 * 60 * 20 {
                let r = g.random();
                g.step(DT, Input { left: r < 0.3, right: r > 0.7, fire: k.is_multiple_of(3) });
                k += 1;
                assert!(g.x >= 0.0 && g.x <= W);
            }
            assert!(g.over, "seed {seed}: still on after 20 minutes, wave {}", g.wave);
            assert!(g.score > 0);
        }
    }
}
