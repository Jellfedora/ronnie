//! The "Suivi" category: project tracking (see `crate::track`), kept on floor with the account. The
//! sidebar lists the projects by client (the hours left on the current counter beside each); the page
//! shows them all, or one: the time spent, its tasks (logged here: a day, a duration typed, a
//! description, what it is charged to), its hour counters (a new one proposed once used up), its
//! quotes, who has access. A project can be tied to a profile of this machine (its tab opened from
//! it), and the first projects made from the profiles picked.
//!
//! Every call to floor goes in a thread; what comes back arrives by a channel, read each frame.

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::sidebar::{paint_panel_title, paint_row_bg};
use super::*;
use crate::track::{self, Billing, Client, Project, Role, Summary, TaskInput, format_minutes as hm, show_day};

/// floor asked again this often while the page is shown (a colleague, or Claude, may have written).
const REFRESH_SECS: f64 = 20.0;
/// And this often otherwise (the sidebar shows the hours left).
const IDLE_REFRESH_SECS: f64 = 300.0;

const RED: Color32 = Color32::from_rgb(0xe0, 0x55, 0x55);
const GREEN: Color32 = Color32::from_rgb(0x4c, 0xc2, 0x6a);
const ORANGE: Color32 = Color32::from_rgb(0xe0, 0xa8, 0x3c);

/// What a call to floor gave back.
enum Done {
    List(Result<Vec<Summary>, String>),
    /// A project read, or written (then shown, when `select`).
    Project {
        result: Box<Result<Project, String>>,
        select: bool,
    },
    /// A project gone (deleted, or left): its id.
    Gone(Result<String, String>),
    People(Result<Vec<String>, String>),
    /// Projects made from profiles: each one and its profile.
    Made(Vec<(Result<Project, String>, Option<Uuid>)>),
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Tasks,
    Counters,
    Quotes,
    Access,
}

/// The task being typed (or changed: `editing`).
#[derive(Default)]
struct TaskForm {
    /// The project it is for (another project shown: it starts again).
    project: String,
    editing: Option<String>,
    day: String,
    duration: String,
    label: String,
    billing: Billing,
    counter: Option<String>,
    quote: Option<String>,
}

#[derive(Default)]
struct CounterForm {
    label: String,
    hours: String,
    day: String,
}

#[derive(Default)]
struct QuoteForm {
    number: String,
    label: String,
    hours: String,
    day: String,
}

/// A new project: its name, client, color, the profile it's tied to.
struct NewProject {
    name: String,
    client: String,
    color: Option<Color32>,
    profile: Option<Uuid>,
}

/// A project's settings being changed.
struct EditProject {
    id: String,
    name: String,
    client: String,
    color: Option<Color32>,
    note: String,
    profile: Option<Uuid>,
    archived: bool,
    confirm: bool,
}

/// The profiles proposed to become projects: each one, picked or not, its client; shown by group
/// of the local profiles (a group ticked picks all its profiles, those of its sub-groups too).
struct Import {
    rows: Vec<(Uuid, bool, String, String)>,
    sections: Vec<Section>,
}

/// A group of local profiles in the take-up window (or those in none).
struct Section {
    /// Its name (None: the profiles in no group).
    name: Option<String>,
    depth: usize,
    /// Its own profiles (rows).
    own: Vec<usize>,
    /// Its profiles and those of its sub-groups: what its box ticks.
    all: Vec<usize>,
}

pub(super) struct Track {
    tx: Sender<Done>,
    rx: Receiver<Done>,
    list: Option<Result<Vec<Summary>, String>>,
    projects: HashMap<String, Project>,
    /// Calls going on.
    busy: usize,
    fetched_at: Option<f64>,
    tab: Tab,
    form: TaskForm,
    counter_form: CounterForm,
    quote_form: QuoteForm,
    share: (String, Role),
    people: Vec<String>,
    new_project: Option<NewProject>,
    import: Option<Import>,
    edit: Option<EditProject>,
    /// A task, counter or quote whose delete was clicked once (a second click deletes it).
    confirm: Option<String>,
    filter: Option<Billing>,
    show_archived: bool,
    /// The profiles proposed once already (with no project yet).
    offered: bool,
}

impl Default for Track {
    fn default() -> Self {
        let (tx, rx) = channel();
        Track {
            tx,
            rx,
            list: None,
            projects: HashMap::new(),
            busy: 0,
            fetched_at: None,
            tab: Tab::default(),
            form: TaskForm::default(),
            counter_form: CounterForm::default(),
            quote_form: QuoteForm::default(),
            share: (String::new(), Role::Editor),
            people: Vec::new(),
            new_project: None,
            import: None,
            edit: None,
            confirm: None,
            filter: None,
            show_archived: false,
            offered: false,
        }
    }
}

/// A "#rrggbb" as a color.
fn color_of(hex: Option<&str>) -> Option<Color32> {
    let hex = hex?.trim_start_matches('#');
    let v = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn hex_of(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// A role, said.
fn role_name(t: &Strings, role: Role) -> &'static str {
    match role {
        Role::Owner => t.track_role_owner,
        Role::Admin => t.track_role_admin,
        Role::Editor => t.track_role_editor,
        Role::Viewer => t.track_role_viewer,
    }
}

fn billing_name(t: &Strings, billing: Billing) -> &'static str {
    match billing {
        Billing::Counter => t.track_bill_counter,
        Billing::Quote => t.track_bill_quote,
        Billing::Free => t.track_bill_free,
        Billing::Maintenance => t.track_bill_maintenance,
    }
}

fn billing_color(theme: &crate::theme::Theme, billing: Billing) -> Color32 {
    match billing {
        Billing::Counter => theme.accent,
        Billing::Quote => theme.ansi[4],
        Billing::Free => GREEN,
        Billing::Maintenance => theme.ansi[3],
    }
}

/// Minutes left on a counter, in its color: red once used up, orange near the end.
fn left_color(theme: &crate::theme::Theme, left: i64, of: u64) -> Color32 {
    if left <= 0 {
        RED
    } else if (left as f64) < of as f64 * 0.15 {
        ORANGE
    } else {
        theme.text_muted
    }
}

impl Track {
    /// A call to floor, in a thread; its answer comes back by the channel.
    fn spawn(&mut self, ctx: &egui::Context, client: Client, call: impl FnOnce(&Client) -> Done + Send + 'static) {
        self.busy += 1;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("track".into()).spawn(move || {
            let _ = tx.send(call(&client));
            ctx.request_repaint();
        });
    }

    /// The projects listed, sorted by client then name (the archived ones only when asked).
    fn shown(&self) -> Vec<Summary> {
        match &self.list {
            Some(Ok(list)) => list.iter().filter(|p| self.show_archived || p.archived.is_none()).cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The clients already named, for the new projects.
    fn clients(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        if let Some(Ok(list)) = &self.list {
            for p in list {
                if let Some(c) = &p.client {
                    set.insert(c.clone());
                }
            }
        }
        set.into_iter().collect()
    }

    /// The task form, ready for `project` (today, its current counter).
    fn reset_form(&mut self, project: &Project) {
        self.form = TaskForm { project: project.id.clone(), day: show_day(&track::today()), ..TaskForm::default() };
        match project.current_counter() {
            Some(c) => {
                self.form.billing = Billing::Counter;
                self.form.counter = Some(c.id.clone());
            }
            None => self.form.billing = Billing::Maintenance,
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
    AddTask,
    SaveTask(String),
    EditTask(track::Task),
    CancelTask,
    DeleteTask(String),
    AddCounter,
    ProposeCounter(u64),
    CloseCounter(String, bool),
    DeleteCounter(String),
    AddQuote,
    CloseQuote(String, bool),
    DeleteQuote(String),
    Share,
    SetRole(String, Role),
    Unshare(String),
    Refresh,
    SignIn,
}

impl App {
    fn track_client(&self) -> Option<Client> {
        self.config.settings.floor.as_ref().filter(|a| a.name.is_some()).map(Client::new)
    }

    /// The list (and the projects changed since) asked of floor.
    fn track_refresh(&mut self) {
        let Some(client) = self.track_client() else {
            return;
        };
        let now = self.ctx.input(|i| i.time);
        self.track.fetched_at = Some(now);
        let ctx = self.ctx.clone();
        self.track.spawn(&ctx, client, |c| Done::List(c.projects()));
    }

    fn track_fetch(&mut self, id: &str, select: bool) {
        let Some(client) = self.track_client() else {
            return;
        };
        let (ctx, id) = (self.ctx.clone(), id.to_owned());
        self.track.spawn(&ctx, client, move |c| Done::Project { result: Box::new(c.project(&id)), select });
    }

    /// A write to floor; the project as it is then comes back.
    fn track_write(&mut self, call: impl FnOnce(&Client) -> Result<Project, String> + Send + 'static) {
        let Some(client) = self.track_client() else {
            return;
        };
        let ctx = self.ctx.clone();
        self.track.spawn(&ctx, client, move |c| Done::Project { result: Box::new(call(c)), select: false });
    }

    /// Collects what floor answered; asks again now and then.
    pub(super) fn track_poll(&mut self) {
        let t = self.t();
        while let Ok(done) = self.track.rx.try_recv() {
            self.track.busy = self.track.busy.saturating_sub(1);
            match done {
                Done::List(Ok(list)) => {
                    // The projects changed since (or never read) read again; those gone forgotten.
                    let stale: Vec<String> = list.iter().filter(|s| self.track.projects.get(&s.id).is_none_or(|p| p.updated != s.updated || p.role != s.role)).map(|s| s.id.clone()).collect();
                    self.track.projects.retain(|id, _| list.iter().any(|s| &s.id == id));
                    if self.config.settings.track_selected.as_ref().is_some_and(|id| !list.iter().any(|s| &s.id == id)) {
                        self.config.settings.track_selected = None;
                    }
                    let first = list.is_empty() && !self.track.offered && self.track_page;
                    self.track.list = Some(Ok(list));
                    for id in stale {
                        self.track_fetch(&id, false);
                    }
                    // No project yet: the profiles proposed, once.
                    if first && !self.config.profiles.is_empty() {
                        self.track.offered = true;
                        self.track_import_open();
                    }
                }
                Done::List(Err(e)) => {
                    if !matches!(self.track.list, Some(Ok(_))) {
                        self.track.list = Some(Err(e));
                    }
                }
                Done::Project { result, select } => match *result {
                    Err(e) => self.error = Some(format!("{} : {e}", t.track_failed)),
                    Ok(project) => {
                        if let Some(Ok(list)) = &mut self.track.list {
                            let summary = project.summary();
                            match list.iter_mut().find(|s| s.id == summary.id) {
                                Some(s) => *s = summary,
                                None => list.push(summary),
                            }
                            list.sort_by_key(|s| (s.client.clone().unwrap_or_default().to_lowercase(), s.name.to_lowercase()));
                        }
                        if select {
                            self.config.settings.track_selected = Some(project.id.clone());
                            self.track.tab = Tab::Tasks;
                        }
                        if self.track.form.project != project.id || select {
                            self.track.reset_form(&project);
                        }
                        self.track.projects.insert(project.id.clone(), project);
                    }
                },
                Done::Gone(Ok(id)) => {
                    self.track.projects.remove(&id);
                    if let Some(Ok(list)) = &mut self.track.list {
                        list.retain(|s| s.id != id);
                    }
                    self.config.settings.track_profiles.remove(&id);
                    if self.config.settings.track_selected.as_deref() == Some(id.as_str()) {
                        self.config.settings.track_selected = None;
                    }
                }
                Done::Gone(Err(e)) => self.error = Some(format!("{} : {e}", t.track_failed)),
                Done::People(Ok(people)) => self.track.people = people,
                Done::People(Err(_)) => {}
                Done::Made(made) => {
                    let mut errors = Vec::new();
                    for (result, profile) in made {
                        match result {
                            Ok(project) => {
                                if let Some(profile) = profile {
                                    self.config.settings.track_profiles.insert(project.id.clone(), profile);
                                }
                                if let Some(Ok(list)) = &mut self.track.list {
                                    list.push(project.summary());
                                    list.sort_by_key(|s| (s.client.clone().unwrap_or_default().to_lowercase(), s.name.to_lowercase()));
                                }
                                self.config.settings.track_selected = Some(project.id.clone());
                                self.track.reset_form(&project);
                                self.track.projects.insert(project.id.clone(), project);
                            }
                            Err(e) => errors.push(e),
                        }
                    }
                    if !errors.is_empty() {
                        self.error = Some(format!("{} : {}", t.track_failed, errors.join(" ; ")));
                    }
                }
            }
        }
        // Asked again: often while shown, now and then otherwise (the sidebar's hours left).
        if self.track_client().is_some() && !self.config.settings.hidden_categories.contains(&Category::Track) && !self.read_only {
            let now = self.ctx.input(|i| i.time);
            let every = if self.track_page { REFRESH_SECS } else { IDLE_REFRESH_SECS };
            if self.track.busy == 0 && self.track.fetched_at.is_none_or(|at| now - at > every) {
                self.track_refresh();
            }
            self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
        }
    }

    /// Debug builds (`RONNIE_OPEN_TRACK`): the page on one of its tabs.
    pub(super) fn track_demo_tab(&mut self, tab: &str) {
        self.track.tab = match tab {
            "counters" => Tab::Counters,
            "quotes" => Tab::Quotes,
            "access" => Tab::Access,
            "import" => {
                self.track_import_open();
                Tab::Tasks
            }
            _ => Tab::Tasks,
        };
    }

    /// The tracking page, in this window: shown instead of the tabs.
    pub(super) fn open_track_page(&mut self) {
        self.track_page = true;
        self.talk_page = false;
        self.modules_page = false;
        self.procs_page = false;
        self.dev_page = false;
        self.music_page = false;
        self.notes_page = false;
        self.home_game = false;
        self.track_refresh();
        if self.track.people.is_empty()
            && let Some(client) = self.track_client()
        {
            let ctx = self.ctx.clone();
            self.track.spawn(&ctx, client, |c| Done::People(c.people()));
        }
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    fn track_select(&mut self, id: Option<String>) {
        self.track.confirm = None;
        if let Some(p) = id.as_ref().and_then(|id| self.track.projects.get(id)) {
            let p = p.clone();
            self.track.reset_form(&p);
            self.track.tab = Tab::Tasks;
        }
        self.config.settings.track_selected = id;
        self.open_track_page();
    }

    /// The profiles proposed to become projects (those not tied to one already).
    fn track_import_open(&mut self) {
        let t = self.t();
        let tied: Vec<Uuid> = self.config.settings.track_profiles.values().copied().collect();
        // The client is often the profile itself: its name, to change.
        let rows: Vec<(Uuid, bool, String, String)> = self.config.profiles.iter().filter(|p| !tied.contains(&p.id)).map(|p| (p.id, false, p.name(t.untitled).to_owned(), p.name(t.untitled).to_owned())).collect();
        if rows.is_empty() {
            self.error = Some(t.track_import_none.to_owned());
            return;
        }
        // The local groups, each followed by its sub-groups; then the profiles in none.
        let groups: Vec<&crate::config::Group> = self.config.groups.iter().filter(|g| g.local).collect();
        let row_of = |id: &Uuid| rows.iter().position(|r| r.0 == *id);
        let mut sections = Vec::new();
        fn walk(groups: &[&crate::config::Group], parent: Option<Uuid>, depth: usize, row_of: &dyn Fn(&Uuid) -> Option<usize>, sections: &mut Vec<Section>) -> Vec<usize> {
            let mut under = Vec::new();
            for g in groups.iter().filter(|g| g.parent == parent) {
                let at = sections.len();
                let own: Vec<usize> = g.items.iter().filter_map(row_of).collect();
                sections.push(Section { name: Some(g.name.clone()), depth, own: own.clone(), all: Vec::new() });
                let mut all = own;
                // Guarded against a group its own ancestor (a broken config).
                if depth < 8 {
                    all.extend(walk(groups, Some(g.id), depth + 1, row_of, sections));
                }
                sections[at].all = all.clone();
                under.extend(all);
            }
            under
        }
        let grouped = walk(&groups, None, 0, &row_of, &mut sections);
        // Empty groups (all their profiles tied already) left out.
        sections.retain(|s| !s.all.is_empty());
        let rest: Vec<usize> = (0..rows.len()).filter(|k| !grouped.contains(k)).collect();
        if !rest.is_empty() {
            sections.push(Section { name: None, depth: 0, own: rest.clone(), all: rest });
        }
        self.track.import = Some(Import { rows, sections });
    }

    fn track_edit_open(&mut self) {
        let Some(p) = self.config.settings.track_selected.as_ref().and_then(|id| self.track.projects.get(id)) else {
            return;
        };
        self.track.edit = Some(EditProject {
            id: p.id.clone(),
            name: p.name.clone(),
            client: p.client.clone().unwrap_or_default(),
            color: color_of(p.color.as_deref()),
            note: p.note.clone().unwrap_or_default(),
            profile: self.config.settings.track_profiles.get(&p.id).copied(),
            archived: p.archived.is_some(),
            confirm: false,
        });
    }

    /// The "Suivi" category: the projects by client, a click shows one.
    pub(super) fn track_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        if self.panel_part.head() {
            let head = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
            paint_panel_title(&painter, head, t.cat_track, &theme);
            // "+": a new project.
            if self.track_client().is_some() {
                let plus = Rect::from_center_size(Pos2::new(head.max.x - 14.0, head.center().y), Vec2::splat(20.0));
                let resp = ui.interact(plus, ui.id().with("track-new"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.track_new);
                painter.text(plus.center(), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), if resp.hovered() { theme.text } else { theme.text_muted });
                if resp.clicked() {
                    self.track.new_project = Some(NewProject { name: String::new(), client: String::new(), color: None, profile: None });
                }
            }
            *y += SECTION_HEADER_H;
        }
        if !self.panel_part.body() {
            return;
        }
        let mut act = None;
        let row = |ui: &mut Ui, y: &mut f32, key: &str, active: bool| -> (Rect, egui::Response) {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 4.0));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("track-row", key)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            paint_row_bg(&painter, slot, active, resp.hovered(), &theme);
            (slot, resp)
        };
        if self.track_client().is_none() {
            let (slot, resp) = row(ui, y, "sign-in", false);
            painter.text(Pos2::new(slot.min.x + 12.0, slot.center().y), Align2::LEFT_CENTER, t.track_sign_in, FontId::proportional(12.5), theme.accent);
            if resp.clicked() {
                act = Some(Act::SignIn);
            }
        } else {
            let selected = self.config.settings.track_selected.clone();
            // The overview.
            let (slot, resp) = row(ui, y, "all", self.track_page && selected.is_none());
            paint_track_icon(&painter, Pos2::new(slot.min.x + 17.0, slot.center().y), theme.text_muted);
            painter.text(Pos2::new(slot.min.x + 36.0, slot.center().y), Align2::LEFT_CENTER, t.track_all, FontId::proportional(13.0), if resp.hovered() { theme.text } else { theme.text_muted });
            if resp.clicked() {
                act = Some(Act::Select(None));
            }
            let mut client: Option<Option<String>> = None;
            for s in self.track.shown() {
                if client.as_ref() != Some(&s.client) {
                    client = Some(s.client.clone());
                    *y += 4.0;
                    painter.text(
                        Pos2::new(left + 10.0, *y + 8.0),
                        Align2::LEFT_CENTER,
                        s.client.as_deref().unwrap_or(t.track_no_client).to_uppercase(),
                        FontId::proportional(10.5),
                        theme.text_muted.gamma_multiply(0.75),
                    );
                    *y += 18.0;
                }
                let active = self.track_page && selected.as_deref() == Some(s.id.as_str());
                let (slot, resp) = row(ui, y, &s.id, active);
                let dot = color_of(s.color.as_deref()).unwrap_or(theme.text_muted.gamma_multiply(0.6));
                painter.circle_filled(Pos2::new(slot.min.x + 17.0, slot.center().y), 4.5, dot);
                let name_color = if resp.hovered() || active { theme.text } else { theme.text_muted };
                // The hours left on its current counter, on the right.
                let note = self.track.projects.get(&s.id).and_then(|p| p.current_counter().map(|c| (hm(c.left()), left_color(&theme, c.left(), c.minutes))));
                let note_w = note.as_ref().map_or(0.0, |(text, color)| {
                    let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(11.0), *color);
                    let w = galley.size().x;
                    painter.galley(Pos2::new(slot.max.x - 10.0 - w, slot.center().y - galley.size().y / 2.0), galley, *color);
                    w + 8.0
                });
                let galley = painter.layout(s.name.clone(), FontId::proportional(13.0), name_color, slot.width() - 46.0 - note_w);
                let one_line = galley.rows.len() <= 1;
                if one_line {
                    painter.galley(Pos2::new(slot.min.x + 30.0, slot.center().y - galley.size().y / 2.0), galley, name_color);
                } else {
                    let clipped = painter.with_clip_rect(Rect::from_min_max(slot.min, Pos2::new(slot.max.x - 10.0 - note_w, slot.max.y)));
                    clipped.text(Pos2::new(slot.min.x + 30.0, slot.center().y), Align2::LEFT_CENTER, &s.name, FontId::proportional(13.0), name_color);
                }
                if resp.clicked() {
                    act = Some(Act::Select(Some(s.id.clone())));
                }
            }
            if matches!(self.track.list, Some(Ok(ref l)) if l.is_empty()) || self.config.profiles.iter().any(|p| !self.config.settings.track_profiles.values().any(|v| *v == p.id)) {
                *y += 6.0;
                let (slot, resp) = row(ui, y, "import", false);
                painter.text(
                    Pos2::new(slot.min.x + 12.0, slot.center().y),
                    Align2::LEFT_CENTER,
                    t.track_import_row,
                    FontId::proportional(12.0),
                    if resp.hovered() { theme.text } else { theme.text_muted.gamma_multiply(0.85) },
                );
                if resp.clicked() {
                    act = Some(Act::Import);
                }
            }
        }
        match act {
            Some(Act::SignIn) => self.track_sign_in(),
            Some(Act::Select(id)) => self.track_select(id),
            Some(Act::Import) => self.track_import_open(),
            _ => {}
        }
    }

    fn track_sign_in(&mut self) {
        self.settings_tab = SettingsTab::Account;
        self.settings_dialog = true;
    }

    /// The tracking page: every project, or the one picked.
    pub(super) fn track_page_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let margin = if rect.width() > 900.0 { 40.0 } else { 18.0 };
        let area = Rect::from_min_max(rect.min + Vec2::new(margin, 22.0), rect.max - Vec2::new(margin, 12.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let ui = &mut child;
        let mut acts = Vec::new();

        if self.track_client().is_none() {
            ui.label(egui::RichText::new(t.cat_track).size(22.0).color(theme.text));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t.track_signed_out).size(13.0).color(theme.text_muted));
            ui.add_space(10.0);
            if ui.add(egui::Button::new(egui::RichText::new(t.track_sign_in).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0))).clicked() {
                self.track_sign_in();
            }
            return;
        }

        let selected = self.config.settings.track_selected.clone().and_then(|id| self.track.projects.get(&id).cloned());
        match selected {
            Some(project) => self.track_project_ui(ui, &project, &mut acts),
            None => self.track_overview_ui(ui, &mut acts),
        }
        for act in acts {
            self.track_act(act);
        }
    }

    fn track_overview_ui(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.cat_track).size(22.0).color(theme.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(egui::RichText::new(format!("+  {}", t.track_new)).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0)))
                    .clicked()
                {
                    acts.push(Act::NewProject);
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.track_import_row).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    acts.push(Act::Import);
                }
                if self.track.busy > 0 {
                    ui.spinner();
                } else if ui.add(egui::Button::new(egui::RichText::new("⟳").size(14.0)).frame_when_inactive(false)).on_hover_text(t.track_refresh).clicked() {
                    acts.push(Act::Refresh);
                }
                let archived = self.track.list.as_ref().and_then(|l| l.as_ref().ok()).is_some_and(|l| l.iter().any(|p| p.archived.is_some()));
                if archived {
                    ui.checkbox(&mut self.track.show_archived, egui::RichText::new(t.track_show_archived).size(12.0).color(theme.text_muted));
                }
            });
        });
        ui.label(egui::RichText::new(t.track_intro).size(12.5).color(theme.text_muted.gamma_multiply(0.85)));
        ui.add_space(14.0);

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
        let shown = self.track.shown();
        if shown.is_empty() {
            ui.label(egui::RichText::new(t.track_none).size(13.0).color(theme.text_muted));
            return;
        }
        egui::ScrollArea::vertical().id_salt("track-overview").auto_shrink([false, true]).show(ui, |ui| {
            let mut client: Option<Option<String>> = None;
            for s in &shown {
                if client.as_ref() != Some(&s.client) {
                    client = Some(s.client.clone());
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(s.client.as_deref().unwrap_or(t.track_no_client)).size(15.0).strong().color(theme.text));
                    ui.add_space(4.0);
                }
                let p = self.track.projects.get(&s.id);
                let resp = Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 5.0, color_of(s.color.as_deref()).unwrap_or(theme.text_muted.gamma_multiply(0.6)));
                        ui.label(egui::RichText::new(&s.name).size(14.5).color(theme.text));
                        if s.archived.is_some() {
                            ui.label(egui::RichText::new(t.track_archived).size(11.0).color(theme.text_muted));
                        }
                        ui.label(egui::RichText::new(role_name(t, s.role)).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(c) = p.and_then(Project::current_counter) {
                                ui.label(egui::RichText::new(t.track_left.replace("{h}", &hm(c.left()))).size(12.5).color(left_color(&theme, c.left(), c.minutes)));
                                ui.label(egui::RichText::new("·").color(theme.text_muted));
                            }
                            ui.label(egui::RichText::new(t.track_spent.replace("{h}", &hm(s.spent as i64))).size(12.5).color(theme.text_muted));
                            if s.members > 1 {
                                ui.label(egui::RichText::new("·").color(theme.text_muted));
                                ui.label(egui::RichText::new(t.track_people_n.replace("{n}", &s.members.to_string())).size(12.0).color(theme.text_muted));
                            }
                        });
                    });
                });
                let resp = ui.interact(resp.response.rect, ui.id().with(("track-card", &s.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    acts.push(Act::Select(Some(s.id.clone())));
                }
                ui.add_space(6.0);
            }
        });
    }

    fn track_project_ui(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        let profile = self.config.settings.track_profiles.get(&p.id).copied().filter(|id| self.config.profiles.iter().any(|x| x.id == *id));

        // The head: back to all, the name, the client, the role; the tab, the settings.
        if ui.add(egui::Button::new(egui::RichText::new(format!("←  {}", t.track_all)).size(12.0).color(theme.text_muted)).frame_when_inactive(false)).clicked() {
            acts.push(Act::Select(None));
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let (bar, _) = ui.allocate_exact_size(Vec2::new(5.0, 30.0), Sense::hover());
            ui.painter().rect_filled(bar, 2.5, color_of(p.color.as_deref()).unwrap_or(theme.accent));
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&p.name).size(22.0).color(theme.text));
                let mut line = p.client.clone().unwrap_or_else(|| t.track_no_client.to_owned());
                line.push_str(&format!("  ·  {}", role_name(t, p.role)));
                if p.archived.is_some() {
                    line.push_str(&format!("  ·  {}", t.track_archived));
                }
                ui.label(egui::RichText::new(line).size(12.0).color(theme.text_muted));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(format!("⚙  {}", t.track_settings)).size(12.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    acts.push(Act::Edit);
                }
                if let Some(profile) = profile
                    && ui.add(egui::Button::new(egui::RichText::new(t.track_open_tab).size(12.5)).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))).clicked()
                {
                    acts.push(Act::OpenProfile(profile));
                }
                if self.track.busy > 0 {
                    ui.spinner();
                }
            });
        });
        if let Some(note) = &p.note {
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new(note).size(12.5).color(theme.text_muted)).wrap());
        }
        ui.add_space(12.0);

        // The tiles: the time spent, by what it is charged to.
        let total: i64 = p.tasks.iter().map(|x| x.minutes as i64).sum();
        let current = p.current_counter();
        let mut tiles: Vec<(&str, String, String, Color32)> = vec![(t.track_total, hm(total), t.track_tasks_n.replace("{n}", &p.tasks.len().to_string()), theme.text)];
        tiles.push(match current {
            Some(c) => (t.track_current_counter, hm(c.left()), format!("{} · {}", c.label, t.track_of.replace("{h}", &hm(c.minutes as i64))), if c.left() <= 0 { RED } else { theme.text }),
            None => (t.track_current_counter, "—".into(), t.track_no_counter.into(), theme.text_muted),
        });
        for b in Billing::ALL {
            tiles.push((billing_name(t, b), hm(p.by_billing(b) as i64), String::new(), billing_color(&theme, b)));
        }
        // As many a row as fit, all as wide (a frame can't be wrapped once drawn).
        let gap = 8.0;
        let avail = ui.available_width();
        let fit = (((avail + gap) / (120.0 + gap)).floor() as usize).clamp(2, tiles.len());
        // Rows as full as each other: 3 and 3 rather than 5 and 1.
        let per_row = tiles.len().div_ceil(tiles.len().div_ceil(fit));
        let w = (avail - gap * (per_row - 1) as f32) / per_row as f32;
        for row in tiles.chunks(per_row) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (title, value, sub, color) in row {
                    ui.allocate_ui_with_layout(Vec2::new(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                            ui.set_width(w - 30.0);
                            ui.label(egui::RichText::new(*title).size(11.0).color(theme.text_muted));
                            ui.label(egui::RichText::new(value).size(19.0).color(*color));
                            // Always a third line: the tiles of a row as tall.
                            ui.add(egui::Label::new(egui::RichText::new(if sub.is_empty() { " " } else { sub.as_str() }).size(11.0).color(theme.text_muted.gamma_multiply(0.8))).truncate());
                        });
                    });
                }
            });
            ui.add_space(gap);
        }

        // A counter used up: a new one proposed (same hours as the last).
        if p.role.manages() && p.counters_spent() {
            let last = p.counters.iter().rev().find(|c| c.closed.is_none()).or(p.counters.last());
            if let Some(last) = last {
                ui.add_space(10.0);
                Frame::NONE.fill(RED.gamma_multiply(0.14)).stroke(Stroke::new(1.0, RED.gamma_multiply(0.5))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let text = t.track_spent_up.replace("{name}", &last.label).replace("{h}", &hm(last.left()));
                        ui.label(egui::RichText::new(text).size(12.5).color(theme.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new(t.track_new_counter_of.replace("{h}", &hm(last.minutes as i64))).size(12.5).color(theme.bg))
                                        .fill(theme.accent)
                                        .corner_radius(6.0),
                                )
                                .clicked()
                            {
                                acts.push(Act::ProposeCounter(last.minutes));
                            }
                        });
                    });
                });
            }
        }
        ui.add_space(14.0);

        // The tabs of the page.
        ui.horizontal(|ui| {
            for (tab, name, n) in [
                (Tab::Tasks, t.track_tab_tasks, p.tasks.len()),
                (Tab::Counters, t.track_tab_counters, p.counters.len()),
                (Tab::Quotes, t.track_tab_quotes, p.quotes.len()),
                (Tab::Access, t.track_tab_access, p.members.len()),
            ] {
                let on = self.track.tab == tab;
                let text = egui::RichText::new(format!("{name}  {n}")).size(13.0).color(if on { theme.bg } else { theme.text_muted });
                let b = egui::Button::new(text).corner_radius(14.0).min_size(Vec2::new(0.0, 26.0));
                if ui.add(if on { b.fill(theme.accent) } else { b.frame_when_inactive(false) }).clicked() {
                    self.track.tab = tab;
                    self.track.confirm = None;
                }
            }
        });
        ui.add_space(10.0);

        egui::ScrollArea::vertical().id_salt(("track-project", &p.id)).auto_shrink([false, true]).show(ui, |ui| match self.track.tab {
            Tab::Tasks => self.track_tasks_ui(ui, p, acts),
            Tab::Counters => self.track_counters_ui(ui, p, acts),
            Tab::Quotes => self.track_quotes_ui(ui, p, acts),
            Tab::Access => self.track_access_ui(ui, p, acts),
        });
    }

    /// What a task is charged to, said (the counter's or quote's name).
    fn track_charged(&self, p: &Project, task: &track::Task) -> String {
        let t = self.t();
        match task.billing {
            Billing::Counter => task.counter.as_ref().and_then(|c| p.counters.iter().find(|x| &x.id == c)).map_or(t.track_bill_counter.to_owned(), |c| c.label.clone()),
            Billing::Quote => task.quote.as_ref().and_then(|q| p.quotes.iter().find(|x| &x.id == q)).map_or(t.track_bill_quote.to_owned(), |q| format!("{} {}", t.track_bill_quote, q.title())),
            b => billing_name(t, b).to_owned(),
        }
    }

    fn track_tasks_ui(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        if self.track.form.project != p.id {
            self.track.reset_form(p);
        }
        // The form: a task to log (or to change).
        if p.role.writes() {
            let editing = self.track.form.editing.clone();
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, if editing.is_some() { theme.accent } else { theme.tab_hover })).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let f = &mut self.track.form;
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(t.track_day).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(86.0).hint_text("18/11"));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(t.track_duration).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.duration).desired_width(64.0).hint_text("1h30"));
                    if let Some(m) = track::parse_duration(&f.duration) {
                        ui.label(egui::RichText::new(hm(m as i64)).size(11.5).color(theme.text_muted.gamma_multiply(0.8)));
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(t.track_charge).size(12.0).color(theme.text_muted));
                    // What it is charged to: a counter (open ones, or the one it already had), a quote, offered, maintenance.
                    let said = match f.billing {
                        Billing::Counter => {
                            f.counter.as_ref().and_then(|c| p.counters.iter().find(|x| &x.id == c)).map_or(t.track_bill_counter.to_owned(), |c| format!("{} · {}", t.track_bill_counter, c.label))
                        }
                        Billing::Quote => {
                            f.quote.as_ref().and_then(|q| p.quotes.iter().find(|x| &x.id == q)).map_or(t.track_bill_quote.to_owned(), |q| format!("{} · {}", t.track_bill_quote, q.title()))
                        }
                        b => billing_name(t, b).to_owned(),
                    };
                    egui::ComboBox::from_id_salt("track-billing").selected_text(said).width(240.0).show_ui(ui, |ui| {
                        let (had_counter, had_quote) = (f.counter.clone(), f.quote.clone());
                        for c in p.counters.iter().filter(|c| c.closed.is_none() || had_counter.as_ref() == Some(&c.id)) {
                            let on = f.billing == Billing::Counter && f.counter.as_ref() == Some(&c.id);
                            if ui.selectable_label(on, format!("{} · {}  ({})", t.track_bill_counter, c.label, t.track_left.replace("{h}", &hm(c.left())))).clicked() {
                                f.billing = Billing::Counter;
                                f.counter = Some(c.id.clone());
                            }
                        }
                        for q in p.quotes.iter().filter(|q| q.closed.is_none() || had_quote.as_ref() == Some(&q.id)) {
                            let on = f.billing == Billing::Quote && f.quote.as_ref() == Some(&q.id);
                            if ui.selectable_label(on, format!("{} · {}", t.track_bill_quote, q.title())).clicked() {
                                f.billing = Billing::Quote;
                                f.quote = Some(q.id.clone());
                            }
                        }
                        for b in [Billing::Free, Billing::Maintenance] {
                            if ui.selectable_label(f.billing == b, billing_name(t, b)).clicked() {
                                f.billing = b;
                            }
                        }
                    });
                });
                ui.add_space(6.0);
                ui.add(egui::TextEdit::multiline(&mut f.label).desired_rows(2).desired_width(f32::INFINITY).hint_text(t.track_label_hint));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ready = track::parse_duration(&f.duration).is_some() && !f.label.trim().is_empty();
                    let label = if editing.is_some() { t.track_save } else { t.track_add_task };
                    if ui.add_enabled(ready, egui::Button::new(egui::RichText::new(label).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))).clicked() {
                        acts.push(match &editing {
                            Some(id) => Act::SaveTask(id.clone()),
                            None => Act::AddTask,
                        });
                    }
                    if editing.is_some() && ui.add(egui::Button::new(egui::RichText::new(t.track_cancel).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0))).clicked() {
                        acts.push(Act::CancelTask);
                    }
                    ui.label(egui::RichText::new(t.track_duration_hint).size(11.0).color(theme.text_muted.gamma_multiply(0.75)));
                });
            });
            ui.add_space(12.0);
        }

        // The filter, the total it gives.
        let shown: Vec<&track::Task> = p.tasks.iter().filter(|x| self.track.filter.is_none_or(|b| x.billing == b)).collect();
        ui.horizontal(|ui| {
            let said = self.track.filter.map_or(t.track_filter_all, |b| billing_name(t, b));
            egui::ComboBox::from_id_salt("track-filter").selected_text(said).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.track.filter, None, t.track_filter_all);
                for b in Billing::ALL {
                    ui.selectable_value(&mut self.track.filter, Some(b), billing_name(t, b));
                }
            });
            let sum: i64 = shown.iter().map(|x| x.minutes as i64).sum();
            ui.label(egui::RichText::new(t.track_filter_total.replace("{n}", &shown.len().to_string()).replace("{h}", &hm(sum))).size(12.0).color(theme.text_muted));
        });
        ui.add_space(6.0);
        if shown.is_empty() {
            ui.label(egui::RichText::new(t.track_no_tasks).size(12.5).color(theme.text_muted));
            return;
        }
        // The tasks, newest first.
        let width = ui.available_width();
        for task in shown {
            let can = p.role.manages() || (p.role.writes() && task.mine);
            let editing = self.track.form.editing.as_deref() == Some(task.id.as_str());
            Frame::NONE.fill(if editing { theme.tab_hover } else { Color32::TRANSPARENT }).corner_radius(6.0).inner_margin(egui::Margin::symmetric(6, 5)).show(ui, |ui| {
                ui.set_width(width - 12.0);
                ui.horizontal_top(|ui| {
                    ui.add_sized(Vec2::new(78.0, 18.0), egui::Label::new(egui::RichText::new(show_day(&task.day)).size(12.0).color(theme.text_muted)));
                    ui.add_sized(Vec2::new(62.0, 18.0), egui::Label::new(egui::RichText::new(hm(task.minutes as i64)).size(12.5).strong().color(theme.text)));
                    let charge = self.track_charged(p, task);
                    let color = billing_color(&theme, task.billing);
                    let chip = egui::RichText::new(charge).size(11.0).color(color);
                    ui.allocate_ui(Vec2::new(150.0, 18.0), |ui| {
                        ui.set_width(150.0);
                        Frame::NONE.fill(color.gamma_multiply(0.15)).corner_radius(9.0).inner_margin(egui::Margin::symmetric(7, 1)).show(ui, |ui| {
                            ui.add(egui::Label::new(chip).truncate());
                        });
                    });
                    let actions_w = if can { 56.0 } else { 0.0 };
                    let author_w = 80.0;
                    let label_w = (ui.available_width() - actions_w - author_w - 8.0).max(80.0);
                    ui.allocate_ui(Vec2::new(label_w, 18.0), |ui| {
                        ui.set_width(label_w);
                        ui.add(egui::Label::new(egui::RichText::new(&task.label).size(12.5).color(theme.text)).wrap());
                    });
                    ui.add_sized(
                        Vec2::new(author_w, 18.0),
                        egui::Label::new(egui::RichText::new(task.author.as_deref().unwrap_or("?")).size(11.5).color(theme.text_muted.gamma_multiply(0.8))).truncate(),
                    );
                    if can {
                        if ui.add(egui::Button::new(egui::RichText::new("✎").size(12.5).color(theme.text_muted)).frame_when_inactive(false)).on_hover_text(t.track_edit).clicked() {
                            acts.push(Act::EditTask(task.clone()));
                        }
                        let confirming = self.track.confirm.as_deref() == Some(task.id.as_str());
                        let x = egui::RichText::new(if confirming { "✕ ?" } else { "✕" }).size(12.5).color(if confirming { RED } else { theme.text_muted });
                        if ui.add(egui::Button::new(x).frame_when_inactive(false)).on_hover_text(t.track_delete).clicked() {
                            if confirming {
                                acts.push(Act::DeleteTask(task.id.clone()));
                            } else {
                                self.track.confirm = Some(task.id.clone());
                            }
                        }
                    }
                });
            });
        }
    }

    fn track_counters_ui(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.label(egui::RichText::new(t.track_counters_intro).size(12.5).color(theme.text_muted));
        ui.add_space(8.0);
        if p.role.manages() {
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let f = &mut self.track.counter_form;
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(t.track_hours).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.hours).desired_width(60.0).hint_text("10"));
                    ui.label(egui::RichText::new(t.track_counter_label).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.label).desired_width(200.0).hint_text(t.track_counter_hint));
                    ui.label(egui::RichText::new(t.track_since).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(86.0).hint_text(t.track_today));
                    let ready = track::parse_duration(&f.hours).is_some();
                    if ui.add_enabled(ready, egui::Button::new(egui::RichText::new(t.track_add_counter).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0)).clicked() {
                        acts.push(Act::AddCounter);
                    }
                });
            });
            ui.add_space(10.0);
        }
        if p.counters.is_empty() {
            ui.label(egui::RichText::new(t.track_no_counter).size(12.5).color(theme.text_muted));
        }
        for c in p.counters.iter().rev() {
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&c.label).size(14.0).color(if c.closed.is_some() { theme.text_muted } else { theme.text }));
                    ui.label(egui::RichText::new(t.track_since_day.replace("{d}", &show_day(&c.day))).size(11.5).color(theme.text_muted));
                    if c.closed.is_some() {
                        ui.label(egui::RichText::new(t.track_closed).size(11.5).color(theme.text_muted));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if p.role.manages() {
                            let confirming = self.track.confirm.as_deref() == Some(c.id.as_str());
                            let x = egui::RichText::new(if confirming { t.track_confirm_delete } else { t.track_delete }).size(11.5).color(if confirming { RED } else { theme.text_muted });
                            if ui.add(egui::Button::new(x).frame_when_inactive(false)).clicked() {
                                if confirming {
                                    acts.push(Act::DeleteCounter(c.id.clone()));
                                } else {
                                    self.track.confirm = Some(c.id.clone());
                                }
                            }
                            let close = if c.closed.is_some() { t.track_reopen } else { t.track_close };
                            if ui.add(egui::Button::new(egui::RichText::new(close).size(11.5)).corner_radius(5.0)).clicked() {
                                acts.push(Act::CloseCounter(c.id.clone(), c.closed.is_none()));
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                let fraction = if c.minutes > 0 { c.used as f32 / c.minutes as f32 } else { 1.0 };
                let color = if c.left() <= 0 {
                    RED
                } else if fraction > 0.85 {
                    ORANGE
                } else {
                    theme.accent
                };
                ui.add(egui::ProgressBar::new(fraction.min(1.0)).fill(color).desired_height(8.0));
                ui.add_space(2.0);
                let said = t.track_counter_line.replace("{used}", &hm(c.used as i64)).replace("{of}", &hm(c.minutes as i64)).replace("{left}", &hm(c.left()));
                ui.label(egui::RichText::new(said).size(12.0).color(left_color(&theme, c.left(), c.minutes)));
            });
            ui.add_space(8.0);
        }
    }

    fn track_quotes_ui(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        if p.role.manages() {
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let f = &mut self.track.quote_form;
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(t.track_quote_number).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.number).desired_width(90.0).hint_text("D-2026-12"));
                    ui.label(egui::RichText::new(t.track_quote_label).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.label).desired_width(200.0));
                    ui.label(egui::RichText::new(t.track_planned).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.hours).desired_width(60.0).hint_text("5"));
                    ui.label(egui::RichText::new(t.track_day).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut f.day).desired_width(86.0).hint_text(t.track_today));
                    let ready = !f.label.trim().is_empty() && (f.hours.trim().is_empty() || track::parse_duration(&f.hours).is_some());
                    if ui.add_enabled(ready, egui::Button::new(egui::RichText::new(t.track_add_quote).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0)).clicked() {
                        acts.push(Act::AddQuote);
                    }
                });
            });
            ui.add_space(10.0);
        }
        if p.quotes.is_empty() {
            ui.label(egui::RichText::new(t.track_no_quotes).size(12.5).color(theme.text_muted));
        }
        for q in p.quotes.iter().rev() {
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(q.title()).size(14.0).color(if q.closed.is_some() { theme.text_muted } else { theme.text }));
                    ui.label(egui::RichText::new(show_day(&q.day)).size(11.5).color(theme.text_muted));
                    if q.closed.is_some() {
                        ui.label(egui::RichText::new(t.track_closed).size(11.5).color(theme.text_muted));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if p.role.manages() {
                            let confirming = self.track.confirm.as_deref() == Some(q.id.as_str());
                            let x = egui::RichText::new(if confirming { t.track_confirm_delete } else { t.track_delete }).size(11.5).color(if confirming { RED } else { theme.text_muted });
                            if ui.add(egui::Button::new(x).frame_when_inactive(false)).clicked() {
                                if confirming {
                                    acts.push(Act::DeleteQuote(q.id.clone()));
                                } else {
                                    self.track.confirm = Some(q.id.clone());
                                }
                            }
                            let close = if q.closed.is_some() { t.track_reopen } else { t.track_close };
                            if ui.add(egui::Button::new(egui::RichText::new(close).size(11.5)).corner_radius(5.0)).clicked() {
                                acts.push(Act::CloseQuote(q.id.clone(), q.closed.is_none()));
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                match q.minutes {
                    Some(planned) => {
                        let fraction = q.used as f32 / planned.max(1) as f32;
                        let over = q.used > planned;
                        ui.add(egui::ProgressBar::new(fraction.min(1.0)).fill(if over { RED } else { theme.ansi[4] }).desired_height(8.0));
                        ui.add_space(2.0);
                        let said = t.track_quote_line.replace("{used}", &hm(q.used as i64)).replace("{of}", &hm(planned as i64));
                        ui.label(egui::RichText::new(said).size(12.0).color(if over { RED } else { theme.text_muted }));
                    }
                    None => {
                        ui.label(egui::RichText::new(t.track_spent.replace("{h}", &hm(q.used as i64))).size(12.0).color(theme.text_muted));
                    }
                }
            });
            ui.add_space(8.0);
        }
    }

    fn track_access_ui(&mut self, ui: &mut Ui, p: &Project, acts: &mut Vec<Act>) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.label(egui::RichText::new(t.track_access_intro).size(12.5).color(theme.text_muted));
        ui.add_space(8.0);
        for m in &p.members {
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let name = if m.me { format!("{}  ({})", m.name, t.track_you) } else { m.name.clone() };
                    ui.label(egui::RichText::new(name).size(13.5).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let changeable = p.role.manages() && m.role != Role::Owner;
                        if changeable || (m.me && m.role != Role::Owner) {
                            let confirming = self.track.confirm.as_deref() == Some(m.id.as_str());
                            let word = if m.me { t.track_leave } else { t.track_remove };
                            let x = egui::RichText::new(if confirming { t.track_confirm_delete } else { word }).size(11.5).color(if confirming { RED } else { theme.text_muted });
                            if ui.add(egui::Button::new(x).frame_when_inactive(false)).clicked() {
                                if confirming {
                                    acts.push(Act::Unshare(m.id.clone()));
                                } else {
                                    self.track.confirm = Some(m.id.clone());
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
                            ui.label(egui::RichText::new(role_name(t, m.role)).size(12.0).color(theme.text_muted));
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
        if !p.role.manages() {
            return;
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t.track_share_title).size(14.0).color(theme.text));
        ui.add_space(4.0);
        let candidates: Vec<String> = self.track.people.iter().filter(|n| !p.members.iter().any(|m| m.name.eq_ignore_ascii_case(n))).cloned().collect();
        ui.horizontal_wrapped(|ui| {
            let (name, role) = &mut self.track.share;
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
            if ui.add_enabled(!name.is_empty(), egui::Button::new(egui::RichText::new(t.track_share).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0)).clicked() {
                acts.push(Act::Share);
            }
        });
        ui.add_space(6.0);
        for r in Role::GIVEN {
            let what = match r {
                Role::Admin => t.track_role_admin_hint,
                Role::Editor => t.track_role_editor_hint,
                _ => t.track_role_viewer_hint,
            };
            ui.label(egui::RichText::new(format!("{} — {what}", role_name(t, r))).size(11.5).color(theme.text_muted.gamma_multiply(0.85)));
        }
    }

    /// Does what was clicked on the page.
    fn track_act(&mut self, act: Act) {
        let t = self.t();
        let id = self.config.settings.track_selected.clone().unwrap_or_default();
        let day = |text: &str| track::parse_day(text);
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
            Act::SignIn => self.track_sign_in(),
            Act::AddTask | Act::SaveTask(_) => {
                let f = &self.track.form;
                let Some(d) = day(&f.day) else {
                    self.error = Some(t.track_bad_day.to_owned());
                    return;
                };
                let Some(minutes) = track::parse_duration(&f.duration).filter(|m| *m <= track::TASK_MAX) else {
                    self.error = Some(t.track_bad_duration.to_owned());
                    return;
                };
                let input = TaskInput {
                    day: d,
                    label: f.label.trim().to_owned(),
                    minutes,
                    billing: f.billing,
                    counter: (f.billing == Billing::Counter).then(|| f.counter.clone()).flatten(),
                    quote: (f.billing == Billing::Quote).then(|| f.quote.clone()).flatten(),
                };
                if (input.billing == Billing::Counter && input.counter.is_none()) || (input.billing == Billing::Quote && input.quote.is_none()) {
                    self.error = Some(t.track_pick_charge.to_owned());
                    return;
                }
                let task = if let Act::SaveTask(task) = act { Some(task) } else { None };
                self.track_write(move |c| match task {
                    Some(task) => c.update_task(&id, &task, &input),
                    None => c.add_task(&id, &input),
                });
                // The form ready for the next one: same day and charge.
                let f = &mut self.track.form;
                f.editing = None;
                f.duration.clear();
                f.label.clear();
            }
            Act::EditTask(task) => {
                let f = &mut self.track.form;
                f.editing = Some(task.id.clone());
                f.day = show_day(&task.day);
                f.duration = hm(task.minutes as i64).replace(' ', "");
                f.label = task.label.clone();
                f.billing = task.billing;
                f.counter = task.counter.clone();
                f.quote = task.quote.clone();
                self.track.tab = Tab::Tasks;
            }
            Act::CancelTask => {
                if let Some(p) = self.track.projects.get(&id).cloned() {
                    self.track.reset_form(&p);
                }
            }
            Act::DeleteTask(task) => {
                self.track.confirm = None;
                self.track_write(move |c| c.delete_task(&id, &task));
            }
            Act::ProposeCounter(minutes) => {
                self.track.tab = Tab::Counters;
                self.track.counter_form = CounterForm { label: String::new(), hours: hm(minutes as i64).replace(' ', ""), day: String::new() };
            }
            Act::AddCounter => {
                let f = &self.track.counter_form;
                let Some(minutes) = track::parse_duration(&f.hours) else {
                    return;
                };
                let Some(d) = day(&f.day) else {
                    self.error = Some(t.track_bad_day.to_owned());
                    return;
                };
                let label = if f.label.trim().is_empty() { t.track_counter_default.replace("{h}", &hm(minutes as i64)).replace("{d}", &show_day(&d)) } else { f.label.trim().to_owned() };
                self.track.counter_form = CounterForm::default();
                // The tasks to come go to it.
                self.track.form.project.clear();
                self.track_write(move |c| c.add_counter(&id, &label, minutes as u64, &d));
            }
            Act::CloseCounter(counter, closed) => {
                self.track.form.project.clear();
                self.track_write(move |c| c.update_counter(&id, &counter, serde_json::json!({ "closed": closed })));
            }
            Act::DeleteCounter(counter) => {
                self.track.confirm = None;
                self.track.form.project.clear();
                self.track_write(move |c| c.delete_counter(&id, &counter));
            }
            Act::AddQuote => {
                let f = &self.track.quote_form;
                let Some(d) = day(&f.day) else {
                    self.error = Some(t.track_bad_day.to_owned());
                    return;
                };
                let minutes = track::parse_duration(&f.hours).map(u64::from);
                let (number, label) = (Some(f.number.trim().to_owned()).filter(|n| !n.is_empty()), f.label.trim().to_owned());
                self.track.quote_form = QuoteForm::default();
                self.track_write(move |c| c.add_quote(&id, number.as_deref(), &label, minutes, &d));
            }
            Act::CloseQuote(quote, closed) => self.track_write(move |c| c.update_quote(&id, &quote, serde_json::json!({ "closed": closed }))),
            Act::DeleteQuote(quote) => {
                self.track.confirm = None;
                self.track_write(move |c| c.delete_quote(&id, &quote));
            }
            Act::Share => {
                let (name, role) = std::mem::replace(&mut self.track.share, (String::new(), Role::Editor));
                self.track_write(move |c| c.share(&id, &name, role));
            }
            Act::SetRole(name, role) => self.track_write(move |c| c.share(&id, &name, role)),
            Act::Unshare(player) => {
                self.track.confirm = None;
                let Some(client) = self.track_client() else {
                    return;
                };
                let ctx = self.ctx.clone();
                self.track.spawn(&ctx, client, move |c| match c.unshare(&id, &player) {
                    Ok(Some(p)) => Done::Project { result: Box::new(Ok(p)), select: false },
                    Ok(None) => Done::Gone(Ok(id)),
                    Err(e) => Done::Gone(Err(e)),
                });
            }
        }
    }

    /// The windows of the page: a new project, the profiles to take up, a project's settings.
    pub(super) fn track_windows(&mut self, ctx: &egui::Context) {
        if self.track.new_project.is_none() && self.track.import.is_none() && self.track.edit.is_none() {
            return;
        }
        let t = self.t();
        let theme = self.theme.clone();
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let button = |text: &str, fill: Option<Color32>| {
            let b = egui::Button::new(egui::RichText::new(text).size(13.0).color(if fill.is_some() { theme.bg } else { theme.text })).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0));
            match fill {
                Some(f) => b.fill(f),
                None => b,
            }
        };
        let profiles: Vec<(Uuid, String, Option<Color32>)> = self.config.profiles.iter().map(|p| (p.id, p.name(t.untitled).to_owned(), p.tab.color)).collect();
        let clients = self.track.clients();
        let profile_combo = |ui: &mut Ui, salt: &str, profile: &mut Option<Uuid>| {
            let said = profile.and_then(|id| profiles.iter().find(|p| p.0 == id)).map_or(t.track_no_profile.to_owned(), |p| p.1.clone());
            egui::ComboBox::from_id_salt(salt).selected_text(said).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(profile, None, t.track_no_profile);
                for (id, name, _) in &profiles {
                    ui.selectable_value(profile, Some(*id), name);
                }
            });
        };
        let color_row = |ui: &mut Ui, color: &mut Option<Color32>| {
            ui.horizontal_wrapped(|ui| {
                for c in TAB_COLORS {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 8.0, c);
                    if *color == Some(c) {
                        ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(2.0, theme.text));
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        *color = if *color == Some(c) { None } else { Some(c) };
                    }
                }
            });
        };
        let client_chips = |ui: &mut Ui, client: &mut String| {
            if clients.is_empty() {
                return;
            }
            ui.horizontal_wrapped(|ui| {
                for c in &clients {
                    if ui.add(egui::Button::new(egui::RichText::new(c).size(11.5)).corner_radius(10.0).selected(client == c)).clicked() {
                        *client = c.clone();
                    }
                }
            });
        };

        // A new project.
        let mut create = None;
        if let Some(n) = &mut self.track.new_project {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("track-new")).frame(frame).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(egui::RichText::new(t.track_new).size(19.0).strong().color(theme.text));
                ui.add_space(10.0);
                ui.label(egui::RichText::new(t.track_client).size(12.0).color(theme.text_muted));
                ui.add(egui::TextEdit::singleline(&mut n.client).desired_width(f32::INFINITY).hint_text(t.track_client_hint));
                client_chips(ui, &mut n.client);
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.track_project_name).size(12.0).color(theme.text_muted));
                ui.add(egui::TextEdit::singleline(&mut n.name).desired_width(f32::INFINITY));
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.track_profile).size(12.0).color(theme.text_muted));
                let before = n.profile;
                profile_combo(ui, "track-new-profile", &mut n.profile);
                // A profile picked: its name and color, unless typed already.
                if n.profile != before
                    && let Some((_, name, color)) = n.profile.and_then(|id| profiles.iter().find(|p| p.0 == id))
                {
                    if n.name.trim().is_empty() {
                        n.name = name.clone();
                    }
                    if n.color.is_none() {
                        n.color = *color;
                    }
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.track_color).size(12.0).color(theme.text_muted));
                color_row(ui, &mut n.color);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!n.name.trim().is_empty(), button(t.track_create, Some(theme.accent))).clicked() {
                        create = Some((n.name.trim().to_owned(), Some(n.client.trim().to_owned()).filter(|c| !c.is_empty()), n.color.map(hex_of), n.profile));
                    }
                    if ui.add(button(t.track_cancel, None)).clicked() {
                        close = true;
                    }
                });
            });
            if close || modal.should_close() || create.is_some() {
                self.track.new_project = None;
            }
        }
        if let Some((name, client, color, profile)) = create
            && let Some(c) = self.track_client()
        {
            let ctx = self.ctx.clone();
            self.track.spawn(&ctx, c, move |c| Done::Made(vec![(c.create(&name, client.as_deref(), color.as_deref()), profile)]));
        }

        // Profiles taken up as projects.
        let mut made = None;
        if let Some(import) = &mut self.track.import {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("track-import")).frame(frame).show(ctx, |ui| {
                ui.set_width(560.0);
                ui.label(egui::RichText::new(t.track_import_title).size(19.0).strong().color(theme.text));
                ui.add_space(6.0);
                ui.add(egui::Label::new(egui::RichText::new(t.track_import_body).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(10.0);
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    let Import { rows, sections } = import;
                    for (s, section) in sections.iter().enumerate() {
                        let indent = section.depth as f32 * 18.0;
                        // The group's box: all its profiles (sub-groups too), ticked or not.
                        let picked = section.all.iter().filter(|k| rows[**k].1).count();
                        let mut all = picked == section.all.len();
                        ui.add_space(if s == 0 { 0.0 } else { 6.0 });
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let name = section.name.as_deref().unwrap_or(t.track_import_ungrouped);
                            let label = egui::RichText::new(format!("{name}  ({})", section.all.len())).size(13.5).strong().color(theme.text);
                            if ui.add(egui::Checkbox::new(&mut all, label).indeterminate(picked > 0 && picked < section.all.len())).changed() {
                                for k in &section.all {
                                    rows[*k].1 = all;
                                }
                            }
                        });
                        for &k in &section.own {
                            let (_, on, name, client) = &mut rows[k];
                            ui.horizontal(|ui| {
                                ui.add_space(indent + 22.0);
                                ui.checkbox(on, egui::RichText::new(name.as_str()).size(13.0).color(theme.text));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_enabled(*on, egui::TextEdit::singleline(client).id_salt(("track-import-client", k)).desired_width(170.0).hint_text(t.track_client));
                                });
                            });
                        }
                    }
                });
                ui.add_space(12.0);
                let n = import.rows.iter().filter(|r| r.1).count();
                ui.horizontal(|ui| {
                    if ui.add_enabled(n > 0, button(&t.track_import_go.replace("{n}", &n.to_string()), Some(theme.accent))).clicked() {
                        made = Some(import.rows.iter().filter(|r| r.1).map(|r| (r.0, r.2.clone(), r.3.trim().to_owned())).collect::<Vec<_>>());
                    }
                    if ui.add(button(t.track_cancel, None)).clicked() {
                        close = true;
                    }
                });
            });
            if close || modal.should_close() || made.is_some() {
                self.track.import = None;
            }
        }
        if let Some(rows) = made
            && let Some(c) = self.track_client()
        {
            let rows: Vec<(Uuid, String, String, Option<String>)> = rows
                .into_iter()
                .map(|(id, name, client)| {
                    let color = profiles.iter().find(|p| p.0 == id).and_then(|p| p.2).map(hex_of);
                    (id, name, client, color)
                })
                .collect();
            let ctx = self.ctx.clone();
            self.track.spawn(&ctx, c, move |c| {
                Done::Made(
                    rows.into_iter()
                        .map(|(id, name, client, color)| {
                            let client = Some(client).filter(|c| !c.is_empty());
                            (c.create(&name, client.as_deref(), color.as_deref()), Some(id))
                        })
                        .collect(),
                )
            });
        }

        // A project's settings.
        enum EditDone {
            Save(String, serde_json::Value, Option<Uuid>),
            Delete(String),
            Link(String, Option<Uuid>),
        }
        let mut done = None;
        if let Some(e) = &mut self.track.edit {
            let role = self.track.projects.get(&e.id).map_or(Role::Viewer, |p| p.role);
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("track-edit")).frame(frame).show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(egui::RichText::new(t.track_settings).size(19.0).strong().color(theme.text));
                ui.add_space(10.0);
                if role.manages() {
                    ui.label(egui::RichText::new(t.track_client).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut e.client).desired_width(f32::INFINITY));
                    client_chips(ui, &mut e.client);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.track_project_name).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(f32::INFINITY));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.track_note).size(12.0).color(theme.text_muted));
                    ui.add(egui::TextEdit::multiline(&mut e.note).desired_rows(3).desired_width(f32::INFINITY).hint_text(t.track_note_hint));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.track_color).size(12.0).color(theme.text_muted));
                    color_row(ui, &mut e.color);
                    ui.add_space(8.0);
                    ui.checkbox(&mut e.archived, egui::RichText::new(t.track_archive).size(13.0).color(theme.text));
                    ui.add_space(8.0);
                }
                ui.label(egui::RichText::new(t.track_profile).size(12.0).color(theme.text_muted));
                profile_combo(ui, "track-edit-profile", &mut e.profile);
                ui.label(egui::RichText::new(t.track_profile_hint).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.add(button(t.track_save, Some(theme.accent))).clicked() {
                        done = Some(if role.manages() {
                            EditDone::Save(
                                e.id.clone(),
                                serde_json::json!({
                                    "name": e.name.trim(),
                                    "client": Some(e.client.trim()).filter(|c| !c.is_empty()),
                                    "color": e.color.map(hex_of),
                                    "note": Some(e.note.trim()).filter(|n| !n.is_empty()),
                                    "archived": e.archived,
                                }),
                                e.profile,
                            )
                        } else {
                            EditDone::Link(e.id.clone(), e.profile)
                        });
                    }
                    if ui.add(button(t.track_cancel, None)).clicked() {
                        close = true;
                    }
                    if role == Role::Owner {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if e.confirm { t.track_delete_project_sure } else { t.track_delete_project };
                            let b = egui::Button::new(egui::RichText::new(label).size(12.5).color(if e.confirm { theme.bg } else { RED })).corner_radius(6.0);
                            if ui.add(if e.confirm { b.fill(RED) } else { b.frame_when_inactive(false) }).clicked() {
                                if e.confirm {
                                    done = Some(EditDone::Delete(e.id.clone()));
                                } else {
                                    e.confirm = true;
                                }
                            }
                        });
                    }
                });
            });
            if close || modal.should_close() || done.is_some() {
                self.track.edit = None;
            }
        }
        let link = |app: &mut App, id: &str, profile: Option<Uuid>| match profile {
            Some(p) => {
                app.config.settings.track_profiles.insert(id.to_owned(), p);
            }
            None => {
                app.config.settings.track_profiles.remove(id);
            }
        };
        match done {
            Some(EditDone::Save(id, fields, profile)) => {
                link(self, &id, profile);
                self.track_write(move |c| c.update(&id, fields));
            }
            Some(EditDone::Link(id, profile)) => link(self, &id, profile),
            Some(EditDone::Delete(id)) => {
                if let Some(c) = self.track_client() {
                    let ctx = self.ctx.clone();
                    self.track.spawn(&ctx, c, move |c| Done::Gone(c.delete(&id).map(|()| id)));
                }
            }
            None => {}
        }
    }
}

/// A clipboard with a tick: the "Suivi" category.
pub(super) fn paint_track_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let board = Rect::from_center_size(c + Vec2::new(0.0, 1.0), Vec2::new(13.0, 15.0));
    painter.rect_stroke(board, 2.0, stroke, egui::StrokeKind::Middle);
    painter.rect_filled(Rect::from_center_size(Pos2::new(c.x, board.min.y), Vec2::new(6.0, 3.0)), 1.0, color);
    painter.line_segment([c + Vec2::new(-3.5, 1.0), c + Vec2::new(-1.0, 3.5)], stroke);
    painter.line_segment([c + Vec2::new(-1.0, 3.5), c + Vec2::new(3.8, -1.8)], stroke);
}
