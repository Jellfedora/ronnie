//! The "Claude" tab of a tracked project's page: a conversation with Claude Code in the folder of
//! the profile tied to the project (see `crate::trackchat`). The user's messages on the right,
//! Claude's answers (light Markdown) and the tools it used on the left; what it would change waits for
//! the user's leave (allowed: the conversation goes on with it granted).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use super::*;
use crate::track::{format_minutes as hm, Project};
use crate::trackchat::{self, Chat, Event, Item, Running};

pub(super) struct ChatUi {
    tx: Sender<(String, Event)>,
    rx: Receiver<(String, Event)>,
    /// The conversations read, by project.
    chats: HashMap<String, Chat>,
    running: HashMap<String, Running>,
    /// What is being typed, by project.
    input: HashMap<String, String>,
    /// "New conversation" clicked once (a second click forgets it).
    confirm_clear: Option<String>,
}

impl Default for ChatUi {
    fn default() -> Self {
        let (tx, rx) = channel();
        ChatUi { tx, rx, chats: HashMap::new(), running: HashMap::new(), input: HashMap::new(), confirm_clear: None }
    }
}

impl ChatUi {
    pub(super) fn is_running(&self, project: &str) -> bool {
        self.running.contains_key(project)
    }

    fn chat(&mut self, project: &str) -> &mut Chat {
        self.chats.entry(project.to_owned()).or_insert_with(|| Chat::load(project))
    }
}

/// What the tools Claude used are said as.
fn tool_said(t: &Strings, name: &str) -> &'static str {
    match name {
        "Read" | "NotebookRead" => t.track_chat_read,
        "Write" => t.track_chat_write,
        "Edit" | "MultiEdit" | "NotebookEdit" => t.track_chat_edit,
        "Bash" => t.track_chat_run,
        "Glob" | "Grep" => t.track_chat_search,
        "WebFetch" | "WebSearch" => t.track_chat_web,
        "TodoWrite" => t.track_chat_todo,
        "Task" | "Agent" => t.track_chat_agent,
        _ => "",
    }
}

/// A line with `code` and **bold** in it, as a layout to wrap at `width`.
fn inline_job(line: &str, size: f32, color: Color32, code_bg: Color32, width: f32, strong: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    let (mut bold, mut code) = (strong, false);
    let mut rest = line;
    while !rest.is_empty() {
        // The next mark: ** or `.
        let next = [rest.find("**").map(|i| (i, 2)), rest.find('`').map(|i| (i, 1))].into_iter().flatten().min_by_key(|(i, _)| *i);
        let (text, mark) = match next {
            Some((i, n)) => (&rest[..i], Some((i, n))),
            None => (rest, None),
        };
        if !text.is_empty() {
            let font = if code { FontId::monospace(size - 1.0) } else { FontId::proportional(size) };
            let mut format = egui::TextFormat { font_id: font, color, ..Default::default() };
            if code {
                format.background = code_bg;
            } else if bold {
                format.font_id = FontId::new(size, egui::FontFamily::Name("note-bold".into()));
            }
            job.append(text, 0.0, format);
        }
        match mark {
            Some((i, 2)) if !code => {
                bold = !bold;
                rest = &rest[i + 2..];
            }
            Some((i, 1)) => {
                code = !code;
                rest = &rest[i + 1..];
            }
            Some((i, n)) => {
                // ** inside code: as it is.
                let format = egui::TextFormat { font_id: FontId::monospace(size - 1.0), color, background: code_bg, ..Default::default() };
                job.append(&rest[i..i + n], 0.0, format);
                rest = &rest[i + n..];
            }
            None => rest = "",
        }
    }
    job
}

impl App {
    /// The folder Claude works in for a project: the first folder of the profile tied to it.
    fn track_chat_cwd(&self, project: &str) -> Option<PathBuf> {
        let profile = self.config.settings.track_profiles.get(project)?;
        let p = self.config.profiles.iter().find(|p| p.id == *profile)?;
        p.tab.layout.cwds().into_iter().flatten().find(|d| d.is_dir())
    }

    /// Collects what the conversations going on said.
    pub(super) fn track_chat_poll(&mut self) {
        let chat = &mut self.track.chat;
        let mut touched = Vec::new();
        while let Ok((project, event)) = chat.rx.try_recv() {
            let c = chat.chats.entry(project.clone()).or_insert_with(|| Chat::load(&project));
            match event {
                Event::Session(id) => c.session = Some(id),
                Event::Item(item) => c.items.push(item),
                Event::Done(error) => {
                    chat.running.remove(&project);
                    if let Some(text) = error {
                        c.items.push(Item::Error { text });
                    }
                    // Claude may have logged a task: the project read again.
                    touched.push(project.clone());
                }
            }
            if let Some(c) = chat.chats.get_mut(&project) {
                c.save(&project);
            }
        }
        for project in touched {
            self.track_refresh();
            if self.config.settings.track_selected.as_deref() == Some(project.as_str()) {
                self.track_fetch(&project, false);
            }
        }
    }

    /// What Claude is told about the project, besides its usual prompt.
    fn track_chat_system(&self, p: &Project, cwd: &std::path::Path) -> String {
        let ronnie = crate::claude::ronnie_command("track");
        let counters: Vec<String> = p.counters.iter().filter(|c| c.closed.is_none()).map(|c| format!("\"{}\" ({} of {} left)", c.label, hm(c.left()), hm(c.minutes as i64))).collect();
        let quotes: Vec<String> = p.quotes.iter().filter(|q| q.closed.is_none()).map(|q| format!("\"{}\"", q.title())).collect();
        let recent: Vec<String> = p.tasks.iter().take(8).map(|x| format!("{} {} {}", x.day, hm(x.minutes as i64), x.label.replace('\n', " "))).collect();
        let language = match self.config.settings.language {
            crate::i18n::Lang::En => "English",
            _ => "French",
        };
        format!(
            "You are talking with the user in the \"Suivi\" (project tracking) page of Ronnie, not in a terminal: answer in {language}, \
short, in light Markdown (paragraphs, lists, `code`, **bold**, code blocks; no tables). This conversation is about the project \
\"{name}\" (client: {client}; tracking id {id}), whose code is in {cwd}, your working folder. The user may ask you to do a task \
in it, to explain or check something, or to log time.\n\
Writing files and running commands are refused at first: the user sees in the page what you wanted to do and allows it or not; \
when allowed, you are resumed and must then do exactly that. Say briefly what you are about to change before trying.\n\
Project tracking, with `{ronnie} …` (always allowed): `{ronnie} show {id}` (counters, quotes, tasks), `{ronnie} task {id} \
<duration> <description> [--day D] [--counter [C] | --quote Q | --free | --maintenance]` (no charge given: the current \
counter; `--item I`: on a planned task), `{ronnie} counter {id} <hours> [label]`, `{ronnie} quote {id} <label> [--number N] [--hours H]`, \
`{ronnie} item {id} <title> --quote Q [--parent I] [--hours H] [--body MD]` (a quote's planned tasks and sub-tasks), `{ronnie} item status {id} <item> \
todo|doing|done`; descriptions in Markdown. Log a task when the user \
asks; when you have finished a piece of work, propose once to log it (a description in the user's language, an estimate of \
the time it would have taken the user, how to charge it) and log it only once they agree. A counter used up: propose a new one.\n\
Open counters: {counters}. Open quotes: {quotes}. Last tasks: {recent}.",
            name = p.name,
            client = p.client.as_deref().unwrap_or("none"),
            id = p.id,
            cwd = cwd.display(),
            counters = if counters.is_empty() { "none".into() } else { counters.join(", ") },
            quotes = if quotes.is_empty() { "none".into() } else { quotes.join(", ") },
            recent = if recent.is_empty() { "none".into() } else { recent.join("; ") },
        )
    }

    /// A message to Claude about the project (`allowed`: actions the user just allowed).
    fn track_chat_send(&mut self, p: &Project, prompt: String, allowed: Vec<String>) {
        let t = self.t();
        let Some(cwd) = self.track_chat_cwd(&p.id) else { return };
        let system = self.track_chat_system(p, &cwd);
        let (tx, ctx) = (self.track.chat.tx.clone(), self.ctx.clone());
        let chat = self.track.chat.chat(&p.id);
        if allowed.is_empty() {
            chat.items.push(Item::User { text: prompt.clone() });
        }
        let ask = trackchat::Ask { project: p.id.clone(), cwd, prompt, session: chat.session.clone(), accept_edits: chat.accept_edits, allowed, system };
        match trackchat::start(ask, tx, move || ctx.request_repaint()) {
            Ok(running) => {
                self.track.chat.running.insert(p.id.clone(), running);
            }
            Err(e) => self.track.chat.chat(&p.id).items.push(Item::Error { text: format!("{} : {e}", t.track_chat_failed) }),
        }
        self.track.chat.chat(&p.id).save(&p.id);
    }

    /// The "Claude" tab: the conversation, then what to send.
    pub(super) fn track_chat_ui(&mut self, ui: &mut Ui, p: &Project) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(cwd) = self.track_chat_cwd(&p.id) else {
            ui.add_space(6.0);
            ui.add(egui::Label::new(egui::RichText::new(t.track_chat_no_folder).size(13.0).color(theme.text_muted)).wrap());
            ui.add_space(8.0);
            if ui.add(egui::Button::new(egui::RichText::new(format!("⚙  {}", t.track_settings)).size(12.5)).corner_radius(6.0)).clicked() {
                self.track_edit_open();
            }
            return;
        };
        let running = self.track.chat.is_running(&p.id);
        let id = p.id.clone();

        // The bar under the messages: its height kept for it.
        let bar_h = 104.0;
        let list_h = (ui.available_height() - bar_h).max(80.0);
        let (mut allow, mut refuse) = (None, None);
        let width = ui.available_width();
        let chat = self.track.chat.chat(&id).clone();
        egui::ScrollArea::vertical().id_salt(("track-chat", &id)).max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.set_width(width - 14.0);
            if chat.items.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.track_chat_intro.replace("{dir}", &cwd.display().to_string())).size(12.5).color(theme.text_muted));
            }
            let bubble_w = (width * 0.78).min(720.0);
            for (k, item) in chat.items.iter().enumerate() {
                match item {
                    Item::User { text } => {
                        ui.add_space(8.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            Frame::NONE.fill(theme.accent.gamma_multiply(0.22)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                                ui.set_max_width(bubble_w);
                                ui.add(egui::Label::new(egui::RichText::new(text).size(13.0).color(theme.text)).wrap());
                            });
                        });
                    }
                    Item::Claude { text } => {
                        ui.add_space(8.0);
                        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                            ui.set_max_width(bubble_w);
                            self.track_markdown(ui, text, bubble_w - 24.0);
                        });
                    }
                    Item::Tool { name, detail } => {
                        let verb = tool_said(t, name);
                        let line = match (verb.is_empty(), detail.is_empty()) {
                            (true, _) => format!("·  {name} {detail}"),
                            (false, true) => format!("·  {verb}"),
                            (false, false) => format!("·  {verb} {detail}"),
                        };
                        ui.add_space(2.0);
                        ui.add(egui::Label::new(egui::RichText::new(line).size(11.5).monospace().color(theme.text_muted.gamma_multiply(0.75))).truncate());
                    }
                    Item::Ask { denials, answered } => {
                        ui.add_space(8.0);
                        let orange = Color32::from_rgb(0xe0, 0xa8, 0x3c);
                        Frame::NONE.fill(orange.gamma_multiply(0.12)).stroke(Stroke::new(1.0, orange.gamma_multiply(0.6))).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                            ui.set_max_width(bubble_w);
                            ui.label(egui::RichText::new(t.track_chat_ask).size(12.5).strong().color(theme.text));
                            for d in denials {
                                let verb = tool_said(t, &d.tool);
                                let verb = if verb.is_empty() { d.tool.as_str() } else { verb };
                                ui.add(egui::Label::new(egui::RichText::new(format!("•  {verb}  {}", d.detail)).size(12.0).monospace().color(theme.text)).wrap());
                            }
                            ui.add_space(6.0);
                            match answered {
                                None if !running => {
                                    ui.horizontal(|ui| {
                                        if ui.add(egui::Button::new(egui::RichText::new(t.track_chat_allow).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0)).clicked() {
                                            allow = Some(k);
                                        }
                                        if ui.add(egui::Button::new(egui::RichText::new(t.track_chat_refuse).size(12.5)).corner_radius(6.0)).clicked() {
                                            refuse = Some(k);
                                        }
                                    });
                                }
                                None => {}
                                Some(true) => {
                                    ui.label(egui::RichText::new(t.track_chat_allowed).size(11.5).color(theme.text_muted));
                                }
                                Some(false) => {
                                    ui.label(egui::RichText::new(t.track_chat_refused).size(11.5).color(theme.text_muted));
                                }
                            }
                        });
                    }
                    Item::Error { text } => {
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(Color32::from_rgb(0xe0, 0x55, 0x55))).wrap());
                    }
                    Item::Stopped => {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(t.track_chat_stopped).size(11.5).color(theme.text_muted));
                    }
                }
            }
            if running {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(t.track_chat_working).size(12.0).color(theme.text_muted));
                });
            }
            ui.add_space(6.0);
        });

        // What to send: Return sends, Shift Return goes to the line.
        ui.add_space(6.0);
        let input_id = egui::Id::new(("track-chat-input", &id));
        let focused = ui.memory(|m| m.has_focus(input_id));
        let enter = focused && !running && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let mut send = None;
        let mut stop = false;
        let mut clear = false;
        let mut accept = chat.accept_edits;
        ui.horizontal(|ui| {
            let text = self.track.chat.input.entry(id.clone()).or_default();
            let w = ui.available_width() - 110.0;
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, if focused { theme.accent } else { theme.tab_hover })).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(text).id(input_id).frame(egui::Frame::NONE).desired_rows(2).desired_width(w - 16.0).hint_text(t.track_chat_hint));
            });
            ui.vertical(|ui| {
                if running {
                    if ui.add(egui::Button::new(egui::RichText::new(format!("■  {}", t.track_chat_stop)).size(13.0)).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() {
                        stop = true;
                    }
                } else {
                    let ready = !text.trim().is_empty();
                    if (ui.add_enabled(ready, egui::Button::new(egui::RichText::new(t.track_chat_send).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(100.0, 30.0))).clicked() || (enter && ready)) && ready {
                        send = Some(std::mem::take(text).trim().to_owned());
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!running, |ui| {
                ui.checkbox(&mut accept, egui::RichText::new(t.track_chat_accept_edits).size(11.5).color(theme.text_muted));
            });
            ui.label(egui::RichText::new(format!("·  {}", cwd.display())).size(11.0).color(theme.text_muted.gamma_multiply(0.7)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !chat.items.is_empty() && !running {
                    let confirming = self.track.chat.confirm_clear.as_deref() == Some(id.as_str());
                    let label = if confirming { t.track_chat_clear_sure } else { t.track_chat_clear };
                    if ui.add(egui::Button::new(egui::RichText::new(label).size(11.5).color(theme.text_muted)).frame_when_inactive(false)).clicked() {
                        if confirming {
                            clear = true;
                        } else {
                            self.track.chat.confirm_clear = Some(id.clone());
                        }
                    }
                }
            });
        });

        if accept != chat.accept_edits {
            let c = self.track.chat.chat(&id);
            c.accept_edits = accept;
            c.save(&id);
        }
        if stop && let Some(r) = self.track.chat.running.remove(&id) {
            r.stop();
            let c = self.track.chat.chat(&id);
            c.items.push(Item::Stopped);
            c.save(&id);
        }
        if clear {
            self.track.chat.confirm_clear = None;
            Chat::clear(&id);
            self.track.chat.chats.remove(&id);
        }
        if let Some(text) = send {
            self.track.chat.confirm_clear = None;
            self.track_chat_send(p, text, Vec::new());
        }
        if let Some(k) = allow {
            let c = self.track.chat.chat(&id);
            if let Some(Item::Ask { denials, answered }) = c.items.get_mut(k) {
                *answered = Some(true);
                let rules: Vec<String> = denials.iter().map(|d| d.rule.clone()).collect();
                let prompt = t.track_chat_allowed_prompt.to_owned();
                self.track_chat_send(p, prompt, rules);
            }
        }
        if let Some(k) = refuse {
            let c = self.track.chat.chat(&id);
            if let Some(Item::Ask { answered, .. }) = c.items.get_mut(k) {
                *answered = Some(false);
            }
            c.save(&id);
        }
    }

    /// Claude's answer, in light Markdown: headings, lists, code blocks, `code` and **bold**.
    fn track_markdown(&self, ui: &mut Ui, text: &str, width: f32) {
        let theme = &self.theme;
        let code_bg = theme.bg;
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                let mut code = Vec::new();
                for l in lines.by_ref() {
                    if l.trim_start().starts_with("```") {
                        break;
                    }
                    code.push(l);
                }
                Frame::NONE.fill(code_bg).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                    ui.set_width(width - 16.0);
                    ui.add(egui::Label::new(egui::RichText::new(code.join("\n")).size(12.0).monospace().color(theme.text)).wrap());
                });
                continue;
            }
            if trimmed.is_empty() {
                ui.add_space(4.0);
                continue;
            }
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if level > 0 && trimmed[level..].starts_with(' ') {
                let size = if level == 1 { 16.0 } else { 14.0 };
                ui.add(egui::Label::new(inline_job(trimmed[level..].trim(), size, theme.text, code_bg, width, true)).wrap());
                continue;
            }
            // A list item: a bullet, indented as written.
            let indent = (line.len() - trimmed.len()) as f32 * 4.0;
            let bullet = ["- ", "* ", "+ "].iter().find_map(|b| trimmed.strip_prefix(b));
            match bullet {
                Some(rest) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(indent);
                        ui.label(egui::RichText::new("•").size(13.0).color(theme.text_muted));
                        ui.add(egui::Label::new(inline_job(rest, 13.0, theme.text, code_bg, width - indent - 16.0, false)).wrap());
                    });
                }
                None => {
                    ui.add(egui::Label::new(inline_job(trimmed, 13.0, theme.text, code_bg, width, false)).wrap());
                }
            }
        }
    }
}
