//! Backstage's world: the corridors behind a stage, where the singer picks up the picks lying around
//! (another one turns up somewhere for each one taken) while Krog hunts him, for as long as he lasts.
//! Krog wanders (hearing him: half the time his way) until he sees him (along a corridor, or close by),
//! then runs at him, and goes to where he saw him last. Every ten picks he gets angrier: faster, seeing
//! farther, the picks worth more; at fifteen he calls his mate Gnar, slow but always knowing where the
//! singer is, at forty Slug, who hunts like him. An amp dropped in a corridor stops them a while.
//! Caught, the show is over. Units are cells; positions are cell centers, (0.5, 0.5) for
//! the top left one.

use std::collections::VecDeque;

pub const W: usize = 21;
pub const H: usize = 13;
/// Amps to drop at the start; one more each time Krog gets angrier, up to the most.
pub const AMPS: u32 = 2;
const AMPS_MAX: u32 = 3;
/// An amp stops them this long (seconds).
pub const AMP_SECS: f32 = 5.0;
/// Picks lying around, always.
const PICKS_ON: usize = 5;
/// Picks taken for Krog to get angrier, for him to call Gnar, Slug.
pub const PER_RAGE: u32 = 10;
const GNAR_AT: u32 = 15;
const SLUG_AT: u32 = 40;

const PLAYER_SPEED: f32 = 5.0;
/// Krog's speed hunting, a step of anger more, at most; Slug's a little less.
const KROG_HUNT: f32 = 3.6;
const KROG_RAGE: f32 = 0.25;
const KROG_MAX: f32 = 5.2;
const SLUG_SLOWER: f32 = 0.4;
/// Wandering, a step of anger more, at most.
const WANDER: f32 = 2.8;
const WANDER_MAX: f32 = 3.6;
/// Gnar, always on his way to the singer.
const GNAR: f32 = 2.4;
const GNAR_MAX: f32 = 3.4;
/// Krog sees this far along a corridor (farther a step of anger more, at most), and this far around
/// the corner (steps of the maze).
const SIGHT: usize = 7;
const SIGHT_MAX: usize = 12;
const NEAR: usize = 3;
/// He goes to where he saw the singer last, for this long.
const MEMORY: f32 = 4.0;
/// Closer than this, they catch him.
const CATCH: f32 = 0.6;
/// Points of a pick, times one more a step of anger.
const PICK_POINTS: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
        }
    }

    fn back(self) -> Dir {
        match self {
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

/// The keys this step.
#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// The way held (the last one pressed).
    pub dir: Option<Dir>,
    /// The amp key went down.
    pub amp: bool,
}

type Cell = (usize, usize);

/// Someone going from cell to cell.
#[derive(Clone, Copy, Debug)]
pub struct Walker {
    /// The cell left, the cell going to (the same: standing), and how far between them (0..1).
    pub from: Cell,
    pub to: Cell,
    pub part: f32,
    /// The way faced.
    pub facing: Dir,
}

impl Walker {
    fn at(cell: Cell) -> Self {
        Self { from: cell, to: cell, part: 0.0, facing: Dir::Right }
    }

    /// Where they are, in cells (centers at .5).
    pub fn pos(&self) -> (f32, f32) {
        let (a, b) = (self.from, self.to);
        (a.0 as f32 + 0.5 + (b.0 as f32 - a.0 as f32) * self.part, a.1 as f32 + 0.5 + (b.1 as f32 - a.1 as f32) * self.part)
    }

    /// The cell they're mostly in.
    pub fn cell(&self) -> Cell {
        if self.part < 0.5 { self.from } else { self.to }
    }
}

/// Krog and his mates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Who {
    Krog,
    Gnar,
    Slug,
}

impl Who {
    pub fn name(self) -> &'static str {
        match self {
            Who::Krog => "Krog",
            Who::Gnar => "Gnar",
            Who::Slug => "Slug",
        }
    }
}

/// One of those after the singer.
#[derive(Clone, Copy, Debug)]
pub struct Chaser {
    pub who: Who,
    pub walker: Walker,
    /// Hunting (he sees the singer, or remembers where; Gnar always).
    pub hunting: bool,
    /// Where he saw him last, and for how long more he goes there.
    seen: Option<(Cell, f32)>,
}

impl Chaser {
    fn new(who: Who, cell: Cell) -> Self {
        Self { who, walker: Walker::at(cell), hunting: who == Who::Gnar, seen: None }
    }
}

/// What a step brought, for the page (sounds, flashes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub picked: bool,
    pub caught: bool,
    /// One of them just saw the singer.
    pub spotted: bool,
    /// Krog got angrier.
    pub angrier: bool,
    /// Krog called a mate.
    pub called: Option<Who>,
}

#[derive(Clone, Debug)]
pub struct Show {
    /// Walls, by row.
    pub walls: Vec<Vec<bool>>,
    pub picks: Vec<Cell>,
    pub player: Walker,
    /// Krog first, then his mates as they come.
    pub chasers: Vec<Chaser>,
    /// Amps on the floor: their cell, seconds left.
    pub amps_down: Vec<(Cell, f32)>,
    pub amps: u32,
    pub score: u32,
    pub picked: u32,
    /// Krog's anger: a step every `PER_RAGE` picks.
    pub rage: u32,
    /// Seconds before anyone moves, at the start.
    pub frozen: f32,
    pub over: bool,
    /// The way asked last: taken at the next crossing it opens.
    wanted: Option<Dir>,
    rng: u64,
}

impl Show {
    pub fn new(seed: u64) -> Self {
        let mut show = Self { walls: Vec::new(), picks: Vec::new(), player: Walker::at((1, 1)), chasers: Vec::new(), amps_down: Vec::new(), amps: AMPS, score: 0, picked: 0, rage: 0, frozen: 1.0, over: false, wanted: None, rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1 };
        show.walls = show.maze();
        show.chasers.push(Chaser::new(Who::Krog, show.krog_start()));
        while show.picks.len() < PICKS_ON {
            show.place_pick();
        }
        show
    }

    fn random(&mut self) -> u64 {
        // xorshift64.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn below(&mut self, n: usize) -> usize {
        (self.random() % n as u64) as usize
    }

    pub fn wall(&self, (x, y): Cell) -> bool {
        self.walls[y][x]
    }

    fn floor_cells(&self) -> Vec<Cell> {
        (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&c| !self.wall(c)).collect()
    }

    /// The cell next to `c` that way, when it isn't a wall.
    fn step_to(&self, c: Cell, dir: Dir) -> Option<Cell> {
        let (dx, dy) = dir.delta();
        let (x, y) = (c.0 as i32 + dx, c.1 as i32 + dy);
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
            return None;
        }
        let next = (x as usize, y as usize);
        (!self.wall(next)).then_some(next)
    }

    /// For those after him: an amp is a wall too, unless the singer stands on it (he walked back onto
    /// his own): it doesn't hide him.
    fn chaser_step_to(&self, c: Cell, dir: Dir) -> Option<Cell> {
        let standing = (self.player.from == self.player.to).then_some(self.player.from);
        self.step_to(c, dir).filter(|n| Some(*n) == standing || !self.amps_down.iter().any(|(a, _)| a == n))
    }

    /// Krog's start: in the middle, where the floor is far enough from the singer's start.
    fn krog_start(&self) -> Cell {
        let far = self.distances((1, 1));
        let mut floor = self.floor_cells();
        floor.sort_by_key(|&(x, y)| x.abs_diff(W / 2) + y.abs_diff(H / 2));
        floor.into_iter().find(|c| far[c.1][c.0] >= 10).unwrap_or((W - 2, H - 2))
    }

    /// Where a mate turns up: the floor farthest from the singer, none of them on it already.
    fn mate_start(&self) -> Cell {
        let d = self.distances(self.player.cell());
        let taken: Vec<Cell> = self.chasers.iter().map(|c| c.walker.cell()).collect();
        self.floor_cells().into_iter().filter(|c| !taken.contains(c) && d[c.1][c.0] != usize::MAX).max_by_key(|c| d[c.1][c.0]).unwrap_or((W - 2, H - 2))
    }

    /// A pick somewhere away from the singer, not on them nor too near another pick.
    fn place_pick(&mut self) {
        let d = self.distances(self.player.cell());
        let taken: Vec<Cell> = self.chasers.iter().map(|c| c.walker.cell()).chain(self.picks.iter().copied()).collect();
        let spots: Vec<Cell> = self.floor_cells().into_iter().filter(|c| d[c.1][c.0] != usize::MAX && d[c.1][c.0] >= 6 && !taken.contains(c)).collect();
        let spaced: Vec<Cell> = spots.iter().copied().filter(|c| self.picks.iter().all(|p| p.0.abs_diff(c.0) + p.1.abs_diff(c.1) >= 3)).collect();
        let spots = if spaced.is_empty() { spots } else { spaced };
        if !spots.is_empty() {
            let k = self.below(spots.len());
            self.picks.push(spots[k]);
        }
    }

    /// A maze dug from the top left (every corridor reaches every other), then some walls knocked down
    /// so that there are loops to run round them.
    fn maze(&mut self) -> Vec<Vec<bool>> {
        let mut walls = vec![vec![true; W]; H];
        let mut stack = vec![(1usize, 1usize)];
        walls[1][1] = false;
        while let Some(&(x, y)) = stack.last() {
            let mut ways: Vec<(usize, usize)> = Vec::new();
            for (dx, dy) in [(-2i32, 0i32), (2, 0), (0, -2), (0, 2)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx > 0 && ny > 0 && nx < W as i32 - 1 && ny < H as i32 - 1 && walls[ny as usize][nx as usize] {
                    ways.push((nx as usize, ny as usize));
                }
            }
            if ways.is_empty() {
                stack.pop();
                continue;
            }
            let (nx, ny) = ways[self.below(ways.len())];
            walls[(y + ny) / 2][(x + nx) / 2] = false;
            walls[ny][nx] = false;
            stack.push((nx, ny));
        }
        // Loops: a wall between two corridors knocked down now and then.
        for y in 1..H - 1 {
            for x in 1..W - 1 {
                if !walls[y][x] {
                    continue;
                }
                let across = (!walls[y][x - 1] && !walls[y][x + 1] && walls[y - 1][x] && walls[y + 1][x]) || (!walls[y - 1][x] && !walls[y + 1][x] && walls[y][x - 1] && walls[y][x + 1]);
                if across && self.below(100) < 12 {
                    walls[y][x] = false;
                }
            }
        }
        // No dead end: one with no way out but back gets its far wall knocked through to the next
        // corridor.
        let open = |walls: &Vec<Vec<bool>>, x: usize, y: usize| [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)].into_iter().filter(|&(a, b)| !walls[b][a]).count();
        for y in 1..H - 1 {
            for x in 1..W - 1 {
                if walls[y][x] || open(&walls, x, y) != 1 {
                    continue;
                }
                let through: Vec<(usize, usize)> = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                    .into_iter()
                    .filter_map(|(dx, dy)| {
                        let (wx, wy) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                        let (bx, by) = (x as i32 + 2 * dx, y as i32 + 2 * dy);
                        let inside = bx > 0 && by > 0 && bx < W as i32 - 1 && by < H as i32 - 1;
                        (walls[wy][wx] && inside && !walls[by as usize][bx as usize]).then_some((wx, wy))
                    })
                    .collect();
                if !through.is_empty() {
                    let (wx, wy) = through[self.below(through.len())];
                    walls[wy][wx] = false;
                }
            }
        }
        walls
    }

    /// Steps from `from` to every cell (usize::MAX: walled off). `chaser`: amps count as walls.
    fn distances_for(&self, from: Cell, chaser: bool) -> Vec<Vec<usize>> {
        let mut d = vec![vec![usize::MAX; W]; H];
        d[from.1][from.0] = 0;
        let mut queue = VecDeque::from([from]);
        while let Some(c) = queue.pop_front() {
            for dir in Dir::ALL {
                let next = if chaser { self.chaser_step_to(c, dir) } else { self.step_to(c, dir) };
                if let Some(n) = next
                    && d[n.1][n.0] == usize::MAX
                {
                    d[n.1][n.0] = d[c.1][c.0] + 1;
                    queue.push_back(n);
                }
            }
        }
        d
    }

    pub fn distances(&self, from: Cell) -> Vec<Vec<usize>> {
        self.distances_for(from, false)
    }

    /// From `j`, the singer is seen: along a straight corridor, or a few steps away.
    fn sees(&self, j: Cell) -> bool {
        let p = self.player.cell();
        if j.0 == p.0 || j.1 == p.1 {
            let steps = j.0.abs_diff(p.0) + j.1.abs_diff(p.1);
            let clear = if j.0 == p.0 { (j.1.min(p.1)..=j.1.max(p.1)).all(|y| !self.wall((j.0, y))) } else { (j.0.min(p.0)..=j.0.max(p.0)).all(|x| !self.wall((x, j.1))) };
            if clear && steps <= (SIGHT + self.rage as usize).min(SIGHT_MAX) {
                return true;
            }
        }
        self.distances_for(j, true)[p.1][p.0] <= NEAR
    }

    /// How fast `c` goes now.
    fn speed(&self, c: &Chaser) -> f32 {
        let rage = self.rage as f32;
        let hunt = (KROG_HUNT + KROG_RAGE * rage).min(KROG_MAX);
        match (c.who, c.hunting) {
            (Who::Gnar, _) => (GNAR + 0.1 * rage).min(GNAR_MAX),
            (Who::Krog, true) => hunt,
            (Who::Slug, true) => hunt - SLUG_SLOWER,
            (_, false) => (WANDER + 0.1 * rage).min(WANDER_MAX),
        }
    }

    /// `dt` seconds of the show.
    pub fn step(&mut self, dt: f32, input: Input) -> Events {
        let mut events = Events::default();
        if self.over {
            return events;
        }
        if input.dir.is_some() {
            self.wanted = input.dir;
        }
        if input.amp && self.amps > 0 && self.frozen <= 0.0 {
            // Behind him: on the cell he leaves, or standing, the one at his back (another way open when
            // that's a wall); never on his own, which would keep him out of their reach. Not on one of them.
            let p = self.player;
            let cell = if p.from != p.to { Some(p.from) } else { [p.facing.back()].into_iter().chain(Dir::ALL).find_map(|d| self.step_to(p.from, d)) };
            if let Some(cell) = cell
                && !self.amps_down.iter().any(|(c, _)| *c == cell)
                && self.chasers.iter().all(|c| c.walker.cell() != cell)
            {
                self.amps_down.push((cell, AMP_SECS));
                self.amps -= 1;
            }
        }
        for amp in &mut self.amps_down {
            amp.1 -= dt;
        }
        self.amps_down.retain(|a| a.1 > 0.0);
        if self.frozen > 0.0 {
            self.frozen -= dt;
            return events;
        }

        // The singer: on to the next cell; at a cell, the way asked if it opens, else straight on.
        self.player = self.walk(self.player, PLAYER_SPEED * dt, None);
        let here = self.player.cell();
        if let Some(k) = self.picks.iter().position(|&p| p == here) {
            self.picks.swap_remove(k);
            self.picked += 1;
            self.score += PICK_POINTS * (1 + self.rage);
            events.picked = true;
            self.place_pick();
            if self.picked % PER_RAGE == 0 {
                self.rage += 1;
                self.amps = (self.amps + 1).min(AMPS_MAX);
                events.angrier = true;
            }
            let call = match self.picked {
                GNAR_AT => Some(Who::Gnar),
                SLUG_AT => Some(Who::Slug),
                _ => None,
            };
            if let Some(who) = call {
                let cell = self.mate_start();
                self.chasers.push(Chaser::new(who, cell));
                events.called = Some(who);
            }
        }

        // Each of them: sees him, or remembers where; else wanders. Gnar always knows.
        for k in 0..self.chasers.len() {
            let mut c = self.chasers[k];
            if c.who != Who::Gnar {
                if self.sees(c.walker.cell()) {
                    if !c.hunting {
                        events.spotted = true;
                    }
                    c.seen = Some((here, MEMORY));
                } else if let Some((cell, left)) = c.seen {
                    let left = left - dt;
                    c.seen = (left > 0.0 && c.walker.cell() != cell).then_some((cell, left));
                }
                c.hunting = c.seen.is_some();
            }
            self.chasers[k] = c;
            let amount = self.speed(&c) * dt;
            self.chasers[k].walker = self.walk(c.walker, amount, Some(k));
        }

        let a = self.player.pos();
        let caught = self.chasers.iter().any(|c| {
            let b = c.walker.pos();
            (a.0 - b.0).hypot(a.1 - b.1) < CATCH
        });
        if caught {
            events.caught = true;
            self.over = true;
        }
        events
    }

    /// `w` moved `amount` cells along the maze. `chaser`: which of them, his way chosen at each cell
    /// (towards the singer when hunting, wandering otherwise), amps in his way; else the singer.
    fn walk(&mut self, mut w: Walker, mut amount: f32, chaser: Option<usize>) -> Walker {
        // Turning back between two cells, at once.
        if chaser.is_none()
            && let Some(want) = self.wanted
            && w.from != w.to
            && want == w.facing.back()
        {
            std::mem::swap(&mut w.from, &mut w.to);
            w.part = 1.0 - w.part;
            w.facing = want;
        }
        while amount > 0.0 {
            if w.from == w.to {
                // At a cell: which way now.
                let next = match chaser {
                    Some(k) => self.chaser_choice(k, w),
                    None => self.player_choice(w),
                };
                let Some(dir) = next else { break };
                let to = if chaser.is_some() { self.chaser_step_to(w.from, dir) } else { self.step_to(w.from, dir) };
                let Some(to) = to else { break };
                w.to = to;
                w.part = 0.0;
                w.facing = dir;
            }
            let left = 1.0 - w.part;
            if amount < left {
                w.part += amount;
                break;
            }
            amount -= left;
            w.from = w.to;
            w.part = 0.0;
        }
        w
    }

    /// The way asked, when it opens here; else on straight. Standing still until a way is asked.
    fn player_choice(&self, w: Walker) -> Option<Dir> {
        let want = self.wanted?;
        [want, w.facing].into_iter().find(|&d| self.step_to(w.from, d).is_some())
    }

    /// The way that brings `w` closest to `target`, of `ways`.
    fn closest(&self, w: Walker, ways: Vec<Dir>, target: Cell) -> Option<Dir> {
        let d = self.distances_for(target, true);
        ways.into_iter().min_by_key(|&dir| self.chaser_step_to(w.from, dir).map_or(usize::MAX, |c| d[c.1][c.0]))
    }

    fn chaser_choice(&mut self, k: usize, w: Walker) -> Option<Dir> {
        let open: Vec<Dir> = Dir::ALL.into_iter().filter(|&d| self.chaser_step_to(w.from, d).is_some()).collect();
        if open.is_empty() {
            return None;
        }
        let c = self.chasers[k];
        let target = if c.who == Who::Gnar { Some(self.player.cell()) } else { c.seen.map(|s| s.0) };
        if let Some(target) = target {
            return self.closest(w, open, target);
        }
        // Wandering: never straight back unless it's a dead end; half the time his way to the singer.
        let ahead: Vec<Dir> = open.iter().copied().filter(|&d| d != w.facing.back()).collect();
        let ways = if ahead.is_empty() { open } else { ahead };
        if ways.len() > 1 && self.below(2) == 0 {
            return self.closest(w, ways, self.player.cell());
        }
        Some(ways[self.below(ways.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f32 = 1.0 / 120.0;

    fn run(show: &mut Show, secs: f32, input: Input) -> Events {
        let mut all = Events::default();
        for _ in 0..(secs / STEP) as usize {
            let e = show.step(STEP, input);
            all.picked |= e.picked;
            all.caught |= e.caught;
            all.spotted |= e.spotted;
            all.angrier |= e.angrier;
            all.called = all.called.or(e.called);
        }
        all
    }

    /// Everyone after the singer walled in at the start by amps that never end.
    fn wall_them_in(show: &mut Show) {
        for c in &mut show.chasers {
            c.walker = Walker::at((1, 1));
        }
        show.amps_down = Dir::ALL.into_iter().filter_map(|d| show.step_to((1, 1), d)).map(|c| (c, 1e9)).collect();
    }

    #[test]
    fn every_show_is_a_maze_where_everything_can_be_reached() {
        for seed in 1..30 {
            let show = Show::new(seed);
            let d = show.distances((1, 1));
            assert!(!show.wall((1, 1)));
            assert_eq!(show.picks.len(), PICKS_ON, "seed {seed}");
            let krog = show.chasers[0].walker.from;
            for &c in show.picks.iter().chain([&krog]) {
                assert_ne!(d[c.1][c.0], usize::MAX, "seed {seed}: {c:?} walled off");
            }
            // The outer wall stays.
            assert!((0..W).all(|x| show.wall((x, 0)) && show.wall((x, H - 1))));
            // No dead end: every corridor leads on.
            for c in show.floor_cells() {
                let ways = Dir::ALL.into_iter().filter(|&d| show.step_to(c, d).is_some()).count();
                assert!(ways >= 2, "seed {seed}: a dead end at {c:?}");
            }
            // Krog alone, far from the singer.
            assert_eq!(show.chasers.len(), 1);
            assert!(d[krog.1][krog.0] >= 10);
        }
        // The same seed, the same backstage.
        assert_eq!(Show::new(5).walls, Show::new(5).walls);
    }

    #[test]
    fn the_singer_walks_the_corridors_and_not_through_walls() {
        let mut show = Show::new(3);
        run(&mut show, 1.2, Input::default());
        // Into a wall: he stays.
        let open = Dir::ALL.into_iter().find(|&d| show.step_to((1, 1), d).is_some()).unwrap();
        let closed = Dir::ALL.into_iter().find(|&d| show.step_to((1, 1), d).is_none()).unwrap();
        show.chasers[0].walker = Walker::at((W - 2, H - 2));
        let mut s = show.clone();
        run(&mut s, 0.5, Input { dir: Some(closed), amp: false });
        assert_eq!(s.player.cell(), (1, 1));
        run(&mut show, 0.18, Input { dir: Some(open), amp: false });
        assert_ne!(show.player.cell(), (1, 1));
        assert!(!show.wall(show.player.cell()));
    }

    #[test]
    fn krog_hunts_a_singer_standing_still_and_catches_him() {
        let mut show = Show::new(11);
        let mut caught = false;
        for _ in 0..120 {
            if run(&mut show, 1.0, Input::default()).caught {
                caught = true;
                break;
            }
        }
        assert!(caught, "Krog never found him");
        // Caught once: the show is over, nothing moves any more.
        assert!(show.over);
        let (score, at) = (show.score, show.player.pos());
        assert_eq!(run(&mut show, 1.0, Input { dir: Some(Dir::Right), amp: false }), Events::default());
        assert_eq!((show.score, show.player.pos()), (score, at));
    }

    #[test]
    fn an_amp_stops_krog() {
        // A single corridor: the singer at one end, Krog at the other seeing him, an amp between them.
        let mut show = Show::new(2);
        show.frozen = 0.0;
        show.walls = vec![vec![true; W]; H];
        for x in 1..11 {
            show.walls[1][x] = false;
        }
        show.picks.clear();
        show.player = Walker::at((1, 1));
        show.chasers[0].walker = Walker::at((10, 1));
        show.amps_down.push(((5, 1), AMP_SECS));
        let e = run(&mut show, AMP_SECS - 0.2, Input::default());
        assert!(e.spotted && !e.caught);
        assert!(show.chasers[0].walker.pos().0 > 5.5, "Krog went through the amp");
        // Gone, he comes through.
        assert!(run(&mut show, 4.0, Input::default()).caught);

        // Dropped by the singer, an amp costs one, and goes after a while.
        let mut s = Show::new(4);
        run(&mut s, 1.2, Input::default());
        s.step(STEP, Input { dir: None, amp: true });
        assert_eq!((s.amps, s.amps_down.len()), (AMPS - 1, 1));
        // Standing, he drops it beside him, not where he stands.
        assert_ne!(s.amps_down[0].0, s.player.cell());
        run(&mut s, AMP_SECS + 0.1, Input::default());
        assert!(s.amps_down.is_empty());
    }

    #[test]
    fn an_amp_never_hides_the_singer_standing_on_it() {
        // A single corridor, the singer standing still on an amp (walked back onto his own), Krog seeing him.
        let mut show = Show::new(2);
        show.frozen = 0.0;
        show.walls = vec![vec![true; W]; H];
        for x in 1..11 {
            show.walls[1][x] = false;
        }
        show.picks.clear();
        show.player = Walker::at((1, 1));
        show.chasers[0].walker = Walker::at((6, 1));
        show.amps_down.push(((1, 1), AMP_SECS));
        assert!(run(&mut show, 3.0, Input::default()).caught);
    }

    #[test]
    fn the_picks_come_back_and_krog_gets_angrier_and_calls_his_mates() {
        let mut show = Show::new(9);
        run(&mut show, 1.2, Input::default());
        let mut score = 0;
        // Teleported onto a pick, again and again.
        for n in 1..=SLUG_AT {
            let rage = show.rage;
            show.player = Walker::at(show.picks[0]);
            wall_them_in(&mut show);
            let e = show.step(STEP, Input::default());
            assert!(e.picked && !e.caught, "pick {n}");
            score += PICK_POINTS * (1 + rage);
            assert_eq!(show.picks.len(), PICKS_ON, "another one turns up");
            assert!(!show.picks.contains(&show.player.cell()));
            assert_eq!(e.angrier, n % PER_RAGE == 0, "pick {n}");
            let mates: Vec<Who> = show.chasers.iter().map(|c| c.who).collect();
            match n {
                GNAR_AT => assert_eq!((e.called, &mates[..]), (Some(Who::Gnar), &[Who::Krog, Who::Gnar][..])),
                SLUG_AT => assert_eq!((e.called, &mates[..]), (Some(Who::Slug), &[Who::Krog, Who::Gnar, Who::Slug][..])),
                _ => assert_eq!(e.called, None, "pick {n}"),
            }
        }
        assert_eq!((show.score, show.picked, show.rage), (score, SLUG_AT, SLUG_AT / PER_RAGE));
        assert_eq!(show.amps, AMPS_MAX);
        // Angrier, Krog hunts faster.
        let krog = Chaser { hunting: true, ..show.chasers[0] };
        assert!(show.speed(&krog) > KROG_HUNT);
    }

    #[test]
    fn gnar_always_knows_where_the_singer_is() {
        let mut show = Show::new(6);
        show.frozen = 0.0;
        // Gnar alone, the singer standing far from him.
        let gnar = show.mate_start();
        show.chasers = vec![Chaser::new(Who::Gnar, gnar)];
        let start = show.distances(gnar)[1][1];
        assert!(start >= 10);
        let e = run(&mut show, 2.0, Input::default());
        assert!(!e.spotted, "nobody saw him");
        let now = show.chasers[0].walker.cell();
        assert!(show.distances(now)[1][1] + 3 <= start, "Gnar went his way");
        assert!(run(&mut show, 30.0, Input::default()).caught);
    }
}
