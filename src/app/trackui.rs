//! The tracking's pages (a sub-module of `track`): every project as cards, or one project with its
//! tabs — overview, time spent (by day, filtered, copied as CSV), quotes with their planned tasks and
//! sub-tasks, hour counters, team, Claude — and the windows to write in (time spent, a quote, a counter,
//! a planned task), their descriptions in Markdown with a toolbar.

use std::collections::HashSet;

use super::super::mdedit::{first_line, md_editor, md_view};
use super::*;
use crate::track::{Item, ItemInput, Quote, Status, Task};
use egui::Id;

/// The time spent shown: when.
#[derive(Default, PartialEq, Clone, Copy)]
pub(super) enum Period {
    #[default]
    All,
    Week,
    Month,
    LastMonth,
}

/// Time spent being written (`editing`: an entry changed).
#[derive(Default)]
pub(super) struct EntryForm {
    editing: Option<String>,
    day: String,
    duration: String,
    label: String,
    billing: Billing,
    counter: Option<String>,
    quote: Option<String>,
    item: Option<String>,
}

#[derive(Default)]
pub(super) struct QuoteForm {
    editing: Option<String>,
    number: String,
    label: String,
    hours: String,
    day: String,
    note: String,
}

#[derive(Default)]
pub(super) struct CounterForm {
    editing: Option<String>,
    label: String,
    hours: String,
    day: String,
}

#[derive(Default)]
pub(super) struct ItemForm {
    editing: Option<String>,
    quote: Option<String>,
    parent: Option<String>,
    title: String,
    body: String,
    hours: String,
    status: Status,
}

/// What the project's page keeps between frames.
pub(super) struct PageState {
    pub(super) tab: Tab,
    entry: Option<EntryForm>,
    quote: Option<QuoteForm>,
    counter: Option<CounterForm>,
    item: Option<ItemForm>,
    /// An error said in the window open.
    form_error: Option<String>,
    period: Period,
    billing: Option<Billing>,
    author: Option<String>,
    search: String,
    /// Quotes unfolded, planned tasks whose description is shown, long entries shown whole.
    open_quotes: HashSet<String>,
    open_items: HashSet<String>,
    open_entries: HashSet<String>,
    /// Something whose delete was clicked once (a second click deletes it).
    confirm: Option<String>,
    share: (String, Role),
    /// The projects' cards: a search.
    find: String,
    /// Debug builds (`RONNIE_OPEN_TRACK`): a window to open once the project is there.
    pub(super) demo: Option<String>,
}

impl Default for PageState {
    fn default() -> Self {
        PageState {
            tab: Tab::default(),
            entry: None,
            quote: None,
            counter: None,
            item: None,
            form_error: None,
            period: Period::All,
            billing: None,
            author: None,
            search: String::new(),
            open_quotes: HashSet::new(),
            open_items: HashSet::new(),
            open_entries: HashSet::new(),
            confirm: None,
            share: (String::new(), Role::Editor),
            find: String::new(),
            demo: None,
        }
    }
}

/// What was clicked on the page, done once it is drawn.
enum Act {
    Select(Option<String>),
    NewProject,
    Import,
    Edit,
    OpenProfile(Uuid),
    Refresh,
    NewEntry { quote: Option<String>, item: Option<String> },
    EditEntry(Task),
    DeleteEntry(String),
    /// An entry's text changed in place (a box ticked).
    EntryText(String, String),
    NewQuote,
    EditQuote(String),
    CloseQuote(String, bool),
    DeleteQuote(String),
    QuoteNote(String, String),
    NewCounter(Option<u64>),
    EditCounter(String),
    CloseCounter(String, bool),
    DeleteCounter(String),
    NewItem { quote: Option<String>, parent: Option<String> },
    EditItem(String),
    SetStatus(String, Status),
    DeleteItem(String),
    ItemBody(String, String),
    Move(String, i64),
    Share,
    SetRole(String, Role),
    Unshare(String),
    Tab(Tab),
    CopyCsv,
}

/// A card: the chrome's color, a thin edge, round corners.
fn card<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(egui::Margin::same(16)).show(ui, |ui| add(ui)).inner
}

/// A small rounded label.
fn chip(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    Frame::NONE.fill(color.gamma_multiply(0.16)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(color)).truncate())).response
}

/// A filled button, the main action.
fn primary(text: &str, theme: &Theme) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(8.0).min_size(Vec2::new(0.0, 32.0))
}

/// A quiet button.
fn quiet(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).size(12.5)).corner_radius(8.0).min_size(Vec2::new(0.0, 30.0))
}

/// A link-like button.
fn link_button(text: &str, color: Color32) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).size(12.0).color(color)).frame_when_inactive(false)
}

/// A bar: `fraction` of it filled.
fn gauge(ui: &mut Ui, width: f32, fraction: f32, color: Color32, theme: &Theme) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), Sense::hover());
    ui.painter().rect_filled(r, 3.0, theme.tab_hover);
    let w = r.width() * fraction.clamp(0.0, 1.0);
    if w > 0.0 {
        ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(w.max(6.0), r.height())), 3.0, color);
    }
}

/// A person's initial in a colored circle.
fn initial(ui: &mut Ui, name: &str, theme: &Theme) {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
    let hue = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let color = theme.ansi[1 + (hue % 6) as usize];
    ui.painter().circle_filled(r.center(), 11.0, color.gamma_multiply(0.85));
    let letter = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    ui.painter().text(r.center(), Align2::CENTER_CENTER, letter, FontId::proportional(11.5), theme.bg);
    resp.on_hover_text(name);
}

/// A section's title in a page.
fn heading(ui: &mut Ui, text: &str, theme: &Theme) {
    ui.label(egui::RichText::new(text).size(14.5).strong().color(theme.text));
}

/// Hours as typed in a form, from minutes: "1h30".
fn typed(minutes: u64) -> String {
    hm(minutes as i64).replace(' ', "").replace("min", "m")
}

/// Today's month: "2026-11".
fn month_of(day: &str) -> &str {
    day.get(..7).unwrap_or(day)
}

impl App {
    /// Every project, as cards by client.
    pub(super) fn track_overview(&mut self, ui: &mut Ui) {
        let t = self.t();
        let theme = self.theme.clone();
        let mut acts = Vec::new();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.cat_track).size(24.0).strong().color(theme.text));
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut self.track.page.find).hint_text(t.track_find).desired_width(220.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(primary(&format!("+  {}", t.track_new), &theme)).clicked() {
                    acts.push(Act::NewProject);
                }
                if ui.add(quiet(t.track_import_row)).clicked() {
                    acts.push(Act::Import);
                }
                if self.track.busy > 0 {
                    ui.spinner();
                } else if ui.add(link_button("⟳", theme.text_muted)).on_hover_text(t.track_refresh).clicked() {
                    acts.push(Act::Refresh);
                }
                let archived = self.track.list.as_ref().and_then(|l| l.as_ref().ok()).is_some_and(|l| l.iter().any(|p| p.archived.is_some()));
                if archived {
                    ui.checkbox(&mut self.track.show_archived, egui::RichText::new(t.track_show_archived).size(12.0).color(theme.text_muted));
                }
            });
        });
        ui.add_space(2.0);
        ui.label(egui::RichText::new(t.track_intro).size(12.5).color(theme.text_muted.gamma_multiply(0.85)));
        ui.add_space(16.0);

        match &self.track.list {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(t.track_loading).color(theme.text_muted));
                });
                return;
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(t.track_offline).size(12.5).color(theme.ansi[3]));
                ui.label(egui::RichText::new(e).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                return;
            }
            Some(Ok(_)) => {}
        }
        let find = self.track.page.find.trim().to_lowercase();
        let shown: Vec<Summary> = self.track.shown().into_iter().filter(|s| find.is_empty() || s.name.to_lowercase().contains(&find) || s.client.as_ref().is_some_and(|c| c.to_lowercase().contains(&find))).collect();
        if shown.is_empty() {
            card(ui, &theme, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(if find.is_empty() { t.track_none } else { t.track_none_found }).size(13.0).color(theme.text_muted));
            });
        }
        egui::ScrollArea::vertical().id_salt("track-overview").auto_shrink([false, true]).show(ui, |ui| {
            let width = ui.available_width();
            let cols = ((width / 330.0).floor() as usize).clamp(1, 4);
            let gap = 12.0;
            let w = (width - gap * (cols - 1) as f32) / cols as f32;
            let mut client: Option<Option<String>> = None;
            let mut groups: Vec<(Option<String>, Vec<Summary>)> = Vec::new();
            for s in shown {
                if client.as_ref() != Some(&s.client) {
                    client = Some(s.client.clone());
                    groups.push((s.client.clone(), Vec::new()));
                }
                if let Some(g) = groups.last_mut() {
                    g.1.push(s);
                }
            }
            for (client, projects) in groups {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(client.as_deref().unwrap_or(t.track_no_client).to_uppercase()).size(11.5).strong().color(theme.text_muted));
                ui.add_space(6.0);
                for row in projects.chunks(cols) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for s in row {
                            let p = self.track.projects.get(&s.id).cloned();
                            let resp = ui.allocate_ui_with_layout(Vec2::new(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                let color = color_of(s.color.as_deref()).unwrap_or(theme.accent);
                                Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(egui::Margin { left: 18, right: 14, top: 14, bottom: 14 }).show(ui, |ui| {
                                    ui.set_width(w - 32.0);
                                    let top = ui.min_rect().min;
                                    ui.painter().rect_filled(Rect::from_min_size(top - Vec2::new(18.0, 14.0), Vec2::new(4.0, 200.0)).intersect(ui.clip_rect()), 0.0, Color32::TRANSPARENT);
                                    ui.horizontal(|ui| {
                                        let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                        ui.painter().circle_filled(dot.center(), 5.0, color);
                                        ui.add(egui::Label::new(egui::RichText::new(&s.name).size(15.0).strong().color(theme.text)).truncate());
                                    });
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(role_name(t, s.role)).size(11.5).color(theme.text_muted));
                                        if s.members > 1 {
                                            ui.label(egui::RichText::new(format!("·  {}", t.track_people_n.replace("{n}", &s.members.to_string()))).size(11.5).color(theme.text_muted));
                                        }
                                        if s.archived.is_some() {
                                            ui.label(egui::RichText::new(format!("·  {}", t.track_archived)).size(11.5).color(theme.text_muted));
                                        }
                                    });
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(hm(s.spent as i64)).size(18.0).color(theme.text));
                                        ui.label(egui::RichText::new(t.track_spent_word).size(11.5).color(theme.text_muted));
                                    });
                                    match p.as_ref().and_then(Project::current_counter) {
                                        Some(c) => {
                                            ui.add_space(6.0);
                                            let fraction = if c.minutes > 0 { c.used as f32 / c.minutes as f32 } else { 1.0 };
                                            gauge(ui, w - 32.0, fraction, left_color_bar(&theme, c.left(), c.minutes), &theme);
                                            ui.label(egui::RichText::new(t.track_counter_short.replace("{left}", &hm(c.left())).replace("{of}", &hm(c.minutes as i64))).size(11.5).color(left_color(&theme, c.left(), c.minutes)));
                                        }
                                        None => {
                                            ui.add_space(6.0);
                                            ui.label(egui::RichText::new(t.track_no_counter).size(11.5).color(theme.text_muted.gamma_multiply(0.7)));
                                        }
                                    }
                                });
                            });
                            let rect = resp.response.rect;
                            let click = ui.interact(rect, ui.id().with(("track-card", &s.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                            if click.hovered() {
                                ui.painter().rect_stroke(rect, 12.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                            }
                            if click.clicked() {
                                acts.push(Act::Select(Some(s.id.clone())));
                            }
                        }
                    });
                    ui.add_space(gap);
                }
                ui.add_space(8.0);
            }
        });
        for act in acts {
            self.track_page_act(act, None);
        }
    }

    /// One project: its head, its tabs.
    pub(super) fn track_project(&mut self, ui: &mut Ui, p: &Project) {
        let t = self.t();
        let theme = self.theme.clone();
        let mut acts = Vec::new();
        let profile = self.config.settings.track_profiles.get(&p.id).copied().filter(|id| self.config.profiles.iter().any(|x| x.id == *id));

        // The way back, the project's name, its actions.
        ui.horizontal(|ui| {
            if ui.add(link_button(&format!("‹  {}", t.cat_track), theme.text_muted)).clicked() {
                acts.push(Act::Select(None));
            }
            if let Some(c) = &p.client {
                ui.label(egui::RichText::new(format!("/  {c}")).size(12.0).color(theme.text_muted));
            }
        });
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 7.0, color_of(p.color.as_deref()).unwrap_or(theme.accent));
            ui.label(egui::RichText::new(&p.name).size(24.0).strong().color(theme.text));
            ui.add_space(6.0);
            chip(ui, role_name(t, p.role), theme.text_muted);
            if p.archived.is_some() {
                chip(ui, t.track_archived, theme.ansi[3]);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if p.role.writes() && ui.add(primary(&format!("+  {}", t.track_log_time), &theme)).clicked() {
                    acts.push(Act::NewEntry { quote: None, item: None });
                }
                if ui.add(quiet(&format!("⚙  {}", t.track_settings))).clicked() {
                    acts.push(Act::Edit);
                }
                if let Some(profile) = profile
                    && ui.add(quiet(t.track_open_tab)).clicked()
                {
                    acts.push(Act::OpenProfile(profile));
                }
                if self.track.busy > 0 {
                    ui.spinner();
                }
            });
        });
        ui.add_space(14.0);

        // The tabs, underlined.
        let tabs = [
            (Tab::Overview, t.track_tab_overview.to_owned()),
            (Tab::Time, format!("{}  {}", t.track_tab_time, p.tasks.len())),
            (Tab::Quotes, format!("{}  {}", t.track_tab_quotes, p.quotes.len())),
            (Tab::Counters, format!("{}  {}", t.track_tab_counters, p.counters.len())),
            (Tab::Team, format!("{}  {}", t.track_tab_access, p.members.len())),
            (Tab::Claude, if self.track.chat.is_running(&p.id) { "Claude  ●".to_owned() } else { "Claude".to_owned() }),
        ];
        let bar_top = ui.cursor().min.y;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            for (tab, label) in tabs {
                let on = self.track.page.tab == tab;
                let text = egui::RichText::new(label).size(13.5).color(if on { theme.text } else { theme.text_muted });
                let resp = ui.add(egui::Button::new(if on { text.strong() } else { text }).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand);
                if on {
                    let r = resp.rect;
                    ui.painter().rect_filled(Rect::from_min_max(Pos2::new(r.min.x, r.max.y + 6.0), Pos2::new(r.max.x, r.max.y + 8.5)), 1.5, theme.accent);
                }
                if resp.clicked() {
                    acts.push(Act::Tab(tab));
                }
            }
        });
        let line_y = ui.cursor().min.y + 4.0;
        let _ = bar_top;
        ui.painter().hline(ui.max_rect().x_range(), line_y + 4.5, Stroke::new(1.0, theme.tab_hover));
        ui.add_space(18.0);

        if self.track.page.tab == Tab::Claude {
            self.track_chat_ui(ui, p);
        } else {
            egui::ScrollArea::vertical().id_salt(("track-project", &p.id, self.track.page.tab as u8)).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(ui.available_width() - 8.0);
                match self.track.page.tab {
                    Tab::Overview => self.track_tab_overview(ui, p, &mut acts),
                    Tab::Time => self.track_tab_time(ui, p, &mut acts),
                    Tab::Quotes => self.track_tab_quotes(ui, p, &mut acts),
                    Tab::Counters => self.track_tab_counters(ui, p, &mut acts),
                    Tab::Team | Tab::Claude => self.track_tab_team(ui, p, &mut acts),
                }
                ui.add_space(30.0);
            });
        }
        for act in acts {
            self.track_page_act(act, Some(p));
        }
    }

    /// What a time entry is charged to, said (the counter's or quote's name).
    fn track_charged(&self, p: &Project, task: &Task) -> String {
        let t = self.t();
        match task.billing {
            Billing::Counter => task.counter.as_ref().and_then(|c| p.counters.iter().find(|x| &x.id == c)).map_or(t.track_bill_counter.to_owned(), |c| c.label.clone()),
            Billing::Quote => task.quote.as_ref().and_then(|q| p.quotes.iter().find(|x| &x.id == q)).map_or(t.track_bill_quote.to_owned(), |q| format!("{} {}", t.track_bill_quote, q.number.clone().unwrap_or(q.label.clone()))),
            b => billing_name(t, b).to_owned(),
        }
    }

    /// A banner: the counter used up, a new one proposed (to whoever manages).
    fn track_spent_banner(&self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = &self.theme;
        if !(p.role.manages() && p.counters_spent()) {
            return;
        }
        let Some(last) = p.counters.iter().rev().find(|c| c.closed.is_none()).or(p.counters.last()) else { return };
        Frame::NONE.fill(RED.gamma_multiply(0.12)).stroke(Stroke::new(1.0, RED.gamma_multiply(0.45))).corner_radius(10.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("⚠").size(15.0).color(RED));
                ui.label(egui::RichText::new(t.track_spent_up.replace("{name}", &last.label).replace("{h}", &hm(last.left()))).size(12.5).color(theme.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary(&t.track_new_counter_of.replace("{h}", &hm(last.minutes as i64)), theme)).clicked() {
                        acts.push(Act::NewCounter(Some(last.minutes)));
                    }
                });
            });
        });
        ui.add_space(14.0);
    }

    fn track_tab_overview(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        self.track_spent_banner(ui, p, acts);
        let total: i64 = p.tasks.iter().map(|x| x.minutes as i64).sum();
        let this_month = track::today();
        let month: i64 = p.tasks.iter().filter(|x| month_of(&x.day) == month_of(&this_month)).map(|x| x.minutes as i64).sum();
        let open_quotes: Vec<&Quote> = p.quotes.iter().filter(|q| q.closed.is_none()).collect();
        let quote_roll = open_quotes.iter().fold((0u64, 0u64), |(e, s), q| {
            let r = p.quote_rollup(q);
            (e + r.estimate, s + r.spent)
        });

        // The figures.
        let width = ui.available_width();
        let cols = if width > 760.0 { 4 } else { 2 };
        let gap = 12.0;
        let w = (width - gap * (cols - 1) as f32) / cols as f32;
        let current = p.current_counter();
        let tiles: Vec<(String, String, String, Color32, Option<f32>)> = vec![
            (t.track_total.to_owned(), hm(total), t.track_tasks_n.replace("{n}", &p.tasks.len().to_string()), theme.text, None),
            (t.track_this_month.to_owned(), hm(month), String::new(), theme.text, None),
            match current {
                Some(c) => (t.track_current_counter.to_owned(), hm(c.left()), format!("{} · {}", c.label, t.track_of.replace("{h}", &hm(c.minutes as i64))), if c.left() <= 0 { RED } else { theme.text }, Some(if c.minutes > 0 { c.used as f32 / c.minutes as f32 } else { 1.0 })),
                None => (t.track_current_counter.to_owned(), "—".into(), t.track_no_counter.into(), theme.text_muted, None),
            },
            (
                t.track_open_quotes.to_owned(),
                if open_quotes.is_empty() { "—".into() } else { format!("{} / {}", hm(quote_roll.1 as i64), hm(quote_roll.0 as i64)) },
                t.track_quotes_n.replace("{n}", &open_quotes.len().to_string()),
                theme.text,
                (quote_roll.0 > 0).then(|| quote_roll.1 as f32 / quote_roll.0 as f32),
            ),
        ];
        for row in tiles.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (title, value, sub, color, fraction) in row {
                    ui.allocate_ui_with_layout(Vec2::new(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        card(ui, &theme, |ui| {
                            ui.set_width(w - 32.0);
                            ui.label(egui::RichText::new(title).size(11.5).color(theme.text_muted));
                            ui.add_space(2.0);
                            ui.label(egui::RichText::new(value).size(22.0).color(*color));
                            match fraction {
                                Some(f) => {
                                    ui.add_space(4.0);
                                    gauge(ui, w - 32.0, *f, if *f > 1.0 { RED } else { theme.accent }, &theme);
                                }
                                None => {
                                    ui.add_space(10.0);
                                }
                            }
                            ui.add(egui::Label::new(egui::RichText::new(if sub.is_empty() { " " } else { sub }).size(11.5).color(theme.text_muted.gamma_multiply(0.85))).truncate());
                        });
                    });
                }
            });
            ui.add_space(gap);
        }
        ui.add_space(6.0);

        // The last entries on the left; how the time splits and the quotes on the right.
        let wide = width > 760.0;
        let left_w = if wide { width * 0.58 } else { width };
        let right_w = if wide { width - left_w - gap } else { width };
        let mut left = |ui: &mut Ui, this: &mut App| {
            card(ui, &theme, |ui| {
                ui.set_width(left_w - 32.0);
                ui.horizontal(|ui| {
                    heading(ui, t.track_last_entries, &theme);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !p.tasks.is_empty() && ui.add(link_button(&format!("{}  ›", t.track_see_all), theme.accent)).clicked() {
                            acts.push(Act::Tab(Tab::Time));
                        }
                    });
                });
                ui.add_space(8.0);
                if p.tasks.is_empty() {
                    ui.label(egui::RichText::new(t.track_no_tasks).size(12.5).color(theme.text_muted));
                }
                for task in p.tasks.iter().take(6) {
                    ui.horizontal(|ui| {
                        ui.add_sized(Vec2::new(54.0, 18.0), egui::Label::new(egui::RichText::new(hm(task.minutes as i64)).size(13.0).strong().color(theme.text)));
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(egui::RichText::new(first_line(&task.label)).size(12.5).color(theme.text)).truncate());
                            ui.label(egui::RichText::new(format!("{}  ·  {}  ·  {}", show_day(&task.day), this.track_charged(p, task), task.author.as_deref().unwrap_or("?"))).size(11.0).color(theme.text_muted));
                        });
                    });
                    ui.add_space(4.0);
                }
            });
        };
        let right = |ui: &mut Ui| {
            card(ui, &theme, |ui| {
                ui.set_width(right_w - 32.0);
                heading(ui, t.track_split, &theme);
                ui.add_space(8.0);
                let total = total.max(1) as f32;
                for b in Billing::ALL {
                    let m = p.by_billing(b);
                    ui.horizontal(|ui| {
                        ui.add_sized(Vec2::new(96.0, 16.0), egui::Label::new(egui::RichText::new(billing_name(t, b)).size(12.0).color(theme.text_muted)));
                        gauge(ui, (right_w - 32.0 - 170.0).max(40.0), m as f32 / total, billing_color(&theme, b), &theme);
                        ui.label(egui::RichText::new(hm(m as i64)).size(12.0).color(theme.text));
                    });
                    ui.add_space(2.0);
                }
                if !open_quotes.is_empty() {
                    ui.add_space(12.0);
                    heading(ui, t.track_tab_quotes, &theme);
                    ui.add_space(6.0);
                    for q in &open_quotes {
                        let r = p.quote_rollup(q);
                        ui.add(egui::Label::new(egui::RichText::new(q.title()).size(12.5).color(theme.text)).truncate());
                        ui.horizontal(|ui| {
                            let f = if r.estimate > 0 { r.spent as f32 / r.estimate as f32 } else { 0.0 };
                            gauge(ui, (right_w - 32.0 - 110.0).max(40.0), f, if f > 1.0 { RED } else { theme.ansi[4] }, &theme);
                            ui.label(egui::RichText::new(format!("{} / {}", hm(r.spent as i64), if r.estimate > 0 { hm(r.estimate as i64) } else { "—".into() })).size(11.5).color(theme.text_muted));
                        });
                        ui.add_space(4.0);
                    }
                }
            });
        };
        if wide {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.allocate_ui_with_layout(Vec2::new(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| left(ui, self));
                ui.allocate_ui_with_layout(Vec2::new(right_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| right(ui));
            });
        } else {
            left(ui, self);
            ui.add_space(gap);
            right(ui);
        }

        // The project's note.
        ui.add_space(gap);
        card(ui, &theme, |ui| {
            ui.set_width(width - 32.0);
            ui.horizontal(|ui| {
                heading(ui, t.track_note, &theme);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if p.role.manages() && ui.add(link_button(t.track_edit, theme.accent)).clicked() {
                        acts.push(Act::Edit);
                    }
                });
            });
            ui.add_space(6.0);
            match p.note.as_deref().filter(|n| !n.trim().is_empty()) {
                Some(note) => {
                    md_view(ui, Id::new(("track-note", &p.id)), note, width - 32.0, &theme, t);
                }
                None => {
                    ui.label(egui::RichText::new(t.track_note_empty).size(12.5).color(theme.text_muted));
                }
            }
        });
    }

    /// The entries the filters keep.
    fn track_filtered<'a>(&self, p: &'a Project) -> Vec<&'a Task> {
        use chrono::Datelike as _;
        let s = &self.track.page;
        let today = chrono::Local::now().date_naive();
        let (from, to): (String, String) = match s.period {
            Period::All => (String::new(), "9999".into()),
            Period::Week => ((today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64)).format("%Y-%m-%d").to_string(), "9999".into()),
            Period::Month => (today.format("%Y-%m-01").to_string(), "9999".into()),
            Period::LastMonth => {
                let first = today.with_day(1).unwrap_or(today);
                let last_month = first - chrono::Duration::days(1);
                (last_month.format("%Y-%m-01").to_string(), first.format("%Y-%m-%d").to_string())
            }
        };
        let search = s.search.trim().to_lowercase();
        p.tasks
            .iter()
            .filter(|x| x.day.as_str() >= from.as_str() && x.day.as_str() < to.as_str())
            .filter(|x| s.billing.is_none_or(|b| x.billing == b))
            .filter(|x| s.author.as_ref().is_none_or(|a| x.author.as_ref() == Some(a)))
            .filter(|x| search.is_empty() || x.label.to_lowercase().contains(&search) || x.item.as_ref().is_some_and(|i| p.item_path(i).to_lowercase().contains(&search)))
            .collect()
    }

    fn track_tab_time(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        self.track_spent_banner(ui, p, acts);
        // The filters.
        let authors: Vec<String> = p.tasks.iter().filter_map(|x| x.author.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        ui.horizontal_wrapped(|ui| {
            let s = &mut self.track.page;
            ui.add(egui::TextEdit::singleline(&mut s.search).hint_text(format!("🔍  {}", t.track_search)).desired_width(200.0));
            let period = |p: Period| match p {
                Period::All => t.track_period_all,
                Period::Week => t.track_period_week,
                Period::Month => t.track_period_month,
                Period::LastMonth => t.track_period_last_month,
            };
            egui::ComboBox::from_id_salt("track-period").selected_text(period(s.period)).show_ui(ui, |ui| {
                for v in [Period::All, Period::Week, Period::Month, Period::LastMonth] {
                    ui.selectable_value(&mut s.period, v, period(v));
                }
            });
            egui::ComboBox::from_id_salt("track-billing-filter").selected_text(s.billing.map_or(t.track_filter_all, |b| billing_name(t, b))).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.billing, None, t.track_filter_all);
                for b in Billing::ALL {
                    ui.selectable_value(&mut s.billing, Some(b), billing_name(t, b));
                }
            });
            if authors.len() > 1 {
                egui::ComboBox::from_id_salt("track-author-filter").selected_text(s.author.clone().unwrap_or_else(|| t.track_everyone.to_owned())).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.author, None, t.track_everyone);
                    for a in &authors {
                        ui.selectable_value(&mut s.author, Some(a.clone()), a);
                    }
                });
            }
        });
        let shown = self.track_filtered(p);
        let sum: i64 = shown.iter().map(|x| x.minutes as i64).sum();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.track_filter_total.replace("{n}", &shown.len().to_string()).replace("{h}", &hm(sum))).size(12.5).color(theme.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !shown.is_empty() && ui.add(link_button(&format!("⧉  {}", t.track_copy_csv), theme.accent)).on_hover_text(t.track_copy_csv_hint).clicked() {
                    acts.push(Act::CopyCsv);
                }
            });
        });
        ui.add_space(10.0);
        if shown.is_empty() {
            card(ui, &theme, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(t.track_no_tasks).size(12.5).color(theme.text_muted));
            });
            return;
        }
        // By day, newest first.
        let width = ui.available_width();
        let mut day: Option<&str> = None;
        let mut k = 0;
        while k < shown.len() {
            let d = shown[k].day.as_str();
            let of_day: Vec<&Task> = shown[k..].iter().take_while(|x| x.day == d).copied().collect();
            k += of_day.len();
            if day != Some(d) {
                day = Some(d);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(long_day(d, t)).size(13.0).strong().color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(hm(of_day.iter().map(|x| x.minutes as i64).sum())).size(12.5).color(theme.text_muted));
                    });
                });
                ui.add_space(6.0);
            }
            card(ui, &theme, |ui| {
                ui.set_width(width - 32.0);
                for (n, task) in of_day.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(8.0);
                        ui.painter().hline(ui.min_rect().x_range(), ui.cursor().min.y - 4.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
                        ui.add_space(4.0);
                    }
                    let can = p.role.manages() || (p.role.writes() && task.mine);
                    ui.horizontal_top(|ui| {
                        ui.add_sized(Vec2::new(70.0, 22.0), egui::Label::new(egui::RichText::new(hm(task.minutes as i64)).size(15.0).strong().color(theme.text)));
                        let actions_w = if can { 70.0 } else { 0.0 };
                        let body_w = (width - 32.0 - 70.0 - 40.0 - actions_w).max(120.0);
                        ui.allocate_ui_with_layout(Vec2::new(body_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(body_w);
                            ui.horizontal_wrapped(|ui| {
                                chip(ui, &self.track_charged(p, task), billing_color(&theme, task.billing));
                                if let Some(item) = &task.item {
                                    chip(ui, &format!("☑  {}", p.item_path(item)), theme.text_muted);
                                }
                            });
                            ui.add_space(4.0);
                            let long = task.label.lines().count() > 6 || task.label.len() > 600;
                            let open = self.track.page.open_entries.contains(&task.id);
                            let text = if long && !open { task.label.lines().take(5).collect::<Vec<_>>().join("\n") } else { task.label.clone() };
                            let clicked = md_view(ui, Id::new(("track-entry", &task.id, open)), &text, body_w, &theme, t);
                            if let Some(new) = clicked.toggled.filter(|_| can && (!long || open)) {
                                acts.push(Act::EntryText(task.id.clone(), new));
                            }
                            if long && ui.add(link_button(if open { t.track_show_less } else { t.track_show_more }, theme.accent)).clicked() {
                                if open {
                                    self.track.page.open_entries.remove(&task.id);
                                } else {
                                    self.track.page.open_entries.insert(task.id.clone());
                                }
                            }
                        });
                        initial(ui, task.author.as_deref().unwrap_or("?"), &theme);
                        if can {
                            if ui.add(link_button("✎", theme.text_muted)).on_hover_text(t.track_edit).clicked() {
                                acts.push(Act::EditEntry((*task).clone()));
                            }
                            let confirming = self.track.page.confirm.as_deref() == Some(task.id.as_str());
                            if ui.add(link_button(if confirming { "✕ ?" } else { "✕" }, if confirming { RED } else { theme.text_muted })).on_hover_text(t.track_delete).clicked() {
                                if confirming {
                                    acts.push(Act::DeleteEntry(task.id.clone()));
                                } else {
                                    self.track.page.confirm = Some(task.id.clone());
                                }
                            }
                        }
                    });
                }
            });
            ui.add_space(8.0);
        }
    }

    fn track_tab_quotes(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.track_quotes_intro).size(12.5).color(theme.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if p.role.manages() && ui.add(primary(&format!("+  {}", t.track_new_quote), &theme)).clicked() {
                    acts.push(Act::NewQuote);
                }
            });
        });
        ui.add_space(12.0);
        let mut quotes: Vec<&Quote> = p.quotes.iter().collect();
        // The open ones first, the newest first.
        quotes.sort_by_key(|q| (q.closed.is_some(), std::cmp::Reverse(q.day.clone())));
        if quotes.is_empty() {
            card(ui, &theme, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(t.track_no_quotes).size(12.5).color(theme.text_muted));
            });
        }
        let width = ui.available_width();
        for q in quotes {
            let r = p.quote_rollup(q);
            // Open quotes unfolded, closed ones folded, but for those clicked.
            let open = (q.closed.is_none()) != self.track.page.open_quotes.contains(&q.id);
            card(ui, &theme, |ui| {
                ui.set_width(width - 32.0);
                let head = ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(if open { "▾" } else { "▸" }).size(14.0).color(theme.text_muted));
                    if let Some(n) = &q.number {
                        chip(ui, n, theme.ansi[4]);
                    }
                    ui.label(egui::RichText::new(&q.label).size(15.0).strong().color(if q.closed.is_some() { theme.text_muted } else { theme.text }));
                    chip(ui, if q.closed.is_some() { t.track_closed } else { t.track_open }, if q.closed.is_some() { theme.text_muted } else { GREEN });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if p.role.manages() {
                            ui.menu_button(egui::RichText::new("⋯").size(16.0), |ui| {
                                if ui.button(t.track_edit).clicked() {
                                    acts.push(Act::EditQuote(q.id.clone()));
                                    ui.close();
                                }
                                if ui.button(if q.closed.is_some() { t.track_reopen } else { t.track_close }).clicked() {
                                    acts.push(Act::CloseQuote(q.id.clone(), q.closed.is_none()));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(egui::RichText::new(t.track_delete).color(RED)).clicked() {
                                    acts.push(Act::DeleteQuote(q.id.clone()));
                                    ui.close();
                                }
                            });
                        }
                        ui.label(egui::RichText::new(show_day(&q.day)).size(11.5).color(theme.text_muted));
                    });
                });
                let toggle = ui.interact(head.response.rect, ui.id().with(("quote-head", &q.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if toggle.clicked() && !self.track.page.open_quotes.remove(&q.id) {
                    self.track.page.open_quotes.insert(q.id.clone());
                }
                ui.add_space(8.0);
                let f = if r.estimate > 0 { r.spent as f32 / r.estimate as f32 } else { 0.0 };
                gauge(ui, width - 32.0, f, if f > 1.0 { RED } else { theme.ansi[4] }, &theme);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let planned = if r.estimate > 0 { hm(r.estimate as i64) } else { t.track_not_estimated.to_owned() };
                    ui.label(egui::RichText::new(t.track_quote_line.replace("{used}", &hm(r.spent as i64)).replace("{of}", &planned)).size(12.0).color(if f > 1.0 { RED } else { theme.text_muted }));
                    if r.count > 0 {
                        ui.label(egui::RichText::new(format!("·  {}", t.track_done_of.replace("{k}", &r.done.to_string()).replace("{n}", &r.count.to_string()))).size(12.0).color(theme.text_muted));
                    }
                });
                if open {
                    ui.add_space(10.0);
                    if let Some(note) = q.note.as_deref().filter(|n| !n.trim().is_empty()) {
                        let clicked = md_view(ui, Id::new(("quote-note", &q.id)), note, width - 32.0, &theme, t);
                        if let Some(new) = clicked.toggled.filter(|_| p.role.manages()) {
                            acts.push(Act::QuoteNote(q.id.clone(), new));
                        }
                        ui.add_space(10.0);
                    }
                    self.track_items_tree(ui, p, Some(&q.id), width - 32.0, acts);
                }
            });
            ui.add_space(10.0);
        }
        // The project's own planned tasks (in no quote).
        let own = p.children(None, None);
        if !own.is_empty() || p.role.manages() {
            ui.add_space(8.0);
            card(ui, &theme, |ui| {
                ui.set_width(width - 32.0);
                heading(ui, t.track_own_items, &theme);
                ui.label(egui::RichText::new(t.track_own_items_hint).size(11.5).color(theme.text_muted));
                ui.add_space(8.0);
                self.track_items_tree(ui, p, None, width - 32.0, acts);
            });
        }
    }

    /// The planned tasks of a quote (or the project's own), as a tree; a task added at the end.
    fn track_items_tree(&mut self, ui: &mut Ui, p: &Project, quote: Option<&str>, width: f32, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        fn walk<'a>(p: &'a Project, quote: Option<&str>, parent: Option<&str>, depth: usize, out: &mut Vec<(&'a Item, usize)>) {
            for i in p.children(quote, parent) {
                out.push((i, depth));
                walk(p, quote, Some(&i.id), depth + 1, out);
            }
        }
        let mut rows = Vec::new();
        walk(p, quote, None, 0, &mut rows);
        if rows.is_empty() {
            ui.label(egui::RichText::new(t.track_no_items).size(12.0).color(theme.text_muted));
        }
        for (k, (item, depth)) in rows.iter().enumerate() {
            let r = p.rollup(item);
            let indent = *depth as f32 * 24.0;
            let open = self.track.page.open_items.contains(&item.id);
            let siblings = p.children(quote, item.parent.as_deref());
            let at = siblings.iter().position(|s| s.id == item.id).unwrap_or(0);
            let row = ui.horizontal(|ui| {
                ui.add_space(indent);
                // The state: a box (to do / done), a dot when going on.
                let (b, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.track_status_hint);
                let color = match item.status {
                    Status::Done => GREEN,
                    Status::Doing => ORANGE,
                    Status::Todo => theme.text_muted,
                };
                ui.painter().rect_stroke(b.shrink(2.0), 4.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
                match item.status {
                    Status::Done => {
                        ui.painter().rect_filled(b.shrink(2.0), 4.0, GREEN);
                        ui.painter().line_segment([b.center() + Vec2::new(-4.0, 0.0), b.center() + Vec2::new(-1.0, 3.0)], Stroke::new(1.8, theme.bg));
                        ui.painter().line_segment([b.center() + Vec2::new(-1.0, 3.0), b.center() + Vec2::new(4.5, -3.5)], Stroke::new(1.8, theme.bg));
                    }
                    Status::Doing => {
                        ui.painter().circle_filled(b.center(), 3.5, ORANGE);
                    }
                    Status::Todo => {}
                }
                if resp.clicked() && p.role.writes() {
                    let next = match item.status {
                        Status::Todo => Status::Doing,
                        Status::Doing => Status::Done,
                        Status::Done => Status::Todo,
                    };
                    acts.push(Act::SetStatus(item.id.clone(), next));
                }
                let title = egui::RichText::new(&item.title).size(13.5).color(if item.status == Status::Done { theme.text_muted } else { theme.text });
                let title = if item.status == Status::Done { title.strikethrough() } else { title };
                let has_body = item.body.as_deref().is_some_and(|b| !b.trim().is_empty());
                let resp = ui.add(egui::Label::new(if *depth == 0 { title.strong() } else { title }).sense(Sense::click()));
                if has_body {
                    let arrow = ui.add(link_button(if open { "▾" } else { "▸" }, theme.text_muted)).on_hover_text(t.track_details);
                    if arrow.clicked() || resp.clicked() {
                        if open {
                            self.track.page.open_items.remove(&item.id);
                        } else {
                            self.track.page.open_items.insert(item.id.clone());
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if p.role.manages() {
                        ui.menu_button(egui::RichText::new("⋯").size(15.0), |ui| {
                            if ui.button(t.track_edit).clicked() {
                                acts.push(Act::EditItem(item.id.clone()));
                                ui.close();
                            }
                            if *depth < 2 && ui.button(format!("+  {}", t.track_new_subtask)).clicked() {
                                acts.push(Act::NewItem { quote: quote.map(str::to_owned), parent: Some(item.id.clone()) });
                                ui.close();
                            }
                            if at > 0 && ui.button(format!("↑  {}", t.track_move_up)).clicked() {
                                acts.push(Act::Move(item.id.clone(), -1));
                                ui.close();
                            }
                            if at + 1 < siblings.len() && ui.button(format!("↓  {}", t.track_move_down)).clicked() {
                                acts.push(Act::Move(item.id.clone(), 1));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(egui::RichText::new(t.track_delete).color(RED)).clicked() {
                                acts.push(Act::DeleteItem(item.id.clone()));
                                ui.close();
                            }
                        });
                    }
                    if p.role.writes() && ui.add(link_button("⏱", theme.text_muted)).on_hover_text(t.track_log_on_item).clicked() {
                        acts.push(Act::NewEntry { quote: quote.map(str::to_owned), item: Some(item.id.clone()) });
                    }
                    // Spent of the estimate.
                    let est = if r.estimate > 0 { hm(r.estimate as i64) } else { "—".into() };
                    let over = r.estimate > 0 && r.spent > r.estimate;
                    ui.label(egui::RichText::new(format!("{} / {est}", hm(r.spent as i64))).size(11.5).color(if over { RED } else { theme.text_muted }));
                    if r.estimate > 0 {
                        gauge(ui, 60.0, r.spent as f32 / r.estimate as f32, if over { RED } else { theme.ansi[4] }, &theme);
                    }
                    if r.count > 1 {
                        ui.label(egui::RichText::new(format!("{}/{}", r.done, r.count)).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                    }
                });
            });
            let _ = (k, row);
            if open && let Some(body) = item.body.as_deref() {
                ui.horizontal(|ui| {
                    ui.add_space(indent + 28.0);
                    ui.vertical(|ui| {
                        let clicked = md_view(ui, Id::new(("item-body", &item.id)), body, width - indent - 40.0, &theme, t);
                        if let Some(new) = clicked.toggled.filter(|_| p.role.manages()) {
                            acts.push(Act::ItemBody(item.id.clone(), new));
                        }
                    });
                });
            }
            ui.add_space(3.0);
        }
        if p.role.manages() {
            ui.add_space(4.0);
            if ui.add(link_button(&format!("+  {}", t.track_new_item), theme.accent)).clicked() {
                acts.push(Act::NewItem { quote: quote.map(str::to_owned), parent: None });
            }
        }
    }

    fn track_tab_counters(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        self.track_spent_banner(ui, p, acts);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.track_counters_intro).size(12.5).color(theme.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if p.role.manages() && ui.add(primary(&format!("+  {}", t.track_new_counter), &theme)).clicked() {
                    acts.push(Act::NewCounter(None));
                }
            });
        });
        ui.add_space(12.0);
        if p.counters.is_empty() {
            card(ui, &theme, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(t.track_no_counter).size(12.5).color(theme.text_muted));
            });
        }
        let width = ui.available_width();
        for c in p.counters.iter().rev() {
            let open = self.track.page.open_quotes.contains(&c.id);
            card(ui, &theme, |ui| {
                ui.set_width(width - 32.0);
                let head = ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(if open { "▾" } else { "▸" }).size(14.0).color(theme.text_muted));
                    ui.label(egui::RichText::new(&c.label).size(15.0).strong().color(if c.closed.is_some() { theme.text_muted } else { theme.text }));
                    let (state, color) = if c.closed.is_some() {
                        (t.track_closed, theme.text_muted)
                    } else if c.left() <= 0 {
                        (t.track_used_up, RED)
                    } else {
                        (t.track_open, GREEN)
                    };
                    chip(ui, state, color);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if p.role.manages() {
                            ui.menu_button(egui::RichText::new("⋯").size(16.0), |ui| {
                                if ui.button(t.track_edit).clicked() {
                                    acts.push(Act::EditCounter(c.id.clone()));
                                    ui.close();
                                }
                                if ui.button(if c.closed.is_some() { t.track_reopen } else { t.track_close }).clicked() {
                                    acts.push(Act::CloseCounter(c.id.clone(), c.closed.is_none()));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(egui::RichText::new(t.track_delete).color(RED)).clicked() {
                                    acts.push(Act::DeleteCounter(c.id.clone()));
                                    ui.close();
                                }
                            });
                        }
                        ui.label(egui::RichText::new(t.track_since_day.replace("{d}", &show_day(&c.day))).size(11.5).color(theme.text_muted));
                    });
                });
                if ui.interact(head.response.rect, ui.id().with(("counter-head", &c.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if open {
                        self.track.page.open_quotes.remove(&c.id);
                    } else {
                        self.track.page.open_quotes.insert(c.id.clone());
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    for (label, value, color) in [(t.track_bought, hm(c.minutes as i64), theme.text), (t.track_used, hm(c.used as i64), theme.text), (t.track_remaining, hm(c.left()), left_color(&theme, c.left(), c.minutes))] {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(label).size(11.0).color(theme.text_muted));
                            ui.label(egui::RichText::new(value).size(18.0).color(if color == theme.text_muted { theme.text } else { color }));
                        });
                        ui.add_space(26.0);
                    }
                });
                ui.add_space(6.0);
                let fraction = if c.minutes > 0 { c.used as f32 / c.minutes as f32 } else { 1.0 };
                gauge(ui, width - 32.0, fraction, left_color_bar(&theme, c.left(), c.minutes), &theme);
                if open {
                    ui.add_space(10.0);
                    let entries: Vec<&Task> = p.tasks.iter().filter(|x| x.counter.as_deref() == Some(c.id.as_str())).collect();
                    if entries.is_empty() {
                        ui.label(egui::RichText::new(t.track_no_tasks).size(12.0).color(theme.text_muted));
                    }
                    for task in entries {
                        ui.horizontal(|ui| {
                            ui.add_sized(Vec2::new(80.0, 16.0), egui::Label::new(egui::RichText::new(show_day(&task.day)).size(11.5).color(theme.text_muted)));
                            ui.add_sized(Vec2::new(56.0, 16.0), egui::Label::new(egui::RichText::new(hm(task.minutes as i64)).size(12.0).strong().color(theme.text)));
                            ui.add(egui::Label::new(egui::RichText::new(first_line(&task.label)).size(12.0).color(theme.text)).truncate());
                        });
                    }
                }
            });
            ui.add_space(10.0);
        }
    }

    fn track_tab_team(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.label(egui::RichText::new(t.track_access_intro).size(12.5).color(theme.text_muted));
        ui.add_space(12.0);
        let width = ui.available_width();
        card(ui, &theme, |ui| {
            ui.set_width(width - 32.0);
            for (n, m) in p.members.iter().enumerate() {
                if n > 0 {
                    ui.add_space(6.0);
                    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().min.y - 3.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    initial(ui, &m.name, &theme);
                    let name = if m.me { format!("{}  ({})", m.name, t.track_you) } else { m.name.clone() };
                    ui.label(egui::RichText::new(name).size(13.5).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let changeable = p.role.manages() && m.role != Role::Owner;
                        if changeable || (m.me && m.role != Role::Owner) {
                            let confirming = self.track.page.confirm.as_deref() == Some(m.id.as_str());
                            let word = if m.me { t.track_leave } else { t.track_remove };
                            if ui.add(link_button(if confirming { t.track_confirm_delete } else { word }, if confirming { RED } else { theme.text_muted })).clicked() {
                                if confirming {
                                    acts.push(Act::Unshare(m.id.clone()));
                                } else {
                                    self.track.page.confirm = Some(m.id.clone());
                                }
                            }
                        }
                        if changeable {
                            let mut role = m.role;
                            egui::ComboBox::from_id_salt(("track-role", &m.id)).selected_text(role_name(t, role)).width(170.0).show_ui(ui, |ui| {
                                for r in Role::GIVEN {
                                    ui.selectable_value(&mut role, r, role_name(t, r));
                                }
                            });
                            if role != m.role {
                                acts.push(Act::SetRole(m.name.clone(), role));
                            }
                        } else {
                            chip(ui, role_name(t, m.role), theme.text_muted);
                        }
                    });
                });
            }
        });
        if !p.role.manages() {
            return;
        }
        ui.add_space(14.0);
        card(ui, &theme, |ui| {
            ui.set_width(width - 32.0);
            heading(ui, t.track_share_title, &theme);
            ui.add_space(8.0);
            let candidates: Vec<String> = self.track.people.iter().filter(|n| !p.members.iter().any(|m| m.name.eq_ignore_ascii_case(n))).cloned().collect();
            ui.horizontal_wrapped(|ui| {
                let (name, role) = &mut self.track.page.share;
                let said = if name.is_empty() { t.track_pick_colleague.to_owned() } else { name.clone() };
                egui::ComboBox::from_id_salt("track-share-name").selected_text(said).width(180.0).show_ui(ui, |ui| {
                    for n in &candidates {
                        ui.selectable_value(name, n.clone(), n);
                    }
                    if candidates.is_empty() {
                        ui.label(egui::RichText::new(t.track_no_colleague).color(theme.text_muted));
                    }
                });
                egui::ComboBox::from_id_salt("track-share-role").selected_text(role_name(t, *role)).width(190.0).show_ui(ui, |ui| {
                    for r in Role::GIVEN {
                        ui.selectable_value(role, r, role_name(t, r));
                    }
                });
                if ui.add_enabled(!name.is_empty(), primary(t.track_share, &theme)).clicked() {
                    acts.push(Act::Share);
                }
            });
            ui.add_space(10.0);
            for r in Role::GIVEN {
                let what = match r {
                    Role::Admin => t.track_role_admin_hint,
                    Role::Editor => t.track_role_editor_hint,
                    _ => t.track_role_viewer_hint,
                };
                ui.horizontal(|ui| {
                    chip(ui, role_name(t, r), theme.accent);
                    ui.label(egui::RichText::new(what).size(12.0).color(theme.text_muted));
                });
            }
        });
    }

    /// The entry window, ready for `project`: today, its current counter (or the quote of `item`).
    fn track_entry_open(&mut self, p: &Project, quote: Option<String>, item: Option<String>) {
        let mut f = EntryForm { day: show_day(&track::today()), ..EntryForm::default() };
        let quote = quote.or_else(|| item.as_ref().and_then(|i| p.items.iter().find(|x| &x.id == i)).and_then(|i| i.quote.clone()));
        match (&quote, p.current_counter()) {
            (Some(q), _) => {
                f.billing = Billing::Quote;
                f.quote = Some(q.clone());
            }
            (None, Some(c)) => {
                f.billing = Billing::Counter;
                f.counter = Some(c.id.clone());
            }
            (None, None) => f.billing = Billing::Maintenance,
        }
        f.item = item;
        self.track.page.form_error = None;
        self.track.page.entry = Some(f);
    }

    /// Does what was clicked.
    fn track_page_act(&mut self, act: Act, p: Option<&Project>) {
        let id = p.map(|p| p.id.clone()).unwrap_or_default();
        match act {
            Act::Select(sel) => self.track_select(sel),
            Act::NewProject => self.track.new_project = Some(NewProject { name: String::new(), client: String::new(), color: None, profile: None }),
            Act::Import => self.track_import_open(),
            Act::Edit => self.track_edit_open(),
            Act::OpenProfile(profile) => self.open_profile(profile),
            Act::Refresh => {
                self.track_refresh();
                if !id.is_empty() {
                    self.track_fetch(&id, false);
                }
            }
            Act::Tab(tab) => {
                self.track.page.tab = tab;
                self.track.page.confirm = None;
            }
            Act::NewEntry { quote, item } => {
                if let Some(p) = p {
                    self.track_entry_open(p, quote, item);
                }
            }
            Act::EditEntry(task) => {
                self.track.page.form_error = None;
                self.track.page.entry = Some(EntryForm {
                    editing: Some(task.id.clone()),
                    day: show_day(&task.day),
                    duration: typed(task.minutes as u64),
                    label: task.label.clone(),
                    billing: task.billing,
                    counter: task.counter.clone(),
                    quote: task.quote.clone(),
                    item: task.item.clone(),
                });
            }
            Act::DeleteEntry(task) => {
                self.track.page.confirm = None;
                self.track_write(move |c| c.delete_task(&id, &task));
            }
            Act::EntryText(task, label) => {
                if let Some(x) = p.and_then(|p| p.tasks.iter().find(|x| x.id == task)) {
                    let input = TaskInput { day: x.day.clone(), label, minutes: x.minutes, billing: x.billing, counter: x.counter.clone(), quote: x.quote.clone(), item: x.item.clone() };
                    self.track_write(move |c| c.update_task(&id, &task, &input));
                }
            }
            Act::NewQuote => {
                self.track.page.form_error = None;
                self.track.page.quote = Some(QuoteForm { day: show_day(&track::today()), ..QuoteForm::default() });
            }
            Act::EditQuote(quote) => {
                if let Some(q) = p.and_then(|p| p.quotes.iter().find(|q| q.id == quote)) {
                    self.track.page.form_error = None;
                    self.track.page.quote = Some(QuoteForm {
                        editing: Some(q.id.clone()),
                        number: q.number.clone().unwrap_or_default(),
                        label: q.label.clone(),
                        hours: q.minutes.map(typed).unwrap_or_default(),
                        day: show_day(&q.day),
                        note: q.note.clone().unwrap_or_default(),
                    });
                }
            }
            Act::CloseQuote(quote, closed) => self.track_write(move |c| c.update_quote(&id, &quote, serde_json::json!({ "closed": closed }))),
            Act::DeleteQuote(quote) => self.track_write(move |c| c.delete_quote(&id, &quote)),
            Act::QuoteNote(quote, note) => self.track_write(move |c| c.update_quote(&id, &quote, serde_json::json!({ "note": note }))),
            Act::NewCounter(minutes) => {
                self.track.page.form_error = None;
                self.track.page.counter = Some(CounterForm { day: show_day(&track::today()), hours: minutes.map(typed).unwrap_or_default(), ..CounterForm::default() });
            }
            Act::EditCounter(counter) => {
                if let Some(c) = p.and_then(|p| p.counters.iter().find(|c| c.id == counter)) {
                    self.track.page.form_error = None;
                    self.track.page.counter = Some(CounterForm { editing: Some(c.id.clone()), label: c.label.clone(), hours: typed(c.minutes), day: show_day(&c.day) });
                }
            }
            Act::CloseCounter(counter, closed) => self.track_write(move |c| c.update_counter(&id, &counter, serde_json::json!({ "closed": closed }))),
            Act::DeleteCounter(counter) => self.track_write(move |c| c.delete_counter(&id, &counter)),
            Act::NewItem { quote, parent } => {
                self.track.page.form_error = None;
                self.track.page.item = Some(ItemForm { quote, parent, ..ItemForm::default() });
            }
            Act::EditItem(item) => {
                if let Some(i) = p.and_then(|p| p.items.iter().find(|i| i.id == item)) {
                    self.track.page.form_error = None;
                    self.track.page.item = Some(ItemForm {
                        editing: Some(i.id.clone()),
                        quote: i.quote.clone(),
                        parent: i.parent.clone(),
                        title: i.title.clone(),
                        body: i.body.clone().unwrap_or_default(),
                        hours: i.minutes.map(typed).unwrap_or_default(),
                        status: i.status,
                    });
                }
            }
            Act::SetStatus(item, status) => self.track_write(move |c| c.update_item(&id, &item, serde_json::json!({ "status": status.word() }))),
            Act::DeleteItem(item) => self.track_write(move |c| c.delete_item(&id, &item)),
            Act::ItemBody(item, body) => self.track_write(move |c| c.update_item(&id, &item, serde_json::json!({ "body": body }))),
            Act::Move(item, by) => {
                // Swapped with its neighbour: both get their place.
                let Some(p) = p else { return };
                let Some(i) = p.items.iter().find(|x| x.id == item) else { return };
                let siblings = p.children(i.quote.as_deref(), i.parent.as_deref());
                let Some(at) = siblings.iter().position(|s| s.id == item) else { return };
                let Some(other) = siblings.get((at as i64 + by) as usize).map(|o| o.id.clone()) else { return };
                let order: Vec<String> = siblings.iter().map(|s| s.id.clone()).collect();
                let mut order = order;
                order.swap(at, (at as i64 + by) as usize);
                let _ = other;
                self.track_write(move |c| {
                    let mut last = Err("nothing moved".to_owned());
                    for (k, id_k) in order.iter().enumerate() {
                        last = c.update_item(&id, id_k, serde_json::json!({ "position": k }));
                        last.as_ref().map_err(String::clone)?;
                    }
                    last
                });
            }
            Act::Share => {
                let (name, role) = std::mem::replace(&mut self.track.page.share, (String::new(), Role::Editor));
                self.track_write(move |c| c.share(&id, &name, role));
            }
            Act::SetRole(name, role) => self.track_write(move |c| c.share(&id, &name, role)),
            Act::Unshare(player) => {
                self.track.page.confirm = None;
                let Some(client) = self.track_client() else { return };
                let ctx = self.ctx.clone();
                self.track.spawn(&ctx, client, move |c| match c.unshare(&id, &player) {
                    Ok(Some(p)) => Done::Project { result: Box::new(Ok(p)), select: false },
                    Ok(None) => Done::Gone(Ok(id)),
                    Err(e) => Done::Gone(Err(e)),
                });
            }
            Act::CopyCsv => {
                if let Some(p) = p {
                    let csv = self.track_csv(p);
                    self.ctx.copy_text(csv);
                    self.error = Some(self.t().track_csv_copied.to_owned());
                }
            }
        }
    }

    /// The entries shown, as CSV for a spreadsheet (semicolons, hours with a comma).
    fn track_csv(&self, p: &Project) -> String {
        let t = self.t();
        let cell = |s: &str| format!("\"{}\"", s.replace('"', "\"\"").replace('\n', " "));
        let mut out = vec![t.track_csv_head.to_owned()];
        for x in self.track_filtered(p) {
            let hours = format!("{:.2}", x.minutes as f64 / 60.0).replace('.', ",");
            let item = x.item.as_ref().map(|i| p.item_path(i)).unwrap_or_default();
            out.push([show_day(&x.day), hours, cell(&self.track_charged(p, x)), cell(&item), cell(&x.label), cell(x.author.as_deref().unwrap_or(""))].join(";"));
        }
        out.join("\n")
    }

    /// The windows of the project's page: time spent, a quote, a counter, a planned task.
    pub(super) fn track_page_windows(&mut self, ctx: &egui::Context) {
        let Some(p) = self.config.settings.track_selected.clone().and_then(|id| self.track.projects.get(&id).cloned()) else {
            self.track.page.entry = None;
            self.track.page.quote = None;
            self.track.page.counter = None;
            self.track.page.item = None;
            return;
        };
        if let Some(demo) = self.track.page.demo.take() {
            match demo.as_str() {
                "entry" => self.track_entry_open(&p, None, p.items.first().map(|i| i.id.clone())),
                "settings" => self.track_edit_open(),
                "item" => self.track_page_act(Act::EditItem(p.items.first().map(|i| i.id.clone()).unwrap_or_default()), Some(&p)),
                _ => self.track_page_act(Act::EditQuote(p.quotes.first().map(|q| q.id.clone()).unwrap_or_default()), Some(&p)),
            }
        }
        let t = self.t();
        let theme = self.theme.clone();
        let frame = Frame::popup(&ctx.global_style()).inner_margin(24.0).fill(theme.chrome_bg).corner_radius(16.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let field = |ui: &mut Ui, label: &str| {
            ui.add_space(10.0);
            ui.label(egui::RichText::new(label).size(12.0).color(theme.text_muted));
            ui.add_space(2.0);
        };
        let footer = |ui: &mut Ui, error: &Option<String>, ok: &str, ready: bool| -> (bool, bool) {
            if let Some(e) = error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(e).size(12.0).color(RED));
            }
            ui.add_space(18.0);
            let mut out = (false, false);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                out.0 = ui.add_enabled(ready, primary(ok, &theme)).clicked();
                out.1 = ui.add(quiet(t.track_cancel)).clicked();
            });
            out
        };
        let id = p.id.clone();

        // Time spent.
        if let Some(f) = &mut self.track.page.entry {
            let mut send = None;
            let mut close = false;
            let error = self.track.page.form_error.clone();
            let modal = egui::Modal::new(Id::new("track-entry")).frame(frame).show(ctx, |ui| {
                ui.set_width(640.0);
                ui.label(egui::RichText::new(if f.editing.is_some() { t.track_edit_entry } else { t.track_log_time }).size(20.0).strong().color(theme.text));
                ui.label(egui::RichText::new(format!("{} · {}", p.client.as_deref().unwrap_or(t.track_no_client), p.name)).size(12.0).color(theme.text_muted));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        field(ui, t.track_day);
                        ui.horizontal(|ui| {
                            let day_field = ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(100.0).hint_text("18/11"));
                            day_picker(ui, &day_field, &mut f.day, t, &theme);
                            for (label, value) in [(t.track_today_word, track::today()), (t.track_yesterday, track::parse_day("hier").unwrap_or_default())] {
                                if ui.add(egui::Button::new(egui::RichText::new(label).size(11.5)).corner_radius(10.0).selected(track::parse_day(&f.day).as_deref() == Some(value.as_str()))).clicked() {
                                    f.day = show_day(&value);
                                }
                            }
                        });
                    });
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_duration);
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut f.duration).desired_width(70.0).hint_text("1h30"));
                            for m in [15u32, 30, 60, 120, 240, 420] {
                                if ui.add(egui::Button::new(egui::RichText::new(hm(m as i64)).size(11.5)).corner_radius(10.0).selected(track::parse_duration(&f.duration) == Some(m))).clicked() {
                                    f.duration = typed(m as u64);
                                }
                            }
                        });
                    });
                });
                field(ui, t.track_charge);
                ui.horizontal(|ui| {
                    for b in Billing::ALL {
                        let on = f.billing == b;
                        let text = egui::RichText::new(billing_name(t, b)).size(12.5).color(if on { theme.bg } else { theme.text });
                        let button = egui::Button::new(text).corner_radius(8.0).min_size(Vec2::new(110.0, 30.0));
                        if ui.add(if on { button.fill(billing_color(&theme, b)) } else { button }).clicked() {
                            f.billing = b;
                            if b != Billing::Quote {
                                f.item = f.item.take().filter(|i| p.items.iter().any(|x| &x.id == i && x.quote.is_none()));
                            }
                        }
                    }
                });
                ui.add_space(8.0);
                match f.billing {
                    Billing::Counter => {
                        let said = f.counter.as_ref().and_then(|c| p.counters.iter().find(|x| &x.id == c)).map_or(t.track_pick_counter.to_owned(), |c| format!("{}  ({})", c.label, t.track_left.replace("{h}", &hm(c.left()))));
                        egui::ComboBox::from_id_salt("entry-counter").selected_text(said).width(380.0).show_ui(ui, |ui| {
                            let had = f.counter.clone();
                            for c in p.counters.iter().filter(|c| c.closed.is_none() || had.as_ref() == Some(&c.id)) {
                                ui.selectable_value(&mut f.counter, Some(c.id.clone()), format!("{}  ({})", c.label, t.track_left.replace("{h}", &hm(c.left()))));
                            }
                            if p.counters.is_empty() {
                                ui.label(egui::RichText::new(t.track_no_counter).color(theme.text_muted));
                            }
                        });
                    }
                    Billing::Quote => {
                        let said = f.quote.as_ref().and_then(|q| p.quotes.iter().find(|x| &x.id == q)).map_or(t.track_pick_quote.to_owned(), Quote::title);
                        let before = f.quote.clone();
                        egui::ComboBox::from_id_salt("entry-quote").selected_text(said).width(380.0).show_ui(ui, |ui| {
                            let had = f.quote.clone();
                            for q in p.quotes.iter().filter(|q| q.closed.is_none() || had.as_ref() == Some(&q.id)) {
                                ui.selectable_value(&mut f.quote, Some(q.id.clone()), q.title());
                            }
                            if p.quotes.is_empty() {
                                ui.label(egui::RichText::new(t.track_no_quotes).color(theme.text_muted));
                            }
                        });
                        if f.quote != before {
                            f.item = None;
                        }
                    }
                    _ => {}
                }
                // The planned task it was spent on: of the quote picked, or the project's own.
                let quote = if f.billing == Billing::Quote { f.quote.clone() } else { None };
                let mut options: Vec<(String, String)> = Vec::new();
                fn walk(p: &Project, quote: Option<&str>, parent: Option<&str>, depth: usize, out: &mut Vec<(String, String)>) {
                    for i in p.children(quote, parent) {
                        out.push((i.id.clone(), format!("{}{}", "    ".repeat(depth), i.title)));
                        walk(p, quote, Some(&i.id), depth + 1, out);
                    }
                }
                walk(&p, quote.as_deref(), None, 0, &mut options);
                if !options.is_empty() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(t.track_on_item).size(12.0).color(theme.text_muted));
                        let said = f.item.as_ref().map_or(t.track_no_item.to_owned(), |i| p.item_path(i));
                        egui::ComboBox::from_id_salt("entry-item").selected_text(said).width(380.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut f.item, None, t.track_no_item);
                            for (iid, label) in &options {
                                ui.selectable_value(&mut f.item, Some(iid.clone()), label);
                            }
                        });
                    });
                }
                field(ui, t.track_description);
                md_editor(ui, Id::new(("entry-label", f.editing.clone())), &mut f.label, 640.0, 6, t.track_label_hint, &theme, t);
                let ready = track::parse_duration(&f.duration).is_some() && !f.label.trim().is_empty();
                let (ok, cancel) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_add_task }, ready);
                close = cancel;
                if ok {
                    send = Some(());
                }
            });
            if send.is_some() {
                let f = self.track.page.entry.as_ref().unwrap();
                let checked = (|| -> Result<(Option<String>, TaskInput), &'static str> {
                    let day = track::parse_day(&f.day).ok_or(t.track_bad_day)?;
                    let minutes = track::parse_duration(&f.duration).filter(|m| *m <= track::TASK_MAX).ok_or(t.track_bad_duration)?;
                    let input = TaskInput {
                        day,
                        label: f.label.trim().to_owned(),
                        minutes,
                        billing: f.billing,
                        counter: (f.billing == Billing::Counter).then(|| f.counter.clone()).flatten(),
                        quote: (f.billing == Billing::Quote).then(|| f.quote.clone()).flatten(),
                        item: f.item.clone(),
                    };
                    if (input.billing == Billing::Counter && input.counter.is_none()) || (input.billing == Billing::Quote && input.quote.is_none()) {
                        return Err(t.track_pick_charge);
                    }
                    Ok((f.editing.clone(), input))
                })();
                match checked {
                    Ok((editing, input)) => {
                        let id = id.clone();
                        self.track_write(move |c| match editing {
                            Some(task) => c.update_task(&id, &task, &input),
                            None => c.add_task(&id, &input),
                        });
                        self.track.page.entry = None;
                    }
                    Err(e) => self.track.page.form_error = Some(e.to_owned()),
                }
            } else if close || modal.should_close() {
                self.track.page.entry = None;
            }
        }

        // A quote.
        if let Some(f) = &mut self.track.page.quote {
            let (mut ok, mut close) = (false, false);
            let error = self.track.page.form_error.clone();
            let modal = egui::Modal::new(Id::new("track-quote")).frame(frame).show(ctx, |ui| {
                ui.set_width(640.0);
                ui.label(egui::RichText::new(if f.editing.is_some() { t.track_edit_quote } else { t.track_new_quote }).size(20.0).strong().color(theme.text));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        field(ui, t.track_quote_number);
                        ui.add(egui::TextEdit::singleline(&mut f.number).desired_width(120.0).hint_text("D-2026-12"));
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_quote_label);
                        ui.add(egui::TextEdit::singleline(&mut f.label).desired_width(300.0));
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_day);
                        ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(100.0));
                    });
                });
                field(ui, t.track_planned);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.hours).desired_width(80.0).hint_text("20"));
                    ui.label(egui::RichText::new(t.track_planned_hint).size(11.5).color(theme.text_muted));
                });
                field(ui, t.track_description);
                md_editor(ui, Id::new(("quote-note-edit", f.editing.clone())), &mut f.note, 640.0, 8, t.track_quote_note_hint, &theme, t);
                let ready = !f.label.trim().is_empty() && (f.hours.trim().is_empty() || track::parse_duration(&f.hours).is_some());
                (ok, close) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_create }, ready);
            });
            if ok {
                let f = self.track.page.quote.as_ref().unwrap();
                match track::parse_day(&f.day) {
                    Some(day) => {
                        let (editing, number, label, minutes, note) = (f.editing.clone(), Some(f.number.trim().to_owned()).filter(|n| !n.is_empty()), f.label.trim().to_owned(), track::parse_duration(&f.hours).map(u64::from), f.note.trim().to_owned());
                        let id = id.clone();
                        self.track_write(move |c| match editing {
                            Some(q) => c.update_quote(&id, &q, serde_json::json!({ "number": number, "label": label, "minutes": minutes, "day": day, "note": note })),
                            None => c.add_quote_full(&id, number.as_deref(), &label, minutes, &day, Some(note.as_str()).filter(|n| !n.is_empty())),
                        });
                        self.track.page.quote = None;
                    }
                    None => self.track.page.form_error = Some(t.track_bad_day.to_owned()),
                }
            } else if close || modal.should_close() {
                self.track.page.quote = None;
            }
        }

        // A counter.
        if let Some(f) = &mut self.track.page.counter {
            let (mut ok, mut close) = (false, false);
            let error = self.track.page.form_error.clone();
            let modal = egui::Modal::new(Id::new("track-counter")).frame(frame).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(egui::RichText::new(if f.editing.is_some() { t.track_edit_counter } else { t.track_new_counter }).size(20.0).strong().color(theme.text));
                ui.label(egui::RichText::new(t.track_counters_intro).size(12.0).color(theme.text_muted));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        field(ui, t.track_hours);
                        ui.add(egui::TextEdit::singleline(&mut f.hours).desired_width(90.0).hint_text("10"));
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_since);
                        ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(110.0));
                    });
                });
                field(ui, t.track_counter_label);
                ui.add(egui::TextEdit::singleline(&mut f.label).desired_width(f32::INFINITY).hint_text(t.track_counter_hint));
                let ready = track::parse_duration(&f.hours).is_some();
                (ok, close) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_create }, ready);
            });
            if ok {
                let f = self.track.page.counter.as_ref().unwrap();
                match (track::parse_day(&f.day), track::parse_duration(&f.hours)) {
                    (Some(day), Some(minutes)) => {
                        let label = if f.label.trim().is_empty() { t.track_counter_default.replace("{h}", &hm(minutes as i64)).replace("{d}", &show_day(&day)) } else { f.label.trim().to_owned() };
                        let (editing, id) = (f.editing.clone(), id.clone());
                        self.track_write(move |c| match editing {
                            Some(k) => c.update_counter(&id, &k, serde_json::json!({ "label": label, "minutes": minutes, "day": day })),
                            None => c.add_counter(&id, &label, minutes as u64, &day),
                        });
                        self.track.page.counter = None;
                    }
                    _ => self.track.page.form_error = Some(t.track_bad_day.to_owned()),
                }
            } else if close || modal.should_close() {
                self.track.page.counter = None;
            }
        }

        // A planned task.
        if let Some(f) = &mut self.track.page.item {
            let (mut ok, mut close) = (false, false);
            let error = self.track.page.form_error.clone();
            let modal = egui::Modal::new(Id::new("track-item")).frame(frame).show(ctx, |ui| {
                ui.set_width(640.0);
                let title = match (&f.editing, &f.parent) {
                    (Some(_), _) => t.track_edit_item,
                    (None, Some(_)) => t.track_new_subtask,
                    (None, None) => t.track_new_item,
                };
                ui.label(egui::RichText::new(title).size(20.0).strong().color(theme.text));
                let mut place = f.quote.as_ref().and_then(|q| p.quotes.iter().find(|x| &x.id == q)).map_or(t.track_own_items.to_owned(), |q| format!("{} {}", t.track_bill_quote, q.title()));
                if let Some(parent) = &f.parent {
                    place = format!("{place}  ›  {}", p.item_path(parent));
                }
                ui.label(egui::RichText::new(place).size(12.0).color(theme.text_muted));
                field(ui, t.track_item_title);
                ui.add(egui::TextEdit::singleline(&mut f.title).desired_width(f32::INFINITY).hint_text(t.track_item_title_hint));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        field(ui, t.track_estimate);
                        ui.add(egui::TextEdit::singleline(&mut f.hours).desired_width(90.0).hint_text("4h"));
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_status);
                        ui.horizontal(|ui| {
                            for (s, label, color) in [(Status::Todo, t.track_status_todo, theme.text_muted), (Status::Doing, t.track_status_doing, ORANGE), (Status::Done, t.track_status_done, GREEN)] {
                                let on = f.status == s;
                                let b = egui::Button::new(egui::RichText::new(label).size(12.0).color(if on { theme.bg } else { theme.text })).corner_radius(8.0).min_size(Vec2::new(84.0, 28.0));
                                if ui.add(if on { b.fill(color) } else { b }).clicked() {
                                    f.status = s;
                                }
                            }
                        });
                    });
                });
                field(ui, t.track_description);
                md_editor(ui, Id::new(("item-body-edit", f.editing.clone())), &mut f.body, 640.0, 6, t.track_item_body_hint, &theme, t);
                let ready = !f.title.trim().is_empty() && (f.hours.trim().is_empty() || track::parse_duration(&f.hours).is_some());
                (ok, close) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_create }, ready);
            });
            if ok {
                let f = self.track.page.item.take().unwrap();
                let input = ItemInput { title: f.title.trim().to_owned(), body: Some(f.body.trim().to_owned()).filter(|b| !b.is_empty()), minutes: track::parse_duration(&f.hours).map(u64::from), status: f.status };
                let id = id.clone();
                // A sub-task's quote unfolded, to see it.
                if let Some(q) = f.quote.as_ref().filter(|q| p.quotes.iter().any(|x| &x.id == *q && x.closed.is_some())) {
                    self.track.page.open_quotes.insert(q.clone());
                }
                self.track_write(move |c| match f.editing {
                    Some(i) => c.update_item(&id, &i, serde_json::json!({ "title": input.title, "body": input.body, "minutes": input.minutes, "status": input.status.word() })),
                    None => c.add_item(&id, f.quote.as_deref(), f.parent.as_deref(), &input),
                });
            } else if close || modal.should_close() {
                self.track.page.item = None;
            }
        }
    }
}

/// A gauge's color: red once used up, orange near the end, the accent before.
fn left_color_bar(theme: &Theme, left: i64, of: u64) -> Color32 {
    if left <= 0 {
        RED
    } else if (left as f64) < of as f64 * 0.15 {
        ORANGE
    } else {
        theme.accent
    }
}

/// A day said whole: "mardi 18 novembre 2026".
fn long_day(day: &str, t: &Strings) -> String {
    use chrono::Datelike as _;
    let Ok(d) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") else { return day.to_owned() };
    let days: Vec<&str> = t.track_weekdays.split('|').collect();
    let months: Vec<&str> = t.track_months.split('|').collect();
    let weekday = days.get(d.weekday().num_days_from_monday() as usize).copied().unwrap_or("");
    let month = months.get(d.month0() as usize).copied().unwrap_or("");
    let today = chrono::Local::now().date_naive();
    let prefix = if d == today {
        format!("{} · ", t.track_today_word)
    } else if d == today - chrono::Duration::days(1) {
        format!("{} · ", t.track_yesterday)
    } else {
        String::new()
    };
    format!("{prefix}{weekday} {} {month} {}", d.day(), d.year())
}

/// A small calendar under a day's field, opened by clicking it (like a browser's date picker): the
/// month shown starts at the typed day, ‹ › change it, a click on a day writes it and closes.
fn day_picker(ui: &mut Ui, field: &egui::Response, day: &mut String, t: &Strings, theme: &Theme) {
    use chrono::{Datelike as _, NaiveDate};
    let today = chrono::Local::now().date_naive();
    let picked = track::parse_day(day).and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    let month_id = field.id.with("calendar-month");
    if field.clicked() {
        let at = picked.unwrap_or(today);
        ui.data_mut(|d| d.insert_temp(month_id, at.with_day(1).unwrap_or(at)));
    }
    let open = field.clicked().then_some(egui::SetOpenCommand::Bool(true));
    egui::Popup::from_response(field).open_memory(open).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let mut first: NaiveDate = ui.data(|d| d.get_temp(month_id)).unwrap_or_else(|| today.with_day(1).unwrap_or(today));
        let months: Vec<&str> = t.track_months.split('|').collect();
        let cell = Vec2::new(30.0, 26.0);
        ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new("‹").frame(false).min_size(cell)).clicked() {
                first = first.checked_sub_months(chrono::Months::new(1)).unwrap_or(first);
            }
            let name = months.get(first.month0() as usize).copied().unwrap_or("");
            let mut chars = name.chars();
            let name: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
            let title = egui::RichText::new(format!("{name} {}", first.year())).size(13.0).strong().color(theme.text);
            ui.add_sized(Vec2::new(cell.x * 5.0 + 8.0, cell.y), egui::Label::new(title));
            if ui.add(egui::Button::new("›").frame(false).min_size(cell)).clicked() {
                first = first.checked_add_months(chrono::Months::new(1)).unwrap_or(first);
            }
        });
        ui.horizontal(|ui| {
            for w in t.track_weekdays.split('|') {
                let letter: String = w.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                ui.add_sized(cell, egui::Label::new(egui::RichText::new(letter).size(11.0).color(theme.text_muted)));
            }
        });
        let skip = first.weekday().num_days_from_monday() as usize;
        let length = first.checked_add_months(chrono::Months::new(1)).map_or(31, |next| (next - first).num_days() as usize);
        let mut chosen = None;
        for week in 0..(skip + length).div_ceil(7) {
            ui.horizontal(|ui| {
                for col in 0..7 {
                    let n = week * 7 + col;
                    if n < skip || n >= skip + length {
                        ui.allocate_space(cell);
                        continue;
                    }
                    let date = first + chrono::Duration::days((n - skip) as i64);
                    let on = picked == Some(date);
                    let text = egui::RichText::new(date.day().to_string()).size(12.0).color(if on { theme.bg } else if col >= 5 { theme.text_muted } else { theme.text });
                    let mut b = egui::Button::new(text).min_size(cell).corner_radius(6.0);
                    b = if on { b.fill(theme.accent) } else { b.frame(false) };
                    if date == today && !on {
                        b = b.frame(true).fill(egui::Color32::TRANSPARENT).stroke(Stroke::new(1.0, theme.accent));
                    }
                    if ui.add(b).clicked() {
                        chosen = Some(date);
                    }
                }
            });
        }
        ui.data_mut(|d| d.insert_temp(month_id, first));
        if let Some(date) = chosen {
            *day = show_day(&date.format("%Y-%m-%d").to_string());
            ui.close();
        }
    });
}
