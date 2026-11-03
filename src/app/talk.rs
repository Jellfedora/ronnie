//! The chat's category (see `crate::talk`): in the sidebar, the conversations and everyone (online first); the
//! page, a conversation (its messages, the field to write). A click on someone (online or not) opens the
//! conversation with them; a group is made with the people picked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::sidebar::{lerp_color, paint_panel_title, paint_row_bg};
use super::*;
use crate::talk::{Fetched, FileInfo, FileRefusal, Kind, Member, Message, Quote, Room, Status};

/// Messages from the same person this close (ms) go under one name.
const GROUPED_MS: i64 = 5 * 60 * 1000;
/// A message reached from a quote stays lit this long (s).
const JUMP_LIT: f32 = 1.6;

/// Now, in milliseconds since 1970 (as floor counts).
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// How long yet: "40 min", "5 h", "3 j".
fn time_left(t: &Strings, ms: i64) -> String {
    let min = (ms / 60_000).max(1);
    if min < 60 {
        format!("{min} min")
    } else if min < 48 * 60 {
        format!("{} h", min / 60)
    } else {
        format!("{} {}", min / (24 * 60), t.talk_days_short)
    }
}

/// What a quote says of its message: its text, its file, or that it was taken back.
fn quote_text(t: &Strings, q: &Quote) -> String {
    if q.deleted {
        t.talk_deleted.to_owned()
    } else if q.text.is_empty() {
        format!("📎 {}", q.file.as_deref().unwrap_or_default())
    } else {
        q.text.clone()
    }
}

/// What a paste sends rather than puts in the field.
enum Media {
    Files(Vec<std::path::PathBuf>),
    /// An image, as a PNG.
    Image(Vec<u8>),
}

/// The clipboard's files (copied in the Finder, the Explorer), or its image; and whether its text
/// still goes in the field (not a mere link or image name, as a browser copies with an image).
/// Office's copies (Excel cells, Word text) come with a picture of them: only their text goes.
/// `text`: what egui would paste (None: it pastes nothing, the clipboard having no text).
fn clipboard_media(text: Option<&str>) -> Option<(Media, bool)> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if text.is_none() && clipboard.get_text().is_ok_and(|s| !s.is_empty()) {
        return None;
    }
    if let Ok(files) = clipboard.get().file_list()
        && !files.is_empty()
    {
        return Some((Media::Files(files), false));
    }
    let keep_text = match text.map(str::trim) {
        Some(text) => {
            if clipboard.get().html().is_ok_and(|h| h.contains("urn:schemas-microsoft-com:office")) {
                return None;
            }
            let lower = text.to_lowercase();
            let link = !text.contains(char::is_whitespace) && (lower.starts_with("http://") || lower.starts_with("https://") || [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| lower.ends_with(e)));
            !link
        }
        None => false,
    };
    let image = clipboard.get_image().ok()?;
    let rgba = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some((Media::Image(png.into_inner()), keep_text))
}

/// A message found, on one line: from a little before the words looked for, those lit.
fn lit_snippet(text: &str, q: &str, theme: &Theme) -> egui::text::LayoutJob {
    let font = FontId::proportional(13.0);
    let plain = egui::TextFormat { font_id: font.clone(), color: theme.text_muted, ..Default::default() };
    let lit = egui::TextFormat { font_id: font, color: theme.text, background: theme.accent.gamma_multiply(0.35), ..Default::default() };
    let mut job = egui::text::LayoutJob::default();
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let needle: Vec<char> = q.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
    let at = (!needle.is_empty()).then(|| lower.windows(needle.len()).position(|w| w == needle.as_slice())).flatten();
    match at {
        Some(at) => {
            let from = at.saturating_sub(30);
            let before: String = chars[from..at].iter().collect();
            job.append(&if from > 0 { format!("…{before}") } else { before }, 0.0, plain.clone());
            job.append(&chars[at..at + needle.len()].iter().collect::<String>(), 0.0, lit);
            job.append(&chars[at + needle.len()..].iter().collect::<String>(), 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job
}

/// A message its author can change: a text, still there.
fn editable(m: &Message) -> bool {
    !m.deleted && m.event.is_none() && m.file.is_none() && !m.text.is_empty()
}

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
    /// A conversation for two to erase, asked first.
    dropping: Option<String>,
    /// floor refused something: its word, shown a moment.
    pub refused: Option<(String, Instant)>,
    /// The conversation on screen, and for how many frames: older messages are asked only once it
    /// has settled at the bottom (its first frame starts at the top).
    settled: Option<(String, u8)>,
    /// The images shown, by file id.
    textures: HashMap<String, egui::TextureHandle>,
    /// The links' preview pictures shown, by address.
    preview_textures: HashMap<String, egui::TextureHandle>,
    /// The animated GIFs shown, by file id.
    animations: HashMap<String, crate::gif::Animation>,
    /// The GIF picker, open (for a conversation's field), its search, its previews made textures.
    gif_open: Option<GifOpen>,
    gif_search: crate::gif::Search,
    gif_previews: HashMap<String, crate::gif::Animation>,
    /// An image open large (its file id).
    viewing: Option<FileInfo>,
    /// A file to save once fetched.
    saving: Option<FileInfo>,
    /// The emojis' sheet (a texture once used).
    emoji_sheet: crate::emoji::Sheet,
    /// The emoji picker, open: for the field or a message's reaction.
    emoji_open: Option<EmojiOpen>,
    /// The `:name` completion: the name it was for, the line lit, and a name it was closed for (Esc).
    completing: (String, usize),
    completion_closed: Option<String>,
    /// The message being answered, by conversation (quoted over the field).
    replying: HashMap<String, Quote>,
    /// A message being changed: its conversation, its id, and the draft put aside meanwhile.
    editing: Option<(String, i64, String)>,
    /// A message to take back, asked first: its conversation and id.
    deleting: Option<(String, i64)>,
    /// A quote clicked: the message to bring on screen (once), lit a moment.
    jump: Option<(i64, Instant, bool)>,
    /// Claude Code installed, and when it was last looked for.
    claude: Option<(bool, Instant)>,
    /// A message being fixed by Claude: its conversation, its text as sent, the answer once come.
    fixing: Option<(String, String, FixAnswer)>,
    /// The window shaking (a wizz came): since when, and where it was (None: maximized or full
    /// screen, its content shakes instead).
    shake: Option<(Instant, Option<Pos2>)>,
    /// Someone's face shown large (their id).
    avatar_open: Option<String>,
    /// The conversation whose files are shown, all of them.
    media_open: Option<String>,
    /// The menu to pin a message (how long), where it was asked: its conversation, its id, pinned yet.
    pin_menu: Option<(String, i64, Pos2, bool)>,
    /// The pinned message shown in a conversation's band, by conversation (a click goes to the next).
    pin_shown: HashMap<String, usize>,
    /// The search bar, open under a conversation's head.
    search: Option<SearchBar>,
    /// A message found, being brought on screen (older ones asked till it comes): its conversation, its id.
    seeking: Option<(String, i64)>,
    /// Someone being given a name for oneself: their id, the name typed.
    nicknaming: Option<(String, String)>,
    /// The names given were handed to the chat (once, at the first frame).
    nicknamed: bool,
    /// The hover bar shown, for which message, where (it stays while the pointer is on it).
    bar: Option<(i64, Rect)>,
}

/// The search bar of a conversation: what is typed, when, what was asked of floor.
pub(super) struct SearchBar {
    room: String,
    q: String,
    typed_at: f64,
    asked: String,
    focus: bool,
    /// Its results shown (hidden once one is picked, back with the typing).
    results: bool,
}

/// Claude's answer to a fix, once come (None inside: it couldn't answer).
type FixAnswer = Arc<Mutex<Option<Option<String>>>>;

/// What a message's hover bar does besides reacting.
#[derive(Clone, Copy, PartialEq)]
enum Act {
    Pin,
    Reply,
    Edit,
    Delete,
}

/// A small icon for a message's action, drawn around `c`.
fn paint_act(painter: &egui::Painter, c: Pos2, act: Act, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let at = |x: f32, y: f32| c + Vec2::new(x, y);
    match act {
        Act::Pin => {
            // A thumbtack, leaning.
            painter.circle_stroke(at(2.5, -3.5), 3.5, stroke);
            painter.line_segment([at(0.0, -1.0), at(-5.5, 6.0)], stroke);
            painter.line_segment([at(-0.5, -6.5), at(5.5, -0.5)], stroke);
        }
        Act::Reply => {
            painter.add(egui::Shape::line(vec![at(6.0, 6.0), at(6.0, 2.0), at(3.0, -1.0), at(-6.0, -1.0)], stroke));
            painter.add(egui::Shape::line(vec![at(-2.0, -5.0), at(-6.0, -1.0), at(-2.0, 3.0)], stroke));
        }
        Act::Edit => {
            painter.add(egui::Shape::closed_line(vec![at(-5.0, 3.0), at(3.0, -5.0), at(5.0, -3.0), at(-3.0, 5.0), at(-6.0, 6.0)], stroke));
        }
        Act::Delete => {
            painter.line_segment([at(-6.0, -4.0), at(6.0, -4.0)], stroke);
            painter.add(egui::Shape::line(vec![at(-2.0, -4.0), at(-2.0, -6.0), at(2.0, -6.0), at(2.0, -4.0)], stroke));
            painter.add(egui::Shape::line(vec![at(-4.5, -4.0), at(-3.5, 6.0), at(3.5, 6.0), at(4.5, -4.0)], stroke));
        }
    }
}

/// The GIF picker, open for a conversation.
pub(super) struct GifOpen {
    room: String,
    /// Where it hangs (its bottom right).
    at: Pos2,
    query: String,
    /// When the search was last typed (it goes a moment after), and the last one asked.
    typed_at: f64,
    searched: Option<String>,
    focus: bool,
    opened: f64,
}

/// What the emoji picker puts its emoji in.
#[derive(Clone, PartialEq)]
enum EmojiTarget {
    /// The field of this conversation (at its cursor).
    Draft(String),
    /// A reaction to this message of this conversation.
    React(String, i64),
}

pub(super) struct EmojiOpen {
    target: EmojiTarget,
    /// Where it hangs (its bottom right for the field, its top right for a message).
    at: Pos2,
    picker: super::emojis::Picker,
    /// When it opened (the click that opened it doesn't close it).
    opened: f64,
}

/// A colour of its own for each pseudo.
fn tint(theme: &Theme, name: &str) -> Color32 {
    let h = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    [theme.ansi[1], theme.ansi[2], theme.ansi[3], theme.ansi[4], theme.ansi[5], theme.ansi[6]][h as usize % 6]
}

/// A round with the pseudo's first letter (a group: two little rounds).
pub(super) fn paint_avatar(painter: &egui::Painter, c: Pos2, r: f32, name: &str, group: bool, face: Option<&egui::TextureHandle>, theme: &Theme) {
    // Its picture (round), when it has one.
    if let (Some(face), false) = (face, group) {
        let rect = Rect::from_center_size(c, Vec2::splat(r * 2.0));
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.add(egui::epaint::RectShape::filled(rect, r, Color32::WHITE).with_texture(face.id(), uv));
        return;
    }
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

/// "03/11/2026 14:05".
fn clock_day(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()
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
        Some("wizz") => t.talk_event_wizz,
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
        if !self.talk_ui.nicknamed {
            self.talk_ui.nicknamed = true;
            self.talk.set_nicknames(&self.config.settings.talk_nicknames);
        }
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
        let t = self.t();
        // Put in a group: told of, with its members.
        for (room, m) in &news.invited {
            let me = self.talk.me.as_ref().map_or("", |m| m.id.as_str());
            let Some(r) = self.talk.room(room) else { continue };
            let title = t.talk_invited.replace("{n}", m.name.as_deref().unwrap_or("?")).replace("{g}", &shown_title(t, r, me));
            let body = r.members.iter().filter(|x| x.id != me).map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
            self.talk_notify(ctx, focused, room.clone(), title, body);
        }
        // A wizz: told of, and the window shakes.
        // (One's own shakes one's window too, as it did on MSN, but isn't told of.)
        for (room, m) in &news.wizzed {
            let mine = self.talk.me.as_ref().is_some_and(|me| me.id == m.from);
            // Someone else's: Ronnie brought to the front (out of the Dock if it was there), its
            // conversation shown.
            let mut wait = Duration::ZERO;
            if !mine {
                let who = self.talk.name_of(&m.from).or_else(|| m.name.clone()).unwrap_or_else(|| "?".into());
                self.talk_notify(ctx, focused, room.clone(), t.talk_wizzed.replace("{n}", &who), String::new());
                if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    // The shaking once it is back (the system animates it out of the Dock).
                    wait = Duration::from_millis(600);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                // The conversation it came from, on screen: who it is from.
                self.talk_ui.room = Some(room.clone());
                self.open_talk_page();
            }
            let fixed = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false) || i.viewport().maximized.unwrap_or(false));
            let outer = ctx.input(|i| i.viewport().outer_rect).filter(|_| !fixed).map(|r| r.min);
            // Shaking already: from where it was before.
            let origin = match self.talk_ui.shake {
                Some((_, o)) => o,
                None => outer,
            };
            self.talk_ui.shake = Some((Instant::now() + wait, origin));
        }
        self.talk_shake(ctx);
        // Someone left a group: told of, with those still in it.
        for (room, m) in &news.departed {
            let me = self.talk.me.as_ref().map_or("", |m| m.id.as_str());
            let Some(r) = self.talk.room(room) else { continue };
            let who = self.talk.name_of(&m.from).or_else(|| m.name.clone()).unwrap_or_else(|| "?".into());
            let title = t.talk_departed.replace("{n}", &who).replace("{g}", &shown_title(t, r, me));
            let body = r.members.iter().filter(|x| x.id != me && x.id != m.from).map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
            self.talk_notify(ctx, focused, room.clone(), title, body);
        }
        let shown = |app: &Self, room: &str| app.talk_page && focused && app.talk_ui.room.as_deref() == Some(room);
        let arrived: Vec<(String, Message)> = news.arrived.into_iter().filter(|(room, _)| !shown(self, room)).collect();
        let Some((room, last)) = arrived.last().cloned() else { return };
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
        self.talk_notify(ctx, focused, room, title, body);
    }

    /// A wizz sent: floor checks it is unlocked (a minesweeper game won with this account) and says
    /// why not, or when the next one may go.
    fn send_wizz(&mut self, room: &str) {
        self.talk.wizz(room);
    }

    /// The window shaken for 2 seconds after a wizz, harder at first, then put back where it was;
    /// maximized or full screen (it can't move), its content shaken instead.
    fn talk_shake(&mut self, ctx: &egui::Context) {
        const SECONDS: f32 = 2.0;
        let Some((since, origin)) = self.talk_ui.shake else { return };
        // Not yet (the window coming back from the Dock).
        let Some(t) = Instant::now().checked_duration_since(since).map(|d| d.as_secs_f32()) else {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        };
        let offset = if t >= SECONDS {
            self.talk_ui.shake = None;
            Vec2::ZERO
        } else {
            let strength = 14.0 * (1.0 - t / SECONDS);
            ctx.request_repaint();
            Vec2::new((t * 61.0).sin() * strength, (t * 47.0).cos() * strength * 0.6)
        };
        match origin {
            Some(origin) => ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(origin + offset)),
            None => {
                let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
                for layer in layers.into_iter().chain([egui::LayerId::background()]) {
                    ctx.set_transform_layer(layer, egui::emath::TSTransform::from_translation(offset));
                }
            }
        }
    }

    /// Told of, as the user likes (the system's notifications, Ronnie's own), a click leading there.
    fn talk_notify(&mut self, ctx: &egui::Context, focused: bool, room: String, title: String, body: String) {
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
        let rooms: Vec<_> = self
            .talk
            .rooms
            .iter()
            .map(|r| {
                let read = self.config.settings.talk_read.get(&r.id).copied().unwrap_or(0);
                let online = r.peer(&me).is_some_and(|p| self.talk.is_online(&p.id));
                let peer = r.peer(&me).map(|p| (p.id.clone(), self.talk.avatar_of(&p.id)));
                (r.id.clone(), shown_title(t, r, &me), r.kind == Kind::Group, r.unread(&me, read), online, peer)
            })
            .collect();
        for (id, title, group, unread, online, peer) in &rooms {
            let face = peer.as_ref().and_then(|(pid, v)| self.avatars.get(ui.ctx(), self.config.settings.floor.as_ref(), pid, v.as_deref()));
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 4.0));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("talk-room", id.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let active = self.talk_page && self.talk_ui.room.as_deref() == Some(id.as_str());
            paint_row_bg(&painter, slot, active, resp.hovered(), &theme);
            let c = Pos2::new(slot.min.x + 17.0, slot.center().y);
            paint_avatar(&painter, c, 10.0, title, *group, face.as_ref(), &theme);
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
            let face = self.avatars.get(ui.ctx(), self.config.settings.floor.as_ref(), &m.id, m.avatar.as_deref());
            paint_avatar(&painter, c, 9.0, &m.name, false, face.as_ref(), &theme);
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
        let me_id = self.talk.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        self.talk_media_window(ui.ctx(), &me_id, t, &theme);
        self.talk_face_window(ui.ctx(), t, &theme);
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
        // Another conversation shown: the pickers open for the previous one close.
        if self.talk_ui.gif_open.as_ref().is_some_and(|o| o.room != room.id) {
            self.close_gifs();
        }
        if let Some(o) = &self.talk_ui.emoji_open
            && !matches!(&o.target, EmojiTarget::Draft(r) | EmojiTarget::React(r, _) if *r == room.id)
        {
            self.talk_ui.emoji_open = None;
        }

        // The head: who it is with, and a group's actions.
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 58.0));
        let painter = ui.painter().clone();
        painter.line_segment([Pos2::new(head.min.x, head.max.y), Pos2::new(head.max.x, head.max.y)], Stroke::new(1.0, theme.tab_hover));
        let title = shown_title(t, &room, &me);
        let c = Pos2::new(head.min.x + margin + 14.0, head.center().y);
        let face = room.peer(&me).and_then(|p| {
            let v = self.talk.avatar_of(&p.id);
            self.avatars.get(ui.ctx(), self.config.settings.floor.as_ref(), &p.id, v.as_deref())
        });
        paint_avatar(&painter, c, 14.0, &title, room.kind == Kind::Group, face.as_ref(), &theme);
        if let Some(peer) = room.peer(&me) {
            let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(30.0)), ui.id().with("talk-head-face"), Sense::click());
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.talk_ui.avatar_open = Some(peer.id.clone());
            }
        }
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
        // A name given: their pseudo beside it.
        if let Some(pseudo) = room.peer(&me).filter(|_| room.kind == Kind::Dm).and_then(|p| self.talk.pseudo_of(&p.id)) {
            let w = painter.layout_no_wrap(title.clone(), FontId::proportional(16.0), theme.text).size().x;
            painter.text(Pos2::new(text_x + w + 8.0, head.center().y - 7.0), Align2::LEFT_CENTER, t.talk_nickname_pseudo.replace("{n}", pseudo), FontId::proportional(12.0), theme.text_muted.gamma_multiply(0.8));
        }
        // With one person: a name of one's own for them, the files.
        if room.kind == Kind::Dm
            && let Some(peer) = room.peer(&me)
        {
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
            if button(t.talk_nickname, "nickname") {
                let given = self.config.settings.talk_nicknames.get(&peer.id).cloned().unwrap_or_default();
                self.talk_ui.nicknaming = Some((peer.id.clone(), given));
            }
            if button(t.talk_media, "media") {
                self.open_media(&room.id);
            }
            if button(t.talk_search, "search") {
                self.toggle_search(&room.id);
            }
            if button(t.talk_wizz, "wizz") {
                self.send_wizz(&room.id);
            }
            if button(t.talk_erase, "erase") {
                self.talk_ui.dropping = Some(room.id.clone());
            }
        }
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
            if button(t.talk_media, "media") {
                self.open_media(&room.id);
            }
            if button(t.talk_search, "search") {
                self.toggle_search(&room.id);
            }
            if button(t.talk_wizz, "wizz") {
                self.send_wizz(&room.id);
            }
        }

        // The field, at the bottom: as high as its lines (six at most).
        // Its lines as shown (those wrapped too), measured at the last frame.
        let rows_id = egui::Id::new(("talk-field-rows", room.id.as_str()));
        let draft = self.talk_ui.drafts.entry(room.id.clone()).or_default();
        let typed_lines = draft.lines().count().max(1) + usize::from(draft.ends_with('\n'));
        let shown_lines = if draft.is_empty() { 1 } else { ui.ctx().data(|d| d.get_temp::<usize>(rows_id)).unwrap_or(1) };
        let lines = typed_lines.max(shown_lines);
        let field_h = 20.0 + 19.0 * lines.min(6) as f32;
        // Over it, the message answered or being changed.
        let editing = self.talk_ui.editing.as_ref().filter(|(r, ..)| *r == room.id).map(|(_, id, _)| *id);
        let replying = if editing.is_some() { None } else { self.talk_ui.replying.get(&room.id).cloned() };
        let banner_h = if editing.is_some() || replying.is_some() { 40.0 } else { 0.0 };
        let foot = Rect::from_min_max(Pos2::new(rect.min.x + margin, rect.max.y - field_h - 44.0 - banner_h), Pos2::new(rect.max.x - margin, rect.max.y));
        // The search bar, under the head.
        let searching = self.talk_ui.search.as_ref().is_some_and(|s| s.room == room.id);
        let bar_h = if searching { 48.0 } else { 0.0 };
        // The messages pinned (still), in a band over it.
        let pins: Vec<Message> = room.pins.iter().filter(|p| p.pinned.as_ref().is_some_and(|x| x.until > now_ms())).cloned().collect();
        let pins_h = if pins.is_empty() { 0.0 } else { 48.0 };
        let list = Rect::from_min_max(Pos2::new(rect.min.x, head.max.y + 1.0 + pins_h + bar_h), Pos2::new(rect.max.x, foot.min.y - 4.0));
        if !pins.is_empty() {
            let band = Rect::from_min_max(Pos2::new(rect.min.x + margin, head.max.y + 8.0), Pos2::new(rect.max.x - margin, head.max.y + pins_h - 2.0));
            self.talk_pins_band(ui, &room, &pins, band, &me, t, &theme);
        }
        if searching {
            let bar = Rect::from_min_max(Pos2::new(rect.min.x + margin, head.max.y + pins_h + 8.0), Pos2::new(rect.max.x - margin, head.max.y + pins_h + bar_h - 2.0));
            self.talk_search_bar(ui, &room, &title, bar, t, &theme);
        }
        self.talk_pin_menu(ui.ctx(), t, &theme);
        self.talk_seek(&room);

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
            // Over the field's text (the field starts after the attach button).
            painter.text(Pos2::new(foot.min.x + 44.0 + 12.0, foot.min.y + 7.0), Align2::LEFT_CENTER, text, FontId::proportional(11.5), theme.text_muted);
        }
        let full = Rect::from_min_max(Pos2::new(foot.min.x, foot.min.y + 16.0 + banner_h), Pos2::new(foot.max.x, foot.min.y + 16.0 + banner_h + field_h));
        if banner_h > 0.0 {
            let band = Rect::from_min_max(Pos2::new(full.min.x + 44.0, foot.min.y + 16.0), Pos2::new(full.max.x, full.min.y - 4.0));
            painter.rect_filled(band, 8.0, theme.chrome_bg);
            painter.rect_filled(Rect::from_min_size(band.min + Vec2::new(8.0, 7.0), Vec2::new(3.0, band.height() - 14.0)), 1.5, theme.accent);
            let (head, body) = match &replying {
                Some(q) => {
                    let who = if q.from.as_deref() == Some(me.as_str()) { t.talk_you.to_owned() } else { q.name.clone().unwrap_or_default() };
                    (t.talk_replying.replace("{n}", &who), quote_text(t, q))
                }
                None => (t.talk_editing.to_owned(), String::new()),
            };
            let text_w = band.width() - 60.0;
            let head_g = painter.layout(head, FontId::proportional(11.5), theme.accent, f32::INFINITY);
            let one_line = if body.is_empty() { 0.0 } else { 8.0 };
            painter.galley(Pos2::new(band.min.x + 20.0, band.center().y - head_g.size().y / 2.0 - one_line), head_g, theme.accent);
            if !body.is_empty() {
                let mut job = egui::text::LayoutJob::simple_singleline(body.replace('\n', " "), FontId::proportional(12.5), theme.text_muted);
                job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
                let g = painter.layout_job(job);
                painter.galley(Pos2::new(band.min.x + 20.0, band.center().y), g, theme.text_muted);
            }
            // × closes it.
            let close = Rect::from_center_size(Pos2::new(band.max.x - 18.0, band.center().y), Vec2::splat(24.0));
            let resp = ui.interact(close, ui.id().with(("talk-banner-close", room.id.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                painter.circle_filled(close.center(), 12.0, theme.tab_hover);
            }
            let c = if resp.hovered() { theme.text } else { theme.text_muted };
            painter.line_segment([close.center() - Vec2::splat(4.5), close.center() + Vec2::splat(4.5)], Stroke::new(1.5, c));
            painter.line_segment([close.center() + Vec2::new(-4.5, 4.5), close.center() + Vec2::new(4.5, -4.5)], Stroke::new(1.5, c));
            if resp.clicked() {
                self.stop_answering(&room.id);
            }
        }
        // On its left, the button to send an image or a file.
        let attach = Rect::from_min_size(Pos2::new(full.min.x, full.max.y - 38.0), Vec2::splat(38.0));
        let field = Rect::from_min_max(Pos2::new(full.min.x + 44.0, full.min.y), full.max);
        let id = egui::Id::new(("talk-field", room.id.as_str()));
        let focused_before = ui.memory(|m| m.has_focus(id));
        // A `:name` being typed: its list above the field (↑ ↓ to choose, Enter or Tab to put it, Esc
        // to close it), its keys taken before the field sees them.
        let cursor = egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index));
        let typed = self.talk_ui.drafts.get(&room.id).cloned().unwrap_or_default();
        let completing = cursor.filter(|_| focused_before).and_then(|c| super::emojis::completion(&typed, c).map(|(start, q)| (start, c, q)));
        let completing = completing.filter(|(_, _, q)| self.talk_ui.completion_closed.as_deref() != Some(q.as_str()));
        let found: Vec<&'static crate::emoji::Emoji> = completing.as_ref().map(|(_, _, q)| crate::emoji::search(q, 6)).unwrap_or_default();
        let mut put: Option<usize> = None;
        if let Some((_, _, q)) = &completing
            && !found.is_empty()
        {
            if self.talk_ui.completing.0 != *q {
                self.talk_ui.completing = (q.clone(), 0);
            }
            let lit = &mut self.talk_ui.completing.1;
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    *lit = (*lit + 1) % found.len();
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    *lit = (*lit + found.len() - 1) % found.len();
                }
                if !i.modifiers.shift && (i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                    put = Some(*lit);
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    self.talk_ui.completion_closed = Some(q.clone());
                }
            });
            *lit = (*lit).min(found.len() - 1);
        }
        // Esc: the answer or the change given up; ↑ in an empty field: one's last message changed.
        if put.is_none() && focused_before && (editing.is_some() || replying.is_some()) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.stop_answering(&room.id);
        }
        if put.is_none() && focused_before && editing.is_none() && typed.is_empty() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
            && let Some(m) = room.messages.iter().rev().find(|m| m.from == me && editable(m))
        {
            self.start_edit(ui.ctx(), &room.id, m);
        }
        // Enter sends (Shift + Enter: a new line): taken before the field sees it. (egui's keys
        // without modifiers match them with Shift too: Shift is looked at first.)
        let send = put.is_none() && focused_before && ui.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        // ⌘ V with files or an image on the clipboard: sent (their name, a link, kept out of the field).
        // With text, egui says so on the press; with none, only the key let go does.
        let mut pasted_files: Vec<std::path::PathBuf> = Vec::new();
        if self.talk.ready() && focused_before {
            let text = ui.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(s) = e { Some(s.clone()) } else { None }));
            let let_go = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. } if modifiers.command)));
            if (text.is_some() || let_go)
                && let Some((media, keep_text)) = clipboard_media(text.as_deref())
            {
                if !keep_text {
                    ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Paste(_))));
                }
                match media {
                    Media::Files(files) => pasted_files = files,
                    Media::Image(png) => {
                        let name = format!("capture-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                        self.talk.upload(ui.ctx(), &room.id, &name, png);
                    }
                }
            }
        }
        let ready = self.talk.ready();
        let hint = if room.kind == Kind::Group { t.talk_hint_group } else { t.talk_hint }.replace("{n}", &title);
        let draft = self.talk_ui.drafts.entry(room.id.clone()).or_default();
        // Inside the frame drawn around it (a field without a frame of its own has no margin), the
        // emoji button on its right.
        let inner = Rect::from_min_max(field.min + Vec2::new(12.0, 9.0), field.max - Vec2::new(112.0, 9.0));
        let sheet = self.talk_ui.emoji_sheet.texture(ui.ctx()).cloned();
        painter.rect(field, 10.0, theme.chrome_bg, Stroke::new(1.0, if focused_before { theme.accent.gamma_multiply(0.6) } else { theme.tab_hover }), egui::StrokeKind::Inside);
        let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        field_ui.set_clip_rect(field.intersect(ui.clip_rect()));
        let resp = egui::ScrollArea::vertical()
            .id_salt(("talk-field-scroll", room.id.as_str()))
            .max_height(inner.height())
            .show(&mut field_ui, |ui| {
                let output = ui.add_enabled_ui(ready, |ui| egui::TextEdit::multiline(&mut *draft).id(id).hint_text(hint).desired_rows(1).char_limit(crate::talk::TEXT_MAX).font(FontId::proportional(14.0)).desired_width(inner.width()).margin(Vec2::ZERO).frame(egui::Frame::NONE).show(ui)).inner;
                let rows = output.galley.rows.len().max(1);
                if ui.ctx().data(|d| d.get_temp::<usize>(rows_id)) != Some(rows) {
                    ui.ctx().data_mut(|d| d.insert_temp(rows_id, rows));
                    ui.ctx().request_repaint();
                }
                // Its emojis in colour, over egui's black and white ones.
                if let Some(sheet) = &sheet {
                    super::emojis::paint_field_emojis(ui.painter(), &output, draft, sheet, theme.chrome_bg);
                }
                output.response
            })
            .inner;
        // The emoji button: the picker, above it.
        let smiley = Rect::from_min_size(Pos2::new(field.max.x - 38.0, field.max.y - 36.0), Vec2::splat(30.0));
        let smiley_resp = ui.interact(smiley, ui.id().with(("talk-emoji", room.id.as_str())), if ready { Sense::click() } else { Sense::hover() }).on_hover_text(t.talk_emoji);
        if let (Some(sheet), Some(cell)) = (&sheet, crate::emoji::cell("😊")) {
            let lit = smiley_resp.hovered() || self.talk_ui.emoji_open.as_ref().is_some_and(|o| o.target == EmojiTarget::Draft(room.id.clone()));
            if lit {
                painter.circle_filled(smiley.center(), 14.0, theme.tab_hover);
            }
            sheet.paint_tinted(&painter, cell, Rect::from_center_size(smiley.center(), Vec2::splat(if lit { 21.0 } else { 19.0 })), if lit { Color32::WHITE } else { Color32::from_white_alpha(150) });
        }
        if ready && smiley_resp.clicked() {
            let now = ui.input(|i| i.time);
            self.talk_ui.emoji_open = match &self.talk_ui.emoji_open {
                Some(o) if o.target == EmojiTarget::Draft(room.id.clone()) => None,
                _ => Some(EmojiOpen { target: EmojiTarget::Draft(room.id.clone()), at: Pos2::new(field.max.x, field.min.y - 6.0), picker: super::emojis::Picker { focus: true, ..Default::default() }, opened: now }),
            };
            // (The GIF picker closed: written out, the draft being borrowed here.)
            self.talk_ui.gif_open = None;
            self.talk_ui.gif_search.clear();
            self.talk_ui.gif_previews.clear();
        }
        // The GIF button, before it: the GIF picker.
        let gif_button = Rect::from_min_size(Pos2::new(field.max.x - 74.0, field.max.y - 33.0), Vec2::new(34.0, 24.0));
        let gif_resp = ui.interact(gif_button, ui.id().with(("talk-gif", room.id.as_str())), if ready { Sense::click() } else { Sense::hover() }).on_hover_text(t.talk_gif);
        let lit = gif_resp.hovered() || self.talk_ui.gif_open.as_ref().is_some_and(|o| o.room == room.id);
        painter.rect(gif_button, 6.0, if lit { theme.tab_hover } else { Color32::TRANSPARENT }, Stroke::new(1.3, if lit { theme.accent } else { theme.text_muted.gamma_multiply(0.7) }), egui::StrokeKind::Inside);
        painter.text(gif_button.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(11.5), if lit { theme.accent } else { theme.text_muted });
        if ready && gif_resp.clicked() {
            if self.talk_ui.gif_open.as_ref().is_some_and(|o| o.room == room.id) {
                self.talk_ui.gif_open = None;
                self.talk_ui.gif_search.clear();
                self.talk_ui.gif_previews.clear();
            } else {
                let now = ui.input(|i| i.time);
                self.talk_ui.emoji_open = None;
                self.talk_ui.gif_open = Some(GifOpen { room: room.id.clone(), at: Pos2::new(field.max.x, field.min.y - 6.0), query: String::new(), typed_at: 0.0, searched: None, focus: true, opened: now });
            }
        }
        // The completion's list, and the emoji chosen in it put in place of the name (once the field
        // is done with the draft).
        let mut completed: Option<(String, usize, usize)> = None;
        if let (Some((start, end, _)), Some(sheet)) = (&completing, &sheet)
            && !found.is_empty()
        {
            let skin = self.config.settings.emoji_skin;
            if let Some(k) = super::emojis::completion_list(ui, Pos2::new(field.min.x, field.min.y - 6.0), &found, self.talk_ui.completing.1, skin, sheet, &theme) {
                put = Some(k);
            }
            if let Some(k) = put {
                completed = Some((found[k].toned(skin).0.to_owned(), *start, *end));
            }
        }
        if std::mem::take(&mut self.talk_ui.focus) {
            resp.request_focus();
        }
        let text = draft.trim().to_owned();
        // Claude's button, before the GIF one: the message's spelling and phrasing fixed (greyed without Claude Code).
        let fix_button = Rect::from_min_size(Pos2::new(field.max.x - 106.0, field.max.y - 33.0), Vec2::new(28.0, 24.0));
        self.talk_fix_button(ui, &room.id, fix_button, t, &theme);
        if resp.changed() && !text.is_empty() {
            self.talk.typing(&room.id);
        }
        if send && !text.is_empty() {
            match self.talk_ui.editing.take() {
                Some((r, id, aside)) if r == room.id => {
                    self.talk.edit(&room.id, id, &text);
                    self.talk_ui.drafts.insert(room.id.clone(), aside);
                }
                other => {
                    self.talk_ui.editing = other;
                    let reply = self.talk_ui.replying.remove(&room.id).map(|q| q.id);
                    self.talk.say(&room.id, &text, reply);
                    if let Some(d) = self.talk_ui.drafts.get_mut(&room.id) {
                        d.clear();
                    }
                }
            }
            resp.request_focus();
        }
        if let Some((chars, start, end)) = completed {
            self.put_emoji(ui.ctx(), &room.id, &chars, Some((start, end)));
        }
        let ready = self.talk.ready();
        let attach_resp = ui.interact(attach, ui.id().with(("talk-attach", room.id.as_str())), if ready { Sense::click() } else { Sense::hover() }).on_hover_text(t.talk_attach);
        painter.rect(attach, 10.0, if ready && attach_resp.hovered() { theme.tab_hover } else { theme.chrome_bg }, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let plus = if ready && attach_resp.hovered() { theme.accent } else { theme.text_muted };
        painter.line_segment([attach.center() - Vec2::new(7.0, 0.0), attach.center() + Vec2::new(7.0, 0.0)], Stroke::new(1.8, plus));
        painter.line_segment([attach.center() - Vec2::new(0.0, 7.0), attach.center() + Vec2::new(0.0, 7.0)], Stroke::new(1.8, plus));
        let mut chosen: Vec<std::path::PathBuf> = pasted_files;
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
        // The files sent: those done go; one refused says why.
        if let Ok(mut uploads) = self.talk.uploads.lock() {
            for u in uploads.iter().filter(|u| u.room == room.id) {
                if let Some(Err(e)) = &u.done {
                    self.talk_ui.refused = Some((t.talk_send_failed.replace("{n}", &u.name).replace("{e}", e), Instant::now()));
                }
            }
            uploads.retain(|u| u.done.is_none());
        }

        // The emoji picker, open (for the field or a reaction), and the GIF picker.
        self.talk_emoji_picker(ui.ctx(), t, &theme);
        self.talk_gif_picker(ui.ctx(), t, &theme);

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
            // The others told (floor shows them "Seen").
            self.talk.read(&room.id, last);
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
        let sheet = self.talk_ui.emoji_sheet.texture(ui.ctx()).cloned();
        // A quote's message not here (gone with the older ones): nothing to light.
        if self.talk_ui.jump.is_some_and(|(_, at, _)| at.elapsed().as_secs_f32() > JUMP_LIT) {
            self.talk_ui.jump = None;
        }
        let mut prev: Option<&Message> = None;
        for (k, m) in room.messages.iter().enumerate() {
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
                    // A wizz stands out: in the accent, bold.
                    let text = egui::RichText::new(format!("{}  ·  {}", event_text(t, m), clock(m.at))).size(12.0).italics();
                    ui.label(if m.event.as_deref() == Some("wizz") { text.strong().color(theme.accent) } else { text.color(theme.text_muted.gamma_multiply(0.85)) });
                });
                ui.add_space(4.0);
                prev = None;
                continue;
            }
            let grouped = prev.is_some_and(|p| p.from == m.from && m.at - p.at < GROUPED_MS);
            // The next one of the same series: this bubble's corner on its author's side, below, stays tight.
            let continues = room.messages.get(k + 1).is_some_and(|n| n.event.is_none() && n.from == m.from && n.at - m.at < GROUPED_MS && day_of(n.at) == day);
            let mine = m.from == me;
            ui.add_space(if grouped { 2.0 } else { 12.0 });
            let bubble_max = (width * 0.72).max(240.0);
            let row = ui.horizontal_top(|ui| {
                ui.add_space(margin);
                if mine {
                    // Mine on the right.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_space(margin);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                            ui.set_max_width(bubble_max);
                            self.talk_bubble(ui, room, m, me, (grouped, continues), sheet.as_ref(), t, theme)
                        })
                        .inner
                    })
                    .inner
                } else {
                    // The others' on the left, their face at the first of a series (room kept for the others).
                    let (avatar, _) = ui.allocate_exact_size(Vec2::new(30.0, if grouped { 1.0 } else { 30.0 }), Sense::hover());
                    if !grouped {
                        let name = m.name.clone().unwrap_or_default();
                        let v = self.talk.avatar_of(&m.from);
                        let face = self.avatars.get(ui.ctx(), self.config.settings.floor.as_ref(), &m.from, v.as_deref());
                        let c = avatar.center() + Vec2::new(0.0, if room.kind == Kind::Group { 16.0 } else { 12.0 });
                        paint_avatar(ui.painter(), c, 14.0, &name, false, face.as_ref(), theme);
                        // A click shows it large.
                        let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(30.0)), ui.id().with(("talk-face", m.id)), Sense::click());
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.talk_ui.avatar_open = Some(m.from.clone());
                        }
                    }
                    ui.add_space(8.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_max_width(bubble_max);
                        self.talk_bubble(ui, room, m, me, (grouped, continues), sheet.as_ref(), t, theme)
                    })
                    .inner
                }
            });
            let (head, column) = row.inner;
            // Under one's last message, who saw it.
            if mine && k + 1 == room.messages.len() {
                let seen: Vec<String> = room.reads.iter().filter(|(id, at)| *id != me && **at >= m.id).map(|(id, _)| self.talk.name_of(id).unwrap_or_else(|| "?".into())).collect();
                if !seen.is_empty() {
                    let others = room.members.iter().filter(|x| x.id != me).count();
                    let text = if room.kind == Kind::Dm || seen.len() >= others { t.talk_seen.to_owned() } else { t.talk_seen_by.replace("{n}", &seen.join(", ")) };
                    ui.add_space(2.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_space(margin + 4.0);
                        ui.label(egui::RichText::new(text).size(10.5).color(theme.text_muted.gamma_multiply(0.8)));
                    });
                }
            }
            // Hovered (across its line, under its name and time): its bar, to react at once (the usual ones, or any from
            // the picker), to answer it, and one's own to change or take back. It stays while the pointer
            // is on it.
            let row = row.response.rect;
            let band = Rect::from_x_y_ranges(ui.max_rect().x_range(), (row.min.y + head)..=row.max.y);
            if self.talk_ui.jump.is_some_and(|(id, ..)| id == m.id) {
                self.light_jump(ui, row, theme);
            }
            let pointer = ui.input(|i| i.pointer.hover_pos());
            let on_bar = self.talk_ui.bar.is_some_and(|(id, r)| id == m.id && pointer.is_some_and(|p| r.expand(2.0).contains(p)));
            // Nothing over it there (a window, a menu): the conversation's layer, or the bar's own.
            let bar_layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(("react-bar", m.id)));
            let on_top = |p: Pos2| ui.ctx().layer_id_at(p).is_none_or(|l| l == ui.layer_id() || l == bar_layer);
            let hovered = pointer.is_some_and(|p| (band.contains(p) || on_bar) && ui.clip_rect().contains(p) && on_top(p));
            if self.talk.ready() && self.talk_ui.emoji_open.is_none() && self.talk_ui.gif_open.is_none() && self.talk_ui.pin_menu.is_none() && !m.deleted && hovered {
                let acts: Vec<Act> = if mine && editable(m) { vec![Act::Pin, Act::Reply, Act::Edit, Act::Delete] } else if mine { vec![Act::Pin, Act::Reply, Act::Delete] } else { vec![Act::Pin, Act::Reply] };
                let when = if day_of(m.at) == Some(chrono::Local::now().date_naive()) { clock(m.at) } else { clock_day(m.at) };
                let when_w = ui.painter().layout_no_wrap(when.clone(), FontId::proportional(11.5), theme.text_muted).size().x + 14.0;
                let bar = Vec2::new(7.0 * 30.0 + 10.0 + 9.0 + acts.len() as f32 * 30.0 + when_w, 36.0);
                // Beside the bubble, on its free side (mine on its left, the others' on its right), inside the page.
                let x = if mine { (column.min.x - 8.0 - bar.x).max(band.min.x + margin) } else { (column.max.x + 8.0).min(band.max.x - margin - bar.x) };
                let at = Pos2::new(x, band.min.y - 6.0);
                let mut open_picker = false;
                let mut chosen: Option<Act> = None;
                let shown = egui::Area::new(egui::Id::new(("react-bar", m.id))).order(egui::Order::Foreground).fixed_pos(at).constrain(true).show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).inner_margin(egui::Margin::symmetric(5, 3)).corner_radius(18.0).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for q in super::emojis::QUICK {
                                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().circle_filled(rect.center(), 14.0, theme.tab_hover);
                                }
                                if let (Some(sheet), Some(cell)) = (&sheet, crate::emoji::cell(q)) {
                                    let side = if resp.hovered() { 22.0 } else { 19.0 };
                                    sheet.paint(ui.painter(), cell, Rect::from_center_size(rect.center(), Vec2::splat(side)));
                                }
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    let own = m.reactions.iter().any(|r| crate::emoji::cell(&r.emoji) == crate::emoji::cell(q) && r.ids.iter().any(|id| id == me));
                                    self.talk.react(&room.id, m.id, q, !own);
                                }
                            }
                            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().circle_filled(rect.center(), 14.0, theme.tab_hover);
                            }
                            let c = if resp.hovered() { theme.accent } else { theme.text_muted };
                            ui.painter().line_segment([rect.center() - Vec2::new(6.0, 0.0), rect.center() + Vec2::new(6.0, 0.0)], Stroke::new(1.6, c));
                            ui.painter().line_segment([rect.center() - Vec2::new(0.0, 6.0), rect.center() + Vec2::new(0.0, 6.0)], Stroke::new(1.6, c));
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.talk_react).clicked() {
                                open_picker = true;
                            }
                            // A thin line, then the actions.
                            let (sep, _) = ui.allocate_exact_size(Vec2::new(5.0, 28.0), Sense::hover());
                            ui.painter().line_segment([sep.center_top() + Vec2::new(0.0, 6.0), sep.center_bottom() - Vec2::new(0.0, 6.0)], Stroke::new(1.0, theme.tab_hover));
                            for act in &acts {
                                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().circle_filled(rect.center(), 14.0, theme.tab_hover);
                                }
                                let c = match (act, resp.hovered()) {
                                    (Act::Delete, true) => theme.ansi[1],
                                    (_, true) => theme.accent,
                                    _ => theme.text_muted,
                                };
                                paint_act(ui.painter(), rect.center(), *act, c);
                                let tip = match act {
                                    Act::Pin if m.pinned.is_some() => t.talk_unpin,
                                    Act::Pin => t.talk_pin,
                                    Act::Reply => t.talk_reply,
                                    Act::Edit => t.talk_edit,
                                    Act::Delete => t.talk_delete,
                                };
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
                                    chosen = Some(*act);
                                }
                            }
                            // When it was sent (no tooltip over the message: it would cover its reactions).
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&when).size(11.5).color(theme.text_muted));
                            ui.add_space(6.0);
                        });
                    });
                });
                self.talk_ui.bar = Some((m.id, shown.response.rect));
                match chosen {
                    Some(Act::Pin) => {
                        let at = ui.input(|i| i.pointer.interact_pos()).unwrap_or(at);
                        self.talk_ui.pin_menu = Some((room.id.clone(), m.id, at, m.pinned.is_some()));
                    }
                    Some(Act::Reply) => {
                        self.stop_answering(&room.id);
                        self.talk_ui.replying.insert(room.id.clone(), Quote::of(m));
                        self.talk_ui.focus = true;
                    }
                    Some(Act::Edit) => self.start_edit(ui.ctx(), &room.id, m),
                    Some(Act::Delete) => self.talk_ui.deleting = Some((room.id.clone(), m.id)),
                    None => {}
                }
                if open_picker {
                    let now = ui.input(|i| i.time);
                    self.talk_ui.emoji_open = Some(EmojiOpen { target: EmojiTarget::React(room.id.clone(), m.id), at: Pos2::new(at.x + 7.0 * 30.0, band.min.y + 30.0), picker: super::emojis::Picker { focus: true, ..Default::default() }, opened: now });
                }
            } else if self.talk_ui.bar.is_some_and(|(id, _)| id == m.id) {
                self.talk_ui.bar = None;
            }
            prev = Some(m);
        }
    }

    /// A message's column, aligned on its author's side by the caller: the series' head (the name in a
    /// group, the time), its file, its text in a bubble (big emojis without one), its reactions.
    /// `series`: whether it follows one of the same author's, and whether one follows it.
    #[allow(clippy::too_many_arguments)]
    /// Returns the height of its head (the name, the time): the message itself is under it; and
    /// the room it all took (the bubble, its file, its reactions).
    fn talk_bubble(&mut self, ui: &mut Ui, room: &Room, m: &Message, me: &str, series: (bool, bool), sheet: Option<&crate::emoji::Faces>, t: &'static Strings, theme: &Theme) -> (f32, Rect) {
        let (grouped, continues) = series;
        let top = ui.cursor().min.y;
        let mine = m.from == me;
        let name = m.name.clone().unwrap_or_default();
        if !grouped {
            if mine {
                ui.label(egui::RichText::new(clock(m.at)).size(11.0).color(theme.text_muted.gamma_multiply(0.75)));
            } else {
                ui.horizontal(|ui| {
                    if room.kind == Kind::Group {
                        ui.label(egui::RichText::new(&name).size(12.5).strong().color(tint(theme, &name)));
                    }
                    ui.label(egui::RichText::new(clock(m.at)).size(11.0).color(theme.text_muted.gamma_multiply(0.75)));
                });
            }
            ui.add_space(2.0);
        }
        let head = ui.cursor().min.y - top;
        // Taken back: only its place left.
        if m.deleted {
            egui::Frame::new()
                .stroke(Stroke::new(1.0, theme.tab_hover))
                .corner_radius(16)
                .inner_margin(egui::Margin::symmetric(12, 7))
                .show(ui, |ui| ui.label(egui::RichText::new(t.talk_deleted).size(13.5).italics().color(theme.text_muted)));
            return (head, ui.min_rect());
        }
        // The message it answers, over it (a click brings it on screen).
        if let Some(q) = &m.reply {
            self.talk_quote(ui, m.id, q, me, t, theme);
            ui.add_space(2.0);
        }
        if let Some(file) = &m.file {
            self.talk_file(ui, file, t, theme);
        }
        if !m.text.is_empty() {
            if crate::emoji::only_emojis(&m.text).is_some() {
                super::emojis::emoji_label(ui, sheet, &m.text, 14.0, theme.text);
            } else {
                // Round, tighter on its author's side where its series goes on.
                let (round, tight) = (16, 5);
                let (top, bottom) = (if grouped { tight } else { round }, if continues { tight } else { round });
                let corners = if mine { egui::CornerRadius { nw: round, ne: top, sw: round, se: bottom } } else { egui::CornerRadius { nw: top, ne: round, sw: bottom, se: round } };
                let (fill, ink) = if mine { (theme.accent, Color32::WHITE) } else { (lerp_color(theme.bg, theme.text, 0.10), theme.text) };
                // As wide as its longest line (no wider than the room left), its text on the left: a
                // region of that width, laid out from the left, wherever the column puts it.
                let font = FontId::proportional(14.0);
                let longest = m.text.lines().map(|l| ui.painter().layout_no_wrap(l.to_owned(), font.clone(), ink).size().x).fold(0.0, f32::max);
                let inner = (longest + 6.0).min(ui.available_width() - 24.0).max(8.0);
                ui.allocate_ui_with_layout(Vec2::new(inner + 24.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::Frame::new()
                        .fill(fill)
                        .corner_radius(corners)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .show(ui, |ui| {
                            ui.set_max_width(inner);
                            super::emojis::emoji_label(ui, sheet, &m.text, 14.0, ink)
                        });
                });
            }
        }
        // Its first link, previewed under it (a click opens it).
        if let Some((link, _)) = crate::talk::links(&m.text).into_iter().find(|(_, l)| *l) {
            self.talk_link_card(ui, link, theme);
        }
        let marks: Vec<&str> = [m.edited.map(|_| t.talk_edited), m.pinned.as_ref().filter(|p| p.until > now_ms()).map(|_| t.talk_pinned)].into_iter().flatten().collect();
        if !marks.is_empty() {
            ui.label(egui::RichText::new(marks.join(" · ")).size(10.5).italics().color(theme.text_muted.gamma_multiply(0.75))).on_hover_text(m.edited.map(clock).unwrap_or_default());
        }
        // Its reactions, under it on its side: each emoji and how many; a click adds or takes back one's own.
        if !m.reactions.is_empty() {
            ui.add_space(3.0);
            let side = if mine { egui::Layout::right_to_left(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) };
            ui.with_layout(side, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for r in &m.reactions {
                    let own = r.ids.iter().any(|id| id == me);
                    let count = r.ids.len().to_string();
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(36.0 + 7.5 * count.len() as f32, 24.0), Sense::click());
                    let (fill, edge) = if own { (theme.accent.gamma_multiply(0.18), theme.accent.gamma_multiply(0.7)) } else { (theme.tab_hover.gamma_multiply(if resp.hovered() { 1.0 } else { 0.55 }), theme.tab_hover) };
                    ui.painter().rect(rect, 12.0, fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
                    if let (Some(sheet), Some(cell)) = (sheet, crate::emoji::cell(&r.emoji)) {
                        sheet.paint(ui.painter(), cell, Rect::from_center_size(Pos2::new(rect.min.x + 16.0, rect.center().y), Vec2::splat(16.0)));
                    }
                    ui.painter().text(Pos2::new(rect.max.x - 9.0, rect.center().y), Align2::RIGHT_CENTER, &count, FontId::proportional(12.0), if own { theme.accent } else { theme.text_muted });
                    let who: Vec<String> = r.ids.iter().map(|id| if id == me { t.talk_you.to_owned() } else { self.talk.name_of(id).unwrap_or_else(|| "?".into()) }).collect();
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(who.join(", ")).clicked() {
                        self.talk.react(&room.id, m.id, &r.emoji, !own);
                    }
                }
            });
        }
        (head, ui.min_rect())
    }

    /// A link's card: its picture, site, title, description (as floor read them); a click opens it.
    fn talk_link_card(&mut self, ui: &mut Ui, link: &str, theme: &Theme) {
        let ctx = ui.ctx().clone();
        let Some(p) = self.talk.preview(&ctx, link).cloned() else { return };
        let width = ui.available_width().min(360.0);
        let texture = p.image.as_ref().map(|image| self.talk_ui.preview_textures.entry(link.to_owned()).or_insert_with(|| ctx.load_texture(format!("preview-{link}"), (**image).clone(), egui::TextureOptions::LINEAR)).clone());
        ui.add_space(4.0);
        let shown = ui.allocate_ui_with_layout(Vec2::new(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12).inner_margin(10).show(ui, |ui| {
                ui.set_width(width - 20.0);
                if let Some(texture) = &texture {
                    let size = texture.size_vec2();
                    let scale = ((width - 20.0) / size.x).min(200.0 / size.y);
                    ui.add(egui::Image::new((texture.id(), size * scale)).corner_radius(8.0));
                    ui.add_space(6.0);
                }
                if let Some(site) = &p.site {
                    ui.add(egui::Label::new(egui::RichText::new(site).size(11.0).color(theme.text_muted)).truncate().selectable(false));
                }
                if let Some(title) = &p.title {
                    ui.add(egui::Label::new(egui::RichText::new(title).size(13.5).strong().color(theme.text)).wrap().selectable(false));
                }
                if let Some(description) = &p.description {
                    ui.add(egui::Label::new(egui::RichText::new(description).size(12.0).color(theme.text_muted)).truncate().selectable(false));
                }
            })
        });
        let rect = shown.inner.response.rect;
        let resp = ui.interact(rect, ui.id().with(("talk-link", link)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter().rect_stroke(rect, 12.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);
        }
        if resp.clicked() {
            crate::terminal::open_url(link);
        }
    }

    /// A message quoted over the one answering it: who wrote it, its text on one line.
    fn talk_quote(&mut self, ui: &mut Ui, of: i64, q: &Quote, me: &str, t: &'static Strings, theme: &Theme) {
        let who = if q.from.as_deref() == Some(me) { t.talk_you.to_owned() } else { q.from.as_deref().and_then(|id| self.talk.name_of(id)).or_else(|| q.name.clone()).unwrap_or_default() };
        let frame = egui::Frame::new().fill(lerp_color(theme.bg, theme.text, 0.05)).corner_radius(12).inner_margin(egui::Margin { left: 16, right: 12, top: 5, bottom: 6 }).show(ui, |ui| {
            ui.label(egui::RichText::new(&who).size(11.5).strong().color(theme.text_muted));
            let text = egui::RichText::new(quote_text(t, q).replace('\n', " ")).size(12.5).color(theme.text_muted);
            ui.add(egui::Label::new(if q.deleted { text.italics() } else { text }).truncate().selectable(false));
        });
        let rect = frame.response.rect;
        ui.painter().rect_filled(Rect::from_min_size(rect.min + Vec2::new(7.0, 6.0), Vec2::new(2.5, rect.height() - 12.0)), 1.25, theme.accent.gamma_multiply(0.7));
        if !q.deleted {
            let resp = ui.interact(rect, ui.id().with(("talk-quote", of)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                self.talk_ui.jump = Some((q.id, Instant::now(), false));
            }
        }
    }

    /// A quote clicked: its message brought on screen (once), lit a moment.
    fn light_jump(&mut self, ui: &mut Ui, row: Rect, theme: &Theme) {
        let Some((_, at, scrolled)) = &mut self.talk_ui.jump else { return };
        if !*scrolled {
            ui.scroll_to_rect(row, Some(egui::Align::Center));
            *scrolled = true;
        }
        let left = 1.0 - at.elapsed().as_secs_f32() / JUMP_LIT;
        if left <= 0.0 {
            self.talk_ui.jump = None;
            return;
        }
        ui.painter().rect_filled(row.expand2(Vec2::new(0.0, 3.0)), 8.0, theme.accent.gamma_multiply(0.18 * left));
        ui.ctx().request_repaint();
    }

    /// One's message put in the field to be changed (the draft put aside till then).
    fn start_edit(&mut self, ctx: &egui::Context, room: &str, m: &Message) {
        self.stop_answering(room);
        let aside = self.talk_ui.drafts.remove(room).unwrap_or_default();
        self.talk_ui.drafts.insert(room.to_owned(), m.text.clone());
        self.talk_ui.editing = Some((room.to_owned(), m.id, aside));
        let id = egui::Id::new(("talk-field", room));
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(m.text.chars().count()))));
        state.store(ctx, id);
        self.talk_ui.focus = true;
    }

    /// The answer or the change given up in a conversation (the draft put aside comes back).
    fn stop_answering(&mut self, room: &str) {
        self.talk_ui.replying.remove(room);
        if self.talk_ui.editing.as_ref().is_some_and(|(r, ..)| r == room)
            && let Some((_, _, aside)) = self.talk_ui.editing.take()
        {
            self.talk_ui.drafts.insert(room.to_owned(), aside);
        }
    }

    /// An emoji put in a conversation's field: at its cursor (at the end without one), or in place of
    /// the `:name` typed (`range`, char indexes). It goes to the recent ones.
    fn put_emoji(&mut self, ctx: &egui::Context, room: &str, chars: &str, range: Option<(usize, usize)>) {
        let id = egui::Id::new(("talk-field", room));
        let draft = self.talk_ui.drafts.entry(room.to_owned()).or_default();
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        let length = draft.chars().count();
        let (start, end) = range.unwrap_or_else(|| {
            let c = state.cursor.char_range().map_or(length, |r| usize::from(r.primary.index).min(length));
            (c, c)
        });
        let (text, at) = super::emojis::replaced(draft, start, end, chars);
        *draft = text;
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
        state.store(ctx, id);
        super::emojis::remember(&mut self.config.settings.emoji_recent, chars);
        self.save_config();
        self.talk_ui.focus = true;
    }

    /// The GIF picker closed: its search and previews forgotten (their memory freed).
    fn close_gifs(&mut self) {
        self.talk_ui.gif_open = None;
        self.talk_ui.gif_search.clear();
        self.talk_ui.gif_previews.clear();
    }

    /// The GIF picker, open: the trending GIFs, or those found (the search goes once the typing
    /// pauses), in three columns of animated previews, more coming at the bottom; the one clicked is
    /// sent (floor copies it). A click elsewhere or Esc closes it.
    fn talk_gif_picker(&mut self, ctx: &egui::Context, t: &'static Strings, theme: &Theme) {
        const COLS: usize = 3;
        const WIDTH: f32 = 408.0;
        const GAP: f32 = 6.0;
        let Some(key) = self.talk.key() else { return };
        let Some(open) = &mut self.talk_ui.gif_open else { return };
        let now = ctx.input(|i| i.time);
        // The search, once the typing pauses (at once when opened: the trending ones).
        let wanted = open.query.trim().to_owned();
        if open.searched.as_deref() != Some(wanted.as_str()) {
            if open.searched.is_none() || now - open.typed_at > 0.4 {
                self.talk_ui.gif_search.search(ctx, &key, &wanted);
                open.searched = Some(wanted.clone());
            } else {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
        }
        let Some(shown) = self.talk_ui.gif_search.shown() else { return };
        let column = (WIDTH - GAP * (COLS as f32 - 1.0)) / COLS as f32;
        let mut chosen: Option<crate::gif::Gif> = None;
        let mut more = false;
        let search = &self.talk_ui.gif_search;
        let previews = &mut self.talk_ui.gif_previews;
        let area = egui::Area::new(egui::Id::new("gif-picker")).order(egui::Order::Foreground).fixed_pos(open.at).pivot(Align2::RIGHT_BOTTOM).constrain(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).inner_margin(10.0).corner_radius(12.0).show(ui, |ui| {
                ui.set_width(WIDTH);
                let resp = ui.add(egui::TextEdit::singleline(&mut open.query).hint_text(t.gif_search).desired_width(WIDTH).margin(Vec2::new(8.0, 6.0)));
                if std::mem::take(&mut open.focus) {
                    resp.request_focus();
                }
                if resp.changed() {
                    open.typed_at = now;
                }
                ui.add_space(6.0);
                if shown.query.is_empty() {
                    ui.label(egui::RichText::new(t.gif_trending).size(11.5).color(theme.text_muted));
                    ui.add_space(4.0);
                }
                let scroll = egui::ScrollArea::vertical().id_salt(("gif-grid", shown.query.as_str())).max_height(360.0).min_scrolled_height(360.0).auto_shrink([false, false]).show(ui, |ui| {
                    if shown.results.is_empty() {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            if shown.loading {
                                ui.spinner();
                            } else if let Some(e) = &shown.error {
                                ui.label(egui::RichText::new(t.gif_unavailable).size(12.5).color(theme.ansi[1])).on_hover_text(e);
                            } else {
                                ui.label(egui::RichText::new(t.gif_none).size(12.5).color(theme.text_muted));
                            }
                        });
                        return;
                    }
                    // Three columns, each GIF in the shortest one (its height by its shape).
                    let mut heights = [0.0f32; COLS];
                    let mut places = Vec::with_capacity(shown.results.len());
                    for gif in &shown.results {
                        let (w, h) = (gif.preview.width.max(1) as f32, gif.preview.height.max(1) as f32);
                        let height = (column * h / w).clamp(60.0, 260.0);
                        let c = (0..COLS).min_by(|a, b| heights[*a].total_cmp(&heights[*b])).unwrap_or(0);
                        places.push((c, heights[c], height));
                        heights[c] += height + GAP;
                    }
                    let total = heights.iter().copied().fold(0.0, f32::max);
                    let (grid, _) = ui.allocate_exact_size(Vec2::new(WIDTH, total), Sense::hover());
                    let visible = ui.clip_rect();
                    let time = (ui.input(|i| i.time) * 1000.0) as u64;
                    for (gif, (c, y, height)) in shown.results.iter().zip(places) {
                        let rect = Rect::from_min_size(Pos2::new(grid.min.x + c as f32 * (column + GAP), grid.min.y + y), Vec2::new(column, height));
                        // Only those near the screen are fetched and played.
                        if !rect.intersects(visible.expand(300.0)) {
                            continue;
                        }
                        if !previews.contains_key(&gif.id)
                            && let Some(frames) = search.preview(ui.ctx(), &key, gif)
                        {
                            previews.insert(gif.id.clone(), crate::gif::Animation::new(ui.ctx(), &format!("gif-preview-{}", gif.id), frames));
                        }
                        let resp = ui.interact(rect, ui.id().with(("gif", gif.id.as_str())), Sense::click());
                        match previews.get(&gif.id) {
                            Some(animation) => {
                                let (texture, wait) = animation.at(time);
                                if rect.intersects(visible) {
                                    ui.ctx().request_repaint_after(wait);
                                }
                                ui.painter().add(egui::epaint::RectShape::filled(rect, 8.0, Color32::WHITE).with_texture(texture.id(), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))));
                            }
                            None => {
                                ui.painter().rect_filled(rect, 8.0, theme.tab_hover.gamma_multiply(0.6));
                            }
                        }
                        if resp.hovered() {
                            ui.painter().rect_stroke(rect, 8.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
                        }
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        let resp = if gif.title.is_empty() { resp } else { resp.on_hover_text(&gif.title) };
                        if resp.clicked() {
                            chosen = Some(gif.clone());
                        }
                    }
                    if shown.loading {
                        ui.add_space(8.0);
                        ui.vertical_centered(|ui| ui.spinner());
                    }
                });
                // Near the bottom: the next page.
                let state = scroll.state;
                if shown.more && !shown.loading && state.offset.y + scroll.inner_rect.height() > scroll.content_size.y - 300.0 {
                    more = true;
                }
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(t.gif_powered).size(10.5).color(theme.text_muted.gamma_multiply(0.8)));
                });
            });
        });
        let rect = area.response.rect;
        if more {
            self.talk_ui.gif_search.more(ctx, &key);
        }
        let since = now - self.talk_ui.gif_open.as_ref().map_or(now, |o| o.opened);
        let outside = since > 0.2 && ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if let Some(gif) = chosen {
            if let Some(open) = &self.talk_ui.gif_open {
                let (room, query) = (open.room.clone(), shown.query.clone());
                self.talk.send_gif(ctx, &room, &gif, &query);
            }
            self.close_gifs();
        } else if outside || escape {
            self.close_gifs();
        }
    }

    /// The emoji picker, open: the emoji clicked goes in the field (the picker stays) or reacts to the
    /// message (it closes); a click elsewhere or Esc closes it.
    fn talk_emoji_picker(&mut self, ctx: &egui::Context, t: &'static Strings, theme: &Theme) {
        let Some(sheet) = self.talk_ui.emoji_sheet.texture(ctx).cloned() else { return };
        let Some(open) = &mut self.talk_ui.emoji_open else { return };
        let recent = self.config.settings.emoji_recent.clone();
        let mut skin = self.config.settings.emoji_skin;
        let pivot = if matches!(open.target, EmojiTarget::Draft(_)) { Align2::RIGHT_BOTTOM } else { Align2::RIGHT_TOP };
        let shown = egui::Area::new(egui::Id::new("emoji-picker")).order(egui::Order::Foreground).fixed_pos(open.at).pivot(pivot).constrain(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme.chrome_bg)
                .stroke(Stroke::new(1.0, theme.tab_hover))
                .inner_margin(10.0)
                .corner_radius(12.0)
                .show(ui, |ui| super::emojis::picker(ui, &mut open.picker, &sheet, &recent, &mut skin, t, theme))
                .inner
        });
        let (chosen, rect) = (shown.inner, shown.response.rect);
        let target = open.target.clone();
        let since = ctx.input(|i| i.time) - open.opened;
        let outside = since > 0.2 && ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if skin != self.config.settings.emoji_skin {
            self.config.settings.emoji_skin = skin;
            self.save_config();
        }
        match (chosen, target) {
            (Some(chars), EmojiTarget::Draft(room)) => self.put_emoji(ctx, &room, &chars, None),
            (Some(chars), EmojiTarget::React(room, message)) => {
                self.talk.react(&room, message, &chars, true);
                super::emojis::remember(&mut self.config.settings.emoji_recent, &chars);
                self.save_config();
                self.talk_ui.emoji_open = None;
            }
            (None, _) if outside || escape => self.talk_ui.emoji_open = None,
            _ => {}
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
                Some(Fetched::Done { image: Some(_), .. }) => {
                    let Some((id, size)) = self.talk_picture(&ctx, file) else { return };
                    let scale = (360.0 / size.x).min(260.0 / size.y).min(1.0);
                    let resp = ui.add(egui::Image::new((id, size * scale)).corner_radius(10.0).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::ZoomIn).on_hover_text(&file.name);
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

    /// Claude's button: its mark; a click sends the draft to Claude Code (Haiku), its fixed text
    /// put in its place when it comes (unless the draft changed meanwhile). Without Claude Code,
    /// greyed, its tooltip saying what it would do.
    fn talk_fix_button(&mut self, ui: &mut Ui, room: &str, rect: Rect, t: &'static Strings, theme: &Theme) {
        // Looked for again now and then (it may be installed meanwhile).
        let installed = match self.talk_ui.claude {
            Some((found, at)) if at.elapsed() < Duration::from_secs(60) => found,
            _ => {
                let found = crate::recap::claude_installed();
                self.talk_ui.claude = Some((found, Instant::now()));
                found
            }
        };
        // Its answer, come.
        if let Some((r, sent, answer)) = &self.talk_ui.fixing
            && let Some(got) = answer.lock().ok().and_then(|mut a| a.take())
        {
            let (r, sent) = (r.clone(), sent.clone());
            self.talk_ui.fixing = None;
            match got.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()) {
                Some(fixed) => {
                    if let Some(draft) = self.talk_ui.drafts.get_mut(&r)
                        && draft.trim() == sent
                    {
                        *draft = fixed;
                        let id = egui::Id::new(("talk-field", r.as_str()));
                        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(draft.chars().count()))));
                        state.store(ui.ctx(), id);
                        self.talk_ui.focus = true;
                    }
                }
                None => self.talk_ui.refused = Some((t.talk_fix_failed.to_owned(), Instant::now())),
            }
        }
        let busy = self.talk_ui.fixing.as_ref().is_some_and(|(r, ..)| r == room);
        let text = self.talk_ui.drafts.get(room).map(|d| d.trim().to_owned()).unwrap_or_default();
        let usable = installed && self.talk.ready() && !text.is_empty() && self.talk_ui.fixing.is_none();
        let resp = ui.interact(rect, ui.id().with(("talk-fix", room)), if installed { Sense::click() } else { Sense::hover() }).on_hover_text(if installed { t.talk_fix } else { t.talk_fix_missing });
        let painter = ui.painter();
        if busy {
            egui::Spinner::new().size(14.0).paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(14.0)));
            return;
        }
        let lit = installed && resp.hovered();
        if lit {
            painter.rect_filled(rect, 6.0, theme.tab_hover);
        }
        // Claude's mark: bright when it can be used, faded without Claude Code.
        let tint = if !installed { Color32::from_white_alpha(60) } else if lit || usable { Color32::WHITE } else { Color32::from_white_alpha(150) };
        super::history::paint_mark(ui, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), tint);
        if usable && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            let answer = Arc::new(Mutex::new(None));
            self.talk_ui.fixing = Some((room.to_owned(), text.clone(), answer.clone()));
            let (prompt, ctx) = (format!("{}\n\n{text}", t.talk_fix_prompt), ui.ctx().clone());
            std::thread::spawn(move || {
                let fixed = crate::recap::ask_claude(&prompt);
                if let Ok(mut a) = answer.lock() {
                    *a = Some(fixed);
                }
                ctx.request_repaint();
            });
        }
    }

    /// Someone's face, large, with their name (and their pseudo, when given another name).
    fn talk_face_window(&mut self, ctx: &egui::Context, t: &'static Strings, theme: &Theme) {
        const SIDE: f32 = 256.0;
        let Some(id) = self.talk_ui.avatar_open.clone() else { return };
        let name = self.talk.name_of(&id).unwrap_or_default();
        let pseudo = self.talk.pseudo_of(&id).map(str::to_owned);
        let v = self.talk.avatar_of(&id);
        let face = self.avatars.get(ctx, self.config.settings.floor.as_ref(), &id, v.as_deref());
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover));
        let modal = egui::Modal::new(egui::Id::new("talk-face")).frame(frame).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIDE), Sense::hover());
                match &face {
                    Some(face) => {
                        egui::Image::new((face.id(), Vec2::splat(SIDE))).corner_radius(SIDE / 2.0).paint_at(ui, rect);
                    }
                    None => paint_avatar(ui.painter(), rect.center(), SIDE / 2.0, &name, false, None, theme),
                }
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&name).size(18.0).strong());
                if let Some(p) = pseudo {
                    ui.label(egui::RichText::new(t.talk_nickname_pseudo.replace("{n}", &p)).size(12.5).color(theme.text_muted));
                }
            });
        });
        if modal.should_close() || ctx.input(|i| i.pointer.any_click()) && modal.response.clicked_elsewhere() {
            self.talk_ui.avatar_open = None;
        }
    }

    /// An image fetched, made a texture (an animated GIF: the frame of the moment), and its size.
    fn talk_picture(&mut self, ctx: &egui::Context, file: &FileInfo) -> Option<(egui::TextureId, Vec2)> {
        let Some(Fetched::Done { image: Some(picture), .. }) = self.talk.files.get(&file.id) else { return None };
        // An animated GIF: its frames made textures once, the one of the moment shown.
        if let Some(frames) = self.talk.frames.remove(&file.id) {
            self.talk_ui.animations.insert(file.id.clone(), crate::gif::Animation::new(ctx, &format!("talk-{}", file.id), frames));
        }
        Some(match self.talk_ui.animations.get(&file.id) {
            Some(animation) => {
                let (texture, wait) = animation.at((ctx.input(|i| i.time) * 1000.0) as u64);
                ctx.request_repaint_after(wait);
                (texture.id(), animation.size())
            }
            None => {
                let texture = self.talk_ui.textures.entry(file.id.clone()).or_insert_with(|| ctx.load_texture(format!("talk-{}", file.id), (**picture).clone(), egui::TextureOptions::LINEAR));
                (texture.id(), texture.size_vec2())
            }
        })
    }

    /// The band of the messages pinned: one at a time (a click brings it on screen and shows the
    /// next), who pinned it and for how long yet; × unpins it.
    #[allow(clippy::too_many_arguments)]
    fn talk_pins_band(&mut self, ui: &mut Ui, room: &Room, pins: &[Message], band: Rect, me: &str, t: &'static Strings, theme: &Theme) {
        let k = self.talk_ui.pin_shown.get(&room.id).copied().unwrap_or(0) % pins.len();
        let m = &pins[k];
        let Some(pin) = &m.pinned else { return };
        let painter = ui.painter().clone();
        let resp = ui.interact(band, ui.id().with(("talk-pins", room.id.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        painter.rect(band, 10.0, if resp.hovered() { theme.tab_hover } else { theme.chrome_bg }, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        paint_act(&painter, Pos2::new(band.min.x + 20.0, band.center().y), Act::Pin, theme.accent);
        let by = if pin.by == me { t.talk_you.to_owned() } else { self.talk.name_of(&pin.by).unwrap_or_else(|| "?".into()) };
        let mut head = t.talk_pinned_by.replace("{n}", &by).replace("{d}", &time_left(t, pin.until - now_ms()));
        if pins.len() > 1 {
            head = format!("{}/{}  ·  {head}", k + 1, pins.len());
        }
        let x = band.min.x + 38.0;
        painter.text(Pos2::new(x, band.min.y + 13.0), Align2::LEFT_CENTER, head, FontId::proportional(11.0), theme.accent);
        let text = if m.text.is_empty() { m.file.as_ref().map(|f| format!("📎 {}", f.name)).unwrap_or_default() } else { m.text.replace('\n', " ") };
        let who = m.name.clone().unwrap_or_default();
        let mut job = egui::text::LayoutJob::simple_singleline(format!("{who} : {text}"), FontId::proportional(13.0), theme.text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(band.width() - 38.0 - 44.0);
        painter.galley(Pos2::new(x, band.min.y + 21.0), painter.layout_job(job), theme.text);
        // × unpins it.
        let close = Rect::from_center_size(Pos2::new(band.max.x - 20.0, band.center().y), Vec2::splat(24.0));
        let close_resp = ui.interact(close, ui.id().with(("talk-unpin", room.id.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.talk_unpin);
        if close_resp.hovered() {
            painter.circle_filled(close.center(), 12.0, theme.bg);
        }
        let c = if close_resp.hovered() { theme.text } else { theme.text_muted };
        painter.line_segment([close.center() - Vec2::splat(4.5), close.center() + Vec2::splat(4.5)], Stroke::new(1.5, c));
        painter.line_segment([close.center() + Vec2::new(-4.5, 4.5), close.center() + Vec2::new(4.5, -4.5)], Stroke::new(1.5, c));
        if close_resp.clicked() {
            self.talk.pin(&room.id, m.id, 0);
        } else if resp.clicked() {
            self.talk_ui.seeking = Some((room.id.clone(), m.id));
            self.talk_ui.pin_shown.insert(room.id.clone(), k + 1);
        }
        // Its time left told again, and the pin gone at its time.
        ui.ctx().request_repaint_after(Duration::from_secs(30));
    }

    /// The menu to pin a message: for how long (or unpinned). A click elsewhere closes it.
    fn talk_pin_menu(&mut self, ctx: &egui::Context, t: &'static Strings, theme: &Theme) {
        let Some((room, id, at, pinned)) = self.talk_ui.pin_menu.clone() else { return };
        const HOURS: [u32; 4] = [1, 3, 24, 168];
        let mut chosen: Option<u32> = None;
        let shown = egui::Area::new(egui::Id::new("talk-pin-menu")).order(egui::Order::Foreground).fixed_pos(at).constrain(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_min_width(170.0);
                ui.label(egui::RichText::new(t.talk_pin).size(11.5).color(theme.text_muted));
                for (k, label) in t.talk_pin_for.iter().enumerate() {
                    if ui.add(egui::Button::new(egui::RichText::new(*label).size(13.0)).frame(false).min_size(Vec2::new(158.0, 26.0))).clicked() {
                        chosen = Some(HOURS[k]);
                    }
                }
                if pinned {
                    ui.separator();
                    if ui.add(egui::Button::new(egui::RichText::new(t.talk_unpin).size(13.0).color(theme.ansi[1])).frame(false).min_size(Vec2::new(158.0, 26.0))).clicked() {
                        chosen = Some(0);
                    }
                }
            });
        });
        if let Some(hours) = chosen {
            self.talk.pin(&room, id, hours);
            self.talk_ui.pin_menu = None;
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || (shown.response.clicked_elsewhere() && ctx.input(|i| i.pointer.any_pressed())) {
            self.talk_ui.pin_menu = None;
        }
    }

    /// The search bar opened (its field focused) or closed.
    fn toggle_search(&mut self, room: &str) {
        self.talk_ui.search = match &self.talk_ui.search {
            Some(s) if s.room == room => None,
            _ => Some(SearchBar { room: room.to_owned(), q: String::new(), typed_at: 0.0, asked: String::new(), focus: true, results: true }),
        };
    }

    /// The search bar: its field (floor asked once the typing pauses), its results under it, the
    /// words found lit; one picked is brought on screen. Esc closes it.
    fn talk_search_bar(&mut self, ui: &mut Ui, room: &Room, title: &str, bar: Rect, t: &'static Strings, theme: &Theme) {
        let now = ui.input(|i| i.time);
        let me = self.talk.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        let Some(search) = &mut self.talk_ui.search else { return };
        let painter = ui.painter().clone();
        painter.rect(bar, 10.0, theme.chrome_bg, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        // A magnifying glass.
        let c = Pos2::new(bar.min.x + 20.0, bar.center().y);
        painter.circle_stroke(c - Vec2::new(1.5, 1.5), 5.5, Stroke::new(1.5, theme.text_muted));
        painter.line_segment([c + Vec2::new(2.5, 2.5), c + Vec2::new(6.5, 6.5)], Stroke::new(1.8, theme.text_muted));
        let field = Rect::from_min_max(Pos2::new(bar.min.x + 36.0, bar.min.y + 8.0), Pos2::new(bar.max.x - 12.0, bar.max.y - 8.0));
        let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(field));
        let resp = field_ui.add(egui::TextEdit::singleline(&mut search.q).hint_text(t.talk_search_hint.replace("{n}", title)).frame(egui::Frame::NONE).margin(Vec2::ZERO).font(FontId::proportional(14.0)).desired_width(field.width()));
        if std::mem::take(&mut search.focus) {
            resp.request_focus();
        }
        if resp.changed() {
            search.typed_at = now;
            search.results = true;
        }
        if resp.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.talk_ui.search = None;
            return;
        }
        // Asked once the typing pauses.
        let q = search.q.trim().to_owned();
        if q.chars().count() >= 2 && q != search.asked {
            if now - search.typed_at > 0.3 {
                search.asked = q.clone();
                self.talk.search(&room.id, &q);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        if q.chars().count() < 2 || !search.results {
            return;
        }
        let asked = search.asked.clone();
        let found = self.talk.found.as_ref().filter(|(r, fq, _)| *r == room.id && *fq == asked).map(|(_, _, m)| m.clone());
        let mut picked: Option<i64> = None;
        egui::Area::new(egui::Id::new(("talk-found", room.id.as_str()))).order(egui::Order::Foreground).fixed_pos(Pos2::new(bar.min.x, bar.max.y + 4.0)).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(bar.width() - 12.0);
                let Some(found) = &found else {
                    ui.vertical_centered(|ui| ui.spinner());
                    return;
                };
                if found.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t.talk_search_none).size(13.0).color(theme.text_muted));
                    ui.add_space(4.0);
                    return;
                }
                // As high as its results, 420 px at most (then it scrolls).
                let high = (found.len() as f32 * (46.0 + ui.spacing().item_spacing.y)).min(420.0);
                egui::ScrollArea::vertical().max_height(high).min_scrolled_height(high).auto_shrink([false, true]).show(ui, |ui| {
                    for m in found {
                        let who = if m.from == me { t.talk_you.to_owned() } else { m.name.clone().unwrap_or_default() };
                        let text = if m.text.is_empty() { m.file.as_ref().map(|f| format!("📎 {}", f.name)).unwrap_or_default() } else { m.text.replace('\n', " ") };
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 6.0, theme.tab_hover);
                        }
                        let p = ui.painter();
                        p.text(Pos2::new(rect.min.x + 10.0, rect.min.y + 13.0), Align2::LEFT_CENTER, &who, FontId::proportional(12.5), tint(theme, &who));
                        p.text(Pos2::new(rect.max.x - 10.0, rect.min.y + 13.0), Align2::RIGHT_CENTER, clock_day(m.at), FontId::proportional(11.0), theme.text_muted);
                        let mut job = lit_snippet(&text, &asked, theme);
                        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 20.0);
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(Pos2::new(rect.min.x + 10.0, rect.min.y + 24.0), g, theme.text);
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            picked = Some(m.id);
                        }
                    }
                });
            });
        });
        if let Some(id) = picked {
            if let Some(s) = &mut self.talk_ui.search {
                s.results = false;
            }
            self.talk_ui.seeking = Some((room.id.clone(), id));
        }
    }

    /// A message found being brought on screen: lit once it is here, older ones asked till then.
    fn talk_seek(&mut self, room: &Room) {
        let Some((r, id)) = self.talk_ui.seeking.clone() else { return };
        if r != room.id {
            return;
        }
        if room.messages.iter().any(|m| m.id == id) {
            self.talk_ui.jump = Some((id, Instant::now(), false));
            self.talk_ui.seeking = None;
        } else if room.done || room.messages.is_empty() {
            self.talk_ui.seeking = None;
        } else {
            self.talk.more(&room.id);
        }
    }

    /// The window of a conversation's files, its list asked of floor.
    fn open_media(&mut self, room: &str) {
        self.talk.ask_media(room);
        self.talk_ui.media_open = Some(room.to_owned());
    }

    /// A conversation's files, all of them: the images in a grid (a click opens one large), the
    /// others in a list (to save). Fetched as they come on screen.
    fn talk_media_window(&mut self, ctx: &egui::Context, me: &str, t: &'static Strings, theme: &Theme) {
        const THUMB: f32 = 160.0;
        const GAP: f32 = 8.0;
        const WIDTH: f32 = THUMB * 5.0 + GAP * 4.0;
        let Some(room) = self.talk_ui.media_open.clone() else { return };
        let Some(title) = self.talk.room(&room).map(|r| shown_title(t, r, me)) else {
            self.talk_ui.media_open = None;
            return;
        };
        let list = self.talk.media.get(&room).cloned();
        let screen = ctx.content_rect();
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover));
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("talk-media")).frame(frame).show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.talk_media_title.replace("{n}", &title)).size(18.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.close).size(13.0)).corner_radius(6.0)).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(12.0);
            let Some(list) = &list else {
                ui.vertical_centered(|ui| ui.spinner());
                return;
            };
            if list.is_empty() {
                ui.label(egui::RichText::new(t.talk_media_none).size(13.0).color(theme.text_muted));
                return;
            }
            let (images, others): (Vec<_>, Vec<_>) = list.iter().partition(|x| x.file.image());
            // As high as what it shows, the screen's height at most (then it scrolls).
            let rows = images.len().div_ceil(5) as f32;
            let content = if images.is_empty() { 0.0 } else { 30.0 + rows * (THUMB + GAP) + 8.0 } + if others.is_empty() { 0.0 } else { 30.0 + others.len() as f32 * 52.0 };
            let high = content.min((screen.height() - 220.0).max(200.0));
            egui::ScrollArea::vertical().max_height(high).min_scrolled_height(high).auto_shrink([false, true]).show(ui, |ui| {
                if !images.is_empty() {
                    ui.label(egui::RichText::new(format!("{}  ·  {}", t.talk_media_images, images.len())).size(12.0).color(theme.text_muted));
                    ui.add_space(6.0);
                    for row in images.chunks(5) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            for x in row {
                                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(THUMB), Sense::click());
                                let resp = resp.on_hover_cursor(egui::CursorIcon::ZoomIn).on_hover_text(format!("{}\n{}", x.file.name, clock_day(x.at)));
                                if !ui.is_rect_visible(rect) {
                                    continue;
                                }
                                self.talk.fetch(ctx, &x.file);
                                match self.talk_picture(ctx, &x.file) {
                                    Some((id, size)) => {
                                        // Cut to a square, from its middle.
                                        let side = size.x.min(size.y);
                                        let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), Vec2::new(side / size.x, side / size.y));
                                        egui::Image::new((id, Vec2::splat(THUMB))).uv(uv).corner_radius(8.0).paint_at(ui, rect);
                                        if resp.hovered() {
                                            ui.painter().rect_stroke(rect, 8.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
                                        }
                                        if resp.clicked() && self.talk_ui.textures.contains_key(&x.file.id) {
                                            self.talk_ui.viewing = Some(x.file.clone());
                                        }
                                    }
                                    None => {
                                        ui.painter().rect_filled(rect, 8.0, theme.bg);
                                        if matches!(self.talk.files.get(&x.file.id), Some(Fetched::Loading) | None) {
                                            egui::Spinner::new().paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(16.0)));
                                        }
                                    }
                                }
                            }
                        });
                        ui.add_space(GAP);
                    }
                    ui.add_space(8.0);
                }
                if !others.is_empty() {
                    ui.label(egui::RichText::new(format!("{}  ·  {}", t.talk_media_files, others.len())).size(12.0).color(theme.text_muted));
                    ui.add_space(6.0);
                    for x in others {
                        let who = if x.from == me { t.talk_you.to_owned() } else { self.talk.name_of(&x.from).unwrap_or_else(|| "?".into()) };
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("📄").size(18.0));
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&x.file.name).size(13.5).color(theme.text));
                                ui.label(egui::RichText::new(format!("{}  ·  {}  ·  {}", super::files::format_size(x.file.size, t), who, clock_day(x.at))).size(11.5).color(theme.text_muted));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new(egui::RichText::new(t.talk_save).size(12.5)).corner_radius(6.0)).clicked() {
                                    self.talk.fetch(ctx, &x.file);
                                    self.talk_ui.saving = Some(x.file.clone());
                                }
                            });
                        });
                        ui.add_space(6.0);
                    }
                }
            });
        });
        if close || modal.should_close() {
            self.talk_ui.media_open = None;
        }
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

        if let Some((id, name)) = &mut self.talk_ui.nicknaming {
            let pseudo = self.talk.pseudo_of(id).map(str::to_owned).or_else(|| self.talk.name_of(id)).unwrap_or_default();
            let (mut done, mut close) = (false, false);
            let modal = egui::Modal::new(egui::Id::new("talk-nickname")).frame(frame).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(t.talk_nickname).size(18.0).strong());
                ui.add_space(8.0);
                ui.add(egui::Label::new(egui::RichText::new(t.talk_nickname_help.replace("{n}", &pseudo)).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(12.0);
                let resp = ui.add(egui::TextEdit::singleline(name).char_limit(crate::talk::NAME_MAX).hint_text(&pseudo).desired_width(f32::INFINITY).margin(Vec2::new(10.0, 7.0)));
                resp.request_focus();
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.save).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
            if done {
                let (id, name) = (id.clone(), name.trim().to_owned());
                if name.is_empty() || name == pseudo {
                    self.config.settings.talk_nicknames.remove(&id);
                } else {
                    self.config.settings.talk_nicknames.insert(id, name);
                }
                self.save_config();
                self.talk.set_nicknames(&self.config.settings.talk_nicknames);
                self.talk_ui.nicknaming = None;
            } else if close || modal.should_close() {
                self.talk_ui.nicknaming = None;
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

        if let Some(room) = self.talk_ui.dropping.clone() {
            let title = self.talk.room(&room).map(|r| shown_title(t, r, &me)).unwrap_or_default();
            let (mut done, mut close) = (false, false);
            let modal = egui::Modal::new(egui::Id::new("talk-drop")).frame(frame).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(t.talk_erase).size(18.0).strong());
                ui.add_space(8.0);
                ui.add(egui::Label::new(egui::RichText::new(t.talk_erase_ask.replace("{n}", &title)).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.talk_erase).size(13.5).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
            if done {
                self.talk.drop_room(&room);
                self.talk_ui.dropping = None;
            } else if close || modal.should_close() {
                self.talk_ui.dropping = None;
            }
        }

        if let Some((room, id)) = self.talk_ui.deleting.clone() {
            let (mut done, mut close) = (false, false);
            let modal = egui::Modal::new(egui::Id::new("talk-delete")).frame(frame).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(t.talk_delete).size(18.0).strong());
                ui.add_space(8.0);
                ui.add(egui::Label::new(egui::RichText::new(t.talk_delete_ask).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(t.talk_delete).size(13.5).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
            if done {
                self.talk.delete(&room, id);
                if self.talk_ui.editing.as_ref().is_some_and(|(r, e, _)| *r == room && *e == id) {
                    self.stop_answering(&room);
                }
                self.talk_ui.deleting = None;
            } else if close || modal.should_close() {
                self.talk_ui.deleting = None;
            }
        }
    }
}
