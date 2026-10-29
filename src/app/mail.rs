//! The Dev category's mail catcher (see `crate::mailbox`): its server started or stopped, its port,
//! and the mails caught: searched, a page at a time, picked several at once to delete or mark, and
//! read as a mail app shows them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use mail_parser::{MessageParser, MimeHeaders};

use super::mailhtml::{self, Block, Picture};
use super::*;
use crate::mailbox::{self, Server, Shared, Store, Summary};

/// Mails a page.
const PAGE: usize = 50;
/// What the HTML and source tabs show at most (a huge mail would freeze the window).
const SHOWN_MAX: usize = 400_000;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ViewTab {
    #[default]
    Preview,
    Text,
    Html,
    Headers,
    Source,
}

struct Attachment {
    name: String,
    mime: String,
    cid: Option<String>,
    bytes: Vec<u8>,
    /// Shown in the mail (an image it refers to), not a file joined.
    inline: bool,
}

/// The mail shown, read once.
struct Opened {
    id: String,
    subject: String,
    from: String,
    to: Vec<String>,
    cc: Vec<String>,
    reply_to: Vec<String>,
    date: String,
    envelope: Vec<String>,
    html: Option<String>,
    blocks: Vec<Block>,
    text: Option<String>,
    headers: String,
    source: String,
    attachments: Vec<Attachment>,
}

type Fetch = Arc<Mutex<Option<Option<Vec<u8>>>>>;

enum Img {
    Ready(egui::TextureHandle, Vec2),
    Loading(Fetch),
    Failed,
}

/// The mail catcher's state.
pub(super) struct MailCatcher {
    store: Shared,
    server: Option<Server>,
    /// Why the server isn't listening.
    error: Option<String>,
    started: bool,
    port_text: String,
    search: String,
    page: usize,
    selected: HashSet<String>,
    opened: Option<Opened>,
    tab: ViewTab,
    images: HashMap<String, Img>,
}

impl Default for MailCatcher {
    fn default() -> Self {
        Self {
            store: Store::load(),
            server: None,
            error: None,
            started: false,
            port_text: String::new(),
            search: String::new(),
            page: 0,
            selected: HashSet::new(),
            opened: None,
            tab: ViewTab::default(),
            images: HashMap::new(),
        }
    }
}

impl MailCatcher {
    /// How many mails aren't read yet.
    pub fn unread(&self) -> usize {
        self.store.lock().unwrap().unread()
    }

    pub fn running(&self) -> Option<u16> {
        self.server.as_ref().map(|s| s.port)
    }

    fn start(&mut self, port: u16, ctx: &egui::Context) -> bool {
        match mailbox::start(port, self.store.clone(), ctx.clone()) {
            Ok(server) => {
                self.server = Some(server);
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                false
            }
        }
    }

    /// The mail with `id` shown (and marked read).
    fn open(&mut self, id: &str) {
        self.images.clear();
        let summary = {
            let mut store = self.store.lock().unwrap();
            store.set_read(&HashSet::from([id.to_owned()]), true);
            store.mails.iter().find(|m| m.id == id).cloned()
        };
        self.opened = summary.and_then(|s| read(&s));
    }
}

/// A mail read from its file.
fn read(summary: &Summary) -> Option<Opened> {
    Some(read_raw(summary, &mailbox::raw(&summary.id)?))
}

fn read_raw(summary: &Summary, raw: &[u8]) -> Opened {
    let lossy = String::from_utf8_lossy(&raw);
    let head_end = lossy.find("\r\n\r\n").or_else(|| lossy.find("\n\n")).unwrap_or(lossy.len());
    let headers = lossy[..head_end].to_owned();
    let source = clip(&lossy);
    let local = |ts: i64| chrono::DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M:%S").to_string()).unwrap_or_default();
    let mut opened = Opened {
        id: summary.id.clone(),
        subject: summary.subject.clone(),
        from: summary.from.clone(),
        to: Vec::new(),
        cc: Vec::new(),
        reply_to: Vec::new(),
        date: local(summary.received),
        envelope: summary.to.clone(),
        html: None,
        blocks: Vec::new(),
        text: None,
        headers,
        source,
        attachments: Vec::new(),
    };
    let Some(message) = MessageParser::default().parse(raw) else {
        opened.text = Some(lossy.into_owned());
        opened.blocks = mailhtml::from_text(opened.text.as_deref().unwrap_or_default());
        return opened;
    };
    opened.to = mailbox::format_address(message.to());
    opened.cc = mailbox::format_address(message.cc());
    opened.reply_to = mailbox::format_address(message.reply_to());
    if let Some(date) = message.date() {
        opened.date = local(date.to_timestamp());
    }
    // The text part as written (not one made from the HTML), and the HTML (not one made from the text).
    opened.text = message.text_bodies().find(|p| p.is_text() && !p.is_text_html()).and_then(|p| p.text_contents()).map(str::to_owned);
    opened.html = message.html_bodies().find(|p| p.is_text_html()).and_then(|p| p.text_contents()).map(str::to_owned);
    for part in message.attachments() {
        let ctype = part.content_type();
        let mime = ctype.map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream"))).unwrap_or_else(|| "application/octet-stream".into()).to_ascii_lowercase();
        let cid = part.content_id().map(|c| c.trim_matches(['<', '>']).to_owned());
        let inline = cid.as_ref().is_some_and(|c| opened.html.as_ref().is_some_and(|h| h.contains(&format!("cid:{c}"))));
        let name = part.attachment_name().map(str::to_owned).unwrap_or_else(|| match &cid {
            Some(c) => c.clone(),
            None => format!("{}.{}", "piece", mime.rsplit('/').next().unwrap_or("bin")),
        });
        // A mail joined to this one: kept as a whole file.
        let bytes = match part.message() {
            Some(m) => m.raw_message().to_vec(),
            None => part.contents().to_vec(),
        };
        opened.attachments.push(Attachment { name, mime, cid, bytes, inline });
    }
    opened.blocks = match &opened.html {
        Some(html) => mailhtml::parse(html),
        None => mailhtml::from_text(opened.text.as_deref().unwrap_or_default()),
    };
    opened
}

/// At most `SHOWN_MAX` bytes of `text` (cut on a character).
fn clip(text: &str) -> String {
    if text.len() <= SHOWN_MAX {
        return text.to_owned();
    }
    let mut end = SHOWN_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…", &text[..end])
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// An image's bytes as a texture.
fn texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<Img> {
    let image = image::load_from_memory(bytes).ok()?;
    let size = Vec2::new(image.width() as f32, image.height() as f32);
    let image = if image.width().max(image.height()) > 2400 { image.thumbnail(2400, 2400) } else { image };
    let rgba = image.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    Some(Img::Ready(ctx.load_texture(format!("mail-image:{name}"), color, egui::TextureOptions::LINEAR), size))
}

/// "16:42" today, "14/10" this year, "14/10/2025" before.
fn short_date(ts: i64) -> String {
    let Some(date) = chrono::DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&chrono::Local)) else { return String::new() };
    let now = chrono::Local::now();
    if date.date_naive() == now.date_naive() {
        date.format("%H:%M").to_string()
    } else if date.format("%Y").to_string() == now.format("%Y").to_string() {
        date.format("%d/%m").to_string()
    } else {
        date.format("%d/%m/%Y").to_string()
    }
}

impl App {
    /// The server started with Ronnie when it was left running; the mails come in and are told of.
    pub(super) fn mail_poll(&mut self, ctx: &egui::Context) {
        if !self.mail.started {
            self.mail.started = true;
            self.mail.port_text = self.config.settings.mail_port.to_string();
            if self.config.settings.mail_running {
                self.mail.start(self.config.settings.mail_port, ctx);
            }
        }
        let arrived = std::mem::take(&mut self.mail.store.lock().unwrap().arrived);
        if arrived.is_empty() {
            return;
        }
        let t = self.t();
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let style = self.config.settings.notify_style;
        // A burst (a test suite sending many) is told of once.
        let (title, body, id) = match arrived.as_slice() {
            [one] => {
                let subject = if one.subject.is_empty() { t.mail_no_subject } else { one.subject.as_str() };
                (t.mail_new.to_owned(), format!("{subject}\n{}", one.from), Some(one.id.clone()))
            }
            many => (t.mail_new_many.replace("{n}", &many.len().to_string()), many.iter().take(3).map(|m| m.subject.as_str()).collect::<Vec<_>>().join("\n"), None),
        };
        // The window's notice too while Ronnie is away: it waits there until it's back in front.
        let system = !focused || style.system();
        crate::log::info(&format!("Mail caught ({}): told {}", arrived.len(), match (system, style.in_app()) {
            (true, true) => "by the system and in the window",
            (true, false) => "by the system",
            (false, _) => "in the window",
        }));
        if system {
            crate::notify::send(&title, &body);
        }
        if style.in_app() {
            self.push_toast(Toast { ok: true, title, body, tab: self.active, at: std::time::Instant::now(), mail: Some(id.unwrap_or_default()), notice: 0, lead: None });
        }
    }

    /// The mail page, from a notice: with that mail open (none: the list).
    pub(super) fn open_mail(&mut self, id: &str) {
        self.dev.tool = dev::DevTool::Mail;
        self.open_dev_page();
        if !id.is_empty() {
            self.mail.open(id);
        }
    }

    pub(super) fn mail_stop(&mut self) {
        self.mail.server = None;
    }

    /// The mail catcher's page: its server, then the mails and the one read.
    pub(super) fn mail_page_ui(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        let height = ui.available_height() - 40.0;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(14.0).inner_margin(20.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(height.max(300.0));
            self.mail_server_ui(ui, t, theme);
            ui.add_space(12.0);
            self.mail_toolbar(ui, t, theme);
            ui.add_space(8.0);
            let rect = ui.available_rect_before_wrap();
            let wide = rect.width() > 820.0;
            if wide {
                let list_w = (rect.width() * 0.38).clamp(300.0, 460.0);
                let left = Rect::from_min_size(rect.min, Vec2::new(list_w, rect.height()));
                let right = Rect::from_min_max(Pos2::new(rect.min.x + list_w + 14.0, rect.min.y), rect.max);
                let mut list = ui.new_child(egui::UiBuilder::new().max_rect(left));
                self.mail_list(&mut list, t, theme);
                ui.painter().vline(rect.min.x + list_w + 7.0, rect.y_range(), Stroke::new(1.0, theme.tab_hover));
                let mut view = ui.new_child(egui::UiBuilder::new().max_rect(right));
                self.mail_view(&mut view, t, theme, false);
            } else {
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                if self.mail.opened.is_some() {
                    self.mail_view(&mut inner, t, theme, true);
                } else {
                    self.mail_list(&mut inner, t, theme);
                }
            }
            ui.allocate_rect(rect, Sense::hover());
        });
    }

    /// The server's line: running or not, start / stop, its port, how projects reach it.
    fn mail_server_ui(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        let running = self.mail.running();
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(Vec2::new(22.0, 28.0), Sense::hover());
            paint_mail_icon(ui.painter(), icon.center(), MAIL_BLUE);
            ui.label(egui::RichText::new(t.mail_title).size(19.0).color(theme.text));
            ui.add_space(10.0);
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            match running {
                Some(port) => {
                    ui.painter().circle_filled(dot.center(), 4.0, dev::NODE_GREEN);
                    ui.label(egui::RichText::new(t.mail_running.replace("{p}", &port.to_string())).size(12.5).color(theme.text));
                }
                None => {
                    ui.painter().circle_stroke(dot.center(), 3.5, Stroke::new(1.2, theme.text_muted));
                    ui.label(egui::RichText::new(t.mail_stopped).size(12.5).color(theme.text_muted));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, fill) = if running.is_some() { (t.mail_stop, theme.tab_hover) } else { (t.mail_start, dev::NODE_GREEN) };
                let text = egui::RichText::new(label).size(12.5).color(if running.is_some() { theme.text } else { Color32::WHITE });
                if ui.add(egui::Button::new(text).fill(fill).corner_radius(6.0).min_size(Vec2::new(90.0, 26.0))).clicked() {
                    if running.is_some() {
                        self.mail_stop();
                        self.config.settings.mail_running = false;
                    } else {
                        let port = self.config.settings.mail_port;
                        let ctx = ui.ctx().clone();
                        self.config.settings.mail_running = self.mail.start(port, &ctx);
                    }
                    self.save_config();
                }
                ui.add_space(8.0);
                // The port: applied with Enter, or when the field is left.
                let field = ui.add(egui::TextEdit::singleline(&mut self.mail.port_text).desired_width(56.0).char_limit(5).horizontal_align(egui::Align::Center));
                self.mail.port_text.retain(|c| c.is_ascii_digit());
                let parsed = self.mail.port_text.parse::<u16>().ok().filter(|p| *p > 0);
                let changed = parsed.is_some_and(|p| p != self.config.settings.mail_port);
                if field.lost_focus() {
                    match parsed {
                        Some(port) if changed => self.mail_set_port(port, ui.ctx()),
                        _ => self.mail.port_text = self.config.settings.mail_port.to_string(),
                    }
                }
                ui.label(egui::RichText::new(t.mail_port).size(12.5).color(theme.text_muted));
            });
        });
        ui.add_space(2.0);
        let port = running.unwrap_or(self.config.settings.mail_port);
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(t.mail_hint.replace("{p}", &port.to_string())).size(12.5).color(theme.text_muted));
            let dsn = format!("smtp://127.0.0.1:{port}");
            if ui.add(egui::Button::new(egui::RichText::new(&dsn).monospace().size(12.0).color(theme.text)).corner_radius(5.0)).on_hover_text(t.mail_copy).clicked() {
                ui.ctx().copy_text(dsn);
            }
        });
        if let Some(e) = &self.mail.error {
            ui.label(egui::RichText::new(t.mail_not_started.replace("{e}", e)).size(12.5).color(theme.ansi[1]));
        }
    }

    /// Another port: the server moves to it if it runs (it stays where it was if it can't).
    fn mail_set_port(&mut self, port: u16, ctx: &egui::Context) {
        if self.mail.running().is_some() {
            let old = self.mail.server.take();
            if !self.mail.start(port, ctx) {
                self.mail.server = old;
                self.mail.port_text = self.config.settings.mail_port.to_string();
                return;
            }
        }
        self.config.settings.mail_port = port;
        self.save_config();
    }

    /// The ids of the mails the search finds, the newest first.
    fn mail_found(&self) -> Vec<Summary> {
        let words: Vec<String> = self.mail.search.split_whitespace().map(str::to_lowercase).collect();
        self.mail.store.lock().unwrap().mails.iter().filter(|m| m.matches(&words)).cloned().collect()
    }

    /// Search, the selection and what's done to it, the pages.
    fn mail_toolbar(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        let found = self.mail_found();
        let pages = found.len().div_ceil(PAGE).max(1);
        self.mail.page = self.mail.page.min(pages - 1);
        let shown: Vec<&Summary> = found.iter().skip(self.mail.page * PAGE).take(PAGE).collect();
        // The selection keeps to the mails still there.
        let kept: HashSet<&str> = found.iter().map(|m| m.id.as_str()).collect();
        self.mail.selected.retain(|id| kept.contains(id.as_str()));
        let (mut read, mut delete) = (None, false);
        ui.horizontal(|ui| {
            let all = !shown.is_empty() && shown.iter().all(|m| self.mail.selected.contains(&m.id));
            let mut check = all;
            let some = !all && shown.iter().any(|m| self.mail.selected.contains(&m.id));
            let resp = ui.add(egui::Checkbox::without_text(&mut check).indeterminate(some)).on_hover_text(t.mail_select_page);
            if resp.changed() {
                for m in &shown {
                    if check {
                        self.mail.selected.insert(m.id.clone());
                    } else {
                        self.mail.selected.remove(&m.id);
                    }
                }
            }
            let search = ui.add(egui::TextEdit::singleline(&mut self.mail.search).hint_text(t.mail_search_hint).desired_width(260.0));
            if search.changed() {
                self.mail.page = 0;
            }
            if !self.mail.search.is_empty() && ui.add(egui::Button::new("✕").frame_when_inactive(false)).clicked() {
                self.mail.search.clear();
                self.mail.page = 0;
            }
            let n = self.mail.selected.len();
            if n > 0 {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(if n == 1 { t.mail_selected_one.to_owned() } else { t.mail_selected.replace("{n}", &n.to_string()) }).size(12.0).color(theme.accent));
                if ui.button(t.mail_mark_read).clicked() {
                    read = Some(true);
                }
                if ui.button(t.mail_mark_unread).clicked() {
                    read = Some(false);
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.delete).color(theme.ansi[1]))).clicked() {
                    delete = true;
                }
                if all && found.len() > shown.len() && n < found.len() && ui.link(t.mail_select_all.replace("{n}", &found.len().to_string())).clicked() {
                    self.mail.selected = found.iter().map(|m| m.id.clone()).collect();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(self.mail.page + 1 < pages, egui::Button::new("›").min_size(Vec2::new(26.0, 22.0))).clicked() {
                    self.mail.page += 1;
                }
                if ui.add_enabled(self.mail.page > 0, egui::Button::new("‹").min_size(Vec2::new(26.0, 22.0))).clicked() {
                    self.mail.page -= 1;
                }
                if !found.is_empty() {
                    let from = self.mail.page * PAGE + 1;
                    let range = t.mail_range.replace("{a}", &from.to_string()).replace("{b}", &(from + shown.len() - 1).to_string()).replace("{n}", &found.len().to_string());
                    ui.label(egui::RichText::new(range).size(12.0).color(theme.text_muted));
                }
            });
        });
        let ids = self.mail.selected.clone();
        if let Some(read) = read {
            self.mail.store.lock().unwrap().set_read(&ids, read);
        }
        if delete {
            self.mail_delete(&ids);
        }
    }

    fn mail_delete(&mut self, ids: &HashSet<String>) {
        self.mail.store.lock().unwrap().delete(ids);
        self.mail.selected.retain(|id| !ids.contains(id));
        if self.mail.opened.as_ref().is_some_and(|o| ids.contains(&o.id)) {
            self.mail.opened = None;
            self.mail.images.clear();
        }
    }

    /// The page of mails: a click reads one, its box picks it.
    fn mail_list(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        let found = self.mail_found();
        if found.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                let text = if !self.mail.search.trim().is_empty() {
                    t.mail_no_match
                } else if self.mail.running().is_some() {
                    t.mail_empty
                } else {
                    t.mail_empty_stopped
                };
                ui.label(egui::RichText::new(text).size(13.0).color(theme.text_muted));
            });
            return;
        }
        let shown: Vec<Summary> = found.into_iter().skip(self.mail.page * PAGE).take(PAGE).collect();
        let mut open = None;
        egui::ScrollArea::vertical().id_salt(("mail-list", self.mail.page)).auto_shrink([false, false]).show(ui, |ui| {
            for mail in &shown {
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
                let active = self.mail.opened.as_ref().is_some_and(|o| o.id == mail.id);
                let picked = self.mail.selected.contains(&mail.id);
                let fill = if active {
                    theme.accent.gamma_multiply(0.16)
                } else if picked {
                    theme.accent.gamma_multiply(0.08)
                } else if resp.hovered() {
                    theme.tab_hover.gamma_multiply(0.6)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 8.0, fill);
                let mut check = picked;
                let check_rect = Rect::from_center_size(Pos2::new(rect.min.x + 16.0, rect.min.y + 19.0), Vec2::splat(18.0));
                // In a child of its own: `put` would move the list's cursor up to under the box.
                let mut boxed = ui.new_child(egui::UiBuilder::new().max_rect(check_rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
                if boxed.add(egui::Checkbox::without_text(&mut check)).changed() {
                    if check {
                        self.mail.selected.insert(mail.id.clone());
                    } else {
                        self.mail.selected.remove(&mail.id);
                    }
                }
                if !mail.read {
                    ui.painter().circle_filled(Pos2::new(rect.min.x + 16.0, rect.min.y + 41.0), 3.5, theme.accent);
                }
                let x = rect.min.x + 34.0;
                let text_w = rect.max.x - x - 10.0;
                let date = short_date(mail.received);
                let date_g = ui.painter().layout_no_wrap(date, FontId::proportional(11.5), theme.text_muted);
                let date_w = date_g.size().x;
                ui.painter().galley(Pos2::new(rect.max.x - 10.0 - date_w, rect.min.y + 10.0), date_g, theme.text_muted);
                let from_font = FontId::new(13.0, egui::FontFamily::Name(if mail.read { "note" } else { "note-bold" }.into()));
                let from = name_of(&mail.from);
                let line = |text: &str, font: FontId, color: Color32, w: f32| {
                    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, color));
                    job.wrap = egui::text::TextWrapping::truncate_at_width(w);
                    ui.painter().layout_job(job)
                };
                ui.painter().galley(Pos2::new(x, rect.min.y + 8.0), line(from, from_font, theme.text, text_w - date_w - 10.0), theme.text);
                let subject = if mail.subject.is_empty() { t.mail_no_subject } else { mail.subject.as_str() };
                ui.painter().galley(Pos2::new(x, rect.min.y + 25.0), line(subject, FontId::proportional(12.5), if mail.read { theme.text_muted } else { theme.text }, text_w - if mail.attachments > 0 { 18.0 } else { 0.0 }), theme.text);
                if mail.attachments > 0 {
                    paint_clip(ui.painter(), Pos2::new(rect.max.x - 16.0, rect.min.y + 32.0), theme.text_muted);
                }
                ui.painter().galley(Pos2::new(x, rect.min.y + 41.0), line(&mail.text, FontId::proportional(11.5), theme.text_muted.gamma_multiply(0.8), text_w), theme.text_muted);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    open = Some(mail.id.clone());
                }
                ui.add_space(2.0);
            }
        });
        if let Some(id) = open {
            self.mail.open(&id);
            self.mail.tab = ViewTab::Preview;
        }
    }

    /// The mail read: who, when, its tabs, its attachments.
    fn mail_view(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme, back: bool) {
        let Some(opened) = &self.mail.opened else {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                paint_mail_icon_sized(ui.painter(), r.center(), theme.text_muted.gamma_multiply(0.5), 2.4);
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.mail_pick).size(13.0).color(theme.text_muted));
            });
            return;
        };
        let (mut close, mut delete, mut unread, mut browser, mut reveal, mut save) = (false, false, false, false, false, None);
        ui.horizontal(|ui| {
            if back && ui.button(t.mail_back).clicked() {
                close = true;
            }
            let subject = if opened.subject.is_empty() { t.mail_no_subject } else { opened.subject.as_str() };
            ui.add(egui::Label::new(egui::RichText::new(subject).size(17.0).color(theme.text).family(egui::FontFamily::Name("note-bold".into()))).wrap());
        });
        ui.add_space(4.0);
        let field = |ui: &mut Ui, name: &str, value: &str| {
            if value.is_empty() {
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{name} :")).size(12.0).color(theme.text_muted));
                ui.add(egui::Label::new(egui::RichText::new(value).size(12.5).color(theme.text)).selectable(true));
            });
        };
        field(ui, t.mail_from, &opened.from);
        field(ui, t.mail_to, &opened.to.join(", "));
        field(ui, t.mail_cc, &opened.cc.join(", "));
        field(ui, t.mail_reply_to, &opened.reply_to.join(", "));
        // The envelope's recipients, when they aren't the headers' (the Bcc ones).
        let listed: Vec<String> = opened.to.iter().chain(&opened.cc).map(|a| address_of(a).to_lowercase()).collect();
        let hidden: Vec<&String> = opened.envelope.iter().filter(|e| !listed.contains(&e.to_lowercase())).collect();
        if !hidden.is_empty() {
            field(ui, t.mail_bcc, &hidden.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        field(ui, t.mail_date, &opened.date);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tab, label, on) in [
                (ViewTab::Preview, t.mail_tab_preview, true),
                (ViewTab::Text, t.mail_tab_text, opened.text.is_some()),
                (ViewTab::Html, t.mail_tab_html, opened.html.is_some()),
                (ViewTab::Headers, t.mail_tab_headers, true),
                (ViewTab::Source, t.mail_tab_source, true),
            ] {
                if on && ui.selectable_label(self.mail.tab == tab, label).clicked() {
                    self.mail.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(t.delete).size(12.0).color(theme.ansi[1])).frame_when_inactive(false)).clicked() {
                    delete = true;
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.mail_mark_unread).size(12.0)).frame_when_inactive(false)).clicked() {
                    unread = true;
                }
                let folder = ui.add(egui::Button::new("    ").frame_when_inactive(false)).on_hover_text(t.mail_reveal);
                super::notes::paint_folder_icon(ui.painter(), folder.rect.center(), if folder.hovered() { theme.text } else { theme.text_muted });
                reveal = folder.clicked();
                if ui.add(egui::Button::new(egui::RichText::new(format!("↗ {}", t.mail_open_browser)).size(12.0)).corner_radius(5.0)).clicked() {
                    browser = true;
                }
            });
        });
        let files: Vec<usize> = opened.attachments.iter().enumerate().filter(|(_, a)| !a.inline).map(|(k, _)| k).collect();
        if !files.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                paint_clip(ui.painter(), ui.cursor().min + Vec2::new(6.0, 10.0), theme.text_muted);
                ui.add_space(14.0);
                for k in files {
                    let a = &opened.attachments[k];
                    let label = format!("{}  ·  {}", a.name, super::files::format_size(a.bytes.len() as u64, t));
                    if ui.add(egui::Button::new(egui::RichText::new(label).size(12.0)).corner_radius(5.0)).on_hover_text(format!("{} — {}", a.mime, t.mail_save)).clicked() {
                        save = Some(k);
                    }
                }
            });
        }
        ui.add_space(8.0);
        let tab = self.mail.tab;
        let id = opened.id.clone();
        egui::ScrollArea::both().id_salt(("mail-view", id.as_str(), tab as u8)).auto_shrink([false, false]).show(ui, |ui| match tab {
            ViewTab::Preview => {
                let ctx = ui.ctx().clone();
                let (images, opened) = (&mut self.mail.images, self.mail.opened.as_ref().unwrap());
                let mut picture = |src: &str| picture(images, opened, &ctx, src);
                Frame::NONE.fill(mailhtml::PAPER).corner_radius(10.0).inner_margin(egui::Margin::symmetric(26, 22)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if opened.blocks.is_empty() {
                        ui.label(egui::RichText::new(t.mail_no_text).color(Color32::from_gray(0x88)));
                    }
                    mailhtml::show(ui, &opened.blocks, &mut picture);
                });
            }
            ViewTab::Text => code(ui, opened.text.as_deref().unwrap_or_default(), theme),
            ViewTab::Html => code(ui, &clip(opened.html.as_deref().unwrap_or_default()), theme),
            ViewTab::Headers => code(ui, &opened.headers, theme),
            ViewTab::Source => code(ui, &opened.source, theme),
        });
        // The images fetched meanwhile.
        if self.mail.images.values().any(|i| matches!(i, Img::Loading(_))) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        }

        let Some(opened) = &self.mail.opened else { return };
        if browser {
            open_in_browser(opened);
        }
        if reveal && let Some(path) = mailbox::path(&opened.id) {
            config::reveal(&path);
        }
        if let Some(k) = save {
            let a = &opened.attachments[k];
            if let Some(out) = rfd::FileDialog::new().set_file_name(&a.name).save_file()
                && let Err(e) = std::fs::write(&out, &a.bytes)
            {
                self.error = Some(format!("{} : {e}", out.display()));
            }
        }
        if unread {
            let ids = HashSet::from([id.clone()]);
            self.mail.store.lock().unwrap().set_read(&ids, false);
            close = true;
        }
        if delete {
            self.mail_delete(&HashSet::from([id]));
        }
        if close {
            self.mail.opened = None;
            self.mail.images.clear();
        }
    }
}

/// A text shown as it is, selectable, in the terminal's font.
fn code(ui: &mut Ui, text: &str, theme: &Theme) {
    ui.add(egui::Label::new(egui::RichText::new(text).monospace().size(12.0).color(theme.text)).selectable(true).extend());
}

/// The image `src` of the mail shown: one it carries (cid:, data:), or one from the web, fetched once.
fn picture(images: &mut HashMap<String, Img>, opened: &Opened, ctx: &egui::Context, src: &str) -> Picture {
    if !images.contains_key(src) {
        let img = if let Some(cid) = src.strip_prefix("cid:") {
            let cid = cid.trim_matches(['<', '>']);
            opened.attachments.iter().find(|a| a.cid.as_deref() == Some(cid)).and_then(|a| texture(ctx, &format!("{}/{src}", opened.id), &a.bytes)).unwrap_or(Img::Failed)
        } else if let Some(data) = src.strip_prefix("data:") {
            let bytes = data.split_once(',').and_then(|(kind, b)| if kind.ends_with(";base64") { mail_parser::decoders::base64::base64_decode(b.trim().as_bytes()) } else { None });
            bytes.and_then(|b| texture(ctx, &format!("{}/{}", opened.id, src.len()), &b)).unwrap_or(Img::Failed)
        } else if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//") {
            let url = if src.starts_with("//") { format!("https:{src}") } else { src.to_owned() };
            let slot: Fetch = Arc::new(Mutex::new(None));
            let (s, c) = (slot.clone(), ctx.clone());
            let _ = std::thread::Builder::new().name("mail-image".into()).spawn(move || {
                let agent = ureq::config::Config::builder().timeout_global(Some(std::time::Duration::from_secs(15))).build().new_agent();
                let bytes = agent.get(&url).call().ok().and_then(|mut r| r.body_mut().with_config().limit(15 * 1024 * 1024).read_to_vec().ok());
                *s.lock().unwrap() = Some(bytes);
                c.request_repaint();
            });
            Img::Loading(slot)
        } else {
            Img::Failed
        };
        images.insert(src.to_owned(), img);
    }
    // A fetch that finished: its texture made.
    let finished = match images.get(src) {
        Some(Img::Loading(slot)) => slot.lock().unwrap().take(),
        _ => None,
    };
    if let Some(done) = finished {
        let img = done.and_then(|b| texture(ctx, &format!("{}/{src}", opened.id), &b)).unwrap_or(Img::Failed);
        images.insert(src.to_owned(), img);
    }
    match images.get(src) {
        Some(Img::Ready(t, size)) => Picture::Ready(t.id(), *size),
        Some(Img::Loading(_)) => Picture::Loading,
        _ => Picture::Failed,
    }
}

/// The mail as the browser shows it: its HTML (its images inside it), or its text.
fn open_in_browser(opened: &Opened) {
    let Some(dir) = crate::config::config_dir().map(|d| d.join("mail").join("view")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    // Only the last one opened is kept.
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let page = match &opened.html {
        Some(html) => {
            let mut html = html.clone();
            for a in &opened.attachments {
                if let Some(cid) = &a.cid {
                    html = html.replace(&format!("cid:{cid}"), &format!("data:{};base64,{}", a.mime, base64(&a.bytes)));
                }
            }
            format!("<!-- {} -->\n<meta charset=\"utf-8\">\n{html}", escape(&opened.subject))
        }
        None => format!("<meta charset=\"utf-8\"><title>{}</title><pre style=\"white-space:pre-wrap;font:14px/1.5 sans-serif\">{}</pre>", escape(&opened.subject), escape(opened.text.as_deref().unwrap_or_default())),
    };
    let path = dir.join(format!("{}.html", opened.id));
    if std::fs::write(&path, page).is_ok() {
        crate::terminal::links::open(&path.to_string_lossy());
    }
}

/// "Name <a@b>" → "Name"; "a@b" stays.
fn name_of(from: &str) -> &str {
    match from.find(" <") {
        Some(at) if at > 0 => from[..at].trim_matches('"'),
        _ => from,
    }
}

/// "Name <a@b>" → "a@b".
fn address_of(from: &str) -> &str {
    match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if b > a => &from[a + 1..b],
        _ => from,
    }
}

/// The mail catcher's blue.
pub(super) const MAIL_BLUE: Color32 = Color32::from_rgb(0x4a, 0x90, 0xd9);

/// An envelope: the mail catcher.
pub(super) fn paint_mail_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    paint_mail_icon_sized(painter, c, color, 1.0);
}

pub(super) fn paint_mail_icon_sized(painter: &egui::Painter, c: Pos2, color: Color32, scale: f32) {
    let stroke = Stroke::new(1.4 * scale.sqrt(), color);
    let r = Rect::from_center_size(c, Vec2::new(14.0, 10.0) * scale);
    painter.rect_stroke(r, 1.5 * scale, stroke, egui::StrokeKind::Middle);
    painter.line_segment([r.left_top(), Pos2::new(c.x, c.y + 1.0 * scale)], stroke);
    painter.line_segment([Pos2::new(c.x, c.y + 1.0 * scale), r.right_top()], stroke);
}

/// A paper clip: attachments.
fn paint_clip(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let top = Rect::from_center_size(c + Vec2::new(0.0, -2.0), Vec2::new(5.0, 8.0));
    painter.rect_stroke(top, 2.5, stroke, egui::StrokeKind::Middle);
    painter.line_segment([c + Vec2::new(0.0, -3.0), c + Vec2::new(0.0, 4.0)], stroke);
}

#[cfg(test)]
mod tests {
    #[test]
    fn multipart() {
        let raw = b"From: \"Shop\" <shop@example.com>\r\nTo: bob@example.com\r\nCc: =?utf-8?b?w4lsaXNl?= <elise@example.com>\r\nSubject: =?utf-8?q?Votre_commande_n=C2=B012?=\r\nDate: Fri, 16 Oct 2026 10:00:00 +0200\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"m\"\r\n\r\n--m\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n--r\r\nContent-Type: multipart/alternative; boundary=\"a\"\r\n\r\n--a\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nMerci =C3=A9norm=C3=A9ment\r\n--a\r\nContent-Type: text/html; charset=iso-8859-1\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>Merci =E9norm=E9ment</p><img src=3D\"cid:logo@x\">\r\n--a--\r\n--r\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--r--\r\n--m\r\nContent-Type: application/pdf; name=\"facture.pdf\"\r\nContent-Disposition: attachment; filename=\"facture.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--m--\r\n";
        let summary = super::Summary { id: "x".into(), subject: "s".into(), to: vec!["bob@example.com".into(), "secret@example.com".into()], ..Default::default() };
        let o = super::read_raw(&summary, raw);
        assert_eq!(o.to, ["bob@example.com"]);
        assert_eq!(o.cc, ["Élise <elise@example.com>"]);
        assert_eq!(o.text.as_deref().map(str::trim), Some("Merci énormément"));
        assert!(o.html.as_deref().unwrap().contains("Merci énormément"));
        let names: Vec<(&str, bool)> = o.attachments.iter().map(|a| (a.name.as_str(), a.inline)).collect();
        assert_eq!(names, [("logo@x", true), ("facture.pdf", false)]);
        assert_eq!(o.attachments[1].bytes, b"%PDF");
        assert!(o.headers.starts_with("From:") && !o.headers.contains("--m"));
    }

    #[test]
    fn base64() {
        assert_eq!(super::base64(b"Ronnie"), "Um9ubmll");
        assert_eq!(super::base64(b"ab"), "YWI=");
        assert_eq!(super::base64(b"a"), "YQ==");
    }
}
