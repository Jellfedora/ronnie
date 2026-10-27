//! The minesweeper's rules: a grid of 16 × 16 with 40 mines, laid at the first click (never on it nor
//! around it, so that it opens a field). A cell shown without a mine around shows its neighbors too.

pub const COLS: usize = 16;
pub const ROWS: usize = 16;
pub const MINES: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cell {
    #[default]
    Hidden,
    Flagged,
    /// Shown: the mines around it.
    Open(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// No click yet: no mines laid.
    Fresh,
    Playing,
    Won,
    /// The mine clicked.
    Lost(usize, usize),
}

#[derive(Clone, Debug)]
pub struct Board {
    mines: [[bool; COLS]; ROWS],
    cells: [[Cell; COLS]; ROWS],
    pub state: State,
    /// Clicks that changed something (shown cells, flags).
    pub clicks: u32,
}

impl Default for Board {
    fn default() -> Self {
        Self { mines: [[false; COLS]; ROWS], cells: [[Cell::Hidden; COLS]; ROWS], state: State::Fresh, clicks: 0 }
    }
}

/// The cells around (c, r), in the grid.
fn around(c: usize, r: usize) -> impl Iterator<Item = (usize, usize)> {
    (-1i32..=1).flat_map(move |dr| (-1i32..=1).map(move |dc| (c as i32 + dc, r as i32 + dr))).filter(move |&(x, y)| (x, y) != (c as i32, r as i32) && (0..COLS as i32).contains(&x) && (0..ROWS as i32).contains(&y)).map(|(x, y)| (x as usize, y as usize))
}

impl Board {
    pub fn cell(&self, c: usize, r: usize) -> Cell {
        self.cells[r][c]
    }

    pub fn mine(&self, c: usize, r: usize) -> bool {
        self.mines[r][c]
    }

    pub fn over(&self) -> bool {
        matches!(self.state, State::Won | State::Lost(..))
    }

    /// The mines not flagged yet (below zero when flagged too many).
    pub fn left(&self) -> i32 {
        MINES as i32 - self.cells.iter().flatten().filter(|&&c| c == Cell::Flagged).count() as i32
    }

    /// The mines laid at random (`seed`), away from (c, r) and its neighbors.
    fn lay(&mut self, c: usize, r: usize, seed: u64) {
        let mut rng = seed | 1;
        let mut laid = 0;
        while laid < MINES {
            // xorshift64.
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let k = (rng % (COLS * ROWS) as u64) as usize;
            let (x, y) = (k % COLS, k / COLS);
            if self.mines[y][x] || (x.abs_diff(c) <= 1 && y.abs_diff(r) <= 1) {
                continue;
            }
            self.mines[y][x] = true;
            laid += 1;
        }
    }

    fn count(&self, c: usize, r: usize) -> u8 {
        around(c, r).filter(|&(x, y)| self.mines[y][x]).count() as u8
    }

    /// A click on (c, r): shows it (the first one lays the mines, with `seed`). On a number with as
    /// many flags around, shows the cells around not flagged.
    pub fn open(&mut self, c: usize, r: usize, seed: u64) {
        if self.over() {
            return;
        }
        if self.state == State::Fresh {
            self.lay(c, r, seed);
            self.state = State::Playing;
        }
        match self.cells[r][c] {
            Cell::Flagged => return,
            Cell::Open(n) => {
                let flags = around(c, r).filter(|&(x, y)| self.cells[y][x] == Cell::Flagged).count() as u8;
                let hidden: Vec<_> = around(c, r).filter(|&(x, y)| self.cells[y][x] == Cell::Hidden).collect();
                if n == 0 || flags != n || hidden.is_empty() {
                    return;
                }
                self.clicks += 1;
                for (x, y) in hidden {
                    self.show(x, y);
                }
            }
            Cell::Hidden => {
                self.clicks += 1;
                self.show(c, r);
            }
        }
        if !self.over() && self.cells.iter().flatten().filter(|c| matches!(c, Cell::Open(_))).count() == COLS * ROWS - MINES {
            self.state = State::Won;
        }
    }

    /// Shows (c, r), and around it while there are no mines around.
    fn show(&mut self, c: usize, r: usize) {
        if self.mines[r][c] {
            self.state = State::Lost(c, r);
            return;
        }
        let mut todo = vec![(c, r)];
        while let Some((x, y)) = todo.pop() {
            if self.cells[y][x] != Cell::Hidden {
                continue;
            }
            let n = self.count(x, y);
            self.cells[y][x] = Cell::Open(n);
            if n == 0 {
                todo.extend(around(x, y));
            }
        }
    }

    /// A flag put on (c, r), or taken off.
    pub fn flag(&mut self, c: usize, r: usize) {
        if self.state != State::Playing {
            return;
        }
        self.cells[r][c] = match self.cells[r][c] {
            Cell::Hidden => Cell::Flagged,
            Cell::Flagged => Cell::Hidden,
            Cell::Open(_) => return,
        };
        self.clicks += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_click_opens_a_field() {
        for seed in 1..50 {
            let mut b = Board::default();
            b.open(0, 0, seed);
            assert_eq!(b.state, State::Playing);
            assert_eq!(b.cell(0, 0), Cell::Open(0));
            assert_eq!(b.mines.iter().flatten().filter(|&&m| m).count(), MINES);
            assert!(around(0, 0).all(|(x, y)| !b.mine(x, y)));
        }
    }

    #[test]
    fn a_mine_loses_flags_protect() {
        let mut b = Board::default();
        b.open(8, 8, 7);
        let (x, y) = (0..COLS * ROWS).map(|k| (k % COLS, k / COLS)).find(|&(x, y)| b.mine(x, y)).unwrap();
        b.flag(x, y);
        assert_eq!(b.left(), MINES as i32 - 1);
        b.open(x, y, 0);
        assert_eq!(b.state, State::Playing);
        b.flag(x, y);
        b.open(x, y, 0);
        assert_eq!(b.state, State::Lost(x, y));
    }

    #[test]
    fn every_safe_cell_shown_wins() {
        let mut b = Board::default();
        b.open(3, 3, 42);
        for k in 0..COLS * ROWS {
            let (x, y) = (k % COLS, k / COLS);
            if !b.mine(x, y) {
                b.open(x, y, 0);
            }
        }
        assert_eq!(b.state, State::Won);
    }

    #[test]
    fn a_number_with_its_flags_shows_around() {
        let mut b = Board::default();
        b.open(8, 8, 11);
        // A number next to a mine: every mine around flagged, then a click on it.
        let (c, r) = (0..COLS * ROWS).map(|k| (k % COLS, k / COLS)).find(|&(x, y)| matches!(b.cell(x, y), Cell::Open(n) if n > 0) && around(x, y).any(|(a, d)| b.cell(a, d) == Cell::Hidden && !b.mine(a, d))).unwrap();
        let mines: Vec<_> = around(c, r).filter(|&(x, y)| b.mine(x, y)).collect();
        for (x, y) in mines {
            b.flag(x, y);
        }
        b.open(c, r, 0);
        assert!(around(c, r).all(|(x, y)| b.mine(x, y) || matches!(b.cell(x, y), Cell::Open(_))));
        assert_eq!(b.state, State::Playing);
    }
}
