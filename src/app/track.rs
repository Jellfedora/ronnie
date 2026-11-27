//! The "Suivi" category: project tracking (see `crate::track`), kept on this computer. The
//! sidebar lists the projects by client (the hours left on the current counter beside each); the page
//! shows them all, or one: the time spent, its tasks (logged here: a day, a duration typed, a
//! description, what it is charged to), its hour counters (a new one proposed once used up), its
//! quotes. A project can be tied to a profile of this machine (its tab opened from
//! it), and the first projects made from the profiles picked.
//!
//! Every call goes in a thread; what comes back arrives by a channel, read each frame.

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::sidebar::{paint_panel_title, paint_row_bg};
use super::*;
use crate::track::{self, Billing, Client, Project, Role, Summary, TaskInput, format_minutes as hm, show_day};

/// Read again this often while the page is shown (Claude may have written, with `ronnie track`).
const REFRESH_SECS: f64 = 20.0;
/// And this often otherwise (the sidebar shows the hours left).
const IDLE_REFRESH_SECS: f64 = 300.0;

const RED: Color32 = Color32::from_rgb(0xe0, 0x55, 0x55);
const GREEN: Color32 = Color32::from_rgb(0x4c, 0xc2, 0x6a);
const ORANGE: Color32 = Color32::from_rgb(0xe0, 0xa8, 0x3c);

/// What a call gave back.
enum Done {
    List(Result<Vec<Summary>, String>),
    /// A project read, or written (then shown, when `select`).
    Project {
        result: Box<Result<Project, String>>,
        select: bool,
    },
    /// A project gone (deleted, or left): its id.
    Gone(Result<String, String>),
    /// Projects made from profiles: each one and its profile.
    Made(Vec<(Result<Project, String>, Option<Uuid>)>),
    /// An image of a description read (`track-files/<id>`).
    Image { src: String, result: Result<Vec<u8>, String> },
    /// A joined file opened (downloaded, then given to the system), or not.
    Opened(Result<(), String>),
}

#[path = "trackui.rs"]
mod page;

/// The tabs of a project's page.
#[derive(Default, PartialEq, Clone, Copy)]
pub(super) enum Tab {
    #[default]
    Overview,
    Time,
    Quotes,
    Counters,
    /// The conversation with Claude Code, in the folder of the tied profile.
    Claude,
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
    /// The project's page: its tab, its windows, its filters.
    page: page::PageState,
    new_project: Option<NewProject>,
    import: Option<Import>,
    edit: Option<EditProject>,
    show_archived: bool,
    /// The profiles proposed once already (with no project yet).
    offered: bool,
    /// The conversations with Claude, by project.
    pub(super) chat: super::trackchat::ChatUi,
    /// The images the descriptions show, read (or just pasted), and those being read.
    images: super::mdedit::Images,
    asked: std::collections::HashSet<String>,
    /// The tasks of every project as one table (« Tâches »), and its filters.
    tasks: page::TasksState,
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
            page: page::PageState::default(),
            new_project: None,
            import: None,
            edit: None,
            show_archived: false,
            offered: false,
            chat: Default::default(),
            images: HashMap::new(),
            asked: std::collections::HashSet::new(),
            tasks: page::TasksState::default(),
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
    /// A call, in a thread; its answer comes back by the channel.
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
}

/// What was clicked in the sidebar.
enum Act {
    Select(Option<String>),
    Import,
}

impl App {
    fn track_client(&self) -> Option<Client> {
        Client::local().ok()
    }

    /// The list (and the projects changed since) read.
    pub(super) fn track_refresh(&mut self) {
        let Some(client) = self.track_client() else {
            return;
        };
        let now = self.ctx.input(|i| i.time);
        self.track.fetched_at = Some(now);
        let ctx = self.ctx.clone();
        self.track.spawn(&ctx, client, |c| Done::List(c.projects()));
    }

    pub(super) fn track_fetch(&mut self, id: &str, select: bool) {
        let Some(client) = self.track_client() else {
            return;
        };
        let (ctx, id) = (self.ctx.clone(), id.to_owned());
        self.track.spawn(&ctx, client, move |c| Done::Project { result: Box::new(c.project(&id)), select });
    }

    /// A write; the project as it is then comes back.
    fn track_write(&mut self, call: impl FnOnce(&Client) -> Result<Project, String> + Send + 'static) {
        let Some(client) = self.track_client() else {
            return;
        };
        let ctx = self.ctx.clone();
        self.track.spawn(&ctx, client, move |c| Done::Project { result: Box::new(call(c)), select: false });
    }

    /// The images `text` shows (`track-files/<id>`) not read yet, read.
    fn track_want_images(&mut self, project: &str, text: &str) {
        let wanted: Vec<String> = super::notes::image_sources(text).into_iter().filter(|s| s.starts_with(track::FILES_PREFIX)).map(str::to_owned).collect();
        for src in wanted {
            if self.track.images.contains_key(&src) || !self.track.asked.insert(src.clone()) {
                continue;
            }
            let Some(client) = self.track_client() else { return };
            let (ctx, project, file) = (self.ctx.clone(), project.to_owned(), src[track::FILES_PREFIX.len()..].to_owned());
            self.track.spawn(&ctx, client, move |c| Done::Image { result: c.download(&project, &file), src });
        }
    }

    /// An image just pasted in a description: shown at once, kept joined to `on`.
    fn track_pasted(&mut self, project: &str, on: &str, file: String, png: Vec<u8>) {
        let src = format!("{}{file}", track::FILES_PREFIX);
        let img = super::notes::Img::from_bytes(&self.ctx, &src, &png);
        self.track.images.insert(src, img);
        self.track_send_file(project, on, file, "image.png".to_owned(), png);
    }

    /// A file kept, joined to `on` (`project`, `task:<id>`...), under the id `file`.
    fn track_send_file(&mut self, project: &str, on: &str, file: String, name: String, bytes: Vec<u8>) {
        let (id, on) = (project.to_owned(), on.to_owned());
        self.track_write(move |c| {
            c.upload(&id, &on, &file, &name, &bytes)?;
            c.project(&id)
        });
    }

    /// Files picked or dropped, sent joined to `on`; too big ones said. The ids given them.
    fn track_attach(&mut self, project: &str, on: &str, paths: Vec<std::path::PathBuf>) -> Vec<String> {
        let mut sent = Vec::new();
        for path in paths {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "fichier".into());
            match std::fs::metadata(&path) {
                Ok(m) if m.len() > track::FILE_MAX => {
                    self.error = Some(self.t().track_file_too_big.replace("{file}", &name));
                    continue;
                }
                Err(e) => {
                    self.error = Some(format!("{name} : {e}"));
                    continue;
                }
                Ok(_) => {}
            }
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let file = Uuid::new_v4().to_string();
                    sent.push(file.clone());
                    self.track_send_file(project, on, file, name, bytes);
                }
                Err(e) => self.error = Some(format!("{name} : {e}")),
            }
        }
        sent
    }

    /// A joined file opened: downloaded in a folder of Ronnie's, then given to the system.
    fn track_open_file(&mut self, project: &str, file: &track::TrackFile) {
        let Some(client) = self.track_client() else { return };
        let (ctx, project, id, name) = (self.ctx.clone(), project.to_owned(), file.id.clone(), file.name.clone());
        self.track.spawn(&ctx, client, move |c| {
            Done::Opened((|| {
                let bytes = c.download(&project, &id)?;
                let dir = std::env::temp_dir().join("ronnie-track").join(&id);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                // Its name, never a path.
                let safe: String = name.chars().map(|ch| if matches!(ch, '/' | '\\' | ':') || ch.is_control() { '_' } else { ch }).collect();
                let path = dir.join(if safe.trim_matches('.').is_empty() { "fichier".to_owned() } else { safe });
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                crate::terminal::links::open(&path.to_string_lossy());
                Ok(())
            })())
        });
    }

    /// What the files of something asked: picked (sent joined to `on`), opened, taken off. The ids of
    /// those sent.
    fn track_file_acts(&mut self, project: &str, on: &str, acts: Vec<page::FileAct>) -> Vec<String> {
        let mut sent = Vec::new();
        for act in acts {
            match act {
                page::FileAct::Pick => {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        sent.extend(self.track_attach(project, on, paths));
                    }
                }
                page::FileAct::Open(f) => self.track_open_file(project, &f),
                page::FileAct::Delete(f) => self.track_delete_file(project, &f),
            }
        }
        sent
    }

    /// A window closed without making what it wrote: the files sent for it taken off again.
    fn track_drop_pending(&mut self, project: &str, pending: Option<Vec<String>>) {
        for file in pending.unwrap_or_default() {
            self.track_delete_file(project, &file);
        }
    }

    fn track_delete_file(&mut self, project: &str, file: &str) {
        let (id, file) = (project.to_owned(), file.to_owned());
        self.track_write(move |c| {
            c.delete_file(&id, &file)?;
            c.project(&id)
        });
    }

    /// Collects what the calls answered; asks again now and then.
    pub(super) fn track_poll(&mut self) {
        self.track_chat_poll();
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
                            self.track.page.tab = Tab::Overview;
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
                Done::Image { src, result } => {
                    self.track.asked.remove(&src);
                    let img = super::notes::Img::from_bytes(&self.ctx, &src, &result.unwrap_or_default());
                    self.track.images.insert(src, img);
                }
                Done::Opened(Ok(())) => {}
                Done::Opened(Err(e)) => self.error = Some(format!("{} : {e}", t.track_failed)),
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
        self.track.page.tab = match tab {
            "time" | "tasks" => Tab::Time,
            "counters" => Tab::Counters,
            "quotes" => Tab::Quotes,
            "claude" => Tab::Claude,
            "entry" | "item" | "quote-edit" | "settings" => {
                self.track.page.demo = Some(tab.to_owned());
                Tab::Overview
            }
            "import" => {
                self.track_import_open();
                Tab::Overview
            }
            // Every task as one table (« Tâches »).
            "table" => {
                self.track.tasks.shown = true;
                Tab::Overview
            }
            _ => Tab::Overview,
        };
    }

    /// The tracking page, in this window: shown instead of the tabs.
    pub(super) fn open_track_page(&mut self) {
        self.track_page = true;
        self.procs_page = false;
        self.dev_page = false;
        self.music_page = false;
        self.notes_page = false;
        self.track_refresh();
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    fn track_select(&mut self, id: Option<String>) {
        if id.is_some() && id != self.config.settings.track_selected {
            self.track.page = page::PageState::default();
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

    pub(super) fn track_edit_open(&mut self) {
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
        if self.track_client().is_some() {
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
            Some(Act::Select(id)) => self.track_select(id),
            Some(Act::Import) => self.track_import_open(),
            _ => {}
        }
    }

    /// The tracking page: every project, or the one picked.
    pub(super) fn track_page_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let theme = self.theme.clone();
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let margin = if rect.width() > 900.0 { 40.0 } else { 18.0 };
        let area = Rect::from_min_max(rect.min + Vec2::new(margin, 22.0), rect.max - Vec2::new(margin, 12.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let ui = &mut child;

        let selected = self.config.settings.track_selected.clone().and_then(|id| self.track.projects.get(&id).cloned());
        match selected {
            Some(project) => self.track_project(ui, &project),
            None => self.track_overview(ui),
        }
    }

    /// The windows of the page: a new project, the profiles to take up, a project's settings.
    pub(super) fn track_windows(&mut self, ctx: &egui::Context) {
        self.track_page_windows(ctx);
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
                ui.spacing_mut().item_spacing.x = 4.0;
                for (i, c) in clients.iter().enumerate() {
                    if i > 0 {
                        ui.label(egui::RichText::new("·").size(12.0).color(theme.text_muted));
                    }
                    let on = client == c;
                    let text = egui::RichText::new(c).size(12.0).color(if on { theme.accent } else { theme.text_muted });
                    if ui.add(egui::Button::new(text).frame_when_inactive(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
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
            let (images, pid, mut pasted) = (&self.track.images, e.id.clone(), None);
            let modal = egui::Modal::new(egui::Id::new("track-edit")).frame(frame).show(ctx, |ui| {
                ui.set_width(560.0);
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
                }
                // The terminal profile: its tab opened from the project, Claude working in its folder.
                ui.label(egui::RichText::new(t.track_profile).size(12.0).color(theme.text_muted));
                profile_combo(ui, "track-edit-profile", &mut e.profile);
                ui.label(egui::RichText::new(t.track_profile_hint).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                ui.add_space(8.0);
                if role.manages() {
                    ui.label(egui::RichText::new(t.track_note).size(12.0).color(theme.text_muted));
                    let edited = super::mdedit::md_editor(ui, egui::Id::new(("project-note", &e.id)), &mut e.note, 560.0, 5, t.track_note_hint, &theme, t, images);
                    if edited.pasted.is_some() {
                        pasted = edited.pasted;
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.track_color).size(12.0).color(theme.text_muted));
                    color_row(ui, &mut e.color);
                    ui.add_space(8.0);
                    ui.checkbox(&mut e.archived, egui::RichText::new(t.track_archive).size(13.0).color(theme.text));
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
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
            // An image pasted in the project's note: joined to the project.
            if let Some((file, png)) = pasted {
                self.track_pasted(&pid, "project", file, png);
            }
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
