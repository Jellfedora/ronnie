//! The tracking's pages (a sub-module of `track`): every project as cards, or one project with its
//! tabs — overview, time spent (by day, filtered, copied as CSV), quotes with their planned tasks and
//! sub-tasks, hour counters, team, Claude — and the windows to write in (time spent, a quote, a counter,
//! a planned task), their descriptions in Markdown with a toolbar.

use std::collections::HashSet;

use super::super::mdedit::{first_line, md_editor, md_view};
use super::*;
use crate::track::{Item, ItemInput, Quote, Status, Task, TrackFile};
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
    /// Files sent while it is being made (joined to the project until then, to it once made).
    pending: Vec<String>,
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
    /// Files sent while it is being made (joined to the project until then, to it once made).
    pending: Vec<String>,
    number: String,
    label: String,
    hours: String,
    day: String,
    note: String,
}

#[derive(Default)]
pub(super) struct CounterForm {
    editing: Option<String>,
    /// Files sent while it is being made (joined to the project until then, to it once made).
    pending: Vec<String>,
    label: String,
    hours: String,
    day: String,
}

#[derive(Default)]
pub(super) struct ItemForm {
    editing: Option<String>,
    /// Files sent while it is being made (joined to the project until then, to it once made).
    pending: Vec<String>,
    quote: Option<String>,
    parent: Option<String>,
    title: String,
    body: String,
    hours: String,
    status: Status,
    /// Its dates as typed (dd/mm), who asked, how far (%), a comment.
    requested: String,
    due: String,
    finished: String,
    live: String,
    requester: String,
    progress: String,
    comment: String,
    env: Option<String>,
}

impl ItemForm {
    /// The window for `i`, changed.
    fn of(i: &Item) -> ItemForm {
        let day = |d: &Option<String>| d.as_deref().map(show_day).unwrap_or_default();
        ItemForm {
            editing: Some(i.id.clone()),
            quote: i.quote.clone(),
            parent: i.parent.clone(),
            title: i.title.clone(),
            body: i.body.clone().unwrap_or_default(),
            hours: i.minutes.map(typed).unwrap_or_default(),
            status: i.status,
            pending: Vec::new(),
            requested: day(&i.requested),
            due: day(&i.due),
            finished: day(&i.finished),
            live: day(&i.live),
            requester: i.requester.clone().unwrap_or_default(),
            progress: i.progress.map(|p| p.to_string()).unwrap_or_default(),
            comment: i.comment.clone().unwrap_or_default(),
            env: i.env.clone(),
        }
    }
}

/// Where a task is: its name and its color (none: nothing said).
pub(super) fn env_name(env: &str) -> &'static str {
    match env {
        "local" => "Local",
        "dev" => "Dev",
        "test" => "Test",
        "prod" => "Prod",
        _ => "?",
    }
}

pub(super) fn env_color(env: &str, theme: &Theme) -> Color32 {
    match env {
        "local" => theme.text_muted,
        "dev" => Color32::from_rgb(0x3d, 0x8b, 0xfd),
        "test" => ORANGE,
        "prod" => GREEN,
        _ => theme.text_muted,
    }
}

/// A planned task's state: its color and its name.
pub(super) fn status_color(status: Status, theme: &Theme) -> Color32 {
    match status {
        Status::Todo => theme.text_muted,
        Status::Waiting => ORANGE,
        Status::Doing => Color32::from_rgb(0x3d, 0x8b, 0xfd),
        Status::Done => GREEN,
        Status::Live => Color32::from_rgb(0x8b, 0x5c, 0xf6),
        Status::Cancelled => Color32::from_rgb(0x88, 0x88, 0x88),
    }
}

pub(super) fn status_name(status: Status, t: &Strings) -> &'static str {
    match status {
        Status::Todo => t.track_status_todo,
        Status::Waiting => t.track_status_waiting,
        Status::Doing => t.track_status_doing,
        Status::Done => t.track_status_done,
        Status::Live => t.track_status_live,
        Status::Cancelled => t.track_status_cancelled,
    }
}

/// A state as a colored pill (filled once started), its name in it.
pub(super) fn status_pill(ui: &mut Ui, status: Status, t: &Strings, theme: &Theme) -> egui::Response {
    let color = status_color(status, theme);
    let filled = status != Status::Todo;
    let text = egui::RichText::new(status_name(status, t)).size(11.5).color(if filled { Color32::WHITE } else { theme.text });
    let b = egui::Button::new(text).corner_radius(6.0).min_size(Vec2::new(86.0, 22.0)).stroke(Stroke::new(1.0, color));
    ui.add(if filled { b.fill(color) } else { b.fill(Color32::TRANSPARENT) }).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The tasks' table: shown instead of the projects' cards, its filters.
#[derive(Default)]
pub(super) struct TasksState {
    pub(super) shown: bool,
    client: Option<String>,
    project: Option<String>,
    status: Option<Status>,
    /// The ones done with (finished, in production, cancelled) instead of those going on.
    archives: bool,
    find: String,
    /// The projects folded in the table, their tasks hidden (a click on their row).
    folded: std::collections::HashSet<String>,
    /// The column filters (the table's heads): who asked, who worked on it, where it is ("" none); empty, all.
    cols: [BTreeSet<String>; 3],
}

/// What a click in the tasks' table asks: a task opened, its state changed, one added to a project.
enum TaskAct {
    Open(String, String),
    Status(String, String, Status),
    New(String),
    Fold(String),
    /// A value of a column's filter ticked or unticked (column 0 who asked, 1 who worked, 2 where); none, the
    /// filter cleared.
    Filter(usize, Option<String>),
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
    ExportCsv,
    /// A file of something (`project`, `task:<id>`...) picked, opened or taken off.
    Files(String, FileAct),
}

/// What the files of a window or a card ask.
pub(super) enum FileAct {
    Pick,
    Open(TrackFile),
    Delete(String),
}

/// The files joined to something: each a chip (its name and size; a click opens it, ✕ takes it off for
/// its author or a manager), then « Joindre un fichier… » when one may add.
fn files_ui(ui: &mut Ui, files: &[&TrackFile], manages: bool, add: bool, theme: &Theme, t: &Strings, out: &mut Vec<FileAct>) {
    if files.is_empty() && !add {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for f in files {
            let text = egui::RichText::new(format!("📎 {}  ·  {}", f.name, track::size_text(f.size))).size(12.0).color(theme.text);
            if ui.add(egui::Button::new(text).corner_radius(10.0)).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.track_file_open).clicked() {
                out.push(FileAct::Open((*f).clone()));
            }
            if (manages || f.mine) && ui.add(egui::Button::new(egui::RichText::new("✕").size(11.0).color(theme.text_muted)).frame(false)).on_hover_text(t.track_file_delete).clicked() {
                out.push(FileAct::Delete(f.id.clone()));
            }
            ui.add_space(6.0);
        }
        if add && ui.add(egui::Button::new(egui::RichText::new(t.track_attach).size(12.0).color(theme.accent)).corner_radius(10.0)).clicked() {
            out.push(FileAct::Pick);
        }
    });
}

/// The files joined to `on` (`project`, `task:<id>`...), but the images shown in a description.
fn files_of<'a>(p: &'a Project, on: &str) -> Vec<&'a TrackFile> {
    let shown: HashSet<&str> = std::iter::once(p.note.as_deref().unwrap_or(""))
        .chain(p.tasks.iter().map(|x| x.label.as_str()))
        .chain(p.quotes.iter().map(|x| x.note.as_deref().unwrap_or("")))
        .chain(p.items.iter().map(|x| x.body.as_deref().unwrap_or("")))
        .flat_map(super::super::notes::image_sources)
        .filter_map(|src| src.strip_prefix(track::FILES_PREFIX))
        .collect();
    p.files.iter().filter(|f| f.on == on && !shown.contains(f.id.as_str())).collect()
}

/// A card's files and its « Joindre » (when `add`): their clicks as acts.
fn files_row(ui: &mut Ui, p: &Project, on: &str, add: bool, theme: &Theme, t: &Strings, acts: &mut Vec<Act>) {
    let files = files_of(p, on);
    if files.is_empty() && !add {
        return;
    }
    let mut out = Vec::new();
    ui.add_space(6.0);
    files_ui(ui, &files, p.role.manages(), add, theme, t, &mut out);
    acts.extend(out.into_iter().map(|a| Act::Files(on.to_owned(), a)));
}

/// After making something: the files sent while it was written (joined to the project meanwhile)
/// joined to it — its id the one of `ids` not in `before`, `kind` its sort (`task`, `quote`...).
fn adopt(c: &track::Client, id: &str, made: Project, ids: impl Fn(&Project) -> Vec<String>, before: &[String], kind: &str, pending: &[String]) -> Result<Project, String> {
    if pending.is_empty() {
        return Ok(made);
    }
    let Some(new) = ids(&made).into_iter().find(|i| !before.contains(i)) else { return Ok(made) };
    for f in pending {
        c.move_file(id, f, &format!("{kind}:{new}"))?;
    }
    c.project(id)
}

/// A card: the chrome's color, a thin edge, round corners.
fn card<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(egui::Margin::same(16)).show(ui, |ui| add(ui)).inner
}

/// A small rounded label.
fn chip(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    Frame::NONE.fill(color.gamma_multiply(0.16)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(color)).truncate())).response
}

/// A table's cells: their padding, a row's height, the heading's.
const CELL_PAD: f32 = 10.0;
const CELL_H: f32 = 38.0;
const HEAD_H: f32 = 34.0;

/// A cell `w` wide, at least `h` high, padded; `layout` within (one line centered, numbers on the right, a
/// text from the top).
fn tcell(ui: &mut Ui, w: f32, h: f32, layout: egui::Layout, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(Vec2::new(w, h), egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.set_width(w);
        ui.set_min_height(h);
        ui.add_space(CELL_PAD);
        ui.allocate_ui_with_layout(Vec2::new(w - 2.0 * CELL_PAD, h), layout, |ui| {
            ui.set_width(w - 2.0 * CELL_PAD);
            ui.set_min_height(h);
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
            add(ui);
        });
    });
}

/// One line, centered in the row's first height, from the left (or the right).
fn line_at(right: bool) -> egui::Layout {
    if right { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) }
}

/// A small tag, its own height whatever the row's.
fn tag(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), color);
    let (r, _) = ui.allocate_exact_size(galley.size() + Vec2::new(16.0, 6.0), Sense::hover());
    ui.painter().rect_filled(r, 9.0, color.gamma_multiply(0.18));
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, color);
}

/// The columns' widths: `fixed`, with the one at `flex` taking what is left of `room` (`min` at least).
fn widths(room: f32, mut fixed: Vec<f32>, flex: usize, min: f32) -> Vec<f32> {
    let others: f32 = fixed.iter().enumerate().filter(|(k, _)| *k != flex).map(|(_, w)| w).sum();
    fixed[flex] = (room - others - 2.0).max(min);
    fixed
}

/// A table drawn as it goes: a frame, a heading row, a line under each row, lines between the columns
/// of the rows that have them (not across a band, a project's or a day's).
struct Table {
    top: egui::Pos2,
    widths: Vec<f32>,
    spans: Vec<(f32, f32)>,
    bottom: f32,
}

impl Table {
    fn new(ui: &Ui, widths: Vec<f32>) -> Self {
        let top = ui.cursor().min;
        Table { top, widths, spans: Vec::new(), bottom: top.y }
    }

    fn width(&self) -> f32 {
        self.widths.iter().sum()
    }

    /// The heading row: `heads` (right aligned where `right`).
    fn head(&mut self, ui: &mut Ui, theme: &Theme, heads: &[&str], right: &[bool]) {
        let bg = ui.painter().add(egui::Shape::Noop);
        let w = self.widths.clone();
        let r = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for (k, h) in heads.iter().enumerate() {
                    tcell(ui, w[k], HEAD_H, line_at(right.get(k).copied().unwrap_or(false)), |ui| {
                        ui.label(egui::RichText::new(*h).size(13.0).strong().color(theme.text_muted));
                    });
                }
            })
            .response
            .rect;
        let r = Rect::from_min_size(r.min, Vec2::new(self.width(), r.height()));
        ui.painter().set(bg, egui::Shape::rect_filled(r, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, theme.tab_hover.gamma_multiply(0.55)));
        ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0, theme.tab_hover));
        self.spans.push((r.top(), r.bottom()));
        self.bottom = r.bottom();
    }

    /// A row, its cells added by `add`; lit under the pointer. `fill` tints it (a band), `lined` draws
    /// the columns' lines across it. Its rectangle, whole width.
    fn row(&mut self, ui: &mut Ui, theme: &Theme, lined: bool, fill: Option<Color32>, add: impl FnOnce(&mut Ui)) -> Rect {
        let bg = ui.painter().add(egui::Shape::Noop);
        let r = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                add(ui);
            })
            .response
            .rect;
        let r = Rect::from_min_size(r.min, Vec2::new(self.width(), r.height()));
        if let Some(c) = fill {
            ui.painter().set(bg, egui::Shape::rect_filled(r, 0.0, c));
        } else if lined && ui.rect_contains_pointer(r) {
            ui.painter().set(bg, egui::Shape::rect_filled(r, 0.0, theme.tab_hover.gamma_multiply(0.35)));
        }
        if lined {
            self.spans.push((r.top(), r.bottom()));
        }
        ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
        self.bottom = r.bottom();
        r
    }

    /// The lines between the columns, the frame.
    fn finish(self, ui: &Ui, theme: &Theme) {
        let thin = Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7));
        let mut x = self.top.x;
        for w in &self.widths[..self.widths.len().saturating_sub(1)] {
            x += w;
            for (a, b) in &self.spans {
                ui.painter().vline(x, *a..=*b, thin);
            }
        }
        let frame = Rect::from_min_max(self.top, egui::pos2(self.top.x + self.width(), self.bottom));
        ui.painter().rect_stroke(frame, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
    }
}

/// The height of the time spent's fields and of the segmented controls beside them, to line them up.
const FIELD_HEIGHT: f32 = 28.0;

/// A filled button, the main action.
fn primary(text: &str, theme: &Theme) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))
}

/// A quiet button.
fn quiet(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).size(12.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))
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

/// The day of a moment (Unix milliseconds), short: « 19/11 ».
fn changed_day(ms: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local.timestamp_millis_opt(ms).single().map_or_else(String::new, |d| d.format("%d/%m").to_string())
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
        ui.horizontal(|ui| {
            // The projects as cards, or every task as one table.
            let chosen = ui.push_id("track-view", |ui| super::segmented::segmented(ui, &theme, 28.0, &[t.track_view_projects, t.track_view_tasks], Some(usize::from(self.track.tasks.shown)))).inner;
            if let Some(i) = chosen {
                self.track.tasks.shown = i == 1;
            }
            if !self.track.tasks.shown {
                ui.add_space(10.0);
                ui.add(egui::Label::new(egui::RichText::new(t.track_intro).size(12.5).color(theme.text_muted.gamma_multiply(0.85))).truncate());
            }
        });
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
        if self.track.tasks.shown {
            self.track_tasks(ui);
            for act in acts {
                self.track_page_act(act, None);
            }
            return;
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
            // The cards in a grid filling the width (sorted by client, then name), the client on each.
            let width = ui.available_width();
            let cols = ((width / 340.0).floor() as usize).clamp(1, 4);
            let gap = 14.0;
            let w = (width - gap * (cols - 1) as f32) / cols as f32;
            let today = track::today();
            let month = format!("{}01", today.get(..8).unwrap_or(""));
            for row in shown.chunks(cols) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for s in row {
                        let p = self.track.projects.get(&s.id).cloned();
                        let resp = ui.allocate_ui_with_layout(Vec2::new(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            let color = color_of(s.color.as_deref()).unwrap_or(theme.accent);
                            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(egui::Margin { left: 18, right: 16, top: 14, bottom: 14 }).show(ui, |ui| {
                                ui.set_width(w - 34.0);
                                ui.set_min_height(196.0);
                                // Its client, its name, its people.
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(s.client.as_deref().unwrap_or(t.track_no_client).to_uppercase()).size(11.0).strong().color(theme.text_muted));
                                    if s.archived.is_some() {
                                        chip(ui, t.track_archived, theme.text_muted);
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(p) = &p {
                                            for m in p.members.iter().take(5) {
                                                initial(ui, &m.name, &theme);
                                            }
                                        }
                                    });
                                });
                                ui.horizontal(|ui| {
                                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                    ui.painter().circle_filled(dot.center(), 5.0, color);
                                    ui.add(egui::Label::new(egui::RichText::new(&s.name).size(16.0).strong().color(theme.text)).truncate());
                                });
                                ui.add_space(10.0);
                                // Its time: this month, all along.
                                let month_spent: u64 = p.as_ref().map_or(0, |p| p.tasks.iter().filter(|x| x.day.as_str() >= month.as_str()).map(|x| u64::from(x.minutes)).sum());
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(hm(month_spent as i64)).size(18.0).color(theme.text));
                                    ui.label(egui::RichText::new(t.track_month_word).size(11.5).color(theme.text_muted));
                                    ui.add_space(10.0);
                                    ui.label(egui::RichText::new(hm(s.spent as i64)).size(13.0).color(theme.text_muted));
                                    ui.label(egui::RichText::new(t.track_in_all).size(11.5).color(theme.text_muted));
                                });
                                // Its counter going on.
                                match p.as_ref().and_then(Project::current_counter) {
                                    Some(c) => {
                                        ui.add_space(4.0);
                                        let fraction = if c.minutes > 0 { c.used as f32 / c.minutes as f32 } else { 1.0 };
                                        gauge(ui, w - 34.0, fraction, left_color_bar(&theme, c.left(), c.minutes), &theme);
                                        ui.label(egui::RichText::new(format!("{} · {}", c.label, t.track_counter_short.replace("{left}", &hm(c.left())).replace("{of}", &hm(c.minutes as i64)))).size(11.5).color(left_color(&theme, c.left(), c.minutes)));
                                    }
                                    None => {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(t.track_no_counter).size(11.5).color(theme.text_muted.gamma_multiply(0.7)));
                                    }
                                }
                                ui.add_space(8.0);
                                // Its tasks: going on, late, waiting; the next one due.
                                if let Some(p) = &p {
                                    let open: Vec<&Item> = p.items.iter().filter(|i| !i.status.closed()).collect();
                                    let going = open.iter().filter(|i| i.status == Status::Doing).count();
                                    let waiting = open.iter().filter(|i| i.status == Status::Waiting).count();
                                    let todo = open.iter().filter(|i| i.status == Status::Todo).count();
                                    let late = open.iter().filter(|i| i.late(&today)).count();
                                    ui.horizontal_wrapped(|ui| {
                                        if open.is_empty() {
                                            ui.label(egui::RichText::new(t.track_no_open_task).size(11.5).color(theme.text_muted));
                                        }
                                        for (n, label, c) in [(going, t.track_status_doing, status_color(Status::Doing, &theme)), (todo, t.track_status_todo, theme.text_muted), (waiting, t.track_status_waiting, status_color(Status::Waiting, &theme)), (late, t.track_late, RED)] {
                                            if n > 0 {
                                                chip(ui, &format!("{n} {}", label.to_lowercase()), c);
                                            }
                                        }
                                    });
                                    if let Some(next) = open.iter().filter(|i| i.due.is_some()).min_by_key(|i| i.due.clone()) {
                                        ui.add_space(4.0);
                                        let late = next.late(&today);
                                        ui.add(egui::Label::new(egui::RichText::new(format!("{} {} · {}", t.track_next_due, show_day(next.due.as_deref().unwrap_or("")), next.title)).size(11.5).color(if late { RED } else { theme.text_muted })).truncate());
                                    }
                                }
                                // When it last changed.
                                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                                    ui.label(egui::RichText::new(format!("{} · {}", role_name(t, s.role), t.track_changed_on.replace("{d}", &changed_day(s.updated)))).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                                });
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
        });
        for act in acts {
            self.track_page_act(act, None);
        }
    }

    /// Every task of every project as one table, by client then project: its dates, its time, how far,
    /// its state (changed with a click), who asked; filtered by client, project, state, words.
    fn track_tasks(&mut self, ui: &mut Ui) {
        let t = self.t();
        let theme = self.theme.clone();
        let today = track::today();
        let mut projects: Vec<Project> = self.track.projects.values().filter(|p| self.track.show_archived || p.archived.is_none()).cloned().collect();
        projects.sort_by_key(|p| (p.client.clone().unwrap_or_default().to_lowercase(), p.name.to_lowercase()));
        let clients: BTreeSet<String> = projects.iter().filter_map(|p| p.client.clone()).collect();
        let mut out = Vec::new();

        // The filters on the left, the search and « + Nouvelle tâche » on the right: one line, every control
        // as high as the others, centered.
        const ROW_H: f32 = 28.0;
        let f = &mut self.track.tasks;
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), ROW_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().interact_size.y = ROW_H;
            ui.spacing_mut().button_padding.y = 5.0;
            egui::ComboBox::from_id_salt("tasks-client").selected_text(f.client.as_deref().unwrap_or(t.track_all_clients)).show_ui(ui, |ui| {
                if ui.selectable_label(f.client.is_none(), t.track_all_clients).clicked() {
                    (f.client, f.project) = (None, None);
                }
                for c in &clients {
                    if ui.selectable_label(f.client.as_deref() == Some(c.as_str()), c).clicked() {
                        (f.client, f.project) = (Some(c.clone()), None);
                    }
                }
            });
            let in_client: Vec<&Project> = projects.iter().filter(|p| f.client.is_none() || p.client == f.client).collect();
            let chosen = f.project.as_ref().and_then(|id| in_client.iter().find(|p| &p.id == id)).map_or(t.track_all_projects.to_owned(), |p| p.name.clone());
            egui::ComboBox::from_id_salt("tasks-project").selected_text(chosen).show_ui(ui, |ui| {
                if ui.selectable_label(f.project.is_none(), t.track_all_projects).clicked() {
                    f.project = None;
                }
                for p in &in_client {
                    if ui.selectable_label(f.project.as_deref() == Some(p.id.as_str()), &p.name).clicked() {
                        f.project = Some(p.id.clone());
                    }
                }
            });
            if let Some(i) = ui.push_id("tasks-archives", |ui| super::segmented::segmented(ui, &theme, ROW_H, &[t.track_going_on, t.track_archives], Some(usize::from(f.archives)))).inner {
                (f.archives, f.status) = (i == 1, None);
            }
            egui::ComboBox::from_id_salt("tasks-status").selected_text(f.status.map_or(t.track_all_statuses, |s| status_name(s, t))).show_ui(ui, |ui| {
                if ui.selectable_label(f.status.is_none(), t.track_all_statuses).clicked() {
                    f.status = None;
                }
                for st in Status::ALL.into_iter().filter(|s| s.closed() == f.archives) {
                    if ui.selectable_label(f.status == Some(st), egui::RichText::new(status_name(st, t)).color(status_color(st, &theme))).clicked() {
                        f.status = Some(st);
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let writable: Vec<&Project> = projects.iter().filter(|p| p.role.manages()).collect();
                if !writable.is_empty() {
                    ui.menu_button(egui::RichText::new(format!("+  {}", t.track_new_item)).size(13.0).color(theme.accent), |ui| {
                        ui.label(egui::RichText::new(t.track_new_task_in).size(11.5).color(theme.text_muted));
                        for p in writable {
                            let name = p.client.as_ref().map_or(p.name.clone(), |c| format!("{c} · {}", p.name));
                            if ui.button(name).clicked() {
                                out.push(TaskAct::New(p.id.clone()));
                                ui.close();
                            }
                        }
                    });
                }
                // The search, just above the table, on the right.
                ui.add(egui::TextEdit::singleline(&mut f.find).hint_text(t.track_find_task).desired_width(220.0).min_size(Vec2::new(0.0, ROW_H)).vertical_align(egui::Align::Center));
            });
        });
        ui.add_space(12.0);

        // The rows: each project's tasks in their order (sub-tasks under theirs), those kept by the filters.
        let f = &self.track.tasks;
        let words = f.find.trim().to_lowercase();
        let keep = |p: &Project, i: &Item| {
            i.status.closed() == f.archives
                && f.status.is_none_or(|s| s == i.status)
                && f.client.as_ref().is_none_or(|c| p.client.as_ref() == Some(c))
                && f.project.as_ref().is_none_or(|id| &p.id == id)
                && (words.is_empty() || [Some(&i.title), i.requester.as_ref(), i.comment.as_ref(), Some(&p.name)].into_iter().flatten().any(|s| s.to_lowercase().contains(&words)))
        };
        // What a task shows in each filtered column ("" none).
        let values = |p: &Project, i: &Item| -> [Vec<String>; 3] { [vec![i.requester.clone().unwrap_or_default()], { let w = p.workers(&i.id); if w.is_empty() { vec![String::new()] } else { w } }, vec![i.env.clone().unwrap_or_default()]] };
        let mut choices: [BTreeSet<String>; 3] = Default::default();
        let mut groups: Vec<(&Project, Vec<(&Item, usize)>)> = Vec::new();
        for p in &projects {
            fn walk<'a>(p: &'a Project, quote: Option<&str>, parent: Option<&str>, depth: usize, out: &mut Vec<(&'a Item, usize)>) {
                for i in p.children(quote, parent) {
                    out.push((i, depth));
                    walk(p, quote, Some(&i.id), depth + 1, out);
                }
            }
            let mut all = Vec::new();
            walk(p, None, None, 0, &mut all);
            for q in &p.quotes {
                walk(p, Some(&q.id), None, 0, &mut all);
            }
            let rows: Vec<(&Item, usize)> = all
                .into_iter()
                .filter(|(i, _)| keep(p, i))
                .filter(|(i, _)| {
                    let v = values(p, i);
                    for (k, vs) in v.iter().enumerate() {
                        choices[k].extend(vs.iter().cloned());
                    }
                    v.iter().zip(&f.cols).all(|(vs, chosen)| chosen.is_empty() || vs.iter().any(|x| chosen.contains(x)))
                })
                .collect();
            if !rows.is_empty() {
                groups.push((p, rows));
            }
        }
        if groups.is_empty() {
            card(ui, &theme, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(t.track_no_planned).size(12.5).color(theme.text_muted));
            });
        }
        let muted = |s: &str| egui::RichText::new(s).size(12.5).color(theme.text_muted);
        // Short: « 17/09 » this year, « 17/09/25 » another.
        let year = today.get(..4).unwrap_or("").to_owned();
        let day = |d: &Option<String>| {
            d.as_deref().map(|d| match (d.get(..4), d.get(2..4), d.get(5..7), d.get(8..10)) {
                (Some(y), _, Some(m), Some(dd)) if y == year => format!("{dd}/{m}"),
                (_, Some(yy), Some(m), Some(dd)) => format!("{dd}/{m}/{yy}"),
                _ => d.to_owned(),
            })
        };
        // A real table: a frame, a heading row, a line under each row, a line between columns. The columns
        // after the task's are each their width (padding inside); the task's takes what is left (220 at
        // least), the table scrolling sideways only in a narrow window.
        const COLS: [f32; 10] = [112.0, 104.0, 112.0, 80.0, 88.0, 86.0, 74.0, 76.0, 76.0, 58.0];
        // The figures, right aligned.
        const RIGHT: [bool; 10] = [false, false, false, false, false, false, false, true, true, true];
        const PAD: f32 = 10.0;
        const LINE_H: f32 = 38.0;
        const HEAD_H: f32 = 34.0;
        let others: f32 = COLS.iter().sum();
        let room = ui.available_width();
        let title_w = (room - others - 2.0).max(220.0);
        let heads = [t.track_status, t.track_requester, t.track_worked_by, t.track_requested, t.track_due, t.track_finished, t.track_env_short, t.track_estimate_short, t.track_spent_short, "%"];
        // A cell `w` wide and `h` high, its content on one line, centered in its height, padded.
        fn cell(ui: &mut Ui, w: f32, h: f32, right: bool, add: impl FnOnce(&mut Ui)) {
            let layout = if right { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
            ui.allocate_ui_with_layout(Vec2::new(w, h), layout, |ui| {
                ui.set_width(w);
                ui.set_height(h);
                ui.add_space(PAD);
                ui.allocate_ui_with_layout(Vec2::new(w - 2.0 * PAD, h), layout, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                    add(ui);
                });
            });
        }
        let mut table = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let row_w = title_w + others;
            let top = ui.cursor().min;
            let painter = ui.painter().clone();
            let line = theme.tab_hover;
            let thin = Stroke::new(1.0, line.gamma_multiply(0.7));
            let full = |y: f32, stroke: Stroke| painter.hline(top.x..=top.x + row_w, y, stroke);
            // Where the columns meet, and the rows they are drawn across (not the projects').
            let mut edges = vec![top.x + title_w];
            for w in &COLS[..COLS.len() - 1] {
                let last = *edges.last().unwrap_or(&top.x);
                edges.push(last + w);
            }
            let mut spans: Vec<(f32, f32)> = Vec::new();
            let head_bg = painter.add(egui::Shape::Noop);
            let head = ui.horizontal(|ui| {
                cell(ui, title_w, HEAD_H, false, |ui| {
                    ui.label(egui::RichText::new(t.track_task_col).size(13.0).strong().color(theme.text_muted));
                });
                for (k, h) in heads.iter().enumerate() {
                    // Who asked, who worked, where: a filter, its values ticked in a menu.
                    let col = match k {
                        1 => Some(0),
                        2 => Some(1),
                        6 => Some(2),
                        _ => None,
                    };
                    cell(ui, COLS[k], HEAD_H, RIGHT[k], |ui| {
                        let Some(c) = col else {
                            ui.label(egui::RichText::new(*h).size(13.0).strong().color(theme.text_muted));
                            return;
                        };
                        let on = !self.track.tasks.cols[c].is_empty();
                        let text = egui::RichText::new(format!("{h}  {}", if on { "●" } else { "▾" })).size(13.0).strong().color(if on { theme.accent } else { theme.text_muted });
                        let resp = ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                            if ui.add_enabled(on, egui::Button::new(t.track_col_all)).clicked() {
                                out.push(TaskAct::Filter(c, None));
                            }
                            ui.separator();
                            for v in &choices[c] {
                                let mut ticked = self.track.tasks.cols[c].contains(v);
                                let name = if v.is_empty() { "—".to_owned() } else if c == 2 { env_name(v).to_owned() } else { v.clone() };
                                if ui.checkbox(&mut ticked, name).changed() {
                                    out.push(TaskAct::Filter(c, Some(v.clone())));
                                }
                            }
                        });
                    });
                }
            });
            let hr = Rect::from_min_size(head.response.rect.min, Vec2::new(row_w, head.response.rect.height()));
            painter.set(head_bg, egui::Shape::rect_filled(hr, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, line.gamma_multiply(0.55)));
            spans.push((hr.top(), hr.bottom()));
            full(hr.bottom(), Stroke::new(1.0, line));
            let mut bottom = hr.bottom();
            for (p, rows) in &groups {
                // The project: a row across, tinted its color, its client and name, its time together.
                let (est, spent) = rows.iter().filter(|(_, d)| *d == 0).fold((0u64, 0u64), |(e, s), (i, _)| {
                    let r = p.rollup(i);
                    (e + r.estimate, s + r.spent)
                });
                let color = color_of(p.color.as_deref()).unwrap_or(theme.accent);
                let band = painter.add(egui::Shape::Noop);
                let shut = self.track.tasks.folded.contains(&p.id);
                let head = ui.horizontal(|ui| {
                    cell(ui, title_w, LINE_H - 4.0, false, |ui| {
                        ui.label(egui::RichText::new(if shut { "▸" } else { "▾" }).size(13.0).color(theme.text_muted));
                        if let Some(c) = &p.client {
                            ui.label(egui::RichText::new(c).size(13.0).color(theme.text_muted));
                            ui.label(egui::RichText::new("›").size(13.0).color(theme.text_muted));
                        }
                        ui.label(egui::RichText::new(&p.name).size(13.5).strong().color(theme.text));
                    });
                    for (k, w) in COLS.iter().enumerate() {
                        cell(ui, *w, LINE_H - 4.0, RIGHT[k], |ui| match k {
                            7 if est > 0 => {
                                ui.label(egui::RichText::new(hm(est as i64)).size(12.5).strong().color(theme.text));
                            }
                            8 if spent > 0 => {
                                ui.label(egui::RichText::new(hm(spent as i64)).size(12.5).strong().color(theme.text));
                            }
                            _ => {}
                        });
                    }
                });
                let r = Rect::from_min_size(head.response.rect.min, Vec2::new(row_w, head.response.rect.height()));
                // A click on it folds its tasks, or unfolds them.
                let fold = ui.interact(r, ui.id().with(("fold", &p.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if fold.clicked() {
                    out.push(TaskAct::Fold(p.id.clone()));
                }
                let color = if fold.hovered() { color.gamma_multiply(1.6) } else { color };
                painter.set(band, egui::Shape::Vec(vec![egui::Shape::rect_filled(r, 0.0, color.gamma_multiply(0.10)), egui::Shape::rect_filled(Rect::from_min_size(r.min, Vec2::new(3.0, r.height())), 0.0, color)]));
                full(r.bottom(), thin);
                bottom = r.bottom();
                if shut {
                    continue;
                }
                for (i, depth) in rows {
                    let back = painter.add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        cell(ui, title_w, LINE_H, false, |ui| {
                            ui.add_space(*depth as f32 * 18.0);
                            // Its title, its comment after it in grey, on one line (whole on hover).
                            let mut job = egui::text::LayoutJob::default();
                            let fmt = |size: f32, color: Color32, italics: bool| egui::TextFormat { font_id: egui::FontId::proportional(size), color, italics, ..Default::default() };
                            job.append(&if *depth > 0 { format!("↳ {}", i.title) } else { i.title.clone() }, 0.0, fmt(13.0, if i.status.closed() { theme.text_muted } else { theme.text }, false));
                            if let Some(c) = &i.comment {
                                job.append(&format!("  ·  {c}"), 0.0, fmt(12.0, theme.text_muted, true));
                            }
                            let full = format!("{}{}", i.title, i.comment.as_ref().map(|c| format!("\n{c}")).unwrap_or_default());
                            let resp = ui.add(egui::Label::new(job).truncate().sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(full);
                            if resp.clicked() {
                                out.push(TaskAct::Open(p.id.clone(), i.id.clone()));
                            }
                        });
                        // The state: a pill; a click, the others.
                        cell(ui, COLS[0], LINE_H, false, |ui| {
                            let pill = status_pill(ui, i.status, t, &theme);
                            if p.role.writes() {
                                egui::Popup::menu(&pill).show(|ui| {
                                    for st in Status::ALL {
                                        if ui.add(egui::Button::new(egui::RichText::new(status_name(st, t)).color(status_color(st, &theme))).selected(st == i.status)).clicked() && st != i.status {
                                            out.push(TaskAct::Status(p.id.clone(), i.id.clone(), st));
                                        }
                                    }
                                });
                            }
                        });
                        cell(ui, COLS[1], LINE_H, false, |ui| {
                            ui.add(egui::Label::new(i.requester.as_deref().map_or_else(|| muted("—"), |w| egui::RichText::new(w).size(12.5).color(theme.text))).truncate());
                        });
                        // Who worked on it: those who logged time on it, or on its sub-tasks.
                        let workers = p.workers(&i.id);
                        cell(ui, COLS[2], LINE_H, false, |ui| {
                            if workers.is_empty() {
                                ui.label(muted("—"));
                            } else {
                                ui.add(egui::Label::new(egui::RichText::new(workers.join(", ")).size(12.5).color(theme.text)).truncate()).on_hover_text(workers.join("\n"));
                            }
                        });
                        cell(ui, COLS[3], LINE_H, false, |ui| {
                            ui.label(day(&i.requested).map_or_else(|| muted("—"), |d| egui::RichText::new(d).size(12.5).color(theme.text)));
                        });
                        let late = i.late(&today);
                        cell(ui, COLS[4], LINE_H, false, |ui| {
                            ui.label(day(&i.due).map_or_else(|| muted("—"), |d| egui::RichText::new(d).size(12.5).color(if late { RED } else { theme.text }).strong()));
                        });
                        cell(ui, COLS[5], LINE_H, false, |ui| {
                            ui.label(day(&i.finished).map_or_else(|| muted("—"), |d| egui::RichText::new(d).size(12.5).color(theme.text)));
                        });
                        cell(ui, COLS[6], LINE_H, false, |ui| match i.env.as_deref() {
                            Some(e) => tag(ui, env_name(e), env_color(e, &theme)),
                            None => {
                                ui.label(muted("—"));
                            }
                        });
                        let r = p.rollup(i);
                        cell(ui, COLS[7], LINE_H, true, |ui| {
                            ui.label(if r.estimate > 0 { egui::RichText::new(hm(r.estimate as i64)).size(12.5).color(theme.text) } else { muted("—") });
                        });
                        cell(ui, COLS[8], LINE_H, true, |ui| {
                            ui.label(if r.spent > 0 { egui::RichText::new(hm(r.spent as i64)).size(12.5).color(if r.estimate > 0 && r.spent > r.estimate { RED } else { theme.text }) } else { muted("—") });
                        });
                        cell(ui, COLS[9], LINE_H, true, |ui| {
                            ui.label(i.progress().map_or_else(|| muted("—"), |v| egui::RichText::new(format!("{v} %")).size(12.5).color(theme.text)));
                        });
                    });
                    // The row under the pointer, lit.
                    let r = Rect::from_min_size(row.response.rect.min, Vec2::new(row_w, row.response.rect.height()));
                    if ui.rect_contains_pointer(r) {
                        painter.set(back, egui::Shape::rect_filled(r, 0.0, line.gamma_multiply(0.35)));
                    }
                    spans.push((r.top(), r.bottom()));
                    full(r.bottom(), thin);
                    bottom = r.bottom();
                }
            }
            for (a, b) in spans {
                for x in &edges {
                    painter.vline(*x, a..=b, thin);
                }
            }
            painter.rect_stroke(Rect::from_min_max(top, egui::pos2(top.x + row_w, bottom)), 8.0, Stroke::new(1.0, line), egui::StrokeKind::Inside);
        };
        if title_w + others <= room + 0.5 {
            table(ui);
        } else {
            egui::ScrollArea::horizontal().id_salt("tasks-table").show(ui, |ui| {
                table(ui);
                // Room for the scroll bar, under the last row.
                ui.add_space(14.0);
            });
        }

        for act in out {
            match act {
                // Its project opened, its window on it.
                TaskAct::Open(pid, item) => {
                    let form = self.track.projects.get(&pid).and_then(|p| p.items.iter().find(|i| i.id == item)).map(ItemForm::of);
                    self.track_select(Some(pid));
                    self.track.page.tab = Tab::Quotes;
                    self.track.page.item = form;
                }
                TaskAct::Filter(c, v) => match v {
                    None => self.track.tasks.cols[c].clear(),
                    Some(v) => {
                        if !self.track.tasks.cols[c].remove(&v) {
                            self.track.tasks.cols[c].insert(v);
                        }
                    }
                },
                TaskAct::Fold(pid) => {
                    if !self.track.tasks.folded.remove(&pid) {
                        self.track.tasks.folded.insert(pid);
                    }
                }
                TaskAct::Status(pid, item, status) => self.track_write(move |c| c.update_item(&pid, &item, serde_json::json!({ "status": status.word() }))),
                TaskAct::New(pid) => {
                    self.track_select(Some(pid));
                    self.track.page.tab = Tab::Quotes;
                    self.track.page.item = Some(ItemForm { requested: show_day(&track::today()), ..ItemForm::default() });
                }
            }
        }
    }

    /// One project: its head, its tabs.
    pub(super) fn track_project(&mut self, ui: &mut Ui, p: &Project) {
        // The images its descriptions show, asked of floor once.
        for text in std::iter::once(p.note.as_deref().unwrap_or("")).chain(p.tasks.iter().map(|x| x.label.as_str())).chain(p.quotes.iter().map(|x| x.note.as_deref().unwrap_or(""))).chain(p.items.iter().map(|x| x.body.as_deref().unwrap_or(""))) {
            self.track_want_images(&p.id, text);
        }
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
                    md_view(ui, Id::new(("track-note", &p.id)), note, width - 32.0, &theme, t, &self.track.images);
                }
                None => {
                    ui.label(egui::RichText::new(t.track_note_empty).size(12.5).color(theme.text_muted));
                }
            }
        });

        // The project's documents (a contract, a specification...).
        ui.add_space(gap);
        card(ui, &theme, |ui| {
            ui.set_width(width - 32.0);
            heading(ui, t.track_documents, &theme);
            files_row(ui, p, "project", p.role.writes(), &theme, t, acts);
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
                if !shown.is_empty() && ui.add(quiet(&format!("⤓  {}", t.track_copy_csv))).on_hover_text(t.track_copy_csv_hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    acts.push(Act::ExportCsv);
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
        // One table, by day (newest first): each day a band with its total, its entries under it.
        let width = ui.available_width();
        // Durée, Imputé à, Description, Par, the actions.
        let w = widths(width, vec![84.0, 210.0, 0.0, 120.0, 70.0], 2, 240.0);
        let table_w: f32 = w.iter().sum();
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let mut tb = Table::new(ui, w.clone());
            tb.head(ui, &theme, &[t.track_duration, t.track_charge, t.track_description, t.track_logged_by, ""], &[true, false, false, false, true]);
            let mut k = 0;
            while k < shown.len() {
                let d = shown[k].day.as_str();
                let of_day: Vec<&Task> = shown[k..].iter().take_while(|x| x.day == d).copied().collect();
                k += of_day.len();
                let total: i64 = of_day.iter().map(|x| x.minutes as i64).sum();
                tb.row(ui, &theme, false, Some(theme.tab_hover.gamma_multiply(0.3)), |ui| {
                    tcell(ui, w[0], CELL_H - 4.0, line_at(true), |ui| {
                        ui.label(egui::RichText::new(hm(total)).size(12.5).strong().color(theme.text));
                    });
                    tcell(ui, table_w - w[0], CELL_H - 4.0, line_at(false), |ui| {
                        ui.label(egui::RichText::new(long_day(d, t)).size(13.0).strong().color(theme.text));
                    });
                });
                for task in of_day {
                    let can = p.role.manages() || (p.role.writes() && task.mine);
                    tb.row(ui, &theme, true, None, |ui| {
                        tcell(ui, w[0], CELL_H, line_at(true), |ui| {
                            ui.label(egui::RichText::new(hm(task.minutes as i64)).size(13.5).strong().color(theme.text));
                        });
                        tcell(ui, w[1], CELL_H, line_at(false), |ui| {
                            tag(ui, &self.track_charged(p, task), billing_color(&theme, task.billing));
                        });
                        // What was done: its task first if any, the text (long ones folded), the files.
                        tcell(ui, w[2], CELL_H, egui::Layout::top_down(egui::Align::Min), |ui| {
                            let body_w = w[2] - 2.0 * CELL_PAD;
                            ui.add_space(9.0);
                            let long = task.label.lines().count() > 6 || task.label.len() > 600;
                            let open = self.track.page.open_entries.contains(&task.id);
                            let text = if long && !open { task.label.lines().take(5).collect::<Vec<_>>().join("\n") } else { task.label.clone() };
                            let clicked = md_view(ui, Id::new(("track-entry", &task.id, open)), &text, body_w, &theme, t, &self.track.images);
                            if let Some(new) = clicked.toggled.filter(|_| can && (!long || open)) {
                                acts.push(Act::EntryText(task.id.clone(), new));
                            }
                            if let Some(item) = &task.item {
                                ui.add(egui::Label::new(egui::RichText::new(format!("☑  {}", p.item_path(item))).size(11.5).color(theme.text_muted)).truncate());
                            }
                            files_row(ui, p, &format!("task:{}", task.id), false, &theme, t, acts);
                            if long && ui.add(link_button(if open { t.track_show_less } else { t.track_show_more }, theme.accent)).clicked() && !self.track.page.open_entries.remove(&task.id) {
                                self.track.page.open_entries.insert(task.id.clone());
                            }
                            ui.add_space(8.0);
                        });
                        tcell(ui, w[3], CELL_H, line_at(false), |ui| {
                            initial(ui, task.author.as_deref().unwrap_or("?"), &theme);
                            ui.add(egui::Label::new(egui::RichText::new(task.author.as_deref().unwrap_or("—")).size(12.5).color(theme.text)).truncate());
                        });
                        tcell(ui, w[4], CELL_H, line_at(true), |ui| {
                            if can {
                                let confirming = self.track.page.confirm.as_deref() == Some(task.id.as_str());
                                if ui.add(link_button(if confirming { "✕ ?" } else { "✕" }, if confirming { RED } else { theme.text_muted })).on_hover_text(t.track_delete).clicked() {
                                    if confirming {
                                        acts.push(Act::DeleteEntry(task.id.clone()));
                                    } else {
                                        self.track.page.confirm = Some(task.id.clone());
                                    }
                                }
                                if ui.add(link_button("✎", theme.text_muted)).on_hover_text(t.track_edit).clicked() {
                                    acts.push(Act::EditEntry((*task).clone()));
                                }
                            }
                        });
                    });
                }
            }
            tb.finish(ui, &theme);
        };
        if table_w <= width + 0.5 {
            draw(ui);
        } else {
            egui::ScrollArea::horizontal().id_salt("entries-table").show(ui, |ui| {
                draw(ui);
                ui.add_space(14.0);
            });
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
                        let clicked = md_view(ui, Id::new(("quote-note", &q.id)), note, width - 32.0, &theme, t, &self.track.images);
                        if let Some(new) = clicked.toggled.filter(|_| p.role.manages()) {
                            acts.push(Act::QuoteNote(q.id.clone(), new));
                        }
                        ui.add_space(10.0);
                    }
                    files_row(ui, p, &format!("quote:{}", q.id), p.role.manages(), &theme, t, acts);
                    ui.add_space(6.0);
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

    /// The planned tasks of a quote (or the project's own), as a table (sub-tasks under theirs); a task
    /// added at the end.
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
        } else {
            let today = track::today();
            let muted = |s: &str| egui::RichText::new(s.to_owned()).size(12.5).color(theme.text_muted);
            let plain = |s: &str| egui::RichText::new(s.to_owned()).size(12.5).color(theme.text);
            // Tâche, État, Demandeur, Fin prévue, Env., Passé, Estimé, %, the actions.
            let w = widths(width, vec![0.0, 112.0, 104.0, 88.0, 74.0, 76.0, 76.0, 58.0, 64.0], 0, 220.0);
            let table_w: f32 = w.iter().sum();
            let mut draw = |ui: &mut Ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let mut tb = Table::new(ui, w.clone());
                const RIGHT: [bool; 9] = [false, false, false, false, false, true, true, true, true];
                tb.head(ui, &theme, &[t.track_task_col, t.track_status, t.track_requester, t.track_due, t.track_env_short, t.track_spent_short, t.track_estimate_short, "%", ""], &RIGHT);
                for (item, depth) in &rows {
                    let r = p.rollup(item);
                    let open = self.track.page.open_items.contains(&item.id);
                    let siblings = p.children(quote, item.parent.as_deref());
                    let at = siblings.iter().position(|s| s.id == item.id).unwrap_or(0);
                    tb.row(ui, &theme, true, None, |ui| {
                        tcell(ui, w[0], CELL_H, line_at(false), |ui| {
                            ui.add_space(*depth as f32 * 20.0);
                            let closed = item.status.closed();
                            let text = if *depth > 0 { format!("↳ {}", item.title) } else { item.title.clone() };
                            let title = egui::RichText::new(text).size(13.0).color(if closed { theme.text_muted } else { theme.text });
                            let title = if closed { title.strikethrough() } else { title };
                            let has_body = item.body.as_deref().is_some_and(|b| !b.trim().is_empty());
                            let mut toggle = false;
                            if has_body {
                                toggle |= ui.add(link_button(if open { "▾" } else { "▸" }, theme.text_muted)).on_hover_text(t.track_details).clicked();
                            }
                            let resp = ui.add(egui::Label::new(if *depth == 0 { title.strong() } else { title }).truncate().sense(Sense::click())).on_hover_text(&item.title);
                            toggle |= has_body && resp.clicked();
                            if toggle && !self.track.page.open_items.remove(&item.id) {
                                self.track.page.open_items.insert(item.id.clone());
                            }
                        });
                        // The state: a pill; a click, the others.
                        tcell(ui, w[1], CELL_H, line_at(false), |ui| {
                            let pill = status_pill(ui, item.status, t, &theme);
                            if p.role.writes() {
                                egui::Popup::menu(&pill).show(|ui| {
                                    for st in Status::ALL {
                                        if ui.add(egui::Button::new(egui::RichText::new(status_name(st, t)).color(status_color(st, &theme))).selected(st == item.status)).clicked() && st != item.status {
                                            acts.push(Act::SetStatus(item.id.clone(), st));
                                        }
                                    }
                                });
                            }
                        });
                        tcell(ui, w[2], CELL_H, line_at(false), |ui| {
                            ui.add(egui::Label::new(item.requester.as_deref().map_or_else(|| muted("—"), plain)).truncate());
                        });
                        tcell(ui, w[3], CELL_H, line_at(false), |ui| {
                            let late = item.late(&today);
                            ui.label(item.due.as_deref().map_or_else(|| muted("—"), |d| egui::RichText::new(show_day(d)).size(12.5).color(if late { RED } else { theme.text })));
                        });
                        tcell(ui, w[4], CELL_H, line_at(false), |ui| match item.env.as_deref() {
                            Some(e) => tag(ui, env_name(e), env_color(e, &theme)),
                            None => {
                                ui.label(muted("—"));
                            }
                        });
                        let over = r.estimate > 0 && r.spent > r.estimate;
                        tcell(ui, w[5], CELL_H, line_at(true), |ui| {
                            ui.label(if r.spent > 0 { egui::RichText::new(hm(r.spent as i64)).size(12.5).color(if over { RED } else { theme.text }) } else { muted("—") });
                        });
                        tcell(ui, w[6], CELL_H, line_at(true), |ui| {
                            ui.label(if r.estimate > 0 { plain(&hm(r.estimate as i64)) } else { muted("—") });
                        });
                        tcell(ui, w[7], CELL_H, line_at(true), |ui| {
                            ui.label(item.progress().map_or_else(|| muted("—"), |v| plain(&format!("{v} %"))));
                        });
                        tcell(ui, w[8], CELL_H, line_at(true), |ui| {
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
                        });
                    });
                    // Its description, unfolded: across the table, under it.
                    if open && let Some(body) = item.body.as_deref() {
                        let indent = 34.0 + *depth as f32 * 20.0;
                        let body_w = table_w - indent - 2.0 * CELL_PAD;
                        tb.row(ui, &theme, false, Some(theme.tab_hover.gamma_multiply(0.18)), |ui| {
                            ui.add_space(indent);
                            ui.vertical(|ui| {
                                ui.add_space(8.0);
                                let clicked = md_view(ui, Id::new(("item-body", &item.id)), body, body_w, &theme, t, &self.track.images);
                                if let Some(new) = clicked.toggled.filter(|_| p.role.manages()) {
                                    acts.push(Act::ItemBody(item.id.clone(), new));
                                }
                                files_row(ui, p, &format!("item:{}", item.id), p.role.writes(), &theme, t, acts);
                                ui.add_space(8.0);
                            });
                        });
                    }
                }
                tb.finish(ui, &theme);
            };
            if table_w <= width + 0.5 {
                draw(ui);
            } else {
                egui::ScrollArea::horizontal().id_salt(("items-table", quote)).show(ui, |ui| {
                    draw(ui);
                    ui.add_space(14.0);
                });
            }
        }
        if p.role.manages() {
            ui.add_space(8.0);
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
                files_row(ui, p, &format!("counter:{}", c.id), open && p.role.manages(), &theme, t, acts);
                ui.add_space(6.0);
                let fraction = if c.minutes > 0 { c.used as f32 / c.minutes as f32 } else { 1.0 };
                gauge(ui, width - 32.0, fraction, left_color_bar(&theme, c.left(), c.minutes), &theme);
                if open {
                    ui.add_space(10.0);
                    let entries: Vec<&Task> = p.tasks.iter().filter(|x| x.counter.as_deref() == Some(c.id.as_str())).collect();
                    if entries.is_empty() {
                        ui.label(egui::RichText::new(t.track_no_tasks).size(12.0).color(theme.text_muted));
                    }
                    if !entries.is_empty() {
                        // Its entries: a table, Date, Durée, Description, Saisi par.
                        let w = widths(width - 32.0, vec![110.0, 84.0, 0.0, 150.0], 2, 160.0);
                        ui.scope(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            let mut tb = Table::new(ui, w.clone());
                            tb.head(ui, &theme, &[t.track_day, t.track_duration, t.track_description, t.track_logged_by], &[false, true, false, false]);
                            for task in entries {
                                tb.row(ui, &theme, true, None, |ui| {
                                    tcell(ui, w[0], CELL_H - 6.0, line_at(false), |ui| {
                                        ui.label(egui::RichText::new(show_day(&task.day)).size(12.5).color(theme.text));
                                    });
                                    tcell(ui, w[1], CELL_H - 6.0, line_at(true), |ui| {
                                        ui.label(egui::RichText::new(hm(task.minutes as i64)).size(12.5).strong().color(theme.text));
                                    });
                                    tcell(ui, w[2], CELL_H - 6.0, line_at(false), |ui| {
                                        ui.add(egui::Label::new(egui::RichText::new(first_line(&task.label)).size(12.5).color(theme.text)).truncate()).on_hover_text(&task.label);
                                    });
                                    tcell(ui, w[3], CELL_H - 6.0, line_at(false), |ui| {
                                        ui.add(egui::Label::new(egui::RichText::new(task.author.as_deref().unwrap_or("—")).size(12.5).color(theme.text_muted)).truncate());
                                    });
                                });
                            }
                            tb.finish(ui, &theme);
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
            Act::Files(on, act) => {
                self.track_file_acts(&id, &on, vec![act]);
            }
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
                    pending: Vec::new(),
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
                        pending: Vec::new(),
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
                    self.track.page.counter = Some(CounterForm { editing: Some(c.id.clone()), label: c.label.clone(), hours: typed(c.minutes), day: show_day(&c.day), pending: Vec::new() });
                }
            }
            Act::CloseCounter(counter, closed) => self.track_write(move |c| c.update_counter(&id, &counter, serde_json::json!({ "closed": closed }))),
            Act::DeleteCounter(counter) => self.track_write(move |c| c.delete_counter(&id, &counter)),
            Act::NewItem { quote, parent } => {
                self.track.page.form_error = None;
                self.track.page.item = Some(ItemForm { quote, parent, requested: show_day(&track::today()), ..ItemForm::default() });
            }
            Act::EditItem(item) => {
                if let Some(i) = p.and_then(|p| p.items.iter().find(|i| i.id == item)) {
                    self.track.page.form_error = None;
                    self.track.page.item = Some(ItemForm::of(i));
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
            Act::ExportCsv => {
                if let Some(p) = p {
                    // A BOM, so that Excel reads the accents right.
                    let csv = format!("\u{feff}{}\r\n", self.track_csv(p).replace('\n', "\r\n"));
                    let name = format!("{} - {} {}.csv", p.name, self.t().track_csv_file, track::today()).replace(['/', '\\', ':'], "-");
                    if let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter("CSV", &["csv"]).save_file() {
                        match std::fs::write(&path, csv) {
                            Ok(()) => self.inform(self.t().track_csv_copied.replace("{file}", &path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
                            Err(e) => self.error = Some(format!("{} : {e}", path.display())),
                        }
                    }
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
            let images = &self.track.images;
            let on_target = f.editing.as_ref().map(|e| format!("task:{e}"));
            let (mut pasted, mut file_acts) = (None::<(String, Vec<u8>)>, Vec::new());
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
                            let day_field = ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(100.0).min_size(Vec2::new(0.0, FIELD_HEIGHT)).vertical_align(egui::Align::Center).hint_text("18/11"));
                            day_picker(ui, &day_field, &mut f.day, t, &theme);
                            let days = [track::today(), track::parse_day("hier").unwrap_or_default()];
                            let typed_day = track::parse_day(&f.day);
                            if let Some(i) = super::segmented::segmented(ui, &theme, FIELD_HEIGHT, &[t.track_today_word, t.track_yesterday], days.iter().position(|d| typed_day.as_deref() == Some(d.as_str()))) {
                                f.day = show_day(&days[i]);
                            }
                        });
                    });
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_duration);
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut f.duration).desired_width(70.0).min_size(Vec2::new(0.0, FIELD_HEIGHT)).vertical_align(egui::Align::Center).hint_text("1h30"));
                            let presets = [15u32, 30, 60, 120, 240, 420];
                            let names: Vec<String> = presets.iter().map(|m| hm(*m as i64)).collect();
                            let names: Vec<&str> = names.iter().map(String::as_str).collect();
                            let typed_minutes = track::parse_duration(&f.duration);
                            if let Some(i) = super::segmented::segmented(ui, &theme, FIELD_HEIGHT, &names, presets.iter().position(|m| typed_minutes == Some(*m))) {
                                f.duration = typed(presets[i] as u64);
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
                let edited = md_editor(ui, Id::new(("entry-label", f.editing.clone())), &mut f.label, 640.0, 6, t.track_label_hint, &theme, t, images);
                if edited.pasted.is_some() {
                    pasted = edited.pasted;
                }
                field(ui, t.track_files);
                let shown: Vec<&TrackFile> = p.files.iter().filter(|x| on_target.as_deref() == Some(x.on.as_str()) || f.pending.contains(&x.id)).collect();
                files_ui(ui, &shown, p.role.manages(), true, &theme, t, &mut file_acts);
                ui.label(egui::RichText::new(t.track_files_hint).size(11.0).color(theme.text_muted));
                let ready = track::parse_duration(&f.duration).is_some() && !f.label.trim().is_empty();
                let (ok, cancel) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_add_task }, ready);
                close = cancel;
                if ok {
                    send = Some(());
                }
            });
            // Its files: an image pasted, files picked or dropped on the window (sent to what it is,
            // or, being made, to the project until it is).
            let target = on_target.clone().unwrap_or_else(|| "project".to_owned());
            let mut sent = Vec::new();
            if let Some((file, png)) = pasted {
                sent.push(file.clone());
                self.track_pasted(&id, &target, file, png);
            }
            sent.extend(self.track_file_acts(&id, &target, file_acts));
            let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|d| d.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
            sent.extend(self.track_attach(&id, &target, dropped));
            if on_target.is_none()
                && let Some(f) = &mut self.track.page.entry
            {
                f.pending.extend(sent);
            }
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
                        let pending = self.track.page.entry.as_ref().map(|f| f.pending.clone()).unwrap_or_default();
                        let before: Vec<String> = p.tasks.iter().map(|x| x.id.clone()).collect();
                        self.track_write(move |c| match editing {
                            Some(task) => c.update_task(&id, &task, &input),
                            None => adopt(c, &id, c.add_task(&id, &input)?, |p| p.tasks.iter().map(|x| x.id.clone()).collect(), &before, "task", &pending),
                        });
                        self.track.page.entry = None;
                    }
                    Err(e) => self.track.page.form_error = Some(e.to_owned()),
                }
            } else if close || modal.should_close() {
                let pending = self.track.page.entry.take().map(|f| f.pending);
                self.track_drop_pending(&id, pending);
            }
        }

        // A quote.
        if let Some(f) = &mut self.track.page.quote {
            let images = &self.track.images;
            let on_target = f.editing.as_ref().map(|e| format!("quote:{e}"));
            let (mut pasted, mut file_acts) = (None::<(String, Vec<u8>)>, Vec::new());
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
                let edited = md_editor(ui, Id::new(("quote-note-edit", f.editing.clone())), &mut f.note, 640.0, 8, t.track_quote_note_hint, &theme, t, images);
                if edited.pasted.is_some() {
                    pasted = edited.pasted;
                }
                field(ui, t.track_files);
                let shown: Vec<&TrackFile> = p.files.iter().filter(|x| on_target.as_deref() == Some(x.on.as_str()) || f.pending.contains(&x.id)).collect();
                files_ui(ui, &shown, p.role.manages(), true, &theme, t, &mut file_acts);
                ui.label(egui::RichText::new(t.track_files_hint).size(11.0).color(theme.text_muted));
                let ready = !f.label.trim().is_empty() && (f.hours.trim().is_empty() || track::parse_duration(&f.hours).is_some());
                (ok, close) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_create }, ready);
            });
            // Its files: an image pasted, files picked or dropped on the window (sent to what it is,
            // or, being made, to the project until it is).
            let target = on_target.clone().unwrap_or_else(|| "project".to_owned());
            let mut sent = Vec::new();
            if let Some((file, png)) = pasted {
                sent.push(file.clone());
                self.track_pasted(&id, &target, file, png);
            }
            sent.extend(self.track_file_acts(&id, &target, file_acts));
            let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|d| d.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
            sent.extend(self.track_attach(&id, &target, dropped));
            if on_target.is_none()
                && let Some(f) = &mut self.track.page.quote
            {
                f.pending.extend(sent);
            }
            if ok {
                let f = self.track.page.quote.as_ref().unwrap();
                match track::parse_day(&f.day) {
                    Some(day) => {
                        let (editing, number, label, minutes, note) = (f.editing.clone(), Some(f.number.trim().to_owned()).filter(|n| !n.is_empty()), f.label.trim().to_owned(), track::parse_duration(&f.hours).map(u64::from), f.note.trim().to_owned());
                        let id = id.clone();
                        let (pending, before): (Vec<String>, Vec<String>) = (f.pending.clone(), p.quotes.iter().map(|x| x.id.clone()).collect());
                        self.track_write(move |c| match editing {
                            Some(q) => c.update_quote(&id, &q, serde_json::json!({ "number": number, "label": label, "minutes": minutes, "day": day, "note": note })),
                            None => adopt(c, &id, c.add_quote_full(&id, number.as_deref(), &label, minutes, &day, Some(note.as_str()).filter(|n| !n.is_empty()))?, |p| p.quotes.iter().map(|x| x.id.clone()).collect(), &before, "quote", &pending),
                        });
                        self.track.page.quote = None;
                    }
                    None => self.track.page.form_error = Some(t.track_bad_day.to_owned()),
                }
            } else if close || modal.should_close() {
                let pending = self.track.page.quote.take().map(|f| f.pending);
                self.track_drop_pending(&id, pending);
            }
        }

        // A counter.
        if let Some(f) = &mut self.track.page.counter {
            let images = &self.track.images;
            let on_target = f.editing.as_ref().map(|e| format!("counter:{e}"));
            let (mut pasted, mut file_acts) = (None::<(String, Vec<u8>)>, Vec::new());
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
                field(ui, t.track_files);
                let shown: Vec<&TrackFile> = p.files.iter().filter(|x| on_target.as_deref() == Some(x.on.as_str()) || f.pending.contains(&x.id)).collect();
                files_ui(ui, &shown, p.role.manages(), true, &theme, t, &mut file_acts);
                ui.label(egui::RichText::new(t.track_files_hint).size(11.0).color(theme.text_muted));
                let _ = (&mut pasted, images);
                let ready = track::parse_duration(&f.hours).is_some();
                (ok, close) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_create }, ready);
            });
            // Its files: an image pasted, files picked or dropped on the window (sent to what it is,
            // or, being made, to the project until it is).
            let target = on_target.clone().unwrap_or_else(|| "project".to_owned());
            let mut sent = Vec::new();
            if let Some((file, png)) = pasted {
                sent.push(file.clone());
                self.track_pasted(&id, &target, file, png);
            }
            sent.extend(self.track_file_acts(&id, &target, file_acts));
            let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|d| d.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
            sent.extend(self.track_attach(&id, &target, dropped));
            if on_target.is_none()
                && let Some(f) = &mut self.track.page.counter
            {
                f.pending.extend(sent);
            }
            if ok {
                let f = self.track.page.counter.as_ref().unwrap();
                match (track::parse_day(&f.day), track::parse_duration(&f.hours)) {
                    (Some(day), Some(minutes)) => {
                        let label = if f.label.trim().is_empty() { t.track_counter_default.replace("{h}", &hm(minutes as i64)).replace("{d}", &show_day(&day)) } else { f.label.trim().to_owned() };
                        let (editing, id) = (f.editing.clone(), id.clone());
                        let (pending, before): (Vec<String>, Vec<String>) = (f.pending.clone(), p.counters.iter().map(|x| x.id.clone()).collect());
                        self.track_write(move |c| match editing {
                            Some(k) => c.update_counter(&id, &k, serde_json::json!({ "label": label, "minutes": minutes, "day": day })),
                            None => adopt(c, &id, c.add_counter(&id, &label, minutes as u64, &day)?, |p| p.counters.iter().map(|x| x.id.clone()).collect(), &before, "counter", &pending),
                        });
                        self.track.page.counter = None;
                    }
                    _ => self.track.page.form_error = Some(t.track_bad_day.to_owned()),
                }
            } else if close || modal.should_close() {
                let pending = self.track.page.counter.take().map(|f| f.pending);
                self.track_drop_pending(&id, pending);
            }
        }

        // A planned task.
        if let Some(f) = &mut self.track.page.item {
            let images = &self.track.images;
            let on_target = f.editing.as_ref().map(|e| format!("item:{e}"));
            let (mut pasted, mut file_acts) = (None::<(String, Vec<u8>)>, Vec::new());
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
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(520.0);
                            for s in Status::ALL {
                                let (on, color) = (f.status == s, status_color(s, &theme));
                                let b = egui::Button::new(egui::RichText::new(status_name(s, t)).size(12.0).color(if on { Color32::WHITE } else { theme.text })).corner_radius(8.0).min_size(Vec2::new(84.0, 28.0));
                                if ui.add(if on { b.fill(color) } else { b }).clicked() && !on {
                                    f.status = s;
                                    // Finished, in production: their date today, unless already said.
                                    let today = show_day(&track::today());
                                    if s.finished() && f.finished.trim().is_empty() {
                                        f.finished = today.clone();
                                    }
                                    if s == Status::Live && f.live.trim().is_empty() {
                                        f.live = today;
                                    }
                                    if s == Status::Live && f.env.is_none() {
                                        f.env = Some("prod".to_owned());
                                    }
                                }
                            }
                        });
                    });
                });
                // Where it is.
                field(ui, t.track_env);
                let labels: Vec<&str> = std::iter::once(t.track_env_none).chain(track::ENVS.iter().map(|e| env_name(e))).collect();
                let at = f.env.as_deref().and_then(|e| track::ENVS.iter().position(|x| *x == e)).map_or(0, |k| k + 1);
                if let Some(k) = ui.push_id("item-env", |ui| super::segmented::segmented(ui, &theme, FIELD_HEIGHT, &labels, Some(at))).inner {
                    f.env = k.checked_sub(1).map(|k| track::ENVS[k].to_owned());
                }
                // Its dates, who asked, how far it is.
                ui.horizontal_top(|ui| {
                    for (label, value) in [(t.track_requested, &mut f.requested), (t.track_due, &mut f.due), (t.track_finished, &mut f.finished), (t.track_live_on, &mut f.live)] {
                        ui.vertical(|ui| {
                            field(ui, label);
                            ui.horizontal(|ui| {
                                let day_field = ui.add(egui::TextEdit::singleline(value).desired_width(92.0).min_size(Vec2::new(0.0, FIELD_HEIGHT)).vertical_align(egui::Align::Center).hint_text("jj/mm"));
                                day_picker(ui, &day_field, value, t, &theme);
                            });
                        });
                        ui.add_space(6.0);
                    }
                });
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        field(ui, t.track_requester);
                        ui.add(egui::TextEdit::singleline(&mut f.requester).desired_width(200.0).hint_text(t.track_requester_hint));
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_progress);
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut f.progress).desired_width(48.0).hint_text("auto"));
                            ui.label(egui::RichText::new("%").size(12.0).color(theme.text_muted));
                        });
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        field(ui, t.track_comment);
                        ui.add(egui::TextEdit::singleline(&mut f.comment).desired_width(f32::INFINITY).hint_text(t.track_comment_hint));
                    });
                });
                field(ui, t.track_description);
                let edited = md_editor(ui, Id::new(("item-body-edit", f.editing.clone())), &mut f.body, 640.0, 6, t.track_item_body_hint, &theme, t, images);
                if edited.pasted.is_some() {
                    pasted = edited.pasted;
                }
                field(ui, t.track_files);
                let shown: Vec<&TrackFile> = p.files.iter().filter(|x| on_target.as_deref() == Some(x.on.as_str()) || f.pending.contains(&x.id)).collect();
                files_ui(ui, &shown, p.role.manages(), true, &theme, t, &mut file_acts);
                ui.label(egui::RichText::new(t.track_files_hint).size(11.0).color(theme.text_muted));
                let dates_ok = [&f.requested, &f.due, &f.finished, &f.live].iter().all(|d| d.trim().is_empty() || track::parse_day(d).is_some());
                let progress_ok = f.progress.trim().is_empty() || f.progress.trim().parse::<u8>().is_ok_and(|p| p <= 100);
                let ready = !f.title.trim().is_empty() && (f.hours.trim().is_empty() || track::parse_duration(&f.hours).is_some()) && dates_ok && progress_ok;
                (ok, close) = footer(ui, &error, if f.editing.is_some() { t.track_save } else { t.track_create }, ready);
            });
            // Its files: an image pasted, files picked or dropped on the window (sent to what it is,
            // or, being made, to the project until it is).
            let target = on_target.clone().unwrap_or_else(|| "project".to_owned());
            let mut sent = Vec::new();
            if let Some((file, png)) = pasted {
                sent.push(file.clone());
                self.track_pasted(&id, &target, file, png);
            }
            sent.extend(self.track_file_acts(&id, &target, file_acts));
            let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|d| d.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
            sent.extend(self.track_attach(&id, &target, dropped));
            if on_target.is_none()
                && let Some(f) = &mut self.track.page.item
            {
                f.pending.extend(sent);
            }
            if ok {
                let f = self.track.page.item.take().unwrap();
                let day = |d: &str| track::parse_day(d.trim());
                let text = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
                let input = ItemInput {
                    title: f.title.trim().to_owned(),
                    body: Some(f.body.trim().to_owned()).filter(|b| !b.is_empty()),
                    minutes: track::parse_duration(&f.hours).map(u64::from),
                    status: f.status,
                    requested: day(&f.requested),
                    due: day(&f.due),
                    finished: day(&f.finished),
                    live: day(&f.live),
                    requester: text(&f.requester),
                    progress: f.progress.trim().parse::<u8>().ok().filter(|p| *p <= 100),
                    comment: text(&f.comment),
                    env: f.env.clone(),
                };
                let id = id.clone();
                // A sub-task's quote unfolded, to see it.
                if let Some(q) = f.quote.as_ref().filter(|q| p.quotes.iter().any(|x| &x.id == *q && x.closed.is_some())) {
                    self.track.page.open_quotes.insert(q.clone());
                }
                let before: Vec<String> = p.items.iter().map(|x| x.id.clone()).collect();
                self.track_write(move |c| match f.editing {
                    Some(i) => c.update_item(&id, &i, serde_json::json!({
                        "title": input.title, "body": input.body, "minutes": input.minutes, "status": input.status.word(),
                        "requested": input.requested, "due": input.due, "finished": input.finished, "live": input.live,
                        "requester": input.requester, "progress": input.progress, "comment": input.comment, "env": input.env,
                    })),
                    None => adopt(c, &id, c.add_item(&id, f.quote.as_deref(), f.parent.as_deref(), &input)?, |p| p.items.iter().map(|x| x.id.clone()).collect(), &before, "item", &f.pending),
                });
            } else if close || modal.should_close() {
                let pending = self.track.page.item.take().map(|f| f.pending);
                self.track_drop_pending(&id, pending);
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
