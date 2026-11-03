//! The chat's category (see `crate::talk`): in the sidebar, the conversations and everyone (online first); the
//! page, a conversation (its messages, the field to write). A click on someone (online or not) opens the
//! conversation with them; a group is made with the people picked.

use std::collections::HashMap;

use super::sidebar::{paint_panel_title, paint_row_bg};
use super::*;
use crate::talk::{Fetched, FileInfo, FileRefusal, Kind, Member, Message, Room, Status};

/// Messages from the same person this close (ms) go under one name.
const GROUPED_MS: i64 = 5 * 60 * 1000;

/// The people picked for a new group, or to add to one.
pub(super) struct Picker {
    /// The group to add them to (None: a new group).
    room: Option<String>,
    name: String,
    chosen: Vec<String>,
    focus: bool,
}

/// What the chat's page keeps (for all windows).
#[derive(Default)]
pub(super) struct TalkUi {
    /// The conversation shown.
    pub room: Option<String>,
    /// What is being written, by conversation.
    drafts: HashMap<String, String>,
    focus: bool,
    picker: Option<Picker>,
    /// A group being renamed: its id, the name typed.
    renaming: Option<(String, String)>,
    /// A group to leave, asked first.
    leaving: Option<String>,
    /// floor refused something: its word, shown a moment.
    pub refused: Option<(String, Instant)>,
    /// The conversation on screen, and for how many frames: older messages are asked only once it
    /// has settled at the bottom (its first frame starts at the top).
    settled: Option<(String, u8)>,
    /// The images shown, by file id.
    textures: HashMap<String, egui::TextureHandle>,
    /// An image open large (its file id).
    viewing: Option<FileInfo>,
    /// A file to save once fetched.
    saving: Option<FileInfo>,
}

/// A colour of its own for each pseudo.
fn tint(theme: &Theme, name: &str) -> Color32 {
    let h = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    [theme.ansi[1], theme.ansi[2], theme.ansi[3], theme.ansi[4], theme.ansi[5], theme.ansi[6]][h as usize % 6]
}

/// A round with the pseudo's first letter (a group: two little rounds).
fn paint_avatar(painter: &egui::Painter, c: Pos2, r: f32, name: &str, group: bool, theme: &Theme) {
    if group {
        painter.circle_filled(c + Vec2::new(-r * 0.35, -r * 0.2), r * 0.62, theme.text_muted.gamma_multiply(0.55));
        painter.circle_filled(c + Vec2::new(r * 0.38, r * 0.25), r * 0.62, theme.text_muted.gamma_multiply(0.8));
        return;
    }
    let color = tint(theme, name);
    painter.circle_filled(c, r, color.gamma_multiply(0.85));
    let letter: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    painter.text(c, Align2::CENTER_CENTER, letter, FontId::proportional(r * 1.05), theme.bg);
}

/// A speech bubble: the chat's category.
pub(super) fn paint_talk_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let body = Rect::from_center_size(c + Vec2::new(0.0, -1.5), Vec2::new(17.0, 12.0));
    painter.rect_stroke(body, 4.0, stroke, egui::StrokeKind::Middle);
    let tail = [Pos2::new(body.min.x + 4.0, body.max.y), Pos2::new(body.min.x + 3.0, body.max.y + 4.5), Pos2::new(body.min.x + 8.5, body.max.y)];
    painter.line_segment([tail[0], tail[1]], stroke);
    painter.line_segment([tail[1], tail[2]], stroke);
    for dx in [-4.0, 0.0, 4.0] {
        painter.circle_filled(body.center() + Vec2::new(dx, 0.0), 1.1, color);
    }
}

/// "14:05", with the day when it isn't today.
fn clock(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at).map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default()
}

fn day_of(at: i64) -> Option<chrono::NaiveDate> {
    chrono::DateTime::from_timestamp_millis(at).map(|d| d.with_timezone(&chrono::Local).date_naive())
}

/// The day's title between messages: today, yesterday, or its date.
fn day_title(t: &Strings, day: chrono::NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    if day == today {
        t.talk_today.to_owned()
    } else if today.pred_opt() == Some(day) {
        t.talk_yesterday.to_owned()
    } else {
        day.format("%d/%m/%Y").to_string()
    }
}

/// A conversation's title, a group alone without a name having one all the same.
fn shown_title(t: &Strings, room: &Room, me: &str) -> String {
    let title = room.title(me);
    if title.is_empty() { t.talk_unnamed_group.to_owned() } else { title }
}

/// What happened in a group, in words.
fn event_text(t: &Strings, m: &Message) -> String {
    let who = m.name.as_deref().unwrap_or("?");
    let text = match m.event.as_deref() {
        Some("created") if m.text.is_empty() => t.talk_event_created_alone,
        Some("created") => t.talk_event_created,
        Some("added") => t.talk_event_added,
        Some("left") => t.talk_event_left,
        Some("renamed") if m.text.is_empty() => t.talk_event_unnamed,
        Some("renamed") => t.talk_event_renamed,
        _ => "{a}",
    };
    text.replace("{a}", who).replace("{b}", &m.text)
}

impl App {
    /// The conversations not read: how many messages, in all.
    pub(super) fn talk_unread(&self) -> usize {
        let Some(me) = &self.talk.me else { return 0 };
        self.talk.rooms.iter().map(|r| r.unread(&me.id, self.config.settings.talk_read.get(&r.id).copied().unwrap_or(0))).sum()
    }

    /// The chat at each frame: connected with a pseudo (unless its category is hidden), the messages
    /// come in, those of a conversation not on screen told of.
    pub(super) fn talk_frame(&mut self, ctx: &egui::Context) {
        let on = !self.config.settings.hidden_categories.contains(&Category::Talk) && !self.read_only;
        let news = self.talk.frame(ctx, self.config.settings.floor.as_ref(), on);
        if news.forgotten {
            if let Some(account) = &mut self.config.settings.floor {
                account.name = None;
            }
            self.save_config();
        }
        if let Some(id) = news.opened {
            self.talk_ui.room = Some(id);
            self.talk_ui.focus = true;
            self.open_talk_page();
        }
        if let Some(e) = news.refused {
            self.talk_ui.refused = Some((e, Instant::now()));
        }
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let shown = |app: &Self, room: &str| app.talk_page && focused && app.talk_ui.room.as_deref() == Some(room);
        let arrived: Vec<(String, Message)> = news.arrived.into_iter().filter(|(room, _)| !shown(self, room)).collect();
        let Some((room, last)) = arrived.last().cloned() else { return };
        let t = self.t();
        let me = self.talk.me.as_ref().map_or("", |m| m.id.as_str());
        let from = last.name.clone().unwrap_or_default();
        let title = match self.talk.room(&room) {
            Some(r) if r.kind == Kind::Group => t.talk_new_in.replace("{n}", &from).replace("{g}", &shown_title(t, r, me)),
            _ => t.talk_new_message.replace("{n}", &from),
        };
        let line = |m: &Message| match &m.file {
            Some(f) => format!("📎 {}", f.name),
            None => m.text.lines().next().unwrap_or("").to_owned(),
        };
        let body: String = if arrived.len() > 1 { arrived.iter().rev().take(3).rev().map(|(_, m)| line(m)).collect::<Vec<_>>().join("\n") } else if last.file.is_some() { line(&last) } else { last.text.clone() };
        let body: String = body.chars().take(240).collect();
        let style = self.config.settings.notify_style;
        if !focused || style.system() {
            crate::notify::send(&title, &body);
        }
        if !focused {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        if style.in_app() {
            self.push_toast(Toast { ok: true, title, body, tab: self.active, at: Instant::now(), mail: None, notice: 0, lead: Some(drive::Lead::Talk(room)) });
        }
    }

    /// The chat's page, in this window: shown instead of the tabs.
    pub(super) fn open_talk_page(&mut self) {
        self.talk_page = true;
        self.modules_page = false;
        self.procs_page = false;
        self.dev_page = false;
        self.music_page = false;
        self.notes_page = false;
        self.home_game = false;
        self.talk_ui.focus = true;
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    /// A conversation shown.
    pub(super) fn open_talk_room(&mut self, room: &str) {
        self.talk_ui.room = Some(room.to_owned());
        self.open_talk_page();
    }

    /// The chat's category in the sidebar: the conversations, then who is online.
    pub(super) fn talk_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let head = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
        paint_panel_title(&painter, head, t.cat_talk, &theme);
        // Its state, on the right: a dot (green: connected).
        let (dot, hint) = match &self.talk.status {
            Status::Ready => (theme.ansi[2], None),
            Status::Connecting => (theme.ansi[3], Some(t.talk_connecting)),
            Status::Down(_) => (theme.ansi[1], Some(t.talk_down)),
            Status::Off => (theme.text_muted.gamma_multiply(0.5), None),
        };
        let dot_at = Pos2::new(head.max.x - 12.0, head.center().y);
        painter.circle_filled(dot_at, 3.5, dot);
        if let Some(hint) = hint {
            ui.interact(Rect::from_center_size(dot_at, Vec2::splat(14.0)), ui.id().with("talk-state"), Sense::hover()).on_hover_text(hint);
        }
        *y += SECTION_HEADER_H;

        // No pseudo: the chat needs one.
        if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_none() {
            let galley = painter.layout(t.talk_need_pseudo.to_owned(), FontId::proportional(12.5), theme.text_muted, row_w - 16.0);
            painter.galley(Pos2::new(left + 8.0, *y + 4.0), galley.clone(), theme.text_muted);
            *y += galley.size().y + 14.0;
            let button = Rect::from_min_size(Pos2::new(left + 8.0, *y), Vec2::new(row_w - 16.0, 30.0));
            if ui.put(button, egui::Button::new(egui::RichText::new(t.talk_pick_pseudo).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0)).clicked() {
                self.ask_pseudo();
            }
            *y += 40.0;
            return;
        }

        let me = self.talk.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        let mut open = None;
        let mut dm = None;
        // The conversations, the most recent first.
        let rooms: Vec<(String, String, bool, usize, bool)> = self
            .talk
            .rooms
            .iter()
            .map(|r| {
                let read = self.config.settings.talk_read.get(&r.id).copied().unwrap_or(0);
                let online = r.peer(&me).is_some_and(|p| self.talk.is_online(&p.id));
                (r.id.clone(), shown_title(t, r, &me), r.kind == Kind::Group, r.unread(&me, read), online)
            })
            .collect();
        for (id, title, group, unread, online) in &rooms {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 4.0));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("talk-room", id.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let active = self.talk_page && self.talk_ui.room.as_deref() == Some(id.as_str());
            paint_row_bg(&painter, slot, active, resp.hovered(), &theme);
            let c = Pos2::new(slot.min.x + 17.0, slot.center().y);
            paint_avatar(&painter, c, 10.0, title, *group, &theme);
            if *online {
                painter.circle_filled(c + Vec2::new(7.5, 7.5), 4.0, theme.chrome_bg);
                painter.circle_filled(c + Vec2::new(7.5, 7.5), 2.8, theme.ansi[2]);
            }
            let strong = *unread > 0;
            let color = if active || resp.hovered() || strong { theme.text } else { theme.text_muted };
            let font = FontId::proportional(13.0);
            let room_for_name = row_w - 36.0 - if strong { 34.0 } else { 8.0 };
            let galley = painter.layout(title.clone(), font, color, f32::INFINITY);
            let galley = if galley.size().x > room_for_name {
                let mut job = egui::text::LayoutJob::simple_singleline(title.clone(), FontId::proportional(13.0), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(room_for_name);
                painter.layout_job(job)
            } else {
                galley
            };
            painter.galley(Pos2::new(slot.min.x + 34.0, slot.center().y - galley.size().y / 2.0), galley, color);
            if strong {
                let text = if *unread > 99 { "99+".to_owned() } else { unread.to_string() };
                let galley = painter.layout_no_wrap(text, FontId::proportional(10.5), theme.bg);
                let pill = Rect::from_center_size(Pos2::new(slot.max.x - 10.0 - (galley.size().x + 10.0).max(18.0) / 2.0, slot.center().y), Vec2::new((galley.size().x + 10.0).max(18.0), 17.0));
                painter.rect_filled(pill, 8.5, theme.accent);
                painter.galley(pill.center() - galley.size() / 2.0, galley, theme.bg);
            }
            if resp.clicked() {
                open = Some(id.clone());
            }
        }
        if rooms.is_empty() && self.talk.ready() {
            painter.text(Pos2::new(left + 10.0, *y + 10.0), Align2::LEFT_CENTER, t.talk_no_rooms, FontId::proportional(12.0), theme.text_muted.gamma_multiply(0.8));
            *y += 24.0;
        }
        // A new group (greyed while the chat isn't connected).
        {
            let ready = self.talk.ready();
            *y += 4.0;
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with("talk-new-group"), if ready { Sense::click() } else { Sense::hover() });
            let resp = if ready { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp.on_hover_text(t.talk_down) };
            paint_row_bg(&painter, slot, false, ready && resp.hovered(), &theme);
            let color = if !ready { theme.text_muted.gamma_multiply(0.45) } else if resp.hovered() { theme.accent } else { theme.text_muted };
            painter.text(Pos2::new(slot.min.x + 17.0, slot.center().y), Align2::CENTER_CENTER, "+", FontId::proportional(17.0), color);
            painter.text(Pos2::new(slot.min.x + 34.0, slot.center().y), Align2::LEFT_CENTER, t.talk_new_group, FontId::proportional(13.0), color);
            if resp.clicked() {
                self.talk_ui.picker = Some(Picker { room: None, name: String::new(), chosen: Vec::new(), focus: true });
                self.open_talk_page();
            }
        }

        // Everyone with a pseudo (the player aside), those online first: a click writes to them, even
        // away (they find it when they come back).
        *y += 10.0;
        paint_panel_title(&painter, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H)), t.talk_online, &theme);
        *y += SECTION_HEADER_H;
        let people = self.talk_people(&me);
        if people.is_empty() {
            let text = if self.talk.ready() { t.talk_nobody } else { t.talk_connecting };
            painter.text(Pos2::new(left + 10.0, *y + 10.0), Align2::LEFT_CENTER, text, FontId::proportional(12.0), theme.text_muted.gamma_multiply(0.8));
            *y += 24.0;
        }
        for (m, online) in &people {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("talk-online", m.id.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.talk_write_to.replace("{n}", &m.name));
            paint_row_bg(&painter, slot, false, resp.hovered(), &theme);
            let c = Pos2::new(slot.min.x + 17.0, slot.center().y);
            paint_avatar(&painter, c, 9.0, &m.name, false, &theme);
            if *online {
                painter.circle_filled(c + Vec2::new(7.0, 7.0), 3.8, theme.chrome_bg);
                painter.circle_filled(c + Vec2::new(7.0, 7.0), 2.6, theme.ansi[2]);
            }
            let color = match (resp.hovered(), *online) {
                (true, _) => theme.text,
                (false, true) => theme.text_muted,
                (false, false) => theme.text_muted.gamma_multiply(0.6),
            };
            painter.text(Pos2::new(slot.min.x + 34.0, slot.center().y), Align2::LEFT_CENTER, &m.name, FontId::proportional(13.0), color);
            if resp.clicked() {
                dm = Some(m.id.clone());
            }
        }
        if let Some(id) = open {
            self.open_talk_room(&id);
        }
        if let Some(id) = dm {
            // Already there: shown at once; else floor opens it.
            let known = self.talk.rooms.iter().find(|r| r.kind == Kind::Dm && r.members.iter().any(|m| m.id == id)).map(|r| r.id.clone());
            match known {
                Some(room) => self.open_talk_room(&room),
                None => self.talk.dm(&id),
            }
        }
    }

    /// The chat's page: the conversation picked, its messages, the field to write.
    pub(super) fn talk_page_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        self.talk_windows(ui.ctx(), t, &theme);
        self.talk_viewer(ui.ctx(), t, &theme);
        self.talk_save(t);
        let me = self.talk.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        let room = self.talk_ui.room.as_ref().and_then(|id| self.talk.room(id)).cloned();
        let Some(room) = room else {
            let text = if crate::floor::pseudo(self.config.settings.floor.as_ref()).is_none() { t.talk_need_pseudo } else { t.talk_empty };
            let galley = ui.painter().layout(text.to_owned(), FontId::proportional(14.0), theme.text_muted, (rect.width() - 80.0).min(420.0));
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, theme.text_muted);
            paint_talk_icon(ui.painter(), rect.center() - Vec2::new(0.0, 48.0), theme.text_muted.gamma_multiply(0.6));
            return;
        };
        let margin = if rect.width() > 760.0 { 28.0 } else { 14.0 };

        // The head: who it is with, and a group's actions.
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 58.0));
        let painter = ui.painter().clone();
        painter.line_segment([Pos2::new(head.min.x, head.max.y), Pos2::new(head.max.x, head.max.y)], Stroke::new(1.0, theme.tab_hover));
        let title = shown_title(t, &room, &me);
        let c = Pos2::new(head.min.x + margin + 14.0, head.center().y);
        paint_avatar(&painter, c, 14.0, &title, room.kind == Kind::Group, &theme);
        let text_x = c.x + 26.0;
        painter.text(Pos2::new(text_x, head.center().y - 8.0), Align2::LEFT_CENTER, &title, FontId::proportional(16.0), theme.text);
        let sub = match room.kind {
            Kind::Dm => match room.peer(&me) {
                Some(p) if self.talk.is_online(&p.id) => (t.talk_online_now.to_owned(), theme.ansi[2]),
                _ => (t.talk_offline.to_owned(), theme.text_muted),
            },
            Kind::Group => {
                let on = room.members.iter().filter(|m| self.talk.is_online(&m.id)).count();
                (t.talk_members.replace("{n}", &room.members.len().to_string()).replace("{o}", &on.to_string()), theme.text_muted)
            }
        };
        let sub_galley = painter.layout_no_wrap(sub.0, FontId::proportional(12.0), sub.1);
        let sub_rect = Rect::from_min_size(Pos2::new(text_x, head.center().y + 2.0), sub_galley.size());
        painter.galley(sub_rect.min, sub_galley, sub.1);
        if room.kind == Kind::Group {
            let names: Vec<String> = room.members.iter().map(|m| if self.talk.is_online(&m.id) { format!("● {}", m.name) } else { format!("○ {}", m.name) }).collect();
            ui.interact(sub_rect, ui.id().with("talk-members"), Sense::hover()).on_hover_text(names.join("\n"));
            // Add, rename, leave.
            let mut x = head.max.x - margin;
            let mut button = |label: &str, key: &str| {
                let galley = painter.layout_no_wrap(label.to_owned(), FontId::proportional(12.5), theme.text);
                let w = galley.size().x + 22.0;
                let r = Rect::from_min_size(Pos2::new(x - w, head.center().y - 14.0), Vec2::new(w, 28.0));
                x -= w + 8.0;
                let resp = ui.interact(r, ui.id().with(("talk-act", key)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                painter.rect_filled(r, 6.0, if resp.hovered() { theme.tab_hover } else { theme.chrome_bg });
                painter.galley(r.center() - galley.size() / 2.0, galley, theme.text);
                resp.clicked()
            };
            if button(t.talk_leave, "leave") {
                self.talk_ui.leaving = Some(room.id.clone());
            }
            if button(t.talk_rename, "rename") {
                self.talk_ui.renaming = Some((room.id.clone(), room.name.clone().unwrap_or_default()));
            }
            if button(t.talk_add, "add") {
                self.talk_ui.picker = Some(Picker { room: Some(room.id.clone()), name: String::new(), chosen: Vec::new(), focus: false });
            }
        }

        // The field, at the bottom: as high as its lines (six at most).
        let draft = self.talk_ui.drafts.entry(room.id.clone()).or_default();
        let lines = draft.lines().count().max(1) + usize::from(draft.ends_with('\n'));
        let field_h = 20.0 + 19.0 * lines.min(6) as f32;
        let foot = Rect::from_min_max(Pos2::new(rect.min.x + margin, rect.max.y - field_h - 44.0), Pos2::new(rect.max.x - margin, rect.max.y));
        let list = Rect::from_min_max(Pos2::new(rect.min.x, head.max.y + 1.0), Pos2::new(rect.max.x, foot.min.y - 4.0));

        // The messages, the newest at the bottom; scrolled to the top, older ones come.
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
        let mut near_top = false;
        let out = egui::ScrollArea::vertical().id_salt(("talk-list", room.id.as_str())).auto_shrink(false).stick_to_bottom(true).show(&mut list_ui, |ui| {
            ui.add_space(10.0);
            ui.set_width(list.width());
            if room.loading {
                ui.vertical_centered(|ui| ui.spinner());
            } else if !room.done {
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(t.talk_older).size(11.5).color(theme.text_muted.gamma_multiply(0.7))));
            }
            self.talk_messages(ui, &room, &me, margin, t, &theme);
            // Those being sent.
            let sending: Vec<String> = self.talk.uploads.lock().map(|u| u.iter().filter(|u| u.room == room.id && u.done.is_none()).map(|u| u.name.clone()).collect()).unwrap_or_default();
            for name in sending {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(margin + 40.0);
                    ui.spinner();
                    ui.label(egui::RichText::new(t.talk_sending.replace("{n}", &name)).size(12.5).color(theme.text_muted));
                });
            }
            ui.add_space(8.0);
        });
        let frames = match &mut self.talk_ui.settled {
            Some((id, n)) if *id == room.id => {
                *n = n.saturating_add(1);
                *n
            }
            _ => {
                self.talk_ui.settled = Some((room.id.clone(), 0));
                0
            }
        };
        if frames > 3 && out.state.offset.y < 40.0 && out.content_size.y > out.inner_rect.height() {
            near_top = true;
        }
        if near_top && !room.done && !room.loading {
            self.talk.more(&room.id);
        }

        // Who is typing, then the field.
        let typing = room.typing_now();
        if !typing.is_empty() {
            let text = if typing.len() == 1 { t.talk_typing_one.replace("{n}", typing[0]) } else { t.talk_typing_many.replace("{n}", &typing.join(", ")) };
            painter.text(Pos2::new(foot.min.x + 4.0, foot.min.y + 8.0), Align2::LEFT_CENTER, text, FontId::proportional(11.5), theme.text_muted);
        }
        let full = Rect::from_min_max(Pos2::new(foot.min.x, foot.min.y + 16.0), Pos2::new(foot.max.x, foot.min.y + 16.0 + field_h));
        // On its left, the button to send an image or a file.
        let attach = Rect::from_min_size(Pos2::new(full.min.x, full.max.y - 38.0), Vec2::splat(38.0));
        let field = Rect::from_min_max(Pos2::new(full.min.x + 44.0, full.min.y), full.max);
        let id = egui::Id::new(("talk-field", room.id.as_str()));
        // Enter sends (Shift + Enter: a new line): taken before the field sees it.
        let focused_before = ui.memory(|m| m.has_focus(id));
        let send = focused_before && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let ready = self.talk.ready();
        let hint = if room.kind == Kind::Group { t.talk_hint_group } else { t.talk_hint }.replace("{n}", &title);
        let draft = self.talk_ui.drafts.entry(room.id.clone()).or_default();
        // Inside the frame drawn around it (a field without a frame of its own has no margin).
        let inner = field.shrink2(Vec2::new(12.0, 9.0));
        let edit = egui::TextEdit::multiline(draft).id(id).hint_text(hint).desired_rows(1).char_limit(crate::talk::TEXT_MAX).font(FontId::proportional(14.0)).desired_width(inner.width()).margin(Vec2::ZERO).frame(egui::Frame::NONE);
        painter.rect(field, 10.0, theme.chrome_bg, Stroke::new(1.0, if focused_before { theme.accent.gamma_multiply(0.6) } else { theme.tab_hover }), egui::StrokeKind::Inside);
        let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        field_ui.set_clip_rect(field.intersect(ui.clip_rect()));
        let resp = egui::ScrollArea::vertical().id_salt(("talk-field-scroll", room.id.as_str())).max_height(inner.height()).show(&mut field_ui, |ui| ui.add_enabled(ready, edit)).inner;
        if std::mem::take(&mut self.talk_ui.focus) {
            resp.request_focus();
        }
        let text = draft.trim().to_owned();
        if resp.changed() && !text.is_empty() {
            self.talk.typing(&room.id);
        }
        if send && !text.is_empty() {
            self.talk.say(&room.id, &text);
            if let Some(d) = self.talk_ui.drafts.get_mut(&room.id) {
                d.clear();
            }
            resp.request_focus();
        }
        let ready = self.talk.ready();
        let attach_resp = ui.interact(attach, ui.id().with(("talk-attach", room.id.as_str())), if ready { Sense::click() } else { Sense::hover() }).on_hover_text(t.talk_attach);
        painter.rect(attach, 10.0, if ready && attach_resp.hovered() { theme.tab_hover } else { theme.chrome_bg }, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let plus = if ready && attach_resp.hovered() { theme.accent } else { theme.text_muted };
        painter.line_segment([attach.center() - Vec2::new(7.0, 0.0), attach.center() + Vec2::new(7.0, 0.0)], Stroke::new(1.8, plus));
        painter.line_segment([attach.center() - Vec2::new(0.0, 7.0), attach.center() + Vec2::new(0.0, 7.0)], Stroke::new(1.8, plus));
        let mut chosen: Vec<std::path::PathBuf> = Vec::new();
        if ready && attach_resp.clicked() {
            let picked = rfd::FileDialog::new().add_filter(t.talk_file_types, &crate::talk::FILE_TYPES).pick_files();
            chosen.extend(picked.unwrap_or_default());
        }
        // Dropped on the page.
        let (hovering, dropped) = ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>()));
        if ready {
            chosen.extend(dropped);
        }
        if hovering && ready {
            painter.rect_filled(rect.shrink(8.0), 14.0, theme.bg.gamma_multiply(0.85));
            painter.rect_stroke(rect.shrink(8.0), 14.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
            painter.text(rect.center(), Align2::CENTER_CENTER, t.talk_drop.replace("{n}", &title), FontId::proportional(17.0), theme.accent);
        }
        for path in chosen {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let refused = match crate::talk::acceptable(&path) {
                Ok(()) => match std::fs::read(&path) {
                    Ok(bytes) => {
                        self.talk.upload(ui.ctx(), &room.id, &name, bytes);
                        None
                    }
                    Err(_) => Some(FileRefusal::Unreadable),
                },
                Err(why) => Some(why),
            };
            if let Some(why) = refused {
                let text = match why {
                    FileRefusal::Type => t.talk_refused_type,
                    FileRefusal::TooBig => t.talk_refused_big,
                    FileRefusal::Empty => t.talk_refused_empty,
                    FileRefusal::Unreadable => t.talk_refused_unreadable,
                };
                self.talk_ui.refused = Some((text.replace("{n}", &name), Instant::now()));
            }
        }
        // ⌘ V with an image and no text on the clipboard (a screenshot): sent as a PNG.
        let pasted = ready && resp.has_focus() && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. } if modifiers.command)));
        if pasted
            && let Ok(mut clipboard) = arboard::Clipboard::new()
            && clipboard.get_text().map_or(true, |s| s.is_empty())
            && let Ok(image) = clipboard.get_image()
            && let Some(rgba) = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())
        {
            let mut png = std::io::Cursor::new(Vec::new());
            if rgba.write_to(&mut png, image::ImageFormat::Png).is_ok() {
                let name = format!("capture-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                self.talk.upload(ui.ctx(), &room.id, &name, png.into_inner());
            }
        }
        // The files sent: those done go; one refused says why.
        if let Ok(mut uploads) = self.talk.uploads.lock() {
            for u in uploads.iter().filter(|u| u.room == room.id) {
                if let Some(Err(e)) = &u.done {
                    self.talk_ui.refused = Some((t.talk_send_failed.replace("{n}", &u.name).replace("{e}", e), Instant::now()));
                }
            }
            uploads.retain(|u| u.done.is_none());
        }

        // Under it: how to send, that no secret goes here; or what floor refused.
        let note_y = field.max.y + 12.0;
        let refused = self.talk_ui.refused.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_secs(6)).map(|(e, _)| e.clone());
        match refused {
            Some(e) => {
                painter.text(Pos2::new(field.min.x + 4.0, note_y), Align2::LEFT_CENTER, e, FontId::proportional(11.5), theme.ansi[1]);
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
            None => {
                let note = if ready { t.talk_send_help } else { t.talk_connecting };
                painter.text(Pos2::new(field.min.x + 4.0, note_y), Align2::LEFT_CENTER, note, FontId::proportional(11.0), theme.text_muted.gamma_multiply(0.75));
                painter.text(Pos2::new(field.max.x - 4.0, note_y), Align2::RIGHT_CENTER, t.talk_no_secrets, FontId::proportional(11.0), theme.text_muted.gamma_multiply(0.6));
            }
        }

        // Read: the window is in front, the conversation on screen.
        if ui.input(|i| i.viewport().focused.unwrap_or(true)) {
            let last = room.last_id();
            let read = self.config.settings.talk_read.entry(room.id.clone()).or_insert(0);
            if last > *read {
                *read = last;
                self.save_config();
            }
        }
    }

    /// The messages of a conversation: the days between them, each person's name over theirs.
    fn talk_messages(&mut self, ui: &mut Ui, room: &Room, me: &str, margin: f32, t: &'static Strings, theme: &Theme) {
        let width = ui.available_width() - margin * 2.0;
        let mut prev: Option<&Message> = None;
        for m in &room.messages {
            let day = day_of(m.at);
            if day.is_some() && prev.and_then(|p| day_of(p.at)) != day {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(day_title(t, day.unwrap_or_default())).size(11.5).color(theme.text_muted.gamma_multiply(0.85))));
                ui.add_space(6.0);
                prev = None;
            }
            if m.event.is_some() {
                ui.add_space(4.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(format!("{}  ·  {}", event_text(t, m), clock(m.at))).size(12.0).italics().color(theme.text_muted.gamma_multiply(0.85)));
                });
                ui.add_space(4.0);
                prev = None;
                continue;
            }
            let grouped = prev.is_some_and(|p| p.from == m.from && m.at - p.at < GROUPED_MS);
            let name = m.name.clone().unwrap_or_default();
            let mine = m.from == me;
            if !grouped {
                ui.add_space(10.0);
            }
            ui.horizontal_top(|ui| {
                ui.add_space(margin);
                // The round of the first of a series; room kept for the others.
                let (avatar, _) = ui.allocate_exact_size(Vec2::new(30.0, if grouped { 1.0 } else { 30.0 }), Sense::hover());
                if !grouped {
                    paint_avatar(ui.painter(), avatar.center(), 14.0, &name, false, theme);
                }
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_max_width(width - 40.0);
                    if !grouped {
                        ui.horizontal(|ui| {
                            let who = if mine { t.talk_you.to_owned() } else { name.clone() };
                            ui.label(egui::RichText::new(who).size(13.0).strong().color(if mine { theme.accent } else { tint(theme, &name) }));
                            ui.label(egui::RichText::new(clock(m.at)).size(11.0).color(theme.text_muted.gamma_multiply(0.75)));
                        });
                    }
                    if let Some(file) = &m.file {
                        self.talk_file(ui, file, t, theme);
                    }
                    if !m.text.is_empty() {
                        let resp = ui.add(egui::Label::new(egui::RichText::new(&m.text).size(14.0).color(theme.text)).wrap().selectable(true));
                        if grouped {
                            resp.on_hover_text(clock(m.at));
                        }
                    }
                });
            });
            prev = Some(m);
        }
    }

    /// A file in a message: an image shown (a click opens it large), a document's card (to save it).
    fn talk_file(&mut self, ui: &mut Ui, file: &FileInfo, t: &'static Strings, theme: &Theme) {
        if file.expired {
            ui.label(egui::RichText::new(format!("📎 {}  ·  {}", file.name, t.talk_expired)).size(12.5).italics().color(theme.text_muted));
            return;
        }
        let ctx = ui.ctx().clone();
        self.talk.fetch(&ctx, file);
        if file.image() {
            match self.talk.files.get(&file.id) {
                Some(Fetched::Done { image: Some(picture), .. }) => {
                    let texture = self.talk_ui.textures.entry(file.id.clone()).or_insert_with(|| ctx.load_texture(format!("talk-{}", file.id), (**picture).clone(), egui::TextureOptions::LINEAR));
                    let size = texture.size_vec2();
                    let scale = (360.0 / size.x).min(260.0 / size.y).min(1.0);
                    let resp = ui.add(egui::Image::new((texture.id(), size * scale)).corner_radius(8.0).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::ZoomIn).on_hover_text(&file.name);
                    if resp.clicked() {
                        self.talk_ui.viewing = Some(file.clone());
                    }
                    return;
                }
                Some(Fetched::Loading) | None => {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(220.0, 140.0), Sense::hover());
                    ui.painter().rect_filled(r, 8.0, theme.chrome_bg);
                    egui::Spinner::new().paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(18.0)));
                    return;
                }
                // Not readable as a picture, or not fetched: a card like the other files.
                _ => {}
            }
        }
        let frame = Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(Vec2::new(12.0, 9.0));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("📄").size(20.0));
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&file.name).size(13.5).color(theme.text));
                    let state = match self.talk.files.get(&file.id) {
                        Some(Fetched::Failed(e)) => e.clone(),
                        _ => super::files::format_size(file.size, t),
                    };
                    ui.label(egui::RichText::new(state).size(11.5).color(theme.text_muted));
                });
                ui.add_space(12.0);
                let ready = matches!(self.talk.files.get(&file.id), Some(Fetched::Done { .. }));
                if ui.add_enabled(ready, egui::Button::new(egui::RichText::new(t.talk_save).size(12.5)).corner_radius(6.0)).clicked() {
                    self.talk_ui.saving = Some(file.clone());
                }
            });
        });
    }

    /// A file saved where the user picks (asked once it is fetched).
    fn talk_save(&mut self, t: &'static Strings) {
        let Some(file) = self.talk_ui.saving.clone() else { return };
        let bytes = match self.talk.files.get(&file.id) {
            Some(Fetched::Done { bytes, .. }) => bytes.clone(),
            Some(Fetched::Loading) | None => return,
            Some(Fetched::Failed(_)) => {
                self.talk_ui.saving = None;
                return;
            }
        };
        self.talk_ui.saving = None;
        if let Some(out) = rfd::FileDialog::new().set_file_name(crate::talk::safe_name(&file.name)).save_file()
            && let Err(e) = std::fs::write(&out, bytes.as_slice())
        {
            self.talk_ui.refused = Some((t.talk_save_failed.replace("{e}", &e.to_string()), Instant::now()));
        }
    }

    /// An image open large, over the page: Escape or a click beside closes it.
    fn talk_viewer(&mut self, ctx: &egui::Context, t: &'static Strings, theme: &Theme) {
        let Some(file) = self.talk_ui.viewing.clone() else { return };
        let Some(texture) = self.talk_ui.textures.get(&file.id).cloned() else {
            self.talk_ui.viewing = None;
            return;
        };
        let screen = ctx.content_rect();
        let frame = Frame::popup(&ctx.global_style()).inner_margin(14.0).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover));
        let (mut close, mut save) = (false, false);
        let modal = egui::Modal::new(egui::Id::new("talk-viewer")).frame(frame).show(ctx, |ui| {
            let size = texture.size_vec2();
            let scale = ((screen.width() - 120.0) / size.x).min((screen.height() - 160.0) / size.y).min(1.0);
            if ui.add(egui::Image::new((texture.id(), size * scale)).corner_radius(6.0).sense(Sense::click())).clicked() {
                close = true;
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&file.name).size(13.0).color(theme.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t.close).clicked() {
                        close = true;
                    }
                    if ui.button(t.talk_save).clicked() {
                        save = true;
                    }
                });
            });
        });
        if save {
            self.talk_ui.saving = Some(file);
            self.talk_ui.viewing = None;
        } else if close || modal.should_close() {
            self.talk_ui.viewing = None;
        }
    }

    /// Those one may write to (the player aside), each with whether they are online: everyone with a
    /// pseudo (with an older floor, those online and those met in a conversation), online first.
    fn talk_people(&self, me: &str) -> Vec<(Member, bool)> {
        let mut people: Vec<(Member, bool)> = Vec::new();
        let rooms = self.talk.rooms.iter().flat_map(|r| r.members.iter());
        for m in self.talk.everyone.iter().chain(self.talk.online.iter()).chain(rooms) {
            if m.id != me && !people.iter().any(|(p, _)| p.id == m.id) {
                people.push((m.clone(), self.talk.is_online(&m.id)));
            }
        }
        people.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase())));
        people
    }

    /// The chat's small windows: a new group (or people to add), a group renamed, a group left.
    fn talk_windows(&mut self, ctx: &egui::Context, t: &'static Strings, theme: &Theme) {
        let me = self.talk.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent));

        if self.talk_ui.picker.is_some() {
            // Those one may pick: everyone, online or not; not those already in the group.
            let already: Vec<String> = self.talk_ui.picker.as_ref().and_then(|p| p.room.as_ref()).and_then(|id| self.talk.room(id)).map(|r| r.members.iter().map(|m| m.id.clone()).collect()).unwrap_or_default();
            let people: Vec<(Member, bool)> = self.talk_people(&me).into_iter().filter(|(m, _)| !already.contains(&m.id)).collect();
            let Some(picker) = &mut self.talk_ui.picker else { return };
            let (mut done, mut close) = (false, false);
            let adding = picker.room.is_some();
            let modal = egui::Modal::new(egui::Id::new("talk-picker")).frame(frame).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(egui::RichText::new(if adding { t.talk_add_title } else { t.talk_new_group }).size(18.0).strong());
                ui.add_space(12.0);
                if !adding {
                    let resp = ui.add(egui::TextEdit::singleline(&mut picker.name).char_limit(crate::talk::NAME_MAX).hint_text(t.talk_group_name).desired_width(f32::INFINITY).margin(Vec2::new(10.0, 7.0)));
                    if std::mem::take(&mut picker.focus) {
                        resp.request_focus();
                    }
                    ui.add_space(12.0);
                }
                ui.label(egui::RichText::new(t.talk_group_who).size(13.0).color(theme.text_muted));
                ui.add_space(6.0);
                if people.is_empty() {
                    ui.label(egui::RichText::new(t.talk_no_one).size(12.5).color(theme.text_muted.gamma_multiply(0.8)));
                }
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (m, online) in &people {
                        let mut on = picker.chosen.contains(&m.id);
                        let label = if *online { format!("{}   ●", m.name) } else { m.name.clone() };
                        if ui.checkbox(&mut on, label).changed() {
                            if on {
                                picker.chosen.push(m.id.clone());
                            } else {
                                picker.chosen.retain(|id| id != &m.id);
                            }
                        }
                    }
                });
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = egui::Button::new(egui::RichText::new(if adding { t.talk_add } else { t.talk_create }).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0));
                    // A new group may start alone: its members are added later.
                    if ui.add_enabled(!adding || !picker.chosen.is_empty(), ok).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
            if modal.should_close() {
                close = true;
            }
            if done {
                match &picker.room {
                    Some(room) => self.talk.add(room, &picker.chosen),
                    None => self.talk.group(picker.name.trim(), &picker.chosen),
                }
                self.talk_ui.picker = None;
            } else if close {
                self.talk_ui.picker = None;
            }
        }

        if let Some((room, name)) = &mut self.talk_ui.renaming {
            let (mut done, mut close) = (false, false);
            let modal = egui::Modal::new(egui::Id::new("talk-rename")).frame(frame).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(t.talk_rename).size(18.0).strong());
                ui.add_space(12.0);
                let resp = ui.add(egui::TextEdit::singleline(name).char_limit(crate::talk::NAME_MAX).hint_text(t.talk_group_name).desired_width(f32::INFINITY).margin(Vec2::new(10.0, 7.0)));
                resp.request_focus();
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.talk_rename).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
            if done {
                self.talk.rename(room, name.trim());
                self.talk_ui.renaming = None;
            } else if close || modal.should_close() {
                self.talk_ui.renaming = None;
            }
        }

        if let Some(room) = self.talk_ui.leaving.clone() {
            let title = self.talk.room(&room).map(|r| shown_title(t, r, &me)).unwrap_or_default();
            let (mut done, mut close) = (false, false);
            let modal = egui::Modal::new(egui::Id::new("talk-leave")).frame(frame).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(t.talk_leave).size(18.0).strong());
                ui.add_space(8.0);
                ui.add(egui::Label::new(egui::RichText::new(t.talk_leave_ask.replace("{n}", &title)).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.talk_leave).size(13.5).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
            if done {
                self.talk.leave(&room);
                self.talk_ui.leaving = None;
                self.talk_ui.room = None;
            } else if close || modal.should_close() {
                self.talk_ui.leaving = None;
            }
        }
    }
}
