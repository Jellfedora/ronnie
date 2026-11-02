//! The pages of the Dev category for the projects' environments (see `crate::envs`): PHP and nginx,
//! the versions installed and those published to install, side by side as for Node.js.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::*;
use crate::envs::{self, Kind as Env, Progress, Release};

/// The versions published, once fetched (or why they couldn't be).
type Fetched = Result<Vec<Release>, String>;

/// What the environments' pages know and do.
#[derive(Default)]
pub(super) struct Envs {
    installed: HashMap<Env, Vec<String>>,
    releases: HashMap<Env, Fetched>,
    fetching: HashMap<Env, Arc<Mutex<Option<Fetched>>>>,
    installs: Vec<Install>,
    /// The version whose delete button was clicked once: a second click deletes it.
    confirm_delete: Option<(Env, String)>,
}

struct Install {
    kind: Env,
    version: String,
    progress: Arc<Mutex<Progress>>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Envs {
    pub fn new() -> Self {
        let mut envs = Self::default();
        envs.refresh();
        envs
    }

    /// The installed versions read again.
    pub fn refresh(&mut self) {
        for kind in Env::ALL {
            self.installed.insert(kind, envs::installed(kind));
        }
    }

    pub fn installed(&self, kind: Env) -> &[String] {
        self.installed.get(&kind).map(Vec::as_slice).unwrap_or_default()
    }

    fn installing(&self, kind: Env, version: &str) -> Option<Progress> {
        self.installs.iter().find(|i| i.kind == kind && i.version == version).map(|i| *i.progress.lock().unwrap())
    }

    /// The list of versions published fetched, unless it is or is being.
    pub fn fetch_if_needed(&mut self, kind: Env, ctx: &egui::Context) {
        if self.fetching.contains_key(&kind) || self.releases.get(&kind).is_some_and(Result::is_ok) {
            return;
        }
        self.fetch(kind, ctx);
    }

    fn fetch(&mut self, kind: Env, ctx: &egui::Context) {
        let slot = Arc::new(Mutex::new(None));
        self.fetching.insert(kind, slot.clone());
        self.releases.remove(&kind);
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("env-releases".into()).spawn(move || {
            *slot.lock().unwrap() = Some(envs::releases(kind).map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
    }

    fn install(&mut self, kind: Env, release: Release, ctx: &egui::Context) {
        if self.installing(kind, &release.version).is_some() {
            return;
        }
        let progress = Arc::new(Mutex::new(Progress::default()));
        let result = Arc::new(Mutex::new(None));
        let (p, r, ctx) = (progress.clone(), result.clone(), ctx.clone());
        let version = release.version.clone();
        let _ = std::thread::Builder::new().name("env-install".into()).spawn(move || {
            let done = envs::install(kind, &release, &p, &ctx).map_err(|e| format!("{e:#}"));
            crate::log::info(&match &done {
                Ok(()) => format!("{} {} installed", kind.name(), release.version),
                Err(e) => format!("{} {} not installed: {e}", kind.name(), release.version),
            });
            *r.lock().unwrap() = Some(done);
            ctx.request_repaint();
        });
        self.installs.push(Install { kind, version, progress, result });
    }
}

impl App {
    /// Collects what finished in the background: the lists of versions, the installs (an error is
    /// shown).
    pub(super) fn envs_poll(&mut self) {
        let done: Vec<(Env, Fetched)> = self.envs.fetching.iter().filter_map(|(k, slot)| slot.lock().unwrap().take().map(|f| (*k, f))).collect();
        for (kind, fetched) in done {
            self.envs.fetching.remove(&kind);
            self.envs.releases.insert(kind, fetched);
        }
        let mut changed = false;
        let t = self.t();
        for install in std::mem::take(&mut self.envs.installs) {
            let done = install.result.lock().unwrap().take();
            match done {
                None => self.envs.installs.push(install),
                Some(Ok(())) => changed = true,
                Some(Err(e)) => self.error = Some(format!("{} : {e}", t.env_install_failed.replace("{name}", install.kind.name()).replace("{v}", &install.version))),
            }
        }
        if changed {
            self.envs.refresh();
        }
    }

    /// How many projects run on `version` of `kind` (PHP, the databases: those set to it; nginx: all
    /// of them, on the newest).
    fn env_users(&self, kind: Env, version: &str) -> usize {
        match kind {
            Env::Php => self.config.projects.iter().filter(|p| p.php_version().as_deref() == Some(version)).count(),
            Env::Nginx => {
                if self.envs.installed(Env::Nginx).first().map(String::as_str) == Some(version) { self.config.projects.len() } else { 0 }
            }
            Env::Mysql | Env::Mariadb => self.config.projects.iter().filter(|p| p.db.as_ref().is_some_and(|db| db.kind == kind && db.version == version)).count(),
        }
    }

    /// The page of an environment: what it is for, installed | to install.
    pub(super) fn env_page_ui(&mut self, ui: &mut Ui, kind: Env, t: &'static Strings, theme: &Theme) {
        let desc = match kind {
            Env::Php => t.env_php_desc,
            Env::Nginx => t.env_nginx_desc,
            Env::Mysql | Env::Mariadb => t.env_db_desc,
        };
        ui.label(egui::RichText::new(desc).size(12.5).color(theme.text_muted.gamma_multiply(0.85)));
        ui.add_space(12.0);
        let height = ui.available_height() - 40.0;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(14.0).inner_margin(20.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(height.max(200.0));
            let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            paint_env_icon(ui.painter(), Pos2::new(head.min.x + 9.0, head.center().y), kind, env_color(kind));
            ui.painter().text(Pos2::new(head.min.x + 26.0, head.center().y), Align2::LEFT_CENTER, kind.name(), FontId::proportional(19.0), theme.text);
            ui.add_space(10.0);
            if ui.available_width() > 640.0 {
                ui.columns(2, |cols| {
                    let height = cols[0].available_height();
                    egui::ScrollArea::vertical().id_salt(("env-installed", kind.name())).max_height(height).auto_shrink([false, true]).show(&mut cols[0], |ui| self.env_installed_list(ui, kind, t, theme));
                    self.env_releases_list(&mut cols[1], kind, t, theme);
                });
            } else {
                let third = ui.available_height() / 3.0;
                egui::ScrollArea::vertical().id_salt(("env-installed-narrow", kind.name())).max_height(third).auto_shrink([false, true]).show(ui, |ui| self.env_installed_list(ui, kind, t, theme));
                ui.add_space(16.0);
                self.env_releases_list(ui, kind, t, theme);
            }
        });
    }

    /// The versions installed: the projects on each, its folder, delete (a second click confirms).
    fn env_installed_list(&mut self, ui: &mut Ui, kind: Env, t: &'static Strings, theme: &Theme) {
        ui.label(egui::RichText::new(t.node_installed_title.to_uppercase()).size(11.0).color(theme.text_muted).extra_letter_spacing(1.2));
        ui.add_space(6.0);
        let installed = self.envs.installed(kind).to_vec();
        if installed.is_empty() {
            ui.label(egui::RichText::new(t.node_none_installed).size(12.5).color(theme.text_muted));
            return;
        }
        let (mut delete, mut open) = (None, None);
        for version in installed {
            let system = version == envs::SYSTEM;
            let users = self.env_users(kind, &version);
            Frame::NONE.fill(theme.bg.gamma_multiply(0.6)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let name = if system { t.env_system.to_owned() } else { version.clone() };
                    ui.label(egui::RichText::new(name).monospace().size(13.5).color(theme.text));
                    if users > 0 {
                        let text = if users == 1 { t.env_project_one.to_owned() } else { t.env_projects.replace("{n}", &users.to_string()) };
                        ui.label(egui::RichText::new(text).size(11.0).color(theme.text_muted));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !system {
                            let confirming = self.envs.confirm_delete.as_ref() == Some(&(kind, version.clone()));
                            let label = if confirming { egui::RichText::new(t.node_confirm_delete).color(theme.bg) } else { egui::RichText::new(t.delete).color(theme.ansi[1]) };
                            let button = egui::Button::new(label.size(12.0)).corner_radius(5.0);
                            let button = if confirming { button.fill(theme.ansi[1]) } else { button.frame_when_inactive(false) };
                            if ui.add(button).clicked() {
                                if confirming { delete = Some(version.clone()) } else { self.envs.confirm_delete = Some((kind, version.clone())) }
                            }
                        }
                        let folder = ui.add(egui::Button::new("    ").frame_when_inactive(false)).on_hover_text(t.open_location);
                        super::notes::paint_folder_icon(ui.painter(), folder.rect.center(), if folder.hovered() { theme.text } else { theme.text_muted });
                        if folder.clicked() {
                            open = Some(version.clone());
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
        if let Some(version) = open {
            let system = match kind {
                Env::Nginx => envs::system_nginx(),
                _ => envs::db_server(kind, envs::SYSTEM),
            };
            let folder = if version == envs::SYSTEM { system.and_then(|p| p.parent().map(Path::to_path_buf)) } else { envs::dir(kind, &version) };
            if let Some(folder) = folder {
                config::open_folder(&folder);
            }
        }
        if let Some(version) = delete {
            self.envs.confirm_delete = None;
            if let Err(e) = envs::uninstall(kind, &version) {
                self.error = Some(format!("{e:#}"));
            }
            self.envs.refresh();
        }
    }

    /// The versions published: install buttons, the progress of those under way.
    fn env_releases_list(&mut self, ui: &mut Ui, kind: Env, t: &'static Strings, theme: &Theme) {
        ui.label(egui::RichText::new(t.env_available_title.replace("{source}", kind.source()).to_uppercase()).size(11.0).color(theme.text_muted).extra_letter_spacing(1.2));
        ui.add_space(6.0);
        let releases = match self.envs.releases.get(&kind) {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(t.node_loading).color(theme.text_muted));
                });
                return;
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(e).color(if kind == Env::Nginx && !cfg!(windows) { theme.text_muted } else { theme.ansi[1] }));
                if kind == Env::Nginx && !cfg!(windows) {
                    ui.add_space(6.0);
                    let found = envs::system_nginx().map(|p| t.env_nginx_found.replace("{path}", &p.display().to_string())).unwrap_or_else(|| t.env_nginx_missing.to_owned());
                    ui.label(egui::RichText::new(found).size(12.5).color(theme.text));
                    if ui.button(t.env_check_again).clicked() {
                        self.envs.refresh();
                    }
                } else if ui.button(t.node_retry).clicked() {
                    self.envs.fetch(kind, ui.ctx());
                }
                return;
            }
            Some(Ok(r)) => r.clone(),
        };
        let installed = self.envs.installed(kind).to_vec();
        let mut install = None;
        egui::ScrollArea::vertical().id_salt(("env-releases", kind.name())).max_height(ui.available_height()).auto_shrink([false, true]).show(ui, |ui| {
            for release in releases {
                let done = installed.contains(&release.version);
                let progress = self.envs.installing(kind, &release.version);
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    ui.label(egui::RichText::new(&release.version).monospace().size(13.0).color(if done { theme.text_muted } else { theme.text }));
                    ui.label(egui::RichText::new(&release.note).size(11.0).color(env_color(kind)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match progress {
                        Some(p) if p.unpacking => {
                            ui.label(egui::RichText::new(t.node_unpacking).size(11.5).color(theme.text_muted));
                        }
                        Some(p) => {
                            let fraction = if p.total > 0 { p.received as f32 / p.total as f32 } else { 0.0 };
                            ui.add(egui::ProgressBar::new(fraction).desired_width(110.0).fill(env_color(kind)).show_percentage());
                        }
                        None if done => {
                            ui.label(egui::RichText::new(format!("✓ {}", t.node_installed_badge)).size(11.5).color(env_color(kind)));
                        }
                        None => {
                            if ui.add(egui::Button::new(egui::RichText::new(format!("↓  {}", t.node_do_install)).size(12.0)).corner_radius(5.0)).clicked() {
                                install = Some(release.clone());
                            }
                        }
                    });
                });
            }
        });
        if let Some(release) = install {
            self.envs.install(kind, release, ui.ctx());
        }
    }
}

/// PHP's mauve, nginx's green, MySQL's blue, MariaDB's brown.
pub(super) fn env_color(kind: Env) -> Color32 {
    match kind {
        Env::Php => Color32::from_rgb(0x88, 0x92, 0xbf),
        Env::Nginx => Color32::from_rgb(0x2f, 0xa0, 0x5c),
        Env::Mysql => Color32::from_rgb(0x3e, 0x8e, 0xc4),
        Env::Mariadb => Color32::from_rgb(0xc4, 0x8a, 0x5a),
    }
}

/// PHP: an ellipse; nginx: a hexagon with its "N"; the databases: a cylinder.
pub(super) fn paint_env_icon(painter: &egui::Painter, c: Pos2, kind: Env, color: Color32) {
    match kind {
        Env::Mysql | Env::Mariadb => super::dbview::paint_db_icon(painter, c, color),
        Env::Php => {
            let points: Vec<Pos2> = (0..32).map(|k| c + Vec2::new((k as f32 / 32.0 * std::f32::consts::TAU).cos() * 8.5, (k as f32 / 32.0 * std::f32::consts::TAU).sin() * 5.0)).collect();
            painter.add(egui::Shape::closed_line(points, Stroke::new(1.4, color)));
            painter.text(c, Align2::CENTER_CENTER, "p", FontId::proportional(9.5), color);
        }
        Env::Nginx => {
            super::dev::paint_node_icon(painter, c, color);
            painter.text(c, Align2::CENTER_CENTER, "N", FontId::proportional(8.5), color);
        }
    }
}
