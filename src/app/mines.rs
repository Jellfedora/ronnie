//! The minesweeper, in the games: the grid (see `crate::mines`), its time and mines left above it, and
//! beside it floor's board of the fastest games won.

use std::time::Instant;

use super::four::HomeGame;
use super::*;
use crate::mines::{Board, Cell, State, COLS, ROWS};

/// The game under way, or just over.
pub(super) struct Mines {
    board: Board,
    /// Since the first click.
    started: Option<Instant>,
    /// The time of the game, once over (milliseconds).
    ended: Option<u32>,
    /// The game won beat the player's best.
    record: bool,
    rng: u64,
    /// floor's board of the fastest games.
    floor: crate::floor::GameFloor,
}

impl Default for Mines {
    fn default() -> Self {
        Self { board: Board::default(), started: None, ended: None, record: false, rng: 0, floor: crate::floor::GameFloor::new("demineur", true) }
    }
}

impl Mines {
    fn restart(&mut self) {
        self.board = Board::default();
        self.started = None;
        self.ended = None;
        self.record = false;
    }

    /// The time shown: the game's once over, else since its first click.
    fn millis(&self) -> u32 {
        self.ended.unwrap_or_else(|| self.started.map_or(0, |s| s.elapsed().as_millis().min(u32::MAX as u128) as u32))
    }
}

/// "1:05.3", "42.0".
pub(super) fn time_text(ms: u32) -> String {
    let (m, s, tenth) = (ms / 60_000, ms / 1000 % 60, ms / 100 % 10);
    if m > 0 { format!("{m}:{s:02}.{tenth}") } else { format!("{s}.{tenth}") }
}

/// The color of a number: blue, green, red, then the others.
fn number_color(theme: &Theme, n: u8) -> Color32 {
    match n {
        1 => theme.ansi[4],
        2 => theme.ansi[2],
        3 => theme.ansi[1],
        4 => theme.ansi[5],
        5 => theme.ansi[3],
        6 => theme.ansi[6],
        7 => theme.text,
        _ => theme.text_muted,
    }
}

impl App {
    /// The minesweeper, on the home page.
    pub(super) fn open_mines(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Mines;
    }

    pub(super) fn mines_page(&mut self, ui: &mut Ui, rect: Rect) {
        // A game under way is the half-day of work's; once played, a fresh grid gives way.
        let state = self.mines.board.state;
        if state == State::Playing {
            self.count_round(HomeGame::Mines);
        } else if state == State::Fresh
            && let Some(p) = self.game_locked(HomeGame::Mines)
        {
            return self.locked_page(ui, rect, HomeGame::Mines, &p);
        }
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        // floor's board, fetched again every minute out of a game.
        self.mines.floor.poll();
        if state != State::Playing {
            self.mines.floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.mines_name, 40.0, theme.accent, 1.0);
        let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
        if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
            self.home_game = false;
        }

        // The grid, and the board beside it when there is room (else below).
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
        let side_w = 240.0;
        let wide = body.width() >= 760.0;
        let room = if wide { Vec2::new(body.width() - side_w - 32.0, body.height() - 72.0) } else { Vec2::new(body.width(), body.height() - 72.0 - 160.0) };
        let cell = (room.x / COLS as f32).min(room.y / ROWS as f32).clamp(14.0, 34.0).floor();
        let grid_size = Vec2::new(cell * COLS as f32, cell * ROWS as f32);
        let block_w = if wide { grid_size.x + 32.0 + side_w } else { grid_size.x };
        let grid = Rect::from_min_size(Pos2::new(body.center().x - block_w / 2.0, body.min.y + 44.0), grid_size);

        // Above the grid: the mines left, the time, a new game.
        let painter = ui.painter().clone();
        let bar_y = grid.min.y - 22.0;
        paint_mine(&painter, Pos2::new(grid.min.x + 7.0, bar_y), 5.0, theme.text);
        painter.text(Pos2::new(grid.min.x + 22.0, bar_y), Align2::LEFT_CENTER, self.mines.board.left().to_string(), FontId::monospace(16.0), theme.text);
        painter.text(Pos2::new(grid.max.x, bar_y), Align2::RIGHT_CENTER, time_text(self.mines.millis()), FontId::monospace(16.0), theme.text);
        let new = Rect::from_center_size(Pos2::new(grid.center().x, bar_y), Vec2::new(150.0, 28.0));
        if ui.put(new, egui::Button::new(egui::RichText::new(t.mines_new).size(13.0)).corner_radius(6.0)).clicked() {
            self.mines.restart();
        }

        self.mines_grid(ui, grid, &theme);

        // Below the grid: how to play, or how the game ended.
        let (line, color) = match self.mines.board.state {
            State::Won if self.mines.record => (format!("{}  {}", t.mines_won.replace("{t}", &time_text(self.mines.millis())), t.game_new_best), theme.accent),
            State::Won => (t.mines_won.replace("{t}", &time_text(self.mines.millis())), theme.accent),
            State::Lost(..) => (t.mines_lost.to_owned(), theme.ansi[1]),
            _ => (t.mines_help.to_owned(), theme.text_muted),
        };
        painter.text(Pos2::new(grid.center().x, grid.max.y + 22.0), Align2::CENTER_CENTER, line, FontId::proportional(13.5), color);

        let side = if wide { Rect::from_min_size(Pos2::new(grid.max.x + 32.0, grid.min.y), Vec2::new(side_w, grid.height())) } else { Rect::from_min_size(Pos2::new(grid.min.x, grid.max.y + 48.0), Vec2::new(grid.width(), body.max.y - grid.max.y - 48.0)) };
        let best = (self.config.settings.mines_best > 0).then(|| t.mines_best.replace("{t}", &time_text(self.config.settings.mines_best)));
        if self.game_board(ui, side, &theme, self.mines.floor.board(), best, time_text) {
            self.ask_pseudo();
        }
        // The time runs: a frame each tenth of a second.
        if self.mines.board.state == State::Playing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// The cells: a click shows one (on a number, its neighbors), a right click flags it.
    fn mines_grid(&mut self, ui: &mut Ui, grid: Rect, theme: &Theme) {
        let cell = grid.width() / COLS as f32;
        let resp = ui.interact(grid, ui.id().with("mines-grid"), Sense::click());
        let at = resp.interact_pointer_pos().or(resp.hover_pos()).map(|p| (((p.x - grid.min.x) / cell) as usize, ((p.y - grid.min.y) / cell) as usize)).filter(|&(c, r)| c < COLS && r < ROWS);
        let over = self.mines.board.over();
        if let Some((c, r)) = at.filter(|_| !over) {
            if resp.clicked() {
                let seed = super::game::random(&mut self.mines.rng);
                if self.mines.board.state == State::Fresh {
                    self.mines.started = Some(Instant::now());
                }
                self.mines.board.open(c, r, seed);
            } else if resp.secondary_clicked() {
                self.mines.board.flag(c, r);
            }
            if self.mines.board.over() {
                self.mines_over();
            }
        }
        if !self.mines.board.over() && at.is_some() {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        let painter = ui.painter_at(grid.expand(2.0));
        let board = &self.mines.board;
        let lost = match board.state {
            State::Lost(c, r) => Some((c, r)),
            _ => None,
        };
        let hovered = at.filter(|_| !board.over());
        for r in 0..ROWS {
            for c in 0..COLS {
                let rect = Rect::from_min_size(grid.min + Vec2::new(c as f32 * cell, r as f32 * cell), Vec2::splat(cell)).shrink(1.0);
                let center = rect.center();
                let mine = board.mine(c, r);
                match board.cell(c, r) {
                    Cell::Open(n) => {
                        painter.rect_filled(rect, 3.0, theme.bg);
                        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                        if n > 0 {
                            painter.text(center, Align2::CENTER_CENTER, n.to_string(), FontId::monospace(cell * 0.6), number_color(theme, n));
                        }
                    }
                    hidden => {
                        let fill = if hovered == Some((c, r)) { theme.tab_active } else { theme.chrome_bg };
                        painter.rect_filled(rect, 3.0, fill);
                        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
                        let flagged = hidden == Cell::Flagged;
                        if lost.is_some() && mine && !flagged {
                            // The game lost: every mine shows, the one clicked in red.
                            if lost == Some((c, r)) {
                                painter.rect_filled(rect, 3.0, theme.ansi[1].gamma_multiply(0.7));
                            }
                            paint_mine(&painter, center, cell * 0.22, theme.text);
                        } else if flagged {
                            // A flag on no mine, once lost: crossed out.
                            let wrong = lost.is_some() && !mine;
                            paint_flag(&painter, center, cell * 0.32, if wrong { theme.text_muted } else { theme.ansi[1] }, theme.text_muted);
                            if wrong {
                                let d = cell * 0.28;
                                painter.line_segment([center - Vec2::splat(d), center + Vec2::splat(d)], Stroke::new(2.0, theme.text));
                            }
                        }
                    }
                }
            }
        }
    }

    /// The game just ended: a game won is kept when it beats the best, and sent to floor.
    fn mines_over(&mut self) {
        let ms = self.mines.millis();
        self.mines.ended = Some(ms);
        if self.mines.board.state != State::Won {
            return;
        }
        let best = self.config.settings.mines_best;
        self.mines.record = best == 0 || ms < best;
        if self.mines.record {
            self.config.settings.mines_best = ms;
            self.save_config();
        }
        // Online only with a pseudo.
        if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.mines.floor.finished(crate::floor::GameRound { score: ms, details: serde_json::json!({ "ms": ms, "clicks": self.mines.board.clicks }) });
            let now = self.ctx.input(|i| i.time);
            self.mines.floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
        }
    }
}

fn paint_mine(painter: &egui::Painter, c: Pos2, radius: f32, color: Color32) {
    painter.circle_filled(c, radius, color);
    for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (0.7, 0.7), (0.7, -0.7)] {
        let d = Vec2::new(dx, dy) * radius * 1.5;
        painter.line_segment([c - d, c + d], Stroke::new((radius * 0.35).max(1.0), color));
    }
}

/// A flag: its cloth (`color`) on a pole.
fn paint_flag(painter: &egui::Painter, c: Pos2, size: f32, color: Color32, pole: Color32) {
    let x = c.x - size * 0.35;
    painter.line_segment([Pos2::new(x, c.y - size), Pos2::new(x, c.y + size)], Stroke::new((size * 0.14).max(1.0), pole));
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(x, c.y - size), Pos2::new(x + size * 1.1, c.y - size * 0.55), Pos2::new(x, c.y - size * 0.1)], color, Stroke::NONE));
}

/// The minesweeper's icon: a mine and a flag beside it.
pub(super) fn paint_mines_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    paint_mine(painter, c + Vec2::new(-2.5 * scale, 1.5 * scale), 3.6 * scale, theme.text);
    paint_flag(painter, c + Vec2::new(5.0 * scale, -1.0 * scale), 5.0 * scale, theme.ansi[1], theme.text_muted);
}
