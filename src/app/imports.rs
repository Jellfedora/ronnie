//! Databases brought in from elsewhere, besides WAMP's (wamp.rs): Local's (WP Engine's WordPress tool),
//! each site's database loaded into a server of Ronnie's under a name of its own; and any .sql file
//! loaded into a new database. Both run in the background, their window showing how far they went.

use std::sync::{Arc, Mutex};

use super::dev::DevTool;
use super::*;
use crate::envs::{self, Kind as Env};
use crate::projects::{self, Db, LocalSite, Stack};

/// What an import brought in.
enum Ended {
    /// The databases made (their names), the sites whose database wasn't, and why.
    Local { imported: Vec<String>, failed: Vec<(String, String)>, server: Db, port: u16 },
    Sql { server: Db, port: u16, name: String, project: Option<Uuid> },
}

/// An import under way: what it does, how far, how it ended.
struct Job {
    step: Arc<Mutex<String>>,
    fraction: Arc<Mutex<f32>>,
    done: Arc<Mutex<Option<Result<Ended, String>>>>,
    /// How long it still takes.
    eta: crate::eta::Eta,
}

impl Job {
    fn new() -> Self {
        Self { step: Arc::new(Mutex::new(String::new())), fraction: Arc::new(Mutex::new(0.0)), done: Arc::new(Mutex::new(None)), eta: Default::default() }
    }
}

/// Local's sites: whether to bring in each one's database, and under which name.
struct LocalDialog {
    sites: Vec<(LocalSite, bool, String)>,
    server: Option<Db>,
}

/// A .sql file to load: into which server, under which name, for which project.
struct SqlDialog {
    path: PathBuf,
    server: Option<Db>,
    name: String,
    project: Option<Uuid>,
}

#[derive(Default)]
pub(super) struct Imports {
    local: Option<LocalDialog>,
    sql: Option<SqlDialog>,
    job: Option<Job>,
    error: Option<String>,
}

/// The database servers installed, the newest first (MySQL, then MariaDB).
fn servers() -> Vec<Db> {
    Env::DATABASES.into_iter().flat_map(|kind| envs::installed(kind).into_iter().map(move |version| Db { kind, version })).collect()
}

/// The server a site of Local's database goes to: the newest of its kind, else any.
fn server_for(kind: Env) -> Option<Db> {
    let all = servers();
    all.iter().find(|db| db.kind == kind).or(all.first()).cloned()
}

/// A server of Ronnie's started for an import (unless it runs), and whether to stop it after.
fn borrow_server(stack: &Arc<Mutex<Stack>>, server: &Db) -> Result<(u16, bool), String> {
    let mut stack = stack.lock().unwrap();
    let was_alone = stack.db_alone(server);
    let port = stack.start_db_alone(server).map_err(|e| format!("{e:#}"))?;
    Ok((port, !was_alone))
}

fn give_back(stack: &Arc<Mutex<Stack>>, server: &Db, stop: bool) {
    if stop {
        stack.lock().unwrap().stop_db_alone(server);
    }
}

impl App {
    /// Local's sites listed, to pick the databases to bring in (an error if Local has none).
    pub(super) fn local_import_open(&mut self) {
        let t = self.t();
        if self.projects.imports.job.is_some() || self.read_only {
            return;
        }
        let sites = projects::local_sites();
        if sites.is_empty() {
            self.error = Some(t.local_none.to_owned());
            return;
        }
        let kind = sites.first().map_or(Env::Mysql, |s| s.db_kind);
        let sites = sites
            .into_iter()
            .map(|site| {
                let name = projects::database_name(&site.folder_name());
                (site, true, name)
            })
            .collect::<Vec<_>>();
        self.projects.imports.error = None;
        self.projects.imports.local = Some(LocalDialog { sites, server: server_for(kind) });
    }

    /// A .sql file picked, then where to load it asked; `prefill`: the server, name and project a
    /// project's settings give.
    pub(super) fn sql_import_open(&mut self, prefill: Option<(Option<Db>, String, Uuid)>) {
        if self.projects.imports.job.is_some() || self.read_only {
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("SQL", &["sql", "gz", "zip"]).pick_file() else { return };
        let stem = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = stem.split('.').next().unwrap_or_default().to_owned();
        let (server, name, project) = match prefill {
            Some((server, name, id)) => (server.or_else(|| servers().into_iter().next()), name, Some(id)),
            None => (servers().into_iter().next(), projects::database_name(&stem), None),
        };
        self.projects.imports.error = None;
        self.projects.imports.sql = Some(SqlDialog { path, server, name, project });
    }

    /// Starts bringing in the sites ticked.
    fn local_import_start(&mut self) {
        let t = self.t();
        let Some(dialog) = &self.projects.imports.local else { return };
        let Some(server) = dialog.server.clone() else {
            self.projects.imports.error = Some(t.local_no_server.to_owned());
            return;
        };
        let sites: Vec<(LocalSite, String)> = dialog.sites.iter().filter(|(_, on, name)| *on && !name.trim().is_empty()).map(|(s, _, name)| (s.clone(), name.trim().to_owned())).collect();
        if sites.is_empty() {
            return;
        }
        let job = Job::new();
        let (step, fraction, done, ctx, stack) = (job.step.clone(), job.fraction.clone(), job.done.clone(), self.ctx.clone(), self.projects.stack.clone());
        let _ = std::thread::Builder::new().name("local-import".into()).spawn(move || {
            let result = local_import(&sites, &server, &stack, t, &step, &fraction, &ctx);
            crate::log::info(&match &result {
                Ok(Ended::Local { imported, failed, .. }) => format!("Local's sites imported: {}, not: {}", imported.len(), failed.iter().map(|(n, e)| format!("{n} ({e})")).collect::<Vec<_>>().join(", ")),
                Err(e) => format!("Local's sites not imported: {e}"),
                _ => String::new(),
            });
            *done.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.projects.imports.error = None;
        self.projects.imports.job = Some(job);
    }

    /// Starts loading the .sql file.
    fn sql_import_start(&mut self) {
        let t = self.t();
        let Some(dialog) = &self.projects.imports.sql else { return };
        let Some(server) = dialog.server.clone() else {
            self.projects.imports.error = Some(t.local_no_server.to_owned());
            return;
        };
        let name = dialog.name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let (path, project) = (dialog.path.clone(), dialog.project);
        let job = Job::new();
        let (step, fraction, done, ctx, stack) = (job.step.clone(), job.fraction.clone(), job.done.clone(), self.ctx.clone(), self.projects.stack.clone());
        let _ = std::thread::Builder::new().name("sql-import".into()).spawn(move || {
            let say = |text: String| {
                *step.lock().unwrap() = text;
                ctx.request_repaint();
            };
            let result = (|| {
                say(t.import_step_server.replace("{name}", &server.label("")));
                let (port, stop) = borrow_server(&stack, &server)?;
                say(t.sql_step_load.replace("{file}", &path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                let loaded = crate::db::load_into(port, &name, &path, &fraction, &ctx);
                give_back(&stack, &server, stop);
                loaded.map_err(|e| if e == crate::db::EXISTS { t.sql_exists.replace("{name}", &name) } else { e })?;
                Ok(Ended::Sql { server: server.clone(), port, name: name.clone(), project })
            })();
            crate::log::info(&match &result {
                Ok(_) => format!("{} loaded into {name} ({})", path.display(), server.label("")),
                Err(e) => format!("{} not loaded: {e}", path.display()),
            });
            *done.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.projects.imports.error = None;
        self.projects.imports.job = Some(job);
    }

    /// A connection to one of Ronnie's servers, made unless there is one.
    fn connection_to(&mut self, server: &Db, port: u16) {
        let exists = self.config.databases.iter().any(|c| c.ssh.is_none() && c.port == port && matches!(c.host.as_str(), "127.0.0.1" | "localhost"));
        if !exists {
            let mut connection = config::DbConnection::new();
            connection.name = server.label(self.t().env_system);
            connection.host = "127.0.0.1".into();
            connection.port = port;
            connection.user = "root".into();
            self.config.databases.push(connection);
        }
    }

    /// Collects an import's end: the sites of Local become projects on their server and PHP; a .sql's
    /// project set on its server. A connection to the server, a notice.
    pub(super) fn imports_poll(&mut self) {
        let Some(result) = self.projects.imports.job.as_ref().and_then(|job| job.done.lock().unwrap().take()) else { return };
        self.projects.imports.job = None;
        let t = self.t();
        let ended = match result {
            Ok(ended) => ended,
            Err(e) => {
                self.projects.imports.error = Some(e);
                return;
            }
        };
        match ended {
            Ended::Local { imported, failed, server, port } => {
                if !imported.is_empty() {
                    self.connection_to(&server, port);
                    self.save_config();
                    self.envs.refresh();
                }
                if failed.is_empty() {
                    self.projects.imports.local = None;
                } else {
                    let lines: Vec<String> = failed.iter().map(|(name, e)| format!("{name} : {e}")).collect();
                    self.projects.imports.error = Some(lines.join("\n"));
                    // Those that went through unticked: trying again takes the others only.
                    if let Some(dialog) = &mut self.projects.imports.local {
                        for (_, on, name) in &mut dialog.sites {
                            if imported.iter().any(|n| n == name.trim()) {
                                *on = false;
                            }
                        }
                    }
                }
                if !imported.is_empty() {
                    let body = t.local_done_body.replace("{server}", &server.label(t.env_system));
                    self.push_toast(Toast { ok: true, title: t.local_done.replace("{names}", &imported.join(", ")), body, tab: self.active, at: Instant::now(), mail: None, notice: 0, lead: None });
                }
            }
            Ended::Sql { server, port, name, project } => {
                self.projects.imports.sql = None;
                self.connection_to(&server, port);
                if let Some(project) = project.and_then(|id| self.config.projects.iter_mut().find(|p| p.id == id)) {
                    project.db = Some(server.clone());
                }
                self.save_config();
                self.envs.refresh();
                let title = t.sql_done.replace("{name}", &name);
                let body = t.sql_done_body.replace("{server}", &server.label(t.env_system));
                self.push_toast(Toast { ok: true, title, body, tab: self.active, at: Instant::now(), mail: None, notice: 0, lead: None });
            }
        }
    }

    /// The windows of the imports (and the one naming a new group of projects).
    pub(super) fn imports_windows(&mut self, ctx: &egui::Context) {
        self.project_group_window(ctx);
        self.project_logs_window(ctx);
        let t = self.t();
        let theme = self.theme.clone();
        let running = self.projects.imports.job.as_mut().map(|job| {
            let fraction = *job.fraction.lock().unwrap();
            (job.step.lock().unwrap().clone(), fraction, job.eta.said(fraction, t))
        });
        if running.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let button = |text: &str, fill: Option<Color32>| {
            let b = egui::Button::new(egui::RichText::new(text).size(13.0).color(if fill.is_some() { theme.bg } else { theme.text })).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0));
            match fill {
                Some(fill) => b.fill(fill),
                None => b,
            }
        };
        let all_servers = servers();
        let server_combo = |ui: &mut Ui, id: &str, server: &mut Option<Db>| {
            let text = server.as_ref().map(|db| db.label(t.env_system)).unwrap_or_else(|| t.projects_db_none.to_owned());
            egui::ComboBox::from_id_salt(id).selected_text(text).show_ui(ui, |ui| {
                for db in &all_servers {
                    if ui.selectable_label(server.as_ref() == Some(db), db.label(t.env_system)).clicked() {
                        *server = Some(db.clone());
                    }
                }
            });
        };
        let progress = |ui: &mut Ui, running: &Option<(String, f32, String)>| {
            if let Some((step, fraction, left)) = running {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(13.0));
                    ui.label(egui::RichText::new(step).size(12.5).color(theme.text));
                });
                if *fraction > 0.0 && *fraction < 1.0 {
                    ui.add(egui::ProgressBar::new(*fraction).desired_height(6.0));
                    if !left.is_empty() {
                        ui.label(egui::RichText::new(left).size(11.5).color(theme.text_muted));
                    }
                }
            }
        };
        let (mut go, mut close, mut install) = (false, false, false);

        if let Some(dialog) = &mut self.projects.imports.local {
            let error = self.projects.imports.error.clone();
            egui::Modal::new(egui::Id::new("local-import")).frame(frame).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(egui::RichText::new(t.local_title).size(19.0).strong().color(theme.text));
                ui.add_space(6.0);
                ui.add(egui::Label::new(egui::RichText::new(t.local_body).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(12.0);
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (k, (site, on, name)) in dialog.sites.iter_mut().enumerate() {
                        ui.add_enabled_ui(running.is_none(), |ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(on, egui::RichText::new(&site.name).size(13.5).color(theme.text));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_enabled(*on, egui::TextEdit::singleline(name).id_salt(("local-db-name", k)).desired_width(170.0).font(egui::TextStyle::Monospace));
                                    ui.label(egui::RichText::new("→").color(theme.text_muted));
                                });
                            });
                        });
                        let note = if site.db_runs() {
                            t.local_db_live.to_owned()
                        } else if let Some(dump) = site.dump() {
                            t.local_db_dump.replace("{file}", &dump.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                        } else {
                            t.local_db_none.to_owned()
                        };
                        let warn = !site.db_runs() && site.dump().is_none();
                        ui.horizontal(|ui| {
                            ui.add_space(28.0);
                            ui.add(egui::Label::new(egui::RichText::new(note).size(11.5).color(if warn { theme.ansi[3] } else { theme.text_muted })).wrap());
                        });
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t.local_into).size(13.0).color(theme.text_muted));
                    if all_servers.is_empty() {
                        if ui.link(t.local_install_server).clicked() {
                            install = true;
                        }
                    } else {
                        server_combo(ui, "local-server", &mut dialog.server);
                    }
                });
                if let Some(error) = &error {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(egui::RichText::new(error).size(12.5).color(theme.ansi[1])).wrap());
                }
                progress(ui, &running);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let n = dialog.sites.iter().filter(|(_, on, name)| *on && !name.trim().is_empty()).count();
                    let ready = n > 0 && dialog.server.is_some() && running.is_none();
                    if ui.add_enabled(ready, button(&t.local_go.replace("{n}", &n.to_string()), Some(theme.accent))).clicked() {
                        go = true;
                    }
                    if ui.add_enabled(running.is_none(), button(t.cancel, None)).clicked() {
                        close = true;
                    }
                });
            });
            if go {
                self.local_import_start();
            }
            if close {
                self.projects.imports.local = None;
            }
        } else if let Some(dialog) = &mut self.projects.imports.sql {
            let error = self.projects.imports.error.clone();
            let projects: Vec<(Uuid, String)> = self.config.projects.iter().map(|p| (p.id, p.name.clone())).collect();
            egui::Modal::new(egui::Id::new("sql-import")).frame(frame).show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(egui::RichText::new(t.sql_title).size(19.0).strong().color(theme.text));
                ui.add_space(6.0);
                ui.add(egui::Label::new(egui::RichText::new(t.sql_body).size(13.0).color(theme.text_muted)).wrap());
                ui.add_space(12.0);
                ui.add_enabled_ui(running.is_none(), |ui| {
                    egui::Grid::new("sql-import-grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new(t.sql_file).color(theme.text_muted));
                        ui.add(egui::Label::new(egui::RichText::new(dialog.path.display().to_string()).monospace().size(12.0)).truncate());
                        ui.end_row();
                        ui.label(egui::RichText::new(t.sql_server).color(theme.text_muted));
                        if all_servers.is_empty() {
                            if ui.link(t.local_install_server).clicked() {
                                install = true;
                            }
                        } else {
                            server_combo(ui, "sql-server", &mut dialog.server);
                        }
                        ui.end_row();
                        ui.label(egui::RichText::new(t.sql_name).color(theme.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(240.0));
                        ui.end_row();
                        ui.label(egui::RichText::new(t.sql_project).color(theme.text_muted));
                        let current = dialog.project.and_then(|id| projects.iter().find(|(p, _)| *p == id)).map(|(_, n)| n.clone()).unwrap_or_else(|| t.sql_no_project.to_owned());
                        egui::ComboBox::from_id_salt("sql-project").selected_text(current).show_ui(ui, |ui| {
                            if ui.selectable_label(dialog.project.is_none(), t.sql_no_project).clicked() {
                                dialog.project = None;
                            }
                            for (id, name) in &projects {
                                if ui.selectable_label(dialog.project == Some(*id), name).clicked() {
                                    dialog.project = Some(*id);
                                }
                            }
                        });
                        ui.end_row();
                    });
                });
                if let Some(error) = &error {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(egui::RichText::new(error).size(12.5).color(theme.ansi[1])).wrap());
                }
                progress(ui, &running);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let ready = !dialog.name.trim().is_empty() && dialog.server.is_some() && running.is_none();
                    if ui.add_enabled(ready, button(t.sql_go, Some(theme.accent))).clicked() {
                        go = true;
                    }
                    if ui.add_enabled(running.is_none(), button(t.cancel, None)).clicked() {
                        close = true;
                    }
                });
            });
            if go {
                self.sql_import_start();
            }
            if close {
                self.projects.imports.sql = None;
            }
        }
        if install {
            self.projects.imports.local = None;
            self.projects.imports.sql = None;
            self.open_env_page(DevTool::Mysql);
        }
    }
}

/// Local's databases brought in (off the interface's thread), the server started once for all of
/// them: each site's database read (live from Local if the site runs there, else its latest dump),
/// then loaded under the name chosen (refused if the server has it already). A site that fails
/// doesn't stop the others.
fn local_import(sites: &[(LocalSite, String)], server: &Db, stack: &Arc<Mutex<Stack>>, t: &'static Strings, step: &Arc<Mutex<String>>, fraction: &Arc<Mutex<f32>>, ctx: &egui::Context) -> Result<Ended, String> {
    let say = |text: String| {
        *step.lock().unwrap() = text;
        *fraction.lock().unwrap() = 0.0;
        ctx.request_repaint();
    };
    say(t.import_step_server.replace("{name}", &server.label("")));
    let (port, stop) = borrow_server(stack, server)?;
    let (mut imported, mut failed) = (Vec::new(), Vec::new());
    for (site, database) in sites {
        let result = (|| -> Result<(), String> {
            let temp = std::env::temp_dir().join(format!("ronnie-local-{database}.sql"));
            let dump = if site.db_runs() {
                say(t.local_step_dump.replace("{name}", &site.name));
                crate::db::dump_from(site.db_port.unwrap_or_default(), &site.user, &site.password, &site.database, &temp, fraction, ctx)?;
                temp.clone()
            } else {
                site.dump().ok_or_else(|| t.local_db_none.to_owned())?
            };
            say(t.local_step_load.replace("{name}", &site.name));
            let loaded = crate::db::load_into(port, database, &dump, fraction, ctx);
            let _ = std::fs::remove_file(&temp);
            loaded.map_err(|e| if e == crate::db::EXISTS { t.sql_exists.replace("{name}", database) } else { e })
        })();
        match result {
            Ok(()) => imported.push(database.clone()),
            Err(e) => failed.push((site.name.clone(), e)),
        }
    }
    give_back(stack, server, stop);
    Ok(Ended::Local { imported, failed, server: server.clone(), port })
}

