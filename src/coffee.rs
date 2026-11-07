//! Pause Café, its rules (the page is `app::coffee`): interns come out of the lift and walk straight
//! ahead, whatever is there, and the player hands out the few jobs of the day to get enough of them to
//! the coffee machine: the project manager stops the others, the DevOps digs, the accountant lays a
//! stair of binders, HR's umbrella softens a fall. The world moves one tick at a time, on a grid.

/// The grid, in cells; below it is the void.
pub const W: usize = 48;
pub const H: usize = 20;
/// Seconds a tick lasts: an intern walks one cell a tick.
pub const TICK: f32 = 0.2;
/// Ticks between two interns out of the lift.
const SPAWN_TICKS: u32 = 8;
/// Ticks a cell takes to dig, a binder to lay, an umbrella to sink one cell.
const WORK_TICKS: u32 = 2;
/// Binders in a stair.
pub const STAIR: u32 = 8;
/// The longest fall an intern gets up from.
pub const FALL_MAX: u32 = 8;
/// Levels to finish to win the Game of Thrones theme.
pub const THRONES_UNLOCK: usize = 3;

/// Whether these bests (one a level, 0 if not finished) won the theme.
pub fn thrones_won(bests: &[u32]) -> bool {
    bests.iter().filter(|&&b| b > 0).count() >= THRONES_UNLOCK
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    /// Plasterboard: it can be dug.
    Soft,
    /// Concrete: it can't.
    Hard,
    /// A binder of a stair: it can be dug.
    Binder,
}

/// The jobs handed out, in the order of `Level::jobs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Job {
    Manager,
    DevOps,
    Accountant,
    Umbrella,
}

pub const JOBS: [Job; 4] = [Job::Manager, Job::DevOps, Job::Accountant, Job::Umbrella];

/// Why a job can't be given (the job isn't spent), in the order of the texts saying so.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// No intern where the click was.
    Nobody,
    NoneLeft,
    InTheAir,
    /// Concrete underfoot: nothing to dig.
    Concrete,
    /// Something ahead or overhead: no room for a stair.
    NoRoom,
    /// Busy with a job already.
    Busy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Walking,
    Falling,
    /// Arms out: the others turn back.
    Stopping,
    Digging,
    /// Binders still to lay.
    Building(u32),
    /// At the coffee machine.
    Saved,
    /// Gone home: fell too far, or into the void.
    Lost,
}

#[derive(Clone, Copy, Debug)]
pub struct Intern {
    pub x: i32,
    pub y: i32,
    /// Where it was a tick ago, to draw it in between.
    pub was: (i32, i32),
    /// 1 to the right, -1 to the left.
    pub dir: i32,
    pub state: State,
    pub umbrella: bool,
    /// Cells fallen so far.
    fell: u32,
    /// Ticks into the work at hand.
    work: u32,
}

/// A rectangle of the level, both corners in it.
type Block = (usize, usize, usize, usize, Cell);

/// A level: its walls, where the interns come out and where the coffee is.
pub struct Level {
    pub blocks: &'static [Block],
    pub lift: (i32, i32),
    pub coffee: (i32, i32),
    pub interns: u32,
    /// How many must get their coffee.
    pub need: u32,
    /// How many of each job, in the order of `JOBS`.
    pub jobs: [u32; 4],
    pub secs: u32,
}

const S: Cell = Cell::Soft;
const C: Cell = Cell::Hard;

pub const LEVELS: [Level; 10] = [
    // The first day: a floor to dig through.
    Level { blocks: &[(1, 8, 46, 8, S), (1, 13, 46, 13, C)], lift: (3, 7), coffee: (40, 12), interns: 10, need: 8, jobs: [0, 2, 0, 0], secs: 120 },
    // A wall of concrete to climb over.
    Level { blocks: &[(1, 13, 46, 13, C), (24, 9, 25, 12, C)], lift: (3, 12), coffee: (40, 12), interns: 10, need: 8, jobs: [0, 0, 2, 0], secs: 120 },
    // A gap: the others wait while the stair is laid.
    Level { blocks: &[(1, 13, 20, 13, C), (27, 13, 46, 13, C)], lift: (3, 12), coffee: (40, 12), interns: 12, need: 9, jobs: [1, 0, 1, 0], secs: 150 },
    // The tower: a long way down.
    Level { blocks: &[(1, 4, 20, 4, C), (1, 17, 46, 17, C)], lift: (3, 3), coffee: (40, 16), interns: 8, need: 6, jobs: [0, 0, 0, 8], secs: 120 },
    // Down through the floor, then over a low wall under the ceiling.
    Level { blocks: &[(1, 7, 19, 7, S), (20, 7, 46, 7, C), (1, 12, 46, 12, C), (24, 10, 25, 11, C)], lift: (3, 6), coffee: (40, 11), interns: 12, need: 10, jobs: [0, 1, 1, 0], secs: 150 },
    // Two edges to keep them from: the coffee is back the other way.
    Level { blocks: &[(1, 10, 28, 10, S), (1, 15, 20, 15, C)], lift: (3, 9), coffee: (4, 14), interns: 12, need: 9, jobs: [2, 1, 0, 0], secs: 150 },
    // Down, then across a gap.
    Level { blocks: &[(1, 6, 17, 6, S), (18, 2, 18, 6, C), (1, 12, 20, 12, C), (27, 12, 46, 12, C)], lift: (3, 5), coffee: (42, 11), interns: 12, need: 9, jobs: [1, 1, 1, 0], secs: 180 },
    // The basement: two thick floors.
    Level { blocks: &[(1, 5, 46, 9, S), (1, 12, 46, 13, S), (1, 18, 46, 18, C)], lift: (3, 4), coffee: (40, 17), interns: 12, need: 10, jobs: [0, 2, 0, 0], secs: 180 },
    // Moving day: a long way down, then a gap.
    Level { blocks: &[(1, 3, 14, 3, C), (1, 16, 22, 16, C), (29, 16, 46, 16, C)], lift: (3, 2), coffee: (42, 15), interns: 10, need: 6, jobs: [1, 0, 1, 7], secs: 200 },
    // Friday night: all of it.
    Level { blocks: &[(1, 4, 21, 4, S), (21, 1, 21, 3, C), (1, 9, 24, 9, C), (30, 9, 46, 9, S), (30, 15, 46, 15, C)], lift: (3, 3), coffee: (44, 14), interns: 14, need: 10, jobs: [1, 2, 1, 0], secs: 240 },
];

/// A day at the office: one level being played.
pub struct Day {
    pub level: usize,
    cells: Vec<Cell>,
    pub interns: Vec<Intern>,
    /// Jobs left, in the order of `JOBS`.
    pub jobs: [u32; 4],
    pub ticks: u32,
    to_come: u32,
    /// The day is over.
    pub over: bool,
}

impl Day {
    pub fn new(level: usize) -> Self {
        let l = &LEVELS[level];
        let mut cells = vec![Cell::Empty; W * H];
        for y in 0..H {
            cells[y * W] = Cell::Hard;
            cells[y * W + W - 1] = Cell::Hard;
        }
        for &(x0, y0, x1, y1, cell) in l.blocks {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells[y * W + x] = cell;
                }
            }
        }
        Self { level, cells, interns: Vec::new(), jobs: l.jobs, ticks: 0, to_come: l.interns, over: false }
    }

    pub fn level(&self) -> &'static Level {
        &LEVELS[self.level]
    }

    pub fn cell(&self, x: usize, y: usize) -> Cell {
        self.cells[y * W + x]
    }

    fn solid(&self, x: i32, y: i32) -> bool {
        if x < 0 || x >= W as i32 || y < 0 {
            return true;
        }
        y < H as i32 && self.cells[y as usize * W + x as usize] != Cell::Empty
    }

    pub fn saved(&self) -> u32 {
        self.interns.iter().filter(|i| i.state == State::Saved).count() as u32
    }

    pub fn lost(&self) -> u32 {
        self.interns.iter().filter(|i| i.state == State::Lost).count() as u32
    }

    pub fn secs_left(&self) -> u32 {
        self.level().secs.saturating_sub((self.ticks as f32 * TICK) as u32)
    }

    /// Enough interns got their coffee.
    pub fn won(&self) -> bool {
        self.saved() >= self.level().need
    }

    /// What the day is worth: 100 an intern, 5 a second left; nothing if it is lost.
    pub fn score(&self) -> u32 {
        if self.won() { self.saved() * 100 + self.secs_left() * 5 } else { 0 }
    }

    /// The player calls it a day.
    pub fn finish(&mut self) {
        self.over = true;
    }

    /// Why intern `i` can't take this job now; `None` if it can.
    pub fn refusal(&self, i: usize, job: Job) -> Option<Refusal> {
        let Some(w) = self.interns.get(i) else { return Some(Refusal::Nobody) };
        // A project manager given the manager's job goes back to walking, for free.
        if job == Job::Manager && w.state == State::Stopping {
            return None;
        }
        if matches!(w.state, State::Saved | State::Lost | State::Stopping) {
            return Some(Refusal::Busy);
        }
        let slot = JOBS.iter().position(|&j| j == job).unwrap_or(0);
        if self.jobs[slot] == 0 {
            return Some(Refusal::NoneLeft);
        }
        if job == Job::Umbrella {
            return w.umbrella.then_some(Refusal::Busy);
        }
        match w.state {
            State::Falling => return Some(Refusal::InTheAir),
            State::Walking => {}
            _ => return Some(Refusal::Busy),
        }
        if !self.solid(w.x, w.y + 1) {
            return Some(Refusal::InTheAir);
        }
        match job {
            Job::DevOps if self.cells[(w.y + 1) as usize * W + w.x as usize] == Cell::Hard => Some(Refusal::Concrete),
            Job::Accountant if self.solid(w.x + w.dir, w.y) || self.solid(w.x + w.dir, w.y - 1) || self.solid(w.x, w.y - 1) => Some(Refusal::NoRoom),
            _ => None,
        }
    }

    /// Gives intern `i` a job if it can take it (see `refusal`); a job refused isn't spent.
    pub fn assign(&mut self, i: usize, job: Job) -> bool {
        if self.refusal(i, job).is_some() {
            return false;
        }
        let intern = self.interns[i];
        if job == Job::Manager && intern.state == State::Stopping {
            self.interns[i].state = State::Walking;
            return true;
        }
        let slot = JOBS.iter().position(|&j| j == job).unwrap_or(0);
        let state = match job {
            Job::Umbrella => intern.state,
            Job::Manager => State::Stopping,
            Job::DevOps => State::Digging,
            Job::Accountant => State::Building(STAIR),
        };
        let intern = &mut self.interns[i];
        intern.state = state;
        intern.work = 0;
        intern.umbrella |= job == Job::Umbrella;
        self.jobs[slot] -= 1;
        true
    }

    /// One tick more.
    pub fn tick(&mut self) {
        if self.over {
            return;
        }
        self.ticks += 1;
        if self.to_come > 0 && (self.ticks - 1).is_multiple_of(SPAWN_TICKS) {
            let (x, y) = self.level().lift;
            self.interns.push(Intern { x, y, was: (x, y), dir: 1, state: State::Walking, umbrella: false, fell: 0, work: 0 });
            self.to_come -= 1;
        }
        for i in 0..self.interns.len() {
            self.move_intern(i);
        }
        let busy = self.to_come > 0 || self.interns.iter().any(|i| !matches!(i.state, State::Saved | State::Lost | State::Stopping));
        if !busy || self.secs_left() == 0 {
            self.over = true;
        }
    }

    fn stopper_at(&self, x: i32, y: i32) -> bool {
        self.interns.iter().any(|i| i.state == State::Stopping && i.x == x && i.y == y)
    }

    fn move_intern(&mut self, i: usize) {
        let mut w = self.interns[i];
        if matches!(w.state, State::Saved | State::Lost) {
            return;
        }
        w.was = (w.x, w.y);
        let ground = self.solid(w.x, w.y + 1);
        match w.state {
            State::Stopping if !ground => w.state = State::Falling,
            State::Stopping => {}
            State::Digging | State::Building(_) | State::Walking if !ground => {
                w.state = State::Falling;
                w.fell = 0;
            }
            State::Walking => self.walk(&mut w),
            State::Digging => {
                w.work += 1;
                if w.work >= WORK_TICKS {
                    w.work = 0;
                    self.dig(&mut w);
                }
            }
            State::Building(left) => {
                w.work += 1;
                if w.work >= WORK_TICKS {
                    w.work = 0;
                    self.build(&mut w, left);
                }
            }
            State::Falling => {
                w.work += 1;
                if !w.umbrella || w.work >= WORK_TICKS {
                    w.work = 0;
                    w.y += 1;
                    w.fell += 1;
                }
                if w.y >= H as i32 {
                    w.state = State::Lost;
                } else if self.solid(w.x, w.y + 1) {
                    w.state = if w.fell > FALL_MAX && !w.umbrella { State::Lost } else { State::Walking };
                    w.fell = 0;
                }
            }
            State::Saved | State::Lost => {}
        }
        if (w.x, w.y) == self.level().coffee && !matches!(w.state, State::Stopping | State::Lost) {
            w.state = State::Saved;
        }
        self.interns[i] = w;
    }

    /// A step ahead: up a single step if it can, back the other way if not.
    fn walk(&self, w: &mut Intern) {
        let (x, y) = (w.x + w.dir, w.y);
        if self.stopper_at(x, y) {
            w.dir = -w.dir;
        } else if !self.solid(x, y) {
            w.x = x;
        } else if !self.solid(x, y - 1) && !self.solid(w.x, y - 1) {
            w.x = x;
            w.y = y - 1;
        } else {
            w.dir = -w.dir;
        }
    }

    /// The cell underfoot dug out; concrete stops the work.
    fn dig(&mut self, w: &mut Intern) {
        let below = (w.y + 1) as usize * W + w.x as usize;
        if self.cells[below] == Cell::Hard {
            w.state = State::Walking;
            return;
        }
        self.cells[below] = Cell::Empty;
        w.y += 1;
    }

    /// A binder laid ahead and climbed; something in the way ends the stair, and the walk goes on.
    fn build(&mut self, w: &mut Intern, left: u32) {
        let (x, y) = (w.x + w.dir, w.y);
        if self.solid(x, y) || self.solid(x, y - 1) || self.solid(w.x, y - 1) {
            w.state = State::Walking;
            return;
        }
        self.cells[y as usize * W + x as usize] = Cell::Binder;
        w.x = x;
        w.y = y - 1;
        w.state = if left > 1 { State::Building(left - 1) } else { State::Walking };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A move of a solution: each waits for its moment, in order.
    enum Do {
        /// An intern going right here, not at that job yet, is given it.
        At(i32, i32, Job),
        /// Some ticks later.
        Wait(u32),
        /// The project manager goes back to walking.
        Free,
    }
    use Do::{At, Free, Wait};
    use Job::{Accountant, DevOps, Manager, Umbrella};

    fn play(level: usize, moves: &[Do]) -> Day {
        let mut day = Day::new(level);
        let mut next = 0;
        let mut waited = 0;
        while !day.over {
            day.tick();
            while let Some(m) = moves.get(next) {
                let done = match *m {
                    At(x, y, job) => {
                        let found = day.interns.iter().position(|i| i.x == x && i.y == y && i.dir == 1 && i.state == State::Walking && !(job == Umbrella && i.umbrella));
                        found.is_some_and(|i| day.assign(i, job))
                    }
                    Wait(n) => {
                        waited += 1;
                        let over = waited >= n;
                        if over {
                            waited = 0;
                        }
                        over
                    }
                    Free => {
                        let i = day.interns.iter().position(|i| i.state == State::Stopping).expect("a manager");
                        day.assign(i, Manager)
                    }
                };
                if !done {
                    break;
                }
                next += 1;
            }
            // The ones left stopping would keep the day going.
            if next == moves.len() && day.won() && day.interns.iter().all(|i| matches!(i.state, State::Saved | State::Lost | State::Stopping)) {
                day.finish();
            }
        }
        assert_eq!(next, moves.len(), "level {level}: move {next} never happened");
        day
    }

    #[test]
    fn every_level_can_be_won() {
        let solutions: [&[Do]; 10] = [
            &[At(10, 7, DevOps)],
            &[At(18, 12, Accountant)],
            &[At(20, 12, Accountant), At(14, 12, Manager), Wait(60), Free],
            &[At(6, 3, Umbrella), At(6, 3, Umbrella), At(6, 3, Umbrella), At(6, 3, Umbrella), At(6, 3, Umbrella), At(6, 3, Umbrella), At(6, 3, Umbrella), At(6, 3, Umbrella)],
            &[At(10, 6, DevOps), At(21, 11, Accountant)],
            &[At(25, 9, Manager), At(12, 9, DevOps), At(18, 14, Manager)],
            &[At(8, 5, DevOps), At(20, 11, Accountant), At(14, 11, Manager), Wait(60), Free],
            &[At(10, 4, DevOps), At(30, 11, DevOps)],
            &[At(6, 2, Umbrella), At(6, 2, Umbrella), At(6, 2, Umbrella), At(6, 2, Umbrella), At(6, 2, Umbrella), At(6, 2, Umbrella), At(6, 2, Umbrella), At(22, 15, Accountant), At(16, 15, Manager), Wait(60), Free],
            &[At(6, 3, DevOps), At(24, 8, Accountant), At(18, 8, Manager), Wait(70), Free, At(40, 8, DevOps)],
        ];
        for (level, moves) in solutions.iter().enumerate() {
            let day = play(level, moves);
            assert!(day.won(), "level {level}: {} saved, {} lost, need {}", day.saved(), day.lost(), day.level().need);
            assert!(day.score() > 0);
        }
    }

    #[test]
    fn no_level_is_won_with_no_job_given() {
        for level in 0..LEVELS.len() {
            let mut day = Day::new(level);
            while !day.over {
                day.tick();
            }
            assert!(!day.won(), "level {level} plays itself");
        }
    }

    #[test]
    fn a_long_fall_loses_an_intern() {
        let mut day = Day::new(3);
        while !day.over {
            day.tick();
        }
        assert_eq!(day.saved(), 0);
        assert_eq!(day.lost(), LEVELS[3].interns);
    }

    #[test]
    fn a_job_refused_is_not_spent() {
        // On concrete: nothing to dig, and the DevOps stays for later.
        let mut day = Day::new(1);
        day.jobs = [0, 1, 0, 0];
        day.tick();
        assert_eq!(day.refusal(0, Job::DevOps), Some(Refusal::Concrete));
        assert!(!day.assign(0, Job::DevOps));
        assert_eq!(day.jobs[1], 1);
        assert_eq!(day.interns[0].state, State::Walking);
        // Against a wall: no room for a stair.
        let mut day = Day::new(0);
        day.jobs = [0, 0, 1, 0];
        day.tick();
        day.interns[0].x = 46;
        assert_eq!(day.refusal(0, Job::Accountant), Some(Refusal::NoRoom));
        assert_eq!(day.jobs[2], 1);
    }

    #[test]
    fn a_job_runs_out_and_a_busy_intern_takes_no_other() {
        let mut day = Day::new(0);
        day.tick();
        assert!(day.assign(0, Job::DevOps));
        assert_eq!(day.refusal(0, Job::DevOps), Some(Refusal::Busy));
        day.jobs[1] = 0;
        day.interns.push(day.interns[0]);
        day.interns[1].state = State::Walking;
        assert_eq!(day.refusal(1, Job::DevOps), Some(Refusal::NoneLeft));
    }

    #[test]
    fn a_manager_can_be_freed_for_nothing() {
        let mut day = Day::new(0);
        day.jobs = [1, 0, 0, 0];
        day.tick();
        assert!(day.assign(0, Job::Manager));
        assert_eq!(day.jobs[0], 0);
        assert!(day.assign(0, Job::Manager));
        assert_eq!(day.interns[0].state, State::Walking);
    }
}
