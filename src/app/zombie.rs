//! Lundi matin, in the games (its rules on floor, its connection in `crate::zombie`): survive the
//! waves of office zombies in the open space, 1 to 4 colleagues. Its welcome screen lists the rooms
//! (open one, join one) beside the world's board; in a room, who's there, the host starts, the others
//! wait (and colleagues online can be invited); in a game, the office seen from above.
//! ZQSD / WASD / arrows to move, the mouse to aim, its button to fire, 1 2 3 4 for a weapon, R to
//! reload, Space to say ready during the coffee break.

use super::four::HomeGame;
use super::*;
use crate::zombie::{Frame, Happened, Kind, Online, Over, State, Status};

/// The players' colors, by seat.
const COLORS: [Color32; 4] = [Color32::from_rgb(79, 195, 255), Color32::from_rgb(255, 122, 182), Color32::from_rgb(255, 211, 79), Color32::from_rgb(94, 224, 122)];
/// The zombies' skin.
const ZOMBIE: Color32 = Color32::from_rgb(143, 209, 106);
const CARPET: [Color32; 2] = [Color32::from_rgb(44, 50, 64), Color32::from_rgb(40, 48, 64)];
const DESK: Color32 = Color32::from_rgb(201, 180, 143);
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
}

impl Monday {
    /// The page left: out of the room. A game under way keeps the character (floor ends it when the
    /// room is empty).
    pub(super) fn leave(&mut self) {
        if self.online.connected() {
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
        if let Some(over) = self.monday.over.clone() {
            self.monday_over(ui, rect, &over);
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
                    };
                    let color = if Some(by) == me { theme.accent } else { theme.text_muted };
                    self.monday.floats.push((format!("+{points}"), x, y, color, now));
                    if Some(by) == me {
                        self.monday_sound(&[(660.0, 440.0, 0.05, 0.12)]);
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
                Happened::Dead(_) | Happened::Back(_) => {}
            }
        }
        self.monday.floats.retain(|f| now - f.4 < 0.9);
        if let Some(over) = news.over {
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
        }
    }

    /// The welcome screen: how to play, the rooms open (join one, or open one), beside the world's board.
    fn monday_lobby(&mut self, ui: &mut Ui, body: Rect, now: f64, has_pseudo: bool) {
        let t = self.t();
        let theme = self.theme.clone();
        let wide = body.width() >= 760.0;
        let width = body.width().min(if wide { 860.0 } else { 420.0 });
        let left_w = if wide { 420.0 } else { width };
        let x0 = body.center().x - width / 2.0;
        let card = Rect::from_min_size(Pos2::new(x0, body.min.y), Vec2::new(left_w, 470.0f32.min(body.height())));
        let painter = ui.painter().clone();
        painter.rect(card, 12.0, theme.chrome_bg, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let left = card.min.x + 20.0;
        let text_w = card.width() - 40.0;
        let mut y = card.min.y + 20.0;
        painter.text(Pos2::new(left, y + 4.0), Align2::LEFT_CENTER, t.bughunt_howto, FontId::proportional(16.0), theme.text);
        y += 26.0;
        let rules = painter.layout(t.monday_rules.to_owned(), FontId::proportional(13.0), theme.text_muted, text_w);
        painter.galley(Pos2::new(left, y), rules.clone(), theme.text_muted);
        y += rules.size().y + 12.0;
        // The zombies, side by side.
        let names: Vec<&str> = t.monday_kinds.split('|').collect();
        let kinds = [Kind::Base, Kind::Intern, Kind::Meeting, Kind::Sales, Kind::ReplyAll];
        let step = text_w / kinds.len() as f32;
        for (k, kind) in kinds.into_iter().enumerate() {
            let c = Pos2::new(left + step * (k as f32 + 0.5), y + 18.0);
            paint_zombie(&painter, c, 1.0, kind, (now * 3.0) as f32 + k as f32, 1.0, false, false);
            painter.text(Pos2::new(c.x, y + 48.0), Align2::CENTER_CENTER, names.get(k).copied().unwrap_or_default(), FontId::proportional(11.0), theme.text_muted);
        }
        y += 66.0;
        let help = painter.layout(t.monday_help.to_owned(), FontId::proportional(12.0), theme.text_muted, text_w);
        painter.galley(Pos2::new(left, y), help.clone(), theme.text_muted);
        y += help.size().y + 16.0;

        // The rooms.
        let status = &self.monday.online.status;
        let mut host = false;
        let mut join = None;
        let mut pick_pseudo = false;
        if !has_pseudo {
            let r = Rect::from_min_size(Pos2::new(left, y), Vec2::new(200.0, 32.0));
            pick_pseudo = ui.put(r, egui::Button::new(egui::RichText::new(t.pseudo_pick).size(13.5)).corner_radius(8.0)).clicked();
        } else if let Status::Failed(why) = status {
            painter.text(Pos2::new(left, y + 10.0), Align2::LEFT_CENTER, why.as_deref().unwrap_or(t.monday_failed), FontId::proportional(13.0), theme.ansi[1]);
            let r = Rect::from_min_size(Pos2::new(left, y + 28.0), Vec2::new(120.0, 30.0));
            if ui.put(r, egui::Button::new(egui::RichText::new(t.monday_retry).size(13.0)).corner_radius(8.0)).clicked() {
                self.monday.online = Online::default();
            }
        } else if *status != Status::Ready {
            painter.text(Pos2::new(left, y + 10.0), Align2::LEFT_CENTER, t.monday_connecting, FontId::proportional(13.0), theme.text_muted);
        } else {
            painter.text(Pos2::new(left, y + 6.0), Align2::LEFT_CENTER, t.monday_rooms, FontId::proportional(14.0), theme.text);
            y += 20.0;
            let lobby = self.monday.online.lobby.clone();
            if lobby.is_empty() {
                painter.text(Pos2::new(left, y + 12.0), Align2::LEFT_CENTER, t.monday_no_rooms, FontId::proportional(12.5), theme.text_muted);
                y += 28.0;
            }
            for room in lobby.iter().take(((card.max.y - 60.0 - y) / 32.0).max(0.0) as usize) {
                let row = Rect::from_min_size(Pos2::new(left, y + 2.0), Vec2::new(text_w, 28.0));
                painter.rect_filled(row, 6.0, theme.bg.gamma_multiply(0.6));
                let label = format!("{}  ·  {}/4", t.monday_room.replace("{n}", &room.host), room.players);
                painter.text(Pos2::new(row.min.x + 10.0, row.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), theme.text);
                let b = Rect::from_min_size(Pos2::new(row.max.x - 92.0, row.min.y + 2.0), Vec2::new(88.0, 24.0));
                if room.started {
                    painter.text(b.center(), Align2::CENTER_CENTER, t.monday_in_game, FontId::proportional(12.0), theme.text_muted);
                } else if room.players < 4 && ui.put(b, egui::Button::new(egui::RichText::new(t.monday_join).size(12.0)).corner_radius(6.0)).clicked() {
                    join = Some(room.id);
                }
                y += 32.0;
            }
            let r = Rect::from_min_size(Pos2::new(left, card.max.y - 50.0), Vec2::new(170.0, 34.0));
            let b = egui::Button::new(egui::RichText::new(t.monday_host).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
            host = ui.put(r, b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            painter.text(Pos2::new(r.max.x + 12.0, r.center().y), Align2::LEFT_CENTER, t.monday_solo, FontId::proportional(12.0), theme.text_muted);
        }
        if pick_pseudo {
            self.ask_pseudo();
        }
        if host {
            self.monday.online.host();
        }
        if let Some(id) = join {
            self.monday.online.join(id);
        }

        // The world's board: beside, or under on a narrow page.
        let board = if wide { Rect::from_min_max(Pos2::new(card.max.x + 32.0, body.min.y + 6.0), Pos2::new(x0 + width, body.max.y)) } else { Rect::from_min_max(Pos2::new(x0, card.max.y + 20.0), Pos2::new(x0 + width, body.max.y)) };
        if board.height() > 60.0 {
            let floor = self.monday.floor.get_or_insert_with(|| crate::floor::GameFloor::new("lundi-matin", false));
            floor.poll();
            floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
            let best = (self.config.settings.monday_best > 0).then(|| t.monday_best.replace("{n}", &self.config.settings.monday_best.to_string()));
            let entries = self.monday.floor.as_ref().and_then(|f| f.board()).map(<[_]>::to_vec);
            let kills = |e: &crate::floor::GameEntry| e.details.get("kills").and_then(serde_json::Value::as_u64).map(|k| t.monday_kills.replace("{n}", &k.to_string()));
            if self.game_board_noted(ui, board, &theme, entries.as_deref(), best, t_wave, &kills) {
                self.ask_pseudo();
            }
        }
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
        let card = Rect::from_min_size(Pos2::new(x0, body.min.y), Vec2::new(left_w, 400.0f32.min(body.height())));
        let painter = ui.painter().clone();
        painter.rect(card, 12.0, theme.chrome_bg, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let left = card.min.x + 20.0;
        let mut y = card.min.y + 24.0;
        painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, t.monday_room.replace("{n}", &room.host), FontId::proportional(17.0), theme.text);
        painter.text(Pos2::new(card.max.x - 20.0, y), Align2::RIGHT_CENTER, format!("{}/4", room.players.len()), FontId::monospace(13.0), theme.text_muted);
        y += 36.0;
        for (k, (_, name, color)) in room.players.iter().enumerate() {
            let c = Pos2::new(left + 16.0, y);
            paint_colleague(&painter, c, 0.9, COLORS[*color as usize % 4], 0.0, (now * 2.0 + k as f64).sin() as f32 * 0.3);
            let label = if *name == room.host { format!("{name}  ·  {}", t.monday_host_tag) } else { name.clone() };
            painter.text(Pos2::new(left + 44.0, y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), if name == pseudo { theme.accent } else { theme.text });
            y += 40.0;
        }
        for _ in room.players.len()..4 {
            painter.circle_stroke(Pos2::new(left + 16.0, y), 14.0, Stroke::new(1.0, theme.tab_hover));
            painter.text(Pos2::new(left + 44.0, y), Align2::LEFT_CENTER, t.monday_free_seat, FontId::proportional(13.0), theme.text_muted.gamma_multiply(0.6));
            y += 40.0;
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
        let online = &self.monday.online;
        let (Some(map), Some(frame)) = (online.map.clone(), online.frame.clone()) else { return };
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

        // The carpet, the walls, the doors where they come in.
        let tile = 64.0;
        for i in 0..(map.w / tile).ceil() as i32 {
            for j in 0..(map.h / tile).ceil() as i32 {
                let r = Rect::from_min_size(at(i as f32 * tile, j as f32 * tile), Vec2::splat(tile * scale + 0.5));
                painter.rect_filled(r, 0.0, CARPET[((i + j) % 2) as usize]);
            }
        }
        let wall = Color32::from_rgb(59, 66, 82);
        for r in [
            Rect::from_min_size(at(0.0, 0.0), Vec2::new(map.w, map.wall) * scale),
            Rect::from_min_size(at(0.0, map.h - map.wall), Vec2::new(map.w, map.wall) * scale),
            Rect::from_min_size(at(0.0, 0.0), Vec2::new(map.wall, map.h) * scale),
            Rect::from_min_size(at(map.w - map.wall, 0.0), Vec2::new(map.wall, map.h) * scale),
        ] {
            painter.rect_filled(r, 0.0, wall);
        }
        for door in &map.doors {
            let (w, h) = if door.along == "y" { (map.wall, door.span) } else { (door.span, map.wall) };
            let r = Rect::from_center_size(at(door.x.clamp(map.wall / 2.0, map.w - map.wall / 2.0), door.y.clamp(map.wall / 2.0, map.h - map.wall / 2.0)), Vec2::new(w, h) * scale);
            painter.rect_filled(r, 2.0, Color32::from_rgb(75, 127, 163));
        }
        // The coffee machine, glowing during the break.
        let glow = if frame.wave_on { 0.12 } else { 0.25 + 0.15 * (now * 3.0).sin() as f32 };
        let c = at(map.coffee.x, map.coffee.y);
        painter.circle_filled(c, 70.0 * scale, Color32::from_rgba_unmultiplied(255, 181, 71, (glow * 255.0) as u8));
        let machine = Rect::from_center_size(c, Vec2::new(60.0, 72.0) * scale);
        painter.rect_filled(machine, 6.0 * scale, Color32::from_rgb(60, 42, 32));
        painter.rect_filled(Rect::from_min_size(machine.min + Vec2::new(10.0, 10.0) * scale, Vec2::new(40.0, 14.0) * scale), 3.0 * scale, Color32::from_rgb(255, 181, 71));
        painter.text(machine.center_top() + Vec2::new(0.0, 17.0 * scale), Align2::CENTER_CENTER, "CAFÉ", FontId::monospace(10.0 * scale), Color32::from_rgb(60, 42, 32));
        // The desks: four on each island (the small one, a meeting table).
        for d in &map.desks {
            let r = Rect::from_min_size(at(d.x, d.y), Vec2::new(d.w, d.h) * scale);
            painter.rect_filled(r.translate(Vec2::new(4.0, 6.0) * scale), 6.0 * scale, Color32::from_black_alpha(64));
            if d.w > 200.0 {
                let (dw, dh) = ((d.w - 10.0) / 2.0, (d.h - 10.0) / 2.0);
                for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    let desk = Rect::from_min_size(at(d.x + i * (dw + 10.0), d.y + j * (dh + 10.0)), Vec2::new(dw, dh) * scale);
                    painter.rect_filled(desk, 5.0 * scale, DESK);
                    let screen = Rect::from_center_size(desk.center() - Vec2::new(0.0, dh * 0.18) * scale, Vec2::new(40.0, 10.0) * scale);
                    painter.rect_filled(screen, 2.0 * scale, Color32::from_rgb(29, 34, 48));
                    painter.circle_filled(desk.max - Vec2::new(16.0, 14.0) * scale, 5.0 * scale, Color32::from_rgb(232, 226, 212));
                }
            } else {
                painter.rect_filled(r, d.h / 2.0 * scale, Color32::from_rgb(150, 120, 90));
            }
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
            paint_zombie(&painter, at(x, y), scale, z.kind, (now * 4.0) as f32 + z.id as f32, z.life, z.slowed, z.charging || z.winding);
        }
        for s in &frame.shots {
            paint_shot(&painter, at(s.x, s.y), scale, s.angle, s.weapon, now);
        }
        for p in &frame.players {
            let (x, y) = mix.player(p.id, p.x, p.y);
            let (name, color) = colors.iter().find(|(id, _, _)| *id == p.id).map(|(_, n, c)| (n.as_str(), *c)).unwrap_or(("?", COLORS[0]));
            let c = at(x, y);
            match p.state {
                State::Up => {
                    let firing = p.reload <= 0.0 && Some(p.id) != me && (now * 30.0 + p.id as f64).sin() > 0.95;
                    paint_colleague(&painter, c, scale, color, p.aim, 0.0);
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
        for (text, x, y, color, since) in &self.monday.floats {
            let k = ((now - since) / 0.9) as f32;
            painter.text(at(*x, *y) - Vec2::new(0.0, 20.0 + 30.0 * k) * scale, Align2::CENTER_CENTER, text, FontId::proportional(14.0 * scale.max(0.7)), color.gamma_multiply(1.0 - k));
        }

        // The player: the keys and the mouse; the crosshair.
        let mine = me.and_then(|id| frame.players.iter().find(|p| p.id == id)).copied();
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| field.contains(*p));
        if let Some(p) = mine {
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
        let top = Rect::from_min_size(field.min + Vec2::new(12.0, 12.0), Vec2::new(250.0, 58.0));
        panel(top);
        let wave = if frame.wave_on { t.monday_wave.replace("{n}", &frame.wave.to_string()) } else { t.monday_break_title.to_owned() };
        paint_metal(&hud, top.min + Vec2::new(14.0, 20.0), Align2::LEFT_CENTER, &wave, 20.0, theme.accent, 1.0);
        let sub = if frame.wave_on { t.monday_left.replace("{n}", &frame.left.to_string()) } else { t.monday_next.replace("{n}", &(frame.wave + 1).to_string()).replace("{s}", &frame.break_left.to_string()) };
        hud.text(top.min + Vec2::new(14.0, 44.0), Align2::LEFT_CENTER, sub, FontId::proportional(13.0), Color32::WHITE);
        let tickets = Rect::from_min_size(Pos2::new(field.max.x - 182.0, field.min.y + 12.0), Vec2::new(170.0, 40.0));
        panel(tickets);
        hud.text(tickets.left_center() + Vec2::new(14.0, 0.0), Align2::LEFT_CENTER, t.monday_tickets, FontId::monospace(11.0), theme.text_muted);
        hud.text(tickets.right_center() - Vec2::new(14.0, 0.0), Align2::RIGHT_CENTER, frame.tickets.to_string(), FontId::monospace(20.0), theme.accent);
        if let Some(p) = mine {
            let weapons: Vec<&str> = t.monday_weapons.split('|').collect();
            let gun = Rect::from_min_size(Pos2::new(field.min.x + 12.0, field.max.y - 70.0), Vec2::new(300.0, 58.0));
            panel(gun);
            hud.text(gun.min + Vec2::new(14.0, 18.0), Align2::LEFT_CENTER, weapons.get(p.weapon).copied().unwrap_or_default().to_uppercase(), FontId::proportional(15.0), Color32::WHITE);
            let ammo = if p.reload > 0.0 { t.monday_reload.to_owned() } else { p.ammo.to_string() };
            hud.text(gun.right_top() + Vec2::new(-14.0, 18.0), Align2::RIGHT_CENTER, ammo, FontId::monospace(16.0), theme.accent);
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
    }

    /// The keys and the mouse, to floor: moving, aiming at the pointer, firing, a weapon, reloading, ready.
    fn monday_controls(&mut self, ui: &Ui, now: f64, me: Pos2, pointer: Option<Pos2>, field: Rect, frame: &Frame) {
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let (mx, my, fire, pick, reload, ready) = ui.input(|i| {
            let down = |keys: &[Key]| free && keys.iter().any(|k| i.key_down(*k));
            let mx = f32::from(down(&[Key::D, Key::ArrowRight])) - f32::from(down(&[Key::Q, Key::A, Key::ArrowLeft]));
            let my = f32::from(down(&[Key::S, Key::ArrowDown])) - f32::from(down(&[Key::Z, Key::W, Key::ArrowUp]));
            let fire = i.pointer.primary_down() && i.pointer.hover_pos().is_some_and(|p| field.contains(p));
            let pick = [Key::Num1, Key::Num2, Key::Num3, Key::Num4].iter().position(|k| free && i.key_pressed(*k));
            (mx, my, fire, pick, free && i.key_pressed(Key::R), free && i.key_pressed(Key::Space))
        });
        let aim = pointer.map_or(0.0, |p| (p - me).angle());
        // The aim in thousandths, so that a still mouse doesn't send anything new.
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
    }

    /// The game just over: how far the team went, the player's kills and revives.
    fn monday_over(&mut self, ui: &mut Ui, rect: Rect, over: &Over) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(140));
        let card = Rect::from_center_size(rect.center(), Vec2::new(380.0, 230.0));
        painter.rect(card, 14.0, theme.chrome_bg, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
        paint_metal(&painter, card.center_top() + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, t.monday_over, 28.0, theme.accent, 1.0);
        let body = t.monday_over_body.replace("{w}", &over.wave.to_string()).replace("{k}", &over.kills.to_string()).replace("{r}", &over.revives.to_string());
        painter.text(card.center_top() + Vec2::new(0.0, 90.0), Align2::CENTER_CENTER, body, FontId::proportional(15.0), theme.text);
        if over.record {
            painter.text(card.center_top() + Vec2::new(0.0, 122.0), Align2::CENTER_CENTER, t.game_new_best, FontId::proportional(14.0), theme.accent);
        }
        let r = Rect::from_center_size(card.center_bottom() - Vec2::new(0.0, 36.0), Vec2::new(170.0, 34.0));
        let b = egui::Button::new(egui::RichText::new(t.monday_ok).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
        if ui.put(r, b).clicked() || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape)) {
            self.monday.over = None;
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

/// A zombie seen from above: arms out toward where it goes, a tie for the salesman, a crown of paper
/// for the HR director; its life under it when hurt.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_zombie(painter: &egui::Painter, c: Pos2, scale: f32, kind: Kind, phase: f32, life: f32, slowed: bool, charging: bool) {
    let r = kind.radius() * scale;
    let skin = match kind {
        Kind::Intern => Color32::from_rgb(165, 224, 124),
        Kind::Meeting => Color32::from_rgb(127, 184, 95),
        Kind::Sales => Color32::from_rgb(111, 169, 85),
        Kind::Hr => Color32::from_rgb(120, 160, 90),
        _ => ZOMBIE,
    };
    let wob = phase.sin() * 3.0 * scale;
    let c = c + Vec2::new(wob * 0.5, 0.0);
    painter.circle_filled(c + Vec2::new(3.0, 6.0) * scale, r, Color32::from_black_alpha(80));
    // Arms reaching to the left (toward the colleagues, who start at the coffee).
    let arm = Stroke::new(7.0 * scale * (r / (18.0 * scale)).sqrt(), skin);
    for (side, w) in [(-1.0, wob), (1.0, -wob)] {
        painter.line_segment([c + Vec2::new(-4.0 * scale, side * r * 0.6), c + Vec2::new(-r - 14.0 * scale, side * r * 0.6 + w)], arm);
    }
    painter.circle_filled(c, r, if slowed { skin.lerp_to_gamma(Color32::from_rgb(200, 230, 255), 0.5) } else { skin });
    if charging {
        painter.circle_stroke(c, r + 4.0 * scale, Stroke::new(2.0 * scale, Color32::from_rgb(255, 93, 93)));
    }
    let eye = Color32::from_rgb(45, 74, 34);
    painter.circle_filled(c + Vec2::new(-r * 0.35, -r * 0.28), 3.0 * scale, eye);
    painter.circle_filled(c + Vec2::new(-r * 0.35, r * 0.28), 3.0 * scale, eye);
    match kind {
        Kind::Sales => {
            let tie = Color32::from_rgb(192, 57, 43);
            painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(2.0, 0.0) * scale, c + Vec2::new(14.0, -4.0) * scale, c + Vec2::new(14.0, 4.0) * scale], tie, Stroke::NONE));
        }
        Kind::Hr => {
            painter.rect_filled(Rect::from_center_size(c + Vec2::new(r * 0.3, 0.0), Vec2::new(14.0, 20.0) * scale), 2.0 * scale, Color32::from_rgb(240, 240, 240));
        }
        Kind::Intern => {
            painter.circle_filled(c + Vec2::new(r * 0.2, 0.0), 3.5 * scale, Color32::from_rgb(255, 224, 102));
        }
        Kind::ReplyAll | Kind::Cc => {
            painter.text(c + Vec2::new(r * 0.25, 0.0), Align2::CENTER_CENTER, "@", FontId::monospace(r * 0.8), eye);
        }
        _ => {}
    }
    if life < 1.0 {
        let bar = Rect::from_min_size(c + Vec2::new(-r, r + 6.0 * scale), Vec2::new(2.0 * r, 4.0 * scale));
        painter.rect_filled(bar, 0.0, Color32::from_black_alpha(140));
        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * life.clamp(0.0, 1.0), bar.height())), 0.0, Color32::from_rgb(255, 93, 93));
    }
}

/// A colleague seen from above, aiming at `aim`: the weapon ahead, two eyes.
fn paint_colleague(painter: &egui::Painter, c: Pos2, scale: f32, color: Color32, aim: f32, bob: f32) {
    let c = c + Vec2::new(0.0, bob * 4.0 * scale);
    painter.circle_filled(c + Vec2::new(3.0, 6.0) * scale, 19.0 * scale, Color32::from_black_alpha(80));
    let dir = Vec2::angled(aim);
    let side = Vec2::new(-dir.y, dir.x);
    let gun = [c + dir * 8.0 * scale + side * 5.0 * scale, c + dir * 38.0 * scale + side * 5.0 * scale, c + dir * 38.0 * scale - side * 5.0 * scale, c + dir * 8.0 * scale - side * 5.0 * scale];
    painter.add(egui::Shape::convex_polygon(gun.to_vec(), Color32::from_rgb(34, 37, 46), Stroke::NONE));
    painter.circle_filled(c, 19.0 * scale, color);
    for s in [-1.0, 1.0] {
        let e = c + dir * 9.0 * scale + side * 6.0 * s * scale;
        painter.circle_filled(e, 4.0 * scale, Color32::WHITE);
        painter.circle_filled(e + dir * 2.0 * scale, 2.0 * scale, Color32::from_rgb(18, 20, 26));
    }
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

/// A shot: a staple, a post-it turning, foam, confetti.
fn paint_shot(painter: &egui::Painter, c: Pos2, scale: f32, angle: f32, weapon: usize, now: f64) {
    let dir = Vec2::angled(angle);
    let side = Vec2::new(-dir.y, dir.x);
    match weapon {
        0 => {
            let s = Stroke::new(2.5 * scale.max(0.6), Color32::from_rgb(223, 230, 240));
            let (a, b) = (c - dir * 6.0 * scale, c + dir * 6.0 * scale);
            painter.line_segment([a - side * 3.0 * scale, a + side * 3.0 * scale], s);
            painter.line_segment([a + side * 3.0 * scale, b + side * 3.0 * scale], s);
            painter.line_segment([b + side * 3.0 * scale, b - side * 3.0 * scale], s);
        }
        1 => {
            let turn = Vec2::angled(angle + now as f32 * 8.0) * 7.0 * scale;
            let n = Vec2::new(-turn.y, turn.x);
            painter.add(egui::Shape::convex_polygon(vec![c + turn + n, c - turn + n, c - turn - n, c + turn - n], Color32::from_rgb(255, 224, 102), Stroke::NONE));
        }
        2 => {
            painter.circle_filled(c, 9.0 * scale, Color32::from_white_alpha(170));
        }
        _ => {
            let colors = [Color32::from_rgb(255, 181, 71), COLORS[0], COLORS[1], COLORS[3]];
            painter.rect_filled(Rect::from_center_size(c, Vec2::new(8.0, 4.0) * scale), 0.0, colors[((c.x + c.y) as usize) % 4]);
        }
    }
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
