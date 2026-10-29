//! Riff Hero, in the games: the song (see `crate::riff`) comes down four strings towards a line, the
//! arrows play it (← ↓ ↑ →, or a click on the arrows of the line). Alone, a song per level, the same
//! for everyone, and its board on floor; or in a battle against another player online (see live.rs):
//! the same song at the same time, and the crowd between the two goes over to the best.

use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::four::{players_ui, wins_ui, HomeGame, LobbyAction};
use super::*;
use crate::live::{Match, RiffSide};
use crate::riff::{Chart, Judgment, Level, Play, Press, LANES, LENGTH};

/// A note shows this long before it reaches the line.
const AHEAD: f64 = 2.4;
/// The countdown before a song played alone.
const LEAD: f64 = 3.0;
/// Where the player is, sent to floor this often during a battle.
const SEND_EVERY: f64 = 1.0;
/// A judgment shows this long above its arrow.
const POP: f64 = 0.55;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Lobby,
    Solo,
    Battle,
}

/// A song being played.
struct Song {
    play: Play,
    /// Its first beat.
    start: Instant,
    /// The battle's match; None: alone.
    battle: Option<String>,
    /// Its sound, made in a thread while the countdown runs.
    samples: Option<Arc<Vec<f32>>>,
    rendering: Option<mpsc::Receiver<Vec<f32>>>,
    /// When each arrow was last pressed (song time), to light it.
    pressed: [f64; LANES],
    /// The presses' judgments, shown a moment above their arrow: song time, string, judgment.
    pops: Vec<(f64, usize, Judgment)>,
    /// The notes hit, a flash on the line: song time, string.
    bursts: Vec<(f64, usize)>,
    /// When where the player is was last sent (song time).
    sent_at: f64,
    /// No sound could be played (no output device): not tried again for this song.
    mute: bool,
    /// Played to the end (or the battle over): the score is kept, or sent as final.
    finished: bool,
    /// It beat the best score alone.
    record: bool,
}

impl Song {
    fn new(ctx: &egui::Context, chart: Chart, start: Instant, battle: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (notes, ctx) = (chart.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("riff-sound".into())
            .spawn(move || {
                let _ = tx.send(crate::riff::render(&notes));
                ctx.request_repaint();
            })
            .ok();
        Self { play: Play::new(chart), start, battle, samples: None, rendering: Some(rx), pressed: [f64::NEG_INFINITY; LANES], pops: Vec::new(), bursts: Vec::new(), sent_at: f64::NEG_INFINITY, mute: false, finished: false, record: false }
    }

    /// Seconds since the first beat (negative: still counting down).
    fn time(&self) -> f64 {
        let now = Instant::now();
        match now.checked_duration_since(self.start) {
            Some(d) => d.as_secs_f64(),
            None => -(self.start - now).as_secs_f64(),
        }
    }

    fn side(&self, done: bool) -> RiffSide {
        let p = &self.play;
        RiffSide { score: p.score, combo: p.combo, hits: p.hits, misses: p.misses, done }
    }

    /// String `lane` played at `t`.
    fn press(&mut self, lane: usize, t: f64) -> Press {
        self.pressed[lane] = t;
        let press = self.play.press(lane, t);
        match press {
            Press::Hit(j) => {
                self.pops.push((t, lane, j));
                self.bursts.push((t, lane));
            }
            Press::Stray => {}
        }
        press
    }
}

/// The song's sound playing, and a mixer for the thud of a note missed. Dropped: silence.
struct Audio {
    _device: rodio::MixerDeviceSink,
    mixer: rodio::mixer::Mixer,
    _player: rodio::Player,
}

impl Audio {
    /// The song from `from` seconds in.
    fn start(samples: &[f32], from: f64) -> Option<Self> {
        let mut device = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| crate::log::error(&format!("Riff Hero sound: {e}"))).ok()?;
        device.log_on_drop(false);
        let mixer = device.mixer().clone();
        let player = rodio::Player::connect_new(&mixer);
        let skip = ((from.max(0.0) * crate::riff::RATE as f64) as usize).min(samples.len());
        let (channels, rate) = (std::num::NonZero::new(1)?, std::num::NonZero::new(crate::riff::RATE)?);
        player.append(rodio::buffer::SamplesBuffer::new(channels, rate, samples[skip..].to_vec()));
        Some(Self { _device: device, mixer, _player: player })
    }

    fn thud(&self) {
        let (Some(channels), Some(rate)) = (std::num::NonZero::new(1), std::num::NonZero::new(crate::riff::RATE)) else { return };
        self.mixer.add(rodio::buffer::SamplesBuffer::new(channels, rate, crate::riff::thud()));
    }
}

pub(super) struct Riff {
    mode: Mode,
    song: Option<Song>,
    audio: Option<Audio>,
    /// The page was last drawn at this app time: out of sight, the sound stops.
    seen: f64,
    /// When the board of battles won was last fetched.
    board_at: Option<f64>,
    /// The level picked to play alone, and each level's board on floor.
    level: Level,
    floors: [crate::floor::GameFloor; 3],
}

impl Default for Riff {
    fn default() -> Self {
        let floors = Level::ALL.map(|l| crate::floor::GameFloor::new(l.floor_game(), false));
        Self { mode: Mode::default(), song: None, audio: None, seen: 0.0, board_at: None, level: Level::default(), floors }
    }
}

impl Riff {
    /// A song is being played.
    fn playing(&self) -> bool {
        self.song.as_ref().is_some_and(|s| !s.finished)
    }

    /// The page is left for another: a song played alone stops, a battle's goes on (its sound comes
    /// back with the page).
    pub(super) fn leave(&mut self) {
        self.audio = None;
        if self.mode == Mode::Solo && self.playing() {
            self.song = None;
            self.mode = Mode::Lobby;
        }
    }

    /// A match of Puissance 4 takes the place of the battle (they share floor's match): to the lobby.
    pub(super) fn leave_battle(&mut self) {
        if self.mode == Mode::Battle {
            self.mode = Mode::Lobby;
            self.song = None;
            self.audio = None;
        }
    }
}

/// The strings' colors, from the left.
fn lane_color(theme: &Theme, lane: usize) -> Color32 {
    [theme.ansi[1], theme.ansi[4], theme.ansi[2], theme.ansi[3]][lane]
}

/// The arrows' keys, from the left.
const KEYS: [Key; LANES] = [Key::ArrowLeft, Key::ArrowDown, Key::ArrowUp, Key::ArrowRight];

impl App {
    /// Riff Hero, on the home page.
    pub(super) fn open_riff(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Riff;
    }

    /// Development: the lobby, or a song alone started ("solo").
    pub(super) fn riff_demo(&mut self, what: &str) {
        if what == "solo" {
            self.riff_solo_start();
        }
    }

    /// A battle starting (an invitation accepted, a rematch): to its page.
    pub(super) fn open_riff_battle(&mut self) {
        self.open_riff();
        if self.riff.mode == Mode::Solo {
            self.riff.song = None;
            self.riff.audio = None;
        }
        self.riff.mode = Mode::Battle;
        // Floor's match is the battle's now: Puissance 4 no longer waits for its own.
        self.four.leave_online();
    }

    /// At each frame: the page out of sight, the song's sound stops; a battle's song goes on, sent to
    /// floor all the same.
    pub(super) fn riff_frame(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let out_of_sight = now - self.riff.seen > 0.5;
        if self.riff.audio.is_some() && out_of_sight {
            self.riff.audio = None;
        }
        self.riff_battle_send(out_of_sight);
    }

    /// Where the player is in a battle, sent to floor every `SEND_EVERY`; at the end the final score,
    /// sent again until floor has it (lost on the way, floor's match still says it isn't). Out of sight
    /// (`out_of_sight`), the notes going by are missed here, the page not being there to.
    fn riff_battle_send(&mut self, out_of_sight: bool) {
        if self.riff.mode != Mode::Battle {
            return;
        }
        let Some((id, over, mine)) = self.versus.game.as_ref().filter(|g| g.riff()).map(|g| (g.id.clone(), g.over, g.riff[g.you as usize - 1])) else { return };
        let Some(song) = self.riff.song.as_mut().filter(|s| s.battle.as_ref() == Some(&id)) else { return };
        if over {
            // The other gave up: the song stops.
            if !song.finished {
                song.finished = true;
                self.riff.audio = None;
            }
            return;
        }
        let time = song.time();
        if !song.finished {
            if out_of_sight && time >= 0.0 {
                song.play.advance(time);
            }
            song.finished = song.play.over(time);
        }
        if song.finished {
            if !mine.done && time - song.sent_at >= SEND_EVERY {
                song.sent_at = time;
                let side = song.side(true);
                self.versus.riff(side);
            }
        } else if time >= 0.0 && time - song.sent_at >= SEND_EVERY && song.side(false) != mine {
            song.sent_at = time;
            let side = song.side(false);
            self.versus.riff(side);
        }
    }

    pub(super) fn riff_page(&mut self, ui: &mut Ui, rect: Rect) {
        let now = ui.input(|i| i.time);
        self.riff.seen = now;
        let playing = self.riff.playing();
        // A song under way is the half-day of work's; once played, the lobby gives way.
        if playing {
            self.count_round(HomeGame::Riff);
        } else if self.riff.mode == Mode::Lobby
            && let Some(p) = self.game_locked(HomeGame::Riff)
        {
            self.fold_for_game(false);
            return self.locked_page(ui, rect, HomeGame::Riff, &p);
        }
        // During a song the sidebar folds away, and unfolds after.
        self.fold_for_game(playing);
        let t = self.t();
        let theme = self.theme.clone();
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, t.riff_name, 40.0, theme.accent, 1.0);
        // Back: from a song to the lobby, from the lobby home (not during a battle: give up first).
        let in_battle = self.riff.mode == Mode::Battle && playing;
        if !in_battle {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.riff_to_lobby();
                return;
            }
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
        match self.riff.mode {
            Mode::Lobby => self.riff_lobby(ui, body, now),
            Mode::Solo => self.riff_solo(ui, body),
            Mode::Battle => self.riff_battle(ui, body),
        }
    }

    /// The back button: a song (or a battle) to the lobby, the lobby to the home page.
    fn riff_to_lobby(&mut self) {
        match self.riff.mode {
            Mode::Lobby => self.home_game = false,
            Mode::Solo => {}
            Mode::Battle => self.versus.unwatch(true),
        }
        self.riff.mode = Mode::Lobby;
        self.riff.song = None;
        self.riff.audio = None;
    }

    fn riff_lobby(&mut self, ui: &mut Ui, body: Rect, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        // The board of battles won, fetched again every half minute while the lobby shows.
        if pseudo.is_some() && self.riff.board_at.is_none_or(|at| now - at > 30.0) {
            self.riff.board_at = Some(now);
            self.versus.fetch_board("riff-hero");
        }
        let width = body.width().min(820.0);
        let wide = width >= 640.0;
        let area = Rect::from_min_size(Pos2::new(body.center().x - width / 2.0, body.min.y), Vec2::new(width, body.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
        let card_w = if wide { (width - 16.0) / 2.0 } else { width };
        let card = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
            Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(18.0).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(card_w - 38.0);
                    ui.set_max_width(card_w - 38.0);
                    add(ui);
                });
            });
        };
        // The board of the level picked, fetched again every minute while the lobby shows.
        let level = self.riff.level;
        for floor in &mut self.riff.floors {
            floor.poll();
        }
        self.riff.floors[level.index()].sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let best = self.config.settings.riff_bests.get(level.index()).copied().unwrap_or(0);
        let mut play = false;
        let mut picked = level;
        let mut ask_pseudo = false;
        let floor_board = self.riff.floors[level.index()].board();
        let mut solo = |ui: &mut Ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                paint_riff_icon(ui.painter(), r.center(), &theme, 0.9);
                ui.label(egui::RichText::new(t.riff_solo).size(16.0).strong());
            });
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.riff_solo_text).size(13.0).color(theme.text_muted));
            ui.add_space(12.0);
            // The levels: each its song, its board.
            ui.horizontal(|ui| {
                for l in Level::ALL {
                    let on = l == level;
                    let text = egui::RichText::new(t.riff_levels[l.index()]).size(13.0).color(if on { theme.bg } else { theme.text });
                    let button = egui::Button::new(text).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(6.0).min_size(Vec2::new(84.0, 28.0));
                    if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        picked = l;
                    }
                }
            });
            ui.add_space(14.0);
            let button = egui::Button::new(egui::RichText::new(t.four_play).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
            let (at, _) = ui.allocate_exact_size(Vec2::new(140.0, 34.0), Sense::hover());
            if ui.put(at, button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                play = true;
            }
            ui.add_space(14.0);
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 200.0), Sense::hover());
            let best = (best > 0).then(|| t.riff_best.replace("{s}", &best.to_string()));
            if self.game_board(ui, area, &theme, floor_board, best, |s| s.to_string()) {
                ask_pseudo = true;
            }
        };
        let live = &self.versus;
        let mut action = None;
        let mut battle = |ui: &mut Ui| {
            action = players_ui(ui, live, t.riff_battle, pseudo.as_deref(), t, &theme);
            ui.add_space(6.0);
            ui.label(egui::RichText::new(t.riff_battle_text).size(12.5).color(theme.text_muted));
            wins_ui(ui, live, "riff-hero", t.riff_wins, pseudo.as_deref(), t, &theme);
        };
        if wide {
            child.horizontal_top(|ui| {
                card(ui, &mut solo);
                ui.add_space(16.0);
                card(ui, &mut battle);
            });
        } else {
            card(&mut child, &mut solo);
            child.add_space(16.0);
            card(&mut child, &mut battle);
        }
        // How to play, under the cards.
        ui.painter().text(Pos2::new(body.center().x, body.max.y - 20.0), Align2::CENTER_CENTER, t.riff_help, FontId::proportional(12.5), theme.text_muted);
        self.riff.level = picked;
        if ask_pseudo {
            self.ask_pseudo();
        }
        if play {
            self.riff_solo_start();
        }
        match action {
            Some(LobbyAction::Invite(name)) => self.versus.invite_to(&name, "riff-hero"),
            Some(LobbyAction::Cancel) => self.versus.cancel(),
            Some(LobbyAction::Pseudo) => self.ask_pseudo(),
            _ => {}
        }
    }

    /// A song alone, after a countdown.
    fn riff_solo_start(&mut self) {
        // Played enough this half-day of work: to the lobby, which says why.
        if self.game_locked(HomeGame::Riff).is_some() {
            self.riff.mode = Mode::Lobby;
            self.riff.song = None;
            return;
        }
        self.riff.audio = None;
        self.riff.song = Some(Song::new(&self.ctx.clone(), Chart::solo(self.riff.level), Instant::now() + Duration::from_secs_f64(LEAD), None));
        self.riff.mode = Mode::Solo;
    }

    fn riff_solo(&mut self, ui: &mut Ui, body: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        if self.riff.song.is_none() {
            self.riff.mode = Mode::Lobby;
            return;
        }
        let side = Rect::from_min_max(Pos2::new(body.max.x - 220.0, body.min.y), body.max);
        self.riff_play_frame(ui, body);
        let level = self.riff.level.index();
        let Some(song) = &mut self.riff.song else { return };
        let time = song.time();
        if !song.finished && song.play.over(time) {
            // Played to the end: kept when it beats the best, and sent to the level's board.
            song.finished = true;
            song.record = song.play.score > self.config.settings.riff_bests.get(level).copied().unwrap_or(0);
            let p = &song.play;
            let round = crate::floor::GameRound { score: p.score, details: serde_json::json!({ "score": p.score, "hits": p.hits, "perfects": p.perfects, "misses": p.misses, "combo": p.best_combo }) };
            if song.record {
                let bests = &mut self.config.settings.riff_bests;
                bests.resize(bests.len().max(level + 1), 0);
                bests[level] = song.play.score;
                self.save_config();
            }
            if round.score > 0 && crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
                let now = ui.input(|i| i.time);
                self.riff.floors[level].finished(round);
                self.riff.floors[level].sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
            }
        }
        let Some(song) = &self.riff.song else { return };
        if !song.finished {
            let best = self.config.settings.riff_bests.get(level).copied().unwrap_or(0);
            let label = t.riff_levels[level];
            if side.width() > 0.0 {
                let text = if best > 0 { format!("{label} · {}", t.riff_best.replace("{s}", &best.to_string())) } else { label.to_owned() };
                ui.painter().text(Pos2::new(side.max.x, side.min.y + 6.0), Align2::RIGHT_TOP, text, FontId::proportional(13.0), theme.text_muted);
            }
            return;
        }
        let title = t.riff_over.replace("{s}", &song.play.score.to_string());
        let record = song.record.then_some(t.game_new_best);
        let stats = stats_text(t, &song.play);
        let locked = self.game_locked(HomeGame::Riff).map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let again = result_card(ui, body, &title, theme.accent, &stats, record, locked.as_deref(), t.riff_again, true, &theme);
        if again && locked.is_none() {
            self.riff_solo_start();
        }
    }

    fn riff_battle(&mut self, ui: &mut Ui, body: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(game) = self.versus.game.clone().filter(Match::riff) else {
            // Waiting for floor's first answer (an invitation just accepted).
            let (text, color) = match &self.versus.refused {
                Some(e) => (e.as_str(), theme.ansi[1]),
                None => (t.four_connecting, theme.text_muted),
            };
            ui.painter().text(body.center(), Align2::CENTER_CENTER, text, FontId::proportional(15.0), color);
            ui.ctx().request_repaint_after(Duration::from_millis(200));
            return;
        };
        // A new battle (or its rematch): its song, from floor's start.
        if self.riff.song.as_ref().and_then(|s| s.battle.as_ref()) != Some(&game.id) {
            let start = if game.starts_in >= 0 { game.at + Duration::from_millis(game.starts_in as u64) } else { game.at.checked_sub(Duration::from_millis(game.starts_in.unsigned_abs())).unwrap_or(game.at) };
            self.riff.audio = None;
            self.riff.song = Some(Song::new(&self.ctx.clone(), Chart::new(game.seed), start, Some(game.id.clone())));
        }
        let them = game.them().unwrap_or(t.game_anonymous).to_owned();
        let theirs = game.riff[2 - game.you as usize];

        // The crowd between the two, above the strings.
        let crowd = Rect::from_min_max(Pos2::new(body.center().x - 260.0, body.min.y), Pos2::new(body.center().x + 260.0, body.min.y + 34.0));
        let score = self.riff.song.as_ref().map_or(0, |s| s.play.score);
        paint_crowd(ui, crowd, score, theirs.score, &them, &theme, t);
        let strings = Rect::from_min_max(Pos2::new(body.min.x, body.min.y + 44.0), body.max);
        self.riff_play_frame(ui, strings);

        // The other's side: their score and combo, and the button to give up.
        let side = Rect::from_min_max(Pos2::new(body.max.x - 200.0, strings.min.y), body.max);
        if side.min.x > body.center().x + 2.0 * 96.0 + 20.0 {
            let painter = ui.painter();
            painter.text(Pos2::new(side.max.x, side.min.y + 8.0), Align2::RIGHT_TOP, &them, FontId::proportional(15.0), theme.ansi[4]);
            painter.text(Pos2::new(side.max.x, side.min.y + 32.0), Align2::RIGHT_TOP, theirs.score.to_string(), FontId::monospace(26.0), theme.text);
            if theirs.combo >= 5 {
                painter.text(Pos2::new(side.max.x, side.min.y + 66.0), Align2::RIGHT_TOP, format!("{} {}", t.riff_combo, theirs.combo), FontId::proportional(13.0), theme.text_muted);
            }
        }
        let playing = self.riff.playing();
        if playing && !game.over {
            let at = Rect::from_min_size(Pos2::new(side.max.x - 130.0, side.max.y - 40.0), Vec2::new(130.0, 32.0));
            if ui.put(at, egui::Button::new(egui::RichText::new(t.four_leave).size(13.0)).corner_radius(8.0)).clicked() {
                self.versus.leave();
            }
        }

        // Where the player is goes to floor at each frame, the page shown or not (see `riff_battle_send`).
        let Some(song) = &self.riff.song else { return };
        if !song.finished {
            return;
        }
        let stats = stats_text(t, &song.play);
        if !game.over {
            let waiting = t.riff_waiting.replace("{n}", &them);
            result_card(ui, body, &t.riff_over.replace("{s}", &song.play.score.to_string()), theme.text, &stats, None, Some(&waiting), "", false, &theme);
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            return;
        }
        let forfeit = game.reason.as_deref() == Some("forfeit");
        let (title, color) = match game.winner {
            Some(3) => (t.riff_draw.to_owned(), theme.text),
            Some(w) if w == game.you && forfeit => (t.four_forfeit_them.replace("{n}", &them), theme.ansi[2]),
            Some(w) if w == game.you => (t.riff_won.to_owned(), theme.ansi[2]),
            Some(_) if forfeit => (t.four_forfeit_you.to_owned(), theme.text_muted),
            _ => (t.riff_lost.replace("{n}", &them), theme.ansi[1]),
        };
        let scores = format!("{}  —  {} {}", song.play.score, theirs.score, them);
        let note = if game.rematch.them && !game.rematch.you { Some(t.four_rematch_them.replace("{n}", &them)) } else { Some(scores) };
        let label = if game.rematch.you { t.four_rematch_asked } else { t.four_rematch };
        let locked = self.game_locked(HomeGame::Riff).map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let below = locked.clone().or(note);
        if result_card(ui, body, &title, color, &stats, None, below.as_deref(), label, !game.rematch.you && locked.is_none(), &theme) && !game.rematch.you && locked.is_none() {
            self.versus.rematch();
        }
    }

    /// A frame of the song under way: the keys and the clicks on the arrows played, the notes gone by
    /// missed, its sound started, and the strings drawn in `area`.
    fn riff_play_frame(&mut self, ui: &mut Ui, area: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let sound = self.config.settings.game_sound != Some(false);
        let riff = &mut self.riff;
        let Some(song) = &mut riff.song else { return };
        let time = song.time();
        if let Some(rx) = &song.rendering
            && let Ok(samples) = rx.try_recv()
        {
            song.samples = Some(Arc::new(samples));
            song.rendering = None;
        }
        let live = !song.finished && time < LENGTH;
        // The sound, once the song started (from where it is, coming back to the page).
        if live && sound && !song.mute && time >= -0.02 && riff.audio.is_none()
            && let Some(samples) = &song.samples
        {
            riff.audio = Audio::start(samples, time);
            song.mute = riff.audio.is_none();
        }
        if !live {
            riff.audio = None;
        }

        let layout = Strings4::new(area);
        // The arrows: keys (when nothing else has the keyboard) and clicks on those of the line.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let mut presses: Vec<usize> = if free { ui.input(|i| (0..LANES).filter(|&l| i.key_pressed(KEYS[l])).collect()) } else { Vec::new() };
        // A click counts when the button goes down, not up: in time with the note.
        let down = ui.input(|i| i.pointer.primary_pressed());
        for lane in 0..LANES {
            let target = layout.target(lane);
            let resp = ui.interact(target.expand(6.0), ui.id().with(("riff-arrow", lane)), Sense::click());
            if down && resp.hovered() && !presses.contains(&lane) {
                presses.push(lane);
            }
            if live {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        }
        let mut thuds = 0;
        if live && time >= -crate::riff::GOOD {
            for lane in presses {
                if song.press(lane, time) == Press::Stray {
                    thuds += 1;
                }
            }
            for lane in song.play.advance(time) {
                song.pops.push((time, lane, Judgment::Miss));
                thuds += 1;
            }
        }
        if thuds > 0
            && let Some(audio) = &riff.audio
        {
            audio.thud();
        }
        song.pops.retain(|(at, ..)| time - at < POP);
        song.bursts.retain(|(at, _)| time - at < 0.3);

        paint_strings(ui, &layout, song, time, &theme, t);
        ui.ctx().request_repaint();
    }
}

/// "{h} notes of {n} · {p} perfect · best combo {c}".
fn stats_text(t: &Strings, play: &Play) -> String {
    t.riff_result.replace("{h}", &play.hits.to_string()).replace("{n}", &play.chart.len().to_string()).replace("{p}", &play.perfects.to_string()).replace("{c}", &play.best_combo.to_string())
}

/// The end of a song, in a card over the strings: `title`, the stats, a line under them (`note`), and
/// a button (`button`, filled when `strong`; none when empty). Its button clicked.
#[allow(clippy::too_many_arguments)]
fn result_card(ui: &mut Ui, body: Rect, title: &str, color: Color32, stats: &str, record: Option<&str>, note: Option<&str>, button: &str, strong: bool, theme: &Theme) -> bool {
    let card = Rect::from_center_size(Pos2::new(body.center().x, body.center().y), Vec2::new(body.width().min(480.0), 230.0));
    let painter = ui.painter();
    painter.rect_filled(card, 14.0, theme.chrome_bg.gamma_multiply(0.97));
    painter.rect_stroke(card, 14.0, Stroke::new(1.5, color.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    let pop = ui.ctx().animate_bool_with_time(ui.id().with(("riff-over", title)), true, 0.45);
    paint_metal(painter, Pos2::new(card.center().x, card.min.y + 44.0), Align2::CENTER_CENTER, title, 18.0 + 10.0 * super::motion::out_back(pop), color, pop.min(1.0));
    painter.text(Pos2::new(card.center().x, card.min.y + 88.0), Align2::CENTER_CENTER, stats, FontId::proportional(13.0), theme.text_muted);
    let mut y = card.min.y + 114.0;
    if let Some(record) = record {
        painter.text(Pos2::new(card.center().x, y), Align2::CENTER_CENTER, record, FontId::proportional(14.0), theme.accent);
        y += 24.0;
    }
    if let Some(note) = note {
        painter.text(Pos2::new(card.center().x, y), Align2::CENTER_CENTER, note, FontId::proportional(13.5), theme.text);
    }
    if button.is_empty() {
        return false;
    }
    let at = Rect::from_center_size(Pos2::new(card.center().x, card.max.y - 36.0), Vec2::new(170.0, 36.0));
    let b = if strong { egui::Button::new(egui::RichText::new(button).size(14.0).strong().color(theme.bg)).fill(theme.accent) } else { egui::Button::new(egui::RichText::new(button).size(14.0)) };
    ui.put(at, b.corner_radius(8.0)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Where the four strings go in an area: the neck in its middle, the line near its bottom.
struct Strings4 {
    neck: Rect,
    lane_w: f32,
    line_y: f32,
}

impl Strings4 {
    fn new(area: Rect) -> Self {
        let lane_w = (area.width() / 4.0).clamp(56.0, 96.0);
        let width = lane_w * LANES as f32;
        let neck = Rect::from_min_max(Pos2::new(area.center().x - width / 2.0, area.min.y), Pos2::new(area.center().x + width / 2.0, area.max.y - 30.0));
        Self { neck, lane_w, line_y: neck.max.y - lane_w * 0.75 }
    }

    fn x(&self, lane: usize) -> f32 {
        self.neck.min.x + (lane as f32 + 0.5) * self.lane_w
    }

    /// The arrow of string `lane` on the line.
    fn target(&self, lane: usize) -> Rect {
        Rect::from_center_size(Pos2::new(self.x(lane), self.line_y), Vec2::splat(self.lane_w * 0.78))
    }

    /// Where a note `ahead` seconds from the line is.
    fn y(&self, ahead: f64) -> f32 {
        self.line_y - (ahead / AHEAD) as f32 * (self.line_y - self.neck.min.y)
    }
}

/// The neck: its strings (shaking when hit), the beats going down, the notes coming, the arrows of the
/// line; the score and the combo on its left; the countdown over it.
fn paint_strings(ui: &Ui, layout: &Strings4, song: &Song, time: f64, theme: &Theme, t: &Strings) {
    let painter = ui.painter();
    let neck = layout.neck;
    let back = super::sidebar::lerp_color(theme.bg, Color32::BLACK, 0.3);
    painter.rect_filled(neck, 10.0, back);
    painter.rect_stroke(neck, 10.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);

    // The beats, as frets going down.
    let beat = 60.0 / crate::riff::BPM;
    let first = ((time - 0.3) / beat).ceil() as i64;
    for k in first..first + (AHEAD / beat) as i64 + 2 {
        let at = k as f64 * beat;
        if !(0.0..LENGTH).contains(&at) {
            continue;
        }
        let y = layout.y(at - time);
        if y < neck.min.y || y > neck.max.y {
            continue;
        }
        let bar = k % 4 == 0;
        painter.line_segment([Pos2::new(neck.min.x + 4.0, y), Pos2::new(neck.max.x - 4.0, y)], Stroke::new(if bar { 2.0 } else { 1.0 }, theme.tab_hover.gamma_multiply(if bar { 0.9 } else { 0.45 })));
    }

    // The strings: each shakes a moment when played.
    for lane in 0..LANES {
        let x = layout.x(lane);
        let since = (time - song.pressed[lane]) as f32;
        let shake = if (0.0..0.25).contains(&since) { (1.0 - since / 0.25) * 3.0 } else { 0.0 };
        let steps = 24;
        let points: Vec<Pos2> = (0..=steps)
            .map(|k| {
                let p = k as f32 / steps as f32;
                let y = neck.min.y + p * (neck.max.y - neck.min.y);
                Pos2::new(x + shake * (p * std::f32::consts::PI * 3.0 + since * 90.0).sin() * (p * std::f32::consts::PI).sin(), y)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, lane_color(theme, lane).gamma_multiply(0.35))));
    }

    // The line, and its arrows: lit when pressed, flashing when a note is hit.
    painter.line_segment([Pos2::new(neck.min.x, layout.line_y), Pos2::new(neck.max.x, layout.line_y)], Stroke::new(2.0, theme.text.gamma_multiply(0.25)));
    let size = layout.lane_w * 0.62;
    for lane in 0..LANES {
        let target = layout.target(lane);
        let color = lane_color(theme, lane);
        let lit = ((0.12 - (time - song.pressed[lane])) / 0.12).clamp(0.0, 1.0) as f32;
        painter.rect_filled(target, 10.0, color.gamma_multiply(0.08 + 0.25 * lit));
        paint_arrow(painter, target.center(), size, lane, None, Stroke::new(2.0, color.gamma_multiply(0.55 + 0.45 * lit)));
    }
    for &(at, lane) in &song.bursts {
        let p = ((time - at) / 0.3) as f32;
        let color = lane_color(theme, lane);
        let c = layout.target(lane).center();
        painter.circle_stroke(c, size * (0.55 + 0.6 * super::motion::out_cubic(p)), Stroke::new(4.0 * (1.0 - p), color.gamma_multiply(1.0 - p)));
        painter.circle_filled(c, size * 0.5 * (1.0 - p), Color32::WHITE.gamma_multiply(0.35 * (1.0 - p)));
    }

    // The notes coming down: not played yet, or missed (going on past the line, dimmed).
    let clip = painter.with_clip_rect(neck);
    for (k, note) in song.play.chart.notes.iter().enumerate() {
        let ahead = note.t - time;
        if ahead > AHEAD {
            break;
        }
        if ahead < -0.4 {
            continue;
        }
        let missed = match song.play.judged[k] {
            None => false,
            Some(Judgment::Miss) => true,
            Some(_) => continue,
        };
        let at = Pos2::new(layout.x(note.lane), layout.y(ahead));
        let color = lane_color(theme, note.lane);
        if missed {
            paint_arrow(&clip, at, size * 0.9, note.lane, Some(color.gamma_multiply(0.25)), Stroke::NONE);
        } else {
            clip.circle_filled(at, size * 0.62, color.gamma_multiply(0.18));
            paint_arrow(&clip, at, size, note.lane, Some(color), Stroke::new(1.5, Color32::WHITE.gamma_multiply(0.6)));
        }
    }

    // What the presses gave, rising above their arrow.
    for &(at, lane, judgment) in &song.pops {
        let p = ((time - at) / POP) as f32;
        let (text, color) = match judgment {
            Judgment::Perfect => (t.riff_perfect, theme.accent),
            Judgment::Good => (t.riff_good, theme.text),
            Judgment::Miss => (t.riff_miss, theme.ansi[1]),
        };
        let y = layout.line_y - layout.lane_w * (0.7 + 0.5 * super::motion::out_cubic(p));
        painter.text(Pos2::new(layout.x(lane), y), Align2::CENTER_CENTER, text, FontId::proportional(13.0 + 3.0 * (1.0 - p)), color.gamma_multiply(1.0 - p * p));
    }

    // The song's progress, along the neck's top.
    let progress = (time / LENGTH).clamp(0.0, 1.0) as f32;
    let bar = Rect::from_min_size(neck.min + Vec2::new(10.0, 6.0), Vec2::new((neck.width() - 20.0) * progress, 3.0));
    painter.rect_filled(bar, 1.5, theme.accent.gamma_multiply(0.7));

    // On the left: the score, the multiplier, the combo.
    let left = neck.min.x - 24.0;
    if left > ui.max_rect().min.x + 120.0 {
        let play = &song.play;
        painter.text(Pos2::new(left, neck.min.y + 8.0), Align2::RIGHT_TOP, t.riff_score, FontId::proportional(13.0), theme.text_muted);
        painter.text(Pos2::new(left, neck.min.y + 28.0), Align2::RIGHT_TOP, play.score.to_string(), FontId::monospace(28.0), theme.text);
        let mult = play.multiplier();
        if mult > 1 {
            paint_metal(painter, Pos2::new(left, neck.min.y + 92.0), Align2::RIGHT_CENTER, &format!("×{mult}"), 26.0, theme.accent, 1.0);
        }
        if play.combo >= 5 {
            painter.text(Pos2::new(left, neck.min.y + 124.0), Align2::RIGHT_TOP, format!("{} {}", t.riff_combo, play.combo), FontId::proportional(14.0), theme.text);
        }
    }

    // The countdown, then "Riff!".
    if time < 0.6 {
        let (text, p) = if time < 0.0 {
            let left = (-time).ceil();
            (if left > 3.0 { t.riff_ready.to_owned() } else { format!("{left}") }, (1.0 - (-time).fract()) as f32)
        } else {
            (t.riff_go.to_owned(), (time / 0.6) as f32)
        };
        let alpha = if time < 0.0 { 1.0 } else { 1.0 - p };
        paint_metal(painter, Pos2::new(neck.center().x, neck.center().y - 40.0), Align2::CENTER_CENTER, &text, 40.0 + 16.0 * super::motion::out_back(p.min(1.0)), theme.accent, alpha);
    }
    // Under the neck: how to play, while counting down.
    if time < 2.0 {
        painter.text(Pos2::new(neck.center().x, neck.max.y + 16.0), Align2::CENTER_CENTER, t.riff_help, FontId::proportional(12.0), theme.text_muted);
    }
}

/// The crowd between the two guitarists: heads jumping, on the player's side (the accent) or the
/// other's, shared by their scores.
fn paint_crowd(ui: &Ui, area: Rect, mine: u32, theirs: u32, them: &str, theme: &Theme, t: &Strings) {
    let painter = ui.painter();
    let now = ui.input(|i| i.time);
    // Even at the start; the share moves as the scores part.
    let share = (mine as f32 + 400.0) / (mine as f32 + theirs as f32 + 800.0);
    let share = ui.ctx().animate_value_with_time(ui.id().with("riff-crowd"), share, 0.6);
    let heads = 28;
    let them_color = theme.ansi[4];
    painter.text(Pos2::new(area.center().x, area.min.y - 2.0), Align2::CENTER_BOTTOM, t.riff_crowd, FontId::proportional(11.5), theme.text_muted);
    painter.text(Pos2::new(area.min.x - 10.0, area.center().y), Align2::RIGHT_CENTER, t.game_you, FontId::proportional(13.0), theme.accent);
    painter.text(Pos2::new(area.max.x + 10.0, area.center().y), Align2::LEFT_CENTER, them, FontId::proportional(13.0), them_color);
    for k in 0..heads {
        let p = (k as f32 + 0.5) / heads as f32;
        let ours = p < share;
        let color = if ours { theme.accent } else { them_color };
        // Each head jumps to its own beat.
        let jump = ((now * 6.0 + k as f64 * 1.7).sin().max(0.0) * 6.0) as f32;
        let x = area.min.x + p * area.width();
        let base = area.max.y - 6.0;
        painter.circle_filled(Pos2::new(x, base - 12.0 - jump), 5.0, color);
        painter.line_segment([Pos2::new(x, base - 7.0 - jump), Pos2::new(x, base - jump)], Stroke::new(3.0, color.gamma_multiply(0.8)));
        // Some throw the horns.
        if k % 3 == 0 {
            painter.line_segment([Pos2::new(x, base - 6.0 - jump), Pos2::new(x + if ours { -5.0 } else { 5.0 }, base - 18.0 - jump)], Stroke::new(1.5, color));
        }
    }
    let split = area.min.x + share * area.width();
    painter.line_segment([Pos2::new(split, area.min.y + 2.0), Pos2::new(split, area.max.y)], Stroke::new(1.5, theme.text.gamma_multiply(0.5)));
}

/// An arrow pointing the way of string `lane` (←, ↓, ↑, →), filled and/or outlined.
fn paint_arrow(painter: &egui::Painter, c: Pos2, size: f32, lane: usize, fill: Option<Color32>, stroke: Stroke) {
    // Pointing up, then turned.
    let angle = [-std::f32::consts::FRAC_PI_2, std::f32::consts::PI, 0.0, std::f32::consts::FRAC_PI_2][lane];
    let (sin, cos) = angle.sin_cos();
    let turn = |x: f32, y: f32| c + Vec2::new(x * cos - y * sin, x * sin + y * cos) * size;
    let head = vec![turn(0.0, -0.5), turn(0.48, 0.02), turn(0.17, 0.02), turn(-0.17, 0.02), turn(-0.48, 0.02)];
    let shaft = vec![turn(-0.17, 0.0), turn(0.17, 0.0), turn(0.17, 0.45), turn(-0.17, 0.45)];
    if let Some(fill) = fill {
        painter.add(egui::Shape::convex_polygon(vec![head[0], head[1], head[4]], fill, Stroke::NONE));
        painter.add(egui::Shape::convex_polygon(shaft, fill, Stroke::NONE));
    }
    if stroke.width > 0.0 {
        let outline = vec![head[0], head[1], head[2], turn(0.17, 0.45), turn(-0.17, 0.45), head[3], head[4]];
        painter.add(egui::Shape::closed_line(outline, stroke));
    }
}

/// A guitar pick with an arrow on it: the game's icon.
pub(super) fn paint_riff_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = 7.5 * scale;
    // The pick: rounded at the top, pointed at the bottom.
    let mut pick = Vec::new();
    for k in 0..=12 {
        let a = std::f32::consts::PI * (1.0 + k as f32 / 12.0);
        pick.push(c + Vec2::new(a.cos() * s, a.sin() * s * 0.8 - s * 0.1));
    }
    pick.push(c + Vec2::new(0.0, s * 1.15));
    painter.add(egui::Shape::convex_polygon(pick, theme.accent, Stroke::NONE));
    paint_arrow(painter, c + Vec2::new(0.0, s * 0.05), s * 1.05, 2, Some(theme.bg), Stroke::NONE);
}
