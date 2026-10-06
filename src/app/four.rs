//! Puissance 4, in the games: against the computer, or against another player online (see live.rs):
//! the lobby (the levels, the players online to invite), the grid, the invitations received.

use std::sync::mpsc;

use super::motion::out_cubic;
use super::*;
use crate::connect4::{Board, Level, COLS, ROWS};

/// The game the home page shows.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(super) enum HomeGame {
    #[default]
    SpeedMetal,
    Four,
    Blob,
}

/// A disc falling into place, since `at`.
#[derive(Clone, Copy)]
struct Falling {
    c: usize,
    r: usize,
    at: f64,
}

/// A game against the computer.
struct AiGame {
    board: Board,
    /// The player's discs (1 starts).
    you: u8,
    turn: u8,
    /// 1 or 2, 3: a draw.
    winner: Option<u8>,
    line: Option<[(usize, usize); 4]>,
}

enum Mode {
    Lobby,
    Ai(AiGame),
    /// The match of `live`.
    Online,
}

pub(super) struct Four {
    mode: Mode,
    level: Level,
    /// The player starts the next game against the computer (they take turns).
    you_start: bool,
    /// The computer's move, on its way.
    thinking: Option<mpsc::Receiver<Option<usize>>>,
    falling: Option<Falling>,
    /// The online match and its moves last drawn: a new move falls.
    seen: Option<(String, u32)>,
    /// The match the page went to (the next one, accepted or rematched, is followed too).
    followed: Option<String>,
    /// When the board of wins was last fetched.
    board_at: Option<f64>,
    /// The last game's grid, emptying (since then) before the next one.
    clearing: Option<(Board, f64)>,
    /// The online grid last drawn: it empties when a rematch starts.
    online_board: Option<Board>,
}

impl Default for Four {
    fn default() -> Self {
        Self { mode: Mode::Lobby, level: Level::Medium, you_start: true, thinking: None, falling: None, seen: None, followed: None, board_at: None, clearing: None, online_board: None }
    }
}

/// What a click in the lobby asks.
enum LobbyAction {
    Level(Level),
    PlayAi,
    Invite(String),
    Cancel,
    Pseudo,
}

/// The discs' colors: red for who starts, yellow for the other.
fn disc_color(theme: &Theme, who: u8) -> Color32 {
    if who == 1 { theme.ansi[1] } else { theme.ansi[3] }
}

fn level_name(t: &Strings, level: Level) -> &'static str {
    match level {
        Level::Easy => t.four_easy,
        Level::Medium => t.four_medium,
        Level::Hard => t.four_hard,
    }
}

impl Four {
    /// Development: a game against the computer with these columns played (digits), or the lobby.
    pub(super) fn demo(&mut self, moves: &str) {
        if moves == "lobby" {
            return;
        }
        let mut game = AiGame { board: Board::default(), you: 1, turn: 1, winner: None, line: None };
        let mut falling = None;
        for c in moves.chars().filter_map(|c| c.to_digit(10)) {
            let who = game.turn;
            drop_disc(&mut game, c as usize, who, &mut falling, -10.0);
        }
        self.mode = Mode::Ai(game);
    }
}

impl App {
    /// Puissance 4, on the home page.
    pub(super) fn open_four(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Four;
    }

    /// floor's side of the game for two, at each frame: the thread started or stopped with the pseudo,
    /// the invitations received asked about, the match accepted (or rematched) opened.
    pub(super) fn live_frame(&mut self, ctx: &egui::Context) {
        if self.versus.frame(ctx, self.config.settings.floor.as_ref()) {
            // floor forgot the account: the pseudo is to pick again.
            if let Some(account) = &mut self.config.settings.floor {
                account.name = None;
            }
            self.save_config();
        }
        let t = self.t();
        let new = self.versus.new_invites();
        if let Some(invite) = new.first() {
            if !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
                let (game, text) = if invite.game.as_deref() == Some("ronnie-io") { (t.blob_name, t.blob_invited) } else { (t.four_name, t.four_invited) };
                crate::notify::send(game, &text.replace("{n}", invite.from.as_deref().unwrap_or(t.game_anonymous)));
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
            }
        }
        // A match started (an invitation accepted, a rematch): to its page.
        if let Some(game) = self.versus.game.as_ref().filter(|g| !g.over && self.four.followed.as_ref() != Some(&g.id)) {
            self.four.followed = Some(game.id.clone());
            self.four.mode = Mode::Online;
            self.open_four();
        }
        self.invite_window(ctx);
    }

    /// "Dio invites you to a game" (of Puissance 4, or on Ronnie.io): play, or decline.
    fn invite_window(&mut self, ctx: &egui::Context) {
        let Some(invite) = self.versus.invites.first().cloned() else { return };
        let blob = invite.game.as_deref() == Some("ronnie-io");
        let t = self.t();
        let theme = self.theme.clone();
        let mut answer = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent));
        let modal = egui::Modal::new(egui::Id::new("four-invite")).frame(frame).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                if blob {
                    super::blob::paint_blob_icon(ui.painter(), rect.center(), &theme, 1.2);
                } else {
                    paint_four_icon(ui.painter(), rect.center(), &theme, 1.2);
                }
                ui.label(egui::RichText::new(if blob { t.blob_name } else { t.four_name }).size(18.0).strong());
            });
            ui.add_space(8.0);
            let text = if blob { t.blob_invited } else { t.four_invited };
            ui.label(egui::RichText::new(text.replace("{n}", invite.from.as_deref().unwrap_or(t.game_anonymous))).size(14.0));
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let play = egui::Button::new(egui::RichText::new(t.four_play).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(110.0, 30.0));
                if ui.add(play).clicked() {
                    answer = Some(true);
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.four_decline).size(13.5)).corner_radius(6.0).min_size(Vec2::new(96.0, 30.0))).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer.get_or_insert(false);
        }
        match answer {
            Some(true) if blob => {
                self.versus.accept(&invite.id);
                self.open_blob();
            }
            Some(true) => {
                self.versus.accept(&invite.id);
                self.four.mode = Mode::Online;
                self.four.seen = None;
                self.open_four();
            }
            Some(false) => self.versus.decline(&invite.id),
            None => {}
        }
    }

    /// The page: the lobby, or a game.
    pub(super) fn four_page(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.four_name, 40.0, theme.accent, 1.0);
        // Back: from a game to the lobby, from the lobby home (not during an online match: give up first).
        let playing_online = matches!(self.four.mode, Mode::Online) && self.versus.game.as_ref().is_some_and(|g| !g.over);
        if !playing_online {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                match self.four.mode {
                    Mode::Lobby => self.home_game = false,
                    Mode::Online => {
                        self.versus.watch(None);
                        self.four.mode = Mode::Lobby;
                    }
                    Mode::Ai(_) => self.four.mode = Mode::Lobby,
                }
                self.four.thinking = None;
                self.four.falling = None;
                self.four.clearing = None;
            }
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
        match self.four.mode {
            Mode::Lobby => self.four_lobby(ui, body, now),
            Mode::Ai(_) => self.four_ai(ui, body, now),
            Mode::Online => self.four_online(ui, body, now),
        }
    }

    fn four_lobby(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        // The board of wins, fetched again every half minute while the lobby shows.
        if pseudo.is_some() && self.four.board_at.is_none_or(|at| now - at > 30.0) {
            self.four.board_at = Some(now);
            self.versus.fetch_board();
        }
        let width = body.width().min(820.0);
        let wide = width >= 640.0;
        let area = Rect::from_min_size(Pos2::new(body.center().x - width / 2.0, body.min.y), Vec2::new(width, body.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
        let card_w = if wide { (width - 16.0) / 2.0 } else { width };
        let mut action = None;
        let card = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
            Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(18.0).show(ui, |ui| {
                // A column, also inside the row of cards (which lays out left to right).
                ui.vertical(|ui| {
                    ui.set_width(card_w - 38.0);
                    ui.set_max_width(card_w - 38.0);
                    add(ui);
                });
            });
        };
        let level = self.four.level;
        let mut ai_action = None;
        let mut ai = |ui: &mut Ui| {
            ui.label(egui::RichText::new(t.four_vs_ai).size(16.0).strong());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                for l in Level::ALL {
                    let b = egui::Button::selectable(l == level, egui::RichText::new(level_name(t, l)).size(13.0)).min_size(Vec2::new(86.0, 28.0)).corner_radius(6.0);
                    if ui.add(b).clicked() {
                        ai_action = Some(LobbyAction::Level(l));
                    }
                }
            });
            ui.add_space(16.0);
            // In a rect of its own: its text centered (the column aligns it to the top).
            let play = egui::Button::new(egui::RichText::new(t.four_play).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
            let (at, _) = ui.allocate_exact_size(Vec2::new(140.0, 34.0), Sense::hover());
            if ui.put(at, play).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ai_action = Some(LobbyAction::PlayAi);
            }
        };
        let live = &self.versus;
        let mut players = |ui: &mut Ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.four_vs_player).size(16.0).strong());
                if pseudo.is_some() && live.reachable == Some(true) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(t.four_online.replace("{n}", &live.online.len().to_string())).size(12.0).color(theme.text_muted));
                    });
                }
            });
            ui.add_space(10.0);
            if pseudo.is_none() {
                ui.label(egui::RichText::new(t.four_need_pseudo).size(13.0).color(theme.text_muted));
                ui.add_space(10.0);
                if ui.button(egui::RichText::new(t.pseudo_pick).size(13.0)).clicked() {
                    action = Some(LobbyAction::Pseudo);
                }
                return;
            }
            match live.reachable {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new(t.four_connecting).size(13.0).color(theme.text_muted));
                    });
                    return;
                }
                Some(false) => {
                    ui.label(egui::RichText::new(t.four_offline).size(13.0).color(theme.ansi[1]));
                    return;
                }
                Some(true) => {}
            }
            let pending = live.sent.as_ref().filter(|s| s.status == "pending");
            if live.online.is_empty() {
                ui.label(egui::RichText::new(t.four_nobody).size(13.0).color(theme.text_muted));
            }
            egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                for player in &live.online {
                    ui.horizontal(|ui| {
                        ui.set_min_height(30.0);
                        ui.label(egui::RichText::new("●").size(10.0).color(theme.ansi[2]));
                        ui.label(egui::RichText::new(&player.name).size(13.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if player.busy {
                                ui.label(egui::RichText::new(t.four_busy).size(12.0).color(theme.text_muted));
                            } else if ui.add_enabled(pending.is_none(), egui::Button::new(egui::RichText::new(t.four_invite).size(12.5)).corner_radius(6.0)).clicked() {
                                action = Some(LobbyAction::Invite(player.name.clone()));
                            }
                        });
                    });
                }
            });
            if let Some(sent) = &live.sent {
                let to = sent.to.as_deref().unwrap_or(t.game_anonymous);
                ui.add_space(8.0);
                match sent.status.as_str() {
                    "pending" => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new(t.four_sent.replace("{n}", to)).size(13.0));
                            if ui.small_button(t.cancel).clicked() {
                                action = Some(LobbyAction::Cancel);
                            }
                        });
                    }
                    "declined" => drop(ui.label(egui::RichText::new(t.four_declined.replace("{n}", to)).size(13.0).color(theme.text_muted))),
                    "expired" => drop(ui.label(egui::RichText::new(t.four_expired.replace("{n}", to)).size(13.0).color(theme.text_muted))),
                    _ => {}
                }
            }
            if let Some(e) = &live.refused {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(e).size(12.5).color(theme.ansi[1]));
            }
            // The board of online wins.
            if let Some(board) = live.board.as_ref().filter(|b| !b.is_empty()) {
                ui.add_space(12.0);
                ui.separator();
                ui.label(egui::RichText::new(t.four_wins).size(12.5).strong().color(theme.text_muted));
                for (k, (name, wins)) in board.iter().take(5).enumerate() {
                    ui.horizontal(|ui| {
                        let me = pseudo.as_deref() == Some(name.as_str());
                        ui.label(egui::RichText::new(format!("{}.", k + 1)).size(12.5).color(theme.text_muted));
                        ui.label(egui::RichText::new(if name.is_empty() { t.game_anonymous } else { name }).size(12.5).color(if me { theme.accent } else { theme.text }));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(wins.to_string()).size(12.5).monospace());
                        });
                    });
                }
            }
        };
        if wide {
            child.horizontal_top(|ui| {
                card(ui, &mut ai);
                ui.add_space(16.0);
                card(ui, &mut players);
            });
        } else {
            card(&mut child, &mut ai);
            child.add_space(16.0);
            card(&mut child, &mut players);
        }
        match action.or(ai_action) {
            Some(LobbyAction::Level(l)) => self.four.level = l,
            Some(LobbyAction::PlayAi) => self.new_ai_game(now),
            Some(LobbyAction::Invite(name)) => self.versus.invite(&name),
            Some(LobbyAction::Cancel) => self.versus.cancel(),
            Some(LobbyAction::Pseudo) => self.ask_pseudo(),
            None => {}
        }
    }

    /// A game against the computer; the last one's discs fall out of the grid first.
    fn new_ai_game(&mut self, now: f64) {
        let you = if self.four.you_start { 1 } else { 2 };
        self.four.you_start = !self.four.you_start;
        self.four.clearing = match &self.four.mode {
            Mode::Ai(old) => Some((old.board.clone(), now)),
            _ => None,
        };
        self.four.mode = Mode::Ai(AiGame { board: Board::default(), you, turn: 1, winner: None, line: None });
        self.four.thinking = None;
        self.four.falling = None;
    }

    fn four_ai(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let level = self.four.level;
        let Mode::Ai(game) = &mut self.four.mode else { return };
        let cleared = self.four.clearing.as_ref().is_none_or(|(_, at)| now - at > CLEAR);
        // The computer's turn (the grid emptied): its move worked out in a thread, played after a pause.
        if cleared && game.winner.is_none() && game.turn != game.you {
            match &self.four.thinking {
                None => {
                    let (tx, rx) = mpsc::channel();
                    let (board, who, ctx) = (game.board.clone(), 3 - game.you, ui.ctx().clone());
                    let seed = (now * 1000.0) as u64 ^ 0x9e37_79b9_7f4a_7c15;
                    std::thread::spawn(move || {
                        let started = std::time::Instant::now();
                        let c = crate::connect4::best_move(&board, who, level, seed);
                        std::thread::sleep(std::time::Duration::from_millis(550).saturating_sub(started.elapsed()));
                        let _ = tx.send(c);
                        ctx.request_repaint();
                    });
                    self.four.thinking = Some(rx);
                }
                Some(rx) => {
                    if let Ok(c) = rx.try_recv() {
                        self.four.thinking = None;
                        if let Some(c) = c {
                            let who = 3 - game.you;
                            drop_disc(game, c, who, &mut self.four.falling, now);
                        }
                    }
                }
            }
        }
        let (status, color) = match game.winner {
            Some(3) => (t.four_draw.to_owned(), theme.text),
            Some(w) if w == game.you => (t.four_won.to_owned(), theme.ansi[2]),
            Some(_) => (t.four_lost.to_owned(), theme.ansi[1]),
            None if game.turn == game.you => (t.four_your_turn.to_owned(), theme.text),
            None => (t.four_ai_thinking.to_owned(), theme.text_muted),
        };
        let you_name = crate::floor::pseudo(self.config.settings.floor.as_ref()).unwrap_or(t.game_you).to_owned();
        let ai_name = format!("{} · {}", t.four_computer, level_name(t, level));
        let names = if game.you == 1 { [you_name, ai_name] } else { [ai_name, you_name] };
        let can_play = cleared && game.winner.is_none() && game.turn == game.you;
        let turn = game.winner.is_none().then_some(game.turn);
        let board_area = game_header(ui, body, &status, color, &names, turn, game.winner.is_some(), &theme);
        let line = game.line;
        let view = BoardView {
            board: &game.board,
            line: line.as_ref().map(|l| &l[..]),
            falling: self.four.falling,
            ghost: can_play.then_some(game.you),
            thinking: self.four.thinking.is_some().then_some(3 - game.you),
            cheer: game.winner == Some(game.you),
            clearing: self.four.clearing.as_ref(),
        };
        let mut clicked = board_ui(ui, board_area, view, now, &theme);
        if can_play {
            clicked = clicked.or_else(|| column_key(ui));
        }
        if let Some(c) = clicked.filter(|_| can_play) {
            let who = game.you;
            drop_disc(game, c, who, &mut self.four.falling, now);
        }
        if game.winner.is_none() {
            keys_hint(ui, body, t, &theme);
        }
        if game.winner.is_some() && bottom_button(ui, body, t.four_again, &theme, true) {
            self.new_ai_game(now);
        }
    }

    fn four_online(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(game) = self.versus.game.clone() else {
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
        let board = Board::from_columns(&game.columns);
        // A move not seen yet (the other's): it falls.
        let seen = (game.id.clone(), game.moves);
        if self.four.seen.as_ref() != Some(&seen) {
            if self.four.seen.as_ref().is_some_and(|(id, moves)| *id == game.id && game.moves > *moves) {
                if let Some([c, r]) = game.last {
                    self.four.falling = Some(Falling { c, r, at: now });
                }
            } else {
                // A rematch: the last grid empties first.
                if self.four.seen.is_some() {
                    self.four.clearing = self.four.online_board.take().map(|b| (b, now));
                }
                self.four.falling = None;
            }
            self.four.seen = Some(seen);
        }
        let them = game.them().unwrap_or(t.game_anonymous).to_owned();
        let (status, color) = match game.winner {
            Some(3) => (t.four_draw.to_owned(), theme.text),
            Some(w) if w == game.you && game.reason.as_deref() == Some("forfeit") => (t.four_forfeit_them.replace("{n}", &them), theme.ansi[2]),
            Some(w) if w == game.you => (t.four_won.to_owned(), theme.ansi[2]),
            Some(_) if game.reason.as_deref() == Some("forfeit") => (t.four_forfeit_you.to_owned(), theme.text_muted),
            Some(_) => (t.four_lost.to_owned(), theme.ansi[1]),
            None if game.turn == game.you => (t.four_your_turn.to_owned(), theme.text),
            None => (t.four_their_turn.replace("{n}", &them), theme.text_muted),
        };
        let names = [0, 1].map(|k| if k + 1 == game.you as usize { t.game_you.to_owned() } else { game.players[k].clone().unwrap_or_else(|| t.game_anonymous.to_owned()) });
        let cleared = self.four.clearing.as_ref().is_none_or(|(_, at)| now - at > CLEAR);
        let can_play = cleared && !game.over && game.turn == game.you;
        let turn = (!game.over).then_some(game.turn);
        let board_area = game_header(ui, body, &status, color, &names, turn, game.over, &theme);
        let line: Option<Vec<(usize, usize)>> = game.line.as_ref().map(|l| l.iter().map(|&[c, r]| (c, r)).collect());
        let view = BoardView {
            board: &board,
            line: line.as_deref(),
            falling: self.four.falling,
            ghost: can_play.then_some(game.you),
            thinking: (cleared && !game.over && game.turn != game.you).then_some(3 - game.you),
            cheer: game.winner == Some(game.you),
            clearing: self.four.clearing.as_ref(),
        };
        let mut clicked = board_ui(ui, board_area, view, now, &theme);
        self.four.online_board = Some(board.clone());
        if can_play {
            clicked = clicked.or_else(|| column_key(ui));
        }
        if let Some(c) = clicked.filter(|&c| can_play && board.can_play(c)) {
            self.four.falling = Some(Falling { c, r: board.height(c), at: now });
            self.four.seen = Some((game.id.clone(), game.moves + 1));
            self.versus.play(c);
        }
        if !game.over {
            keys_hint(ui, body, t, &theme);
            if bottom_button(ui, body, t.four_leave, &theme, false) {
                self.versus.leave();
            }
        } else {
            if game.rematch.them && !game.rematch.you {
                ui.painter().text(Pos2::new(body.center().x, body.max.y - 74.0), Align2::CENTER_CENTER, t.four_rematch_them.replace("{n}", &them), FontId::proportional(13.5), theme.accent);
            }
            let label = if game.rematch.you { t.four_rematch_asked } else { t.four_rematch };
            if bottom_button(ui, body, label, &theme, !game.rematch.you) && !game.rematch.you {
                self.versus.rematch();
            }
        }
    }
}

/// A disc dropped in a game against the computer: the turn passes, or the game ends.
fn drop_disc(game: &mut AiGame, c: usize, who: u8, falling: &mut Option<Falling>, now: f64) {
    let Some(r) = game.board.play(c, who) else { return };
    *falling = Some(Falling { c, r, at: now });
    if let Some(line) = game.board.line(c, r) {
        game.winner = Some(who);
        game.line = Some(line);
    } else if game.board.full() {
        game.winner = Some(3);
    } else {
        game.turn = 3 - who;
    }
}

/// Keys 1 to 7: that column.
fn column_key(ui: &Ui) -> Option<usize> {
    use egui::Key::*;
    let keys = [Num1, Num2, Num3, Num4, Num5, Num6, Num7];
    ui.input(|i| keys.iter().position(|k| i.key_pressed(*k)))
}

/// The status line and the two players (with their discs' colors) above the grid; where the grid goes.
/// `turn`: whose disc pulses. `over`: the status pops, larger, in the metal letters.
#[allow(clippy::too_many_arguments)]
fn game_header(ui: &Ui, body: Rect, status: &str, color: Color32, names: &[String; 2], turn: Option<u8>, over: bool, theme: &Theme) -> Rect {
    let painter = ui.painter();
    let now = ui.input(|i| i.time);
    let at = Pos2::new(body.center().x, body.min.y + 6.0);
    let pop = ui.ctx().animate_bool_with_time(ui.id().with("four-over"), over, 0.45);
    if pop > 0.0 {
        let size = 20.0 + 14.0 * super::motion::out_back(pop);
        paint_metal(painter, at, Align2::CENTER_CENTER, status, size, color, pop.min(1.0));
    } else {
        painter.text(at, Align2::CENTER_CENTER, status, FontId::proportional(20.0), color);
    }
    let y = body.min.y + 40.0;
    let font = FontId::proportional(13.5);
    let name_color = |k: usize| if turn.is_none_or(|w| w as usize == k + 1) { theme.text } else { theme.text_muted };
    let left = painter.layout_no_wrap(names[0].clone(), font.clone(), name_color(0));
    let vs = painter.layout_no_wrap("  —  ".to_owned(), font.clone(), theme.text_muted);
    let right = painter.layout_no_wrap(names[1].clone(), font, name_color(1));
    let total = 18.0 + left.size().x + vs.size().x + 18.0 + right.size().x;
    let mut x = body.center().x - total / 2.0;
    for (k, galley) in [left, right].into_iter().enumerate() {
        let who = k as u8 + 1;
        let dot = Pos2::new(x + 6.0, y);
        if turn == Some(who) {
            // Its turn: the disc breathes, a ring spreads from it.
            let p = (now * 1.2).fract() as f32;
            painter.circle_stroke(dot, 6.0 + 6.0 * out_cubic(p), Stroke::new(1.5, disc_color(theme, who).gamma_multiply(1.0 - p)));
            ui.ctx().request_repaint();
        }
        let r = if turn == Some(who) { 6.0 + 0.8 * (now * 5.0).sin() as f32 } else { 6.0 };
        painter.circle_filled(dot, r, disc_color(theme, who));
        x += 18.0;
        let w = galley.size().x;
        painter.galley(Pos2::new(x, y - galley.size().y / 2.0), galley, theme.text);
        x += w;
        if k == 0 {
            let w = vs.size().x;
            painter.galley(Pos2::new(x, y - vs.size().y / 2.0), vs.clone(), theme.text_muted);
            x += w;
        }
    }
    Rect::from_min_max(Pos2::new(body.min.x, body.min.y + 64.0), Pos2::new(body.max.x, body.max.y - 92.0))
}

/// "Click, or keys 1 to 7", under the grid.
fn keys_hint(ui: &Ui, body: Rect, t: &Strings, theme: &Theme) {
    ui.painter().text(Pos2::new(body.center().x, body.max.y - 74.0), Align2::CENTER_CENTER, t.four_keys, FontId::proportional(12.0), theme.text_muted.gamma_multiply(0.8));
}

/// The button under the grid; `strong`: filled with the accent.
fn bottom_button(ui: &mut Ui, body: Rect, text: &str, theme: &Theme, strong: bool) -> bool {
    let at = Rect::from_center_size(Pos2::new(body.center().x, body.max.y - 36.0), Vec2::new(170.0, 36.0));
    let button = if strong {
        egui::Button::new(egui::RichText::new(text).size(14.0).strong().color(theme.bg)).fill(theme.accent)
    } else {
        egui::Button::new(egui::RichText::new(text).size(14.0))
    };
    ui.put(at, button.corner_radius(8.0)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Gravity, in cells per second², and the share of its speed a disc keeps when it bounces.
const GRAVITY: f32 = 70.0;
const BOUNCE: f32 = 0.22;
/// How long the grid takes to empty, between two games.
const CLEAR: f64 = 0.9;

/// A disc dropped `h` cells above its hole, `t` seconds ago: how high above it it still is (in cells),
/// with two small bounces; and when it first landed.
fn fall(h: f32, t: f32) -> (f32, f32) {
    let land = (2.0 * h / GRAVITY).sqrt();
    if t < land {
        return (h - 0.5 * GRAVITY * t * t, land);
    }
    let (mut v, mut t) = (GRAVITY * land * BOUNCE, t - land);
    for _ in 0..2 {
        let hop = 2.0 * v / GRAVITY;
        if t < hop {
            return (v * t - 0.5 * GRAVITY * t * t, land);
        }
        t -= hop;
        v *= BOUNCE;
    }
    (0.0, land)
}

/// How long a fall from `h` cells lasts, bounces included.
fn fall_length(h: f32) -> f32 {
    let land = (2.0 * h / GRAVITY).sqrt();
    land * (1.0 + 2.0 * BOUNCE * (1.0 + BOUNCE))
}

/// A number in 0..1, the same for the same `k` and `salt`.
fn noise(k: u32, salt: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x & 0xff_ffff) as f32 / 0xff_ffff as f32
}

/// What `board_ui` draws.
struct BoardView<'a> {
    board: &'a Board,
    line: Option<&'a [(usize, usize)]>,
    falling: Option<Falling>,
    /// Whose disc follows the pointer above the grid, when a column can be clicked.
    ghost: Option<u8>,
    /// Whose disc hesitates above the grid, the other thinking.
    thinking: Option<u8>,
    /// The player won: confetti.
    cheer: bool,
    /// The last game's grid, emptying since then.
    clearing: Option<&'a (Board, f64)>,
}

/// The grid in `area`: the discs behind its plate, the one falling (and bouncing), the line of four.
/// The column clicked.
fn board_ui(ui: &mut Ui, area: Rect, view: BoardView, now: f64, theme: &Theme) -> Option<usize> {
    let BoardView { board, line, falling, ghost, thinking, cheer, clearing } = view;
    let cell = (area.width() / COLS as f32).min(area.height() / (ROWS as f32 + 0.9)).clamp(24.0, 78.0);
    let mut grid = Rect::from_min_size(Pos2::new(area.center().x - cell * COLS as f32 / 2.0, area.max.y - cell * ROWS as f32), Vec2::new(cell * COLS as f32, cell * ROWS as f32));
    let radius = cell * 0.38;
    let painter = ui.painter().clone();
    let mut busy = false;

    // The disc falling: where it is, when it landed. The grid shakes a little as it lands.
    let mut landed_at = None;
    let mut falling_at = None;
    if let Some(f) = falling {
        let h = (ROWS - f.r) as f32;
        let t = (now - f.at) as f32;
        let (above, land) = fall(h, t);
        landed_at = Some(f.at + land as f64);
        if t < fall_length(h) {
            busy = true;
            falling_at = Some((f.c, f.r, above));
        }
        let since = t - land;
        if (0.0..0.3).contains(&since) {
            let shake = (1.0 - since / 0.3).powi(2) * (since * 70.0).sin() * cell * 0.03 * h / ROWS as f32;
            grid = grid.translate(Vec2::new(0.0, shake.abs()));
        }
    }
    let center = |c: usize, r: usize| Pos2::new(grid.min.x + (c as f32 + 0.5) * cell, grid.max.y - (r as f32 + 0.5) * cell);
    // The line of four, from when its last disc landed (or long ago, the game opened already won).
    let won_at = line.and(falling).filter(|f| line.is_some_and(|l| l.contains(&(f.c, f.r)))).and(landed_at).unwrap_or(f64::NEG_INFINITY);
    let won = line.is_some() && falling_at.is_none();

    // The pointer's column (eased), with a disc above it; or the other's, wandering while it thinks.
    let zone = Rect::from_min_max(Pos2::new(grid.min.x, grid.min.y - cell * 1.2), grid.max);
    let resp = ui.interact(zone, ui.id().with("four-grid"), Sense::click());
    let hovered = ghost.and(resp.hover_pos()).map(|p| (((p.x - grid.min.x) / cell).floor().max(0.0) as usize).min(COLS - 1)).filter(|&c| board.can_play(c));
    let hover_id = ui.id().with("four-ghost");
    let target = hovered.map_or(grid.center().x, |c| grid.min.x + (c as f32 + 0.5) * cell);
    let ghost_x = ui.ctx().animate_value_with_time(hover_id, target, 0.09);
    let shown = ui.ctx().animate_bool_with_time(hover_id.with("shown"), hovered.is_some(), 0.12);
    let above = grid.min.y - cell * 0.62;
    if let (Some(who), true) = (ghost, shown > 0.0) {
        let x = ghost_x;
        let column = Rect::from_min_max(Pos2::new(x - cell / 2.0, grid.min.y), Pos2::new(x + cell / 2.0, grid.max.y));
        painter.rect_filled(column, cell * 0.12, disc_color(theme, who).gamma_multiply(0.10 * shown));
        let bob = ((now * 5.0).sin() as f32) * cell * 0.04;
        paint_disc(&painter, Pos2::new(x, above + bob), radius * (0.8 + 0.2 * shown), disc_color(theme, who).gamma_multiply(0.85 * shown));
        busy = true;
    }
    if let Some(who) = thinking {
        // Between the columns, as if weighing them.
        let k = 0.5 + 0.32 * (now * 1.7).sin() as f32 + 0.18 * (now * 2.9 + 1.3).sin() as f32;
        let x = grid.min.x + cell * (0.5 + k * (COLS as f32 - 1.0));
        let bob = ((now * 4.0).sin() as f32) * cell * 0.05;
        paint_disc(&painter, Pos2::new(x, above + bob), radius, disc_color(theme, who).gamma_multiply(0.7));
        busy = true;
    }

    // Behind the plate: the back of the grid, the last game's discs falling out, the discs.
    let back = super::sidebar::lerp_color(theme.bg, Color32::BLACK, 0.25);
    painter.rect_filled(grid, 0.0, back);
    if let Some((old, at)) = clearing {
        let t = (now - at) as f32;
        if t < CLEAR as f32 {
            busy = true;
            let clip = painter.with_clip_rect(Rect::from_min_max(Pos2::new(grid.min.x, area.min.y - cell * 2.0), grid.max));
            for c in 0..COLS {
                for r in 0..old.height(c) {
                    let t = (t - c.abs_diff(3) as f32 * 0.04).max(0.0);
                    let drop = 0.5 * GRAVITY * t * t * cell;
                    paint_disc(&clip, center(c, r) + Vec2::new(0.0, drop), radius, disc_color(theme, old.get(c, r)));
                }
            }
        }
    }
    let dim = if won { 1.0 - 0.45 * super::motion::phase(now, won_at + 0.3, 0.5, out_cubic) } else { 1.0 };
    for c in 0..COLS {
        for r in 0..ROWS {
            let who = board.get(c, r);
            if who == 0 {
                continue;
            }
            let mut at = center(c, r);
            if let Some((fc, fr, above)) = falling_at
                && (fc, fr) == (c, r)
            {
                at.y -= above * cell;
            }
            let in_line = line.is_some_and(|l| l.contains(&(c, r)));
            let color = disc_color(theme, who);
            paint_disc(&painter, at, radius, if in_line || !won { color } else { color.gamma_multiply(dim) });
        }
    }

    // The plate, with its holes, over them; its frame, its feet.
    let plate = super::sidebar::lerp_color(theme.chrome_bg, theme.accent, 0.16);
    painter.add(plate_mesh(grid, cell, radius, plate));
    let margin = cell * 0.12;
    painter.rect_stroke(grid, cell * 0.2, Stroke::new(margin, plate), egui::StrokeKind::Outside);
    painter.rect_stroke(grid.expand(margin), cell * 0.2 + margin, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
    for c in 0..COLS {
        for r in 0..ROWS {
            // The holes' depth: a shadow along their top.
            let at = center(c, r);
            painter.circle_stroke(at + Vec2::new(0.0, -radius * 0.04), radius, Stroke::new(radius * 0.08, Color32::from_black_alpha(70)));
        }
    }
    for side in [-1.0, 1.0] {
        let x = grid.center().x + side * (grid.width() / 2.0 + margin * 0.5);
        let foot = vec![Pos2::new(x - cell * 0.12, grid.max.y), Pos2::new(x + cell * 0.12, grid.max.y), Pos2::new(x + side * cell * 0.3 + cell * 0.1, grid.max.y + cell * 0.35), Pos2::new(x + side * cell * 0.3 - cell * 0.1, grid.max.y + cell * 0.35)];
        painter.add(egui::Shape::convex_polygon(foot, plate, Stroke::new(1.0, theme.tab_hover)));
    }

    // The disc landing: a ring spreads from its hole.
    if let (Some(f), Some(at)) = (falling, landed_at) {
        let p = ((now - at) / 0.4) as f32;
        if (0.0..1.0).contains(&p) {
            busy = true;
            let color = disc_color(theme, board.get(f.c, f.r));
            painter.circle_stroke(center(f.c, f.r), radius * (1.0 + 0.5 * out_cubic(p)), Stroke::new(cell * 0.05 * (1.0 - p), color.gamma_multiply(1.0 - p)));
        }
    }

    // The line of four, once its last disc landed: its discs pop one after the other, a stroke joins
    // them, then they glow.
    if let Some(line) = line.filter(|_| won) {
        busy = true;
        let points: Vec<Pos2> = line.iter().map(|&(c, r)| center(c, r)).collect();
        let who = board.get(line[0].0, line[0].1);
        let color = disc_color(theme, who);
        let reach = super::motion::phase(now, won_at + 0.15, 0.45, out_cubic);
        let (first, last) = (points[0], points[points.len() - 1]);
        let end = first + (last - first) * reach;
        let pulse = 0.7 + 0.3 * ((now * 4.0).sin() as f32);
        painter.line_segment([first, end], Stroke::new(cell * 0.1, theme.text.gamma_multiply(0.35 * pulse)));
        for (k, &p) in points.iter().enumerate() {
            let pop = super::motion::phase(now, won_at + 0.08 * k as f64, 0.35, |x| x);
            let scale = 1.0 + 0.28 * (std::f32::consts::PI * pop).sin();
            if pop > 0.0 && pop < 1.0 {
                paint_disc(&painter, p, radius * scale, color);
            }
            let glow = 0.5 + 0.5 * ((now * 4.0 - k as f64 * 0.6).sin() as f32);
            painter.circle_stroke(p, radius * scale + 2.0, Stroke::new(3.0, theme.text.gamma_multiply(pulse)));
            painter.circle_stroke(p, radius * scale + 6.0, Stroke::new(4.0, color.gamma_multiply(0.35 * glow)));
        }
        if cheer {
            confetti(&painter, first + (last - first) * 0.5, cell, now - won_at - 0.2, theme, color);
        }
    }

    if busy {
        ui.ctx().request_repaint();
    }
    let clicked = resp.clicked().then_some(hovered).flatten();
    if ghost.is_some() && hovered.is_some() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    clicked
}

/// The plate in front of the discs: each cell a square with a round hole.
fn plate_mesh(grid: Rect, cell: f32, radius: f32, color: Color32) -> egui::Mesh {
    const STEPS: u32 = 40;
    let mut mesh = egui::Mesh::default();
    for c in 0..COLS {
        for r in 0..ROWS {
            let center = Pos2::new(grid.min.x + (c as f32 + 0.5) * cell, grid.min.y + (r as f32 + 0.5) * cell);
            let base = mesh.vertices.len() as u32;
            for k in 0..STEPS {
                let a = k as f32 / STEPS as f32 * std::f32::consts::TAU;
                let dir = Vec2::new(a.cos(), a.sin());
                // The square's edge, along the same ray (its corners at a multiple of 45°).
                let edge = cell / 2.0 / dir.x.abs().max(dir.y.abs());
                mesh.colored_vertex(center + dir * radius, color);
                mesh.colored_vertex(center + dir * edge, color);
            }
            for k in 0..STEPS {
                let (i0, o0) = (base + 2 * k, base + 2 * k + 1);
                let next = (k + 1) % STEPS;
                let (i1, o1) = (base + 2 * next, base + 2 * next + 1);
                mesh.add_triangle(i0, o0, i1);
                mesh.add_triangle(o0, o1, i1);
            }
        }
    }
    mesh
}

/// A burst of confetti from `from`, `t` seconds in: the winner's color, the accent, and white.
fn confetti(painter: &egui::Painter, from: Pos2, cell: f32, t: f64, theme: &Theme, color: Color32) {
    const LIFE: f32 = 2.6;
    let t = t as f32;
    if !(0.0..LIFE).contains(&t) {
        return;
    }
    let colors = [color, theme.accent, Color32::WHITE, theme.ansi[2], theme.ansi[4]];
    for k in 0..110u32 {
        let delay = noise(k, 1) * 0.25;
        let t = t - delay;
        if t <= 0.0 {
            continue;
        }
        // Upward, in a wide fan; slowed by the air, then floating down.
        let angle = -std::f32::consts::FRAC_PI_2 + (noise(k, 2) - 0.5) * 2.4;
        let speed = cell * (5.0 + 9.0 * noise(k, 3));
        let drag = 2.2;
        let gone = (1.0 - (-drag * t).exp()) / drag;
        let fall = cell * 3.0 * (t - gone);
        let at = from + Vec2::new(angle.cos(), angle.sin()) * speed * gone + Vec2::new((t * 3.0 + noise(k, 4) * 6.0).sin() * cell * 0.15, fall);
        let alpha = ((LIFE - delay - t) / 0.6).clamp(0.0, 1.0);
        let spin = t * (4.0 + 8.0 * noise(k, 5)) + noise(k, 6) * 6.0;
        let (w, h) = (cell * 0.11, cell * 0.06 * spin.sin().abs().max(0.2));
        let (cos, sin) = ((spin * 0.7).cos(), (spin * 0.7).sin());
        let corner = |x: f32, y: f32| at + Vec2::new(x * cos - y * sin, x * sin + y * cos);
        let piece = vec![corner(-w, -h), corner(w, -h), corner(w, h), corner(-w, h)];
        let c = colors[(noise(k, 7) * colors.len() as f32) as usize % colors.len()];
        painter.add(egui::Shape::convex_polygon(piece, c.gamma_multiply(alpha), Stroke::NONE));
    }
}

/// A disc: its rim a shade darker, a ring inside, a shine at its top left.
fn paint_disc(painter: &egui::Painter, at: Pos2, radius: f32, color: Color32) {
    let dark = super::sidebar::lerp_color(color, Color32::BLACK.gamma_multiply(color.a() as f32 / 255.0), 0.35);
    painter.circle_filled(at, radius, dark);
    painter.circle_filled(at - Vec2::splat(radius * 0.05), radius * 0.9, color);
    painter.circle_stroke(at, radius * 0.6, Stroke::new(radius * 0.1, dark));
    painter.circle_filled(at + Vec2::new(-0.36, -0.38) * radius, radius * 0.16, Color32::WHITE.gamma_multiply(0.4 * color.a() as f32 / 255.0));
}

/// Four discs in a square, two of each color: the game's icon.
pub(super) fn paint_four_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let d = 3.6 * scale;
    for (k, (dx, dy)) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
        let who = if k == 0 || k == 3 { 1 } else { 2 };
        painter.circle_filled(c + Vec2::new(dx * d, dy * d), 3.1 * scale, disc_color(theme, who));
    }
}
