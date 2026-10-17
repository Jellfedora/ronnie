//! The "Downloads" category: the modules (see `crate::modules`), apps of their own installed on demand
//! and started in their own window. The sidebar lists them; the page has a card for each: download,
//! open, update, uninstall.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::sidebar::{paint_panel_title, paint_row_bg};
use super::*;
use crate::modules::{self, Latest, Module};

/// The catalog looked at again (when the page opens, and in the background while a module is
/// installed, for its updates) at most this often.
const REFRESH_SECS: f64 = 3600.0;

/// What a look at floor and GitHub found: the catalog, the newest release of each module (by id),
/// their icons.
struct Fetched {
    catalog: Result<Vec<Module>, String>,
    latest: HashMap<String, Result<Option<Latest>, String>>,
    icons: HashMap<String, Vec<u8>>,
}

struct Job {
    module: Module,
    progress: Arc<Mutex<(u64, u64)>>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

#[derive(Default)]
pub(super) struct Modules {
    /// The catalog, once fetched (or why it couldn't be).
    catalog: Option<Result<Vec<Module>, String>>,
    latest: HashMap<String, Result<Option<Latest>, String>>,
    fetching: Option<Arc<Mutex<Option<Fetched>>>>,
    /// When it was last fetched (egui time).
    fetched_at: Option<f64>,
    /// Installed: id → version.
    installed: HashMap<String, String>,
    /// The installed modules as described when installed: listed when floor can't be reached.
    installed_modules: Vec<Module>,
    jobs: Vec<Job>,
    icons: HashMap<String, egui::TextureHandle>,
    /// The module whose uninstall button was clicked once: a second click uninstalls it.
    confirm_uninstall: Option<String>,
    /// Looked once at startup for updates of the installed modules.
    started: bool,
}

impl Modules {
    fn read_installed(&mut self) {
        self.installed_modules = modules::installed();
        self.installed = self.installed_modules.iter().filter_map(|m| Some((m.id.clone(), modules::installed_version(&m.id)?))).collect();
    }

    fn fetch(&mut self, ctx: &egui::Context) {
        if self.fetching.is_some() {
            return;
        }
        let slot = Arc::new(Mutex::new(None));
        self.fetching = Some(slot.clone());
        self.fetched_at = Some(ctx.input(|i| i.time));
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("modules".into()).spawn(move || {
            let catalog = modules::catalog().map_err(|e| format!("{e:#}"));
            let (mut latest, mut icons) = (HashMap::new(), HashMap::new());
            // The installed ones too: one taken off the catalog can still be updated.
            let mut all = catalog.as_ref().map(Vec::clone).unwrap_or_default();
            for m in modules::installed() {
                if !all.iter().any(|c| c.id == m.id) {
                    all.push(m);
                }
            }
            for m in &all {
                latest.insert(m.id.clone(), modules::latest(m).map_err(|e| format!("{e:#}")));
                if let Some(bytes) = modules::icon(m) {
                    icons.insert(m.id.clone(), bytes);
                }
            }
            *slot.lock().unwrap() = Some(Fetched { catalog, latest, icons });
            ctx.request_repaint();
        });
    }

    /// The modules to show: the catalog's, then those installed it no longer lists (or all the
    /// installed ones, when floor couldn't be reached).
    fn shown(&self) -> Vec<Module> {
        let mut shown = match &self.catalog {
            Some(Ok(catalog)) => catalog.clone(),
            _ => Vec::new(),
        };
        for m in &self.installed_modules {
            if !shown.iter().any(|s| s.id == m.id) {
                shown.push(m.clone());
            }
        }
        shown
    }

    fn job(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.module.id == id)
    }

    fn latest_of(&self, id: &str) -> Option<&Latest> {
        self.latest.get(id).and_then(|r| r.as_ref().ok()).and_then(Option::as_ref)
    }

    /// An installed module has a newer version published.
    pub(super) fn any_update(&self) -> bool {
        self.installed.keys().any(|id| self.has_update(id))
    }

    /// A newer version of installed module `id` is published.
    fn has_update(&self, id: &str) -> bool {
        match (self.installed.get(id), self.latest_of(id)) {
            (Some(installed), Some(latest)) => modules::is_update(latest, installed),
            _ => false,
        }
    }

    fn install(&mut self, ctx: &egui::Context, module: Module, latest: Latest) {
        if self.job(&module.id).is_some() {
            return;
        }
        let progress = Arc::new(Mutex::new((0, 0)));
        let result = Arc::new(Mutex::new(None));
        let (p, r, m, ctx) = (progress.clone(), result.clone(), module.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("module-install".into()).spawn(move || {
            let done = modules::install(&m, &latest, p, &ctx).map_err(|e| format!("{e:#}"));
            crate::log::info(&match &done {
                Ok(()) => format!("module {} {} installed", m.id, latest.version),
                Err(e) => format!("module {} not installed: {e}", m.id),
            });
            *r.lock().unwrap() = Some(done);
            ctx.request_repaint();
        });
        self.jobs.push(Job { module, progress, result });
    }
}

impl App {
    /// Collects what finished in the background: the catalog, the installs (an error is shown). At
    /// startup, looks for updates of the installed modules.
    pub(super) fn modules_poll(&mut self) {
        let t = self.t();
        let m = &mut self.modules;
        if !m.started {
            m.started = true;
            m.read_installed();
        }
        // With a module installed: its updates looked for at startup, then every hour.
        let now = self.ctx.input(|i| i.time);
        if !m.installed.is_empty() && m.fetched_at.is_none_or(|at| now - at > REFRESH_SECS) {
            let ctx = self.ctx.clone();
            m.fetch(&ctx);
        }
        if !m.installed.is_empty() {
            self.ctx.request_repaint_after(std::time::Duration::from_secs(REFRESH_SECS as u64));
        }
        if let Some(fetched) = m.fetching.as_ref().and_then(|f| f.lock().unwrap().take()) {
            m.fetching = None;
            m.catalog = Some(fetched.catalog);
            m.latest = fetched.latest;
            for (id, bytes) in fetched.icons {
                let texture = load_png(&self.ctx, &format!("module-{id}"), &bytes);
                m.icons.insert(id, texture);
            }
        }
        let mut changed = false;
        for job in std::mem::take(&mut m.jobs) {
            let done = job.result.lock().unwrap().take();
            match done {
                None => m.jobs.push(job),
                Some(Ok(())) => changed = true,
                Some(Err(e)) => self.error = Some(format!("{} : {e}", t.modules_install_failed.replace("{name}", &job.module.name))),
            }
        }
        if changed {
            self.modules.read_installed();
        }
    }

    /// The Downloads page, in this window: shown instead of the tabs. The catalog is fetched again
    /// when it is old (or failed).
    pub(super) fn open_modules_page(&mut self) {
        self.modules_page = true;
        self.dev_page = false;
        self.music_page = false;
        self.notes_page = false;
        self.home_game = false;
        self.modules.read_installed();
        let now = self.ctx.input(|i| i.time);
        let stale = self.modules.fetched_at.is_none_or(|at| now - at > REFRESH_SECS) || matches!(self.modules.catalog, Some(Err(_)));
        if stale {
            let ctx = self.ctx.clone();
            self.modules.fetch(&ctx);
        }
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    fn launch_module(&mut self, module: &Module) {
        if let Err(e) = modules::launch(module) {
            let t = self.t();
            self.error = Some(format!("{} : {e:#}", t.modules_launch_failed.replace("{name}", &module.name)));
        }
    }

    /// The "Downloads" category: the modules, one a row (installed or not, an update to take); a
    /// click shows the page.
    pub(super) fn modules_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        paint_panel_title(&painter, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H)), t.cat_modules, &theme);
        *y += SECTION_HEADER_H;
        let shown = self.modules.shown();
        let mut open = false;
        let mut launch = None;
        // Before the catalog comes: a single row leading to the page.
        let rows: Vec<Option<Module>> = if shown.is_empty() { vec![None] } else { shown.into_iter().map(Some).collect() };
        for module in rows {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 8.0));
            *y += slot.height() + ROW_GAP;
            let id = module.as_ref().map_or("", |m| m.id.as_str());
            let resp = ui.interact(slot, ui.id().with(("module", id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            paint_row_bg(&painter, slot, self.modules_page && module.is_none(), resp.hovered(), &theme);
            let icon = Pos2::new(slot.min.x + 17.0, slot.center().y);
            match self.modules.icons.get(id) {
                Some(texture) => {
                    let rect = Rect::from_center_size(icon, Vec2::splat(20.0));
                    painter.image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => paint_modules_icon(&painter, icon, theme.text_muted),
            }
            let name = module.as_ref().map_or(t.cat_modules, |m| m.name.as_str());
            painter.text(Pos2::new(slot.min.x + 36.0, slot.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), if resp.hovered() { theme.text } else { theme.text_muted });
            let note = if self.modules.has_update(id) {
                Some((t.modules_update_note, theme.accent))
            } else if self.modules.installed.contains_key(id) {
                Some((t.modules_installed_note, theme.text_muted.gamma_multiply(0.85)))
            } else {
                None
            };
            if let Some((note, color)) = note {
                painter.text(Pos2::new(slot.max.x - 10.0, slot.center().y), Align2::RIGHT_CENTER, note, FontId::proportional(11.5), color);
            }
            // A double click on an installed module opens it; a click shows the page.
            if resp.double_clicked() && self.modules.installed.contains_key(id) {
                launch = module.clone();
            } else if resp.clicked() {
                open = true;
            }
        }
        if let Some(module) = launch {
            self.launch_module(&module);
        } else if open {
            self.open_modules_page();
        }
    }

    /// The Downloads page: a card per module.
    pub(super) fn modules_page_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let margin = if rect.width() > 700.0 { 40.0 } else { 16.0 };
        let area = Rect::from_min_max(rect.min + Vec2::new(margin, 24.0), rect.max - Vec2::new(margin, 24.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let ui = &mut ui;

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.modules_intro).size(12.5).color(theme.text_muted.gamma_multiply(0.85)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.modules.fetching.is_some() {
                    ui.spinner();
                } else if ui.add(egui::Button::new(egui::RichText::new(format!("⟳  {}", t.modules_retry)).size(12.0)).frame_when_inactive(false).corner_radius(5.0)).clicked() {
                    self.modules.fetched_at = None;
                    self.modules.fetch(ui.ctx());
                }
            });
        });
        ui.add_space(12.0);

        let shown = self.modules.shown();
        match &self.modules.catalog {
            None if shown.is_empty() => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(t.modules_loading).color(theme.text_muted));
                });
                return;
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(t.modules_offline).size(12.5).color(theme.ansi[3]));
                ui.label(egui::RichText::new(e).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                ui.add_space(10.0);
            }
            _ => {}
        }
        if shown.is_empty() {
            ui.label(egui::RichText::new(t.modules_none).color(theme.text_muted));
            return;
        }

        let (mut launch, mut install, mut uninstall, mut changes) = (None, None, None, None);
        egui::ScrollArea::vertical().id_salt("modules-page").auto_shrink([false, true]).show(ui, |ui| {
            for module in &shown {
                let installed = self.modules.installed.get(&module.id).cloned();
                let latest = self.modules.latest_of(&module.id).cloned();
                let looked = self.modules.latest.contains_key(&module.id);
                let job = self.modules.job(&module.id).map(|j| *j.progress.lock().unwrap());
                Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(14.0).inner_margin(18.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        // The icon.
                        let (icon, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        match self.modules.icons.get(&module.id) {
                            Some(texture) => {
                                ui.painter().image(texture.id(), icon, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                            }
                            None => {
                                ui.painter().rect_filled(icon.shrink(4.0), 12.0, theme.tab_hover);
                                paint_modules_icon(ui.painter(), icon.center(), theme.text_muted);
                            }
                        }
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&module.name).size(19.0).color(theme.text));
                            ui.label(egui::RichText::new(&module.description).size(12.5).color(theme.text_muted));
                            ui.add_space(4.0);
                            let status = match &installed {
                                Some(v) => t.modules_version.replace("{v}", v),
                                None => t.modules_not_installed.to_owned(),
                            };
                            ui.label(egui::RichText::new(status).size(11.5).color(theme.text_muted.gamma_multiply(0.8)));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let button = |text: String, fill: Option<Color32>| {
                                let label = egui::RichText::new(text).size(13.0);
                                match fill {
                                    Some(fill) => egui::Button::new(label.color(theme.bg)).fill(fill).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0)),
                                    None => egui::Button::new(label).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0)),
                                }
                            };
                            if let Some((received, total)) = job {
                                let fraction = if total > 0 { received as f32 / total as f32 } else { 0.0 };
                                ui.add(egui::ProgressBar::new(fraction).desired_width(140.0).fill(theme.accent).show_percentage());
                                return;
                            }
                            match (&installed, &latest) {
                                (Some(v), Some(l)) if modules::is_update(l, v) => {
                                    if ui.add(button(t.modules_update.replace("{v}", &l.version), Some(theme.accent))).clicked() {
                                        install = Some((module.clone(), l.clone()));
                                    }
                                    if ui.add(button(t.modules_open.to_owned(), None)).clicked() {
                                        launch = Some(module.clone());
                                    }
                                }
                                (Some(_), _) => {
                                    if ui.add(button(t.modules_open.to_owned(), Some(theme.accent))).clicked() {
                                        launch = Some(module.clone());
                                    }
                                }
                                (None, Some(l)) => {
                                    if ui.add(button(format!("↓  {}", t.modules_download), Some(theme.accent))).clicked() {
                                        install = Some((module.clone(), l.clone()));
                                    }
                                }
                                (None, None) if looked => {
                                    let why = match self.modules.latest.get(&module.id) {
                                        Some(Err(e)) => e.clone(),
                                        _ => t.modules_not_published.to_owned(),
                                    };
                                    ui.label(egui::RichText::new(why).size(11.5).color(theme.text_muted));
                                }
                                (None, None) => {
                                    ui.spinner();
                                }
                            }
                        });
                    });
                    // Under the card's head: what's new, uninstall.
                    if installed.is_some() || latest.is_some() {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_space(76.0);
                            if let Some(l) = &latest
                                && ui.add(egui::Button::new(egui::RichText::new(format!("{} {}", t.modules_changes, l.version)).size(11.5).color(theme.text_muted)).frame_when_inactive(false)).clicked()
                            {
                                changes = Some(l.url.clone());
                            }
                            if installed.is_some() && job.is_none() {
                                let confirming = self.modules.confirm_uninstall.as_deref() == Some(module.id.as_str());
                                let label = if confirming { t.modules_confirm_uninstall } else { t.modules_uninstall };
                                let text = egui::RichText::new(label).size(11.5).color(if confirming { theme.bg } else { theme.ansi[1] });
                                let b = egui::Button::new(text).corner_radius(5.0);
                                let b = if confirming { b.fill(theme.ansi[1]) } else { b.frame_when_inactive(false) };
                                if ui.add(b).clicked() {
                                    if confirming {
                                        uninstall = Some(module.id.clone());
                                    } else {
                                        self.modules.confirm_uninstall = Some(module.id.clone());
                                    }
                                }
                            }
                        });
                    }
                });
                ui.add_space(10.0);
            }
        });

        if let Some(module) = launch {
            self.launch_module(&module);
        }
        if let Some((module, latest)) = install {
            self.modules.confirm_uninstall = None;
            let ctx = self.ctx.clone();
            self.modules.install(&ctx, module, latest);
        }
        if let Some(id) = uninstall {
            self.modules.confirm_uninstall = None;
            if let Err(e) = modules::uninstall(&id) {
                self.error = Some(format!("{e:#}"));
            }
            self.modules.read_installed();
        }
        if let Some(url) = changes.filter(|u| u.starts_with("https://")) {
            crate::terminal::open_url(&url);
        }
    }
}

/// An arrow down into a tray: the Downloads category.
pub(super) fn paint_modules_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.line_segment([c + Vec2::new(0.0, -7.0), c + Vec2::new(0.0, 2.5)], stroke);
    painter.line_segment([c + Vec2::new(-3.8, -1.3), c + Vec2::new(0.0, 2.5)], stroke);
    painter.line_segment([c + Vec2::new(3.8, -1.3), c + Vec2::new(0.0, 2.5)], stroke);
    painter.line_segment([c + Vec2::new(-7.0, 2.0), c + Vec2::new(-7.0, 6.5)], stroke);
    painter.line_segment([c + Vec2::new(-7.0, 6.5), c + Vec2::new(7.0, 6.5)], stroke);
    painter.line_segment([c + Vec2::new(7.0, 6.5), c + Vec2::new(7.0, 2.0)], stroke);
}
