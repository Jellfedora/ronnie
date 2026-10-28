//! Ronnie Run's world: a singer running on a stage, ever faster, jumping over the gear on the floor and
//! ducking under the lights hung low. Units are the stage's (the ground at 0, up is positive); the
//! page scales them to its room.

/// Where the singer runs, from the left of the stage.
pub const PLAYER_X: f32 = 80.0;
const STAND: (f32, f32) = (22.0, 46.0);
const DUCK: (f32, f32) = (30.0, 26.0);

const START_SPEED: f32 = 320.0;
const MAX_SPEED: f32 = 900.0;
/// Speed gained a second.
const SPEED_UP: f32 = 7.0;
const JUMP: f32 = 640.0;
const GRAVITY: f32 = 2200.0;
/// While the jump key is held on the way up: a higher jump.
const GRAVITY_HELD: f32 = 1250.0;
/// Ducking in the air: down fast.
const GRAVITY_DIVE: f32 = 5200.0;
/// Stage units a meter.
const METER: f32 = 40.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// An amp: low and wide.
    Amp,
    /// A flight case: tall.
    Case,
    /// Cables coiled on the floor: flat, long.
    Cable,
    /// A light hung low: to duck under.
    Light,
}

impl Kind {
    /// Its width, its height, and how high its bottom is.
    pub fn size(self) -> (f32, f32, f32) {
        match self {
            Kind::Amp => (42.0, 38.0, 0.0),
            Kind::Case => (28.0, 58.0, 0.0),
            Kind::Cable => (64.0, 12.0, 0.0),
            Kind::Light => (46.0, 22.0, 32.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub kind: Kind,
    /// Its left side.
    pub x: f32,
}

/// The keys this frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// The jump key went down.
    pub jump: bool,
    /// The jump key is held.
    pub jump_held: bool,
    pub duck: bool,
}

#[derive(Clone, Debug)]
pub struct Run {
    /// The singer's feet above the ground.
    pub y: f32,
    vy: f32,
    pub ducking: bool,
    pub speed: f32,
    /// Stage units run.
    pub distance: f32,
    pub obstacles: Vec<Obstacle>,
    /// Where the next obstacle comes in.
    next: f32,
    pub jumps: u32,
    pub over: bool,
    rng: u64,
}

impl Run {
    pub fn new(seed: u64) -> Self {
        Self { y: 0.0, vy: 0.0, ducking: false, speed: START_SPEED, distance: 0.0, obstacles: Vec::new(), next: 700.0, jumps: 0, over: false, rng: seed | 1 }
    }

    pub fn meters(&self) -> u32 {
        (self.distance / METER) as u32
    }

    pub fn grounded(&self) -> bool {
        self.y <= 0.0
    }

    /// The singer's box: left, bottom, width, height.
    pub fn player_box(&self) -> (f32, f32, f32, f32) {
        let (w, h) = if self.ducking { DUCK } else { STAND };
        (PLAYER_X, self.y, w, h)
    }

    fn random(&mut self) -> u64 {
        // xorshift64.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn unit(&mut self) -> f32 {
        (self.random() % 10_000) as f32 / 10_000.0
    }

    /// `dt` seconds of the run.
    pub fn step(&mut self, dt: f32, input: Input) {
        if self.over {
            return;
        }
        let dt = dt.min(0.05);
        self.speed = (self.speed + SPEED_UP * dt).min(MAX_SPEED);
        // Jump from the ground; duck on it, dive in the air.
        if input.jump && self.grounded() && !input.duck {
            self.vy = JUMP;
            self.jumps += 1;
        }
        self.ducking = input.duck && self.grounded();
        let gravity = if input.duck { GRAVITY_DIVE } else if input.jump_held && self.vy > 0.0 { GRAVITY_HELD } else { GRAVITY };
        self.vy -= gravity * dt;
        self.y += self.vy * dt;
        if self.y <= 0.0 {
            self.y = 0.0;
            self.vy = 0.0;
            self.ducking = input.duck;
        }

        let moved = self.speed * dt;
        self.distance += moved;
        for o in &mut self.obstacles {
            o.x -= moved;
        }
        self.obstacles.retain(|o| o.x + o.kind.size().0 > -50.0);
        self.next -= moved;
        if self.next <= 0.0 {
            self.spawn();
        }
        let (px, py, pw, ph) = self.player_box();
        // A little forgiving: the boxes touch by a few units before it counts.
        let hit = self.obstacles.iter().any(|o| {
            let (w, h, bottom) = o.kind.size();
            px + pw - 4.0 > o.x && px + 4.0 < o.x + w && py + 2.0 < bottom + h && py + ph - 2.0 > bottom
        });
        if hit {
            self.over = true;
        }
    }

    /// The next obstacle, off the right of the stage; the lights once the run got going.
    fn spawn(&mut self) {
        let roll = self.unit();
        let kind = match roll {
            r if r < 0.35 => Kind::Amp,
            r if r < 0.6 => Kind::Case,
            r if r < 0.8 => Kind::Cable,
            _ if self.distance > 60.0 * METER => Kind::Light,
            _ => Kind::Amp,
        };
        self.obstacles.push(Obstacle { kind, x: 1100.0 });
        // Room to land and jump again, more as it goes faster; now and then a longer break.
        let gap = self.speed * (0.8 + self.unit() * 0.9) + 120.0;
        self.next = gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    #[test]
    fn a_held_jump_goes_higher() {
        let top = |held: bool| {
            let mut run = Run::new(1);
            run.step(DT, Input { jump: true, jump_held: held, duck: false });
            let mut top: f32 = 0.0;
            while !run.grounded() {
                run.step(DT, Input { jump: false, jump_held: held, duck: false });
                top = top.max(run.y);
            }
            top
        };
        let (short, long) = (top(false), top(true));
        // High enough for the tallest case, either way.
        assert!(short > Kind::Case.size().1, "{short}");
        assert!(long > short * 1.4, "{short} {long}");
    }

    #[test]
    fn an_obstacle_stops_the_run_a_jump_clears_it() {
        let place = |kind| {
            let mut run = Run::new(3);
            run.next = f32::MAX;
            run.obstacles.push(Obstacle { kind, x: PLAYER_X + 260.0 });
            run
        };
        // Standing still in its way.
        let mut run = place(Kind::Amp);
        for _ in 0..240 {
            run.step(DT, Input::default());
        }
        assert!(run.over);
        // A jump at the right time.
        let mut run = place(Kind::Amp);
        for k in 0..240 {
            run.step(DT, Input { jump: k == 40, jump_held: true, duck: false });
        }
        assert!(!run.over, "cleared");
        assert_eq!(run.jumps, 1);
    }

    #[test]
    fn a_light_is_ducked_under() {
        let place = || {
            let mut run = Run::new(5);
            run.next = f32::MAX;
            run.obstacles.push(Obstacle { kind: Kind::Light, x: PLAYER_X + 200.0 });
            run
        };
        let mut standing = place();
        let mut ducking = place();
        for _ in 0..240 {
            standing.step(DT, Input::default());
            ducking.step(DT, Input { duck: true, ..Default::default() });
        }
        assert!(standing.over);
        assert!(!ducking.over);
    }

    #[test]
    fn it_speeds_up_and_counts_meters() {
        let mut run = Run::new(9);
        run.next = f32::MAX;
        for _ in 0..1200 {
            run.step(DT, Input::default());
        }
        assert!(run.speed > START_SPEED && run.speed <= MAX_SPEED);
        assert!(run.meters() > 80, "{}", run.meters());
    }
}
