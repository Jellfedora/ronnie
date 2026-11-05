//! The Président, in the games (its rules in `crate::president`): against the computer, or online at a
//! table of floor, from 3 to 6. The lobby (how many, the computer's level; the players online to invite;
//! the rules), the table waiting for its players online, then the table: the others around the top
//! (their role, their cards left, whether they passed), the trick in the middle, the player's hand at
//! the bottom, a click picks cards, then Play or Pass. Between two rounds, the roles and the cards given.

use super::four::{level_name, HomeGame, LobbyAction};
use super::*;
use crate::connect4::Level;
use crate::president::{self as rules, Card, Dice, Role, Round};

/// The computer's moves, and once the player is out.
const AI_DELAY: f64 = 0.9;
const AI_DELAY_FAST: f64 = 0.45;

enum Mode {
    Lobby,
    /// Against the computer.
    Ai(Box<AiTable>),
    /// At the table of floor (`live`).
    Online,
}

enum Phase {
    Playing,
    /// The round over: the order out, the roles.
    Over,
    /// The player, Président or Vice, picks the cards to give back (the hands dealt, before the exchange).
    Give { hands: Vec<Vec<Card>>, roles: Vec<Role>, k: usize, to: usize },
}

/// A game against the computer: the player is seat 0.
struct AiTable {
    names: Vec<String>,
    level: Level,
    round: Round,
    phase: Phase,
    /// The roles of the round before, shown during this one.
    roles: Option<Vec<Role>>,
    ai_at: Option<f64>,
    dice: Dice,
    /// How many times each one was Président.
    presidencies: Vec<u32>,
    /// The trick just taken (its cards fade): when.
    cleared: Option<f64>,
}

pub(super) struct President {
    mode: Mode,
    level: Level,
    players: usize,
    /// The cards picked in the hand (or to give back).
    picked: Vec<Card>,
    /// What the exchange did for the player, said over the table for a while.
    note: Option<(String, f64)>,
    /// The table online followed (its page opened once), and its round whose exchange was said.
    pub(super) followed: Option<String>,
    said: Option<(String, u32)>,
    /// The chat of the table online, beside it.
    chat: super::chat::Chat,
}

impl Default for President {
    fn default() -> Self {
        Self { mode: Mode::Lobby, level: Level::Medium, players: 4, picked: Vec::new(), note: None, followed: None, said: None, chat: Default::default() }
    }
}

/// Where the table is, to show.
enum Stage {
    Playing,
    /// The round over; `next`: the player deals the next one, else who will.
    Over { next: bool, host: String },
    /// The player gives back `k` cards to `to`.
    Give { k: usize, to: usize },
    /// Others are picking the cards they give back.
    GiveWait(Vec<usize>),
    /// Online, the table stopped: why.
    Ended(String),
}

/// What the table shows, from the game against the computer or from floor's.
struct Show {
    names: Vec<String>,
    /// The player's seat.
    you: usize,
    /// The cards left of each one, the player's hand.
    counts: Vec<usize>,
    hand: Vec<Card>,
    turn: Option<usize>,
    top: Option<rules::Top>,
    laid: Vec<Card>,
    leader: Option<usize>,
    passed: Vec<bool>,
    out: Vec<usize>,
    /// The roles of the round before.
    roles: Option<Vec<Role>>,
    presidencies: Vec<u32>,
    stage: Stage,
    /// How faded the trick's cards are (taken).
    fade: f32,
}

/// What the player did at the table.
enum Act {
    Play(Vec<Card>),
    Pass,
    Give(Vec<Card>),
    Next,
    /// Back to the lobby (the table online stopped).
    Back,
}

fn card_of(c: [u8; 2]) -> Card {
    Card { rank: c[0], suit: c[1] }
}

fn wire(cards: &[Card]) -> Vec<[u8; 2]> {
    cards.iter().map(|c| [c.rank, c.suit]).collect()
}

fn role_of(name: &str) -> Role {
    match name {
        "president" => Role::President,
        "vice" => Role::Vice,
        "vice-trou" => Role::ViceTrou,
        "trou" => Role::Trou,
        _ => Role::Neutral,
    }
}

impl AiTable {
    fn new(n: usize, level: Level, names: Vec<String>, seed: u64) -> Self {
        let mut dice = Dice::new(seed);
        let hands = rules::deal(n, dice.below(n), &mut dice);
        // The 3 of clubs leads the first round.
        let lead = hands.iter().position(|h| h.contains(&Card::FIRST)).unwrap_or(0);
        Self { names, level, round: Round::new(hands, lead), phase: Phase::Playing, roles: None, ai_at: None, dice, presidencies: vec![0; n], cleared: None }
    }

    fn n(&self) -> usize {
        self.round.players()
    }

    /// The next round: dealt, the cards exchanged by the roles of this one (the player picks theirs to
    /// give back first, if Président or Vice); the Trou leads. What the exchange did for the player.
    fn next_round(&mut self, t: &Strings) -> Option<String> {
        let n = self.n();
        let roles = self.round.roles();
        let first = self.dice.below(n);
        let hands = rules::deal(n, first, &mut self.dice);
        let k = rules::exchange_count(roles[0]);
        if matches!(roles[0], Role::President | Role::Vice)
            && let Some(to) = rules::partner(&roles, 0)
        {
            self.phase = Phase::Give { hands, roles, k, to };
            return None;
        }
        self.start(hands, roles, None, t)
    }

    /// The exchange done (`mine`: the cards the player gives back), the round starts.
    fn start(&mut self, mut hands: Vec<Vec<Card>>, roles: Vec<Role>, mine: Option<Vec<Card>>, t: &Strings) -> Option<String> {
        let n = self.n();
        let before = hands[0].clone();
        let mut given_back = vec![None; n];
        given_back[0] = mine;
        rules::exchange(&mut hands, &roles, &given_back);
        let gave: Vec<Card> = before.iter().filter(|c| !hands[0].contains(c)).copied().collect();
        let got: Vec<Card> = hands[0].iter().filter(|c| !before.contains(c)).copied().collect();
        let note = rules::partner(&roles, 0).filter(|_| !gave.is_empty()).map(|q| swap_text(t, &gave, &self.names[q], &got));
        let lead = roles.iter().position(|&r| r == Role::Trou).unwrap_or(0);
        self.round = Round::new(hands, lead);
        self.roles = Some(roles);
        self.phase = Phase::Playing;
        self.ai_at = None;
        self.cleared = None;
        note
    }

    /// After a move: the trick taken (to fade what lay on it), the round over (the tally).
    fn after_move(&mut self, had_top: bool, now: f64) {
        if had_top && self.round.top.is_none() {
            self.cleared = Some(now);
        } else if self.round.top.is_some() {
            self.cleared = None;
        }
        if self.round.over() {
            self.phase = Phase::Over;
            self.presidencies[self.round.out[0]] += 1;
        }
    }

    /// The computer's move when its turn has come (after a pause). True while it is to play.
    fn computer(&mut self, now: f64) -> bool {
        if !matches!(self.phase, Phase::Playing) || self.round.over() || self.round.turn == 0 {
            self.ai_at = None;
            return false;
        }
        let delay = if self.round.out.contains(&0) { AI_DELAY_FAST } else { AI_DELAY };
        let at = *self.ai_at.get_or_insert(now + delay);
        if now < at {
            return true;
        }
        self.ai_at = None;
        let p = self.round.turn;
        let had_top = self.round.top.is_some();
        match rules::ai_move(&self.round, p, self.level, &mut self.dice) {
            Some(cards) => {
                self.round.play(p, &cards);
            }
            None => {
                self.round.pass(p);
            }
        }
        self.after_move(had_top, now);
        true
    }

    fn show(&self, now: f64) -> Show {
        let r = &self.round;
        let (hand, counts, stage) = match &self.phase {
            Phase::Give { hands, k, to, .. } => (hands[0].clone(), hands.iter().map(Vec::len).collect(), Stage::Give { k: *k, to: *to }),
            Phase::Over => (r.hands[0].clone(), r.hands.iter().map(Vec::len).collect(), Stage::Over { next: true, host: String::new() }),
            Phase::Playing => (r.hands[0].clone(), r.hands.iter().map(Vec::len).collect(), Stage::Playing),
        };
        let playing = matches!(self.phase, Phase::Playing);
        Show {
            names: self.names.clone(),
            you: 0,
            counts,
            hand,
            turn: (playing && !r.over()).then_some(r.turn),
            top: r.top,
            laid: if playing { r.laid.clone() } else { Vec::new() },
            leader: Some(r.leader),
            passed: r.passed.clone(),
            out: r.out.clone(),
            roles: self.roles.clone(),
            presidencies: self.presidencies.clone(),
            stage,
            fade: self.cleared.map_or(0.0, |at| (((now - at) / 0.6) as f32).clamp(0.0, 1.0) * 0.75),
        }
    }
}

/// "Exchange: you gave … to …, got …".
fn swap_text(t: &Strings, gave: &[Card], to: &str, got: &[Card]) -> String {
    t.president_gave.replace("{a}", &cards_text(gave)).replace("{n}", to).replace("{b}", &cards_text(got))
}

/// What floor's table shows the player.
fn show_online(table: &crate::live::Table, t: &Strings) -> Show {
    let names: Vec<String> = table.seats.iter().map(|s| s.name.clone().unwrap_or_else(|| t.game_anonymous.to_owned())).collect();
    let host = names.get(table.host.max(0) as usize).cloned().unwrap_or_default();
    let stage = match table.status.as_str() {
        "over" => Stage::Over { next: table.hosting(), host },
        "give" => match table.give {
            Some(g) => Stage::Give { k: g.k, to: g.to },
            None => Stage::GiveWait(table.waiting.clone()),
        },
        "ended" => {
            let who = table.left.clone().unwrap_or_default();
            Stage::Ended(match table.reason.as_deref() {
                Some("closed") => t.president_ended_closed.replace("{n}", &who),
                Some("left" | "away") => t.president_ended_left.replace("{n}", &who),
                _ => t.president_ended.to_owned(),
            })
        }
        _ => Stage::Playing,
    };
    let roles = table.seats.iter().all(|s| s.role.is_some()).then(|| table.seats.iter().map(|s| role_of(s.role.as_deref().unwrap_or_default())).collect());
    Show {
        you: table.you.max(0) as usize,
        counts: table.seats.iter().map(|s| s.cards).collect(),
        hand: table.hand.iter().copied().map(card_of).collect(),
        turn: table.turn,
        top: table.top.map(|top| rules::Top { rank: top.rank, count: top.count }),
        laid: if table.status == "playing" { table.laid.iter().copied().map(card_of).collect() } else { Vec::new() },
        leader: table.leader,
        passed: table.seats.iter().map(|s| s.passed).collect(),
        out: table.out.clone(),
        roles,
        presidencies: table.presidencies.clone(),
        stage,
        fade: if table.top.is_none() { 0.6 } else { 0.0 },
        names,
    }
}

/// A click on a card of the hand: picked or put back; when the trick asks for several, as many of its
/// value at once.
fn pick(show: &Show, picked: &mut Vec<Card>, card: Card) {
    if picked.contains(&card) {
        picked.retain(|c| *c != card);
        return;
    }
    if let Some(top) = show.top {
        let same: Vec<Card> = show.hand.iter().filter(|c| c.rank == card.rank).copied().collect();
        if same.len() >= top.count as usize {
            let mut out = vec![card];
            out.extend(same.into_iter().filter(|c| *c != card).take(top.count as usize - 1));
            *picked = out;
            return;
        }
    }
    // Another value than the picked ones: start over with it.
    if picked.first().is_some_and(|c| c.rank != card.rank) {
        picked.clear();
    }
    picked.push(card);
}

/// Whether `cards` may go on the trick (one value, as many as asked, stronger), for the buttons.
fn may_play(show: &Show, cards: &[Card]) -> bool {
    let Some(first) = cards.first() else { return false };
    cards.len() <= 4
        && cards.iter().all(|c| c.rank == first.rank && show.hand.contains(c))
        && show.top.is_none_or(|top| cards.len() == top.count as usize && first.rank > top.rank)
}

const FACE: Color32 = Color32::from_rgb(0xf6, 0xf3, 0xec);
const FACE_EDGE: Color32 = Color32::from_rgb(0xc9, 0xc3, 0xb6);
const RED: Color32 = Color32::from_rgb(0xd0, 0x2a, 0x2a);
const BLACK: Color32 = Color32::from_rgb(0x1c, 0x1e, 0x24);
/// A card's size in the hand.
const CARD: Vec2 = Vec2::new(62.0, 90.0);

/// A suit drawn around `c`, `s` its half-size: ♣ ♦ ♥ ♠.
fn paint_suit(painter: &egui::Painter, c: Pos2, s: f32, suit: u8, color: Color32) {
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let poly = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::convex_polygon(points.iter().map(|&(x, y)| at(x, y)).collect(), color, Stroke::NONE));
    };
    match suit {
        0 => {
            for (x, y) in [(0.0, -0.45), (-0.45, 0.08), (0.45, 0.08)] {
                painter.circle_filled(at(x, y), s * 0.42, color);
            }
            poly(&[(-0.12, 0.0), (0.12, 0.0), (0.3, 0.95), (-0.3, 0.95)]);
        }
        1 => poly(&[(0.0, -1.0), (0.72, 0.0), (0.0, 1.0), (-0.72, 0.0)]),
        2 => {
            painter.circle_filled(at(-0.42, -0.35), s * 0.47, color);
            painter.circle_filled(at(0.42, -0.35), s * 0.47, color);
            poly(&[(-0.88, -0.2), (0.88, -0.2), (0.0, 0.95)]);
        }
        _ => {
            painter.circle_filled(at(-0.42, 0.22), s * 0.44, color);
            painter.circle_filled(at(0.42, 0.22), s * 0.44, color);
            poly(&[(0.0, -1.0), (0.85, 0.12), (-0.85, 0.12)]);
            poly(&[(-0.1, 0.3), (0.1, 0.3), (0.32, 1.0), (-0.32, 1.0)]);
        }
    }
}

/// A card face up in `r`: its value in the corners, its suit big in the middle. `dim`: faded.
fn paint_card(painter: &egui::Painter, r: Rect, card: Card, dim: f32) {
    let alpha = 1.0 - dim;
    let k = r.width() / CARD.x;
    painter.rect_filled(r.translate(Vec2::new(1.5, 2.5) * k), 6.0 * k, Color32::from_black_alpha((70.0 * alpha) as u8));
    painter.rect(r, 6.0 * k, FACE.gamma_multiply(alpha), Stroke::new(1.0, FACE_EDGE.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    let color = if card.red() { RED } else { BLACK }.gamma_multiply(alpha);
    let font = FontId::proportional(17.0 * k);
    painter.text(r.min + Vec2::new(6.0, 4.0) * k, Align2::LEFT_TOP, card.label(), font.clone(), color);
    paint_suit(painter, r.min + Vec2::new(12.0, 30.0) * k, 5.0 * k, card.suit, color);
    painter.text(r.max - Vec2::new(6.0, 4.0) * k, Align2::RIGHT_BOTTOM, card.label(), font, color);
    paint_suit(painter, r.center() + Vec2::new(0.0, 4.0 * k), 13.0 * k, card.suit, color);
}

/// A card face down in `r`: the theme's color, a frame, a diamond in the middle.
fn paint_back(painter: &egui::Painter, r: Rect, theme: &Theme) {
    let k = r.width() / CARD.x;
    painter.rect_filled(r.translate(Vec2::new(1.0, 2.0) * k), 5.0 * k, Color32::from_black_alpha(60));
    painter.rect(r, 5.0 * k, theme.accent.lerp_to_gamma(theme.chrome_bg, 0.35), Stroke::new(1.0, theme.accent.lerp_to_gamma(Color32::WHITE, 0.3)), egui::StrokeKind::Inside);
    painter.rect_stroke(r.shrink(4.0 * k), 3.0 * k, Stroke::new(1.0, theme.bg.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    paint_suit(painter, r.center(), 8.0 * k, 1, theme.bg.gamma_multiply(0.45));
}

/// The icon: two cards fanned, a crown over them.
pub(super) fn paint_president_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let size = Vec2::new(9.0, 13.0) * s;
    for (dx, color) in [(-3.0, theme.text.gamma_multiply(0.55)), (3.0, theme.text.gamma_multiply(0.9))] {
        painter.rect_filled(Rect::from_center_size(c + Vec2::new(dx * s, 2.5 * s), size), 1.8 * s, color);
    }
    paint_suit(painter, c + Vec2::new(3.0 * s, 3.0 * s), 2.6 * s, 2, theme.bg);
    // The crown.
    let base = c + Vec2::new(0.0, -6.5 * s);
    let at = |x: f32, y: f32| base + Vec2::new(x, y) * s;
    painter.add(egui::Shape::convex_polygon(vec![at(-5.0, 1.5), at(5.0, 1.5), at(5.5, -2.5), at(-5.5, -2.5)], theme.accent, Stroke::NONE));
    for x in [-5.0, 0.0, 5.0] {
        painter.add(egui::Shape::convex_polygon(vec![at(x - 1.6, -2.0), at(x + 1.6, -2.0), at(x, -5.0)], theme.accent, Stroke::NONE));
    }
}

/// A role's name.
fn role_name(t: &Strings, role: Role) -> &'static str {
    let names: Vec<&'static str> = t.president_roles.split('|').collect();
    let k = match role {
        Role::President => 0,
        Role::Vice => 1,
        Role::Neutral => 2,
        Role::ViceTrou => 3,
        Role::Trou => 4,
    };
    names.get(k).copied().unwrap_or_default()
}

fn role_color(role: Role, theme: &Theme) -> Color32 {
    match role {
        Role::President => theme.ansi[3],
        Role::Vice => theme.ansi[6],
        Role::Neutral => theme.text_muted,
        Role::ViceTrou => theme.ansi[5],
        Role::Trou => theme.ansi[1],
    }
}

/// Cards as text: "D♥ D♠".
fn cards_text(cards: &[Card]) -> String {
    cards.iter().map(|c| format!("{}{}", c.label(), ["♣", "♦", "♥", "♠"][c.suit as usize])).collect::<Vec<_>>().join(" ")
}

impl App {
    pub(super) fn open_president(&mut self) {
        self.open_game();
        self.home_which = HomeGame::President;
    }

    /// To the table online (an invitation accepted, a table opened by inviting).
    pub(super) fn president_online(&mut self) {
        self.president.mode = Mode::Online;
        self.president.picked.clear();
        self.open_president();
    }

    /// The page: the lobby, the table against the computer, or the table online.
    pub(super) fn president_page(&mut self, ui: &mut Ui, rect: Rect) {
        let playing = match &self.president.mode {
            Mode::Ai(table) => matches!(table.phase, Phase::Playing),
            Mode::Online => self.versus.table.as_ref().is_some_and(|t| t.status == "playing"),
            Mode::Lobby => false,
        };
        if playing {
            self.count_round(HomeGame::President);
        } else if matches!(self.president.mode, Mode::Lobby)
            && let Some(p) = self.game_locked(HomeGame::President)
        {
            return self.locked_page(ui, rect, HomeGame::President, &p);
        }
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.president_name, 40.0, theme.accent, 1.0);
        // Back: from a game to the lobby, from the lobby home. From a table online: up from it.
        let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(150.0, 28.0));
        let online = matches!(self.president.mode, Mode::Online);
        let label = if online && self.versus.table.as_ref().is_some_and(|t| t.status != "ended") { t.president_leave } else { t.home_back };
        if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {label}")).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
            match self.president.mode {
                Mode::Lobby => self.home_game = false,
                Mode::Ai(_) => self.president.mode = Mode::Lobby,
                Mode::Online => self.president_leave_table(),
            }
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
        match self.president.mode {
            Mode::Lobby => self.president_lobby(ui, body),
            Mode::Ai(_) => self.president_ai(ui, body, now),
            Mode::Online => self.president_table_online(ui, body, now),
        }
    }

    /// Up from the table online (before the game, the seat freed; during, the table stops), to the lobby.
    fn president_leave_table(&mut self) {
        if self.versus.table.as_ref().is_some_and(|t| t.status != "ended") || self.versus.seated.is_some() {
            self.versus.table_leave();
        }
        self.versus.watch_table(None);
        self.president.mode = Mode::Lobby;
    }

    fn president_lobby(&mut self, ui: &mut Ui, body: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
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
        let (level, players) = (self.president.level, self.president.players);
        let (mut picked_level, mut picked_players, mut play) = (None, None, false);
        let mut ai = |ui: &mut Ui| {
            ui.label(egui::RichText::new(t.four_vs_ai).size(16.0).strong());
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                for n in 3..=6 {
                    let b = egui::Button::selectable(n == players, egui::RichText::new(t.president_players.replace("{n}", &n.to_string())).size(13.0)).min_size(Vec2::new(86.0, 28.0)).corner_radius(6.0);
                    if ui.add(b).clicked() {
                        picked_players = Some(n);
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for l in Level::ALL {
                    let b = egui::Button::selectable(l == level, egui::RichText::new(level_name(t, l)).size(13.0)).min_size(Vec2::new(86.0, 28.0)).corner_radius(6.0);
                    if ui.add(b).clicked() {
                        picked_level = Some(l);
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
        // Online: the players to invite to the player's table, or back to it.
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let live = &self.versus;
        let faces = super::frames::Faces { avatars: &self.avatars, account: self.config.settings.floor.as_ref() };
        let mut action = None;
        let mut back_to_table = false;
        let mut online = |ui: &mut Ui| {
            ui.label(egui::RichText::new(t.president_online_title).size(16.0).strong());
            ui.add_space(8.0);
            if live.seated.is_some() {
                ui.label(egui::RichText::new(t.president_seated).size(13.0).color(theme.text_muted));
                ui.add_space(10.0);
                if ui.add(egui::Button::new(egui::RichText::new(t.president_back_table).size(13.5).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0).min_size(Vec2::new(160.0, 32.0))).clicked() {
                    back_to_table = true;
                }
                return;
            }
            ui.label(egui::RichText::new(t.president_invite_hint).size(13.0).color(theme.text_muted));
            ui.add_space(10.0);
            action = invite_list(ui, live, &faces, &[], pseudo.as_deref(), t, &theme);
        };
        let mut rules_card = |ui: &mut Ui| {
            ui.label(egui::RichText::new(t.larsen_rules_title).size(16.0).strong());
            ui.add_space(10.0);
            ui.label(egui::RichText::new(t.president_rules).size(13.0).color(theme.text_muted));
            ui.add_space(12.0);
            // The values, weakest to strongest.
            let (strip, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::hover());
            let size = Vec2::new(30.0, 44.0);
            let step = ((strip.width() - size.x) / 12.0).min(size.x + 4.0);
            for rank in 0..rules::RANKS {
                let r = Rect::from_min_size(strip.min + Vec2::new(step * rank as f32, 2.0), size);
                paint_card(ui.painter(), r, Card { rank, suit: (rank % 4 + 1) % 4 }, 0.0);
            }
        };
        egui::ScrollArea::vertical().id_salt("president-lobby").auto_shrink([false, false]).show(&mut child, |ui| {
            if wide {
                ui.horizontal_top(|ui| {
                    card(ui, card_w, &mut ai);
                    ui.add_space(16.0);
                    card(ui, card_w, &mut online);
                });
            } else {
                card(ui, card_w, &mut ai);
                ui.add_space(16.0);
                card(ui, card_w, &mut online);
            }
            ui.add_space(16.0);
            card(ui, width, &mut rules_card);
        });
        if let Some(l) = picked_level {
            self.president.level = l;
        }
        if let Some(n) = picked_players {
            self.president.players = n;
        }
        if play {
            self.new_president(ui.input(|i| i.time));
        }
        if back_to_table {
            self.president_online();
        }
        match action {
            // The first invitation opens the player's table on floor: to it.
            Some(LobbyAction::Invite(name)) => {
                self.versus.invite_to(&name, "president");
                self.president_online();
            }
            Some(LobbyAction::Pseudo) => self.ask_pseudo(),
            _ => {}
        }
    }

    /// A table against the computer: the player and its others, named.
    fn new_president(&mut self, now: f64) {
        if self.game_locked(HomeGame::President).is_some() {
            self.president.mode = Mode::Lobby;
            return;
        }
        let t = self.t();
        let n = self.president.players;
        let you = crate::floor::pseudo(self.config.settings.floor.as_ref()).unwrap_or(t.game_you).to_owned();
        let mut names = vec![you];
        names.extend(t.president_names.split('|').take(n - 1).map(str::to_owned));
        let seed = (now * 1000.0) as u64 ^ 0x9e37_79b9_7f4a_7c15;
        self.president.picked.clear();
        self.president.note = None;
        self.president.mode = Mode::Ai(Box::new(AiTable::new(n, self.president.level, names, seed)));
    }

    fn president_ai(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let Mode::Ai(table) = &mut self.president.mode else { return };
        if table.computer(now) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        }
        if table.cleared.is_some_and(|at| now - at < 0.7) {
            ui.ctx().request_repaint();
        }
        let show = table.show(now);
        let Some(act) = self.president_paint(ui, body, &show, now) else { return };
        let Mode::Ai(table) = &mut self.president.mode else { return };
        match act {
            Act::Play(cards) => {
                let had_top = table.round.top.is_some();
                table.round.play(0, &cards);
                table.after_move(had_top, now);
            }
            Act::Pass => {
                table.round.pass(0);
                table.after_move(true, now);
            }
            Act::Give(mine) => {
                if let Phase::Give { hands, roles, .. } = std::mem::replace(&mut table.phase, Phase::Playing) {
                    self.president.note = table.start(hands, roles, Some(mine), t).map(|n| (n, now));
                }
            }
            Act::Next => {
                if self.game_locked(HomeGame::President).is_some() {
                    self.president.mode = Mode::Lobby;
                } else if let Mode::Ai(table) = &mut self.president.mode {
                    self.president.note = table.next_round(t).map(|n| (n, now));
                }
            }
            Act::Back => self.president.mode = Mode::Lobby,
        }
    }

    /// The table online: waiting for its players, or the game.
    fn president_table_online(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(table) = self.versus.table.clone() else {
            // Not there yet (an invitation just sent or accepted), or gone.
            let text = self.versus.refused.clone().unwrap_or_else(|| t.four_connecting.to_owned());
            ui.painter().text(body.center(), Align2::CENTER_CENTER, text, FontId::proportional(15.0), theme.text_muted);
            if self.versus.seated.is_none() && self.versus.refused.is_some() && ui.put(Rect::from_center_size(body.center() + Vec2::new(0.0, 44.0), Vec2::new(170.0, 34.0)), egui::Button::new(t.home_back)).clicked() {
                self.president.mode = Mode::Lobby;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            return;
        };
        // The chat on the right, the table in what's left.
        let chat_w = (body.width() * 0.24).clamp(190.0, 260.0);
        let chat_area = Rect::from_min_max(Pos2::new(body.max.x - chat_w, body.min.y + 20.0), Pos2::new(body.max.x, body.max.y - 16.0));
        let lines: Vec<super::chat::Line> = table.chat.iter().map(|(from, text, mine)| super::chat::Line { from: from.as_deref().unwrap_or(t.game_anonymous), text, mine: *mine, age: None }).collect();
        if let Some(text) = self.president.chat.ui(ui, chat_area, &lines, true, true, "president", &theme, t) {
            self.versus.table_say(&text);
        }
        let body = Rect::from_min_max(body.min, Pos2::new(chat_area.min.x - 16.0, body.max.y));
        if table.status == "waiting" {
            return self.president_waiting(ui, body, &table);
        }
        // A new round after the exchange: what the player gave and got, said once.
        let key = (table.id.clone(), table.rounds);
        if table.status == "playing" && self.president.said.as_ref() != Some(&key) {
            self.president.said = Some(key);
            self.president.picked.clear();
            if let Some(swap) = &table.swap {
                let with = table.seats.get(swap.with).and_then(|s| s.name.clone()).unwrap_or_default();
                let gave: Vec<Card> = swap.gave.iter().copied().map(card_of).collect();
                let got: Vec<Card> = swap.got.iter().copied().map(card_of).collect();
                self.president.note = Some((swap_text(t, &gave, &with, &got), now));
            }
        }
        // Cards picked that left the hand (played, given): forgotten.
        let hand: Vec<Card> = table.hand.iter().copied().map(card_of).collect();
        self.president.picked.retain(|c| hand.contains(c));
        let show = show_online(&table, t);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        match self.president_paint(ui, body, &show, now) {
            Some(Act::Play(cards)) => self.versus.table_play(wire(&cards)),
            Some(Act::Pass) => self.versus.table_pass(),
            Some(Act::Give(cards)) => self.versus.table_give(wire(&cards)),
            Some(Act::Next) => self.versus.table_next(),
            Some(Act::Back) => self.president_leave_table(),
            None => {}
        }
    }

    /// The table before the game: who sits at it, who is invited; the host invites more and starts.
    fn president_waiting(&mut self, ui: &mut Ui, body: Rect, table: &crate::live::Table) {
        let t = self.t();
        let theme = self.theme.clone();
        let host = table.seats.get(table.host.max(0) as usize).and_then(|s| s.name.clone()).unwrap_or_default();
        let width = body.width().min(560.0);
        let area = Rect::from_min_size(Pos2::new(body.center().x - width / 2.0, body.min.y), Vec2::new(width, body.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let (mut action, mut start) = (None, false);
        let live = &self.versus;
        let faces = super::frames::Faces { avatars: &self.avatars, account: self.config.settings.floor.as_ref() };
        Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(18.0).show(&mut child, |ui| {
            ui.set_width(width - 38.0);
            let title = if table.hosting() { t.president_table.to_owned() } else { t.president_host_table.replace("{n}", &host) };
            ui.label(egui::RichText::new(title).size(16.0).strong());
            ui.add_space(10.0);
            for (k, seat) in table.seats.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    ui.label(egui::RichText::new("●").size(10.0).color(if seat.online { theme.ansi[2] } else { theme.text_muted }));
                    ui.label(egui::RichText::new(seat.name.as_deref().unwrap_or(t.game_anonymous)).size(14.0).color(if k as i32 == table.you { theme.accent } else { theme.text }));
                    if k as i32 == table.host {
                        ui.label(egui::RichText::new(t.president_host).size(11.5).color(theme.text_muted));
                    }
                });
            }
            for name in table.invited.iter().flatten() {
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    ui.label(egui::RichText::new("○").size(10.0).color(theme.text_muted));
                    ui.label(egui::RichText::new(name).size(14.0).color(theme.text_muted));
                    ui.label(egui::RichText::new(t.president_invited_wait).size(11.5).italics().color(theme.text_muted));
                });
            }
            ui.add_space(14.0);
            if table.hosting() {
                let enough = table.seats.len() >= 3;
                let b = egui::Button::new(egui::RichText::new(t.president_start).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0).min_size(Vec2::new(170.0, 34.0));
                if ui.add_enabled(enough, b).on_disabled_hover_text(t.president_need_three).clicked() {
                    start = true;
                }
                if table.seats.len() < 6 {
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(t.president_invite_more).size(13.0).color(theme.text_muted));
                    ui.add_space(6.0);
                    let taken: Vec<&str> = table.seats.iter().filter_map(|s| s.name.as_deref()).chain(table.invited.iter().flatten().map(String::as_str)).collect();
                    action = invite_list(ui, live, &faces, &taken, pseudo.as_deref(), t, &theme);
                }
            } else {
                ui.label(egui::RichText::new(t.president_wait_host.replace("{n}", &host)).size(13.5).color(theme.text_muted));
            }
        });
        if start {
            self.versus.table_start();
        }
        if let Some(LobbyAction::Invite(name)) = action {
            self.versus.invite_to(&name, "president");
        }
        if let Some(e) = &self.versus.refused {
            ui.painter().text(Pos2::new(body.center().x, body.max.y - 20.0), Align2::CENTER_CENTER, e, FontId::proportional(13.0), theme.ansi[1]);
        }
    }

    /// The table, from either side: the status on top, the others around, the trick in the middle, the
    /// player's hand and their buttons at the bottom, the results once the round is over. What the
    /// player did.
    fn president_paint(&mut self, ui: &mut Ui, body: Rect, show: &Show, now: f64) -> Option<Act> {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let n = show.names.len();
        let you = show.you;
        let your_turn = matches!(show.stage, Stage::Playing) && show.turn == Some(you);
        let name = |p: usize| show.names.get(p).cloned().unwrap_or_default();

        // The status, on top.
        let status = match &show.stage {
            Stage::Over { next: true, .. } => t.president_round_over.to_owned(),
            Stage::Over { host, .. } => t.president_wait_next.replace("{n}", host),
            Stage::Give { k, to } => t.president_give.replace("{k}", &k.to_string()).replace("{n}", &name(*to)),
            Stage::GiveWait(who) => t.president_wait_give.replace("{n}", &who.iter().map(|&p| name(p)).collect::<Vec<_>>().join(", ")),
            Stage::Ended(why) => why.clone(),
            Stage::Playing if show.out.contains(&you) => {
                let place = show.out.iter().position(|&p| p == you).unwrap_or(0);
                t.president_you_out.replace("{r}", role_name(t, rules::role(place, n)))
            }
            Stage::Playing if your_turn && show.top.is_none() => t.president_your_lead.to_owned(),
            Stage::Playing if your_turn => t.president_your_turn.to_owned(),
            Stage::Playing => t.president_their_turn.replace("{n}", &show.turn.map(name).unwrap_or_default()),
        };
        let status_color = match show.stage {
            Stage::Ended(_) => theme.ansi[1],
            Stage::Give { .. } => theme.accent,
            _ if your_turn => theme.accent,
            _ => theme.text,
        };
        painter.text(Pos2::new(body.center().x, body.min.y + 4.0), Align2::CENTER_CENTER, status, FontId::proportional(19.0), status_color);
        if your_turn {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }

        // The others, around the top, in the order of play after the player.
        let seats_top = body.min.y + 28.0;
        let others: Vec<usize> = (1..n).map(|k| (you + k) % n).collect();
        let gaps = 12.0 * others.len().saturating_sub(1) as f32;
        let seat_w = ((body.width() - gaps) / others.len().max(1) as f32).min(190.0);
        let row_w = seat_w * others.len() as f32 + gaps;
        for (k, &p) in others.iter().enumerate() {
            let seat = Rect::from_min_size(Pos2::new(body.center().x - row_w / 2.0 + k as f32 * (seat_w + 12.0), seats_top), Vec2::new(seat_w, 104.0));
            self.president_seat(ui, seat, show, p, now);
        }

        // The trick, in the middle.
        let middle = Rect::from_min_max(Pos2::new(body.min.x, seats_top + 116.0), Pos2::new(body.max.x, body.max.y - CARD.y - 96.0));
        if !show.laid.is_empty() {
            let size = CARD * 0.95;
            let total = size.x + (show.laid.len() - 1) as f32 * 30.0;
            for (k, card) in show.laid.iter().enumerate() {
                let r = Rect::from_min_size(Pos2::new(middle.center().x - total / 2.0 + k as f32 * 30.0, middle.center().y - size.y / 2.0), size);
                paint_card(&painter, r, *card, show.fade);
            }
            if let Some(leader) = show.leader {
                painter.text(Pos2::new(middle.center().x, middle.center().y + size.y / 2.0 + 14.0), Align2::CENTER_CENTER, name(leader), FontId::proportional(13.0), theme.text_muted.gamma_multiply(1.0 - show.fade));
            }
        }
        // What the exchange did, for a few seconds.
        if let Some((text, at)) = &self.president.note {
            let age = now - at;
            if age < 6.0 {
                let alpha = (1.0 - ((age - 5.0).max(0.0)) as f32).clamp(0.0, 1.0);
                let galley = painter.layout(text.clone(), FontId::proportional(14.0), theme.text.gamma_multiply(alpha), middle.width() * 0.8);
                let r = Rect::from_center_size(Pos2::new(middle.center().x, middle.min.y + 18.0), galley.size() + Vec2::new(24.0, 12.0));
                painter.rect_filled(r, 8.0, theme.chrome_bg.gamma_multiply(alpha));
                painter.galley(r.min + Vec2::new(12.0, 6.0), galley, theme.text);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            } else {
                self.president.note = None;
            }
        }
        if matches!(show.stage, Stage::Over { .. }) {
            self.president_results(ui, middle, show);
        }

        // The player's hand, at the bottom; a click picks a card.
        let giving = matches!(show.stage, Stage::Give { .. });
        let can_pick = your_turn || giving;
        let hand = &show.hand;
        let hand_y = body.max.y - CARD.y - 56.0;
        let step = if hand.len() > 1 { ((body.width() - 40.0 - CARD.x) / (hand.len() - 1) as f32).min(CARD.x + 6.0) } else { 0.0 };
        let total = CARD.x + step * hand.len().saturating_sub(1) as f32;
        let rects: Vec<Rect> = (0..hand.len()).map(|k| Rect::from_min_size(Pos2::new(body.center().x - total / 2.0 + k as f32 * step, hand_y), CARD)).collect();
        // The card under the pointer: the last drawn (the rightmost) that contains it.
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let hovered = pointer.filter(|_| can_pick).and_then(|at| rects.iter().rposition(|r| r.expand2(Vec2::new(0.0, 14.0)).contains(at)));
        for (k, card) in hand.iter().enumerate() {
            let picked = self.president.picked.contains(card);
            let lift = if picked { 18.0 } else if hovered == Some(k) { 6.0 } else { 0.0 };
            let r = rects[k].translate(Vec2::new(0.0, -lift));
            paint_card(&painter, r, *card, 0.0);
            if picked {
                painter.rect_stroke(r, 6.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Outside);
            }
        }
        if can_pick && !hand.is_empty() {
            let hand_rect = rects.iter().fold(Rect::NOTHING, |a, r| a.union(*r)).expand2(Vec2::new(0.0, 14.0));
            let resp = ui.interact(hand_rect, ui.id().with("president-hand"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked()
                && let Some(card) = hovered.map(|k| hand[k])
            {
                match show.stage {
                    Stage::Give { k, .. } => {
                        let picked = &mut self.president.picked;
                        if picked.contains(&card) {
                            picked.retain(|c| *c != card);
                        } else if picked.len() < k {
                            picked.push(card);
                        }
                    }
                    _ => pick(show, &mut self.president.picked, card),
                }
            }
        }

        // The player's seat (name, role), and the buttons under the hand.
        let mut label = name(you);
        if let Some(role) = show.roles.as_ref().and_then(|r| r.get(you)) {
            label = format!("{label}  ·  {}", role_name(t, *role));
        }
        painter.text(Pos2::new(body.min.x, hand_y - 26.0), Align2::LEFT_CENTER, label, FontId::proportional(14.0), if your_turn { theme.accent } else { theme.text });
        let buttons_y = body.max.y - 22.0;
        let button = |ui: &mut Ui, x: f32, text: &str, strong: bool, enabled: bool| -> bool {
            let r = Rect::from_center_size(Pos2::new(x, buttons_y), Vec2::new(150.0, 34.0));
            let b = if strong { egui::Button::new(egui::RichText::new(text).size(14.0).strong().color(theme.bg)).fill(theme.accent) } else { egui::Button::new(egui::RichText::new(text).size(14.0)) };
            ui.put(r, |ui: &mut Ui| ui.add_enabled(enabled, b.corner_radius(8.0))).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        };
        let cx = body.center().x;
        match &show.stage {
            Stage::Playing if your_turn => {
                // Nothing of the hand can go: Pass is the one lit.
                let can_play = show.top.is_none_or(|top| {
                    let need = top.count as usize;
                    hand.iter().any(|c| {
                        let same: Vec<Card> = hand.iter().filter(|x| x.rank == c.rank).copied().collect();
                        same.len() >= need && may_play(show, &same[..need])
                    })
                });
                if button(ui, cx - 82.0, t.president_play, can_play, may_play(show, &self.president.picked)) {
                    return Some(Act::Play(std::mem::take(&mut self.president.picked)));
                }
                if button(ui, cx + 82.0, t.president_pass, !can_play, show.top.is_some()) {
                    self.president.picked.clear();
                    return Some(Act::Pass);
                }
            }
            Stage::Give { k, .. } if button(ui, cx, t.president_give_ok, true, self.president.picked.len() == *k) => {
                return Some(Act::Give(std::mem::take(&mut self.president.picked)));
            }
            Stage::Over { next: true, .. } if button(ui, cx, t.president_next, true, true) => return Some(Act::Next),
            Stage::Ended(_) if button(ui, cx, t.home_back, true, true) => return Some(Act::Back),
            _ => {}
        }
        None
    }

    /// One of the others at the table, in `seat`: their name, their role, their cards face down, passed or out.
    fn president_seat(&self, ui: &mut Ui, seat: Rect, show: &Show, p: usize, now: f64) {
        let t = self.t();
        let theme = &self.theme;
        let painter = ui.painter();
        let n = show.names.len();
        let turn = matches!(show.stage, Stage::Playing) && show.turn == Some(p);
        painter.rect(seat, 12.0, theme.chrome_bg, Stroke::new(if turn { 2.0 } else { 1.0 }, if turn { theme.accent } else { theme.tab_hover }), egui::StrokeKind::Inside);
        if turn {
            // Its turn: the frame breathes.
            let k = ((now * 3.0).sin() * 0.5 + 0.5) as f32;
            painter.rect_stroke(seat.expand(2.0 + 2.0 * k), 14.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.4 * (1.0 - k))), egui::StrokeKind::Outside);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        painter.text(seat.min + Vec2::new(12.0, 16.0), Align2::LEFT_CENTER, &show.names[p], FontId::proportional(14.0), if turn { theme.text } else { theme.text.gamma_multiply(0.85) });
        if let Some(role) = show.roles.as_ref().and_then(|r| r.get(p)) {
            painter.text(Pos2::new(seat.max.x - 12.0, seat.min.y + 16.0), Align2::RIGHT_CENTER, role_name(t, *role), FontId::proportional(11.5), role_color(*role, theme));
        }
        if let Some(place) = show.out.iter().position(|&q| q == p) {
            // Out: the role it gets.
            let role = rules::role(place, n);
            paint_metal(painter, Pos2::new(seat.center().x, seat.center().y + 14.0), Align2::CENTER_CENTER, role_name(t, role), 18.0, role_color(role, theme), 1.0);
            return;
        }
        // Its cards face down, fanned, and how many.
        let hand = show.counts.get(p).copied().unwrap_or(0);
        let size = Vec2::new(22.0, 32.0);
        let shown = hand.min(12);
        let step = if shown > 1 { ((seat.width() - 60.0 - size.x) / (shown - 1) as f32).min(9.0) } else { 0.0 };
        for k in 0..shown {
            let r = Rect::from_min_size(Pos2::new(seat.min.x + 12.0 + k as f32 * step, seat.min.y + 34.0), size);
            paint_back(painter, r, theme);
        }
        painter.text(Pos2::new(seat.max.x - 12.0, seat.min.y + 50.0), Align2::RIGHT_CENTER, hand.to_string(), FontId::proportional(18.0), theme.text);
        let live = show.top.is_some() && matches!(show.stage, Stage::Playing);
        if live && show.passed.get(p).copied().unwrap_or(false) {
            painter.text(Pos2::new(seat.center().x, seat.max.y - 14.0), Align2::CENTER_CENTER, t.president_passed, FontId::proportional(12.5), theme.text_muted);
        } else if live && show.leader == Some(p) {
            painter.text(Pos2::new(seat.center().x, seat.max.y - 14.0), Align2::CENTER_CENTER, cards_text(&show.laid), FontId::proportional(12.5), theme.accent);
        }
    }

    /// The round over: each one's place and role, the Présidents counted.
    fn president_results(&self, ui: &mut Ui, area: Rect, show: &Show) {
        let t = self.t();
        let theme = &self.theme;
        let n = show.names.len();
        let painter = ui.painter();
        let h = 30.0 + n as f32 * 26.0;
        let panel = Rect::from_center_size(area.center(), Vec2::new(360.0, h));
        painter.rect(panel, 12.0, theme.chrome_bg, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        painter.text(Pos2::new(panel.max.x - 16.0, panel.min.y + 18.0), Align2::RIGHT_CENTER, t.president_tally, FontId::proportional(11.5), theme.text_muted);
        for (place, &p) in show.out.iter().enumerate() {
            let y = panel.min.y + 44.0 + place as f32 * 26.0;
            let role = rules::role(place, n);
            painter.text(Pos2::new(panel.min.x + 16.0, y), Align2::LEFT_CENTER, format!("{}.", place + 1), FontId::proportional(13.0), theme.text_muted);
            painter.text(Pos2::new(panel.min.x + 40.0, y), Align2::LEFT_CENTER, &show.names[p], FontId::proportional(14.0), if p == show.you { theme.accent } else { theme.text });
            painter.text(Pos2::new(panel.min.x + 190.0, y), Align2::LEFT_CENTER, role_name(t, role), FontId::proportional(13.0), role_color(role, theme));
            painter.text(Pos2::new(panel.max.x - 16.0, y), Align2::RIGHT_CENTER, show.presidencies.get(p).copied().unwrap_or(0).to_string(), FontId::proportional(13.0), theme.text_muted);
        }
    }
}

/// The players online to invite to the table (not those in `taken`, seated or invited): without a
/// pseudo, a button to pick one; floor not reached, or nobody.
#[allow(clippy::too_many_arguments)]
fn invite_list(ui: &mut Ui, live: &crate::live::Live, faces: &super::frames::Faces, taken: &[&str], pseudo: Option<&str>, t: &Strings, theme: &Theme) -> Option<LobbyAction> {
    if pseudo.is_none() {
        ui.label(egui::RichText::new(t.four_need_pseudo).size(13.0).color(theme.text_muted));
        ui.add_space(8.0);
        return ui.button(egui::RichText::new(t.pseudo_pick).size(13.0)).clicked().then_some(LobbyAction::Pseudo);
    }
    match live.reachable {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(t.four_connecting).size(13.0).color(theme.text_muted));
            });
            return None;
        }
        Some(false) => {
            ui.label(egui::RichText::new(t.four_offline).size(13.0).color(theme.ansi[1]));
            return None;
        }
        Some(true) => {}
    }
    let players: Vec<&crate::live::Online> = live.online.iter().filter(|p| !taken.contains(&p.name.as_str())).collect();
    if players.is_empty() {
        ui.label(egui::RichText::new(t.four_nobody).size(13.0).color(theme.text_muted));
    }
    let mut action = None;
    egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
        for player in players {
            ui.horizontal(|ui| {
                ui.set_min_height(30.0);
                super::frames::row_avatar(ui, 10.0, &player.name, player.id.as_deref(), player.avatar.as_deref(), player.frame.as_deref(), faces, theme);
                ui.label(egui::RichText::new(&player.name).size(13.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if player.busy {
                        ui.label(egui::RichText::new(t.four_busy).size(12.0).color(theme.text_muted));
                    } else if ui.add(egui::Button::new(egui::RichText::new(t.four_invite).size(12.5)).corner_radius(6.0)).clicked() {
                        action = Some(LobbyAction::Invite(player.name.clone()));
                    }
                });
            });
        }
    });
    action
}
