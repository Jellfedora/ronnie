//! Lundi matin, in the games (its rules on floor, its connection in `crate::zombie`): survive the
//! waves of office zombies in the open space, 1 to 4 colleagues. Its welcome screen lists the rooms
//! (open one, join one) beside the world's board; in a room, who's there, the host starts, the others
//! wait (and colleagues online can be invited); in a game, the office seen from above.
//! ZQSD / WASD / arrows to move, the mouse to aim, its button to fire, 1 2 3 4 for a weapon, R to
//! reload, Space to say ready during the coffee break.

use super::four::HomeGame;
use super::*;
use crate::zombie::{AgnesState, Frame, Happened, Kind, Milestone, Online, Over, State, Status};

/// The players' colors, by seat.
const COLORS: [Color32; 4] = [Color32::from_rgb(79, 195, 255), Color32::from_rgb(255, 122, 182), Color32::from_rgb(255, 211, 79), Color32::from_rgb(94, 224, 122)];
/// The zombies' skin.
const ZOMBIE: Color32 = Color32::from_rgb(143, 209, 106);
/// The input is sent again at least this often (s), and whenever it changes.
const INPUT_EVERY: f64 = 0.1;
/// Seconds a revive takes (floor's REVIVE).
const REVIVE: f32 = 3.0;

#[derive(Default)]
pub(super) struct Monday {
    pub(super) online: Online,
    pub(super) floor: Option<crate::floor::GameFloor>,
    /// The game just over, shown until dismissed.
    over: Option<Over>,
    /// The last input sent, and when.
    sent: Option<((i8, i8, i32, bool), f64)>,
    /// Little texts on the field: text, x, y (world), color, since.
    floats: Vec<(String, f32, f32, Color32, f64)>,
    /// A line across the top: text, color, since.
    banner: Option<(String, Color32, f64)>,
    /// An error floor sent (a room full...), and since.
    error: Option<(String, f64)>,
    sound: Option<super::blob::Sound>,
    /// The ready key, pressed during this break (not sent again at each frame).
    ready: bool,
    /// The perks picked for the next game (sent to floor as they change).
    perks: Option<Vec<String>>,
    /// The letters' window is open.
    letters_open: bool,
    /// Shift, at the last frame.
    shift_was: bool,
    /// The menu of Escape is open (alone, the game is paused meanwhile).
    menu: bool,
    /// The game won: the end credits rolling, since (NaN: from the next frame).
    credits: Option<f64>,
    /// Blood on the floor, for a while: x, y (world), since, seed, size. A zombie gone leaves a pool,
    /// a hit a few drops.
    stains: Vec<(f32, f32, f64, u32, f32)>,
    /// The zombies at the last frame painted (id, x, y, life, radius), and its floor.
    seen: (usize, Vec<Seen>),
    /// The welcome screen's height, at the last frame (for its scrolling).
    lobby_h: f32,
    /// A zombie of the bestiary opened (its place in BESTIARY).
    beast: Option<usize>,
}

/// The welcome screen's cards.
#[derive(Clone, Copy)]
enum Card {
    Play,
    Career,
    Bestiary,
    Howto,
    Rewards,
    Board,
}

/// Inside a card of the welcome screen, around.
const LOBBY_PAD: f32 = 18.0;

/// The bestiary, in the order they come: each kind, its first wave, a boss. (Its place here is its bit
/// in `monday_seen`.)
const BESTIARY: [(Kind, u32, bool); 11] = [
    (Kind::Base, 1, false),
    (Kind::Intern, 2, false),
    (Kind::Meeting, 3, false),
    (Kind::Sales, 4, false),
    (Kind::Hr, 5, true),
    (Kind::ReplyAll, 6, false),
    (Kind::Visio, 7, false),
    (Kind::Accountant, 9, false),
    (Kind::Coach, 12, false),
    (Kind::It, 14, false),
    (Kind::Board, 20, true),
];

/// A zombie as last painted: id, x, y, life, radius.
type Seen = (u32, f32, f32, f32, f32);

/// How long blood stays on the floor, in seconds (drying, then fading at the end).
const STAIN_LIFE: f64 = 24.0;

impl Monday {
    /// Blood where zombies were hit or went down since the last frame painted; the old one gone.
    fn bleed(&mut self, frame: &Frame, now: f64) {
        if self.seen.0 != frame.floor {
            self.stains.clear();
            self.seen.1.clear();
        }
        let mut seed = (now * 1000.0) as u32;
        for &(id, x, y, life, r) in &self.seen.1 {
            seed = seed.wrapping_mul(747_796_405).wrapping_add(id);
            match frame.zombies.iter().find(|z| z.id == id) {
                None => self.stains.push((x, y, now, seed, r * 1.5)),
                Some(z) if z.life < life - 0.02 => self.stains.push((z.x, z.y, now, seed, r * (0.45 + (life - z.life).min(0.6)))),
                _ => {}
            }
        }
        self.seen = (frame.floor, frame.zombies.iter().map(|z| (z.id, z.x, z.y, z.life, z.kind.radius())).collect());
        self.stains.retain(|s| now - s.2 < STAIN_LIFE);
        let over = self.stains.len().saturating_sub(120);
        self.stains.drain(..over);
    }
}

impl Monday {
    /// The page left: out of the room. A game under way alone is paused (it waits for the player's
    /// return, half an hour at most); with colleagues, the player quits it (the character stays until
    /// the end, offline).
    pub(super) fn leave(&mut self) {
        if self.online.connected() {
            if self.online.playing() {
                if self.online.room.as_ref().is_some_and(|r| r.players.len() == 1) {
                    self.online.pause(true);
                } else {
                    self.online.quit();
                }
            }
            self.menu = false;
            self.online.disconnect();
            self.floats.clear();
            self.banner = None;
        }
    }
}

impl App {
    pub(super) fn open_monday(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Monday;
    }

    /// After an invitation accepted: to the room of who invited.
    pub(super) fn monday_join(&mut self, host: Option<String>) {
        if let Some(host) = host {
            if self.monday.online.status == Status::Ready {
                self.monday.online.join_host(&host);
            } else {
                self.monday.online.join_host = Some(host);
            }
        }
        self.open_monday();
    }

    fn monday_sound(&mut self, notes: &[(f32, f32, f32, f32)]) {
        if self.config.settings.game_sound == Some(false) {
            return;
        }
        if self.monday.sound.is_none() {
            self.monday.sound = super::blob::Sound::open();
        }
        if let Some(sound) = &self.monday.sound {
            sound.tones(notes);
        }
    }

    pub(super) fn monday_page(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        let playing = self.monday.online.playing();
        // A game under way is the half-day of work's; once played, the rooms give way (its end shown first).
        if playing {
            self.count_round(HomeGame::Monday);
        } else if self.monday.over.is_none()
            && let Some(p) = self.game_locked(HomeGame::Monday)
        {
            if self.monday.online.room.is_some() {
                self.monday.online.leave();
            }
            self.fold_for_game(false);
            return self.locked_page(ui, rect, HomeGame::Monday, &p);
        }
        self.fold_for_game(playing);
        ui.painter_at(rect).rect_filled(rect, 0.0, theme.bg);

        // Connected while the page shows, with a pseudo.
        let account = self.config.settings.floor.clone();
        let pseudo = crate::floor::pseudo(account.as_ref()).map(str::to_owned);
        if let (Some(account), true) = (account.as_ref(), pseudo.is_some())
            && !self.monday.online.connected()
            && !matches!(self.monday.online.status, Status::Failed(_))
        {
            self.monday.online.connect(ui.ctx(), account);
        }
        let news = self.monday.online.poll();
        self.monday_news(news, now, pseudo.as_deref());
        self.monday_mirror();
        ui.ctx().request_repaint();

        if self.monday.online.playing() && self.monday.online.frame.is_some() {
            self.monday_game(ui, rect, now, pseudo.as_deref().unwrap_or_default());
        } else {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.monday.online.leave();
                self.home_game = false;
                return;
            }
            paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 40.0), Align2::CENTER_CENTER, t.monday_name, 34.0, theme.accent, 1.0);
            let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 80.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
            if self.monday.online.room.is_some() {
                self.monday_room(ui, body, now, pseudo.as_deref().unwrap_or_default());
            } else {
                self.monday_lobby(ui, body, now, pseudo.is_some());
            }
        }
        if let Some(start) = self.monday.credits {
            let start = if start.is_nan() { now } else { start };
            self.monday.credits = Some(start);
            let skip = now - start > 0.5 && ui.input(|i| i.pointer.any_pressed() || i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
            if skip || !credits(ui, rect, (now - start) as f32, t) {
                self.monday.credits = None;
            }
        } else if let Some(over) = self.monday.over.clone() {
            self.monday_over(ui, rect, &over);
        }
        if self.monday.letters_open {
            self.monday_letters(ui.ctx());
        }
        if let Some(k) = self.monday.beast {
            self.monday_beast(ui.ctx(), k, now);
        }
        if let Some((text, at)) = &self.monday.error
            && now - at < 3.0
        {
            paint_pill(ui.painter(), Pos2::new(rect.center().x, rect.max.y - 40.0), text, theme.ansi[1], &theme);
        }
    }

    /// What floor said happened: sounds, little texts, the banner, the game's end.
    fn monday_news(&mut self, news: crate::zombie::News, now: f64, pseudo: Option<&str>) {
        let t = self.t();
        let theme = self.theme.clone();
        let me = pseudo.and_then(|p| self.monday.online.me(p));
        if let Some(error) = news.error {
            self.monday.error = Some((error, now));
        }
        for h in news.happened {
            match h {
                Happened::Wave { wave, boss } => {
                    self.monday.ready = false;
                    let text = if boss { format!("{}  ·  {}", t.monday_wave.replace("{n}", &wave.to_string()), t.monday_boss) } else { t.monday_wave.replace("{n}", &wave.to_string()) };
                    self.monday.banner = Some((text, if boss { theme.ansi[1] } else { theme.accent }, now));
                    self.monday_sound(&[(220.0, 220.0, 0.12, 0.25), (330.0, 330.0, 0.12, 0.25), (440.0, 440.0, 0.25, 0.25)]);
                }
                Happened::Cleared(wave) => {
                    self.monday.banner = Some((t.monday_cleared.replace("{n}", &wave.to_string()), theme.ansi[2], now));
                    self.monday_sound(&[(523.0, 523.0, 0.1, 0.2), (659.0, 659.0, 0.1, 0.2), (784.0, 784.0, 0.2, 0.2)]);
                }
                Happened::FinalBoss => {
                    self.monday.banner = Some((t.monday_final_boss.to_owned(), Color32::from_rgb(222, 180, 80), now));
                    self.monday_sound(&[(98.0, 70.0, 0.6, 0.32), (130.0, 98.0, 0.6, 0.3)]);
                }
                Happened::Floor(k) => {
                    let names: Vec<&str> = t.monday_floors.split('|').collect();
                    self.monday.banner = Some((names.get(k).copied().unwrap_or_default().to_owned(), theme.accent, now));
                    self.monday.floats.clear();
                    self.monday_sound(&[(392.0, 392.0, 0.15, 0.22), (523.0, 523.0, 0.15, 0.22), (659.0, 659.0, 0.15, 0.22), (784.0, 784.0, 0.35, 0.22)]);
                }
                Happened::Won => {
                    self.monday.banner = Some((t.monday_won.to_owned(), Color32::from_rgb(222, 180, 80), now));
                    self.monday_sound(&[(523.0, 523.0, 0.15, 0.25), (659.0, 659.0, 0.15, 0.25), (784.0, 784.0, 0.15, 0.25), (1046.0, 1046.0, 0.6, 0.25)]);
                }
                Happened::Paused(_) => {}
                Happened::Boss | Happened::Recruit => {
                    let text = if h == Happened::Boss { t.monday_boss } else { t.monday_recruit };
                    self.monday.banner = Some((text.to_owned(), theme.ansi[1], now));
                    self.monday_sound(&[(110.0, 80.0, 0.5, 0.3)]);
                }
                Happened::Kill { kind, x, y, by } => {
                    let points = match kind {
                        Kind::Base => 10,
                        Kind::Intern => 8,
                        Kind::Meeting => 25,
                        Kind::Sales => 20,
                        Kind::ReplyAll => 15,
                        Kind::Cc => 3,
                        Kind::Hr => 300,
                        Kind::Board => 400,
                        Kind::Visio => 14,
                        Kind::Accountant => 22,
                        Kind::Coach => 30,
                        Kind::It => 25,
                    };
                    let color = if Some(by) == me { theme.accent } else { theme.text_muted };
                    self.monday.floats.push((format!("+{points}"), x, y, color, now));
                    if Some(by) == me {
                        self.monday_sound(&[(660.0, 440.0, 0.05, 0.12)]);
                    }
                }
                Happened::Reboot { x, y } => {
                    self.monday.floats.push((t.monday_reboot.to_owned(), x, y - 30.0, Color32::from_rgb(110, 200, 255), now));
                    self.monday_sound(&[(880.0, 880.0, 0.06, 0.12), (660.0, 660.0, 0.06, 0.12), (440.0, 440.0, 0.12, 0.12)]);
                }
                Happened::Healed { player, x, y } => {
                    self.monday.floats.push(("+40 ♥".to_owned(), x, y - 30.0, Color32::from_rgb(120, 230, 120), now));
                    if Some(player) == me {
                        self.monday_sound(&[(523.0, 523.0, 0.08, 0.2), (784.0, 784.0, 0.15, 0.2)]);
                    }
                }
                Happened::Down(id) => {
                    if Some(id) == me {
                        self.monday_sound(&[(300.0, 120.0, 0.5, 0.3)]);
                    }
                }
                Happened::Revived { player, by } => {
                    if Some(player) == me || Some(by) == me {
                        self.monday_sound(&[(392.0, 392.0, 0.08, 0.2), (523.0, 523.0, 0.08, 0.2), (659.0, 659.0, 0.15, 0.2)]);
                    }
                }
                Happened::Agnes => self.monday.banner = Some((t.monday_agnes.to_owned(), AGNES, now)),
                Happened::Gerard => self.monday.banner = Some((t.monday_gerard.to_owned(), GERARD, now)),
                Happened::AgnesHit(_) => {
                    self.monday.banner = Some((t.monday_agnes_hit.to_owned(), theme.ansi[1], now));
                    self.monday_sound(&[(520.0, 300.0, 0.25, 0.2)]);
                }
                Happened::AgnesDown => {
                    self.monday.banner = Some((t.monday_agnes_down.to_owned(), theme.ansi[1], now));
                    self.monday_sound(&[(440.0, 180.0, 0.4, 0.22)]);
                }
                Happened::Drop(w) => {
                    let names: Vec<&str> = t.monday_weapons.split('|').collect();
                    self.monday.banner = Some((t.monday_drop.replace("{w}", names.get(w).copied().unwrap_or_default()), WEAPON_COLORS[w], now));
                    self.monday_sound(&[(500.0, 500.0, 0.05, 0.15), (750.0, 750.0, 0.08, 0.15)]);
                }
                Happened::Picked { player, weapon, gain } => {
                    if Some(player) == me {
                        let names: Vec<&str> = t.monday_weapons.split('|').collect();
                        let lines: Vec<&str> = t.monday_picked.split('|').collect();
                        let k = ["new", "dmg", "mag", "ammo"].iter().position(|g| *g == gain).unwrap_or(3);
                        let text = lines.get(k).copied().unwrap_or_default().replace("{w}", names.get(weapon).copied().unwrap_or_default());
                        self.monday.banner = Some((text, WEAPON_COLORS[weapon], now));
                        self.monday_sound(&[(600.0, 600.0, 0.06, 0.18), (900.0, 900.0, 0.06, 0.18), (1200.0, 1200.0, 0.15, 0.18)]);
                    }
                }
                Happened::AgnesUp(_) => {
                    if let Some(a) = self.monday.online.frame.as_ref().and_then(|f| f.agnes) {
                        self.monday.floats.push((t.monday_agnes_up.to_owned(), a.x, a.y - 30.0, AGNES, now));
                    }
                }
                Happened::Coffee(_) => {
                    self.monday.banner = Some((t.monday_coffee.to_owned(), AGNES, now));
                    self.monday_sound(&[(523.0, 523.0, 0.12, 0.2), (659.0, 659.0, 0.12, 0.2), (784.0, 784.0, 0.12, 0.2), (1046.0, 1046.0, 0.3, 0.2)]);
                }
                Happened::PostIt => {
                    self.monday.banner = Some((t.monday_postit.to_owned(), AGNES, now));
                    self.monday_sound(&[(784.0, 784.0, 0.2, 0.2), (659.0, 659.0, 0.2, 0.2), (1046.0, 1046.0, 0.5, 0.2)]);
                }
                Happened::Reunion => {
                    self.monday.banner = Some((t.monday_reunion.to_owned(), Color32::from_rgb(255, 122, 182), now));
                    // Seen once: Gérard strolls along the foot of the screen from now on.
                    if !self.config.settings.monday_gerard {
                        self.config.settings.monday_gerard = true;
                        self.save_config();
                        self.push_toast(super::Toast { ok: true, title: t.monday_ms_gerard.to_owned(), body: t.unlock_gerard.to_owned(), tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
                    }
                    self.monday_sound(&[(659.0, 659.0, 0.2, 0.2), (784.0, 784.0, 0.2, 0.2), (988.0, 988.0, 0.2, 0.2), (1318.0, 1318.0, 0.6, 0.2)]);
                }
                Happened::LetterDrop => self.monday.banner = Some((t.monday_letter_drop.to_owned(), AGNES, now)),
                Happened::Letter(by) => {
                    if Some(by) == me {
                        self.monday.banner = Some((t.monday_letter.to_owned(), AGNES, now));
                        self.monday_sound(&[(880.0, 880.0, 0.1, 0.2), (1175.0, 1175.0, 0.25, 0.2)]);
                    }
                }
                Happened::Bought { player, .. } => {
                    if Some(player) == me {
                        self.monday_sound(&[(700.0, 900.0, 0.08, 0.18), (900.0, 900.0, 0.1, 0.18)]);
                    }
                }
                Happened::Dash(by) | Happened::Teleport(by) => {
                    if Some(by) == me {
                        self.monday_sound(&[(300.0, 700.0, 0.12, 0.15)]);
                    }
                }
                Happened::Dead(_) | Happened::Back(_) => {}
            }
        }
        self.monday.floats.retain(|f| now - f.4 < 0.9);
        if let Some(over) = news.over {
            if over.won {
                self.monday.credits = Some(f64::NAN);
            }
            self.monday.over = Some(over.clone());
            if over.wave > self.config.settings.monday_best {
                self.config.settings.monday_best = over.wave;
                self.save_config();
            }
            self.monday.banner = None;
            self.monday.floats.clear();
            self.monday.ready = false;
            if let Some(floor) = &mut self.monday.floor {
                // Fetched again: the board just changed.
                floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
            }
            self.monday_sound(&[(392.0, 330.0, 0.3, 0.25), (330.0, 262.0, 0.3, 0.25), (262.0, 196.0, 0.6, 0.25)]);
            // The steps passed: each said in a toast (the rewards hang on them).
            for m in &over.milestones {
                let (title, body) = self.milestone_text(m);
                self.push_toast(super::Toast { ok: true, title, body, tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
            }
        }
    }

    /// The welcome screen, in cards: to play (the rooms), the career, the bestiary, how to play, the
    /// rewards, the world's board; three columns on a wide page, two, or one. Scrolls when too tall.
    fn monday_lobby(&mut self, ui: &mut Ui, body: Rect, now: f64, has_pseudo: bool) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt("monday-lobby").auto_shrink(false).show(&mut child, |ui| {
            let w = ui.available_width();
            let (area, _) = ui.allocate_exact_size(Vec2::new(w, self.monday.lobby_h.max(body.height() - 8.0)), Sense::hover());
            self.monday.lobby_h = self.monday_lobby_cards(ui, area, now, has_pseudo);
        });
    }

    /// The cards in `area`; the height they take.
    fn monday_lobby_cards(&mut self, ui: &mut Ui, area: Rect, now: f64, has_pseudo: bool) -> f32 {
        let gap = 18.0;
        let cols: usize = if area.width() >= 1180.0 { 3 } else if area.width() >= 760.0 { 2 } else { 1 };
        let width = area.width().min(if cols == 3 { 1280.0 } else if cols == 2 { 900.0 } else { 480.0 });
        let col_w = (width - gap * (cols - 1) as f32) / cols as f32;
        let x0 = area.center().x - width / 2.0;
        let mut bottoms = vec![area.min.y; cols];
        let order: &[(Card, usize)] = match cols {
            3 => &[(Card::Play, 0), (Card::Career, 0), (Card::Bestiary, 1), (Card::Howto, 1), (Card::Rewards, 2), (Card::Board, 2)],
            2 => &[(Card::Play, 0), (Card::Career, 0), (Card::Howto, 0), (Card::Bestiary, 1), (Card::Rewards, 1), (Card::Board, 1)],
            _ => &[(Card::Play, 0), (Card::Career, 0), (Card::Bestiary, 0), (Card::Howto, 0), (Card::Rewards, 0), (Card::Board, 0)],
        };
        for &(card, col) in order {
            let x = x0 + col as f32 * (col_w + gap);
            let top = bottoms[col];
            // The card's background, under what's drawn in it (its height known once drawn).
            let back = ui.painter().add(egui::Shape::Noop);
            let inner = Rect::from_min_size(Pos2::new(x + LOBBY_PAD, top + LOBBY_PAD), Vec2::new(col_w - 2.0 * LOBBY_PAD, 0.0));
            let h = match card {
                Card::Play => self.lobby_play(ui, inner, has_pseudo),
                Card::Career => self.lobby_career(ui, inner),
                Card::Bestiary => self.lobby_bestiary(ui, inner, now),
                Card::Howto => self.lobby_howto(ui, inner),
                Card::Rewards => self.lobby_rewards(ui, inner),
                Card::Board => self.lobby_board(ui, inner, now),
            };
            if h <= 0.0 {
                continue;
            }
            let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(col_w, h + 2.0 * LOBBY_PAD));
            ui.painter().set(back, egui::Shape::rect_filled(rect, 14.0, self.theme.chrome_bg));
            ui.painter().rect_stroke(rect, 14.0, Stroke::new(1.0, self.theme.tab_hover), egui::StrokeKind::Inside);
            bottoms[col] = rect.max.y + gap;
        }
        bottoms.into_iter().fold(0.0f32, f32::max) - area.min.y
    }

    /// A card's title, at the top of `inner`; with something on its right (a count...).
    fn lobby_title(&self, ui: &Ui, inner: Rect, title: &str, right: Option<(String, Color32)>) {
        let theme = &self.theme;
        let p = ui.painter();
        p.rect_filled(Rect::from_min_size(inner.min + Vec2::new(0.0, 2.0), Vec2::new(3.0, 16.0)), 1.5, theme.accent);
        p.text(inner.min + Vec2::new(12.0, 10.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), theme.text);
        if let Some((text, color)) = right {
            p.text(Pos2::new(inner.max.x, inner.min.y + 10.0), Align2::RIGHT_CENTER, text, FontId::proportional(11.5), color);
        }
    }

    /// To play: open a game (alone or with colleagues), or join one open.
    fn lobby_play(&mut self, ui: &mut Ui, inner: Rect, has_pseudo: bool) -> f32 {
        let t = self.t();
        let theme = self.theme.clone();
        self.lobby_title(ui, inner, t.monday_play, None);
        let (left, w) = (inner.min.x, inner.width());
        let mut y = inner.min.y + 36.0;
        let status = self.monday.online.status.clone();
        if !has_pseudo {
            let galley = ui.painter().layout(t.game_need_pseudo.to_owned(), FontId::proportional(12.5), theme.text_muted, w);
            let h = galley.size().y;
            ui.painter().galley(Pos2::new(left, y), galley, theme.text_muted);
            y += h + 10.0;
            if ui.put(Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 38.0)), egui::Button::new(egui::RichText::new(t.pseudo_pick).size(14.0)).corner_radius(10.0)).clicked() {
                self.ask_pseudo();
            }
            return y + 38.0 - inner.min.y;
        }
        if let Status::Failed(why) = &status {
            ui.painter().text(Pos2::new(left, y + 8.0), Align2::LEFT_CENTER, why.as_deref().unwrap_or(t.monday_failed), FontId::proportional(13.0), theme.ansi[1]);
            y += 26.0;
            if ui.put(Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 36.0)), egui::Button::new(egui::RichText::new(t.monday_retry).size(13.5)).corner_radius(10.0)).clicked() {
                self.monday.online = Online::default();
            }
            return y + 36.0 - inner.min.y;
        }
        if status != Status::Ready {
            ui.painter().text(Pos2::new(left, y + 8.0), Align2::LEFT_CENTER, t.monday_connecting, FontId::proportional(13.0), theme.text_muted);
            return y + 20.0 - inner.min.y;
        }
        // The big button, then what it means.
        let b = egui::Button::new(egui::RichText::new(t.monday_host).size(15.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(10.0);
        if ui.put(Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 42.0)), b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.monday.online.host();
        }
        y += 50.0;
        ui.painter().text(Pos2::new(inner.center().x, y + 6.0), Align2::CENTER_CENTER, t.monday_solo, FontId::proportional(12.0), theme.text_muted);
        y += 26.0;
        // The games open.
        ui.painter().line_segment([Pos2::new(left, y), Pos2::new(inner.max.x, y)], Stroke::new(1.0, theme.tab_hover));
        y += 12.0;
        ui.painter().text(Pos2::new(left, y + 8.0), Align2::LEFT_CENTER, t.monday_rooms, FontId::proportional(13.0), theme.text);
        y += 24.0;
        let lobby = self.monday.online.lobby.clone();
        if lobby.is_empty() {
            ui.painter().text(Pos2::new(left, y + 8.0), Align2::LEFT_CENTER, t.monday_no_rooms, FontId::proportional(12.5), theme.text_muted);
            y += 20.0;
        }
        let mut join = None;
        for room in lobby.iter().take(8) {
            let row = Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 34.0));
            ui.painter().rect_filled(row, 8.0, theme.bg.gamma_multiply(0.7));
            ui.painter().text(Pos2::new(row.min.x + 12.0, row.center().y), Align2::LEFT_CENTER, t.monday_room.replace("{n}", &room.host), FontId::proportional(13.0), theme.text);
            // The seats taken, as dots.
            for k in 0..4 {
                let c = Pos2::new(row.max.x - 112.0 + k as f32 * 9.0, row.center().y);
                if k < room.players as usize {
                    ui.painter().circle_filled(c, 3.0, theme.accent);
                } else {
                    ui.painter().circle_stroke(c, 3.0, Stroke::new(1.0, theme.text_muted));
                }
            }
            let b = Rect::from_min_size(Pos2::new(row.max.x - 74.0, row.min.y + 5.0), Vec2::new(68.0, 24.0));
            if room.started {
                ui.painter().text(b.center(), Align2::CENTER_CENTER, t.monday_in_game, FontId::proportional(11.5), theme.text_muted);
            } else if room.players < 4 && ui.put(b, egui::Button::new(egui::RichText::new(t.monday_join).size(12.0)).corner_radius(6.0)).clicked() {
                join = Some(room.id);
            }
            y += 40.0;
        }
        if let Some(id) = join {
            self.monday.online.join(id);
        }
        y - inner.min.y
    }

    /// The career: the grade and its XP, the day's challenge, the week's goals, Agnès' letters.
    fn lobby_career(&mut self, ui: &mut Ui, inner: Rect) -> f32 {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(c) = self.monday.online.career.clone() else { return 0.0 };
        self.lobby_title(ui, inner, t.monday_career, None);
        let (left, right, w) = (inner.min.x, inner.max.x, inner.width());
        let p = ui.painter().clone();
        let mut y = inner.min.y + 36.0;
        // What it's for, in a line or two.
        let hint = p.layout(t.monday_career_hint.to_owned(), FontId::proportional(11.5), theme.text_muted, w);
        let h = hint.size().y;
        p.galley(Pos2::new(left, y), hint, theme.text_muted);
        y += h + 12.0;
        p.text(Pos2::new(left, y + 8.0), Align2::LEFT_CENTER, grade_name(t, c.grade, c.stars), FontId::proportional(17.0), theme.accent);
        p.text(Pos2::new(right, y + 8.0), Align2::RIGHT_CENTER, t.monday_xp.replace("{xp}", &c.xp.to_string()).replace("{next}", &c.next.to_string()), FontId::monospace(11.5), theme.text_muted);
        y += 22.0;
        let bar = Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 8.0));
        p.rect_filled(bar, 4.0, theme.bg);
        let k = (c.xp.saturating_sub(c.from)) as f32 / (c.next.saturating_sub(c.from)).max(1) as f32;
        p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * k.clamp(0.0, 1.0), bar.height())), 4.0, theme.accent);
        y += 22.0;
        let goals: Vec<&str> = t.monday_goals.split('|').collect();
        let ids = ["revives", "kills", "waves", "escort", "games", "boss"];
        let label = |id: &str, need: u32| if id == "floor" { t.monday_goal_floor.to_owned() } else { ids.iter().position(|i| *i == id).and_then(|k| goals.get(k)).copied().unwrap_or_default().replace("{n}", &need.to_string()) };
        // The day's challenge, set apart.
        if let Some((id, done, need, ok)) = &c.daily {
            let r#box = Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 52.0));
            let tint = if *ok { theme.ansi[2] } else { theme.accent };
            p.rect_filled(r#box, 10.0, tint.gamma_multiply(0.12));
            p.rect_stroke(r#box, 10.0, Stroke::new(1.0, tint.gamma_multiply(0.5)), egui::StrokeKind::Inside);
            p.text(r#box.min + Vec2::new(12.0, 14.0), Align2::LEFT_CENTER, t.monday_daily, FontId::proportional(11.0), tint);
            p.text(r#box.min + Vec2::new(12.0, 34.0), Align2::LEFT_CENTER, label(id, *need), FontId::proportional(13.0), theme.text);
            p.text(Pos2::new(r#box.max.x - 12.0, r#box.center().y), Align2::RIGHT_CENTER, if *ok { "✓".to_owned() } else { format!("{}/{need}", done.min(need)) }, FontId::monospace(13.0), tint);
            y += 64.0;
        }
        // The week's goals, each with its bar.
        p.text(Pos2::new(left, y + 8.0), Align2::LEFT_CENTER, t.monday_week, FontId::proportional(13.0), theme.text);
        p.text(Pos2::new(right, y + 8.0), Align2::RIGHT_CENTER, t.monday_week_xp.replace("{n}", "500"), FontId::proportional(11.0), theme.text_muted);
        y += 24.0;
        for (id, done, need, ok) in &c.week {
            let color = if *ok { theme.ansi[2] } else { theme.text };
            p.text(Pos2::new(left, y + 7.0), Align2::LEFT_CENTER, if *ok { format!("✓  {}", label(id, *need)) } else { label(id, *need) }, FontId::proportional(12.5), color);
            p.text(Pos2::new(right, y + 7.0), Align2::RIGHT_CENTER, format!("{}/{need}", done.min(need)), FontId::monospace(11.5), if *ok { theme.ansi[2] } else { theme.text_muted });
            let bar = Rect::from_min_size(Pos2::new(left, y + 18.0), Vec2::new(w, 3.0));
            p.rect_filled(bar, 1.5, theme.bg);
            p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(w * (*done as f32 / (*need).max(1) as f32).min(1.0), 3.0)), 1.5, if *ok { theme.ansi[2] } else { theme.accent.gamma_multiply(0.8) });
            y += 30.0;
        }
        if c.letters > 0 {
            y += 4.0;
            let r = Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, 32.0));
            let label = t.monday_letters_button.replace("{n}", &c.letters.to_string());
            if ui.put(r, egui::Button::new(egui::RichText::new(label).size(12.5).color(AGNES)).corner_radius(8.0)).clicked() {
                self.monday.letters_open = true;
            }
            y += 32.0;
        }
        y - inner.min.y
    }

    /// The bestiary: every zombie on its tile, its name and its wave; those never met, only their
    /// shadow. Hovered, what it does.
    fn lobby_bestiary(&mut self, ui: &mut Ui, inner: Rect, now: f64) -> f32 {
        let t = self.t();
        let theme = self.theme.clone();
        let s = &self.config.settings;
        let seen: Vec<bool> = BESTIARY.iter().enumerate().map(|(k, (_, from, _))| s.monday_seen & (1 << k) != 0 || s.monday_best >= *from).collect();
        let count = seen.iter().filter(|s| **s).count();
        self.lobby_title(ui, inner, t.monday_bestiary_title, Some((t.monday_seen_count.replace("{n}", &count.to_string()).replace("{max}", &BESTIARY.len().to_string()), theme.text_muted)));
        let names: Vec<&str> = t.monday_bestiary.split('|').collect();
        let notes: Vec<&str> = t.monday_bestiary_notes.split('|').collect();
        let gap = 10.0;
        let cols = (((inner.width() + gap) / (100.0 + gap)) as usize).clamp(3, 6);
        let tile_w = (inner.width() - gap * (cols - 1) as f32) / cols as f32;
        let tile_h = 118.0;
        let top = inner.min.y + 38.0;
        let p = ui.painter().clone();
        for (k, &(kind, from, boss)) in BESTIARY.iter().enumerate() {
            let tile = Rect::from_min_size(Pos2::new(inner.min.x + (k % cols) as f32 * (tile_w + gap), top + (k / cols) as f32 * (tile_h + gap)), Vec2::new(tile_w, tile_h));
            let resp = ui.interact(tile, ui.id().with(("bestiary", k)), Sense::hover());
            let lit = seen[k] && resp.hovered();
            // Each kept in its tile (the coach's halo, the bosses' size).
            let p = p.with_clip_rect(tile.intersect(p.clip_rect()));
            p.rect_filled(tile, 10.0, if lit { theme.tab_hover } else { theme.bg });
            if boss {
                p.rect_stroke(tile, 10.0, Stroke::new(1.0, theme.ansi[1].gamma_multiply(if seen[k] { 0.6 } else { 0.25 })), egui::StrokeKind::Inside);
            }
            let c = Pos2::new(tile.center().x, tile.min.y + 44.0);
            let scale = (22.0 / kind.radius()).min(1.15);
            let phase = (now * 3.0) as f32 + k as f32;
            if seen[k] {
                let cycle = (now * 0.5 + k as f64 * 0.7) % 3.0 < 1.0;
                let fx = Fx { frozen: kind == Kind::Visio && cycle, healing: kind == Kind::Coach, rebooting: kind == Kind::It && cycle, ..Fx::default() };
                paint_zombie(&p, c, scale, kind, std::f32::consts::FRAC_PI_2, k as u32 * 7 + 3, phase, 1.0, fx);
            } else {
                paint_shadow(&p, c, scale, kind, phase);
            }
            let name = if seen[k] { names.get(k).copied().unwrap_or_default() } else { "???" };
            p.text(Pos2::new(tile.center().x, tile.max.y - 30.0), Align2::CENTER_CENTER, name, FontId::proportional(12.0), if seen[k] { theme.text } else { theme.text_muted });
            let wave = t.monday_wave_short.replace("{n}", &from.to_string());
            let (label, color) = if boss { (format!("{}  ·  {wave}", t.monday_boss_tag), theme.ansi[1]) } else { (wave, theme.text_muted) };
            p.text(Pos2::new(tile.center().x, tile.max.y - 13.0), Align2::CENTER_CENTER, label, FontId::proportional(10.5), color.gamma_multiply(if seen[k] { 1.0 } else { 0.6 }));
            if seen[k] {
                let resp = resp.on_hover_text(notes.get(k).copied().unwrap_or_default()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if ui.interact(tile, ui.id().with(("bestiary-click", k)), Sense::click()).clicked() || resp.clicked() {
                    self.monday.beast = Some(k);
                }
            } else {
                resp.on_hover_text(t.monday_unseen);
            }
        }
        let rows = BESTIARY.len().div_ceil(cols);
        top + rows as f32 * (tile_h + gap) - gap - inner.min.y
    }

    /// How to play: the rules, then the keys, each on its keycap.
    fn lobby_howto(&mut self, ui: &mut Ui, inner: Rect) -> f32 {
        let t = self.t();
        let theme = self.theme.clone();
        self.lobby_title(ui, inner, t.bughunt_howto, None);
        let p = ui.painter().clone();
        let mut y = inner.min.y + 36.0;
        let rules = p.layout(t.monday_rules.to_owned(), FontId::proportional(12.5), theme.text_muted, inner.width());
        let h = rules.size().y;
        p.galley(Pos2::new(inner.min.x, y), rules, theme.text_muted);
        y += h + 16.0;
        // « ZQSD ou flèches : bouger · souris : viser, clic : tirer · ... »: the keys, what they do.
        let keys: Vec<(&str, &str)> = t.monday_help.split(" · ").filter_map(|part| part.split_once(" : ").or_else(|| part.split_once(": "))).collect();
        let cap_w = keys.iter().map(|(k, _)| p.layout_no_wrap((*k).to_owned(), FontId::monospace(11.5), theme.text).size().x).fold(0.0f32, f32::max) + 16.0;
        let cap_w = cap_w.min(inner.width() * 0.5);
        for (key, what) in keys {
            let cap = Rect::from_min_size(Pos2::new(inner.min.x, y), Vec2::new(cap_w, 24.0));
            p.rect_filled(cap.translate(Vec2::new(0.0, 2.0)), 6.0, theme.tab_hover);
            p.rect_filled(cap, 6.0, theme.bg);
            p.rect_stroke(cap, 6.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
            p.text(cap.center(), Align2::CENTER_CENTER, key, FontId::monospace(11.5), theme.text);
            p.text(Pos2::new(cap.max.x + 12.0, cap.center().y), Align2::LEFT_CENTER, what, FontId::proportional(12.5), theme.text_muted);
            y += 32.0;
        }
        y - 8.0 - inner.min.y
    }

    /// The rewards, and the credits to see again once the game is finished.
    fn lobby_rewards(&mut self, ui: &mut Ui, inner: Rect) -> f32 {
        let t = self.t();
        let items = self.unlocks(HomeGame::Monday);
        let done = items.iter().filter(|u| u.done && !u.info).count();
        let total = items.iter().filter(|u| !u.info).count();
        self.lobby_title(ui, inner, t.monday_rewards, Some((format!("{done}/{total}"), if done == total { self.theme.ansi[2] } else { self.theme.accent })));
        let painter = ui.painter().clone();
        let mut h = 38.0 + self.monday_rewards(&painter, Rect::from_min_max(inner.min + Vec2::new(0.0, 38.0), Pos2::new(inner.max.x, inner.min.y + 4000.0)));
        if self.config.settings.monday_wins > 0 {
            let r = Rect::from_min_size(Pos2::new(inner.min.x, inner.min.y + h), Vec2::new(inner.width(), 32.0));
            if ui.put(r, egui::Button::new(egui::RichText::new(t.monday_credits_again).size(12.5)).corner_radius(8.0)).clicked() {
                self.monday.credits = Some(f64::NAN);
            }
            h += 32.0;
        }
        h
    }

    /// The world's board.
    fn lobby_board(&mut self, ui: &mut Ui, inner: Rect, now: f64) -> f32 {
        let t = self.t();
        let theme = self.theme.clone();
        let h = 330.0;
        let floor = self.monday.floor.get_or_insert_with(|| crate::floor::GameFloor::new("lundi-matin", false));
        floor.poll();
        floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        let best = (self.config.settings.monday_best > 0).then(|| t.monday_best.replace("{n}", &self.config.settings.monday_best.to_string()));
        let entries = self.monday.floor.as_ref().and_then(|f| f.board()).map(<[_]>::to_vec);
        let kills = |e: &crate::floor::GameEntry| e.details.get("kills").and_then(serde_json::Value::as_u64).map(|k| t.monday_kills.replace("{n}", &k.to_string()));
        if self.game_board_noted(ui, Rect::from_min_size(inner.min, Vec2::new(inner.width(), h)), &theme, entries.as_deref(), best, t_wave, &kills) {
            self.ask_pseudo();
        }
        h
    }

    /// The rewards of the game, at the top of `area`: each one, how to win it, how far the player is.
    /// Its height.
    fn monday_rewards(&self, painter: &egui::Painter, area: Rect) -> f32 {
        let t = self.t();
        let theme = &self.theme;
        let items = self.unlocks(HomeGame::Monday);
        let mut y = area.min.y;
        for u in &items {
            let color = if u.done { theme.ansi[2] } else { theme.accent };
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, if u.done { "✓" } else { "•" }, FontId::proportional(13.0), color);
            painter.text(Pos2::new(area.min.x + 16.0, y), Align2::LEFT_TOP, &u.what, FontId::proportional(13.0), theme.text);
            let state = match (u.done, u.progress) {
                (true, _) => t.unlock_done.to_owned(),
                (false, Some((have, need))) => format!("{}/{need}", have.min(need)),
                (false, None) => String::new(),
            };
            painter.text(Pos2::new(area.max.x, y), Align2::RIGHT_TOP, state, FontId::monospace(11.5), color);
            y += 18.0;
            let said = if u.done { u.said(t) } else { u.how.clone() };
            let how = painter.layout(said, FontId::proportional(11.5), if u.done { theme.text } else { theme.text_muted }, area.width() - 16.0);
            let h = how.size().y;
            painter.galley(Pos2::new(area.min.x + 16.0, y), how, if u.done { theme.text } else { theme.text_muted });
            y += h + 10.0;
        }
        y - 10.0 - area.min.y
    }

    /// In a room, before its game: who's there, start (the host) or wait, invite colleagues online.
    fn monday_room(&mut self, ui: &mut Ui, body: Rect, now: f64, pseudo: &str) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(room) = self.monday.online.room.clone() else { return };
        let wide = body.width() >= 760.0;
        let width = body.width().min(if wide { 860.0 } else { 420.0 });
        let left_w = if wide { 420.0 } else { width };
        let x0 = body.center().x - width / 2.0;
        let card = Rect::from_min_size(Pos2::new(x0, body.min.y), Vec2::new(left_w, 460.0f32.min(body.height())));
        let painter = ui.painter().clone();
        painter.rect(card, 12.0, theme.chrome_bg, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let left = card.min.x + 20.0;
        let mut y = card.min.y + 24.0;
        painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, t.monday_room.replace("{n}", &room.host), FontId::proportional(17.0), theme.text);
        painter.text(Pos2::new(card.max.x - 20.0, y), Align2::RIGHT_CENTER, format!("{}/4", room.players.len()), FontId::monospace(13.0), theme.text_muted);
        y += 36.0;
        for (k, (_, name, color)) in room.players.iter().enumerate() {
            let c = Pos2::new(left + 16.0, y);
            paint_colleague(&painter, c, 0.9, COLORS[*color as usize % 4], 0.0, (now * 2.0 + k as f64).sin() as f32 * 0.3, k % 4, now);
            let label = if *name == room.host { format!("{name}  ·  {}", t.monday_host_tag) } else { name.clone() };
            painter.text(Pos2::new(left + 44.0, y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), if name == pseudo { theme.accent } else { theme.text });
            y += 40.0;
        }
        for _ in room.players.len()..4 {
            painter.circle_stroke(Pos2::new(left + 16.0, y), 14.0, Stroke::new(1.0, theme.tab_hover));
            painter.text(Pos2::new(left + 44.0, y), Align2::LEFT_CENTER, t.monday_free_seat, FontId::proportional(13.0), theme.text_muted.gamma_multiply(0.6));
            y += 40.0;
        }
        // The perks for this game, among those of the player's grade.
        if let Some(c) = self.monday.online.career.clone() {
            let names: Vec<&str> = t.monday_perk_names.split('|').collect();
            let notes: Vec<&str> = t.monday_perk_notes.split('|').collect();
            let ids = ["rtt", "mutuelle", "anciennete", "prime", "teletravail"];
            y += 2.0;
            if c.slots == 0 {
                painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, t.monday_no_perks, FontId::proportional(12.0), theme.text_muted);
            } else {
                painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, t.monday_perks.replace("{n}", &c.slots.to_string()), FontId::proportional(12.5), theme.text);
                let picked = self.monday.perks.get_or_insert_with(|| self.monday.online.perks.clone()).clone();
                let mut x = left;
                let mut changed = None;
                for perk in &c.perks {
                    let k = ids.iter().position(|i| i == perk).unwrap_or(0);
                    let on = picked.contains(perk);
                    let label = names.get(k).copied().unwrap_or(perk.as_str());
                    let w = 14.0 + label.len() as f32 * 7.0;
                    if x + w > card.max.x - 20.0 {
                        break;
                    }
                    let r = Rect::from_min_size(Pos2::new(x, y + 12.0), Vec2::new(w, 24.0));
                    let b = egui::Button::new(egui::RichText::new(label).size(12.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.bg }).corner_radius(12.0);
                    if ui.put(r, b).on_hover_text(notes.get(k).copied().unwrap_or_default()).clicked() {
                        let mut list = picked.clone();
                        if on {
                            list.retain(|p| p != perk);
                        } else if (list.len() as u32) < c.slots {
                            list.push(perk.clone());
                        }
                        changed = Some(list);
                    }
                    x += w + 6.0;
                }
                if let Some(list) = changed {
                    self.monday.online.choose_perks(&list);
                    self.monday.perks = Some(list);
                }
            }
        }
        let bottom = card.max.y - 36.0;
        if room.host == pseudo {
            let r = Rect::from_min_size(Pos2::new(left, bottom - 17.0), Vec2::new(170.0, 34.0));
            let b = egui::Button::new(egui::RichText::new(t.monday_start).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
            if ui.put(r, b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.monday.online.start();
            }
        } else {
            painter.text(Pos2::new(left, bottom), Align2::LEFT_CENTER, t.monday_wait_host.replace("{n}", &room.host), FontId::proportional(13.0), theme.text_muted);
        }
        let r = Rect::from_min_size(Pos2::new(card.max.x - 140.0, bottom - 15.0), Vec2::new(120.0, 30.0));
        if ui.put(r, egui::Button::new(egui::RichText::new(t.monday_leave_room).size(12.5)).corner_radius(8.0)).clicked() {
            self.monday.online.leave();
        }
        let chat_y = card.max.y + 12.0;

        // Colleagues online, to invite.
        let area = if wide { Rect::from_min_max(Pos2::new(card.max.x + 32.0, body.min.y), Pos2::new(x0 + width, body.min.y + 320.0f32.min(body.height()))) } else { Rect::from_min_max(Pos2::new(x0, chat_y + 8.0), Pos2::new(x0 + width, (chat_y + 260.0).min(body.max.y))) };
        if area.height() > 80.0 && room.players.len() < 4 {
            painter.rect_filled(area, 16.0, theme.chrome_bg.gamma_multiply(0.94));
            painter.rect_stroke(area, 16.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
            painter.text(Pos2::new(area.min.x + 18.0, area.min.y + 22.0), Align2::LEFT_CENTER, t.monday_invite, FontId::proportional(14.0), theme.text);
            let inner = Rect::from_min_max(Pos2::new(area.min.x + 14.0, area.min.y + 42.0), Pos2::new(area.max.x - 14.0, area.max.y - 12.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
            let live = &self.versus;
            let faces = super::frames::Faces { avatars: &self.avatars, account: self.config.settings.floor.as_ref() };
            let mut invite = None;
            if live.online.is_empty() {
                child.label(egui::RichText::new(t.four_nobody).size(12.5).color(theme.text_muted));
            }
            let pending = live.sent.as_ref().filter(|s| s.status == "pending");
            egui::ScrollArea::vertical().max_height(inner.height() - 30.0).auto_shrink([false, true]).show(&mut child, |ui| {
                for player in live.online.iter().filter(|p| !room.players.iter().any(|(_, n, _)| *n == p.name)) {
                    ui.horizontal(|ui| {
                        ui.set_min_height(28.0);
                        super::frames::row_avatar(ui, 9.0, &player.name, player.id.as_deref(), player.avatar.as_deref(), player.frame.as_deref(), &faces, &theme);
                        ui.label(egui::RichText::new(&player.name).size(13.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(pending.is_none(), egui::Button::new(egui::RichText::new(t.four_invite).size(12.0)).corner_radius(6.0)).clicked() {
                                invite = Some(player.name.clone());
                            }
                        });
                    });
                }
            });
            if let Some(sent) = pending {
                child.label(egui::RichText::new(t.four_sent.replace("{n}", sent.to.as_deref().unwrap_or(t.game_anonymous))).size(12.0).color(theme.text_muted));
            }
            if let Some(name) = invite {
                self.versus.invite_to(&name, "lundi-matin");
            }
        }
    }

    /// The game: the office as big as the page lets it, everyone in it, the counters around.
    fn monday_game(&mut self, ui: &mut Ui, rect: Rect, now: f64, pseudo: &str) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(frame) = self.monday.online.frame.clone() else { return };
        self.monday.bleed(&frame, now);
        // The kinds met, for the bestiary.
        let met = frame.zombies.iter().filter_map(|z| BESTIARY.iter().position(|b| b.0 == z.kind)).fold(0u32, |bits, k| bits | 1 << k);
        if met & !self.config.settings.monday_seen != 0 {
            self.config.settings.monday_seen |= met;
            self.save_config();
        }
        let online = &self.monday.online;
        let Some(map) = online.floor_map(frame.floor).cloned() else { return };
        let me = online.me(pseudo);
        let colors: Vec<(u32, String, Color32)> = online.room.as_ref().map(|r| r.players.iter().map(|(id, n, c)| (*id, n.clone(), COLORS[*c as usize % 4])).collect()).unwrap_or_default();
        // Between the last two frames: everyone moves smoothly.
        let mix = frame_mix(online.before.as_ref(), &frame);

        let room = Vec2::new(rect.width() - 24.0, rect.height() - 24.0);
        let scale = (room.x / map.w).min(room.y / map.h).max(0.2);
        let size = Vec2::new(map.w, map.h) * scale;
        let field = Rect::from_center_size(rect.center(), size);
        let at = |x: f32, y: f32| field.min + Vec2::new(x, y) * scale;
        let painter = ui.painter_at(field);

        // The floor of the tower: its ground, walls, doors and furniture; the reception's door.
        let boss = frame.zombies.iter().any(|z| z.kind == Kind::Board);
        super::zombie_floors::paint_floor(&painter, &map, &at, scale, now, boss);
        // The reception's door, Agnès' own (no zombie comes in there); her way to the coffee while she
        // walks it.
        if let Some(door) = &map.reception {
            let r = Rect::from_center_size(at(door.x, door.y.clamp(map.wall / 2.0, map.h - map.wall / 2.0)), Vec2::new(door.span, map.wall) * scale);
            painter.rect_filled(r, 2.0, AGNES.gamma_multiply(0.8));
            painter.text(r.center_top() - Vec2::new(0.0, 10.0 * scale), Align2::CENTER_CENTER, t.monday_reception, FontId::monospace(10.0 * scale.max(0.8)), AGNES);
            if let Some(a) = frame.agnes.filter(|a| a.state != AgnesState::Served) {
                let mut way = vec![Pos2::new(door.x, door.y - 30.0)];
                way.extend(map.agnes_path.iter().map(|s| Pos2::new(s.x, s.y)));
                // From the leg she's on: the first point still ahead of her.
                let leg = (0..way.len() - 1)
                    .min_by(|&i, &j| seg_dist(way[i], way[i + 1], Pos2::new(a.x, a.y)).total_cmp(&seg_dist(way[j], way[j + 1], Pos2::new(a.x, a.y))))
                    .unwrap_or(0);
                let mut points = vec![Pos2::new(a.x, a.y)];
                points.extend(way[leg + 1..].iter().copied());
                for pair in points.windows(2) {
                    let (from, to) = (pair[0], pair[1]);
                    let n = ((to - from).length() / 24.0) as usize;
                    for k in 0..n {
                        let q = from + (to - from) * ((k as f32 + 0.5) / n.max(1) as f32);
                        painter.circle_filled(at(q.x, q.y), 2.5 * scale, AGNES.gamma_multiply(0.35));
                    }
                }
            }
        }
        super::zombie_floors::coffee_machine(&painter, at(map.coffee.x, map.coffee.y), scale, now, frame.wave_on);

        // Blood on the floor: fresh and shining, then drying darker, then gone.
        for &(x, y, since, seed, size) in &self.monday.stains {
            let age = (now - since) as f32;
            let grow = (age * 6.0).min(1.0);
            let alpha = 0.9 * (1.0 - ((age - (STAIN_LIFE as f32 - 6.0)) / 6.0).clamp(0.0, 1.0));
            super::monday_theme::splat(&painter, at(x, y), size * scale * (0.6 + 0.4 * grow), seed, alpha * (1.0 - 0.35 * (age / 10.0).min(1.0)));
        }
        // The zombies (the meeting-itis' aura under everyone), the shots, the colleagues.
        for z in &frame.zombies {
            if z.kind == Kind::Meeting {
                let (x, y) = mix.zombie(z.id, z.x, z.y);
                painter.circle_filled(at(x, y), 90.0 * scale, ZOMBIE.gamma_multiply(0.1));
            }
        }
        for z in &frame.zombies {
            let (x, y) = mix.zombie(z.id, z.x, z.y);
            paint_zombie(&painter, at(x, y), scale, z.kind, z.heading, z.id, (now * 4.0) as f32 + z.id as f32, z.life, Fx { slowed: z.slowed, charging: z.charging || z.winding, frozen: z.frozen, healing: z.healing, rebooting: z.rebooting });
        }
        for s in &frame.shots {
            paint_shot(&painter, at(s.x, s.y), scale, s.angle, s.weapon, now);
        }
        for &(x, y, aim, left) in &frame.turrets {
            paint_turret(&painter, at(x, y), scale, aim, left);
        }
        for &(x, y) in &frame.letters {
            paint_letter(&painter, at(x, y), scale, now);
        }
        let short: Vec<&str> = t.monday_weapons_short.split('|').collect();
        for &(x, y, weapon, left) in &frame.drops {
            paint_drop(&painter, at(x, y), scale, weapon, left, now, short.get(weapon).copied().unwrap_or_default());
        }
        for &(x, y, left) in &frame.kits {
            paint_kit(&painter, at(x, y), scale, left, now);
        }
        if let Some((x, y, met)) = frame.gerard {
            paint_gerard(&painter, at(x, y), scale, now, met);
        }
        if let Some(a) = frame.agnes {
            let lines: Vec<&str> = t.monday_agnes_lines.split('|').collect();
            let line = lines[((now / 4.0) as usize) % lines.len().max(1)].replace("{n}", &(frame.wave + 1).to_string());
            paint_agnes(&painter, at(a.x, a.y), scale, now, a, &line, frame.wave >= 10 && frame.gerard.is_some_and(|g| g.2));
        }
        for p in &frame.players {
            let (x, y) = mix.player(p.id, p.x, p.y);
            let (name, color) = colors.iter().find(|(id, _, _)| *id == p.id).map(|(_, n, c)| (n.as_str(), *c)).unwrap_or(("?", COLORS[0]));
            let c = at(x, y);
            match p.state {
                State::Up => {
                    let firing = p.reload <= 0.0 && Some(p.id) != me && (now * 30.0 + p.id as f64).sin() > 0.95;
                    paint_colleague(&painter, c, scale, color, p.aim, 0.0, p.weapon, now);
                    if firing {
                        painter.circle_filled(c + Vec2::angled(p.aim) * 42.0 * scale, 5.0 * scale, Color32::from_rgb(255, 233, 168));
                    }
                    let bar = Rect::from_min_size(c + Vec2::new(-22.0, 26.0) * scale, Vec2::new(44.0, 5.0) * scale);
                    painter.rect_filled(bar, 0.0, Color32::from_black_alpha(150));
                    painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * (p.hp / 100.0).clamp(0.0, 1.0), bar.height())), 0.0, if p.hp > 60.0 { theme.ansi[2] } else { theme.accent });
                }
                State::Down => {
                    paint_down(&painter, c, scale, color, p.revive / REVIVE, &theme);
                    painter.text(c + Vec2::new(0.0, 50.0 * scale), Align2::CENTER_CENTER, format!("{} s", p.bleed.ceil() as u32), FontId::monospace(12.0 * scale.max(0.7)), theme.ansi[1]);
                }
                State::Dead => {
                    painter.circle_stroke(c, 18.0 * scale, Stroke::new(1.5, color.gamma_multiply(0.35)));
                }
            }
            let label = if p.offline { format!("{name} ({})", t.monday_offline) } else { name.to_owned() };
            painter.text(c - Vec2::new(0.0, 34.0 * scale), Align2::CENTER_CENTER, label, FontId::proportional(13.0 * scale.max(0.7)), if p.offline { theme.text_muted } else { Color32::WHITE });
        }
        super::zombie_floors::paint_shade(&painter, field, map.id == "roof");
        for (text, x, y, color, since) in &self.monday.floats {
            let k = ((now - since) / 0.9) as f32;
            painter.text(at(*x, *y) - Vec2::new(0.0, 20.0 + 30.0 * k) * scale, Align2::CENTER_CENTER, text, FontId::proportional(14.0 * scale.max(0.7)), color.gamma_multiply(1.0 - k));
        }

        // The player: the keys and the mouse; the crosshair.
        let mine = me.and_then(|id| frame.players.iter().find(|p| p.id == id)).copied();
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| field.contains(*p));
        // Escape: the menu (alone, the game pauses meanwhile). Back paused (the page left alone): the
        // menu too.
        let solo = self.monday.online.room.as_ref().is_some_and(|r| r.players.len() == 1);
        let escape = self.monday.over.is_none() && ui.input(|i| i.key_pressed(Key::Escape));
        if escape {
            self.monday.menu = !self.monday.menu;
            if solo {
                self.monday.online.pause(self.monday.menu);
            }
        } else if frame.paused && solo && !self.monday.menu {
            self.monday.menu = true;
        }
        if self.monday.menu {
            if self.monday.sent.is_some_and(|(last, _)| last.0 != 0 || last.1 != 0 || last.3) {
                self.monday.online.input(0.0, 0.0, mine.map_or(0.0, |p| p.aim), false);
                self.monday.sent = Some(((0, 0, 0, false), now));
            }
        } else if let Some(p) = mine {
            let (x, y) = mix.player(p.id, p.x, p.y);
            self.monday_controls(ui, now, at(x, y), pointer, field, &frame);
        }
        if let Some(p) = pointer {
            let s = Stroke::new(2.0, Color32::from_white_alpha(210));
            painter.circle_stroke(p, 12.0, s);
            for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                painter.line_segment([p + d * 6.0, p + d * 18.0], s);
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }

        // The counters: the wave, the team's tickets, the weapon, the team.
        let hud = ui.painter_at(rect);
        let panel = |r: Rect| hud.rect_filled(r, 10.0, Color32::from_rgba_unmultiplied(18, 20, 26, 210));
        let top = Rect::from_min_size(field.min + Vec2::new(12.0, 12.0), Vec2::new(300.0, 58.0));
        panel(top);
        let wave = if frame.wave_on { format!("{} / {}", t.monday_wave.replace("{n}", &frame.wave.to_string()), crate::zombie::LAST_WAVE) } else { t.monday_break_title.to_owned() };
        let floors: Vec<&str> = t.monday_floor_short.split('|').collect();
        hud.text(top.right_top() + Vec2::new(-12.0, 14.0), Align2::RIGHT_CENTER, floors.get(frame.floor).copied().unwrap_or_default().to_uppercase(), FontId::monospace(10.5), theme.text_muted);
        paint_metal(&hud, top.min + Vec2::new(14.0, 20.0), Align2::LEFT_CENTER, &wave, 20.0, theme.accent, 1.0);
        let sub = if frame.wave_on { t.monday_left.replace("{n}", &frame.left.to_string()) } else { t.monday_next.replace("{n}", &(frame.wave + 1).to_string()).replace("{s}", &frame.break_left.to_string()) };
        hud.text(top.min + Vec2::new(14.0, 44.0), Align2::LEFT_CENTER, sub, FontId::proportional(13.0), Color32::WHITE);
        let tickets = Rect::from_min_size(Pos2::new(field.max.x - 182.0, field.min.y + 12.0), Vec2::new(170.0, 40.0));
        panel(tickets);
        hud.text(tickets.left_center() + Vec2::new(14.0, 0.0), Align2::LEFT_CENTER, t.monday_tickets, FontId::monospace(11.0), theme.text_muted);
        hud.text(tickets.right_center() - Vec2::new(14.0, 0.0), Align2::RIGHT_CENTER, frame.tickets.to_string(), FontId::monospace(20.0), theme.accent);
        if let Some(p) = mine {
            // The four weapons, side by side: the key, the name, the ammo, the upgrades (damage on top,
            // magazine under); the one in hand lit, those not had yet dimmed.
            let short: Vec<&str> = t.monday_weapons_short.split('|').collect();
            let (slot_w, gap) = (96.0, 6.0);
            let gun = Rect::from_min_size(Pos2::new(field.min.x + 12.0, field.max.y - 104.0), Vec2::new(20.0 + 4.0 * slot_w + 3.0 * gap, 92.0));
            panel(gun);
            for (k, &tint) in WEAPON_COLORS.iter().enumerate() {
                let slot = Rect::from_min_size(gun.min + Vec2::new(10.0 + k as f32 * (slot_w + gap), 10.0), Vec2::new(slot_w, 52.0));
                let owned = p.owned & (1 << k) != 0;
                let held = p.weapon == k;
                hud.rect_filled(slot, 6.0, if held { tint.gamma_multiply(0.22) } else { Color32::from_rgb(30, 34, 44) });
                if held {
                    hud.rect_stroke(slot, 6.0, Stroke::new(2.0, tint), egui::StrokeKind::Inside);
                }
                let ink = if owned { Color32::WHITE } else { Color32::from_white_alpha(60) };
                hud.text(slot.min + Vec2::new(7.0, 9.0), Align2::LEFT_CENTER, (k + 1).to_string(), FontId::monospace(10.0), if owned { theme.text_muted } else { ink });
                hud.circle_filled(slot.right_top() + Vec2::new(-9.0, 9.0), 4.0, if owned { tint } else { tint.gamma_multiply(0.25) });
                hud.text(slot.min + Vec2::new(7.0, 24.0), Align2::LEFT_CENTER, short.get(k).copied().unwrap_or_default(), FontId::proportional(12.5), ink);
                if owned {
                    let ammo = if held && p.reload > 0.0 { t.monday_reload.to_owned() } else { p.all_ammo[k].to_string() };
                    hud.text(slot.min + Vec2::new(7.0, 42.0), Align2::LEFT_CENTER, ammo, FontId::monospace(if held && p.reload > 0.0 { 10.0 } else { 13.0 }), if held { theme.accent } else { theme.text_muted });
                    let (dmg, mag) = p.all_ups[k];
                    for (row, level, color) in [(0.0, dmg, Color32::from_rgb(255, 120, 90)), (7.0, mag, Color32::from_rgb(110, 180, 255))] {
                        for i in 0..3 {
                            let c = Pos2::new(slot.max.x - 30.0 + i as f32 * 9.0, slot.min.y + 38.0 + row);
                            hud.circle_filled(c, 3.0, if (i as u8) < level { color } else { Color32::from_white_alpha(28) });
                        }
                    }
                } else {
                    hud.text(slot.min + Vec2::new(7.0, 42.0), Align2::LEFT_CENTER, "—", FontId::monospace(13.0), ink);
                }
            }
            let gun = Rect::from_min_size(gun.min + Vec2::new(0.0, 34.0), Vec2::new(gun.width(), 58.0));
            let state = match p.state {
                State::Up if !frame.wave_on => if p.ready { t.monday_ready_done.to_owned() } else { t.monday_ready.to_owned() },
                State::Up => t.monday_keys.to_owned(),
                State::Down => t.monday_down.replace("{s}", &(p.bleed.ceil() as u32).to_string()),
                State::Dead => t.monday_dead.to_owned(),
            };
            let color = if p.state == State::Up { theme.text_muted } else { theme.ansi[1] };
            hud.text(gun.min + Vec2::new(14.0, 42.0), Align2::LEFT_CENTER, state, FontId::proportional(12.0), color);
        }
        let team = Rect::from_min_size(Pos2::new(field.max.x - 222.0, field.max.y - 18.0 - frame.players.len() as f32 * 26.0), Vec2::new(210.0, 8.0 + frame.players.len() as f32 * 26.0));
        panel(team);
        for (k, p) in frame.players.iter().enumerate() {
            let y = team.min.y + 17.0 + k as f32 * 26.0;
            let (name, color) = colors.iter().find(|(id, _, _)| *id == p.id).map(|(_, n, c)| (n.as_str(), *c)).unwrap_or(("?", COLORS[0]));
            hud.circle_filled(Pos2::new(team.min.x + 16.0, y), 6.0, color);
            hud.text(Pos2::new(team.min.x + 30.0, y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), Color32::WHITE);
            let bar = Rect::from_min_size(Pos2::new(team.max.x - 86.0, y - 4.0), Vec2::new(72.0, 8.0));
            hud.rect_filled(bar, 2.0, Color32::from_rgb(44, 49, 64));
            match p.state {
                State::Up => {
                    hud.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * (p.hp / 100.0).clamp(0.0, 1.0), bar.height())), 2.0, if p.hp > 60.0 { theme.ansi[2] } else { theme.accent });
                }
                State::Down => {
                    hud.text(bar.center(), Align2::CENTER_CENTER, "K.O.", FontId::monospace(11.0), theme.ansi[1]);
                }
                State::Dead => {
                    hud.text(bar.center(), Align2::CENTER_CENTER, "…", FontId::monospace(11.0), theme.text_muted);
                }
            }
        }
        // The coffee machine, during the break: what the team's tickets buy.
        if let (false, Some(p)) = (frame.wave_on, mine.filter(|p| p.state == State::Up)) {
            self.monday_shop(ui, field, &frame, p);
        }
        // The perks' keys, and when they're back.
        if let Some(p) = mine {
            let mut y = field.max.y - 120.0;
            for (perk, key, left) in [("rtt", t.monday_dash, p.dash_in), ("teletravail", t.monday_tp, p.teleport_in)] {
                if self.monday.online.perks.iter().any(|q| q == perk) {
                    let text = if left > 0 { format!("{key}  {left} s") } else { key.to_owned() };
                    hud.text(Pos2::new(field.min.x + 18.0, y), Align2::LEFT_CENTER, text, FontId::proportional(12.0), if left > 0 { theme.text_muted } else { theme.accent });
                    y -= 18.0;
                }
            }
        }
        if let Some((text, color, since)) = &self.monday.banner {
            let k = now - since;
            if k < 2.5 {
                let alpha = (1.0 - ((k - 2.0).max(0.0) / 0.5)) as f32;
                paint_metal(&hud, Pos2::new(field.center().x, field.min.y + 110.0), Align2::CENTER_CENTER, text, 30.0, color.gamma_multiply(alpha), alpha);
            }
        }
        if let Status::Reconnecting(_) = self.monday.online.status {
            paint_pill(&hud, Pos2::new(field.center().x, field.min.y + 30.0), t.monday_reconnecting, theme.accent, &theme);
        }
        if self.monday.menu {
            self.monday_menu(ui, field, solo);
        }
    }

    /// The menu of Escape: resume, or quit the game (alone it ends there and counts; with colleagues,
    /// the game goes on without the player).
    fn monday_menu(&mut self, ui: &mut Ui, field: Rect, solo: bool) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter_at(field);
        painter.rect_filled(field, 0.0, Color32::from_black_alpha(150));
        let card = Rect::from_center_size(field.center(), Vec2::new(320.0, if solo { 190.0 } else { 220.0 }));
        painter.rect(card, 14.0, theme.chrome_bg, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
        paint_metal(&painter, card.center_top() + Vec2::new(0.0, 38.0), Align2::CENTER_CENTER, t.monday_paused, 28.0, theme.accent, 1.0);
        let mut y = card.min.y + 70.0;
        if !solo {
            painter.text(Pos2::new(card.center().x, y + 8.0), Align2::CENTER_CENTER, t.monday_pause_multi, FontId::proportional(12.5), theme.text_muted);
            y += 30.0;
        }
        let resume = Rect::from_center_size(Pos2::new(card.center().x, y + 20.0), Vec2::new(200.0, 34.0));
        let b = egui::Button::new(egui::RichText::new(t.monday_resume).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
        if ui.put(resume, b).clicked() {
            self.monday.menu = false;
            if solo {
                self.monday.online.pause(false);
            }
        }
        let quit = Rect::from_center_size(Pos2::new(card.center().x, y + 66.0), Vec2::new(200.0, 34.0));
        if ui.put(quit, egui::Button::new(egui::RichText::new(t.monday_quit).size(14.0).color(theme.ansi[1])).corner_radius(8.0)).clicked() {
            self.monday.menu = false;
            self.monday.online.quit();
        }
    }

    /// The keys and the mouse, to floor: moving, aiming at the pointer, firing, a weapon, reloading,
    /// ready, the perks (Shift: dash, T: back to the coffee).
    fn monday_controls(&mut self, ui: &Ui, now: f64, me: Pos2, pointer: Option<Pos2>, field: Rect, frame: &Frame) {
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let (mx, my, fire, pick, reload, ready, shift, tp) = ui.input(|i| {
            let down = |keys: &[Key]| free && keys.iter().any(|k| i.key_down(*k));
            let mx = f32::from(down(&[Key::D, Key::ArrowRight])) - f32::from(down(&[Key::Q, Key::A, Key::ArrowLeft]));
            let my = f32::from(down(&[Key::S, Key::ArrowDown])) - f32::from(down(&[Key::Z, Key::W, Key::ArrowUp]));
            let fire = i.pointer.primary_down() && i.pointer.hover_pos().is_some_and(|p| field.contains(p));
            let pick = if free { i.events.iter().find_map(|e| super::digit_key(e, 4)) } else { None };
            (mx, my, fire, pick, free && i.key_pressed(Key::R), free && i.key_pressed(Key::Space), free && i.modifiers.shift, free && i.key_pressed(Key::T))
        });
        let aim = pointer.map_or(0.0, |p| (p - me).angle());
        // The aim in steps, so that a still mouse doesn't send anything new.
        let input = (mx as i8, my as i8, (aim * 200.0) as i32, fire);
        let due = self.monday.sent.is_none_or(|(last, at)| last != input || now - at > INPUT_EVERY);
        if due {
            self.monday.online.input(mx, my, aim, fire);
            self.monday.sent = Some((input, now));
        }
        if let Some(k) = pick {
            self.monday.online.pick(k);
        }
        if reload {
            self.monday.online.reload();
        }
        if ready && !frame.wave_on {
            self.monday.ready = !self.monday.ready;
            self.monday.online.ready(self.monday.ready);
        }
        // Shift pressed (not held): a dash.
        if shift && !self.monday.shift_was {
            self.monday.online.dash();
        }
        self.monday.shift_was = shift;
        if tp {
            self.monday.online.teleport();
        }
    }

    /// The game just over: how far the team went, the player's kills and revives.
    fn monday_over(&mut self, ui: &mut Ui, rect: Rect, over: &Over) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(140));
        let lines: Vec<(String, String)> = over.milestones.iter().map(|m| self.milestone_text(m)).collect();
        let card = Rect::from_center_size(rect.center(), Vec2::new(440.0, 250.0 + lines.len() as f32 * 22.0 + if over.won { 30.0 } else { 0.0 }));
        let gold = Color32::from_rgb(222, 180, 80);
        let accent = if over.won { gold } else { theme.accent };
        painter.rect(card, 14.0, theme.chrome_bg, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
        paint_metal(&painter, card.center_top() + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, if over.won { t.monday_won } else { t.monday_over }, 28.0, accent, 1.0);
        let body = t.monday_over_body.replace("{w}", &over.wave.to_string()).replace("{k}", &over.kills.to_string()).replace("{r}", &over.revives.to_string());
        let mut y = card.min.y + 88.0;
        if over.won {
            let won = painter.layout(t.monday_over_won.to_owned(), FontId::proportional(13.5), gold, card.width() - 40.0);
            let h = won.size().y;
            painter.galley(Pos2::new(card.center().x - won.size().x / 2.0, y - 10.0), won, gold);
            y += h + 6.0;
        }
        painter.text(Pos2::new(card.center().x, y), Align2::CENTER_CENTER, body, FontId::proportional(15.0), theme.text);
        y += 28.0;
        if over.xp > 0 {
            painter.text(Pos2::new(card.center().x, y), Align2::CENTER_CENTER, t.monday_xp_gained.replace("{n}", &over.xp.to_string()), FontId::monospace(15.0), theme.accent);
            y += 26.0;
        }
        if over.record {
            painter.text(Pos2::new(card.center().x, y), Align2::CENTER_CENTER, t.game_new_best, FontId::proportional(14.0), theme.accent);
            y += 24.0;
        }
        for (title, _) in &lines {
            painter.text(Pos2::new(card.center().x, y), Align2::CENTER_CENTER, format!("★ {title}"), FontId::proportional(13.5), AGNES);
            y += 22.0;
        }
        let r = Rect::from_center_size(card.center_bottom() - Vec2::new(0.0, 36.0), Vec2::new(170.0, 34.0));
        let b = egui::Button::new(egui::RichText::new(t.monday_ok).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
        if ui.put(r, b).clicked() || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape)) {
            self.monday.over = None;
        }
    }

    /// A step passed, in words: its title and a line under it.
    fn milestone_text(&self, m: &Milestone) -> (String, String) {
        let t = self.t();
        let weapons: Vec<&str> = t.monday_weapons.split('|').collect();
        match m {
            Milestone::Grade { value, stars } => {
                let k = crate::zombie::GRADES.iter().position(|g| g == value).unwrap_or(0) as u32;
                (t.monday_ms_grade.replace("{g}", &grade_name(t, k, *stars)), t.monday_name.to_owned())
            }
            Milestone::Mastery { weapon, level } => {
                let k = crate::zombie::WEAPONS.iter().position(|w| w == weapon).unwrap_or(0);
                (t.monday_ms_mastery.replace("{w}", weapons.get(k).copied().unwrap_or_default()).replace("{n}", &level.to_string()), t.monday_name.to_owned())
            }
            Milestone::Wave { value } => (t.monday_ms_wave.replace("{n}", &value.to_string()), t.monday_name.to_owned()),
            Milestone::Revives { value } => (t.monday_ms_revives.replace("{n}", &value.to_string()), t.unlock_bite.to_owned()),
            Milestone::Escorts { value } => (t.monday_ms_escorts.replace("{n}", &value.to_string()), t.unlock_agnes_card.to_owned()),
            Milestone::Letter { value } => (t.monday_ms_letter.replace("{n}", &value.to_string()), t.unlock_letters.to_owned()),
            Milestone::Horde => (t.monday_ms_horde.to_owned(), t.unlock_horde.to_owned()),
            Milestone::Weekly { .. } => (t.monday_ms_weekly.to_owned(), t.monday_name.to_owned()),
            Milestone::Final => (t.monday_ms_final.to_owned(), t.unlock_monday_theme.to_owned()),
            Milestone::Daily { .. } => (t.monday_ms_daily.to_owned(), t.monday_name.to_owned()),
            Milestone::Dailies { value } => (t.monday_ms_dailies.replace("{n}", &value.to_string()), t.unlock_monday_frame.to_owned()),
        }
    }

    /// The career floor sent, kept in the settings too (sealed): the rewards it unlocks show offline,
    /// and travel with the others.
    fn monday_mirror(&mut self) {
        let Some(c) = &self.monday.online.career else { return };
        let s = &mut self.config.settings;
        let (revives, escorts, letters, horde, best) = (c.revives.max(s.monday_revives), c.escorts.max(s.monday_escorts), c.letters.max(s.monday_letters), c.horde || s.monday_horde, c.best_wave.max(s.monday_best));
        let (wins, dailies) = (c.wins.max(s.monday_wins), c.dailies.max(s.monday_dailies));
        if (revives, escorts, letters, horde, best, wins, dailies) != (s.monday_revives, s.monday_escorts, s.monday_letters, s.monday_horde, s.monday_best, s.monday_wins, s.monday_dailies) {
            s.monday_wins = wins;
            s.monday_dailies = dailies;
            s.monday_revives = revives;
            s.monday_escorts = escorts;
            s.monday_letters = letters;
            s.monday_horde = horde;
            s.monday_best = best;
            self.save_config();
        }
    }

    /// The coffee machine, on the right of the field during the break: what the team's tickets buy
    /// (what the player's grade opens), at its price.
    fn monday_shop(&mut self, ui: &mut Ui, field: Rect, frame: &Frame, me: crate::zombie::Player) {
        let t = self.t();
        let theme = self.theme.clone();
        let names: Vec<&str> = t.monday_shop_items.split('|').collect();
        let notes: Vec<&str> = t.monday_shop_notes.split('|').collect();
        let items = ["extinguisher", "confetti", "turret", "mug", "dmg", "mag"];
        let open: Vec<String> = self.monday.online.career.as_ref().map(|c| c.shop.clone()).unwrap_or_else(|| vec!["mug".into(), "dmg".into(), "mag".into()]);
        let panel = Rect::from_min_size(Pos2::new(field.max.x - 262.0, field.min.y + 64.0), Vec2::new(250.0, 40.0 + items.len() as f32 * 46.0));
        let painter = ui.painter_at(field);
        painter.rect_filled(panel, 10.0, Color32::from_rgba_unmultiplied(18, 20, 26, 225));
        painter.rect_stroke(panel, 10.0, Stroke::new(1.0, Color32::from_rgb(255, 181, 71).gamma_multiply(0.6)), egui::StrokeKind::Inside);
        painter.text(panel.min + Vec2::new(14.0, 20.0), Align2::LEFT_CENTER, t.monday_shop, FontId::proportional(14.0), Color32::from_rgb(255, 181, 71));
        let mut buy = None;
        for (k, item) in items.iter().enumerate() {
            let row = Rect::from_min_size(panel.min + Vec2::new(10.0, 38.0 + k as f32 * 46.0), Vec2::new(panel.width() - 20.0, 40.0));
            let weapon = crate::zombie::WEAPONS.iter().position(|w| w == item);
            let owned = weapon.is_some_and(|w| me.owned & (1 << w) != 0);
            let price = self.monday.online.price(item, &me);
            let unlocked = open.iter().any(|o| o == item);
            let (right, can) = if owned {
                (t.monday_owned.to_owned(), false)
            } else if !unlocked {
                let grade = crate::zombie::SHOP_GRADE.iter().find(|(i, _)| i == item).map_or(0, |(_, g)| *g);
                (t.monday_locked_grade.replace("{g}", &grade_name(t, grade, 0)), false)
            } else if let Some(price) = price {
                (format!("{price}"), frame.tickets >= price && !(*item == "mug" && me.hp >= me.max_hp))
            } else {
                (t.monday_maxed.to_owned(), false)
            };
            let resp = ui.interact(row, ui.id().with(("monday-shop", k)), if can { Sense::click() } else { Sense::hover() });
            let hot = can && resp.hovered();
            painter.rect_filled(row, 6.0, if hot { Color32::from_rgb(60, 52, 40) } else { Color32::from_rgb(28, 30, 38) });
            let alpha = if can || owned { 1.0 } else { 0.45 };
            painter.text(row.min + Vec2::new(10.0, 13.0), Align2::LEFT_CENTER, names.get(k).copied().unwrap_or_default(), FontId::proportional(13.0), Color32::WHITE.gamma_multiply(alpha));
            painter.text(row.min + Vec2::new(10.0, 29.0), Align2::LEFT_CENTER, notes.get(k).copied().unwrap_or_default(), FontId::proportional(10.5), theme.text_muted.gamma_multiply(alpha));
            painter.text(row.right_center() - Vec2::new(10.0, 0.0), Align2::RIGHT_CENTER, right, FontId::monospace(12.0), Color32::from_rgb(255, 181, 71).gamma_multiply(alpha));
            if can && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                buy = Some(*item);
            }
        }
        if let Some(item) = buy {
            self.monday.online.buy(item);
        }
    }

    /// Agnès' letters to Gérard, those found (the others, still to find).
    /// A zombie of the bestiary, opened: big, its name and wave, its life, its bite, its pace, what it
    /// brings, what it does, its story.
    fn monday_beast(&mut self, ctx: &egui::Context, k: usize, now: f64) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(&(kind, from, boss)) = BESTIARY.get(k) else {
            self.monday.beast = None;
            return;
        };
        let name = t.monday_bestiary.split('|').nth(k).unwrap_or_default();
        let note = t.monday_bestiary_notes.split('|').nth(k).unwrap_or_default();
        let lore = t.monday_bestiary_lore.split('|').nth(k).unwrap_or_default();
        let labels: Vec<&str> = t.monday_stat_names.split('|').collect();
        let tint = if boss { theme.ansi[1] } else { theme.accent };
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).stroke(Stroke::new(1.5, tint)).corner_radius(14.0);
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("monday-beast")).frame(frame).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.horizontal_top(|ui| {
                // The zombie, big, on its tile.
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(170.0), Sense::hover());
                let p = ui.painter_at(tile);
                p.rect_filled(tile, 12.0, theme.bg);
                let scale = (52.0 / kind.radius()).min(2.6);
                let cycle = (now * 0.5) % 3.0 < 1.0;
                let fx = Fx { frozen: kind == Kind::Visio && cycle, healing: kind == Kind::Coach, rebooting: kind == Kind::It && cycle, ..Fx::default() };
                paint_zombie(&p, tile.center() - Vec2::new(0.0, 12.0), scale, kind, std::f32::consts::FRAC_PI_2, k as u32 * 7 + 3, (now * 3.0) as f32, 1.0, fx);
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(name).size(22.0).strong().color(theme.text));
                    let wave = t.monday_wave_short.replace("{n}", &from.to_string());
                    let tag = if boss { format!("{}  ·  {wave}", t.monday_boss_tag) } else { wave };
                    ui.label(egui::RichText::new(tag).size(12.0).color(tint));
                    ui.add_space(12.0);
                    // Its numbers, each with a bar against the strongest.
                    let stats = kind_stats(kind);
                    let tops = [900.0, 25.0, 125.0, 400.0];
                    for (i, value) in stats.into_iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_sized(Vec2::new(92.0, 18.0), egui::Label::new(egui::RichText::new(labels.get(i).copied().unwrap_or_default()).size(12.5).color(theme.text_muted)));
                            let (bar, _) = ui.allocate_exact_size(Vec2::new(150.0, 18.0), Sense::hover());
                            let line = Rect::from_center_size(bar.center(), Vec2::new(bar.width(), 6.0));
                            ui.painter().rect_filled(line, 3.0, theme.bg);
                            let k = (value / tops[i]).sqrt().clamp(0.04, 1.0);
                            ui.painter().rect_filled(Rect::from_min_size(line.min, Vec2::new(line.width() * k, line.height())), 3.0, tint);
                            ui.label(egui::RichText::new(format!("{value}")).size(12.5).monospace().color(theme.text));
                        });
                    }
                });
            });
            ui.add_space(16.0);
            ui.label(egui::RichText::new(note).size(14.0).strong().color(tint));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(lore).size(13.0).italics().color(theme.text_muted));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.monday_stat_hint).size(11.0).color(theme.text_muted.gamma_multiply(0.7)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(egui::RichText::new(t.close).size(13.0)).clicked();
                });
            });
        });
        if close || modal.should_close() {
            self.monday.beast = None;
        }
    }

    fn monday_letters(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let theme = self.theme.clone();
        let found = self.config.settings.monday_letters.max(self.monday.online.career.as_ref().map_or(0, |c| c.letters)) as usize;
        let texts = if self.config.settings.language == crate::i18n::Lang::En { &LETTERS_EN } else { &LETTERS_FR };
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg).stroke(Stroke::new(1.5, AGNES));
        let modal = egui::Modal::new(egui::Id::new("monday-letters")).frame(frame).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(egui::RichText::new(t.monday_letters_title).size(18.0).strong().color(AGNES));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (k, text) in texts.iter().enumerate() {
                    if k < found {
                        ui.label(egui::RichText::new(format!("{} {}", k + 1, "✉")).size(12.0).color(theme.text_muted));
                        ui.label(egui::RichText::new(*text).size(13.5).italics());
                    } else {
                        ui.label(egui::RichText::new(t.monday_letter_locked.replace("{n}", &(k + 1).to_string())).size(12.5).color(theme.text_muted.gamma_multiply(0.6)));
                    }
                    ui.add_space(10.0);
                }
            });
        });
        if modal.should_close() {
            self.monday.letters_open = false;
        }
    }
}

/// The wave, as the board shows it.
fn t_wave(n: u32) -> String {
    format!("{n}")
}

/// Where things were at the frame before, to move them smoothly toward the last one.
struct Mix {
    k: f32,
    players: Vec<(u32, f32, f32)>,
    zombies: Vec<(u32, f32, f32)>,
}

impl Mix {
    fn between(&self, list: &[(u32, f32, f32)], id: u32, x: f32, y: f32) -> (f32, f32) {
        match list.iter().find(|(i, _, _)| *i == id) {
            // Far (a respawn, a reconnection): straight there.
            Some(&(_, bx, by)) if (bx - x).hypot(by - y) < 120.0 => (bx + (x - bx) * self.k, by + (y - by) * self.k),
            _ => (x, y),
        }
    }

    fn player(&self, id: u32, x: f32, y: f32) -> (f32, f32) {
        self.between(&self.players, id, x, y)
    }

    fn zombie(&self, id: u32, x: f32, y: f32) -> (f32, f32) {
        self.between(&self.zombies, id, x, y)
    }
}

fn frame_mix(before: Option<&Frame>, frame: &Frame) -> Mix {
    let Some(before) = before else { return Mix { k: 1.0, players: Vec::new(), zombies: Vec::new() } };
    let gap = frame.at.duration_since(before.at).as_secs_f32().max(0.01);
    let k = (frame.at.elapsed().as_secs_f32() / gap).clamp(0.0, 1.0);
    Mix { k, players: before.players.iter().map(|p| (p.id, p.x, p.y)).collect(), zombies: before.zombies.iter().map(|z| (z.id, z.x, z.y)).collect() }
}

fn paint_pill(painter: &egui::Painter, at: Pos2, text: &str, color: Color32, theme: &Theme) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(14.0), color);
    let area = Rect::from_center_size(at, galley.size() + Vec2::new(30.0, 14.0));
    painter.rect_filled(area, area.height() / 2.0, theme.chrome_bg.gamma_multiply(0.92));
    painter.rect_stroke(area, area.height() / 2.0, Stroke::new(1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    painter.galley(area.center() - galley.size() / 2.0, galley, color);
}

/// The weapons' colors (the HUD, the ones on the floor), in WEAPONS' order.
const WEAPON_COLORS: [Color32; 4] = [Color32::from_rgb(180, 188, 204), Color32::from_rgb(255, 224, 102), Color32::from_rgb(232, 72, 60), Color32::from_rgb(255, 122, 182)];
const BLOOD: Color32 = Color32::from_rgb(112, 20, 22);

/// The distance from `p` to the segment a → b.
fn seg_dist(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    let ab = b - a;
    let k = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * k - p).length()
}

/// An oval turned toward `dir`: `along` its half-length that way, `across` its half-width.
fn oval(c: Pos2, dir: Vec2, along: f32, across: f32) -> Vec<Pos2> {
    let side = Vec2::new(-dir.y, dir.x);
    (0..28)
        .map(|k| {
            let a = k as f32 / 28.0 * std::f32::consts::TAU;
            c + dir * a.cos() * along + side * a.sin() * across
        })
        .collect()
}

/// A rectangle turned toward `dir`, centered on `c`: `len` that way, `wide` across.
fn quad(c: Pos2, dir: Vec2, len: f32, wide: f32) -> Vec<Pos2> {
    let side = Vec2::new(-dir.y, dir.x);
    let (a, b) = (dir * len / 2.0, side * wide / 2.0);
    vec![c - a - b, c + a - b, c + a + b, c - a + b]
}

fn fill(painter: &egui::Painter, points: Vec<Pos2>, color: Color32) {
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A head seen from above, facing `dir`: the hair behind (none: bald, a shine on the skull), the
/// face ahead.
fn paint_head(painter: &egui::Painter, c: Pos2, dir: Vec2, r: f32, skin: Color32, hair: Option<Color32>) {
    match hair {
        Some(hair) => {
            painter.circle_filled(c, r, hair);
            fill(painter, oval(c + dir * r * 0.32, dir, r * 0.62, r * 0.8), skin);
        }
        None => {
            painter.circle_filled(c, r, skin);
            painter.circle_filled(c - dir * r * 0.25 + Vec2::new(-dir.y, dir.x) * r * 0.25, r * 0.22, Color32::from_white_alpha(40));
        }
    }
}

/// A zombie seen from above, heading `heading`: arms out ahead, out of step, its office clothes torn
/// and stained (the same stains for the same zombie, from `seed`), its pale skin, yellowed eyes.
/// The employee in shirt and loose tie, the intern in his hoodie with his badge, the meeting-itis
/// bald in his cardigan behind his laptop, the salesman in suit and red tie on his phone, "reply all"
/// in his creased shirt among his emails, its copies paler, the HR director in her burgundy blazer,
/// glasses, the red file. Its life under it when hurt.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_zombie(painter: &egui::Painter, c: Pos2, scale: f32, kind: Kind, heading: f32, seed: u32, phase: f32, life: f32, fx: Fx) {
    let Fx { slowed, charging, frozen, healing, rebooting } = fx;
    let r = kind.radius() * scale;
    // In a video call, frozen: its picture shifted, red and blue apart, a band of noise.
    if kind == Kind::Visio && frozen {
        let jolt = Vec2::new(((phase * 37.0).sin() * 3.0).round(), 0.0) * scale;
        painter.circle_filled(c - Vec2::new(3.0, 0.0) * scale + jolt, r, Color32::from_rgba_unmultiplied(255, 40, 60, 60));
        painter.circle_filled(c + Vec2::new(3.0, 0.0) * scale + jolt, r, Color32::from_rgba_unmultiplied(40, 200, 255, 60));
    }
    // The coach, healing: a green halo breathing around her.
    if kind == Kind::Coach && healing {
        let breath = (phase * 0.8).sin() * 0.5 + 0.5;
        painter.circle_filled(c, r * (3.2 + 0.4 * breath), Color32::from_rgba_unmultiplied(90, 220, 140, 22));
        painter.circle_stroke(c, r * (3.2 + 0.4 * breath), Stroke::new(1.5 * scale, Color32::from_rgba_unmultiplied(90, 220, 140, 90)));
    }
    let dir = Vec2::angled(heading);
    let side = Vec2::new(-dir.y, dir.x);
    let (clothes, hair, skin) = match kind {
        Kind::Base => (Color32::from_rgb(168, 186, 204), Some(Color32::from_rgb(86, 62, 44)), Color32::from_rgb(160, 172, 140)),
        Kind::Intern => (Color32::from_rgb(112, 118, 130), Some(Color32::from_rgb(34, 30, 28)), Color32::from_rgb(170, 182, 146)),
        Kind::Meeting => (Color32::from_rgb(176, 154, 120), None, Color32::from_rgb(166, 170, 138)),
        Kind::Sales => (Color32::from_rgb(42, 52, 82), Some(Color32::from_rgb(20, 20, 24)), Color32::from_rgb(154, 168, 136)),
        Kind::ReplyAll | Kind::Cc => (Color32::from_rgb(214, 204, 156), Some(Color32::from_rgb(140, 110, 70)), Color32::from_rgb(162, 176, 140)),
        Kind::Hr => (Color32::from_rgb(110, 32, 46), Some(Color32::from_rgb(150, 150, 156)), Color32::from_rgb(168, 172, 150)),
        Kind::Board => (Color32::from_rgb(30, 32, 40), Some(Color32::from_rgb(200, 200, 206)), Color32::from_rgb(150, 166, 132)),
        // A shirt for the camera (the pyjama under the desk), the hair as out of bed.
        Kind::Visio => (Color32::from_rgb(120, 168, 214), Some(Color32::from_rgb(60, 44, 36)), Color32::from_rgb(164, 180, 150)),
        Kind::Accountant => (Color32::from_rgb(150, 136, 104), Some(Color32::from_rgb(110, 104, 98)), Color32::from_rgb(158, 168, 140)),
        Kind::Coach => (Color32::from_rgb(40, 200, 170), Some(Color32::from_rgb(230, 190, 90)), Color32::from_rgb(172, 186, 148)),
        Kind::It => (Color32::from_rgb(34, 36, 42), Some(Color32::from_rgb(40, 30, 26)), Color32::from_rgb(160, 174, 144)),
    };
    let ghost = |col: Color32| if kind == Kind::Cc { col.gamma_multiply(0.75) } else { col };
    let sleeve = ghost(clothes.lerp_to_gamma(Color32::BLACK, 0.18));
    let (clothes, skin) = (ghost(clothes), ghost(skin));
    // It shambles: the body swings side to side.
    let c = c + side * phase.sin() * 1.6 * scale;
    let hash = seed.wrapping_mul(2_654_435_761).rotate_left(7) ^ seed.wrapping_mul(40_503);
    let rnd = |k: u32| ((hash >> (k * 5 % 27)) & 31) as f32 / 31.0;

    fill(painter, oval(c + Vec2::new(3.0, 6.0) * scale, dir, r * 0.8, r * 1.1), Color32::from_black_alpha(70));
    // The arms, reaching ahead out of step.
    let arm = r * 0.34;
    let mut hands = [c; 2];
    for (k, s) in [-1.0f32, 1.0].into_iter().enumerate() {
        let reach = r * (1.0 + 0.18 * (phase * 1.3 + s * 1.7).sin());
        let shoulder = c + side * s * r * 0.7;
        let hand = shoulder + dir * reach - side * s * r * 0.1;
        painter.line_segment([shoulder, hand], Stroke::new(arm, sleeve));
        painter.circle_filled(shoulder, arm * 0.5, sleeve);
        painter.circle_filled(hand, arm * 0.55, skin);
        hands[k] = hand;
    }
    // What they carry.
    match kind {
        Kind::Meeting => {
            let at = c + dir * r * 1.05;
            fill(painter, quad(at, dir, r * 0.55, r * 0.95), Color32::from_rgb(52, 56, 64));
            fill(painter, quad(at + dir * r * 0.05, dir, r * 0.32, r * 0.8), Color32::from_rgb(120, 170, 210).gamma_multiply(0.6));
        }
        Kind::Sales => fill(painter, quad(hands[1] + dir * r * 0.15, dir, r * 0.4, r * 0.22), Color32::from_rgb(20, 22, 26)),
        Kind::Hr => fill(painter, quad(hands[1] + dir * r * 0.1 + side * r * 0.12, dir, r * 0.5, r * 0.38), Color32::from_rgb(196, 40, 40)),
        // The briefcase, leather and gold.
        Kind::Board => {
            let at = hands[1] + dir * r * 0.12 + side * r * 0.18;
            fill(painter, quad(at, dir, r * 0.62, r * 0.42), Color32::from_rgb(92, 52, 30));
            fill(painter, quad(at, dir, r * 0.1, r * 0.42), Color32::from_rgb(222, 180, 80));
        }
        // The accountant's binder, held ahead like a shield: green, its white label, its ring.
        Kind::Accountant => {
            let at = c + dir * r * 1.12;
            painter.add(egui::Shape::convex_polygon(quad(at, dir, r * 0.34, r * 1.55), Color32::from_rgb(34, 110, 70), Stroke::new(1.2 * scale, Color32::from_rgb(16, 60, 36))));
            fill(painter, quad(at + dir * r * 0.05, dir, r * 0.2, r * 0.6), Color32::from_rgb(240, 238, 228));
            painter.circle_filled(at - side * r * 0.55, r * 0.09, Color32::from_rgb(200, 200, 206));
        }
        // The coach's green smoothie, its straw.
        Kind::Coach => {
            let cup = hands[1] + dir * r * 0.1;
            painter.circle_filled(cup, r * 0.24, Color32::from_rgb(120, 200, 80));
            painter.circle_stroke(cup, r * 0.24, Stroke::new(1.0 * scale, Color32::from_white_alpha(160)));
            painter.line_segment([cup, cup + dir * r * 0.3 + side * r * 0.12], Stroke::new(1.5 * scale, Color32::from_rgb(255, 100, 140)));
        }
        // The IT guy's laptop, open, its screen lit, a cable trailing.
        Kind::It => {
            let at = c + dir * r * 1.05;
            fill(painter, quad(at, dir, r * 0.5, r * 0.9), Color32::from_rgb(70, 74, 84));
            let screen = if rebooting { Color32::from_rgb(30, 90, 200) } else { Color32::from_rgb(20, 24, 30) };
            fill(painter, quad(at + dir * r * 0.04, dir, r * 0.34, r * 0.76), screen);
            if !rebooting {
                for k in 0..3 {
                    let y = at + dir * r * (0.12 - k as f32 * 0.1) - side * r * 0.25;
                    painter.line_segment([y, y + side * r * (0.2 + 0.25 * ((k as f32 + phase).sin() * 0.5 + 0.5))], Stroke::new(1.2 * scale, Color32::from_rgb(90, 230, 110)));
                }
            }
            painter.line_segment([at - side * r * 0.45, c - dir * r * 0.9 - side * r * 0.9], Stroke::new(1.5 * scale, Color32::from_rgb(60, 120, 220)));
        }
        Kind::ReplyAll | Kind::Cc => {
            for k in 0..3 {
                let a = phase * 0.5 + k as f32 * 2.1;
                let at = c + Vec2::angled(a) * r * 1.45;
                fill(painter, quad(at, Vec2::angled(a * 1.7), r * 0.42, r * 0.3), ghost(Color32::from_rgb(244, 244, 238)));
            }
        }
        _ => {}
    }
    // The torso, in its clothes.
    fill(painter, oval(c, dir, r * 0.56, r * 0.98), clothes);
    match kind {
        Kind::Base => {
            fill(painter, quad(c + dir * r * 0.32, dir, r * 0.42, r * 0.13), Color32::from_rgb(40, 52, 96));
        }
        Kind::Sales => {
            fill(painter, vec![c + dir * r * 0.52 - side * r * 0.24, c + dir * r * 0.05, c + dir * r * 0.52 + side * r * 0.24], Color32::from_rgb(232, 232, 236));
            fill(painter, quad(c + dir * r * 0.34, dir, r * 0.4, r * 0.12), Color32::from_rgb(196, 36, 36));
        }
        Kind::Intern => {
            // The hood behind, the badge on its lanyard.
            painter.circle_filled(c - dir * r * 0.42, r * 0.36, sleeve);
            painter.line_segment([c + side * r * 0.25, c + dir * r * 0.4 + side * r * 0.4], Stroke::new(1.5 * scale, Color32::from_rgb(40, 90, 200)));
            fill(painter, quad(c + dir * r * 0.42 + side * r * 0.42, dir, r * 0.24, r * 0.3), Color32::from_rgb(255, 224, 102));
        }
        Kind::Hr => {
            fill(painter, vec![c + dir * r * 0.5 - side * r * 0.2, c + dir * r * 0.1, c + dir * r * 0.5 + side * r * 0.2], Color32::from_rgb(236, 226, 220));
        }
        // The white shirt, the gold tie, the pocket square.
        Kind::Board => {
            fill(painter, vec![c + dir * r * 0.55 - side * r * 0.26, c + dir * r * 0.02, c + dir * r * 0.55 + side * r * 0.26], Color32::from_rgb(240, 240, 244));
            fill(painter, quad(c + dir * r * 0.3, dir, r * 0.46, r * 0.13), Color32::from_rgb(222, 180, 80));
            fill(painter, quad(c + dir * r * 0.2 - side * r * 0.42, dir, r * 0.16, r * 0.16), Color32::from_rgb(196, 40, 60));
        }
        // Pyjama stripes peeking under the shirt.
        Kind::Visio => {
            for k in 0..3 {
                let at = c - dir * r * (0.3 + k as f32 * 0.18);
                painter.line_segment([at - side * r * 0.45, at + side * r * 0.45], Stroke::new(1.5 * scale, Color32::from_rgb(230, 150, 190)));
            }
        }
        // The cardigan's buttons.
        Kind::Accountant => {
            for k in 0..3 {
                painter.circle_filled(c + dir * r * (0.35 - k as f32 * 0.25), r * 0.06, Color32::from_rgb(70, 60, 40));
            }
        }
        // A sports top: the white stripes.
        Kind::Coach => {
            for s in [-1.0, 1.0] {
                painter.line_segment([c + side * s * r * 0.3 + dir * r * 0.5, c + side * s * r * 0.3 - dir * r * 0.6], Stroke::new(1.6 * scale, Color32::WHITE));
            }
        }
        // The T-shirt and its joke: « 404 ».
        Kind::It => {
            painter.text(c - dir * r * 0.25, Align2::CENTER_CENTER, "404", FontId::monospace(r * 0.38), Color32::from_rgb(90, 230, 110));
        }
        _ => {}
    }
    // Torn and stained.
    for k in 0..3 {
        let at = c + dir * (rnd(k) - 0.5) * r * 0.8 + side * (rnd(k + 3) - 0.5) * r * 1.4;
        painter.circle_filled(at, r * (0.08 + 0.12 * rnd(k + 6)), BLOOD.gamma_multiply(0.85));
    }
    painter.line_segment([c + side * (rnd(9) - 0.5) * r, c + side * (rnd(10) - 0.5) * r + dir * r * 0.3], Stroke::new(1.5 * scale, clothes.lerp_to_gamma(Color32::BLACK, 0.45)));
    // The head: its face ahead, yellowed eyes, the mouth open.
    let hr = r * if kind == Kind::Hr { 0.38 } else { 0.47 };
    let hc = c + dir * r * 0.06;
    paint_head(painter, hc, dir, hr, skin, hair);
    for s in [-1.0, 1.0] {
        let eye = hc + dir * hr * 0.62 + side * s * hr * 0.36;
        let eye_color = if kind == Kind::It && rebooting { Color32::from_rgb(110, 200, 255) } else { Color32::from_rgb(232, 224, 150) };
        painter.circle_filled(eye, hr * 0.15, eye_color);
        if matches!(kind, Kind::Hr | Kind::Accountant) {
            painter.circle_stroke(eye, hr * 0.28, Stroke::new(1.3 * scale, Color32::from_rgb(30, 30, 34)));
        }
    }
    match kind {
        // The headset: its band over the head, the microphone at the mouth.
        Kind::Visio => {
            painter.line_segment([hc - side * hr, hc + side * hr], Stroke::new(2.2 * scale, Color32::from_rgb(30, 30, 34)));
            for s in [-1.0, 1.0] {
                painter.circle_filled(hc + side * s * hr, hr * 0.26, Color32::from_rgb(30, 30, 34));
            }
            painter.line_segment([hc + side * hr, hc + dir * hr * 1.05 + side * hr * 0.3], Stroke::new(1.4 * scale, Color32::from_rgb(30, 30, 34)));
            painter.circle_filled(hc + dir * hr * 1.05 + side * hr * 0.3, hr * 0.12, Color32::from_rgb(60, 60, 66));
        }
        // The coach's headband.
        Kind::Coach => {
            painter.line_segment([hc - side * hr * 0.95 + dir * hr * 0.2, hc + side * hr * 0.95 + dir * hr * 0.2], Stroke::new(2.4 * scale, Color32::from_rgb(255, 90, 150)));
        }
        _ => {}
    }
    painter.circle_filled(hc + dir * hr * 0.88, hr * 0.17, Color32::from_rgb(70, 16, 18));
    if slowed {
        // The extinguisher's foam on it.
        for k in 0..5 {
            let at = c + Vec2::angled(k as f32 * 1.3 + rnd(k)) * r * 0.6;
            painter.circle_filled(at, r * 0.28, Color32::from_white_alpha(120));
        }
    }
    if charging {
        painter.circle_stroke(c, r + 5.0 * scale, Stroke::new(2.0 * scale, Color32::from_rgb(255, 93, 93)));
    }
    // Frozen: bands of noise across it, the bars of a bad connection above.
    if kind == Kind::Visio && frozen {
        for k in 0..4 {
            let y = c.y + (rnd(k) - 0.5) * r * 1.8;
            let w = r * (0.8 + rnd(k + 4));
            painter.rect_filled(Rect::from_center_size(Pos2::new(c.x + (rnd(k + 8) - 0.5) * r * 0.6, y), Vec2::new(w * 2.0, 2.0 * scale)), 0.0, Color32::from_white_alpha(110));
        }
        let base = c + Vec2::new(-7.0, -r - 8.0 * scale);
        for k in 0..4 {
            let h = (3.0 + k as f32 * 3.0) * scale;
            let color = if k == 0 { Color32::from_rgb(255, 93, 93) } else { Color32::from_white_alpha(50) };
            painter.rect_filled(Rect::from_min_max(base + Vec2::new(k as f32 * 4.0 * scale, -h), base + Vec2::new(k as f32 * 4.0 * scale + 3.0 * scale, 0.0)), 0.0, color);
        }
    }
    // Healing: little crosses rising from her.
    if kind == Kind::Coach && healing {
        for k in 0..3 {
            let rise = (phase * 0.4 + k as f32 / 3.0).fract();
            let at = c + Vec2::new((k as f32 - 1.0) * r * 0.7, -r * (0.6 + 1.6 * rise));
            let a = (1.0 - rise) * 220.0;
            let color = Color32::from_rgba_unmultiplied(110, 235, 140, a as u8);
            painter.line_segment([at - Vec2::new(3.0, 0.0) * scale, at + Vec2::new(3.0, 0.0) * scale], Stroke::new(1.8 * scale, color));
            painter.line_segment([at - Vec2::new(0.0, 3.0) * scale, at + Vec2::new(0.0, 3.0) * scale], Stroke::new(1.8 * scale, color));
        }
    }
    // Rebooting: greyed, the wheel turning above.
    if kind == Kind::It && rebooting {
        painter.circle_filled(c, r * 1.05, Color32::from_black_alpha(90));
        let top = c - Vec2::new(0.0, r + 12.0 * scale);
        for k in 0..8 {
            let a = k as f32 / 8.0 * std::f32::consts::TAU + phase * 2.0;
            painter.circle_filled(top + Vec2::angled(a) * 6.0 * scale, 1.6 * scale, Color32::from_white_alpha(40 + k * 26));
        }
    }
    if life < 1.0 {
        let bar = Rect::from_min_size(c + Vec2::new(-r, r + 8.0 * scale), Vec2::new(2.0 * r, 4.0 * scale));
        painter.rect_filled(bar, 0.0, Color32::from_black_alpha(140));
        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * life.clamp(0.0, 1.0), bar.height())), 0.0, Color32::from_rgb(255, 93, 93));
    }
}

/// A colleague seen from above, aiming at `aim`: a shirt in their color, both hands on the weapon
/// held ahead, the hair, the face toward where they aim.
#[allow(clippy::too_many_arguments)]
fn paint_colleague(painter: &egui::Painter, c: Pos2, scale: f32, color: Color32, aim: f32, bob: f32, weapon: usize, now: f64) {
    let c = c + Vec2::new(0.0, bob * 4.0 * scale);
    let r = 19.0 * scale;
    let dir = Vec2::angled(aim);
    let side = Vec2::new(-dir.y, dir.x);
    let skin = Color32::from_rgb(236, 196, 160);
    let sleeve = color.lerp_to_gamma(Color32::BLACK, 0.2);
    fill(painter, oval(c + Vec2::new(3.0, 6.0) * scale, dir, r * 0.8, r * 1.1), Color32::from_black_alpha(80));
    // The weapon, then the arms holding it.
    paint_held(painter, c, dir, r, weapon, now);
    for (s, grip) in [(-1.0, 0.75), (1.0, 1.15)] {
        let shoulder = c + side * s * r * 0.72;
        let hand = c + dir * r * grip + side * s * r * 0.1;
        painter.line_segment([shoulder, hand], Stroke::new(r * 0.32, sleeve));
        painter.circle_filled(shoulder, r * 0.17, sleeve);
        painter.circle_filled(hand, r * 0.18, skin);
    }
    fill(painter, oval(c, dir, r * 0.56, r * 1.0), color);
    // A collar.
    fill(painter, vec![c + dir * r * 0.5 - side * r * 0.2, c + dir * r * 0.18, c + dir * r * 0.5 + side * r * 0.2], Color32::from_rgb(245, 245, 245));
    paint_head(painter, c - dir * r * 0.04, dir, r * 0.46, skin, Some(Color32::from_rgb(72, 50, 34)));
}

/// A weapon dropped on the floor: a glow of its color, a cardboard box, its name; blinking before it
/// goes.
fn paint_drop(painter: &egui::Painter, c: Pos2, scale: f32, weapon: usize, left: u32, now: f64, name: &str) {
    if left <= 5 && (now * 6.0).sin() < 0.0 {
        return;
    }
    let color = WEAPON_COLORS[weapon];
    let pulse = 0.5 + 0.5 * (now * 4.0).sin() as f32;
    painter.circle_filled(c, (26.0 + 6.0 * pulse) * scale, color.gamma_multiply(0.18 + 0.12 * pulse));
    let float = Vec2::new(0.0, -3.0 * (now * 3.0).sin() as f32 * scale);
    let r#box = Rect::from_center_size(c + float, Vec2::new(30.0, 22.0) * scale);
    painter.rect_filled(r#box.translate(Vec2::new(2.0, 4.0) * scale), 3.0 * scale, Color32::from_black_alpha(80));
    painter.rect_filled(r#box, 3.0 * scale, Color32::from_rgb(170, 128, 82));
    painter.rect_filled(Rect::from_center_size(r#box.center(), Vec2::new(30.0, 5.0) * scale), 0.0, color);
    painter.text(r#box.center_top() - Vec2::new(0.0, 10.0 * scale), Align2::CENTER_CENTER, name, FontId::proportional(11.0 * scale.max(0.8)), color);
}

/// A colleague down: lying, the ring of the help received around.
fn paint_down(painter: &egui::Painter, c: Pos2, scale: f32, color: Color32, progress: f32, theme: &Theme) {
    painter.add(egui::Shape::ellipse_filled(c, Vec2::new(20.0, 14.0) * scale, color.gamma_multiply(0.55)));
    painter.circle_stroke(c, 34.0 * scale, Stroke::new(4.0 * scale, Color32::from_white_alpha(40)));
    if progress > 0.0 {
        let n = 40;
        let points: Vec<Pos2> = (0..=n).map(|k| c + Vec2::angled(-std::f32::consts::FRAC_PI_2 + progress.min(1.0) * std::f32::consts::TAU * k as f32 / n as f32) * 34.0 * scale).collect();
        painter.add(egui::Shape::line(points, Stroke::new(4.0 * scale, COLORS[0])));
    }
    painter.text(c, Align2::CENTER_CENTER, "K.O.", FontId::proportional(12.0 * scale.max(0.7)), theme.text);
}

/// The weapon in the hands, ahead of `c` toward `dir` (`r`: the colleague's size): the stapler, the
/// post-it launcher (a tube, a pad of post-its loaded), the extinguisher (red, its black horn), the
/// confetti cannon (a striped cone).
fn paint_held(painter: &egui::Painter, c: Pos2, dir: Vec2, r: f32, weapon: usize, now: f64) {
    let side = Vec2::new(-dir.y, dir.x);
    let outline = Stroke::new((r * 0.06).max(0.8), Color32::from_black_alpha(140));
    let poly = |points: Vec<Pos2>, color: Color32| {
        painter.add(egui::Shape::convex_polygon(points, color, outline));
    };
    match weapon {
        0 => {
            // A big office stapler: its grey base, its black arm over it, the chrome tip.
            let base = c + dir * r * 1.35;
            poly(quad(base, dir, r * 1.5, r * 0.42), Color32::from_rgb(120, 126, 140));
            poly(quad(base + dir * r * 0.1 + side * r * 0.02, dir, r * 1.25, r * 0.3), Color32::from_rgb(28, 30, 36));
            painter.circle_filled(base - dir * r * 0.55, r * 0.12, Color32::from_rgb(200, 205, 215));
            poly(quad(base + dir * r * 0.72, dir, r * 0.18, r * 0.36), Color32::from_rgb(214, 220, 232));
        }
        1 => {
            // A yellow tube, a pad of post-its on top, a pink one ready at the mouth.
            let body = c + dir * r * 1.35;
            poly(quad(body, dir, r * 1.6, r * 0.36), Color32::from_rgb(70, 74, 88));
            poly(quad(body + dir * r * 0.85, dir, r * 0.22, r * 0.5), Color32::from_rgb(40, 42, 50));
            let pad = body - dir * r * 0.15 + side * r * 0.08;
            poly(quad(pad, dir, r * 0.62, r * 0.62), Color32::from_rgb(255, 224, 102));
            poly(quad(pad - dir * r * 0.06, dir, r * 0.2, r * 0.62), Color32::from_rgb(232, 196, 70));
            let mouth = body + dir * r * 1.02;
            poly(quad(mouth, dir, r * 0.26, r * 0.26), Color32::from_rgb(255, 140, 190));
        }
        2 => {
            // The red bottle held across the belly, its black hose and horn toward the aim.
            let bottle = c + dir * r * 0.95 - side * r * 0.25;
            poly(oval(bottle, dir, r * 0.55, r * 0.34), Color32::from_rgb(214, 44, 38));
            painter.circle_filled(bottle - dir * r * 0.4, r * 0.16, Color32::from_rgb(40, 40, 44));
            painter.line_segment([bottle - dir * r * 0.15 + side * r * 0.15, bottle + dir * r * 0.55 + side * r * 0.25], Stroke::new(r * 0.07, Color32::from_rgb(240, 240, 240)));
            let horn = c + dir * r * 1.75 + side * r * 0.05;
            painter.line_segment([bottle - dir * r * 0.4, horn - dir * r * 0.3], Stroke::new(r * 0.12, Color32::from_rgb(30, 30, 34)));
            poly(vec![horn - dir * r * 0.32 - side * r * 0.08, horn + dir * r * 0.22 - side * r * 0.2, horn + dir * r * 0.22 + side * r * 0.2, horn - dir * r * 0.32 + side * r * 0.08], Color32::from_rgb(30, 30, 34));
            // A wisp of foam at the horn.
            let puff = (now * 7.0).sin() as f32 * 0.5 + 0.5;
            painter.circle_filled(horn + dir * r * 0.32, r * (0.08 + 0.06 * puff), Color32::from_white_alpha(180));
        }
        _ => {
            // A cone widening toward the aim, pink and gold stripes, confetti peeking out.
            let back = c + dir * r * 0.65;
            let mouth = c + dir * r * 2.05;
            poly(vec![back - side * r * 0.16, mouth - side * r * 0.42, mouth + side * r * 0.42, back + side * r * 0.16], Color32::from_rgb(255, 122, 182));
            for k in 1..4 {
                let f = k as f32 / 4.0;
                let p = back + (mouth - back) * f;
                let w = 0.16 + (0.42 - 0.16) * f;
                painter.line_segment([p - side * r * w, p + side * r * w], Stroke::new(r * 0.09, Color32::from_rgb(255, 210, 90)));
            }
            painter.line_segment([mouth - side * r * 0.42, mouth + side * r * 0.42], Stroke::new(r * 0.08, Color32::from_rgb(255, 240, 200)));
            for (k, color) in CONFETTI.iter().enumerate() {
                let a = (k as f32 - 1.5) * 0.25;
                let p = mouth + (dir * (a.cos() * 0.12) + side * a) * r;
                painter.rect_filled(Rect::from_center_size(p, Vec2::splat(r * 0.13)), 0.0, *color);
            }
        }
    }
}

/// What a zombie is doing, seen on it.
#[derive(Clone, Copy, Default)]
pub(super) struct Fx {
    pub(super) slowed: bool,
    pub(super) charging: bool,
    pub(super) frozen: bool,
    pub(super) healing: bool,
    pub(super) rebooting: bool,
}

/// A kind's life, bite (each second at contact), pace (px/s) and the tickets it brings, as floor's
/// KINDS has them (before the game's scaling).
fn kind_stats(kind: Kind) -> [f32; 4] {
    match kind {
        Kind::Base => [40.0, 10.0, 62.0, 10.0],
        Kind::Intern => [20.0, 6.0, 125.0, 8.0],
        Kind::Meeting => [130.0, 12.0, 36.0, 25.0],
        Kind::Sales => [80.0, 16.0, 52.0, 20.0],
        Kind::ReplyAll => [50.0, 10.0, 60.0, 15.0],
        Kind::Cc => [16.0, 5.0, 80.0, 3.0],
        Kind::Hr => [900.0, 25.0, 38.0, 300.0],
        Kind::Board => [650.0, 22.0, 44.0, 400.0],
        Kind::Visio => [45.0, 9.0, 55.0, 14.0],
        Kind::Accountant => [110.0, 12.0, 46.0, 22.0],
        Kind::Coach => [70.0, 8.0, 50.0, 30.0],
        Kind::It => [60.0, 10.0, 58.0, 25.0],
    }
}

/// The zombies met (places in BESTIARY): seen in a game, or at a wave the player went past.
pub(super) fn met_kinds(s: &crate::config::Settings) -> Vec<u8> {
    BESTIARY.iter().enumerate().filter(|(k, (_, from, _))| s.monday_seen & (1 << k) != 0 || s.monday_best >= *from).map(|(k, _)| k as u8).collect()
}

/// The kind at this place of the bestiary.
pub(super) fn bestiary_kind(k: u8) -> Kind {
    BESTIARY.get(k as usize).map_or(Kind::Base, |b| b.0)
}

/// A zombie never met: its shadow only (the body, the arms, the head, what it carries), a « ? ».
fn paint_shadow(painter: &egui::Painter, c: Pos2, scale: f32, kind: Kind, phase: f32) {
    let r = kind.radius() * scale;
    let dir = Vec2::new(0.0, 1.0);
    let side = Vec2::new(-1.0, 0.0);
    let ink = Color32::from_black_alpha(150);
    let c = c + side * phase.sin() * 1.6 * scale;
    for s in [-1.0f32, 1.0] {
        let shoulder = c + side * s * r * 0.7;
        let hand = shoulder + dir * r * (1.0 + 0.18 * (phase * 1.3 + s * 1.7).sin()) - side * s * r * 0.1;
        painter.line_segment([shoulder, hand], Stroke::new(r * 0.34, ink));
        painter.circle_filled(hand, r * 0.19, ink);
    }
    match kind {
        Kind::Meeting | Kind::It => fill(painter, quad(c + dir * r * 1.05, dir, r * 0.5, r * 0.9), ink),
        Kind::Accountant => fill(painter, quad(c + dir * r * 1.12, dir, r * 0.34, r * 1.55), ink),
        Kind::Board | Kind::Hr => fill(painter, quad(c + dir * r * 1.1 - side * r * 0.5, dir, r * 0.55, r * 0.4), ink),
        _ => {}
    }
    fill(painter, oval(c, dir, r * 0.56, r * 0.98), ink);
    painter.circle_filled(c + dir * r * 0.06, r * if kind == Kind::Hr { 0.38 } else { 0.47 }, ink);
    painter.text(c + dir * r * 0.06, Align2::CENTER_CENTER, "?", FontId::proportional(r * 0.7), Color32::from_white_alpha(90));
}

/// A first-aid kit on the floor: white, its red cross, a green glow breathing; blinking before it goes.
fn paint_kit(painter: &egui::Painter, c: Pos2, scale: f32, left: u32, now: f64) {
    if left <= 5 && (now * 6.0).sin() < 0.0 {
        return;
    }
    let pulse = 0.5 + 0.5 * (now * 3.0).sin() as f32;
    painter.circle_filled(c, (24.0 + 5.0 * pulse) * scale, Color32::from_rgba_unmultiplied(110, 235, 140, (30.0 + 30.0 * pulse) as u8));
    let float = Vec2::new(0.0, -2.5 * (now * 2.5).sin() as f32 * scale);
    let r#box = Rect::from_center_size(c + float, Vec2::new(26.0, 20.0) * scale);
    painter.rect_filled(r#box.translate(Vec2::new(2.0, 4.0) * scale), 4.0 * scale, Color32::from_black_alpha(80));
    painter.rect_filled(r#box, 4.0 * scale, Color32::from_rgb(244, 244, 240));
    painter.rect_stroke(r#box, 4.0 * scale, Stroke::new(1.0, Color32::from_rgb(200, 200, 196)), egui::StrokeKind::Inside);
    painter.rect_filled(Rect::from_center_size(r#box.center() - Vec2::new(0.0, 12.0) * scale, Vec2::new(10.0, 4.0) * scale), 1.5, Color32::from_rgb(200, 200, 196));
    let red = Color32::from_rgb(220, 40, 50);
    painter.rect_filled(Rect::from_center_size(r#box.center(), Vec2::new(12.0, 4.0) * scale), 0.0, red);
    painter.rect_filled(Rect::from_center_size(r#box.center(), Vec2::new(4.0, 12.0) * scale), 0.0, red);
}

/// The confetti's colours.
const CONFETTI: [Color32; 4] = [Color32::from_rgb(255, 181, 71), Color32::from_rgb(110, 200, 255), Color32::from_rgb(140, 220, 120), Color32::from_rgb(255, 122, 182)];

/// A shot: a staple flying (a trail behind), a post-it turning (folded corner, shadow), a cloud of foam,
/// a burst of confetti.
fn paint_shot(painter: &egui::Painter, c: Pos2, scale: f32, angle: f32, weapon: usize, now: f64) {
    let dir = Vec2::angled(angle);
    let side = Vec2::new(-dir.y, dir.x);
    // Each shot its own look, kept while it flies: from its angle (its place changes every frame).
    let seed = ((angle * 4096.0) as i32).unsigned_abs();
    match weapon {
        0 => {
            painter.line_segment([c - dir * 22.0 * scale, c - dir * 6.0 * scale], Stroke::new(2.0 * scale, Color32::from_white_alpha(50)));
            let s = Stroke::new(2.5 * scale.max(0.6), Color32::from_rgb(223, 230, 240));
            let (a, b) = (c - dir * 6.0 * scale, c + dir * 6.0 * scale);
            painter.line_segment([a - side * 3.0 * scale, a + side * 3.0 * scale], s);
            painter.line_segment([a + side * 3.0 * scale, b + side * 3.0 * scale], s);
            painter.line_segment([b + side * 3.0 * scale, b - side * 3.0 * scale], s);
        }
        1 => {
            let spin = angle + now as f32 * 8.0;
            let turn = Vec2::angled(spin) * 8.0 * scale;
            let n = Vec2::new(-turn.y, turn.x);
            let sq = |o: Vec2| vec![c + o + turn + n, c + o - turn + n, c + o - turn - n, c + o + turn - n];
            painter.add(egui::Shape::convex_polygon(sq(Vec2::new(3.0, 4.0) * scale), Color32::from_black_alpha(60), Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(sq(Vec2::ZERO), Color32::from_rgb(255, 224, 102), Stroke::new(0.8, Color32::from_rgb(214, 180, 60))));
            // The sticky band, the corner curling.
            painter.line_segment([c + turn * 0.7 + n, c + turn * 0.7 - n], Stroke::new(2.5 * scale, Color32::from_rgb(240, 204, 80)));
            painter.add(egui::Shape::convex_polygon(vec![c - turn - n, c - turn * 0.45 - n, c - turn - n * 0.45], Color32::from_rgb(255, 244, 180), Stroke::NONE));
        }
        2 => {
            // Foam: puffs swelling and shaking, a mist around.
            painter.circle_filled(c, 15.0 * scale, Color32::from_white_alpha(40));
            for k in 0..6u32 {
                let a = noise(seed + k) * std::f32::consts::TAU + now as f32 * (1.0 + noise(seed + k + 9));
                let d = 3.0 + 6.0 * noise(seed + k + 3);
                let r = 4.0 + 4.0 * noise(seed + k + 5);
                painter.circle_filled(c + Vec2::angled(a) * d * scale - dir * k as f32 * 1.5 * scale, r * scale, Color32::from_rgb(236, 244, 250).gamma_multiply(0.75));
            }
            painter.circle_filled(c + dir * 2.0 * scale, 4.0 * scale, Color32::WHITE);
        }
        _ => {
            // A burst: pieces of every colour, turning, a ribbon curling behind.
            let ribbon: Vec<Pos2> = (0..8).map(|k| c - dir * k as f32 * 3.0 * scale + side * ((k as f32 * 1.3 + now as f32 * 10.0).sin() * 3.0 * scale)).collect();
            painter.add(egui::Shape::line(ribbon, Stroke::new(1.5 * scale, CONFETTI[(seed % 4) as usize])));
            for k in 0..7u32 {
                let a = noise(seed + k) * std::f32::consts::TAU;
                let d = 2.0 + 8.0 * noise(seed + k + 11);
                let p = c + Vec2::angled(a) * d * scale;
                let spin = Vec2::angled(now as f32 * (4.0 + 6.0 * noise(seed + k + 7)) + a) * 3.2 * scale;
                let n = Vec2::new(-spin.y, spin.x) * 0.5;
                painter.add(egui::Shape::convex_polygon(vec![p + spin + n, p - spin + n, p - spin - n, p + spin - n], CONFETTI[((seed + k) % 4) as usize], Stroke::NONE));
            }
        }
    }
}

/// A number from 0 to 1, fixed for `k`.
fn noise(k: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// The game's icon: a zombie head with its tie.
pub(super) fn paint_monday_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let r = 9.0 * scale;
    painter.circle_filled(c, r, ZOMBIE);
    let eye = Color32::from_rgb(45, 74, 34);
    painter.circle_filled(c + Vec2::new(-3.2, -1.5) * scale, 1.6 * scale, eye);
    painter.circle_filled(c + Vec2::new(3.2, -1.5) * scale, 1.6 * scale, eye);
    painter.line_segment([c + Vec2::new(-3.0, 4.0) * scale, c + Vec2::new(3.0, 4.0) * scale], Stroke::new(1.2 * scale, eye));
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, r), c + Vec2::new(2.5 * scale, r + 6.0 * scale), c + Vec2::new(0.0, r + 9.0 * scale), c + Vec2::new(-2.5 * scale, r + 6.0 * scale)], theme.ansi[1], Stroke::NONE));
}

/// The end credits, a game won, `t` seconds in (false once over): black, the title, every job done by
/// Claude (and one by Julien Lecointe), scrolling up over the zombies shambling by.
fn credits(ui: &Ui, rect: Rect, t: f32, s: &Strings) -> bool {
    const SPEED: f32 = 46.0;
    let painter = ui.painter_at(rect);
    let fade = (t / 1.2).clamp(0.0, 1.0);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha((fade * 250.0) as u8));
    let jobs: Vec<&str> = s.monday_credit_jobs.split('|').collect();
    let ends: Vec<&str> = s.monday_credit_ends.split('|').collect();
    let c = rect.center().x;
    let mut y = rect.max.y + 40.0 - (t - 1.0).max(0.0) * SPEED;
    let gold = Color32::from_rgb(222, 180, 80);
    let soft = Color32::from_rgb(170, 170, 176);
    paint_metal(&painter, Pos2::new(c, y), Align2::CENTER_CENTER, "LUNDI MATIN", 54.0, super::monday_theme::BLOOD, fade);
    y += 70.0;
    painter.text(Pos2::new(c, y), Align2::CENTER_CENTER, s.monday_credit_sub, FontId::proportional(16.0), soft);
    y += 110.0;
    // Each job, its holder: Claude, every time (but one).
    for (k, job) in jobs.iter().enumerate() {
        let (name, color) = if *job == s.monday_credit_assistant { ("Julien Lecointe", gold) } else { ("Claude", Color32::WHITE) };
        if (rect.min.y - 40.0..rect.max.y + 40.0).contains(&y) {
            painter.text(Pos2::new(c - 14.0, y), Align2::RIGHT_CENTER, job.to_uppercase(), FontId::proportional(13.0), soft);
            painter.text(Pos2::new(c + 14.0, y), Align2::LEFT_CENTER, name, FontId::proportional(18.0), color);
        }
        y += if k % 4 == 3 { 64.0 } else { 40.0 };
    }
    y += 80.0;
    for line in ends {
        if (rect.min.y - 40.0..rect.max.y + 40.0).contains(&y) {
            painter.text(Pos2::new(c, y), Align2::CENTER_CENTER, line, FontId::proportional(14.0), soft);
        }
        y += 44.0;
    }
    y += 140.0;
    let end = y;
    if (rect.min.y - 60.0..rect.max.y + 60.0).contains(&end.min(rect.center().y)) {
        paint_metal(&painter, Pos2::new(c, end.max(rect.center().y)), Align2::CENTER_CENTER, s.monday_credit_fin, 48.0, super::monday_theme::BLOOD, 1.0);
    }
    // The zombies, crossing at the foot of the screen.
    let now = f64::from(t);
    for k in 0..5u32 {
        let x = rect.min.x + ((t * (30.0 + k as f32 * 9.0) + k as f32 * 260.0) % (rect.width() + 80.0)) - 40.0;
        super::monday_theme::shambler(&painter, Pos2::new(x, rect.max.y - 18.0), 46.0, 1.0, now, k);
    }
    // Over a few seconds after the end reached the middle.
    end > rect.center().y - SPEED * 4.0
}

/// Agnès' cardigan, and her words on the field.
const AGNES: Color32 = Color32::from_rgb(199, 155, 255);
const GERARD: Color32 = Color32::from_rgb(196, 160, 110);

/// A grade's name (floor's index), with its stars past the CEO.
fn grade_name(t: &Strings, grade: u32, stars: u32) -> String {
    let names: Vec<&str> = t.monday_grades.split('|').collect();
    let name = names.get(grade as usize).copied().unwrap_or_default();
    if stars > 0 { format!("{name} {}", "★".repeat(stars.min(5) as usize)) } else { name.to_owned() }
}

/// Agnès seen from above: her purple cardigan, her grey bun, the post-it on her forehead (off once
/// she found Gérard), her badge, her mug; waving, a speech bubble; sitting, the ring of the help.
fn paint_agnes(painter: &egui::Painter, c: Pos2, scale: f32, now: f64, a: crate::zombie::Agnes, line: &str, postit_off: bool) {
    let sitting = a.state == AgnesState::Sitting;
    let r = 18.0 * scale;
    let dir = Vec2::angled(a.heading);
    let side = Vec2::new(-dir.y, dir.x);
    let skin = Color32::from_rgb(190, 222, 168);
    let cardigan = Color32::from_rgb(142, 108, 201);
    let sleeve = cardigan.lerp_to_gamma(Color32::BLACK, 0.18);
    let hair = Color32::from_rgb(222, 222, 228);
    let outline = Stroke::new((1.0 * scale).max(0.6), Color32::from_black_alpha(90));
    // Her halo, the shadow.
    let breath = 0.5 + 0.5 * (now * 2.0).sin() as f32;
    painter.circle_filled(c, (34.0 + 3.0 * breath) * scale, AGNES.gamma_multiply(0.1 + 0.05 * breath));
    fill(painter, oval(c + Vec2::new(3.0, 6.0) * scale, dir, r * 0.8, r * 1.1), Color32::from_black_alpha(80));
    if sitting {
        // On the floor: her legs out ahead, the coffee spilt beside, stars around her head.
        for s in [-1.0f32, 1.0] {
            let knee = c + dir * r * 0.9 + side * s * r * 0.3;
            painter.line_segment([c + side * s * r * 0.3, knee], Stroke::new(r * 0.34, Color32::from_rgb(70, 60, 90)));
            painter.circle_filled(knee + dir * r * 0.15, r * 0.2, Color32::from_rgb(120, 40, 50));
        }
        painter.add(egui::Shape::ellipse_filled(c + side * r * 1.4 + dir * r * 0.6, Vec2::new(r * 0.6, r * 0.4), Color32::from_rgb(110, 70, 40).gamma_multiply(0.8)));
        painter.circle_filled(c + side * r * 1.15 + dir * r * 0.3, r * 0.26, Color32::from_rgb(250, 250, 246));
    }
    // The arms: one holding her mug of coffee ahead, the other waving (or down, sitting).
    let mug_hand = c + dir * r * 0.95 + side * r * 0.45;
    let wave = (now * 6.0).sin() as f32 * 0.35;
    let free_hand = if sitting { c - side * r * 1.1 + dir * r * 0.2 } else { c - side * r * 1.05 - dir * r * (0.2 + wave) };
    for (shoulder, hand) in [(c + side * r * 0.72, mug_hand), (c - side * r * 0.72, free_hand)] {
        painter.line_segment([shoulder, hand], Stroke::new(r * 0.32, sleeve));
        painter.circle_filled(shoulder, r * 0.17, sleeve);
        painter.circle_filled(hand, r * 0.17, skin);
    }
    // The cardigan, the blouse's flowers at the collar, the pearls.
    fill(painter, oval(c, dir, r * 0.56, r * 1.0), cardigan);
    painter.add(egui::Shape::convex_polygon(oval(c, dir, r * 0.56, r * 1.0), Color32::TRANSPARENT, outline));
    fill(painter, vec![c + dir * r * 0.52 - side * r * 0.3, c + dir * r * 0.05, c + dir * r * 0.52 + side * r * 0.3], Color32::from_rgb(250, 236, 240));
    for k in 0..5 {
        let at = c + dir * r * 0.36 + side * (k as f32 - 2.0) * r * 0.14;
        painter.circle_filled(at, r * 0.06, Color32::from_rgb(250, 248, 240));
    }
    for k in 0..4 {
        painter.circle_filled(c - dir * r * (0.2 + 0.18 * (k % 2) as f32) + side * (k as f32 - 1.5) * r * 0.26, r * 0.07, Color32::from_rgb(255, 170, 200).gamma_multiply(0.7));
    }
    // The post-it on her back (until Gérard takes it off): « ♥ ».
    if !postit_off {
        let at = c - dir * r * 0.62;
        fill(painter, quad(at, dir, r * 0.5, r * 0.5), Color32::from_rgb(255, 224, 102));
        painter.text(at, Align2::CENTER_CENTER, "♥", FontId::proportional(r * 0.4), Color32::from_rgb(220, 60, 90));
    }
    // Her head: the grey bun, a pencil through it, her glasses, her rosy cheeks.
    let hc = c + dir * r * 0.06;
    let hr = r * 0.46;
    painter.circle_filled(hc, hr, hair);
    fill(painter, oval(hc + dir * hr * 0.32, dir, hr * 0.62, hr * 0.8), skin);
    let bun = hc - dir * hr * 0.75;
    painter.circle_filled(bun, hr * 0.48, hair);
    painter.circle_stroke(bun, hr * 0.48, Stroke::new(0.8 * scale, Color32::from_rgb(190, 190, 196)));
    painter.line_segment([bun - side * hr * 0.8 - dir * hr * 0.2, bun + side * hr * 0.8 + dir * hr * 0.2], Stroke::new(1.6 * scale, Color32::from_rgb(240, 196, 60)));
    for s in [-1.0, 1.0] {
        let eye = hc + dir * hr * 0.62 + side * s * hr * 0.34;
        painter.circle_stroke(eye, hr * 0.24, Stroke::new(1.1 * scale, Color32::from_rgb(120, 70, 140)));
        painter.circle_filled(eye, hr * 0.09, Color32::from_rgb(60, 50, 70));
        painter.circle_filled(eye + dir * hr * 0.22 + side * s * hr * 0.12, hr * 0.12, Color32::from_rgb(255, 150, 170).gamma_multiply(0.6));
    }
    painter.line_segment([hc + dir * hr * 0.62 - side * hr * 0.1, hc + dir * hr * 0.62 + side * hr * 0.1], Stroke::new(1.1 * scale, Color32::from_rgb(120, 70, 140)));
    // The mug: white, a red heart, the coffee, its steam (spilt, sitting).
    if !sitting {
        painter.circle_filled(mug_hand + dir * r * 0.12, r * 0.26, Color32::from_rgb(250, 250, 246));
        painter.circle_filled(mug_hand + dir * r * 0.12, r * 0.18, Color32::from_rgb(110, 70, 40));
        painter.circle_stroke(mug_hand + dir * r * 0.12 + side * r * 0.3, r * 0.1, Stroke::new(1.4 * scale, Color32::from_rgb(250, 250, 246)));
        for k in 0..2 {
            let rise = ((now * 0.7 + k as f64 * 0.5) % 1.0) as f32;
            let at = mug_hand + dir * r * 0.12 + Vec2::new((rise * 9.0 + k as f32).sin() * 3.0 * scale, -rise * 16.0 * scale);
            painter.circle_filled(at, r * (0.1 + 0.08 * rise), Color32::from_white_alpha((120.0 * (1.0 - rise)) as u8));
        }
    } else {
        for k in 0..3 {
            let a = now as f32 * 3.0 + k as f32 * 2.1;
            let at = hc + Vec2::angled(a) * hr * 1.4;
            let gold = Stroke::new(1.4 * scale, Color32::from_rgb(255, 224, 102));
            painter.line_segment([at - Vec2::new(3.5, 0.0) * scale, at + Vec2::new(3.5, 0.0) * scale], gold);
            painter.line_segment([at - Vec2::new(0.0, 3.5) * scale, at + Vec2::new(0.0, 3.5) * scale], gold);
        }
    }
    if sitting {
        let n = 36;
        let k = (a.help / 3.0).clamp(0.0, 1.0);
        painter.circle_stroke(c, 30.0 * scale, Stroke::new(3.0 * scale, Color32::from_white_alpha(40)));
        if k > 0.0 {
            let points: Vec<Pos2> = (0..=n).map(|i| c + Vec2::angled(-std::f32::consts::FRAC_PI_2 + k * std::f32::consts::TAU * i as f32 / n as f32) * 30.0 * scale).collect();
            painter.add(egui::Shape::line(points, Stroke::new(3.0 * scale, AGNES)));
        }
    }
    // Her name, on a badge.
    let font = FontId::proportional(10.5 * scale.max(0.75));
    let galley = painter.layout_no_wrap("AGNÈS".to_owned(), font, Color32::WHITE);
    let tag = Rect::from_center_size(c + Vec2::new(0.0, 33.0 * scale), galley.size() + Vec2::new(12.0, 4.0));
    painter.rect_filled(tag, tag.height() / 2.0, AGNES.lerp_to_gamma(Color32::BLACK, 0.25));
    painter.galley(tag.center() - galley.size() / 2.0, galley, Color32::WHITE);
    if a.state == AgnesState::Walking && a.life < 1.0 {
        let bar = Rect::from_min_size(c + Vec2::new(-22.0, 44.0) * scale, Vec2::new(44.0, 5.0) * scale);
        painter.rect_filled(bar, 0.0, Color32::from_black_alpha(150));
        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * a.life.clamp(0.0, 1.0), bar.height())), 0.0, if a.life > 0.4 { AGNES } else { Color32::from_rgb(255, 93, 93) });
    }
    let text = if sitting { "Aïe…" } else { line };
    let font = FontId::proportional(13.0 * scale.max(0.75));
    let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::from_rgb(61, 42, 99));
    let bubble = Rect::from_center_size(c - Vec2::new(0.0, 56.0 * scale), galley.size() + Vec2::new(20.0, 12.0));
    painter.rect_filled(bubble, 8.0, Color32::from_rgb(245, 240, 255));
    painter.add(egui::Shape::convex_polygon(vec![bubble.center_bottom() - Vec2::new(6.0, 0.0), bubble.center_bottom() + Vec2::new(6.0, 0.0), bubble.center_bottom() + Vec2::new(0.0, 9.0)], Color32::from_rgb(245, 240, 255), Stroke::NONE));
    painter.galley(bubble.center() - galley.size() / 2.0, galley, Color32::from_rgb(61, 42, 99));
}

/// Gérard, a kind zombie (no one bites him, no one shoots him): a brown jacket, a moustache, a hand
/// waving, looking for her; hearts once he found her.
fn paint_gerard(painter: &egui::Painter, c: Pos2, scale: f32, now: f64, met: bool) {
    painter.circle_filled(c, 34.0 * scale, GERARD.gamma_multiply(0.12));
    painter.circle_filled(c + Vec2::new(3.0, 6.0) * scale, 18.0 * scale, Color32::from_black_alpha(80));
    let wave = (now * 6.0).sin() as f32 * 0.5;
    painter.circle_filled(c + Vec2::angled(-1.9 - wave) * 22.0 * scale, 5.0 * scale, Color32::from_rgb(170, 214, 140));
    painter.circle_filled(c, 18.0 * scale, Color32::from_rgb(120, 86, 60));
    painter.circle_filled(c - Vec2::new(0.0, 2.0) * scale, 12.0 * scale, Color32::from_rgb(170, 214, 140));
    painter.line_segment([c + Vec2::new(-5.0, 3.0) * scale, c + Vec2::new(5.0, 3.0) * scale], Stroke::new(2.5 * scale, Color32::from_rgb(90, 70, 50)));
    painter.text(c + Vec2::new(0.0, 32.0 * scale), Align2::CENTER_CENTER, "GÉRARD", FontId::monospace(11.0 * scale.max(0.7)), GERARD);
    let text = if met { "Mon Agnès !" } else { "Agnès ?" };
    let font = FontId::proportional(13.0 * scale.max(0.75));
    let ink = Color32::from_rgb(70, 50, 30);
    let galley = painter.layout_no_wrap(text.to_owned(), font, ink);
    let bubble = Rect::from_center_size(c - Vec2::new(0.0, if met { 70.0 } else { 52.0 }) * scale, galley.size() + Vec2::new(20.0, 12.0));
    painter.rect_filled(bubble, 8.0, Color32::from_rgb(250, 240, 222));
    painter.add(egui::Shape::convex_polygon(vec![bubble.center_bottom() - Vec2::new(6.0, 0.0), bubble.center_bottom() + Vec2::new(6.0, 0.0), bubble.center_bottom() + Vec2::new(0.0, 9.0)], Color32::from_rgb(250, 240, 222), Stroke::NONE));
    painter.galley(bubble.center() - galley.size() / 2.0, galley, ink);
    if met {
        for k in 0..3 {
            let rise = ((now * 0.8 + k as f64 / 3.0) % 1.0) as f32;
            painter.text(c + Vec2::new(-14.0 + k as f32 * 14.0, -26.0 - rise * 30.0) * scale, Align2::CENTER_CENTER, "♥", FontId::proportional(14.0 * scale.max(0.7)), Color32::from_rgb(255, 122, 182).gamma_multiply(1.0 - rise));
        }
    }
}

/// The photocopier turret: a grey box, its barrel toward its target, its seconds left.
fn paint_turret(painter: &egui::Painter, c: Pos2, scale: f32, aim: f32, left: u32) {
    let body = Rect::from_center_size(c, Vec2::new(30.0, 24.0) * scale);
    painter.rect_filled(body.translate(Vec2::new(3.0, 5.0) * scale), 4.0 * scale, Color32::from_black_alpha(80));
    painter.rect_filled(body, 4.0 * scale, Color32::from_rgb(200, 204, 214));
    painter.rect_filled(Rect::from_min_size(body.min + Vec2::new(4.0, 4.0) * scale, Vec2::new(22.0, 5.0) * scale), 1.0, Color32::from_rgb(90, 160, 220));
    painter.line_segment([c, c + Vec2::angled(aim) * 22.0 * scale], Stroke::new(5.0 * scale, Color32::from_rgb(60, 64, 76)));
    painter.text(c + Vec2::new(0.0, 22.0 * scale), Align2::CENTER_CENTER, format!("{left} s"), FontId::monospace(10.0 * scale.max(0.7)), Color32::WHITE);
}

/// A letter on the floor: an envelope with a heart, glowing.
fn paint_letter(painter: &egui::Painter, c: Pos2, scale: f32, now: f64) {
    let glow = 0.5 + 0.5 * (now * 4.0).sin() as f32;
    painter.circle_filled(c, 22.0 * scale, AGNES.gamma_multiply(0.15 + 0.15 * glow));
    let r = Rect::from_center_size(c, Vec2::new(22.0, 15.0) * scale);
    painter.rect_filled(r, 2.0, Color32::from_rgb(250, 245, 230));
    painter.line_segment([r.left_top(), r.center()], Stroke::new(1.0, Color32::from_rgb(180, 170, 150)));
    painter.line_segment([r.right_top(), r.center()], Stroke::new(1.0, Color32::from_rgb(180, 170, 150)));
    painter.text(r.center() + Vec2::new(0.0, 2.0 * scale), Align2::CENTER_CENTER, "♥", FontId::proportional(9.0 * scale.max(0.7)), Color32::from_rgb(230, 80, 120));
}

/// Agnès' twelve letters to Gérard, found one by one in the games.
const LETTERS_FR: [&str; 12] = [
    "Mon Gérard, la nouvelle machine à café est arrivée ce matin. Elle fait un bruit bizarre et le café a un goût de terre. Je t’en garde un quand même. Bisous, Agnès.",
    "Gérard, les commerciaux sont revenus du séminaire tout verts. Ils disent que c’est le team building. Moi je trouve qu’ils sentent le marécage.",
    "Mon chéri, Kevin le stagiaire m’a mordu le badge. Le badge, pas la main, heureusement. Il voulait juste dire bonjour, je crois.",
    "Gérard, toute la compta a répondu à tous au même mail, quarante fois. Puis ils se sont levés en même temps, et ils marchent tous vers l’ascenseur.",
    "Je n’arrive plus à te joindre. Ton poste sonne dans le vide, et ton mug est encore sur ton bureau, plein. Ça ne te ressemble pas de laisser un café.",
    "J’ai un peu mal à la tête et je n’ai plus faim. Mais je dis toujours bonjour à tout le monde : quoi qu’il arrive, l’accueil reste ouvert.",
    "Mon Gérard, je crois que je suis un peu zombie moi aussi. Mais les autres ne me font rien, et j’ai encore envie de café. C’est bon signe, non ?",
    "Des collègues tiennent encore l’open space, avec des agrafeuses. Ils sont gentils, ils m’ont raccompagnée jusqu’au café. Je leur ai servi un double.",
    "Le DRH a recruté douze stagiaires ce matin. Aucun n’a de contrat. Je les ai quand même tous badgés, la procédure c’est la procédure.",
    "On m’a dit qu’on t’avait vu au parking, niveau -2, en train de chercher ta voiture. Ça, ça te ressemble. Remonte par l’escalier, mon cœur.",
    "J’ai écrit ton nom sur un post-it et je l’ai collé sur mon front, pour ne jamais t’oublier, même si ma tête s’en va un peu.",
    "Gérard, si tu lis ceci, viens me chercher à l’accueil. Je t’attendrai avec deux cafés, et cette fois je te laisse le sucre. Ton Agnès, pour toujours.",
];
const LETTERS_EN: [&str; 12] = [
    "My Gérard, the new coffee machine arrived this morning. It makes a strange noise and the coffee tastes of soil. I’m keeping you one anyway. Love, Agnès.",
    "Gérard, the sales team came back from the seminar all green. They say it’s team building. I think they smell like a swamp.",
    "Darling, Kevin the intern bit my badge. The badge, not my hand, luckily. He only wanted to say hello, I think.",
    "Gérard, the whole accounting team replied all to the same email, forty times. Then they stood up together, and they’re all walking to the lift.",
    "I can’t reach you anymore. Your phone rings and rings, and your mug is still on your desk, full. Leaving a coffee isn’t like you.",
    "My head aches a little and I’m not hungry anymore. But I still say hello to everyone: whatever happens, the reception stays open.",
    "My Gérard, I think I’m a bit of a zombie too. But the others leave me alone, and I still want coffee. That’s a good sign, isn’t it?",
    "Some colleagues are still holding the open space, with staplers. They’re kind, they walked me back to the coffee machine. I made them a double.",
    "The HR director hired twelve interns this morning. None has a contract. I gave them all a badge anyway: procedure is procedure.",
    "Someone saw you in the car park, level -2, looking for your car. That’s just like you. Come up by the stairs, sweetheart.",
    "I wrote your name on a post-it and stuck it on my forehead, so I never forget you, even if my head wanders off a little.",
    "Gérard, if you read this, come and find me at the reception desk. I’ll wait with two coffees, and this time you can have the sugar. Your Agnès, forever.",
];
