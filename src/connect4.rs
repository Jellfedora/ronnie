//! Puissance 4: the grid, its rules, and the computer that plays against the player (a minimax with
//! alpha-beta pruning, deeper as the level rises). Online, floor keeps the grid; here it is drawn.

pub const COLS: usize = 7;
pub const ROWS: usize = 6;

/// Columns tried from the middle out: the best moves first, the pruning cuts more.
const ORDER: [usize; COLS] = [3, 2, 4, 1, 5, 0, 6];
/// Four in a row (more for a win found sooner).
const WIN: i32 = 1_000_000;

/// The grid: 0 empty, 1 or 2 the player's discs, from the bottom row (0) up.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Board {
    cells: [[u8; ROWS]; COLS],
    heights: [usize; COLS],
}

impl Board {
    /// The grid as floor sends it: a stack per column, from the bottom.
    pub fn from_columns(columns: &[Vec<u8>]) -> Self {
        let mut board = Board::default();
        for (c, column) in columns.iter().enumerate().take(COLS) {
            for &who in column.iter().take(ROWS) {
                board.play(c, who);
            }
        }
        board
    }

    pub fn get(&self, c: usize, r: usize) -> u8 {
        self.cells[c][r]
    }

    pub fn height(&self, c: usize) -> usize {
        self.heights[c]
    }

    pub fn can_play(&self, c: usize) -> bool {
        c < COLS && self.heights[c] < ROWS
    }

    /// The disc of `who` dropped in column `c`: the row it lands on (None: the column is full).
    pub fn play(&mut self, c: usize, who: u8) -> Option<usize> {
        if !self.can_play(c) {
            return None;
        }
        let r = self.heights[c];
        self.cells[c][r] = who;
        self.heights[c] += 1;
        Some(r)
    }

    fn undo(&mut self, c: usize) {
        self.heights[c] -= 1;
        self.cells[c][self.heights[c]] = 0;
    }

    pub fn full(&self) -> bool {
        self.heights.iter().all(|&h| h == ROWS)
    }

    #[cfg(test)]
    pub fn moves(&self) -> usize {
        self.heights.iter().sum()
    }

    /// Four discs in a row through (c, r), if any: the four cells, left to right (bottom to top).
    pub fn line(&self, c: usize, r: usize) -> Option<[(usize, usize); 4]> {
        let who = self.cells[c][r];
        if who == 0 {
            return None;
        }
        let at = |c: isize, r: isize| (0..COLS as isize).contains(&c) && (0..ROWS as isize).contains(&r) && self.cells[c as usize][r as usize] == who;
        for (dc, dr) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
            let (c0, r0) = (c as isize, r as isize);
            // Back to the first disc of the run, then four from there.
            let mut k = 0;
            while at(c0 - (k + 1) * dc, r0 - (k + 1) * dr) {
                k += 1;
            }
            let (sc, sr) = (c0 - k * dc, r0 - k * dr);
            if (0..4).all(|i| at(sc + i * dc, sr + i * dr)) {
                return Some(std::array::from_fn(|i| ((sc + i as isize * dc) as usize, (sr + i as isize * dr) as usize)));
            }
        }
        None
    }

    /// A line of four anywhere on the grid.
    #[cfg(test)]
    pub fn winner_line(&self) -> Option<[(usize, usize); 4]> {
        (0..COLS).flat_map(|c| (0..self.heights[c]).map(move |r| (c, r))).find_map(|(c, r)| self.line(c, r))
    }
}

/// How well the computer plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    Easy,
    #[default]
    Medium,
    Hard,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Easy, Level::Medium, Level::Hard];

    /// Moves looked ahead.
    fn depth(self) -> u32 {
        match self {
            Level::Easy => 2,
            Level::Medium => 4,
            Level::Hard => 8,
        }
    }
}

/// The column the computer (`who`) plays. `seed` breaks ties (and makes the easy level err).
pub fn best_move(board: &Board, who: u8, level: Level, seed: u64) -> Option<usize> {
    let mut board = board.clone();
    let mut rng = seed | 1;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let legal: Vec<usize> = ORDER.into_iter().filter(|&c| board.can_play(c)).collect();
    if legal.is_empty() {
        return None;
    }
    // The easy level plays at random one move in three (still taking a win it sees).
    if level == Level::Easy && next() % 3 == 0 {
        let win = legal.iter().copied().find(|&c| {
            let r = board.play(c, who).unwrap_or(0);
            let won = board.line(c, r).is_some();
            board.undo(c);
            won
        });
        return Some(win.unwrap_or(legal[(next() % legal.len() as u64) as usize]));
    }
    let depth = level.depth();
    let mut best = Vec::new();
    let mut best_score = i32::MIN;
    for &c in &legal {
        let r = board.play(c, who).unwrap_or(0);
        let score = if board.line(c, r).is_some() { WIN + depth as i32 } else { -negamax(&mut board, 3 - who, depth - 1, -WIN * 2, WIN * 2) };
        board.undo(c);
        if score > best_score {
            best_score = score;
            best.clear();
        }
        if score == best_score {
            best.push(c);
        }
    }
    Some(best[(next() % best.len() as u64) as usize])
}

fn negamax(board: &mut Board, who: u8, depth: u32, mut alpha: i32, beta: i32) -> i32 {
    if board.full() {
        return 0;
    }
    if depth == 0 {
        return evaluate(board, who);
    }
    let mut best = -WIN * 2;
    for c in ORDER {
        let Some(r) = board.play(c, who) else { continue };
        let score = if board.line(c, r).is_some() { WIN + depth as i32 } else { -negamax(board, 3 - who, depth - 1, -beta, -alpha) };
        board.undo(c);
        best = best.max(score);
        alpha = alpha.max(score);
        if alpha >= beta {
            break;
        }
    }
    best
}

/// The grid seen by `who`: its discs in the middle, and its rows of four still open, against the other's.
fn evaluate(board: &Board, who: u8) -> i32 {
    let mut score = 0;
    for r in 0..ROWS {
        score += match board.cells[3][r] {
            0 => 0,
            w if w == who => 3,
            _ => -3,
        };
    }
    for (dc, dr) in [(1isize, 0isize), (0, 1), (1, 1), (1, -1)] {
        for c in 0..COLS as isize {
            for r in 0..ROWS as isize {
                let (ec, er) = (c + 3 * dc, r + 3 * dr);
                if !(0..COLS as isize).contains(&ec) || !(0..ROWS as isize).contains(&er) {
                    continue;
                }
                let (mut mine, mut theirs) = (0, 0);
                for i in 0..4 {
                    match board.cells[(c + i * dc) as usize][(r + i * dr) as usize] {
                        0 => {}
                        w if w == who => mine += 1,
                        _ => theirs += 1,
                    }
                }
                score += match (mine, theirs) {
                    (3, 0) => 5,
                    (2, 0) => 2,
                    (0, 3) => -4,
                    (0, 2) => -1,
                    _ => 0,
                };
            }
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(moves: &[(usize, u8)]) -> Board {
        let mut b = Board::default();
        for &(c, who) in moves {
            b.play(c, who);
        }
        b
    }

    #[test]
    fn four_in_a_row_every_way() {
        let b = board(&[(0, 1), (1, 1), (2, 1), (3, 1)]);
        assert_eq!(b.line(2, 0), Some([(0, 0), (1, 0), (2, 0), (3, 0)]));
        let b = board(&[(5, 2), (5, 2), (5, 2), (5, 2)]);
        assert_eq!(b.line(5, 3), Some([(5, 0), (5, 1), (5, 2), (5, 3)]));
        // Rising, and falling.
        let b = board(&[(0, 1), (1, 2), (1, 1), (2, 2), (2, 2), (2, 1), (3, 2), (3, 2), (3, 2), (3, 1)]);
        assert_eq!(b.winner_line(), Some([(0, 0), (1, 1), (2, 2), (3, 3)]));
        let b = board(&[(6, 1), (5, 2), (5, 1), (4, 2), (4, 2), (4, 1), (3, 2), (3, 2), (3, 2), (3, 1)]);
        assert_eq!(b.line(6, 0), Some([(3, 3), (4, 2), (5, 1), (6, 0)]));
        assert_eq!(board(&[(0, 1), (1, 1), (2, 1), (3, 2)]).winner_line(), None);
    }

    #[test]
    fn columns_fill_up() {
        let mut b = Board::default();
        for k in 0..ROWS {
            assert_eq!(b.play(4, 1 + (k % 2) as u8), Some(k));
        }
        assert_eq!(b.play(4, 1), None);
        assert_eq!(Board::from_columns(&[vec![1, 2], vec![], vec![2]]).get(0, 1), 2);
    }

    #[test]
    fn the_computer_wins_and_blocks() {
        // Three of its own in a row: it plays the fourth.
        let b = board(&[(0, 2), (1, 2), (2, 2), (0, 1), (1, 1), (6, 1)]);
        for level in [Level::Medium, Level::Hard] {
            for seed in 1..20 {
                assert_eq!(best_move(&b, 2, level, seed), Some(3), "{level:?} wins");
            }
        }
        // The player's three: blocked.
        let b = board(&[(0, 1), (1, 1), (2, 1), (6, 2), (6, 2)]);
        for level in [Level::Medium, Level::Hard] {
            for seed in 1..20 {
                assert_eq!(best_move(&b, 2, level, seed), Some(3), "{level:?} blocks");
            }
        }
    }

    #[test]
    fn the_hard_level_beats_the_easy_one() {
        let mut wins = 0;
        for seed in 1..=4u64 {
            let mut b = Board::default();
            let mut who = if seed % 2 == 0 { 1 } else { 2 };
            loop {
                let level = if who == 1 { Level::Hard } else { Level::Easy };
                let c = best_move(&b, who, level, seed * 7919 + b.moves() as u64).unwrap();
                let r = b.play(c, who).unwrap();
                if b.line(c, r).is_some() {
                    wins += usize::from(who == 1);
                    break;
                }
                if b.full() {
                    break;
                }
                who = 3 - who;
            }
        }
        assert!(wins >= 3, "{wins} wins out of 4");
    }
}
