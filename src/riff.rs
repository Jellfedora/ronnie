//! Riff Hero's song: a minute of metal made from a seed (the same seed, the same song: both players of
//! a battle play the same one), its notes on four strings played with the arrows, and its sound, a
//! distorted guitar playing those notes over a bass and drums. Times are in seconds from the song's
//! first beat.

/// The song's tempo.
pub const BPM: f64 = 150.0;
const BEAT: f64 = 60.0 / BPM;
const BAR: f64 = 4.0 * BEAT;
/// Its bars: an intro, a verse, a chorus, a solo, the last chorus; then the last chord rings.
const BARS: usize = 36;
/// How long the song lasts, the last chord included.
pub const LENGTH: f64 = BARS as f64 * BAR + 2.0;

/// Pressed this close to a note: perfect, or good (in seconds, either side).
pub const PERFECT: f64 = 0.075;
pub const GOOD: f64 = 0.16;
/// Points of a note, before the multiplier.
const PERFECT_POINTS: u32 = 100;
const GOOD_POINTS: u32 = 50;
/// The multiplier goes up every this many notes in a row, up to ×4.
pub const COMBO_STEP: u32 = 10;
pub const MAX_MULTIPLIER: u32 = 4;

/// The four strings, from the left: ←, ↓, ↑, →.
pub const LANES: usize = 4;
/// The power chord each string plays, in semitones above the low E (E, G, A, B).
const PITCHES: [i32; LANES] = [0, 3, 5, 7];
const LOW_E: f32 = 82.41;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub t: f64,
    pub lane: usize,
}

/// The song's notes, in time order (two at the same time: a chord).
#[derive(Clone, Debug)]
pub struct Chart {
    pub notes: Vec<Note>,
}

/// xorshift32: the same numbers from the same seed, here and in the other player's Ronnie.
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    /// In 0..1.
    fn unit(&mut self) -> f64 {
        (self.next() >> 8) as f64 / (1u32 << 24) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }
}

/// What a part of the song is like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Intro,
    Verse,
    Chorus,
    Solo,
    Last,
}

fn part(bar: usize) -> Part {
    match bar {
        0..4 => Part::Intro,
        4..12 => Part::Verse,
        12..20 => Part::Chorus,
        20..28 => Part::Solo,
        _ => Part::Last,
    }
}

/// How hard a song played alone is. Each has its own song, the same for everyone, so that the scores
/// compare on floor's board.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Level {
    Easy,
    #[default]
    Medium,
    Hard,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Easy, Level::Medium, Level::Hard];

    /// Its song: never to change, or the boards would mix songs.
    fn seed(self) -> u32 {
        match self {
            Level::Easy => 0x5EED_0001,
            Level::Medium => 0x5EED_0002,
            Level::Hard => 0x5EED_0003,
        }
    }

    /// Its board on floor.
    pub fn floor_game(self) -> &'static str {
        match self {
            Level::Easy => "riff-hero-facile",
            Level::Medium => "riff-hero-moyen",
            Level::Hard => "riff-hero-difficile",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

impl Chart {
    /// The song of a battle, its `seed` drawn by floor: medium, as it always was (the other player's
    /// Ronnie may be older).
    pub fn new(seed: u32) -> Self {
        Self::make(seed, Level::Medium)
    }

    /// The song of `level` played alone, the same for everyone.
    pub fn solo(level: Level) -> Self {
        Self::make(level.seed(), level)
    }

    /// The song of `seed`: a riff of two bars per part, played again with changes, so that it can be
    /// learned along the way.
    fn make(seed: u32, level: Level) -> Self {
        let mut rng = Rng::new(seed);
        let mut notes = Vec::new();
        let mut riffs: Vec<(Part, Vec<Vec<usize>>)> = Vec::new();
        for bar in 0..BARS - 1 {
            let p = part(bar);
            // A part's riff, written at its first bar; its bars played in turn, a step changed now and then.
            if !riffs.iter().any(|(q, _)| *q == p) {
                let riff = (0..2).map(|k| write_bar(&mut rng, p, k == 1, level)).collect();
                riffs.push((p, riff));
            }
            let riff = &riffs.iter().find(|(q, _)| *q == p).unwrap().1;
            let mut steps = riff[bar % 2].clone();
            if bar % 4 == 3 && p != Part::Intro {
                let k = rng.below(steps.len());
                if steps[k] != 0 {
                    steps[k] = 1 << rng.below(LANES);
                }
            }
            let start = bar as f64 * BAR;
            let step = BAR / steps.len() as f64;
            for (k, &lanes) in steps.iter().enumerate() {
                for lane in 0..LANES {
                    if lanes & (1 << lane) != 0 {
                        notes.push(Note { t: start + k as f64 * step, lane });
                    }
                }
            }
        }
        // The last chord, on the first beat of the last bar.
        let end = (BARS - 1) as f64 * BAR;
        notes.push(Note { t: end, lane: 0 });
        notes.push(Note { t: end, lane: 3 });
        Self { notes }
    }

    /// How many notes there are to play.
    pub fn len(&self) -> usize {
        self.notes.len()
    }
}

/// A bar of a riff for part `p`: each step the strings it plays (bits). `answer`: the second bar of
/// two, which ends higher. Easy: on the beats, few chords; hard: sixteenths in the solo, more chords.
fn write_bar(rng: &mut Rng, p: Part, answer: bool, level: Level) -> Vec<usize> {
    // The other beats are played this often in easy.
    let beats = if level == Level::Easy { 0.55 } else { 0.8 };
    let (steps, density, chords) = match (level, p) {
        (_, Part::Intro) => (4, 1.0, 0.0),
        (Level::Easy, Part::Verse) => (4, 0.0, 0.0),
        (Level::Easy, Part::Chorus) => (4, 0.0, 0.05),
        (Level::Easy, Part::Solo) => (8, 0.2, 0.0),
        (Level::Easy, Part::Last) => (4, 0.0, 0.1),
        (Level::Medium, Part::Verse) => (8, 0.2, 0.0),
        (Level::Medium, Part::Chorus) => (8, 0.3, 0.1),
        (Level::Medium, Part::Solo) => (8, 0.45, 0.0),
        (Level::Medium, Part::Last) => (8, 0.4, 0.15),
        (Level::Hard, Part::Verse) => (8, 0.45, 0.1),
        (Level::Hard, Part::Chorus) => (8, 0.55, 0.25),
        (Level::Hard, Part::Solo) => (16, 0.5, 0.0),
        (Level::Hard, Part::Last) => (8, 0.6, 0.3),
    };
    let mut bar = vec![0; steps];
    let mut lane = 0usize;
    for (k, slot) in bar.iter_mut().enumerate() {
        let downbeat = k % (steps / 4) == 0;
        // The first beat is always played, the other beats mostly; the steps between by the part's density.
        if !(k == 0 || (downbeat && rng.unit() < beats) || (!downbeat && rng.unit() < density)) {
            continue;
        }
        lane = if p == Part::Intro {
            // The intro chugs on the low string, answered on the beat before the next bar.
            if k == steps - 1 && answer { 3 } else { 0 }
        } else if p == Part::Solo {
            // Runs: up or down a string, or the same one again.
            match rng.below(3) {
                0 => lane.saturating_sub(1),
                1 => (lane + 1).min(LANES - 1),
                _ => lane,
            }
        } else if downbeat && rng.unit() < 0.5 {
            // Metal: back to the low string on the beat.
            0
        } else {
            // Mostly the string next to the last one: no wild jumps.
            match rng.below(4) {
                0 => lane.saturating_sub(1),
                1 | 2 => (lane + 1).min(LANES - 1),
                _ => lane,
            }
        };
        if answer && k == steps - 1 {
            lane = lane.max(2);
        }
        *slot = 1 << lane;
        if downbeat && rng.unit() < chords {
            let other = (lane + 1 + rng.below(LANES - 1)) % LANES;
            *slot |= 1 << other;
        }
    }
    bar
}

/// How a note was played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judgment {
    Perfect,
    Good,
    Miss,
}

/// A press: a note hit, or nothing to hit there (the combo breaks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Hit(Judgment),
    Stray,
}

/// A song being played: each note's judgment, the score, the combo.
#[derive(Clone, Debug)]
pub struct Play {
    pub chart: Chart,
    pub judged: Vec<Option<Judgment>>,
    /// Notes before this one are all judged.
    first: usize,
    pub score: u32,
    pub combo: u32,
    pub best_combo: u32,
    pub hits: u32,
    pub perfects: u32,
    pub misses: u32,
}

impl Play {
    pub fn new(chart: Chart) -> Self {
        let n = chart.len();
        Self { chart, judged: vec![None; n], first: 0, score: 0, combo: 0, best_combo: 0, hits: 0, perfects: 0, misses: 0 }
    }

    /// ×1 to ×4, by the notes hit in a row.
    pub fn multiplier(&self) -> u32 {
        (1 + self.combo / COMBO_STEP).min(MAX_MULTIPLIER)
    }

    /// String `lane` played at `t`: the closest note not played yet on it, if near enough. A press on
    /// nothing costs nothing (only a note gone by breaks the combo).
    pub fn press(&mut self, lane: usize, t: f64) -> Press {
        let mut best: Option<(usize, f64)> = None;
        for k in self.first..self.chart.notes.len() {
            let note = self.chart.notes[k];
            if note.t > t + GOOD {
                break;
            }
            let off = (note.t - t).abs();
            if note.lane == lane && self.judged[k].is_none() && off <= GOOD && best.is_none_or(|(_, b)| off < b) {
                best = Some((k, off));
            }
        }
        let Some((k, off)) = best else { return Press::Stray };
        let judgment = if off <= PERFECT { Judgment::Perfect } else { Judgment::Good };
        self.combo += 1;
        self.best_combo = self.best_combo.max(self.combo);
        self.score += self.multiplier() * if judgment == Judgment::Perfect { PERFECT_POINTS } else { GOOD_POINTS };
        self.hits += 1;
        if judgment == Judgment::Perfect {
            self.perfects += 1;
        }
        self.judged[k] = Some(judgment);
        Press::Hit(judgment)
    }

    /// The song at `t`: the notes gone past the line unplayed are missed. Their strings.
    pub fn advance(&mut self, t: f64) -> Vec<usize> {
        let mut missed = Vec::new();
        while self.first < self.chart.notes.len() {
            let note = self.chart.notes[self.first];
            if note.t + GOOD >= t {
                break;
            }
            if self.judged[self.first].is_none() {
                self.judged[self.first] = Some(Judgment::Miss);
                self.misses += 1;
                self.combo = 0;
                missed.push(note.lane);
            }
            self.first += 1;
        }
        missed
    }

    /// The song is over at `t`.
    pub fn over(&self, t: f64) -> bool {
        t >= LENGTH
    }
}

/// The sound's sample rate.
pub const RATE: u32 = 44_100;

/// The song of `chart`, mono: the guitar plays its notes (a chord held until the next note, muted short
/// when the next one is close), the bass under it an octave lower, the drums keep time.
pub fn render(chart: &Chart) -> Vec<f32> {
    let rate = RATE as f64;
    let mut out = vec![0.0f32; (LENGTH * rate) as usize];
    let mut noise = Rng::new(0x5eed);

    // The guitar and the bass: the notes at the same time make one chord.
    let mut onsets: Vec<(f64, Vec<usize>)> = Vec::new();
    for note in &chart.notes {
        match onsets.last_mut() {
            Some((t, lanes)) if (*t - note.t).abs() < 1e-6 => lanes.push(note.lane),
            _ => onsets.push((note.t, vec![note.lane])),
        }
    }
    for (k, (t, lanes)) in onsets.iter().enumerate() {
        let next = onsets.get(k + 1).map_or(LENGTH, |(n, _)| *n);
        let gap = next - t;
        let last = k + 1 == onsets.len();
        // Palm-muted when the next note comes soon; the last chord rings.
        let length = if last { LENGTH - t } else { gap.min(0.45) };
        let decay = if last { 1.2 } else if gap < 0.25 { 14.0 } else { 5.0 };
        let root = LOW_E * 2f32.powf(lanes.iter().map(|&l| PITCHES[l]).min().unwrap_or(0) as f32 / 12.0);
        let high = lanes.iter().map(|&l| PITCHES[l]).max().unwrap_or(0);
        let top = LOW_E * 2f32.powf(high as f32 / 12.0);
        guitar(&mut out, *t, length, decay, root, (lanes.len() > 1).then_some(top));
        bass(&mut out, *t, length, decay * 0.6, root / 2.0);
    }

    // The drums: the kick on 1 and 3 (every eighth in the solo and the last chorus), the snare on 2 and 4,
    // the hi-hat on the eighths. The intro's first bars keep only the hi-hat and the kick.
    for bar in 0..BARS - 1 {
        let p = part(bar);
        let start = bar as f64 * BAR;
        for eighth in 0..8 {
            let t = start + eighth as f64 * BEAT / 2.0;
            let double = matches!(p, Part::Solo | Part::Last);
            if eighth % 4 == 0 || (double && eighth % 2 == 1) || (p == Part::Chorus && eighth % 2 == 0) {
                kick(&mut out, t);
            }
            if eighth % 4 == 2 && (p != Part::Intro || bar >= 2) {
                snare(&mut out, t, &mut noise);
            }
            hat(&mut out, t, eighth % 2 == 0, &mut noise);
        }
        // A crash at the start of each part.
        if bar % 8 == 4 || bar == 0 {
            crash(&mut out, start, &mut noise);
        }
    }
    let end = (BARS - 1) as f64 * BAR;
    kick(&mut out, end);
    crash(&mut out, end, &mut noise);

    // Loud, then kept under 1 with a soft clip.
    for s in &mut out {
        *s = (*s * 1.4).tanh() * 0.7;
    }
    out
}

/// The span of `out` from `t` for `length` seconds, and the first sample's index.
fn span(out: &mut [f32], t: f64, length: f64) -> (usize, usize) {
    let from = ((t * RATE as f64) as usize).min(out.len());
    let to = (((t + length) * RATE as f64) as usize).min(out.len());
    (from, to)
}

/// A sawtooth in -1..1.
fn saw(phase: f32) -> f32 {
    2.0 * (phase - phase.floor()) - 1.0
}

/// A distorted power chord on `root` (root, fifth, octave; with `top`, its fifth too).
fn guitar(out: &mut [f32], t: f64, length: f64, decay: f64, root: f32, top: Option<f32>) {
    let (from, to) = span(out, t, length);
    let fifth = root * 1.498;
    let mut low = 0.0f32;
    for (k, s) in out[from..to].iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        let mut v = saw(root * x) + saw(fifth * x * 1.002) + 0.6 * saw(2.0 * root * x * 0.998);
        if let Some(top) = top {
            v += 0.7 * saw(top * 1.498 * x);
        }
        // The amp: driven hard, then rounded off.
        let driven = (v * 4.0).tanh();
        low += 0.22 * (driven - low);
        let envelope = (x * 400.0).min(1.0) * (-(x as f64) * decay).exp() as f32;
        // The end of a muted note fades out quickly rather than stopping dead.
        let tail = (((length as f32) - x) * 80.0).clamp(0.0, 1.0);
        *s += low * envelope * tail * 0.28;
    }
}

fn bass(out: &mut [f32], t: f64, length: f64, decay: f64, f: f32) {
    let (from, to) = span(out, t, length);
    for (k, s) in out[from..to].iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        let phase = f * x;
        // A triangle, a little dirty.
        let v = (4.0 * (phase - (phase + 0.5).floor()).abs() - 1.0) * 1.2;
        let envelope = (x * 300.0).min(1.0) * (-(x as f64) * decay).exp() as f32;
        let tail = (((length as f32) - x) * 60.0).clamp(0.0, 1.0);
        *s += v.tanh() * envelope * tail * 0.22;
    }
}

fn kick(out: &mut [f32], t: f64) {
    let (from, to) = span(out, t, 0.35);
    let mut phase = 0.0f32;
    for (k, s) in out[from..to].iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        let f = 45.0 + 110.0 * (-x * 30.0).exp();
        phase += f / RATE as f32;
        *s += (phase * std::f32::consts::TAU).sin() * (-x * 11.0).exp() * 0.55;
    }
}

fn snare(out: &mut [f32], t: f64, noise: &mut Rng) {
    let (from, to) = span(out, t, 0.25);
    for (k, s) in out[from..to].iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        let n = noise.unit() as f32 * 2.0 - 1.0;
        *s += n * (-x * 18.0).exp() * 0.32 + (x * 190.0 * std::f32::consts::TAU).sin() * (-x * 35.0).exp() * 0.25;
    }
}

fn hat(out: &mut [f32], t: f64, accent: bool, noise: &mut Rng) {
    let (from, to) = span(out, t, 0.06);
    let mut last = 0.0f32;
    for (k, s) in out[from..to].iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        let n = noise.unit() as f32 * 2.0 - 1.0;
        // Only the high of the noise: each sample less the last.
        let high = n - last;
        last = n;
        *s += high * (-x * 70.0).exp() * if accent { 0.07 } else { 0.045 };
    }
}

fn crash(out: &mut [f32], t: f64, noise: &mut Rng) {
    let (from, to) = span(out, t, 1.4);
    let mut last = 0.0f32;
    for (k, s) in out[from..to].iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        let n = noise.unit() as f32 * 2.0 - 1.0;
        let high = n - last;
        last = n;
        *s += high * (-x * 3.0).exp() * 0.06;
    }
}

/// A dull thud, when a note is missed: the string muted.
pub fn thud() -> Vec<f32> {
    let n = (0.12 * RATE as f64) as usize;
    let mut out = vec![0.0; n];
    for (k, s) in out.iter_mut().enumerate() {
        let x = k as f32 / RATE as f32;
        *s = (saw(70.0 * x) * 3.0).tanh() * (-x * 40.0).exp() * 0.25;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_makes_the_same_song() {
        let (a, b, c) = (Chart::new(42), Chart::new(42), Chart::new(43));
        assert_eq!(a.notes, b.notes);
        assert_ne!(a.notes, c.notes);
        // In time order, on the strings, within the song.
        assert!(a.notes.windows(2).all(|w| w[0].t <= w[1].t));
        assert!(a.notes.iter().all(|n| n.lane < LANES && n.t >= 0.0 && n.t < LENGTH));
        // Enough to play, not too many: a minute of metal.
        assert!((110..260).contains(&a.len()), "{} notes", a.len());
        // Every bar starts with a note.
        for bar in 0..BARS {
            let t = bar as f64 * BAR;
            assert!(a.notes.iter().any(|n| (n.t - t).abs() < 1e-9), "bar {bar}");
        }
    }

    #[test]
    fn the_levels_go_from_few_notes_to_many() {
        let [easy, medium, hard] = Level::ALL.map(|l| Chart::solo(l).len());
        assert!(easy < medium && medium < hard, "{easy} {medium} {hard}");
        // Always the same song for a level.
        assert_eq!(Chart::solo(Level::Hard).notes, Chart::solo(Level::Hard).notes);
        for level in Level::ALL {
            let chart = Chart::solo(level);
            assert!(chart.notes.windows(2).all(|w| w[0].t <= w[1].t));
            assert!(chart.notes.iter().all(|n| n.lane < LANES && n.t >= 0.0 && n.t < LENGTH));
        }
    }

    #[test]
    fn notes_are_judged_by_how_close_the_press_is() {
        let chart = Chart { notes: vec![Note { t: 1.0, lane: 0 }, Note { t: 2.0, lane: 1 }, Note { t: 3.0, lane: 2 }, Note { t: 3.0, lane: 3 }] };
        let mut play = Play::new(chart);
        assert_eq!(play.press(0, 1.02), Press::Hit(Judgment::Perfect));
        // Already played: nothing more to hit there, and the combo holds.
        assert_eq!(play.press(0, 1.03), Press::Stray);
        assert_eq!(play.combo, 1);
        assert_eq!(play.press(1, 1.88), Press::Hit(Judgment::Good));
        // The wrong string, or too early: nothing.
        assert_eq!(play.press(3, 2.5), Press::Stray);
        assert!(play.advance(2.9).is_empty());
        assert_eq!(play.press(2, 3.0), Press::Hit(Judgment::Perfect));
        // The other string of the chord goes by: missed.
        assert_eq!(play.advance(3.2), vec![3]);
        assert_eq!((play.hits, play.perfects, play.misses), (3, 2, 1));
        assert_eq!(play.score, 100 + 50 + 100);
        assert!(play.judged.iter().all(Option::is_some));
    }

    #[test]
    fn the_multiplier_grows_with_the_combo() {
        let notes = (0..45).map(|k| Note { t: k as f64 * 0.5, lane: 0 }).collect();
        let mut play = Play::new(Chart { notes });
        for k in 0..45 {
            play.press(0, k as f64 * 0.5);
        }
        assert_eq!(play.multiplier(), MAX_MULTIPLIER);
        // 10 notes ×1, 10 ×2, 10 ×3, 15 ×4 (the multiplier counts the note just hit).
        assert_eq!(play.score, 100 * (9 + 2 * 10 + 3 * 10 + 4 * 16));
        assert_eq!(play.best_combo, 45);
    }

    #[test]
    fn the_song_sounds_and_stays_in_bounds() {
        for chart in [Chart::new(7), Chart::solo(Level::Easy), Chart::solo(Level::Medium), Chart::solo(Level::Hard)] {
            let samples = render(&chart);
            assert_eq!(samples.len(), (LENGTH * RATE as f64) as usize);
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            let loud = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(loud > 0.3, "{loud}");
        }
    }
}
