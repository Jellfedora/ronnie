//! The "My PHP projects" category (see `crate::projects`): the projects in the sidebar; their page, in
//! place of the tabs, shows them as cards (each one's environment: PHP, nginx, its database), the one
//! picked started from there, its settings below.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::dev::DevTool;
use super::envs::{env_color, paint_env_icon};
use super::notes::paint_folder_icon;
use super::sidebar::{paint_panel_title, paint_row_bg};
use super::*;
use crate::envs::{self, Kind as Env};
use crate::projects::{self, Db, Project, Stack, Start};

/// The end of a start (the port of its database server) or a stop, in the background.
type Done = Arc<Mutex<Option<Result<Option<u16>, String>>>>;

/// The end of an import (the project's new folder), in the background.
type Imported = Arc<Mutex<Option<Result<PathBuf, String>>>>;

/// A project being copied into the folder of the projects: its name, how far, how it ended.
struct Importing {
    name: String,
    copied: Arc<projects::Copied>,
    done: Imported,
    eta: crate::eta::Eta,
}

const RUNNING: Color32 = Color32::from_rgb(0x4c, 0xc2, 0x6a);
const STARTING: Color32 = Color32::from_rgb(0xe0, 0xa8, 0x3c);

/// What runs the projects, and what the interface knows of it (the stack is busy while a project
/// starts: the interface never waits for it).
pub(super) struct Projects {
    pub(super) stack: Arc<Mutex<Stack>>,
    /// The project whose page is shown.
    pub selected: Option<Uuid>,
    running: HashSet<Uuid>,
    /// Starting, and whether to open it in the browser once it runs.
    starting: Vec<(Uuid, bool, Done)>,
    stopping: Vec<(Uuid, Done)>,
    /// Why a project didn't start, until it does.
    errors: HashMap<Uuid, String>,
    /// Stopping to be started again (its settings changed while it ran).
    restart_after: HashSet<Uuid>,
    /// The projects being copied into the folder of the projects, by name.
    importing: Vec<Importing>,
    /// A new WordPress being set (its form shown).
    new_wp: Option<NewWordPress>,
    /// The new WordPress sites being made: their name, PHP version and database server.
    creating: Vec<(String, Option<String>, Db, Imported)>,
    /// When the folder of the projects was last read.
    scanned: Option<Instant>,
    /// The port of the database server of each project running.
    db_ports: HashMap<Uuid, u16>,
    /// What each project's folder holds ("WordPress 6.9.7"), read once.
    found: HashMap<Uuid, projects::Found>,
    /// The database servers started by hand (without a project).
    manual_dbs: Vec<Db>,
    /// A database server being started (true) or stopped by hand.
    db_jobs: Vec<(Db, bool, Done)>,
    /// A project's database to open in the view of a connection, once shown.
    db_to_show: Option<(Uuid, String)>,
    /// The projects whose address in their database a start made Ronnie's, to tell.
    fixed: Arc<Mutex<Vec<Uuid>>>,
    /// Local's sites and .sql files being brought in (imports.rs).
    pub(super) imports: super::imports::Imports,
    /// What the page's search field holds.
    search: String,
    /// A project whose new group is being named, and the name so far.
    new_group: Option<(Uuid, String)>,
    /// The window of a project's logs (projlogs.rs), and when each project's were last shown.
    pub(super) logs: Option<super::projlogs::LogsView>,
    pub(super) logs_seen: HashMap<Uuid, std::time::SystemTime>,
    pub(super) logs_since: std::time::SystemTime,
    /// Xdebug being downloaded for a project (xdebug.rs).
    pub(super) xdebug: Option<super::xdebug::XdebugInstall>,
}

/// WordPress's versions to install, once fetched (or why they couldn't be).
type WpVersions = Arc<Mutex<Option<Result<Vec<String>, String>>>>;

/// What a new WordPress is made with, picked before it is.
struct NewWordPress {
    name: String,
    /// WordPress's version (None: the newest, once the list is there).
    version: Option<String>,
    php: Option<String>,
    db: Option<Db>,
    /// WordPress's versions, fetched in the background.
    versions: WpVersions,
}

/// The folder of the projects is read again so often (a folder added in it shows up).
const SCAN_EVERY: Duration = Duration::from_secs(3);

impl Projects {
    pub fn new() -> Self {
        Self {
            stack: Arc::new(Mutex::new(Stack::default())),
            selected: None,
            running: HashSet::new(),
            starting: Vec::new(),
            stopping: Vec::new(),
            errors: HashMap::new(),
            restart_after: HashSet::new(),
            importing: Vec::new(),
            new_wp: None,
            creating: Vec::new(),
            scanned: None,
            db_ports: HashMap::new(),
            found: HashMap::new(),
            manual_dbs: Vec::new(),
            db_jobs: Vec::new(),
            db_to_show: None,
            fixed: Arc::new(Mutex::new(Vec::new())),
            imports: Default::default(),
            search: String::new(),
            new_group: None,
            logs: None,
            logs_seen: HashMap::new(),
            logs_since: std::time::SystemTime::now(),
            xdebug: None,
        }
    }

    fn starting(&self, id: Uuid) -> bool {
        self.starting.iter().any(|(s, _, _)| *s == id)
    }

    fn stopping(&self, id: Uuid) -> bool {
        self.stopping.iter().any(|(s, _)| *s == id)
    }

    fn busy(&self, id: Uuid) -> bool {
        self.starting(id) || self.stopping(id)
    }
}

/// How a project is doing.
#[derive(Clone, Copy, PartialEq)]
enum State {
    Stopped,
    Starting,
    Running,
    Failed,
}

impl App {
    fn project_state(&self, id: Uuid) -> State {
        if self.projects.busy(id) {
            State::Starting
        } else if self.projects.running.contains(&id) {
            State::Running
        } else if self.projects.errors.contains_key(&id) {
            State::Failed
        } else {
            State::Stopped
        }
    }

    pub(super) fn project(&self, id: Uuid) -> Option<&Project> {
        self.config.projects.iter().find(|p| p.id == id)
    }

    /// Collects the starts and stops that finished: a project started opens in the browser if asked,
    /// one that didn't says why.
    pub(super) fn projects_poll(&mut self) {
        self.projects_scan();
        self.db_jobs_poll();
        let fixed: Vec<Uuid> = std::mem::take(&mut *self.projects.fixed.lock().unwrap());
        for id in fixed {
            if let Some(project) = self.project(id) {
                let t = self.t();
                let title = t.projects_address_fixed.replace("{name}", &project.name);
                let body = t.projects_address_fixed_body.replace("{url}", &project.url(self.config.settings.projects_port));
                self.push_toast(Toast { ok: true, title, body, tab: self.active, at: Instant::now(), mail: None, notice: 0, lead: None });
            }
        }
        let t = self.t();
        for (id, open, done) in std::mem::take(&mut self.projects.starting) {
            let result = done.lock().unwrap().take();
            match result {
                None => self.projects.starting.push((id, open, done)),
                Some(Ok(db_port)) => {
                    self.projects.running.insert(id);
                    if let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) {
                        project.started = Some(chrono::Utc::now().timestamp());
                        self.save_config();
                    }
                    if let Some(port) = db_port {
                        self.projects.db_ports.insert(id, port);
                    }
                    self.projects.errors.remove(&id);
                    if open {
                        self.project_open_site(id);
                    }
                }
                Some(Err(e)) => {
                    let name = self.project(id).map(|p| p.name.clone()).unwrap_or_default();
                    self.error = Some(format!("{} : {e}", t.projects_start_failed.replace("{name}", &name)));
                    self.projects.errors.insert(id, e);
                }
            }
        }
        for (id, done) in std::mem::take(&mut self.projects.stopping) {
            if done.lock().unwrap().take().is_some() {
                self.projects.running.remove(&id);
                self.projects.db_ports.remove(&id);
            } else {
                self.projects.stopping.push((id, done));
            }
        }
        self.project_xdebug_poll();
        for job in std::mem::take(&mut self.projects.importing) {
            let result = job.done.lock().unwrap().take();
            let name = job.name.clone();
            match result {
                None => self.projects.importing.push(job),
                Some(Ok(folder)) => {
                    self.projects.scanned = None;
                    self.projects_scan();
                    if let Some(project) = self.config.projects.iter().find(|p| p.path == folder) {
                        self.projects.selected = Some(project.id);
                    }
                }
                Some(Err(e)) => self.error = Some(format!("{} : {e}", t.projects_import_failed.replace("{name}", &name))),
            }
        }
        for (name, php, db, done) in std::mem::take(&mut self.projects.creating) {
            let result = done.lock().unwrap().take();
            match result {
                None => self.projects.creating.push((name, php, db, done)),
                // On the PHP and database picked, started and opened in the browser: on WordPress's
                // install page.
                Some(Ok(folder)) => {
                    self.projects.scanned = None;
                    self.projects_scan();
                    if let Some(project) = self.config.projects.iter_mut().find(|p| p.path == folder) {
                        project.php = php.or(project.php.take());
                        project.db = Some(db);
                        let id = project.id;
                        self.save_config();
                        self.projects.selected = Some(id);
                        self.project_start(id, true);
                    }
                }
                Some(Err(e)) => self.error = Some(format!("{} : {e}", t.projects_wp_failed.replace("{name}", &name))),
            }
        }
    }

    /// The form of a new WordPress shown: the newest PHP and database server installed picked, the
    /// list of WordPress's versions fetched.
    fn projects_new_wordpress_form(&mut self) {
        let versions = Arc::new(Mutex::new(None));
        let (slot, ctx) = (versions.clone(), self.ctx.clone());
        let _ = std::thread::Builder::new().name("wordpress-versions".into()).spawn(move || {
            *slot.lock().unwrap() = Some(projects::wordpress_versions().map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        let php = self.envs.installed(Env::Php).first().cloned();
        self.projects.new_wp = Some(NewWordPress { name: String::new(), version: None, php, db: projects::newest_db(), versions });
    }

    /// A new WordPress made in the background in the folder of the projects (see
    /// `projects::new_wordpress`), as picked in its form: then started, its install page opened.
    fn projects_new_wordpress(&mut self, name: String, version: String, php: Option<String>, db: Option<Db>) {
        let t = self.t();
        let name = name.trim().to_owned();
        let Some(dir) = self.projects_dir() else { return };
        if self.read_only || self.projects.creating.iter().any(|(n, ..)| *n == name) {
            return;
        }
        if !projects::valid_site_name(&name) || dir.join(&name).exists() {
            self.error = Some(t.projects_wp_bad_name.replace("{name}", &name));
            return;
        }
        let Some(db) = db else {
            self.error = Some(t.projects_wp_no_db.to_owned());
            return self.open_env_page(DevTool::Mysql);
        };
        let french = self.config.settings.language == crate::i18n::Lang::Fr;
        let done: Imported = Arc::new(Mutex::new(None));
        let (stack, slot, ctx, folder_name, server) = (self.projects.stack.clone(), done.clone(), self.ctx.clone(), name.clone(), db.clone());
        let _ = std::thread::Builder::new().name("new-wordpress".into()).spawn(move || {
            let result = projects::new_wordpress(&stack, &dir, &folder_name, &version, &server, french).map_err(|e| format!("{e:#}"));
            crate::log::info(&match &result {
                Ok(_) => format!("WordPress {folder_name} made"),
                Err(e) => format!("WordPress {folder_name} not made: {e}"),
            });
            *slot.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.projects.creating.push((name, php, db, done));
        self.projects.new_wp = None;
    }

    /// Starts a project in the background (its PHP, nginx and database installed first, or the way to
    /// install them shown); `open`: in the browser once it runs.
    pub(super) fn project_start(&mut self, id: Uuid, open: bool) {
        if self.projects.busy(id) || self.read_only {
            return;
        }
        let t = self.t();
        let Some(project) = self.project(id).cloned() else { return };
        let Some(php) = project.php_version() else {
            self.error = Some(t.projects_no_php.to_owned());
            return self.open_env_page(DevTool::Php);
        };
        let Some(nginx) = self.envs.installed(Env::Nginx).first().cloned() else {
            self.error = Some(match envs::system_only(Env::Nginx) {
                Some(how) => t.env_system_missing.replace("{name}", Env::Nginx.name()).replace("{how}", how),
                None => t.projects_no_nginx.to_owned(),
            });
            return self.open_env_page(DevTool::Nginx);
        };
        let db = project.db_server();
        if let (Some(wanted), None) = (&project.db, &db) {
            self.error = Some(t.projects_no_db.replace("{name}", &wanted.label(t.env_system)));
            return self.open_env_page(if wanted.kind == Env::Mariadb { DevTool::Mariadb } else { DevTool::Mysql });
        }
        let start = Start { project, php, nginx, db, port: self.config.settings.projects_port, mail_port: self.config.settings.mail_port };
        let done: Done = Arc::new(Mutex::new(None));
        let (stack, slot, ctx, fixed) = (self.projects.stack.clone(), done.clone(), self.ctx.clone(), self.projects.fixed.clone());
        let _ = std::thread::Builder::new().name("project-start".into()).spawn(move || {
            let result = {
                let mut stack = stack.lock().unwrap();
                let result = stack.start(&start).map_err(|e| format!("{e:#}"));
                if let Some(id) = stack.fixed.take() {
                    fixed.lock().unwrap().push(id);
                }
                result
            };
            crate::log::info(&match &result {
                Ok(_) => format!("project {} started", start.project.name),
                Err(e) => format!("project {} not started: {e}", start.project.name),
            });
            *slot.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.projects.starting.push((id, open, done));
    }

    pub(super) fn project_stop(&mut self, id: Uuid) {
        if self.projects.busy(id) || !self.projects.running.contains(&id) {
            return;
        }
        let done: Done = Arc::new(Mutex::new(None));
        let (stack, slot, ctx) = (self.projects.stack.clone(), done.clone(), self.ctx.clone());
        let _ = std::thread::Builder::new().name("project-stop".into()).spawn(move || {
            stack.lock().unwrap().stop(id);
            *slot.lock().unwrap() = Some(Ok(None));
            ctx.request_repaint();
        });
        self.projects.stopping.push((id, done));
    }

    /// Ronnie's database server behind a connection (on this computer, on the port of one of
    /// Ronnie's servers with data), and how it is doing.
    pub(super) fn local_db_of(&self, conn: &crate::config::DbConnection) -> Option<(Db, super::dbview::LocalServer)> {
        use super::dbview::LocalServer;
        if conn.ssh.is_some() || conn.engine != crate::config::Engine::Mysql || !matches!(conn.host.as_str(), "127.0.0.1" | "localhost") {
            return None;
        }
        let (db, _) = projects::local_servers().into_iter().find(|(_, port)| *port == conn.port)?;
        let busy = self.projects.db_jobs.iter().any(|(d, _, _)| projects::same_data(d, &db))
            || self.config.projects.iter().any(|p| self.projects.busy(p.id) && p.db.as_ref().is_some_and(|d| projects::same_data(d, &db)));
        let manual = self.projects.manual_dbs.iter().any(|d| projects::same_data(d, &db));
        let for_project = self.config.projects.iter().any(|p| self.projects.running.contains(&p.id) && p.db.as_ref().is_some_and(|d| projects::same_data(d, &db)));
        let state = if busy {
            LocalServer::Busy
        } else if manual || for_project {
            LocalServer::Running { manual }
        } else {
            LocalServer::Stopped
        };
        Some((db, state))
    }

    /// A database server of Ronnie's started by hand, in the background (its tabs connected again once
    /// it runs).
    pub(super) fn db_server_start(&mut self, db: Db) {
        if self.read_only || self.projects.db_jobs.iter().any(|(d, _, _)| projects::same_data(d, &db)) {
            return;
        }
        let done: Done = Arc::new(Mutex::new(None));
        let (stack, slot, ctx, server) = (self.projects.stack.clone(), done.clone(), self.ctx.clone(), db.clone());
        let _ = std::thread::Builder::new().name("db-start".into()).spawn(move || {
            let result = stack.lock().unwrap().start_db_alone(&server).map(Some).map_err(|e| format!("{e:#}"));
            crate::log::info(&match &result {
                Ok(_) => format!("{} started by hand", server.label("")),
                Err(e) => format!("{} not started: {e}", server.label("")),
            });
            *slot.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.projects.db_jobs.push((db, true, done));
    }

    /// A database server started by hand stopped (unless a project started uses it).
    pub(super) fn db_server_stop(&mut self, db: Db) {
        if self.projects.db_jobs.iter().any(|(d, _, _)| projects::same_data(d, &db)) {
            return;
        }
        let done: Done = Arc::new(Mutex::new(None));
        let (stack, slot, ctx, server) = (self.projects.stack.clone(), done.clone(), self.ctx.clone(), db.clone());
        let _ = std::thread::Builder::new().name("db-stop".into()).spawn(move || {
            stack.lock().unwrap().stop_db_alone(&server);
            *slot.lock().unwrap() = Some(Ok(None));
            ctx.request_repaint();
        });
        self.projects.db_jobs.push((db, false, done));
    }

    /// The starts and stops by hand that finished: the tabs of a server started connected again.
    fn db_jobs_poll(&mut self) {
        let t = self.t();
        for (db, start, done) in std::mem::take(&mut self.projects.db_jobs) {
            let result = done.lock().unwrap().take();
            match result {
                None => self.projects.db_jobs.push((db, start, done)),
                Some(Ok(port)) if start => {
                    if !self.projects.manual_dbs.iter().any(|d| projects::same_data(d, &db)) {
                        self.projects.manual_dbs.push(db);
                    }
                    let port = port.unwrap_or_default();
                    let ids: Vec<Uuid> = self.config.databases.iter().filter(|c| c.ssh.is_none() && c.port == port && matches!(c.host.as_str(), "127.0.0.1" | "localhost")).map(|c| c.id).collect();
                    let ctx = self.ctx.clone();
                    for tab in &mut self.tabs {
                        if let (Some(id), Some(view)) = (tab.db, tab.db_view.as_mut())
                            && ids.contains(&id)
                            && view.closed()
                        {
                            view.reconnect(&ctx);
                        }
                    }
                }
                Some(Ok(_)) => self.projects.manual_dbs.retain(|d| !projects::same_data(d, &db)),
                Some(Err(e)) => self.error = Some(format!("{} : {e}", t.db_server_failed.replace("{name}", &db.label(t.env_system)))),
            }
        }
    }

    /// A project's database opened in Ronnie: the tab of its server (a connection made for it if
    /// there is none), the server started if it doesn't run, its database opened once connected.
    fn project_open_db(&mut self, id: Uuid) {
        let t = self.t();
        let Some(project) = self.project(id).cloned() else { return };
        let Some(db) = project.db.clone() else { return };
        let Some((server, port)) = projects::local_servers().into_iter().find(|(d, _)| projects::same_data(d, &db)) else {
            self.error = Some(t.projects_no_db.replace("{name}", &db.label(t.env_system)));
            return;
        };
        let local = |c: &crate::config::DbConnection| c.ssh.is_none() && c.engine == crate::config::Engine::Mysql && c.port == port && matches!(c.host.as_str(), "127.0.0.1" | "localhost");
        let conn_id = match self.config.databases.iter().find(|c| local(c)) {
            Some(c) => c.id,
            None => {
                let mut connection = crate::config::DbConnection::new();
                connection.name = server.label(t.env_system);
                connection.host = "127.0.0.1".into();
                connection.port = port;
                connection.user = "root".into();
                let id = connection.id;
                self.config.databases.push(connection);
                self.save_config();
                id
            }
        };
        if let Some(conn) = self.config.databases.iter().find(|c| c.id == conn_id).cloned()
            && matches!(self.local_db_of(&conn), Some((_, super::dbview::LocalServer::Stopped)))
        {
            self.db_server_start(server);
        }
        self.dev_page = false;
        self.open_db(conn_id);
        if let Some(name) = self.project_found(&project).database.clone() {
            self.projects.db_to_show = Some((conn_id, name));
        }
    }

    /// The database to open in the view of connection `id`, asked from a project's card.
    pub(super) fn take_db_to_show(&mut self, id: Uuid) -> Option<String> {
        match &self.projects.db_to_show {
            Some((conn, _)) if *conn == id => self.projects.db_to_show.take().map(|(_, name)| name),
            _ => None,
        }
    }

    /// Started again (its settings changed while it ran).
    pub(super) fn project_restart(&mut self, id: Uuid) {
        if !self.projects.running.contains(&id) || self.projects.busy(id) {
            return;
        }
        let done: Done = Arc::new(Mutex::new(None));
        let (stack, slot, ctx) = (self.projects.stack.clone(), done.clone(), self.ctx.clone());
        let _ = std::thread::Builder::new().name("project-stop".into()).spawn(move || {
            stack.lock().unwrap().stop(id);
            *slot.lock().unwrap() = Some(Ok(None));
            ctx.request_repaint();
        });
        self.projects.running.remove(&id);
        self.projects.stopping.push((id, done));
        self.projects.restart_after.insert(id);
    }

    pub(super) fn project_open_site(&mut self, id: Uuid) {
        if let Some(project) = self.project(id) {
            crate::terminal::open_url(&project.url(self.config.settings.projects_port));
        }
    }

    /// A click on a project: picked, its card shown (started from there).
    fn project_clicked(&mut self, id: Uuid) {
        self.projects.selected = Some(id);
        self.open_projects_page();
    }

    /// What a project's folder holds, read once.
    fn project_found(&mut self, project: &Project) -> &projects::Found {
        self.projects.found.entry(project.id).or_insert_with(|| projects::detect(&project.path))
    }

    /// The folder of the projects: the one picked, or `Documents/Ronnie/Projets`.
    fn projects_dir(&self) -> Option<PathBuf> {
        self.config.settings.projects_dir.clone().or_else(projects::default_dir)
    }

    /// A project from elsewhere (in WAMP's `www`…) picked, copied into the folder of the projects in the
    /// background; the original stays where it is.
    fn projects_import(&mut self) {
        let Some(dir) = self.projects_dir() else { return };
        let mut dialog = rfd::FileDialog::new();
        if let Some(start) = Some(PathBuf::from(r"C:\wamp64\www")).filter(|d| d.is_dir()).or_else(|| directories::UserDirs::new().map(|d| d.home_dir().to_path_buf())) {
            dialog = dialog.set_directory(start);
        }
        let Some(folder) = dialog.pick_folder() else { return };
        // Already in it: only shown.
        if folder.parent() != Some(dir.as_path()) {
            let _ = std::fs::create_dir_all(&dir);
            let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let done: Imported = Arc::new(Mutex::new(None));
            let copied = Arc::new(projects::Copied::default());
            let (slot, count, ctx) = (done.clone(), copied.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                *slot.lock().unwrap() = Some(projects::import(&folder, &dir, &count).map_err(|e| format!("{e:#}")));
                ctx.request_repaint();
            });
            self.projects.importing.push(Importing { name, copied, done, eta: Default::default() });
        }
        self.open_projects_page();
    }

    /// The folder of the projects picked (anywhere on the computer): its folders become the projects.
    fn projects_pick_dir(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = self.projects_dir().filter(|d| d.is_dir()).or_else(|| directories::UserDirs::new().map(|d| d.home_dir().to_path_buf())) {
            dialog = dialog.set_directory(dir);
        }
        let Some(folder) = dialog.pick_folder() else { return };
        self.config.settings.projects_dir = Some(folder);
        self.save_config();
        self.projects.scanned = None;
        self.projects_scan();
        self.open_projects_page();
    }

    /// The folder of the projects read (every `SCAN_EVERY`, made if it is the usual one and missing): the
    /// projects are its folders only. A folder new in it becomes a project; the project of a folder
    /// gone from it, or out of it (another folder picked), leaves the list unless it runs.
    fn projects_scan(&mut self) {
        if self.read_only || self.projects.scanned.is_some_and(|at| at.elapsed() < SCAN_EVERY) {
            return;
        }
        self.projects.scanned = Some(Instant::now());
        let Some(dir) = self.projects_dir() else { return };
        if self.config.settings.projects_dir.is_none() && !dir.is_dir() {
            let _ = std::fs::create_dir_all(&dir);
        }
        let folders = projects::folders_in(&dir);
        let before = self.config.projects.len();
        let running = &self.projects.running;
        self.config.projects.retain(|p| folders.contains(&p.path) || running.contains(&p.id));
        let mut changed = self.config.projects.len() != before;
        for folder in folders {
            if !self.config.projects.iter().any(|p| p.path == folder) {
                self.config.projects.push(Project::of_folder(&folder));
                changed = true;
            }
        }
        if changed {
            self.config.projects.sort_by_key(|p| p.name.to_lowercase());
            self.save_config();
        }
    }

    /// The user's groups of projects, by name.
    fn project_groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self.config.projects.iter().map(|p| p.group.trim().to_owned()).filter(|g| !g.is_empty()).collect();
        groups.sort_by_key(|g| g.to_lowercase());
        groups.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        groups
    }

    /// The projects as shown, sorted (the favorites first) under their titles (none when they aren't
    /// grouped); `search`: only those matching the page's search field.
    fn projects_shown(&self, search: bool) -> Vec<(Option<String>, Vec<Project>)> {
        let t = self.t();
        let query = if search { self.projects.search.trim().to_lowercase() } else { String::new() };
        let matches = |p: &Project| {
            query.is_empty() || {
                let label = self.projects.found.get(&p.id).and_then(|f| f.label.clone()).unwrap_or_default();
                [p.name.as_str(), &p.slug(), p.kind.name(), &label, &p.group].iter().any(|text| text.to_lowercase().contains(&query))
            }
        };
        let mut list: Vec<Project> = self.config.projects.iter().filter(|p| matches(p)).cloned().collect();
        let kind_rank = |p: &Project| projects::Kind::ALL.iter().position(|k| *k == p.kind).unwrap_or(9);
        let running = |p: &Project| !(self.projects.running.contains(&p.id) || self.projects.busy(p.id));
        list.sort_by(|a, b| {
            let then = match self.config.settings.projects_sort {
                projects::Sort::Name => std::cmp::Ordering::Equal,
                projects::Sort::Kind => kind_rank(a).cmp(&kind_rank(b)),
                projects::Sort::Recent => b.started.cmp(&a.started),
                projects::Sort::Running => running(a).cmp(&running(b)),
            };
            b.favorite.cmp(&a.favorite).then(then).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        match self.config.settings.projects_grouping {
            projects::Grouping::None => vec![(None, list)],
            projects::Grouping::Kind => projects::Kind::ALL
                .iter()
                .map(|kind| (Some(kind.name().to_owned()), list.iter().filter(|p| p.kind == *kind).cloned().collect::<Vec<_>>()))
                .filter(|(_, l)| !l.is_empty())
                .collect(),
            projects::Grouping::Mine => {
                let mut sections: Vec<(Option<String>, Vec<Project>)> = self
                    .project_groups()
                    .into_iter()
                    .map(|g| {
                        let inside = list.iter().filter(|p| p.group.trim().eq_ignore_ascii_case(&g)).cloned().collect::<Vec<_>>();
                        (Some(g), inside)
                    })
                    .filter(|(_, l)| !l.is_empty())
                    .collect();
                let rest: Vec<Project> = list.iter().filter(|p| p.group.trim().is_empty()).cloned().collect();
                if !rest.is_empty() {
                    sections.push((Some(t.projects_no_group.to_owned()), rest));
                }
                sections
            }
        }
    }

    fn project_set_favorite(&mut self, id: Uuid, favorite: bool) {
        if let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) {
            project.favorite = favorite;
            self.save_config();
        }
    }

    /// A project put in group `group` ("": none); shown grouped by the user's groups once it has one.
    fn project_set_group(&mut self, id: Uuid, group: &str) {
        if let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) {
            project.group = group.trim().to_owned();
            if !project.group.is_empty() {
                self.config.settings.projects_grouping = projects::Grouping::Mine;
            }
            self.save_config();
        }
    }

    /// The window naming a project's new group.
    pub(super) fn project_group_window(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some((id, name)) = &mut self.projects.new_group else { return };
        let id = *id;
        let (mut create, mut close) = (false, false);
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let modal = egui::Modal::new(egui::Id::new("project-new-group")).frame(frame).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(egui::RichText::new(t.projects_group_new_title).size(17.0).strong().color(theme.text));
            ui.add_space(10.0);
            let resp = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY).hint_text(t.projects_group_name_hint));
            resp.request_focus();
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(t.projects_group_create).corner_radius(6.0)).clicked() {
                    create = true;
                }
                if ui.button(t.cancel).clicked() {
                    close = true;
                }
            });
        });
        if modal.should_close() {
            close = true;
        }
        if create && !name.trim().is_empty() {
            let name = name.clone();
            self.projects.new_group = None;
            self.project_set_group(id, &name);
        } else if close {
            self.projects.new_group = None;
        }
    }

    /// The favorite and group items of a project's menu.
    fn project_menu_extra(&mut self, ui: &mut Ui, project: &Project, groups: &[String]) {
        let t = self.t();
        let star = if project.favorite { t.projects_unfavorite } else { t.projects_favorite };
        if ui.button(star).clicked() {
            self.project_set_favorite(project.id, !project.favorite);
            ui.close();
        }
        ui.menu_button(t.projects_group, |ui| {
            for group in groups {
                if ui.selectable_label(project.group.trim().eq_ignore_ascii_case(group), group).clicked() {
                    self.project_set_group(project.id, group);
                    ui.close();
                }
            }
            if !project.group.is_empty() && ui.button(t.projects_no_group).clicked() {
                self.project_set_group(project.id, "");
                ui.close();
            }
            if !groups.is_empty() {
                ui.separator();
            }
            if ui.button(t.projects_group_new).clicked() {
                self.projects.new_group = Some((project.id, String::new()));
                ui.close();
            }
        });
    }

    /// The page of the project selected (or of the first one), in place of the tabs.
    pub(super) fn open_projects_page(&mut self) {
        if self.projects.selected.is_none_or(|id| self.project(id).is_none()) {
            self.projects.selected = self.config.projects.first().map(|p| p.id);
        }
        self.dev.tool = DevTool::Project;
        self.open_dev_page();
    }

    /// The page of an environment of the Dev category, its versions fetched.
    pub(super) fn open_env_page(&mut self, tool: DevTool) {
        self.dev.tool = tool;
        self.open_dev_page();
    }

    /// Stops every project (Ronnie quits).
    pub(super) fn projects_stop_all(&mut self) {
        if let Ok(mut stack) = self.projects.stack.lock() {
            stack.stop_all();
        }
    }

    /// The "My projects" category: its projects, one a row, their state; + imports some, the folder
    /// icon picks the folder of the projects.
    pub(super) fn projects_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        if self.panel_part.head() {
            let head = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H));
            paint_panel_title(&painter, head, t.cat_projects, &theme);
            let plus = Rect::from_center_size(Pos2::new(head.max.x - 14.0, head.center().y), Vec2::splat(22.0));
            let plus_resp = ui.interact(plus, ui.id().with("projects-import"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.projects_import_hint);
            if plus_resp.hovered() {
                painter.rect_filled(plus, 5.0, theme.tab_hover);
            }
            painter.text(plus.center(), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), if plus_resp.hovered() { theme.text } else { theme.text_muted });
            let folder = plus.translate(Vec2::new(-26.0, 0.0));
            let folder_tip = match self.projects_dir() {
                Some(dir) => t.projects_dir.replace("{path}", &dir.display().to_string()),
                None => t.projects_dir_pick.to_owned(),
            };
            let folder_resp = ui.interact(folder, ui.id().with("projects-dir"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(folder_tip);
            if folder_resp.hovered() {
                painter.rect_filled(folder, 5.0, theme.tab_hover);
            }
            paint_folder_icon(&painter, folder.center(), if folder_resp.hovered() { theme.text } else { theme.text_muted });
            *y += SECTION_HEADER_H;
            if plus_resp.clicked() {
                self.projects_import();
            }
            if folder_resp.clicked() {
                self.projects_pick_dir();
            }
        }
        if !self.panel_part.body() {
            return;
        }
        if self.config.projects.is_empty() {
            let galley = painter.layout(t.projects_empty_short.to_owned(), FontId::proportional(12.0), theme.text_muted, row_w - 16.0);
            painter.galley(Pos2::new(left + 8.0, *y + 4.0), galley.clone(), theme.text_muted);
            *y += galley.size().y + 12.0;
            return;
        }
        let shown_page = self.dev_page && self.dev.tool == DevTool::Project;
        let (mut clicked, mut menu_action) = (None, None);
        let groups = self.project_groups();
        let rows: Vec<(Option<String>, Project)> = self
            .projects_shown(false)
            .into_iter()
            .flat_map(|(title, list)| list.into_iter().enumerate().map(move |(k, p)| (if k == 0 { title.clone() } else { None }, p)))
            .collect();
        for (title, project) in rows {
            if let Some(title) = title {
                painter.text(Pos2::new(left + 10.0, *y + 10.0), Align2::LEFT_CENTER, title.to_uppercase(), FontId::proportional(10.5), theme.text_muted.gamma_multiply(0.8));
                *y += 20.0;
            }
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 8.0));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("project", project.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(project.url(self.config.settings.projects_port));
            let active = shown_page && self.projects.selected == Some(project.id);
            paint_row_bg(&painter, slot, active, resp.hovered(), &theme);
            let state = self.project_state(project.id);
            let dot = Pos2::new(slot.min.x + 15.0, slot.center().y);
            match state {
                State::Running => painter.circle_filled(dot, 4.5, Color32::from_rgb(0x4c, 0xc2, 0x6a)),
                State::Starting => painter.circle_stroke(dot, 4.0, Stroke::new(1.6, Color32::from_rgb(0xe0, 0xa8, 0x3c))),
                State::Failed => painter.circle_filled(dot, 4.5, theme.ansi[1]),
                State::Stopped => painter.circle_stroke(dot, 4.0, Stroke::new(1.3, theme.text_muted.gamma_multiply(0.7))),
            };
            if state == State::Starting {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            }
            painter.text(Pos2::new(slot.min.x + 30.0, slot.center().y), Align2::LEFT_CENTER, &project.name, FontId::proportional(13.0), if active || resp.hovered() { theme.text } else { theme.text_muted });
            let note = match project.kind {
                projects::Kind::Php => project.php_version().map(|v| format!("PHP {}", envs::line(&v))).unwrap_or_default(),
                kind => kind.name().to_owned(),
            };
            let mut note_x = slot.max.x - 10.0;
            if project.favorite {
                paint_star(&painter, Pos2::new(note_x - 5.0, slot.center().y), 5.0, true, STAR);
                note_x -= 16.0;
            }
            painter.text(Pos2::new(note_x, slot.center().y), Align2::RIGHT_CENTER, note, FontId::proportional(11.0), theme.text_muted.gamma_multiply(0.8));
            if resp.clicked() {
                clicked = Some(project.id);
            }
            resp.context_menu(|ui| {
                self.project_menu_extra(ui, &project, &groups);
                ui.separator();
                let running = state == State::Running;
                menu_item(ui, t.projects_open_site, (project.id, Menu::Open), &mut menu_action);
                if running {
                    menu_item(ui, t.projects_stop, (project.id, Menu::Stop), &mut menu_action);
                } else if state != State::Starting {
                    menu_item(ui, t.projects_start, (project.id, Menu::Start), &mut menu_action);
                }
                ui.separator();
                menu_item(ui, t.projects_terminal, (project.id, Menu::Terminal), &mut menu_action);
                menu_item(ui, t.projects_folder, (project.id, Menu::Folder), &mut menu_action);
                if project.db.is_some() {
                    menu_item(ui, t.projects_open_db, (project.id, Menu::Database), &mut menu_action);
                }
                menu_item(ui, t.projects_settings, (project.id, Menu::Settings), &mut menu_action);
            });
        }
        if let Some(id) = clicked {
            self.project_clicked(id);
        }
        if let Some((id, action)) = menu_action {
            self.project_menu(id, action);
        }
    }

    fn project_menu(&mut self, id: Uuid, action: Menu) {
        match action {
            Menu::Open if self.projects.running.contains(&id) => self.project_open_site(id),
            Menu::Open | Menu::Start => self.project_start(id, action == Menu::Open),
            Menu::Stop => self.project_stop(id),
            Menu::Terminal => {
                if let Some(path) = self.project(id).map(|p| p.path.clone()) {
                    let ctx = self.ctx.clone();
                    self.dev_page = false;
                    self.new_tab_in(&ctx, Some(&path));
                }
            }
            Menu::Folder => {
                if let Some(path) = self.project(id).map(|p| p.path.clone()) {
                    config::open_folder(&path);
                }
            }
            Menu::Settings => {
                self.projects.selected = Some(id);
                self.open_projects_page();
            }
            Menu::Database => self.project_open_db(id),
            Menu::Logs => self.project_logs_open(id),
            // Only the page asks these, and handles them itself.
            Menu::Php | Menu::Db | Menu::Restart | Menu::ImportSql => {}
        }
    }

    /// The page of the projects: a card each (its environment, its state), the one picked started
    /// from its card, its settings below.
    pub(super) fn project_page_ui(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        let dir = self.projects_dir();
        let (mut action, mut import, mut pick_dir, mut new_wp, mut open_wp, mut wp_more) = (None, false, false, None, false, None);
        let groups = self.project_groups();
        egui::ScrollArea::vertical().id_salt("project-page").auto_shrink([false, false]).show(ui, |ui| {
            // Title, the folder of the projects (picked, opened); projects imported into it.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.cat_projects).size(24.0).color(theme.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = |text: String| egui::Button::new(egui::RichText::new(text).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0));
                    if ui.add(button(format!("+  {}", t.projects_import))).on_hover_text(t.projects_import_hint).clicked() {
                        import = true;
                    }
                    if ui.add(button(format!("+  {}", t.projects_wp_new))).on_hover_text(t.projects_wp_hint).clicked() && !self.read_only && self.projects.new_wp.is_none() {
                        open_wp = true;
                    }
                    if ui.add(button(t.projects_dir_pick.to_owned())).clicked() {
                        pick_dir = true;
                    }
                    if let Some(dir) = &dir
                        && ui.add(button(format!("📁  {}", t.projects_dir_open))).clicked()
                    {
                        let _ = std::fs::create_dir_all(dir);
                        config::open_folder(dir);
                    }
                });
            });
            if let Some(dir) = &dir {
                ui.label(egui::RichText::new(t.projects_dir_line.replace("{path}", &dir.display().to_string())).size(12.0).color(theme.text_muted));
            }
            // A new WordPress: its name, WordPress's version, its PHP and database server picked.
            let mut cancel = false;
            let php_installed = self.envs.installed(Env::Php).to_vec();
            let servers: Vec<Db> = Env::DATABASES.into_iter().flat_map(|kind| self.envs.installed(kind).iter().map(move |version| Db { kind, version: version.clone() })).collect();
            if let Some(form) = &mut self.projects.new_wp {
                let versions = form.versions.lock().unwrap().clone();
                if form.version.is_none()
                    && let Some(Ok(list)) = &versions
                {
                    form.version = list.first().cloned();
                }
                ui.add_space(10.0);
                Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
                    egui::Grid::new("new-wordpress").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new(t.projects_wp_name).color(theme.text_muted));
                        let field = ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(240.0).hint_text("mon-site"));
                        if !field.has_focus() && form.name.is_empty() {
                            field.request_focus();
                        }
                        ui.end_row();

                        ui.label(egui::RichText::new("WordPress").color(theme.text_muted));
                        match &versions {
                            None => {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new().size(12.0));
                                    ui.label(egui::RichText::new(t.node_loading).color(theme.text_muted));
                                });
                            }
                            Some(Err(e)) => {
                                ui.label(egui::RichText::new(e).size(12.0).color(theme.ansi[1]));
                            }
                            Some(Ok(list)) => {
                                let shown = |v: &String| if list.first() == Some(v) { format!("{v}  ({})", t.projects_wp_latest) } else { v.clone() };
                                egui::ComboBox::from_id_salt("new-wp-version").selected_text(form.version.as_ref().map(shown).unwrap_or_default()).show_ui(ui, |ui| {
                                    for v in list {
                                        ui.selectable_value(&mut form.version, Some(v.clone()), shown(v));
                                    }
                                });
                            }
                        }
                        ui.end_row();

                        ui.label(egui::RichText::new("PHP").color(theme.text_muted));
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("new-wp-php").selected_text(form.php.clone().unwrap_or_else(|| t.projects_php_none.to_owned())).show_ui(ui, |ui| {
                                for v in &php_installed {
                                    ui.selectable_value(&mut form.php, Some(v.clone()), v);
                                }
                            });
                            if ui.link(t.projects_php_more).clicked() {
                                wp_more = Some(DevTool::Php);
                            }
                        });
                        ui.end_row();

                        ui.label(egui::RichText::new(t.projects_db_server).color(theme.text_muted));
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("new-wp-db").selected_text(form.db.as_ref().map(|db| db.label(t.env_system)).unwrap_or_else(|| t.projects_php_none.to_owned())).show_ui(ui, |ui| {
                                for db in &servers {
                                    ui.selectable_value(&mut form.db, Some(db.clone()), db.label(t.env_system));
                                }
                            });
                            if ui.link(t.projects_db_more).clicked() {
                                wp_more = Some(DevTool::Mysql);
                            }
                        });
                        ui.end_row();
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let ok = projects::valid_site_name(&form.name) && form.version.is_some() && form.db.is_some();
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui.add_enabled(ok, egui::Button::new(format!("  {}  ", t.projects_wp_create))).clicked() || enter) && ok {
                            new_wp = Some((form.name.clone(), form.version.clone().unwrap_or_default(), form.php.clone(), form.db.clone()));
                        }
                        if ui.button(t.cancel).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel = true;
                        }
                    });
                });
            }
            if cancel {
                self.projects.new_wp = None;
            }
            for (name, ..) in &self.projects.creating {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(egui::RichText::new(t.projects_wp_creating.replace("{name}", name)).size(12.0).color(theme.text_muted));
                });
            }
            if !self.projects.importing.is_empty() {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
            for job in &mut self.projects.importing {
                let fraction = job.copied.fraction();
                let left = fraction.map(|f| job.eta.said(f, t)).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    let mut text = t.projects_importing.replace("{name}", &job.name);
                    if let Some(f) = fraction {
                        text += &format!("  ·  {:.0} %", f * 100.0);
                    }
                    if !left.is_empty() {
                        text += &format!("  ·  {left}");
                    }
                    ui.label(egui::RichText::new(text).size(12.0).color(theme.text_muted));
                });
                if let Some(f) = fraction {
                    ui.add(egui::ProgressBar::new(f).desired_width(320.0).desired_height(5.0).fill(theme.accent));
                }
            }
            ui.add_space(14.0);
            if self.config.projects.is_empty() {
                ui.label(egui::RichText::new(t.projects_empty).size(13.0).color(theme.text_muted));
                return;
            }

            // Search, sort, grouping.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.add(egui::TextEdit::singleline(&mut self.projects.search).desired_width(220.0).hint_text(format!("🔍  {}", t.projects_search_hint)));
                if !self.projects.search.is_empty() && ui.small_button("✕").clicked() {
                    self.projects.search.clear();
                }
                ui.add_space(8.0);
                let (mut sort, mut grouping) = (self.config.settings.projects_sort, self.config.settings.projects_grouping);
                ui.label(egui::RichText::new(t.projects_sort).size(12.5).color(theme.text_muted));
                egui::ComboBox::from_id_salt("projects-sort").selected_text(sort_name(sort, t)).show_ui(ui, |ui| {
                    for s in projects::Sort::ALL {
                        ui.selectable_value(&mut sort, s, sort_name(s, t));
                    }
                });
                ui.label(egui::RichText::new(t.projects_grouping).size(12.5).color(theme.text_muted));
                egui::ComboBox::from_id_salt("projects-grouping").selected_text(grouping_name(grouping, t)).show_ui(ui, |ui| {
                    for g in projects::Grouping::ALL {
                        ui.selectable_value(&mut grouping, g, grouping_name(g, t));
                    }
                });
                if (sort, grouping) != (self.config.settings.projects_sort, self.config.settings.projects_grouping) {
                    self.config.settings.projects_sort = sort;
                    self.config.settings.projects_grouping = grouping;
                    self.save_config();
                }
            });
            if self.config.settings.projects_grouping == projects::Grouping::Mine && groups.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.projects_groups_hint).size(12.0).color(theme.text_muted));
            }
            ui.add_space(12.0);

            // The cards, as many a row as fit, under their titles.
            const GAP: f32 = 12.0;
            let width = ui.available_width();
            let columns = ((width + GAP) / (280.0 + GAP)).floor().max(1.0) as usize;
            let card_w = (width - GAP * (columns as f32 - 1.0)) / columns as f32;
            let sections = self.projects_shown(true);
            if sections.iter().all(|(_, l)| l.is_empty()) {
                ui.label(egui::RichText::new(t.projects_no_match.replace("{q}", self.projects.search.trim())).size(13.0).color(theme.text_muted));
            }
            for (title, list) in sections {
                if let Some(title) = title {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&title).size(14.5).strong().color(theme.text));
                        ui.label(egui::RichText::new(list.len().to_string()).size(12.0).color(theme.text_muted));
                    });
                    ui.add_space(6.0);
                }
                for row in list.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for project in row {
                            self.project_card(ui, project, card_w, t, theme, &groups, &mut action);
                        }
                    });
                    ui.add_space(GAP);
                }
            }

            // The project picked: its settings.
            if let Some(id) = self.projects.selected.filter(|id| self.project(*id).is_some()) {
                ui.add_space(10.0);
                self.project_settings_ui(ui, id, t, theme, &mut action);
            }
        });
        if import {
            self.projects_import();
        }
        if pick_dir {
            self.projects_pick_dir();
        }
        if open_wp {
            self.projects_new_wordpress_form();
        }
        if let Some(tool) = wp_more {
            self.open_env_page(tool);
        }
        if let Some((name, version, php, db)) = new_wp {
            self.projects_new_wordpress(name, version, php, db);
        }
        let Some((id, action)) = action else { return };
        match action {
            Menu::Php => self.open_env_page(DevTool::Php),
            Menu::Db => self.open_env_page(DevTool::Mysql),
            Menu::Restart => self.project_restart(id),
            Menu::ImportSql => {
                if let Some(project) = self.project(id).cloned() {
                    let name = self.project_found(&project).database.clone().unwrap_or_else(|| projects::database_name(&project.name));
                    self.sql_import_open(Some((project.db.clone(), name, id)));
                }
            }
            other => self.project_menu(id, other),
        }
    }

    /// A project's card: its state and name, what it is, its address, its environment (PHP, nginx,
    /// its database); a click picks it, the one picked starts, opens or stops from it.
    #[allow(clippy::too_many_arguments)]
    fn project_card(&mut self, ui: &mut Ui, project: &Project, width: f32, t: &'static Strings, theme: &Theme, groups: &[String], action: &mut Option<(Uuid, Menu)>) {
        let id = project.id;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 138.0), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let picked = self.projects.selected == Some(id);
        let state = self.project_state(id);
        let label = self.project_found(project).label.clone();
        let painter = ui.painter().with_clip_rect(rect);
        let fill = if resp.hovered() && !picked { theme.tab_hover.gamma_multiply(0.6) } else { theme.chrome_bg };
        painter.rect_filled(rect, 12.0, fill);
        let stroke = if picked { Stroke::new(2.0, theme.accent) } else { Stroke::new(1.0, theme.tab_hover) };
        painter.rect_stroke(rect, 12.0, stroke, egui::StrokeKind::Inside);
        let inner = rect.shrink(14.0);

        // State and name; what it is.
        let dot = Pos2::new(inner.min.x + 4.0, inner.min.y + 9.0);
        match state {
            State::Running => painter.circle_filled(dot, 4.5, RUNNING),
            State::Starting => painter.circle_stroke(dot, 4.0, Stroke::new(1.6, STARTING)),
            State::Failed => painter.circle_filled(dot, 4.5, theme.ansi[1]),
            State::Stopped => painter.circle_stroke(dot, 4.0, Stroke::new(1.3, theme.text_muted.gamma_multiply(0.7))),
        };
        if state == State::Starting {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        painter.text(Pos2::new(inner.min.x + 16.0, dot.y), Align2::LEFT_CENTER, &project.name, FontId::proportional(15.0), theme.text);
        // The star (a favorite stays at the head of the list), shown on hover unless it is one.
        let star = Rect::from_center_size(Pos2::new(inner.max.x - 7.0, dot.y), Vec2::splat(20.0));
        let star_resp = ui.interact(star, ui.id().with(("project-star", id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(if project.favorite { t.projects_unfavorite } else { t.projects_favorite_tip });
        if project.favorite || resp.hovered() || star_resp.hovered() {
            let color = if project.favorite { STAR } else if star_resp.hovered() { theme.text } else { theme.text_muted.gamma_multiply(0.7) };
            paint_star(&painter, star.center(), 6.5, project.favorite, color);
        }
        if star_resp.clicked() {
            self.project_set_favorite(id, !project.favorite);
        }
        let what = label.unwrap_or_else(|| project.kind.name().to_owned());
        painter.text(Pos2::new(inner.max.x - 22.0, dot.y), Align2::RIGHT_CENTER, what, FontId::proportional(11.0), theme.text_muted);
        if !project.group.is_empty() && self.config.settings.projects_grouping != projects::Grouping::Mine {
            painter.text(Pos2::new(inner.max.x, inner.min.y + 28.0), Align2::RIGHT_CENTER, &project.group, FontId::proportional(11.0), theme.text_muted.gamma_multiply(0.85));
        }
        painter.text(Pos2::new(inner.min.x, inner.min.y + 28.0), Align2::LEFT_CENTER, project.url(self.config.settings.projects_port), FontId::proportional(11.5), theme.accent.gamma_multiply(0.9));

        // Its environment.
        let mut x = inner.min.x;
        let y = inner.min.y + 54.0;
        let php = project.php_version();
        let nginx = self.envs.installed(Env::Nginx).first().cloned();
        let db = project.db.clone();
        let db_missing = db.is_some() && project.db_server().is_none();
        let mut chips: Vec<(Env, String, bool)> = vec![
            (Env::Php, php.map(|v| format!("PHP {v}")).unwrap_or_else(|| format!("PHP {}", t.projects_php_none)), project.php_version().is_none()),
            (Env::Nginx, nginx.map(|v| if v == envs::SYSTEM { "nginx".to_owned() } else { format!("nginx {v}") }).unwrap_or_else(|| format!("nginx {}", t.projects_php_none)), self.envs.installed(Env::Nginx).is_empty()),
        ];
        match &db {
            Some(db) => {
                let port = self.projects.db_ports.get(&id).filter(|p| **p != projects::DB_PORT).map(|p| format!(" :{p}")).unwrap_or_default();
                chips.push((db.kind, format!("{}{port}", db.label(t.env_system)), db_missing));
            }
            None => chips.push((Env::Mysql, t.projects_db_none.to_owned(), false)),
        }
        for (kind, text, missing) in chips {
            let none = kind == Env::Mysql && db.is_none();
            let color = if missing { theme.ansi[1] } else if none { theme.text_muted.gamma_multiply(0.6) } else { env_color(kind) };
            let galley = painter.layout_no_wrap(text, FontId::proportional(11.5), if missing { theme.ansi[1] } else if none { theme.text_muted } else { theme.text });
            let chip = Rect::from_min_size(Pos2::new(x, y - 11.0), Vec2::new(galley.size().x + 32.0, 22.0));
            if chip.max.x > inner.max.x + 4.0 && x > inner.min.x {
                break;
            }
            painter.rect_filled(chip, 11.0, color.gamma_multiply(0.14));
            paint_env_icon(&painter, Pos2::new(chip.min.x + 12.0, y), kind, color);
            painter.galley(Pos2::new(chip.min.x + 24.0, y - galley.size().y / 2.0), galley, theme.text);
            x = chip.max.x + 6.0;
        }

        // What to do: on the card picked, start or open and stop; on the others, their state. Its
        // database opened in Ronnie from the right end, its logs beside, on all of them.
        let mut bottom = Rect::from_min_max(Pos2::new(inner.min.x, inner.max.y - 28.0), inner.max);
        {
            let square = Rect::from_min_max(Pos2::new(bottom.max.x - 28.0, bottom.min.y), bottom.max);
            bottom.max.x = square.min.x - 6.0;
            let fresh = self.project_logs_new(project);
            let tip = if fresh { t.projects_logs_new } else { t.projects_logs_tip };
            let button = ui.place(square, egui::Button::new("").corner_radius(6.0)).on_hover_text(tip);
            paint_logs_icon(ui.painter(), square.center(), if button.hovered() { theme.accent } else { theme.text_muted });
            if fresh {
                ui.painter().circle_filled(Pos2::new(square.max.x - 5.0, square.min.y + 5.0), 3.5, theme.ansi[1]);
            }
            if button.clicked() {
                *action = Some((id, Menu::Logs));
            }
        }
        if db.is_some() {
            let square = Rect::from_min_max(Pos2::new(bottom.max.x - 28.0, bottom.min.y), bottom.max);
            bottom.max.x = square.min.x - 8.0;
            let name = self.project_found(project).database.clone().unwrap_or_else(|| project.name.clone());
            let button = ui.place(square, egui::Button::new("").corner_radius(6.0)).on_hover_text(t.projects_open_db_tip.replace("{name}", &name));
            super::dbview::paint_db_icon(ui.painter(), square.center(), if button.hovered() { theme.accent } else { theme.text_muted });
            if button.clicked() {
                *action = Some((id, Menu::Database));
            }
        }
        let state_text = match state {
            State::Running => t.projects_running,
            State::Starting => t.projects_starting,
            State::Failed => t.projects_failed,
            State::Stopped => t.projects_stopped,
        };
        let state_color = match state {
            State::Running => RUNNING,
            State::Starting => STARTING,
            State::Failed => theme.ansi[1],
            State::Stopped => theme.text_muted,
        };
        if picked {
            let button = |text: String, fill: Option<Color32>| {
                let b = egui::Button::new(egui::RichText::new(text).size(13.0).color(if fill.is_some() { Color32::WHITE } else { theme.text })).corner_radius(6.0);
                match fill {
                    Some(fill) => b.fill(fill),
                    None => b,
                }
            };
            let half = (bottom.width() - 8.0) / 2.0;
            let left = Rect::from_min_size(bottom.min, Vec2::new(half, bottom.height()));
            let right = Rect::from_min_size(Pos2::new(bottom.min.x + half + 8.0, bottom.min.y), Vec2::new(half, bottom.height()));
            match state {
                State::Running => {
                    if ui.place(left, button(format!("↗  {}", t.projects_open_site), Some(RUNNING.gamma_multiply(0.85)))).clicked() {
                        *action = Some((id, Menu::Open));
                    }
                    if ui.place(right, button(format!("■  {}", t.projects_stop), None)).clicked() {
                        *action = Some((id, Menu::Stop));
                    }
                }
                State::Starting => {
                    painter.text(bottom.left_center(), Align2::LEFT_CENTER, format!("…  {}", t.projects_starting), FontId::proportional(12.5), STARTING);
                }
                State::Stopped | State::Failed => {
                    if ui.place(bottom, button(format!("▶  {}", t.projects_launch), Some(theme.accent))).clicked() {
                        *action = Some((id, Menu::Open));
                    }
                }
            }
        } else {
            painter.text(bottom.left_center(), Align2::LEFT_CENTER, state_text, FontId::proportional(12.0), state_color);
        }
        if resp.clicked() {
            self.projects.selected = Some(id);
        }
        resp.context_menu(|ui| {
            self.project_menu_extra(ui, project, groups);
            ui.separator();
            menu_item(ui, t.projects_open_site, (id, Menu::Open), action);
            if state == State::Running {
                menu_item(ui, t.projects_stop, (id, Menu::Stop), action);
            }
            ui.separator();
            menu_item(ui, t.projects_terminal, (id, Menu::Terminal), action);
            menu_item(ui, t.projects_folder, (id, Menu::Folder), action);
            menu_item(ui, t.projects_logs_tip, (id, Menu::Logs), action);
            if project.db.is_some() {
                menu_item(ui, t.projects_open_db, (id, Menu::Database), action);
            }
        });
    }

    /// The settings of the project picked: a change while it runs restarts it.
    fn project_settings_ui(&mut self, ui: &mut Ui, id: Uuid, t: &'static Strings, theme: &Theme, action: &mut Option<(Uuid, Menu)>) {
        let Some(mut project) = self.project(id).cloned() else { return };
        let groups = self.project_groups();
        let state = self.project_state(id);
        let found = self.project_found(&project).database.clone();
        let xdebug = self.project_xdebug_state(id);
        let mut xdebug_to: Option<bool> = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.projects_settings).size(17.0).color(theme.text));
            ui.add_space(10.0);
            if ui.button(t.projects_terminal).clicked() {
                *action = Some((id, Menu::Terminal));
            }
            if ui.button(t.projects_folder).clicked() {
                *action = Some((id, Menu::Folder));
            }
        });
        if let Some(error) = self.projects.errors.get(&id) {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(error).size(12.5).color(theme.ansi[1]));
        }
        ui.add_space(8.0);
        let mut changed = false;
        let mut restart = false;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(12.0).inner_margin(16.0).show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            egui::Grid::new("project-settings").num_columns(2).spacing([18.0, 12.0]).show(ui, |ui| {
                ui.label(egui::RichText::new(t.projects_name).color(theme.text_muted));
                let resp = ui.add(egui::TextEdit::singleline(&mut project.name).desired_width(260.0));
                if resp.changed() {
                    changed = true;
                }
                if resp.lost_focus() {
                    restart = true;
                }
                ui.end_row();

                ui.label(egui::RichText::new(t.projects_kind).color(theme.text_muted));
                egui::ComboBox::from_id_salt("project-kind").selected_text(project.kind.name()).show_ui(ui, |ui| {
                    for kind in projects::Kind::ALL {
                        if ui.selectable_value(&mut project.kind, kind, kind.name()).changed() {
                            changed = true;
                            restart = true;
                        }
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new("PHP").color(theme.text_muted));
                let installed = self.envs.installed(Env::Php).to_vec();
                ui.horizontal(|ui| {
                    let current = project.php_version();
                    egui::ComboBox::from_id_salt("project-php").selected_text(current.clone().unwrap_or_else(|| t.projects_php_none.to_owned())).show_ui(ui, |ui| {
                        for version in &installed {
                            if ui.selectable_label(current.as_deref() == Some(version.as_str()), version).clicked() {
                                project.php = Some(version.clone());
                                changed = true;
                                restart = true;
                            }
                        }
                    });
                    if ui.link(t.projects_php_more).clicked() {
                        *action = Some((id, Menu::Php));
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new(t.projects_db_server).color(theme.text_muted));
                let servers: Vec<Db> = Env::DATABASES.into_iter().flat_map(|kind| self.envs.installed(kind).iter().map(move |version| Db { kind, version: version.clone() })).collect();
                ui.horizontal(|ui| {
                    let current = project.db.as_ref().map(|db| db.label(t.env_system)).unwrap_or_else(|| t.projects_db_none.to_owned());
                    egui::ComboBox::from_id_salt("project-db").selected_text(current).show_ui(ui, |ui| {
                        if ui.selectable_label(project.db.is_none(), t.projects_db_none).clicked() {
                            project.db = None;
                            changed = true;
                            restart = true;
                        }
                        for db in &servers {
                            if ui.selectable_label(project.db.as_ref() == Some(db), db.label(t.env_system)).clicked() {
                                project.db = Some(db.clone());
                                changed = true;
                                restart = true;
                            }
                        }
                    });
                    if ui.link(t.projects_db_more).clicked() {
                        *action = Some((id, Menu::Db));
                    }
                    if ui.link(t.projects_import_sql).on_hover_text(t.sql_body).clicked() {
                        *action = Some((id, Menu::ImportSql));
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new(t.projects_group).color(theme.text_muted));
                ui.horizontal(|ui| {
                    if ui.add(egui::TextEdit::singleline(&mut project.group).desired_width(180.0).hint_text(t.projects_no_group)).changed() {
                        changed = true;
                    }
                    if !groups.is_empty() {
                        egui::ComboBox::from_id_salt("project-group").selected_text("").width(28.0).show_ui(ui, |ui| {
                            for group in &groups {
                                if ui.selectable_label(project.group.trim().eq_ignore_ascii_case(group), group).clicked() {
                                    project.group = group.clone();
                                    changed = true;
                                }
                            }
                        });
                    }
                    ui.add_space(8.0);
                    if ui.checkbox(&mut project.favorite, t.projects_favorite_label).changed() {
                        changed = true;
                    }
                });
                ui.end_row();

                if let Some(db) = &found {
                    ui.label(egui::RichText::new(t.projects_database).color(theme.text_muted));
                    ui.label(egui::RichText::new(db).monospace().size(12.0).color(theme.text));
                    ui.end_row();
                }

                ui.label(egui::RichText::new(t.projects_root).color(theme.text_muted));
                let resp = ui.add(egui::TextEdit::singleline(&mut project.root).desired_width(260.0).hint_text(t.projects_root_hint));
                if resp.changed() {
                    changed = true;
                }
                if resp.lost_focus() {
                    restart = true;
                }
                ui.end_row();

                ui.label(egui::RichText::new(t.projects_path).color(theme.text_muted));
                ui.label(egui::RichText::new(project.path.display().to_string()).monospace().size(12.0).color(theme.text));
                ui.end_row();

                ui.label(egui::RichText::new(t.projects_logs).color(theme.text_muted));
                if ui.link(t.projects_logs_show).clicked() {
                    *action = Some((id, Menu::Logs));
                }
                ui.end_row();

                ui.label(egui::RichText::new("Xdebug").color(theme.text_muted));
                match &xdebug {
                    super::xdebug::XdebugState::Downloading(text) => {
                        ui.ctx().request_repaint_after(Duration::from_millis(250));
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(12.0));
                            ui.label(egui::RichText::new(text).size(12.5).color(theme.text_muted));
                        });
                    }
                    super::xdebug::XdebugState::Unavailable => {
                        ui.label(egui::RichText::new(t.projects_xdebug_unavailable).size(12.5).color(theme.text_muted));
                    }
                    state => {
                        let mut on = matches!(state, super::xdebug::XdebugState::On);
                        if ui.checkbox(&mut on, t.projects_xdebug_check).on_hover_text(t.projects_xdebug_tip).changed() {
                            xdebug_to = Some(on);
                        }
                    }
                }
                ui.end_row();
            });
        });
        if changed {
            if let Some(slot) = self.config.projects.iter_mut().find(|p| p.id == id) {
                *slot = project.clone();
            }
            self.save_config();
        }
        if let Some(on) = xdebug_to {
            self.project_xdebug_set(id, on);
        } else if restart && state == State::Running {
            *action = Some((id, Menu::Restart));
        }
        ui.add_space(18.0);
        ui.label(egui::RichText::new(t.projects_in_dir).size(12.5).color(theme.text_muted));
    }


    /// A project stopped to be started again (its settings changed): started once stopped.
    pub(super) fn projects_restart_poll(&mut self) {
        let ready: Vec<Uuid> = self.projects.restart_after.iter().copied().filter(|id| !self.projects.busy(*id)).collect();
        for id in ready {
            self.projects.restart_after.remove(&id);
            self.project_start(id, false);
        }
    }
}

/// What the menu of a project, or its page, asks.
#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Open,
    Start,
    Stop,
    Terminal,
    Folder,
    Settings,
    Php,
    Db,
    Logs,
    Restart,
    /// Its database opened in Ronnie.
    Database,
    /// A .sql file loaded into a new database for it.
    ImportSql,
}

/// A sheet of lines: a project's logs.
fn paint_logs_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let sheet = Rect::from_center_size(c, Vec2::new(11.0, 13.0));
    painter.rect_stroke(sheet, 1.5, Stroke::new(1.3, color), egui::StrokeKind::Middle);
    for (k, w) in [7.0, 5.0, 7.0].into_iter().enumerate() {
        let y = sheet.min.y + 3.5 + k as f32 * 3.0;
        painter.line_segment([Pos2::new(sheet.min.x + 2.0, y), Pos2::new(sheet.min.x + 2.0 + w, y)], Stroke::new(1.1, color));
    }
}

/// The gold of a favorite's star.
const STAR: Color32 = Color32::from_rgb(0xf2, 0xc1, 0x4e);

/// A five-pointed star of outer radius `r`: filled, or its outline.
fn paint_star(painter: &egui::Painter, c: Pos2, r: f32, filled: bool, color: Color32) {
    let point = |k: usize, radius: f32| {
        let angle = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
        c + Vec2::new(angle.cos(), angle.sin()) * radius
    };
    let inner = r * 0.45;
    let outline: Vec<Pos2> = (0..10).map(|k| point(k, if k % 2 == 0 { r } else { inner })).collect();
    if filled {
        // The inner pentagon, then a triangle a point (a star isn't convex).
        let pentagon: Vec<Pos2> = (0..5).map(|k| point(2 * k + 1, inner)).collect();
        painter.add(egui::Shape::convex_polygon(pentagon, color, Stroke::NONE));
        for k in 0..5 {
            let tri = vec![point(2 * k + 1 + 9, inner), point(2 * k, r), point(2 * k + 1, inner)];
            painter.add(egui::Shape::convex_polygon(tri, color, Stroke::NONE));
        }
    } else {
        painter.add(egui::Shape::closed_line(outline, Stroke::new(1.2, color)));
    }
}

fn sort_name(sort: projects::Sort, t: &Strings) -> &'static str {
    match sort {
        projects::Sort::Name => t.projects_sort_name,
        projects::Sort::Kind => t.projects_sort_kind,
        projects::Sort::Recent => t.projects_sort_recent,
        projects::Sort::Running => t.projects_sort_running,
    }
}

fn grouping_name(grouping: projects::Grouping, t: &Strings) -> &'static str {
    match grouping {
        projects::Grouping::None => t.projects_group_none,
        projects::Grouping::Kind => t.projects_group_kind,
        projects::Grouping::Mine => t.projects_group_mine,
    }
}

/// PHP's logo: its ellipse, "php" inside.
pub(super) fn paint_projects_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let points: Vec<Pos2> = (0..40).map(|k| k as f32 / 40.0 * std::f32::consts::TAU).map(|a| c + Vec2::new(a.cos() * 10.5, a.sin() * 6.5)).collect();
    painter.add(egui::Shape::closed_line(points, Stroke::new(1.3, color)));
    painter.text(c + Vec2::new(0.0, -0.5), Align2::CENTER_CENTER, "php", FontId::proportional(9.5), color);
}

/// What Claude asks of the PHP projects (`ronnie projects`, `ronnie project …`): answered in words.
impl App {
    pub(super) fn projects_running(&self, id: Uuid) -> bool {
        self.projects.running.contains(&id)
    }

    /// A project by its name or its address's name, whatever the case.
    pub(super) fn project_named(&self, name: &str) -> Result<Uuid, String> {
        let wanted = name.trim().to_lowercase();
        self.config
            .projects
            .iter()
            .find(|p| p.name.to_lowercase() == wanted || p.slug() == wanted)
            .map(|p| p.id)
            .ok_or_else(|| format!("no PHP project {name} (see `ronnie projects`)"))
    }

    pub(super) fn projects_text(&self) -> String {
        let port = self.config.settings.projects_port;
        let mut out = match self.projects_dir() {
            Some(dir) => format!("The folder of the projects: {} (each folder in it is a project; one copied in shows up)\n", dir.display()),
            None => String::new(),
        };
        if self.config.projects.is_empty() {
            out.push_str("No PHP project yet.\n");
        }
        for p in &self.config.projects {
            let state = match self.project_state(p.id) {
                State::Stopped => "stopped".to_owned(),
                State::Starting => "starting".to_owned(),
                State::Running => "running".to_owned(),
                State::Failed => format!("didn't start: {}", self.projects.errors.get(&p.id).map(String::as_str).unwrap_or("?")),
            };
            let label = self.projects.found.get(&p.id).and_then(|f| f.label.clone()).unwrap_or_else(|| p.kind.name().to_owned());
            let php = match (&p.php, p.php_version()) {
                (_, Some(v)) => format!("PHP {v}"),
                (Some(v), None) => format!("PHP {v} (not installed)"),
                (None, None) => "no PHP installed".to_owned(),
            };
            let db = p.db.as_ref().map_or_else(|| "no database".to_owned(), |db| format!("{}{}", db.label("system"), if p.db_server().is_none() { " (not installed)" } else { "" }));
            let root = if p.root.is_empty() { String::new() } else { format!(", root {}", p.root) };
            out.push_str(&format!("- \"{}\" ({label}) {state}, {}: {}, {php}, {db}{root}\n", p.name, p.url(port), p.path.display()));
        }
        let installed: Vec<String> = Env::ALL.iter().map(|k| format!("{} {}", k.name(), if self.envs.installed(*k).is_empty() { "none".to_owned() } else { self.envs.installed(*k).join(", ") })).collect();
        out.push_str(&format!("Installed in Ronnie: {}", installed.join("; ")));
        out
    }

    /// A project's settings as Claude gives them, checked (what they name installed).
    fn project_apply(project: &mut Project, settings: &crate::drive::ProjectSettings) -> Result<(), String> {
        if let Some(kind) = &settings.kind {
            project.kind = match kind.to_lowercase().as_str() {
                "php" => projects::Kind::Php,
                "wordpress" => projects::Kind::WordPress,
                "prestashop" => projects::Kind::PrestaShop,
                "laravel" => projects::Kind::Laravel,
                _ => return Err(format!("{kind}: php, laravel, wordpress or prestashop")),
            };
            if settings.root.is_none() && project.kind == projects::Kind::Laravel {
                project.root = "public".into();
            }
        }
        if let Some(root) = &settings.root {
            let root = root.trim().trim_matches('/').to_owned();
            if root.split('/').any(|part| part == "..") {
                return Err(format!("{root}: a folder in the project's"));
            }
            project.root = root;
        }
        if let Some(php) = &settings.php {
            let installed = envs::installed(Env::Php);
            let version = installed.iter().find(|v| *v == php).or_else(|| installed.iter().find(|v| v.starts_with(&format!("{php}.")))).cloned();
            project.php = Some(version.ok_or_else(|| format!("PHP {php} isn't installed in Ronnie (installed: {}); `ronnie env install php {php}` once the user agrees", if installed.is_empty() { "none".into() } else { installed.join(", ") }))?);
        }
        if let Some(db) = &settings.db {
            project.db = match db.to_lowercase().as_str() {
                "none" | "" => None,
                spec => {
                    let (kind, version) = spec.split_once(':').map_or((spec, None), |(k, v)| (k, Some(v)));
                    let kind = match kind {
                        "mysql" => Env::Mysql,
                        "mariadb" => Env::Mariadb,
                        _ => return Err(format!("{db}: none, mysql, mariadb, or mysql:<version>")),
                    };
                    let installed = envs::installed(kind);
                    let found = match version {
                        Some(v) => installed.iter().find(|i| *i == v || i.starts_with(&format!("{v}."))).cloned(),
                        None => installed.first().cloned(),
                    };
                    let version = found.ok_or_else(|| format!("{} {} isn't installed in Ronnie; `ronnie env install {} <version>` once the user agrees", kind.name(), version.unwrap_or(""), kind.name().to_lowercase()))?;
                    Some(Db { kind, version })
                }
            };
        }
        Ok(())
    }

    pub(super) fn project_new_asked(&mut self, name: &str, settings: &crate::drive::ProjectSettings) -> Result<String, String> {
        let name = name.trim();
        if self.read_only {
            return Err("this Ronnie is read-only (another one runs)".into());
        }
        if !projects::valid_site_name(name) {
            return Err(format!("{name}: not a project's name (no / \\ : * ? \" < > |, not wamp…)"));
        }
        if let Ok(id) = self.project_named(name) {
            let path = self.project(id).map(|p| p.path.display().to_string()).unwrap_or_default();
            return Err(format!("the PHP project {name} exists already ({path}): `ronnie project set` changes it"));
        }
        let dir = self.projects_dir().ok_or("no folder for the projects")?;
        let folder = dir.join(name);
        if folder.exists() {
            return Err(format!("{} exists already", folder.display()));
        }
        let mut project = Project::of_folder(&folder);
        if project.php.is_none() {
            project.php = envs::installed(Env::Php).into_iter().next();
        }
        Self::project_apply(&mut project, settings)?;
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let url = project.url(self.config.settings.projects_port);
        let id = project.id;
        self.config.projects.push(project);
        self.config.projects.sort_by_key(|p| p.name.to_lowercase());
        self.save_config();
        let t = self.t();
        self.claude_notice(t.claude_made_project, name.to_owned(), super::drive::Lead::Project(id));
        let missing = self.project_missing(id);
        Ok(format!("PHP project {name} made: its folder {} (empty: put the site in it), its address {url} once started (`ronnie project start {name}`).{missing}", folder.display()))
    }

    pub(super) fn project_set_asked(&mut self, name: &str, settings: &crate::drive::ProjectSettings) -> Result<String, String> {
        let id = self.project_named(name)?;
        let k = self.config.projects.iter().position(|p| p.id == id).ok_or("gone")?;
        let mut project = self.config.projects[k].clone();
        Self::project_apply(&mut project, settings)?;
        self.config.projects[k] = project;
        self.projects.found.remove(&id);
        self.save_config();
        let restarted = if self.projects.running.contains(&id) {
            self.project_restart(id);
            " It restarts with them."
        } else {
            ""
        };
        Ok(format!("{name} changed.{restarted}{}", self.project_missing(id)))
    }

    pub(super) fn project_start_asked(&mut self, name: &str) -> Result<String, String> {
        let id = self.project_named(name)?;
        if self.projects.running.contains(&id) {
            let url = self.project(id).map(|p| p.url(self.config.settings.projects_port)).unwrap_or_default();
            return Ok(format!("{name} runs already: {url}"));
        }
        let missing = self.project_missing(id);
        if !missing.is_empty() {
            return Err(format!("{name} can't start yet.{missing}"));
        }
        self.project_start(id, false);
        let url = self.project(id).map(|p| p.url(self.config.settings.projects_port)).unwrap_or_default();
        Ok(format!("Starting {name}: {url} once it runs (`ronnie projects` says when, or why not)."))
    }

    /// What a project still needs installed to start, in words ("" when nothing).
    fn project_missing(&self, id: Uuid) -> String {
        let Some(project) = self.project(id) else { return String::new() };
        let mut missing = Vec::new();
        if project.php_version().is_none() {
            missing.push("PHP (`ronnie env available php`)".to_owned());
        }
        if self.envs.installed(Env::Nginx).is_empty() {
            missing.push(match envs::system_only(Env::Nginx) {
                Some(how) => format!("nginx, on the system: `{how}`"),
                None => "nginx (`ronnie env available nginx`)".to_owned(),
            });
        }
        if let (Some(db), None) = (&project.db, project.db_server()) {
            missing.push(format!("{} (`ronnie env available {}`)", db.label("system"), db.kind.name().to_lowercase()));
        }
        if missing.is_empty() {
            String::new()
        } else {
            format!(" Missing: {}; to install once the user agrees.", missing.join(", "))
        }
    }
}
