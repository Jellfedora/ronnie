//! Larsen, in the games: against another player online (see live.rs and floor's `larsen`). Each one
//! sets up their gear in secret on their stage (an amp wall, drums, a mixing desk, a keyboard, a
//! guitar), then sends feedback in turn onto a cell of the other's stage: silence, or a hit; a piece hit
//! on all its cells fries. The first to fry all the other's gear wins. The lobby (the players online to
//! invite, the board of wins), the gear set up, the stages side by side, the chat beside.

use super::four::{bottom_button, game_header, level_name, players_ui, wins_ui, HomeGame, LobbyAction};
use super::*;
use crate::connect4::Level;
use crate::live::{Piece, Stage};

/// The stage's side, and the gear: a piece of each length (as floor has them).
const SIZE: u8 = 10;
const GEAR: [u8; 5] = [5, 4, 3, 3, 2];

enum Mode {
    Lobby,
    /// Against the computer.
    Ai(Box<AiGame>),
    /// The match of `live`.
    Online,
}

pub(super) struct Larsen {
    mode: Mode,
    /// The computer's level, and whether the player shoots first in the next game against it (they take turns).
    level: Level,
    you_start: bool,
    /// The gear being set up: a piece per length of `GEAR` (None: not laid yet).
    setup: [Option<Piece>; 5],
    /// The piece in hand (its index in `GEAR`), and which way it lies.
    hand: Option<usize>,
    horizontal: bool,
    /// The match the setup is for: another one (a rematch) is set up afresh.
    setup_for: Option<String>,
    /// When the board of wins was last fetched.
    pub(super) board_at: Option<f64>,
    /// The chat with the other player, beside the stages.
    chat: super::chat::Chat,
    /// The match and its moves last drawn: a new shot sparks where it fell (on whose stage, when).
    seen: Option<(String, u32)>,
    spark: Option<((u8, u8), bool, f64)>,
}

impl Default for Larsen {
    fn default() -> Self {
        Self { mode: Mode::Lobby, level: Level::Medium, you_start: true, setup: [None; 5], hand: Some(0), horizontal: true, setup_for: None, board_at: None, chat: Default::default(), seen: None, spark: None }
    }
}

impl Larsen {
    /// A match accepted or started: its page.
    pub(super) fn start_online(&mut self) {
        self.mode = Mode::Online;
        self.seen = None;
        self.spark = None;
    }

    /// Another game's match takes the place of this one (they share floor's match): to the lobby.
    pub(super) fn leave_online(&mut self) {
        self.mode = Mode::Lobby;
    }

    /// The gear to set up again (a new match, a new game).
    fn reset_setup(&mut self, id: &str) {
        self.setup = [None; 5];
        self.hand = Some(0);
        self.horizontal = true;
        self.setup_for = Some(id.to_owned());
        self.spark = None;
    }

    fn laid(&self) -> Vec<Piece> {
        self.setup.iter().flatten().copied().collect()
    }
}

/// Whether `piece` fits among `others`: on the stage, touching none (not even by a corner).
fn fits(piece: &Piece, others: &[Piece]) -> bool {
    let inside = piece.cells().all(|(x, y)| x < SIZE && y < SIZE);
    inside && !piece.cells().any(|(x, y)| others.iter().flat_map(|o| o.cells()).any(|(ox, oy)| (x as i16 - ox as i16).abs() <= 1 && (y as i16 - oy as i16).abs() <= 1))
}

/// All the gear set up at random.
fn random_setup() -> [Option<Piece>; 5] {
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
    let mut next = |n: u8| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as u8
    };
    // Rarely, the last pieces find no room: all of it again.
    loop {
        let mut laid: Vec<Piece> = Vec::new();
        for &length in &GEAR {
            for _ in 0..200 {
                let horizontal = next(2) == 0;
                let (w, h) = if horizontal { (SIZE - length + 1, SIZE) } else { (SIZE, SIZE - length + 1) };
                let piece = Piece { x: next(w), y: next(h), length, horizontal, fried: false };
                if fits(&piece, &laid) {
                    laid.push(piece);
                    break;
                }
            }
        }
        if laid.len() == GEAR.len() {
            let mut out = [None; 5];
            for (k, piece) in laid.into_iter().enumerate() {
                out[k] = Some(piece);
            }
            return out;
        }
    }
}

/// A small random generator (xorshift), for the computer.
struct Dice(u64);

impl Dice {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// Whether every cell of `piece` took feedback.
fn is_fried(piece: &Piece, shots: &[(u8, u8, bool)]) -> bool {
    piece.cells().all(|c| shots.iter().any(|s| (s.0, s.1) == c))
}

/// Feedback at (x, y) on `gear`: the shot, a hit or not.
fn shot_at(gear: &[Piece], (x, y): (u8, u8)) -> (u8, u8, bool) {
    (x, y, gear.iter().any(|p| p.cells().any(|c| c == (x, y))))
}

/// Where the computer sends its feedback on the player's stage: `shots`, its shots so far; `fried`,
/// the player's pieces fried (they are announced). Easy: at random, sometimes beside a hit. Medium:
/// as a player hunts, around a hit and along the line found, else one cell in two. Hard: the cell the
/// most placements of the gear left go through (none on silence, none touching a fried piece), the
/// placements through a hit not fried yet counting far more.
fn ai_shot(level: Level, shots: &[(u8, u8, bool)], fried: &[Piece], seed: u64) -> (u8, u8) {
    let mut dice = Dice::new(seed);
    let shot = |c: (u8, u8)| shots.iter().any(|s| (s.0, s.1) == c);
    let fried_cells: Vec<(u8, u8)> = fried.iter().flat_map(|p| p.cells().collect::<Vec<_>>()).collect();
    // Beside a fried piece there is nothing (pieces don't touch).
    let empty_near = |(x, y): (u8, u8)| fried_cells.iter().any(|&(fx, fy)| (x as i16 - fx as i16).abs() <= 1 && (y as i16 - fy as i16).abs() <= 1);
    let all: Vec<(u8, u8)> = (0..SIZE).flat_map(|y| (0..SIZE).map(move |x| (x, y))).collect();
    let mut open: Vec<(u8, u8)> = all.iter().copied().filter(|&c| !shot(c) && !empty_near(c)).collect();
    if open.is_empty() {
        open = all.iter().copied().filter(|&c| !shot(c)).collect();
    }
    // Hits on a piece not fried yet: it is there, around them.
    let pending: Vec<(u8, u8)> = shots.iter().filter(|s| s.2 && !fried_cells.contains(&(s.0, s.1))).map(|s| (s.0, s.1)).collect();
    let around = |cells: &[(u8, u8)]| -> Vec<(u8, u8)> {
        let mut out: Vec<(u8, u8)> = Vec::new();
        for &(x, y) in cells {
            for (dx, dy) in [(1i16, 0i16), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x as i16 + dx, y as i16 + dy);
                if (0..SIZE as i16).contains(&nx) && (0..SIZE as i16).contains(&ny) {
                    let c = (nx as u8, ny as u8);
                    if open.contains(&c) && !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
        }
        out
    };
    let pick = |cells: &[(u8, u8)], dice: &mut Dice| cells[dice.below(cells.len())];
    match level {
        Level::Easy => {
            let near = around(&pending);
            if !near.is_empty() && dice.below(2) == 0 { pick(&near, &mut dice) } else { pick(&open, &mut dice) }
        }
        Level::Medium => {
            if !pending.is_empty() {
                // Two hits in a row: along their line first.
                let row = pending.len() > 1 && pending.iter().all(|c| c.1 == pending[0].1);
                let column = pending.len() > 1 && pending.iter().all(|c| c.0 == pending[0].0);
                let near = around(&pending);
                let along: Vec<(u8, u8)> = near.iter().copied().filter(|c| (row && c.1 == pending[0].1) || (column && c.0 == pending[0].0)).collect();
                if !along.is_empty() {
                    return pick(&along, &mut dice);
                }
                if !near.is_empty() {
                    return pick(&near, &mut dice);
                }
            }
            // The smallest piece takes two cells: one cell in two finds them all.
            let half: Vec<(u8, u8)> = open.iter().copied().filter(|c| (c.0 + c.1) % 2 == 0).collect();
            pick(if half.is_empty() { &open } else { &half }, &mut dice)
        }
        Level::Hard => {
            let mut left: Vec<u8> = GEAR.to_vec();
            for p in fried {
                if let Some(k) = left.iter().position(|&l| l == p.length) {
                    left.remove(k);
                }
            }
            let silence = |c: (u8, u8)| shots.iter().any(|s| (s.0, s.1) == c && !s.2);
            let mut weight = [[0u32; SIZE as usize]; SIZE as usize];
            for &length in &left {
                for horizontal in [true, false] {
                    for y in 0..SIZE {
                        for x in 0..SIZE {
                            let piece = Piece { x, y, length, horizontal, fried: false };
                            let cells: Vec<(u8, u8)> = piece.cells().collect();
                            if cells.iter().any(|&(cx, cy)| cx >= SIZE || cy >= SIZE || silence((cx, cy)) || fried_cells.contains(&(cx, cy)) || empty_near((cx, cy))) {
                                continue;
                            }
                            let through = cells.iter().filter(|c| pending.contains(c)).count() as u32;
                            if !pending.is_empty() && through == 0 {
                                continue;
                            }
                            for &(cx, cy) in &cells {
                                if !shot((cx, cy)) {
                                    weight[cy as usize][cx as usize] += 1 + 40 * through;
                                }
                            }
                        }
                    }
                }
            }
            let best = open.iter().map(|&(x, y)| weight[y as usize][x as usize]).max().unwrap_or(0);
            let top: Vec<(u8, u8)> = open.iter().copied().filter(|&(x, y)| weight[y as usize][x as usize] == best).collect();
            pick(if best == 0 { &open } else { &top }, &mut dice)
        }
    }
}

/// A game against the computer: the player's gear (empty until set up) and the computer's, the shots
/// on each, whose turn, who won.
struct AiGame {
    level: Level,
    gear: Vec<Piece>,
    theirs: Vec<Piece>,
    /// The computer's shots on the player's stage, the player's on the computer's.
    on_me: Vec<(u8, u8, bool)>,
    on_them: Vec<(u8, u8, bool)>,
    your_turn: bool,
    /// The player won (true) or lost.
    winner: Option<bool>,
    /// When the computer sends its feedback.
    ai_at: Option<f64>,
}

impl AiGame {
    /// The stages as the player sees them (as floor would show them).
    fn stage(&self) -> crate::live::Stage {
        let standing = |gear: &[Piece], shots: &[(u8, u8, bool)]| gear.iter().filter(|p| !is_fried(p, shots)).count() as u8;
        crate::live::Stage {
            placed: crate::live::Placed { you: !self.gear.is_empty(), them: true },
            gear: (!self.gear.is_empty()).then(|| self.gear.iter().map(|p| Piece { fried: is_fried(p, &self.on_me), ..*p }).collect()),
            received: self.on_me.clone(),
            fired: self.on_them.clone(),
            fried: self.theirs.iter().filter(|p| is_fried(p, &self.on_them)).copied().collect(),
            standing: crate::live::Standing { you: if self.gear.is_empty() { GEAR.len() as u8 } else { standing(&self.gear, &self.on_me) }, them: standing(&self.theirs, &self.on_them) },
        }
    }
}

/// The stage's colors: its boards and their joints, the gear, feedback (a hit), silence (a miss),
/// what fried.
struct Look {
    boards: Color32,
    joints: Color32,
    gear: Color32,
    cone: Color32,
    hit: Color32,
    silence: Color32,
    fried: Color32,
}

impl Look {
    fn of(theme: &Theme) -> Self {
        Self {
            boards: theme.chrome_bg,
            joints: theme.text_muted.gamma_multiply(0.18),
            gear: theme.text.gamma_multiply(0.72),
            cone: theme.bg.gamma_multiply(0.85),
            hit: theme.ansi[3],
            silence: theme.text_muted.gamma_multiply(0.55),
            fried: theme.ansi[1],
        }
    }
}

/// A stage in `area` (square, its letters and numbers around, its title above): its rect and a cell's side.
fn stage_rect(area: Rect) -> (Rect, f32) {
    let side = (area.width().min(area.height()) - 22.0).max(100.0);
    let cell = (side / SIZE as f32).floor();
    let stage = Rect::from_min_size(Pos2::new(area.center().x - cell * SIZE as f32 / 2.0 + 11.0, area.min.y + 22.0), Vec2::splat(cell * SIZE as f32));
    (stage, cell)
}

fn cell_rect(stage: Rect, cell: f32, (x, y): (u8, u8)) -> Rect {
    Rect::from_min_size(stage.min + Vec2::new(x as f32 * cell, y as f32 * cell), Vec2::splat(cell))
}

/// The stage's boards (a joint across each row, staggered ones between cells), the letters A to J
/// above and the numbers 1 to 10 on the left; `title` over it; `live`: the edge lit, its turn.
fn paint_stage(painter: &egui::Painter, stage: Rect, cell: f32, title: &str, live: bool, look: &Look, theme: &Theme) {
    painter.text(Pos2::new(stage.center().x, stage.min.y - 40.0), Align2::CENTER_CENTER, title, FontId::proportional(14.0), if live { theme.text } else { theme.text_muted });
    painter.rect_filled(stage, 6.0, look.boards);
    let joint = Stroke::new(1.0, look.joints);
    for y in 1..SIZE {
        let d = y as f32 * cell;
        painter.line_segment([Pos2::new(stage.min.x, stage.min.y + d), Pos2::new(stage.max.x, stage.min.y + d)], joint);
    }
    // The boards' ends, staggered from a row to the next: a floor of planks rather than a grid.
    for y in 0..SIZE {
        for x in 1..SIZE {
            let d = x as f32 * cell;
            let top = stage.min.y + y as f32 * cell;
            let at = if (x + y) % 2 == 0 { 0.0 } else { cell * 0.5 };
            painter.line_segment([Pos2::new(stage.min.x + d, top + at), Pos2::new(stage.min.x + d, top + at + cell * 0.5)], joint);
        }
    }
    let edge = if live { Stroke::new(1.8, theme.accent) } else { Stroke::new(1.0, theme.tab_hover) };
    painter.rect_stroke(stage, 6.0, edge, egui::StrokeKind::Outside);
    let font = FontId::proportional((cell * 0.38).clamp(9.0, 12.0));
    for k in 0..SIZE {
        let mid = k as f32 * cell + cell / 2.0;
        painter.text(Pos2::new(stage.min.x + mid, stage.min.y - 10.0), Align2::CENTER_CENTER, ((b'A' + k) as char).to_string(), font.clone(), theme.text_muted);
        painter.text(Pos2::new(stage.min.x - 11.0, stage.min.y + mid), Align2::CENTER_CENTER, (k + 1).to_string(), font.clone(), theme.text_muted);
    }
}

/// A piece of gear: a cabinet from its first cell to its last, a speaker in each cell; fried, charred red.
fn paint_piece(painter: &egui::Painter, stage: Rect, cell: f32, piece: &Piece, fill: Color32, look: &Look, edge: Option<Stroke>) {
    let last = if piece.horizontal { (piece.x + piece.length - 1, piece.y) } else { (piece.x, piece.y + piece.length - 1) };
    let cabinet = cell_rect(stage, cell, (piece.x, piece.y)).union(cell_rect(stage, cell, last)).shrink(cell * 0.1);
    painter.rect_filled(cabinet, cell * 0.14, fill);
    for c in piece.cells() {
        let at = cell_rect(stage, cell, c).center();
        painter.circle_filled(at, cell * 0.27, look.cone);
        painter.circle_stroke(at, cell * 0.27, Stroke::new(1.0, fill.gamma_multiply(0.6)));
        painter.circle_filled(at, cell * 0.08, fill.gamma_multiply(0.8));
    }
    if let Some(edge) = edge {
        painter.rect_stroke(cabinet, cell * 0.14, edge, egui::StrokeKind::Inside);
    }
}

/// A bolt of lightning centered on `c`, `h` high.
fn bolt(painter: &egui::Painter, c: Pos2, h: f32, color: Color32) {
    let p = |x: f32, y: f32| c + Vec2::new(x * h, y * h);
    // Two triangles overlapping in the middle (egui fills convex shapes only): the zigzag.
    painter.add(egui::Shape::convex_polygon(vec![p(0.16, -0.5), p(0.08, 0.08), p(-0.26, 0.08)], color, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![p(-0.08, -0.08), p(0.26, -0.08), p(-0.16, 0.5)], color, Stroke::NONE));
}

/// A shot: a bolt when it hit (feedback!), a faint ring when it fell in silence.
fn paint_shot(painter: &egui::Painter, stage: Rect, cell: f32, (x, y, hit): (u8, u8, bool), look: &Look) {
    let c = cell_rect(stage, cell, (x, y)).center();
    if hit {
        painter.circle_filled(c, cell * 0.36, look.hit.gamma_multiply(0.22));
        bolt(painter, c, cell * 0.72, look.hit);
    } else {
        painter.circle_filled(c, cell * 0.09, look.silence);
        painter.circle_stroke(c, cell * 0.24, Stroke::new(1.0, look.silence.gamma_multiply(0.6)));
    }
}

/// A shot just fallen: sparks from it when it hit, a ring spreading when it didn't.
fn paint_spark(painter: &egui::Painter, stage: Rect, cell: f32, at: (u8, u8), hit: bool, age: f64, look: &Look) {
    let p = (age / 0.7).clamp(0.0, 1.0) as f32;
    let c = cell_rect(stage, cell, at).center();
    let e = super::motion::out_cubic(p);
    if hit {
        let color = look.hit.gamma_multiply(1.0 - p);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::TAU / 8.0 + 0.3;
            let dir = Vec2::new(a.cos(), a.sin());
            painter.line_segment([c + dir * cell * (0.3 + 0.5 * e), c + dir * cell * (0.45 + 0.75 * e)], Stroke::new(2.0, color));
        }
    } else {
        painter.circle_stroke(c, cell * (0.3 + 0.9 * e), Stroke::new(2.0, look.silence.gamma_multiply(1.0 - p)));
    }
}

/// The cell under the pointer, if it is over the stage.
fn hovered_cell(resp: &egui::Response, stage: Rect, cell: f32) -> Option<(u8, u8)> {
    let p = resp.hover_pos().filter(|p| stage.contains(*p))?;
    let x = ((p.x - stage.min.x) / cell).floor() as i32;
    let y = ((p.y - stage.min.y) / cell).floor() as i32;
    ((0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y)).then_some((x as u8, y as u8))
}

/// An amp, struck by a bolt.
pub(super) fn paint_larsen_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let cabinet = Rect::from_center_size(c + Vec2::new(-1.5 * s, 1.0 * s), Vec2::new(14.0 * s, 16.0 * s));
    painter.rect_filled(cabinet, 2.5 * s, theme.text.gamma_multiply(0.8));
    painter.circle_filled(cabinet.center() + Vec2::new(0.0, 2.0 * s), 4.6 * s, theme.bg);
    painter.circle_filled(cabinet.center() + Vec2::new(0.0, 2.0 * s), 1.6 * s, theme.text.gamma_multiply(0.8));
    for k in 0..3 {
        painter.circle_filled(Pos2::new(cabinet.min.x + (3.0 + 2.6 * k as f32) * s, cabinet.min.y + 2.6 * s), 0.8 * s, theme.bg);
    }
    bolt(painter, c + Vec2::new(6.5 * s, -5.0 * s), 12.0 * s, theme.accent);
}

impl App {
    /// Larsen, on the home page.
    pub(super) fn open_larsen(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Larsen;
    }

    /// The page: the lobby, a game against the computer, or the match online.
    pub(super) fn larsen_page(&mut self, ui: &mut Ui, rect: Rect) {
        let online = matches!(self.larsen.mode, Mode::Online) && self.versus.game.as_ref().is_some_and(|g| g.larsen() && !g.over);
        let in_game = online || matches!(&self.larsen.mode, Mode::Ai(game) if game.winner.is_none() && !game.gear.is_empty());
        if in_game {
            self.count_round(HomeGame::Larsen);
        } else if matches!(self.larsen.mode, Mode::Lobby)
            && let Some(p) = self.game_locked(HomeGame::Larsen)
        {
            return self.locked_page(ui, rect, HomeGame::Larsen, &p);
        }
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.larsen_name, 40.0, theme.accent, 1.0);
        // Back: from a game to the lobby, from the lobby home (not during a match online: give up first).
        if !online {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                match self.larsen.mode {
                    Mode::Lobby => self.home_game = false,
                    Mode::Online => {
                        self.versus.unwatch("larsen");
                        self.larsen.mode = Mode::Lobby;
                    }
                    Mode::Ai(_) => self.larsen.mode = Mode::Lobby,
                }
            }
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
        match self.larsen.mode {
            Mode::Lobby => self.larsen_lobby(ui, body, now),
            Mode::Ai(_) => self.larsen_ai(ui, body, now),
            Mode::Online => self.larsen_online(ui, body, now),
        }
    }

    fn larsen_lobby(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        // The board of wins, fetched again every half minute while the lobby shows.
        if pseudo.is_some() && self.larsen.board_at.is_none_or(|at| now - at > 30.0) {
            self.larsen.board_at = Some(now);
            self.versus.fetch_board("larsen");
        }
        let width = body.width().min(820.0);
        let wide = width >= 640.0;
        let area = Rect::from_min_size(Pos2::new(body.center().x - width / 2.0, body.min.y), Vec2::new(width, body.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
        let card_w = if wide { (width - 16.0) / 2.0 } else { width };
        let card = |ui: &mut Ui, w: f32, add: &mut dyn FnMut(&mut Ui)| {
            Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(18.0).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(w - 38.0);
                    ui.set_max_width(w - 38.0);
                    add(ui);
                });
            });
        };
        // Against the computer: its level, play.
        let level = self.larsen.level;
        let (mut picked, mut play) = (None, false);
        let mut ai = |ui: &mut Ui| {
            ui.label(egui::RichText::new(t.four_vs_ai).size(16.0).strong());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                for l in Level::ALL {
                    let b = egui::Button::selectable(l == level, egui::RichText::new(level_name(t, l)).size(13.0)).min_size(Vec2::new(86.0, 28.0)).corner_radius(6.0);
                    if ui.add(b).clicked() {
                        picked = Some(l);
                    }
                }
            });
            ui.add_space(16.0);
            let button = egui::Button::new(egui::RichText::new(t.four_play).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
            let (at, _) = ui.allocate_exact_size(Vec2::new(140.0, 34.0), Sense::hover());
            if ui.put(at, button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                play = true;
            }
        };
        let look = Look::of(&theme);
        let mut rules = |ui: &mut Ui| {
            ui.label(egui::RichText::new(t.larsen_rules_title).size(16.0).strong());
            ui.add_space(10.0);
            ui.label(egui::RichText::new(t.larsen_rules).size(13.0).color(theme.text_muted));
            ui.add_space(12.0);
            let names: Vec<&str> = t.larsen_pieces.split('|').collect();
            for (k, length) in GEAR.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(5.0 * 16.0, 16.0), Sense::hover());
                    let row = Rect::from_min_size(rect.min, Vec2::new(*length as f32 * 16.0, 16.0));
                    let piece = Piece { x: 0, y: 0, length: *length, horizontal: true, fried: false };
                    paint_piece(ui.painter(), row, 16.0, &piece, look.gear, &look, None);
                    ui.label(egui::RichText::new(names.get(k).copied().unwrap_or_default()).size(12.5));
                });
            }
        };
        let live = &self.versus;
        let mut action = None;
        let mut players = |ui: &mut Ui| {
            action = players_ui(ui, live, t.larsen_vs_player, pseudo.as_deref(), t, &theme);
            wins_ui(ui, live, "larsen", t.four_wins, pseudo.as_deref(), t, &theme);
        };
        egui::ScrollArea::vertical().id_salt("larsen-lobby").auto_shrink([false, false]).show(&mut child, |ui| {
            if wide {
                ui.horizontal_top(|ui| {
                    card(ui, card_w, &mut ai);
                    ui.add_space(16.0);
                    card(ui, card_w, &mut players);
                });
            } else {
                card(ui, card_w, &mut ai);
                ui.add_space(16.0);
                card(ui, card_w, &mut players);
            }
            ui.add_space(16.0);
            card(ui, width, &mut rules);
        });
        if let Some(l) = picked {
            self.larsen.level = l;
        }
        if play {
            self.new_larsen_ai();
        }
        match action {
            Some(LobbyAction::Invite(name)) => self.versus.invite_to(&name, "larsen"),
            Some(LobbyAction::Cancel) => self.versus.cancel(),
            Some(LobbyAction::Pseudo) => self.ask_pseudo(),
            _ => {}
        }
    }

    /// A game against the computer: its gear set up at random, the player's to set up; the first shot
    /// alternates from a game to the next.
    fn new_larsen_ai(&mut self) {
        // Played enough this half-day of work: to the lobby, which says why.
        if self.game_locked(HomeGame::Larsen).is_some() {
            self.larsen.mode = Mode::Lobby;
            return;
        }
        let theirs: Vec<Piece> = random_setup().iter().flatten().copied().collect();
        let your_turn = self.larsen.you_start;
        self.larsen.you_start = !self.larsen.you_start;
        self.larsen.reset_setup("ai");
        self.larsen.mode = Mode::Ai(Box::new(AiGame { level: self.larsen.level, gear: Vec::new(), theirs, on_me: Vec::new(), on_them: Vec::new(), your_turn, winner: None, ai_at: None }));
    }

    fn larsen_ai(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let look = Look::of(&theme);
        let Mode::Ai(game) = &mut self.larsen.mode else { return };
        // The computer's turn: its feedback after a pause.
        if game.winner.is_none() && !game.gear.is_empty() && !game.your_turn {
            let at = *game.ai_at.get_or_insert(now + 0.8);
            if now >= at {
                let fried: Vec<Piece> = game.gear.iter().filter(|p| is_fried(p, &game.on_me)).copied().collect();
                let target = ai_shot(game.level, &game.on_me, &fried, (now * 1000.0) as u64 ^ 0x9e37_79b9_7f4a_7c15);
                game.on_me.push(shot_at(&game.gear, target));
                self.larsen.spark = Some((target, false, now));
                game.ai_at = None;
                if game.gear.iter().all(|p| is_fried(p, &game.on_me)) {
                    game.winner = Some(false);
                } else {
                    game.your_turn = true;
                }
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let stage = game.stage();
        let (level, your_turn, winner) = (game.level, game.your_turn, game.winner);
        let computer = t.four_computer;
        let (last_mine, fried_now) = last_shot(&self.larsen.spark, &stage, now);
        let (status, color) = match winner {
            Some(true) => (t.larsen_won.to_owned(), theme.ansi[2]),
            Some(false) => (t.larsen_lost.to_owned(), theme.ansi[1]),
            None if stage.gear.is_none() => (t.larsen_setup.to_owned(), theme.text),
            None if fried_now => (t.larsen_fried.to_owned(), theme.ansi[3]),
            None if last_mine.is_some_and(|s| s.2) => (t.larsen_hit.to_owned(), theme.ansi[3]),
            None if last_mine.is_some() => (t.larsen_silence.replace("{n}", computer), theme.text_muted),
            None if your_turn => (t.larsen_your_turn.to_owned(), theme.text),
            None => (t.larsen_their_turn.replace("{n}", computer), theme.text_muted),
        };
        let you_name = crate::floor::pseudo(self.config.settings.floor.as_ref()).unwrap_or(t.game_you).to_owned();
        let names = [you_name, format!("{computer} · {}", level_name(t, level))];
        let turn = (winner.is_none() && stage.gear.is_some()).then_some(if your_turn { 1 } else { 2 });
        let area = game_header(ui, body, &status, color, &names, turn, winner.is_some(), &theme);
        if stage.gear.is_none() {
            if let Some(gear) = self.larsen_setup(ui, area, &look, &theme, t)
                && let Mode::Ai(game) = &mut self.larsen.mode
            {
                game.gear = gear;
            }
            return;
        }
        let can_shoot = winner.is_none() && your_turn;
        if let Some(at) = self.larsen_stages(ui, area, &stage, computer, can_shoot, now)
            && let Mode::Ai(game) = &mut self.larsen.mode
        {
            game.on_them.push(shot_at(&game.theirs, at));
            self.larsen.spark = Some((at, true, now));
            if game.theirs.iter().all(|p| is_fried(p, &game.on_them)) {
                game.winner = Some(true);
            } else {
                game.your_turn = false;
            }
        }
        if winner.is_some() && bottom_button(ui, body, t.four_again, &theme, true) {
            self.new_larsen_ai();
        }
    }

    fn larsen_online(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let look = Look::of(&theme);
        let Some(game) = self.versus.game.clone().filter(|g| g.larsen()) else {
            // Waiting for floor's first answer (an invitation just accepted).
            let at = body.center();
            if let Some(e) = &self.versus.refused {
                ui.painter().text(at, Align2::CENTER_CENTER, e, FontId::proportional(15.0), theme.ansi[1]);
            } else {
                ui.painter().text(at, Align2::CENTER_CENTER, t.four_connecting, FontId::proportional(15.0), theme.text_muted);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        };
        let stage = game.stage.clone().unwrap_or_default();
        // Another match (a rematch): the gear to set up again.
        if self.larsen.setup_for.as_deref() != Some(&game.id) {
            self.larsen.reset_setup(&game.id);
        }
        // A shot not seen yet: it sparks where it fell (the player's, on the right; the other's, on the left).
        let seen = (game.id.clone(), game.moves);
        if self.larsen.seen.as_ref() != Some(&seen) {
            if self.larsen.seen.as_ref().is_some_and(|(id, moves)| *id == game.id && game.moves > *moves)
                && let Some([x, y]) = game.last
            {
                // Whose shot: after it, the turn passed to the other (or the game ended on it).
                let mine = if game.over { game.winner == Some(game.you) } else { game.turn != game.you };
                self.larsen.spark = Some(((x as u8, y as u8), mine, now));
            }
            self.larsen.seen = Some(seen);
        }
        let them = game.them().unwrap_or(t.game_anonymous).to_owned();
        let (last_mine, fried_now) = last_shot(&self.larsen.spark, &stage, now);
        let (status, color) = match game.winner {
            Some(w) if w == game.you && game.reason.as_deref() == Some("forfeit") => (t.four_forfeit_them.replace("{n}", &them), theme.ansi[2]),
            Some(w) if w == game.you => (t.larsen_won.to_owned(), theme.ansi[2]),
            Some(_) if game.reason.as_deref() == Some("forfeit") => (t.four_forfeit_you.to_owned(), theme.text_muted),
            Some(_) => (t.larsen_lost.to_owned(), theme.ansi[1]),
            None if !stage.placed.you => (t.larsen_setup.to_owned(), theme.text),
            None if !stage.placed.them => (t.larsen_wait_setup.replace("{n}", &them), theme.text_muted),
            None if fried_now => (t.larsen_fried.to_owned(), theme.ansi[3]),
            None if last_mine.is_some_and(|s| s.2) => (t.larsen_hit.to_owned(), theme.ansi[3]),
            None if last_mine.is_some() => (t.larsen_silence.replace("{n}", &them), theme.text_muted),
            None if game.turn == game.you => (t.larsen_your_turn.to_owned(), theme.text),
            None => (t.larsen_their_turn.replace("{n}", &them), theme.text_muted),
        };
        let names = [0, 1].map(|k| if k + 1 == game.you as usize { t.game_you.to_owned() } else { game.players[k].clone().unwrap_or_else(|| t.game_anonymous.to_owned()) });
        let playing = stage.placed.you && stage.placed.them;
        let turn = (!game.over && playing).then_some(game.turn);
        // The chat on the right, the stages in what's left.
        let chat_w = (body.width() * 0.26).clamp(200.0, 280.0);
        let chat_area = Rect::from_min_max(Pos2::new(body.max.x - chat_w, body.min.y + 64.0), Pos2::new(body.max.x, body.max.y - 16.0));
        let body = Rect::from_min_max(body.min, Pos2::new(chat_area.min.x - 16.0, body.max.y));
        let area = game_header(ui, body, &status, color, &names, turn, game.over, &theme);
        let lines: Vec<super::chat::Line> = game.chat.iter().map(|(from, text, mine)| super::chat::Line { from: from.as_deref().unwrap_or(t.game_anonymous), text, mine: *mine, age: None }).collect();
        if let Some(text) = self.larsen.chat.ui(ui, chat_area, &lines, true, true, "larsen", &theme, t) {
            self.versus.say(&text);
        }
        if !stage.placed.you && !game.over {
            if let Some(gear) = self.larsen_setup(ui, area, &look, &theme, t) {
                self.versus.gear(gear);
            }
        } else if let Some(at) = self.larsen_stages(ui, area, &stage, &them, playing && !game.over && game.turn == game.you, now) {
            self.larsen.spark = Some((at, true, now));
            self.larsen.seen = Some((game.id.clone(), game.moves + 1));
            self.versus.shoot(at.0, at.1);
        }
        if !game.over {
            if bottom_button(ui, body, t.four_leave, &theme, false) {
                self.versus.leave();
            }
        } else {
            if game.rematch.them && !game.rematch.you {
                ui.painter().text(Pos2::new(body.center().x, body.max.y - 74.0), Align2::CENTER_CENTER, t.four_rematch_them.replace("{n}", &them), FontId::proportional(13.5), theme.accent);
            }
            let label = if game.rematch.you { t.four_rematch_asked } else { t.four_rematch };
            if bottom_button(ui, body, label, &theme, !game.rematch.you) && !game.rematch.you {
                if self.game_locked(HomeGame::Larsen).is_some() {
                    self.versus.watch(None);
                    self.larsen.mode = Mode::Lobby;
                } else {
                    self.versus.rematch();
                }
            }
        }
    }

    /// Both stages side by side: the player's on the left (the feedback it took), the other's on the
    /// right (`them`: whose), aimed at when `can_shoot`. The cell clicked, to send feedback to.
    fn larsen_stages(&mut self, ui: &mut Ui, area: Rect, stage: &Stage, them: &str, can_shoot: bool, now: f64) -> Option<(u8, u8)> {
        let t = self.t();
        let theme = self.theme.clone();
        let look = Look::of(&theme);
        let half = (area.width() - 32.0) / 2.0;
        let left = Rect::from_min_size(area.min + Vec2::new(0.0, 28.0), Vec2::new(half, area.height() - 28.0));
        let right = Rect::from_min_size(area.min + Vec2::new(half + 32.0, 28.0), Vec2::new(half, area.height() - 28.0));
        let (mine, cell) = stage_rect(left);
        let (theirs, _) = stage_rect(right);
        let painter = ui.painter().clone();
        paint_stage(&painter, mine, cell, &format!("{}  ·  {}", t.larsen_my_stage, t.larsen_standing.replace("{n}", &stage.standing.you.to_string())), false, &look, &theme);
        for piece in stage.gear.iter().flatten() {
            let fill = if piece.fried { look.fried.gamma_multiply(0.45) } else { look.gear };
            paint_piece(&painter, mine, cell, piece, fill, &look, piece.fried.then(|| Stroke::new(1.5, look.fried)));
        }
        for &shot in &stage.received {
            paint_shot(&painter, mine, cell, shot, &look);
        }
        let title = format!("{}  ·  {}", t.larsen_their_stage.replace("{n}", them), t.larsen_standing.replace("{n}", &stage.standing.them.to_string()));
        paint_stage(&painter, theirs, cell, &title, can_shoot, &look, &theme);
        for piece in &stage.fried {
            paint_piece(&painter, theirs, cell, piece, look.fried.gamma_multiply(0.45), &look, Some(Stroke::new(1.5, look.fried)));
        }
        for &shot in &stage.fired {
            paint_shot(&painter, theirs, cell, shot, &look);
        }
        if let Some((at, on_theirs, when)) = self.larsen.spark.filter(|s| now - s.2 < 0.7) {
            let shots = if on_theirs { &stage.fired } else { &stage.received };
            let hit = shots.iter().any(|s| (s.0, s.1) == at && s.2);
            paint_spark(&painter, if on_theirs { theirs } else { mine }, cell, at, hit, now - when, &look);
            ui.ctx().request_repaint();
        }
        // Aiming: the cell under the pointer, a click sends the feedback (not twice on the same cell).
        let resp = ui.interact(theirs, ui.id().with("larsen-aim"), Sense::click());
        if can_shoot && let Some(at) = hovered_cell(&resp, theirs, cell).filter(|at| !stage.fired.iter().any(|s| (s.0, s.1) == *at)) {
            let r = cell_rect(theirs, cell, at).shrink(2.0);
            painter.rect_stroke(r, 4.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
            bolt(&painter, r.center(), cell * 0.5, theme.accent.gamma_multiply(0.5));
            if resp.on_hover_cursor(egui::CursorIcon::Crosshair).clicked() {
                return Some(at);
            }
        }
        None
    }

    /// The gear set up: the stage on the left (a click lays the piece in hand, or takes back a piece
    /// laid; R or a right click turns it), the pieces and the buttons on the right. The gear once ready.
    fn larsen_setup(&mut self, ui: &mut Ui, area: Rect, look: &Look, theme: &Theme, t: &Strings) -> Option<Vec<Piece>> {
        let stage_w = (area.width() * 0.62).min(area.height() + 40.0);
        let (stage, cell) = stage_rect(Rect::from_min_size(area.min + Vec2::new(0.0, 28.0), Vec2::new(stage_w, area.height() - 28.0)));
        let painter = ui.painter().clone();
        paint_stage(&painter, stage, cell, t.larsen_my_stage, true, look, theme);
        for piece in &self.larsen.laid() {
            paint_piece(&painter, stage, cell, piece, look.gear, look, None);
        }
        if !self.larsen.chat.typing && ui.input(|i| i.key_pressed(egui::Key::R)) {
            self.larsen.horizontal = !self.larsen.horizontal;
        }
        let resp = ui.interact(stage, ui.id().with("larsen-setup"), Sense::click());
        if resp.secondary_clicked() {
            self.larsen.horizontal = !self.larsen.horizontal;
        }
        if let Some((x, y)) = hovered_cell(&resp, stage, cell) {
            match self.larsen.hand {
                // The piece in hand, where it would go: in the accent where it fits, red where it doesn't.
                Some(k) => {
                    let piece = Piece { x, y, length: GEAR[k], horizontal: self.larsen.horizontal, fried: false };
                    let others: Vec<Piece> = self.larsen.setup.iter().enumerate().filter(|(i, _)| *i != k).filter_map(|(_, p)| *p).collect();
                    let ok = fits(&piece, &others);
                    let shown = Piece { length: if piece.horizontal { piece.length.min(SIZE - x) } else { piece.length.min(SIZE - y) }, ..piece };
                    paint_piece(&painter, stage, cell, &shown, if ok { theme.accent.gamma_multiply(0.6) } else { look.fried.gamma_multiply(0.5) }, look, None);
                    if resp.clicked() && ok {
                        self.larsen.setup[k] = Some(piece);
                        self.larsen.hand = (0..GEAR.len()).find(|&i| self.larsen.setup[i].is_none());
                    }
                }
                // A piece laid, under the pointer: a click takes it back.
                None => {
                    if let Some(k) = self.larsen.setup.iter().position(|p| p.is_some_and(|p| p.cells().any(|c| c == (x, y)))) {
                        if let Some(piece) = self.larsen.setup[k] {
                            paint_piece(&painter, stage, cell, &piece, look.gear, look, Some(Stroke::new(1.5, theme.accent)));
                        }
                        if resp.clicked() {
                            self.larsen.horizontal = self.larsen.setup[k].is_some_and(|p| p.horizontal);
                            self.larsen.setup[k] = None;
                            self.larsen.hand = Some(k);
                        }
                    }
                }
            }
        }
        // The pieces to lay, the one in hand picked out; random, clear, ready.
        let side = Rect::from_min_max(Pos2::new(area.min.x + stage_w + 32.0, stage.min.y), Pos2::new(area.max.x, area.max.y));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(side).layout(egui::Layout::top_down(egui::Align::Min)));
        let names: Vec<&str> = t.larsen_pieces.split('|').collect();
        for (k, length) in GEAR.iter().enumerate() {
            let laid = self.larsen.setup[k].is_some();
            let picked = self.larsen.hand == Some(k);
            let label = format!("{}  ({})", names.get(k).copied().unwrap_or_default(), length);
            let text = egui::RichText::new(label).size(13.0).color(if laid { theme.text_muted } else { theme.text });
            if child.add(egui::Button::selectable(picked, text).min_size(Vec2::new(side.width().min(240.0), 28.0)).corner_radius(6.0)).clicked() {
                self.larsen.hand = Some(k);
                if let Some(piece) = self.larsen.setup[k].take() {
                    self.larsen.horizontal = piece.horizontal;
                }
            }
        }
        child.add_space(10.0);
        child.label(egui::RichText::new(t.larsen_setup_hint).size(12.0).color(theme.text_muted));
        child.add_space(14.0);
        child.horizontal(|ui| {
            if ui.button(egui::RichText::new(format!("🎲  {}", t.larsen_random)).size(13.0)).clicked() {
                self.larsen.setup = random_setup();
                self.larsen.hand = None;
            }
            if ui.button(egui::RichText::new(t.larsen_clear).size(13.0)).clicked() {
                self.larsen.setup = [None; 5];
                self.larsen.hand = Some(0);
            }
        });
        child.add_space(14.0);
        let ready = self.larsen.setup.iter().all(Option::is_some);
        let button = egui::Button::new(egui::RichText::new(t.larsen_ready).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0).min_size(Vec2::new(170.0, 36.0));
        child.add_enabled(ready, button).clicked().then(|| self.larsen.laid())
    }
}

/// The player's last shot, for a moment after it: the shot (a hit or not), and whether it fried a piece.
fn last_shot(spark: &Option<((u8, u8), bool, f64)>, stage: &Stage, now: f64) -> (Option<(u8, u8, bool)>, bool) {
    let last = spark.filter(|(_, mine, at)| *mine && now - at < 2.5).and_then(|(at, _, _)| stage.fired.iter().find(|s| (s.0, s.1) == at).copied());
    let fried = last.is_some_and(|s| s.2 && stage.fried.iter().any(|p| p.cells().any(|c| c == (s.0, s.1))));
    (last, fried)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_random_setup_fits_the_rules() {
        for _ in 0..50 {
            let gear: Vec<Piece> = random_setup().iter().map(|p| p.expect("every piece laid")).collect();
            let mut lengths: Vec<u8> = gear.iter().map(|p| p.length).collect();
            lengths.sort_unstable_by(|a, b| b.cmp(a));
            assert_eq!(lengths, GEAR);
            for (k, piece) in gear.iter().enumerate() {
                let others: Vec<Piece> = gear.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, p)| *p).collect();
                assert!(fits(piece, &others), "{piece:?} among {others:?}");
            }
        }
    }

    /// A whole game of the computer against `gear`: its shots until all of it fried.
    fn shots_to_fry(level: Level, gear: &[Piece], seed: u64) -> usize {
        let mut shots: Vec<(u8, u8, bool)> = Vec::new();
        while !gear.iter().all(|p| is_fried(p, &shots)) {
            let fried: Vec<Piece> = gear.iter().filter(|p| is_fried(p, &shots)).copied().collect();
            let at = ai_shot(level, &shots, &fried, seed.wrapping_add(shots.len() as u64 * 7919));
            assert!(!shots.iter().any(|s| (s.0, s.1) == at), "{level:?} shot twice at {at:?}");
            // Never beside a fried piece: nothing can be there.
            assert!(!fried.iter().flat_map(|p| p.cells().collect::<Vec<_>>()).any(|(x, y)| (x as i16 - at.0 as i16).abs() <= 1 && (y as i16 - at.1 as i16).abs() <= 1), "{level:?} shot beside a fried piece");
            shots.push(shot_at(gear, at));
            assert!(shots.len() <= 100);
        }
        shots.len()
    }

    #[test]
    fn the_computer_hunts_around_a_hit() {
        for level in [Level::Medium, Level::Hard] {
            for seed in 0..20 {
                let (x, y) = ai_shot(level, &[(4, 4, true)], &[], seed);
                assert_eq!((x as i16 - 4).abs() + (y as i16 - 4).abs(), 1, "{level:?} shot at {x},{y}");
            }
        }
    }

    #[test]
    fn the_computer_fries_everything_and_its_levels_get_stronger() {
        let mut totals = Vec::new();
        for level in Level::ALL {
            let mut total = 0;
            for game in 0..40u64 {
                let gear: Vec<Piece> = random_setup().iter().flatten().copied().collect();
                total += shots_to_fry(level, &gear, game * 104_729 + 1);
            }
            totals.push(total);
        }
        assert!(totals[0] > totals[1] && totals[1] > totals[2], "shots to fry it all, easy to hard: {totals:?}");
    }

    #[test]
    fn pieces_dont_touch_even_by_a_corner() {
        let piece = |x, y, length, horizontal| Piece { x, y, length, horizontal, fried: false };
        let amps = [piece(0, 0, 5, true)];
        assert!(fits(&piece(0, 2, 4, true), &amps));
        assert!(!fits(&piece(0, 1, 4, true), &amps));
        assert!(!fits(&piece(5, 1, 2, false), &amps));
        assert!(fits(&piece(6, 1, 2, false), &amps));
        assert!(!fits(&piece(7, 0, 4, true), &[]));
    }
}
