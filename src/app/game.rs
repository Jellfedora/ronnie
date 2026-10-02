//! "Speed Metal", the home page's typing game: a minute to type as many words as possible. Each
//! letter counts; words typed without a mistake build a combo, every 5 of them adding seconds to the
//! round; a finished word bursts into sparks.

use super::motion::{in_out_cubic, out_back, out_cubic, phase};
use super::*;
use crate::config::{TypingScore, TYPING_SCORES};

/// Length of a round, in seconds.
const ROUND: f64 = 60.0;
/// Seconds added to the round by each 5 words of a combo.
const COMBO_BONUS: f64 = 2.0;
/// The 3, 2, 1 before it starts.
const COUNTDOWN: f64 = 3.0;
/// The last seconds, when the clock turns red and beats.
const HURRY: f64 = 10.0;
/// The soundtrack ("Thrash Metal", by Alex Morgan on Pixabay), inside the app.
const MUSIC: &[u8] = include_bytes!("../../assets/audio/alex-morgan-thrash-metal-591343.mp3");
const MUSIC_VOLUME: f32 = 0.7;
/// The music fades out this long after the round.
const MUSIC_FADE: f64 = 2.5;
pub(super) const MUSIC_CREDIT_URL: &str = "https://pixabay.com/fr/users/alex-morgan-54692529/";

const WORDS_FR: &[&str] = &[
    "guitare", "ampli", "riff", "solo", "batterie", "basse", "headbang", "distorsion", "pédale", "médiator",
    "scène", "concert", "rappel", "tournée", "public", "micro", "larsen", "baguette", "cymbale", "grosse",
    "caisse", "tonnerre", "éclair", "flamme", "enfer", "démon", "dragon", "guerrier", "épée", "bouclier",
    "tempête", "volcan", "acier", "chrome", "métal", "rouille", "crâne", "corbeau", "loup", "minuit",
    "terminal", "commande", "serveur", "commit", "branche", "fusion", "compile", "script", "curseur", "clavier",
    "processus", "mémoire", "fichier", "dossier", "réseau", "tunnel", "clé", "session", "onglet", "fenêtre",
    "requête", "table", "index", "jointure", "export", "import", "sauvegarde", "journal", "noyau", "shell",
    "rugir", "hurler", "frapper", "vibrer", "exploser", "brûler", "foncer", "cogner", "déchirer", "gronder",
    "légende", "chaos", "furie", "rage", "gloire", "victoire", "bataille", "royaume", "trône", "couronne",
    "vitesse", "puissance", "fureur", "tonitruant", "électrique", "sauvage", "éternel", "infernal", "brutal", "lourd",
];

const WORDS_EN: &[&str] = &[
    "guitar", "amp", "riff", "solo", "drums", "bass", "headbang", "distortion", "pedal", "pick",
    "stage", "concert", "encore", "tour", "crowd", "mic", "feedback", "sticks", "cymbal", "snare",
    "thunder", "lightning", "flame", "inferno", "demon", "dragon", "warrior", "sword", "shield", "storm",
    "volcano", "steel", "chrome", "metal", "rust", "skull", "raven", "wolf", "midnight", "legend",
    "terminal", "command", "server", "commit", "branch", "merge", "compile", "script", "cursor", "keyboard",
    "process", "memory", "file", "folder", "network", "tunnel", "key", "session", "tab", "window",
    "query", "table", "index", "join", "export", "import", "backup", "log", "kernel", "shell",
    "roar", "scream", "strike", "shake", "explode", "burn", "rush", "pound", "shred", "rumble",
    "chaos", "fury", "rage", "glory", "victory", "battle", "kingdom", "throne", "crown", "speed",
    "power", "wild", "eternal", "infernal", "brutal", "heavy", "electric", "loud", "fast", "fierce",
];

#[derive(Clone, Copy, PartialEq, Default)]
enum State {
    #[default]
    Idle,
    /// The first time: sound or not? (then the round starts)
    AskSound,
    /// Since when (the input clock).
    Countdown(f64),
    Playing(f64),
    Over(f64),
}

/// A spark: flies off, falls, fades.
struct Spark {
    pos: Pos2,
    vel: Vec2,
    born: f64,
    life: f32,
    color: Color32,
    size: f32,
}

/// A text that rises and fades: "+7", "COMBO x10", a word finished.
struct Float {
    text: String,
    pos: Pos2,
    born: f64,
    size: f32,
    color: Color32,
}

/// The music playing: the sound device (closed when dropped) and the track.
struct Music {
    _device: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl Music {
    /// From the start, in a loop; None without a sound device.
    fn start() -> Option<Self> {
        let mut device = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| crate::log::error(&format!("game music: {e}"))).ok()?;
        device.log_on_drop(false);
        let player = rodio::Player::connect_new(device.mixer());
        let track = rodio::Decoder::new_looped(std::io::Cursor::new(MUSIC)).map_err(|e| crate::log::error(&format!("game music: {e}"))).ok()?;
        player.append(track);
        player.set_volume(MUSIC_VOLUME);
        Some(Self { _device: device, player })
    }
}

/// What a frame of the game gives the app to keep.
#[derive(Default)]
pub(super) struct GameOut {
    /// A round just finished, for the board.
    pub finished: Option<TypingScore>,
    /// The sound turned on or off.
    pub sound: Option<bool>,
    /// The name put on the shared card, changed.
    pub player: Option<String>,
}

#[derive(Default)]
pub(super) struct Game {
    state: State,
    words: Vec<&'static str>,
    /// Letters of the current word typed right.
    typed: usize,
    letters: u32,
    done: u32,
    mistakes: u32,
    combo: u32,
    best_combo: u32,
    /// Seconds won by the combos, added to the round.
    bonus: f64,
    /// When the last wrong key was typed (the word shakes).
    shake: f64,
    sparks: Vec<Spark>,
    floats: Vec<Float>,
    rng: u64,
    /// This round beat the best score.
    record: bool,
    /// Its place on the board (from 0), when it made it.
    rank: Option<usize>,
    music: Option<Music>,
    /// When the home page (the game) appeared: its entrance plays from there.
    appeared: Option<f64>,
    /// The card of the round, shown to be shared.
    share: Option<super::scorecard::ScoreCard>,
    /// Kept: on Linux, what was copied goes with it.
    clipboard: Option<arboard::Clipboard>,
    /// The name typed on a card, to save.
    renamed: Option<String>,
}

/// The letter without its accent: "é" typed as "e" counts.
fn fold(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'à' | 'â' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' => 'i',
        'ô' | 'ö' => 'o',
        'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        c => c,
    }
}

impl Game {
    fn random(&mut self) -> u64 {
        if self.rng == 0 {
            self.rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9e37_79b9) | 1;
        }
        // xorshift64.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn unit(&mut self) -> f32 {
        (self.random() % 10_000) as f32 / 10_000.0
    }

    /// A word not among the last ones shown.
    fn push_word(&mut self, list: &'static [&'static str]) {
        loop {
            let word = list[(self.random() % list.len() as u64) as usize];
            if !self.words.iter().rev().take(8).any(|w| *w == word) {
                self.words.push(word);
                return;
            }
        }
    }

    /// A new round (the music from the start, when on).
    fn start(&mut self, now: f64, list: &'static [&'static str], sound: bool) {
        let (rng, appeared) = (self.rng, self.appeared);
        *self = Game { rng, appeared, ..Game::default() };
        for _ in 0..6 {
            self.push_word(list);
        }
        self.state = State::Countdown(now);
        if sound {
            self.music = Music::start();
        }
    }

    /// Play asked: the first time, whether to have sound first.
    fn play(&mut self, now: f64, list: &'static [&'static str], sound: Option<bool>) {
        match sound {
            None => self.state = State::AskSound,
            Some(on) => self.start(now, list, on),
        }
    }

    /// The round's length, with the seconds won.
    fn length(&self) -> f64 {
        ROUND + self.bonus
    }

    /// The round's length as shown: "1 min", "1 min 08 s".
    pub(super) fn length_text(&self) -> String {
        round_time(self.length())
    }

    /// A round is on (its countdown included).
    pub fn playing(&self) -> bool {
        matches!(self.state, State::Countdown(_) | State::Playing(_))
    }

    /// From 0 to 1 as a round starts (the board and the tips slide away, the game takes the room),
    /// back to 0 after.
    pub fn focus(&self, ctx: &egui::Context) -> f32 {
        ctx.animate_bool_with_time_and_easing(egui::Id::new("game-focus"), self.playing(), 0.5, in_out_cubic)
    }

    /// How far the home page's entrance is at `now`, for a part starting `delay` seconds in.
    pub fn entrance(&self, now: f64, delay: f64) -> f32 {
        self.appeared.map_or(0.0, |at| phase(now, at + delay, 0.6, out_cubic))
    }

    /// The home page is left: no round going on unseen, no music.
    pub fn leave(&mut self) {
        if matches!(self.state, State::Countdown(_) | State::Playing(_) | State::AskSound) {
            self.state = State::Idle;
        }
        self.music = None;
        // The entrance plays again next time.
        self.appeared = None;
    }

    fn burst(&mut self, at: Pos2, now: f64, colors: [Color32; 3], count: usize) {
        for _ in 0..count {
            let angle = self.unit() * std::f32::consts::TAU;
            let speed = 120.0 + self.unit() * 320.0;
            let color = colors[(self.random() % 3) as usize];
            let (life, size) = (0.5 + self.unit() * 0.6, 1.5 + self.unit() * 2.5);
            let spread = self.unit() * 40.0 - 20.0;
            self.sparks.push(Spark { pos: at + Vec2::new(spread, 0.0), vel: Vec2::angled(angle) * speed - Vec2::new(0.0, 120.0), born: now, life, color, size });
        }
    }

    /// A key typed while playing.
    fn key(&mut self, c: char, now: f64, at: Pos2, theme: &Theme, list: &'static [&'static str]) {
        let Some(word) = self.words.first().copied() else { return };
        let Some(expected) = word.chars().nth(self.typed) else { return };
        if fold(c) != fold(expected) {
            self.mistakes += 1;
            self.combo = 0;
            self.shake = now;
            return;
        }
        self.typed += 1;
        self.letters += 1;
        if self.typed < word.chars().count() {
            return;
        }
        // The word is done: it flies away in sparks, the next one comes.
        self.done += 1;
        self.combo += 1;
        self.best_combo = self.best_combo.max(self.combo);
        self.typed = 0;
        self.words.remove(0);
        self.push_word(list);
        let fire = [theme.accent, theme.ansi[3], theme.ansi[1]];
        self.burst(at, now, fire, 18 + (self.combo as usize).min(20) * 2);
        self.floats.push(Float { text: word.to_owned(), pos: at, born: now, size: 52.0, color: theme.accent });
        let n = word.chars().count();
        self.floats.push(Float { text: format!("+{n}"), pos: at + Vec2::new(0.0, -46.0), born: now, size: 22.0, color: theme.ansi[2] });
        if self.combo.is_multiple_of(5) {
            self.bonus += COMBO_BONUS;
            self.floats.push(Float { text: format!("COMBO ×{}  +{COMBO_BONUS} s", self.combo), pos: at + Vec2::new(0.0, -90.0), born: now, size: 30.0, color: theme.ansi[3] });
            self.burst(at, now, [theme.ansi[3], Color32::WHITE, theme.accent], 40);
        }
    }

    /// Draws the game in `rect`, with the board of `scores` (the best first); a round just finished,
    /// to add to it.
    /// `sound`: the music on or off (None: not asked yet).
    #[allow(clippy::too_many_arguments)]
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings, scores: &[TypingScore], french: bool, sound: Option<bool>, player: &str) -> GameOut {
        let mut out = GameOut::default();
        let best = scores.first().map_or(0, |s| s.letters);
        let now = ui.input(|i| i.time);
        // The entrance: each part comes in at its time.
        let appeared = *self.appeared.get_or_insert(now);
        let intro = |delay: f64, length: f64, ease: fn(f32) -> f32| phase(now, appeared + delay, length, ease);
        if now - appeared < 1.4 {
            ui.ctx().request_repaint();
        }
        // Wide enough: the board in a column on the right, else under Play. As a round starts it slides
        // away and the game spreads over its room; it comes back after.
        let focus = self.focus(ui.ctx());
        let wide = rect.width() >= 980.0;
        let full = rect;
        let rect = Rect::from_min_max(rect.min, Pos2::new(rect.max.x - if wide { 390.0 * (1.0 - focus) } else { 0.0 }, rect.max.y));
        if wide && focus < 0.995 {
            let arrive = intro(0.3, 0.7, out_cubic);
            let slide = focus * 440.0 + (1.0 - arrive) * 180.0;
            let rows = scores.len().max(1) as f32;
            let top = full.min.y + 60.0;
            let board = Rect::from_min_max(Pos2::new(full.max.x - 370.0 + slide, top), Pos2::new(full.max.x - 30.0 + slide, (top + 104.0 + rows * 28.0).min(full.max.y - 10.0)));
            let highlight = if matches!(self.state, State::Over(_)) { self.rank } else { None };
            paint_board(&ui.painter_at(full), board, scores, highlight, theme, t, (1.0 - focus) * arrive, now);
        }
        let list = if french { WORDS_FR } else { WORDS_EN };
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let word_at = Pos2::new(center.x, center.y + 10.0);

        // What is typed: only when nothing else (a field of the sidebar...) has the keyboard.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let (chars, enter, escape) = if free {
            ui.input(|i| {
                let chars: Vec<char> = i.events.iter().filter_map(|e| if let egui::Event::Text(s) = e { Some(s.chars().collect::<Vec<_>>()) } else { None }).flatten().collect();
                (chars, i.key_pressed(Key::Enter), i.key_pressed(Key::Escape))
            })
        } else {
            (Vec::new(), false, false)
        };

        // Clock.
        match self.state {
            State::Countdown(since) if now - since >= COUNTDOWN => {
                self.state = State::Playing(since + COUNTDOWN);
                // GO: a burst of sparks from the first word.
                self.burst(word_at, now, [theme.accent, theme.ansi[3], Color32::WHITE], 110);
            }
            State::Playing(since) if now - since >= self.length() => {
                self.state = State::Over(now);
                let score = self.score();
                // Below the rounds it ties with.
                let place = scores.iter().filter(|s| s.letters >= score.letters).count();
                self.rank = (score.letters > 0 && place < TYPING_SCORES).then_some(place);
                if score.letters > 0 {
                    out.finished = Some(score);
                }
                if self.letters > best {
                    self.record = true;
                    for k in 0..3 {
                        let at = Pos2::new(rect.min.x + rect.width() * (0.25 + 0.25 * k as f32), rect.min.y + rect.height() * 0.3);
                        self.burst(at, now, [theme.ansi[3], theme.accent, Color32::WHITE], 60);
                    }
                }
            }
            _ => {}
        }
        match self.state {
            State::Playing(_) | State::Countdown(_) if escape => {
                self.state = State::Idle;
                self.music = None;
            }
            State::AskSound if enter || escape => {
                out.sound = Some(enter);
                self.start(now, list, enter);
            }
            State::Playing(_) => {
                for c in chars {
                    if !c.is_control() {
                        self.key(c, now, word_at, theme, list);
                    }
                }
            }
            _ => {}
        }

        // The last seconds: the whole page beats red.
        if let State::Playing(since) = self.state {
            let left = self.length() - (now - since);
            if left < HURRY {
                let beat = ((now * std::f64::consts::TAU * 2.0).sin() * 0.5 + 0.5) as f32;
                painter.rect_filled(rect, 0.0, theme.ansi[1].gamma_multiply(0.03 + 0.05 * beat));
            }
        }

        // Title, always: it drops in, and lands.
        let drop = intro(0.0, 0.75, out_back);
        let title_y = rect.min.y + (rect.height() * 0.14).max(46.0) - (1.0 - drop) * 160.0;
        paint_metal(&painter, Pos2::new(center.x, title_y), Align2::CENTER_CENTER, "Speed Metal", 46.0, theme.accent, drop.clamp(0.0, 1.0));

        // The music: fading out after the round; the speaker, top right, turns it on or off.
        if let (State::Over(at), Some(music)) = (self.state, &self.music) {
            let k = ((now - at) / MUSIC_FADE).min(1.0) as f32;
            music.player.set_volume(MUSIC_VOLUME * (1.0 - k));
            if k >= 1.0 {
                self.music = None;
            } else {
                ui.ctx().request_repaint();
            }
        }
        if sound.is_some() {
            let on = sound == Some(true);
            let at = Rect::from_center_size(Pos2::new(rect.max.x - 28.0, rect.min.y + 28.0), Vec2::splat(30.0));
            let resp = ui.interact(at, egui::Id::new("game-sound"), Sense::click()).on_hover_text(if on { t.game_sound_mute } else { t.game_sound_unmute }).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                painter.rect_filled(at, 6.0, theme.tab_hover);
            }
            paint_speaker(&painter, at.center(), on, if resp.hovered() { theme.text } else { theme.text_muted });
            if resp.clicked() {
                out.sound = Some(!on);
                if on {
                    self.music = None;
                } else if matches!(self.state, State::Countdown(_) | State::Playing(_)) {
                    self.music = Music::start();
                }
            }
        }

        match self.state {
            State::Idle => {
                let rules = intro(0.35, 0.5, out_cubic);
                painter.text(Pos2::new(center.x, title_y + 46.0 + (1.0 - rules) * 14.0), Align2::CENTER_TOP, t.game_rules, FontId::proportional(14.0), theme.text_muted.gamma_multiply(rules));
                if self.play_button(ui, Pos2::new(center.x, center.y + 20.0), t.game_play, theme, now, intro(0.45, 0.55, out_back)) {
                    self.play(now, list, sound);
                }
                // The soundtrack's author.
                let credit = painter.layout_no_wrap(t.game_music_credit.to_owned(), FontId::proportional(11.5), theme.text_muted);
                let credit_at = Rect::from_center_size(Pos2::new(center.x, rect.max.y - 16.0), credit.size());
                let resp = ui.interact(credit_at, egui::Id::new("game-credit"), Sense::click()).on_hover_text(MUSIC_CREDIT_URL).on_hover_cursor(egui::CursorIcon::PointingHand);
                let color = if resp.hovered() { theme.text } else { theme.text_muted.gamma_multiply(0.8 * intro(0.7, 0.5, out_cubic)) };
                painter.text(credit_at.center(), Align2::CENTER_CENTER, t.game_music_credit, FontId::proportional(11.5), color);
                if resp.hovered() {
                    painter.hline(credit_at.x_range(), credit_at.max.y, Stroke::new(1.0, color));
                }
                if resp.clicked() {
                    crate::terminal::open_url(MUSIC_CREDIT_URL);
                }
                // No room on the side: the board under Play, as far as it fits.
                if !wide {
                    let arrive = intro(0.55, 0.6, out_cubic);
                    let below = Rect::from_min_max(Pos2::new(center.x - 180.0, center.y + 104.0 + (1.0 - arrive) * 40.0), Pos2::new(center.x + 180.0, rect.max.y - 34.0));
                    if below.height() > 70.0 {
                        paint_board(&painter, below, scores, None, theme, t, arrive, now);
                    }
                }
            }
            State::AskSound => {
                // A card: sound or not, remembered.
                let card = Rect::from_center_size(Pos2::new(center.x, center.y + 20.0), Vec2::new(380.0, 170.0));
                painter.rect_filled(card, 14.0, theme.chrome_bg);
                painter.rect_stroke(card, 14.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                paint_speaker(&painter, Pos2::new(card.center().x, card.min.y + 32.0), true, theme.accent);
                painter.text(Pos2::new(card.center().x, card.min.y + 66.0), Align2::CENTER_CENTER, t.game_sound_ask, FontId::proportional(18.0), theme.text);
                painter.text(Pos2::new(card.center().x, card.min.y + 92.0), Align2::CENTER_CENTER, t.game_sound_later, FontId::proportional(12.0), theme.text_muted);
                let yes = Rect::from_center_size(Pos2::new(card.center().x + 82.0, card.max.y - 32.0), Vec2::new(150.0, 34.0));
                let no = Rect::from_center_size(Pos2::new(card.center().x - 82.0, card.max.y - 32.0), Vec2::new(150.0, 34.0));
                let yes_button = egui::Button::new(egui::RichText::new(t.game_sound_yes).size(14.0).color(theme.bg)).fill(theme.accent).corner_radius(8.0);
                if ui.put(yes, yes_button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    out.sound = Some(true);
                    self.start(now, list, true);
                } else if ui.put(no, egui::Button::new(egui::RichText::new(t.game_sound_no).size(14.0)).corner_radius(8.0)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    out.sound = Some(false);
                    self.start(now, list, false);
                }
            }
            State::Countdown(since) => {
                let elapsed = now - since;
                let n = (COUNTDOWN - elapsed).ceil().max(1.0) as u32;
                let k = (elapsed.fract()) as f32;
                // Each number slams in, a shock wave rolling out from it.
                let slam = out_back((k / 0.35).min(1.0));
                let size = 60.0 + 110.0 * slam - 30.0 * k;
                let color = theme.accent.gamma_multiply(1.0 - k * 0.7);
                let wave = out_cubic(k);
                painter.circle_stroke(word_at, 40.0 + 300.0 * wave, Stroke::new(10.0 * (1.0 - wave), theme.accent.gamma_multiply(0.5 * (1.0 - wave))));
                painter.circle_stroke(word_at, 20.0 + 180.0 * wave, Stroke::new(4.0 * (1.0 - wave), theme.ansi[3].gamma_multiply(0.4 * (1.0 - wave))));
                painter.text(word_at, Align2::CENTER_CENTER, n.to_string(), FontId::new(size, egui::FontFamily::Name("metal".into())), color);
                ui.ctx().request_repaint();
            }
            State::Playing(since) => self.playing_ui(ui, rect, word_at, theme, t, now, now - since),
            State::Over(at) => {
                if self.over_ui(ui, rect, theme, t, now - at, best.max(self.letters), now, player) {
                    self.play(now, list, sound);
                }
                out.player = self.renamed.take();
            }
        }

        // Sparks and rising texts, over everything.
        let dt_gravity = 520.0;
        self.sparks.retain(|s| now - s.born < s.life as f64);
        for s in &self.sparks {
            let age = (now - s.born) as f32;
            let pos = s.pos + s.vel * age + Vec2::new(0.0, 0.5 * dt_gravity * age * age);
            let k = 1.0 - age / s.life;
            painter.circle_filled(pos, s.size * (0.4 + 0.6 * k), s.color.gamma_multiply(k));
        }
        self.floats.retain(|f| now - f.born < 0.9);
        for f in &self.floats {
            let k = ((now - f.born) / 0.9) as f32;
            let pos = f.pos - Vec2::new(0.0, 70.0 * k);
            painter.text(pos, Align2::CENTER_CENTER, &f.text, FontId::monospace(f.size * (1.0 + 0.25 * k)), f.color.gamma_multiply(1.0 - k));
        }
        if !self.sparks.is_empty() || !self.floats.is_empty() {
            ui.ctx().request_repaint();
        }
        out
    }

    /// This round, for the board.
    fn score(&self) -> TypingScore {
        let typed = self.letters + self.mistakes;
        TypingScore {
            letters: self.letters,
            words: self.done,
            wpm: (self.letters as f64 / 5.0 / (self.length() / 60.0)).round() as u32,
            accuracy: (self.letters * 100).checked_div(typed).unwrap_or(100),
            combo: self.best_combo,
            at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        }
    }

    /// The big button (Play, Play again): varnished, a shine sweeping over it, the text in the metal
    /// font; it lifts when hovered and sinks when pressed; "or [Enter]" under it. `appear`: 0 to 1 (a
    /// little past 1 while landing) as it pops in.
    #[allow(clippy::too_many_arguments)]
    fn play_button(&self, ui: &mut Ui, at: Pos2, text: &str, theme: &Theme, now: f64, appear: f32) -> bool {
        if appear <= 0.01 {
            return false;
        }
        let size = Vec2::new(230.0, 62.0) * appear.max(0.0);
        let base = Rect::from_center_size(at, size);
        let resp = ui.interact(base, egui::Id::new("game-play"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let (hot, down) = (resp.hovered(), resp.is_pointer_button_down_on());
        let lift = ui.ctx().animate_bool_with_time_and_easing(resp.id.with("lift"), hot, 0.18, out_cubic);
        let push = ui.ctx().animate_bool_with_time(resp.id.with("push"), down, 0.06);
        let rect = base.translate(Vec2::new(0.0, -3.0 * lift + 3.0 * push)).expand(3.0 * lift - 2.0 * push);
        let painter = ui.painter();
        let radius = 14.0 * appear.min(1.0);
        let fade = appear.clamp(0.0, 1.0);
        // The glow around it, breathing, brighter when hovered.
        let breath = ((now * 2.4).sin() * 0.5 + 0.5) as f32;
        for k in 1..=5 {
            let spread = k as f32 * (3.0 + 2.0 * lift + 1.5 * breath);
            painter.rect_filled(rect.expand(spread), radius + spread, theme.accent.gamma_multiply((0.035 + 0.035 * lift) * fade));
        }
        // Its shadow, the body, a darker lower half and a varnish on the upper one.
        painter.rect_filled(rect.translate(Vec2::new(0.0, 5.0 - 3.0 * push)), radius, Color32::from_black_alpha((90.0 * fade) as u8));
        let body = theme.accent.lerp_to_gamma(Color32::WHITE, 0.08 * lift);
        painter.rect_filled(rect, radius, body.gamma_multiply(fade));
        let r = radius as u8;
        let (top, bottom) = rect.split_top_bottom_at_fraction(0.5);
        painter.rect_filled(bottom, egui::CornerRadius { nw: 0, ne: 0, sw: r, se: r }, theme.ansi[1].gamma_multiply(0.22 * fade));
        painter.rect_filled(top.shrink2(Vec2::new(3.0, 0.0)).translate(Vec2::new(0.0, 2.0)), egui::CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, Color32::WHITE.gamma_multiply(0.14 * fade));
        painter.rect_stroke(rect, radius, Stroke::new(1.5, theme.accent.lerp_to_gamma(Color32::WHITE, 0.45).gamma_multiply(fade)), egui::StrokeKind::Inside);
        // A shine sweeping across now and then (faster over it).
        let period = if hot { 1.4 } else { 3.2 };
        let sweep = ((now % period) / period) as f32 * 1.8 - 0.4;
        if (0.0..=1.0).contains(&sweep) {
            let x = rect.min.x + rect.width() * sweep;
            let shine = ui.painter_at(rect.shrink(2.0));
            let band = vec![Pos2::new(x - 10.0, rect.max.y), Pos2::new(x + 12.0, rect.min.y), Pos2::new(x + 34.0, rect.min.y), Pos2::new(x + 12.0, rect.max.y)];
            shine.add(egui::Shape::convex_polygon(band, Color32::WHITE.gamma_multiply(0.22 * fade), Stroke::NONE));
        }
        // The text, in the metal font, with its shadow.
        let font = FontId::new((26.0 * appear).max(1.0), egui::FontFamily::Name("metal".into()));
        painter.text(rect.center() + Vec2::new(0.0, 2.0), Align2::CENTER_CENTER, text, font.clone(), Color32::from_black_alpha((150.0 * fade) as u8));
        painter.text(rect.center(), Align2::CENTER_CENTER, text, font, Color32::WHITE.gamma_multiply(fade));
        ui.ctx().request_repaint();
        resp.clicked()
    }

    #[allow(clippy::too_many_arguments)]
    fn playing_ui(&mut self, ui: &mut Ui, rect: Rect, word_at: Pos2, theme: &Theme, t: &Strings, now: f64, elapsed: f64) {
        ui.ctx().request_repaint();
        let painter = ui.painter_at(rect);
        let left = (self.length() - elapsed).max(0.0);
        let hurry = left < HURRY;

        // GO: the word blows up and away, the screen flashes.
        if elapsed < 0.8 {
            let k = out_cubic((elapsed / 0.8) as f32);
            painter.rect_filled(rect, 0.0, Color32::WHITE.gamma_multiply(0.18 * (1.0 - k)));
            painter.text(word_at - Vec2::new(0.0, 4.0), Align2::CENTER_CENTER, t.game_go, FontId::new(70.0 + 260.0 * k, egui::FontFamily::Name("metal".into())), theme.ansi[3].gamma_multiply(1.0 - k));
        }
        // The counters pop in.
        let pop = out_back(((elapsed - 0.15) / 0.5) as f32).max(0.0);

        // The clock: a ring emptying, the seconds inside; red and beating at the end.
        let clock = Pos2::new(rect.center().x, rect.min.y + (rect.height() * 0.30).max(120.0));
        let beat = if hurry { ((now * std::f64::consts::TAU * 2.0).sin() * 0.5 + 0.5) as f32 } else { 0.0 };
        let radius = (36.0 + 4.0 * beat) * pop;
        let color = if hurry { theme.ansi[1] } else { theme.accent };
        painter.circle_stroke(clock, radius, Stroke::new(5.0, theme.tab_hover));
        // The seconds won fill it up again, up to full.
        let fraction = (left / ROUND).min(1.0) as f32;
        let steps = (64.0 * fraction).ceil() as usize + 1;
        let points: Vec<Pos2> = (0..=steps).map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * fraction * k as f32 / steps as f32;
            clock + Vec2::angled(a) * radius
        }).collect();
        painter.add(egui::Shape::line(points, Stroke::new(5.0, color)));
        painter.text(clock, Align2::CENTER_CENTER, format!("{}", left.ceil() as u32), FontId::monospace(24.0), if hurry { theme.ansi[1] } else { theme.text });

        // Letters on the left of the clock, words and combo on its right.
        let stat = |x: f32, align: Align2, value: String, label: &str, color: Color32| {
            painter.text(Pos2::new(x, clock.y - 8.0), align, value, FontId::monospace((34.0 * pop).max(1.0)), color.gamma_multiply(pop.min(1.0)));
            painter.text(Pos2::new(x, clock.y + 20.0), align, label, FontId::proportional(11.5), theme.text_muted.gamma_multiply(pop.min(1.0)));
        };
        stat(clock.x - 90.0, Align2::RIGHT_CENTER, self.letters.to_string(), t.game_letters, theme.text);
        stat(clock.x + 90.0, Align2::LEFT_CENTER, self.done.to_string(), t.game_words, theme.text);
        if self.combo >= 2 {
            let pulse = 1.0 + 0.08 * ((now * 8.0).sin() as f32);
            painter.text(Pos2::new(clock.x + 90.0, clock.y + 42.0), Align2::LEFT_CENTER, format!("COMBO ×{}", self.combo), FontId::monospace(14.0 * pulse), theme.ansi[3]);
        }

        // The word: typed letters lit, the next one underlined, a shake after a mistake.
        let Some(word) = self.words.first().copied() else { return };
        let since_mistake = now - self.shake;
        let dx = if since_mistake < 0.3 { ((since_mistake * 70.0).sin() * 10.0 * (1.0 - since_mistake / 0.3)) as f32 } else { 0.0 };
        let font = FontId::monospace(52.0);
        let mut job = egui::text::LayoutJob::default();
        for (k, c) in word.chars().enumerate() {
            let mut format = egui::TextFormat::simple(font.clone(), theme.text);
            if k < self.typed {
                format.color = theme.accent;
            } else if k == self.typed {
                format.color = if since_mistake < 0.3 { theme.ansi[1] } else { theme.text };
                format.underline = Stroke::new(3.0, theme.accent.gamma_multiply(0.5 + 0.5 * ((now * 5.0).sin() as f32 * 0.5 + 0.5)));
            } else {
                format.color = theme.text_muted;
            }
            job.append(&c.to_string(), 0.0, format);
        }
        let galley = painter.layout_job(job);
        let size = galley.size();
        if since_mistake < 0.3 {
            painter.rect_filled(Rect::from_center_size(word_at, size + Vec2::new(40.0, 20.0)), 12.0, theme.ansi[1].gamma_multiply(0.12 * (1.0 - since_mistake as f32 / 0.3)));
        }
        painter.galley(word_at - size / 2.0 + Vec2::new(dx, 0.0), galley, theme.text);

        // The next ones, fading into the distance.
        let mut x = word_at.x - 0.0;
        let y = word_at.y + 64.0;
        let next: Vec<&str> = self.words.iter().skip(1).take(4).copied().collect();
        let galleys: Vec<_> = next.iter().enumerate().map(|(k, w)| painter.layout_no_wrap(w.to_string(), FontId::monospace(20.0 - 2.0 * k as f32), theme.text_muted.gamma_multiply(0.8 - 0.17 * k as f32))).collect();
        let total: f32 = galleys.iter().map(|g| g.size().x).sum::<f32>() + 24.0 * galleys.len().saturating_sub(1) as f32;
        x -= total / 2.0;
        for g in galleys {
            let w = g.size().x;
            painter.galley(Pos2::new(x, y - g.size().y / 2.0), g, theme.text_muted);
            x += w + 24.0;
        }
        painter.text(Pos2::new(word_at.x, rect.max.y - 24.0), Align2::CENTER_CENTER, t.game_escape, FontId::proportional(11.5), theme.text_muted.gamma_multiply(0.7));
    }

    /// The results; true when "play again" was clicked.
    #[allow(clippy::too_many_arguments)]
    fn over_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings, since: f64, best: u32, now: f64, player: &str) -> bool {
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let at = |delay: f64, length: f64, ease: fn(f32) -> f32| ease(((since - delay) / length) as f32);
        if since < 2.4 || self.record {
            ui.ctx().request_repaint();
        }
        // The time is up: a flash, "end of the set" slammed down, the score counting up, the rest after.
        if since < 0.35 {
            painter.rect_filled(rect, 0.0, Color32::WHITE.gamma_multiply(0.35 * (1.0 - since as f32 / 0.35)));
        }
        let top = rect.min.y + (rect.height() * 0.14).max(46.0) + 60.0;
        let slam = at(0.0, 0.45, out_back);
        painter.text(Pos2::new(center.x, top), Align2::CENTER_CENTER, t.game_over, FontId::new(26.0 + 40.0 * (1.0 - slam.min(1.0)), egui::FontFamily::Name("metal".into())), theme.text.gamma_multiply(slam.clamp(0.0, 1.0)));
        let count = at(0.3, 1.2, out_cubic);
        let shown = (self.letters as f32 * count).round() as u32;
        let score_pop = at(0.3, 0.5, out_back).max(0.0);
        painter.text(Pos2::new(center.x, top + 62.0), Align2::CENTER_CENTER, shown.to_string(), FontId::new((84.0 * score_pop).max(1.0), egui::FontFamily::Name("metal".into())), theme.accent);
        painter.text(Pos2::new(center.x, top + 116.0), Align2::CENTER_CENTER, format!("{}  ·  {}", t.game_letters, self.length_text()), FontId::proportional(13.0), theme.text_muted.gamma_multiply(score_pop.min(1.0)));
        let verdict = at(1.3, 0.4, out_cubic);
        if verdict <= 0.0 {
        } else if self.record {
            let pulse = 1.0 + 0.06 * ((now * 6.0).sin() as f32);
            painter.text(Pos2::new(center.x, top + 146.0), Align2::CENTER_CENTER, t.game_record, FontId::proportional(18.0 * pulse), theme.ansi[3].gamma_multiply(verdict));
        } else if let Some(rank) = self.rank {
            painter.text(Pos2::new(center.x, top + 146.0), Align2::CENTER_CENTER, if rank == 0 { t.game_rank_first.to_owned() } else { t.game_rank.replace("{n}", &(rank + 1).to_string()) }, FontId::proportional(14.0), theme.ansi[3].gamma_multiply(verdict));
        } else {
            painter.text(Pos2::new(center.x, top + 146.0), Align2::CENTER_CENTER, t.game_best.replace("{n}", &best.to_string()), FontId::proportional(13.0), theme.ansi[3].gamma_multiply(verdict));
        }
        // Words, per minute, accuracy, best combo.
        let typed = self.letters + self.mistakes;
        let accuracy = (self.letters * 100).checked_div(typed).unwrap_or(100);
        let wpm = self.letters as f64 / 5.0 / (self.length() / 60.0);
        let stats = [
            (self.done.to_string(), t.game_words),
            (format!("{wpm:.0}"), t.game_wpm),
            (format!("{accuracy} %"), t.game_accuracy),
            (format!("×{}", self.best_combo), t.game_combo),
        ];
        let y = top + 196.0;
        let step = 120.0;
        // One after the other, rising into place.
        for (i, (value, label)) in stats.iter().enumerate() {
            let a = at(0.9 + 0.12 * i as f64, 0.45, out_cubic);
            let x = center.x + (i as f32 - 1.5) * step;
            let dy = (1.0 - a) * 18.0;
            painter.text(Pos2::new(x, y + dy), Align2::CENTER_CENTER, value, FontId::monospace(24.0), theme.text.gamma_multiply(a));
            painter.text(Pos2::new(x, y + 24.0 + dy), Align2::CENTER_CENTER, *label, FontId::proportional(11.5), theme.text_muted.gamma_multiply(a));
        }
        let again = self.play_button(ui, Pos2::new(center.x, y + 84.0), t.game_again, theme, now, at(1.6, 0.5, out_back));
        // Below: the card of the round, to share.
        let share = at(1.9, 0.4, out_cubic);
        if share > 0.01 && self.share.is_none() {
            let button = egui::Button::new(egui::RichText::new(t.game_share).size(13.0).color(theme.text_muted.gamma_multiply(share))).frame_when_inactive(false).corner_radius(6.0);
            let at = Rect::from_center_size(Pos2::new(center.x, y + 84.0 + 31.0 + 26.0), Vec2::new(220.0, 28.0));
            if ui.put(at, button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.share = Some(super::scorecard::ScoreCard::new(player, self.letters, self.length_text(), self.done, wpm.round() as u32, accuracy, self.best_combo, self.record));
            }
        }
        if let Some(card) = &mut self.share {
            if card.ui(ui.ctx(), theme, t, now, &mut self.clipboard) {
                // The name typed, kept for the next cards (an empty field forgets nothing).
                let name = card.name.trim().to_owned();
                if !name.is_empty() && name != player {
                    self.renamed = Some(name);
                }
                self.share = None;
            }
            return false;
        }
        again
    }
}

/// `secs` as "1 min" or "1 min 08 s" (rounded to the second).
fn round_time(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 60 && s.is_multiple_of(60) { format!("{} min", s / 60) } else { format_duration(std::time::Duration::from_secs(s)) }
}

/// A speaker; with sound waves when `on`, crossed out otherwise.
fn paint_speaker(painter: &egui::Painter, c: Pos2, on: bool, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let body = vec![c + Vec2::new(-8.0, -3.0), c + Vec2::new(-4.5, -3.0), c + Vec2::new(0.0, -7.5), c + Vec2::new(0.0, 7.5), c + Vec2::new(-4.5, 3.0), c + Vec2::new(-8.0, 3.0)];
    painter.add(egui::Shape::convex_polygon(body, color, Stroke::NONE));
    if on {
        for r in [4.5, 8.0] {
            let points: Vec<Pos2> = (0..=10).map(|k| c + Vec2::new(1.5, 0.0) + Vec2::angled(-0.8 + 1.6 * k as f32 / 10.0) * r).collect();
            painter.add(egui::Shape::line(points, stroke));
        }
    } else {
        painter.line_segment([c + Vec2::new(3.0, -3.5), c + Vec2::new(10.0, 3.5)], stroke);
        painter.line_segment([c + Vec2::new(3.0, 3.5), c + Vec2::new(10.0, -3.5)], stroke);
    }
}

/// The leaderboard in `rect`: its best rounds, the podium in gold, silver and bronze, `highlight` (the
/// round just played) lit; `alpha`: dimmed while playing.
#[allow(clippy::too_many_arguments)]
fn paint_board(painter: &egui::Painter, rect: Rect, scores: &[TypingScore], highlight: Option<usize>, theme: &Theme, t: &Strings, alpha: f32, now: f64) {
    let fade = |c: Color32| c.gamma_multiply(alpha);
    painter.rect_filled(rect, 12.0, fade(theme.chrome_bg));
    painter.rect_stroke(rect, 12.0, Stroke::new(1.0, fade(theme.tab_hover)), egui::StrokeKind::Inside);
    let inner = rect.shrink2(Vec2::new(16.0, 14.0));
    painter.text(Pos2::new(inner.center().x, inner.min.y + 10.0), Align2::CENTER_CENTER, t.game_board, FontId::proportional(15.0), fade(theme.text));
    painter.hline(inner.center().x - 40.0..=inner.center().x + 40.0, inner.min.y + 26.0, Stroke::new(2.0, fade(theme.accent)));
    if scores.is_empty() {
        painter.text(Pos2::new(inner.center().x, inner.min.y + 58.0), Align2::CENTER_CENTER, t.game_board_empty, FontId::proportional(12.5), fade(theme.text_muted));
        return;
    }
    // Columns: place, letters, per minute, accuracy, date. Laid out from the right, the date being
    // the widest (measured), the numbers sharing what is left.
    let date_w = painter.layout_no_wrap("00/00 00:00".to_owned(), FontId::monospace(11.0), theme.text).size().x;
    let accuracy_x = inner.max.x - date_w - 38.0;
    let letters_x = inner.min.x + 40.0;
    let wpm_x = (letters_x + 44.0 + accuracy_x - 26.0) / 2.0;
    let cols = [inner.min.x + 12.0, letters_x, wpm_x, accuracy_x, inner.max.x];
    let head_y = inner.min.y + 46.0;
    let small = FontId::proportional(10.5);
    for (x, align, label) in [(cols[1], Align2::LEFT_CENTER, t.game_letters), (cols[2], Align2::CENTER_CENTER, t.game_wpm), (cols[3], Align2::CENTER_CENTER, t.game_accuracy), (cols[4], Align2::RIGHT_CENTER, t.game_date)] {
        painter.text(Pos2::new(x, head_y), align, label, small.clone(), fade(theme.text_muted));
    }
    const ROW: f32 = 28.0;
    let bronze = Color32::from_rgb(0xcd, 0x7f, 0x32);
    for (k, s) in scores.iter().enumerate() {
        let y = head_y + 22.0 + k as f32 * ROW;
        if y + ROW / 2.0 > inner.max.y {
            break;
        }
        let row = Rect::from_center_size(Pos2::new(inner.center().x, y), Vec2::new(inner.width() + 12.0, ROW - 4.0));
        if highlight == Some(k) {
            let pulse = ((now * 4.0).sin() * 0.5 + 0.5) as f32;
            painter.rect_filled(row, 6.0, fade(theme.accent.gamma_multiply(0.14 + 0.1 * pulse)));
        } else if k % 2 == 0 {
            painter.rect_filled(row, 6.0, fade(theme.bg.gamma_multiply(0.5)));
        }
        let medal = match k {
            0 => Some(theme.ansi[3]),
            1 => Some(theme.text),
            2 => Some(bronze),
            _ => None,
        };
        let place = Pos2::new(cols[0], y);
        match medal {
            Some(c) => {
                painter.circle_filled(place, 9.0, fade(c.gamma_multiply(0.25)));
                painter.text(place, Align2::CENTER_CENTER, (k + 1).to_string(), FontId::monospace(12.0), fade(c));
            }
            None => {
                painter.text(place, Align2::CENTER_CENTER, (k + 1).to_string(), FontId::monospace(12.0), fade(theme.text_muted));
            }
        }
        painter.text(Pos2::new(cols[1], y), Align2::LEFT_CENTER, s.letters.to_string(), FontId::monospace(15.0), fade(medal.unwrap_or(theme.text)));
        painter.text(Pos2::new(cols[2], y), Align2::CENTER_CENTER, s.wpm.to_string(), FontId::monospace(12.5), fade(theme.fg));
        painter.text(Pos2::new(cols[3], y), Align2::CENTER_CENTER, format!("{} %", s.accuracy), FontId::monospace(12.5), fade(theme.fg));
        let when = {
            use chrono::TimeZone as _;
            chrono::Local.timestamp_opt(s.at, 0).single().map(|d| d.format("%d/%m %H:%M").to_string()).unwrap_or_default()
        };
        painter.text(Pos2::new(cols[4], y), Align2::RIGHT_CENTER, when, FontId::monospace(11.0), fade(theme.text_muted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_are_optional() {
        assert_eq!(fold('é'), 'e');
        assert_eq!(fold('Ç'), 'c');
        assert_eq!(fold('a'), 'a');
    }

    #[test]
    fn a_word_typed_counts() {
        let theme = crate::theme::PRESETS[0].theme();
        let mut game = Game::default();
        game.start(0.0, WORDS_EN, false);
        game.state = State::Playing(0.0);
        let word = game.words[0];
        let second = game.words[1];
        // A wrong key first: counted, the combo lost, nothing typed.
        game.key('#', 1.0, Pos2::ZERO, &theme, WORDS_EN);
        assert_eq!((game.mistakes, game.typed), (1, 0));
        for c in word.chars() {
            game.key(c, 1.0, Pos2::ZERO, &theme, WORDS_EN);
        }
        assert_eq!(game.letters as usize, word.chars().count());
        assert_eq!((game.done, game.combo, game.typed), (1, 1, 0));
        assert_eq!(game.words[0], second);
        assert!(!game.sparks.is_empty());
    }
}
