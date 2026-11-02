//! WAMP's databases taken over in one go (the Databases category's + menu): for each of its servers
//! (MySQL, MariaDB), its exact version installed, WAMP's database services stopped, its data copied
//! with its settings and port; then a connection to each, and each project on its database's server.

use std::sync::{Arc, Mutex};

use super::*;
use crate::envs::{self, Progress};
use crate::projects::{self, Db, WampData};

/// The servers imported and their ports, or why not.
type Imported = Result<Vec<(Db, u16)>, String>;

/// An import under way: what it does, how far its download went, how it ended.
pub(super) struct WampImport {
    step: Arc<Mutex<String>>,
    progress: Arc<Mutex<Progress>>,
    done: Arc<Mutex<Option<Imported>>>,
}

impl App {
    /// Starts the import of WAMP's databases in the background, unless one is under way.
    pub(super) fn wamp_import_start(&mut self) {
        if self.wamp.is_some() || self.read_only {
            return;
        }
        let t = self.t();
        let wamps = projects::wamp_data();
        if wamps.is_empty() {
            self.error = Some(t.wamp_none.to_owned());
            return;
        }
        let job = WampImport { step: Arc::new(Mutex::new(String::new())), progress: Arc::new(Mutex::new(Progress::default())), done: Arc::new(Mutex::new(None)) };
        let (step, progress, done, ctx) = (job.step.clone(), job.progress.clone(), job.done.clone(), self.ctx.clone());
        let _ = std::thread::Builder::new().name("wamp-import".into()).spawn(move || {
            let result = wamp_import(&wamps, t, &step, &progress, &ctx).map_err(|e| format!("{e:#}"));
            crate::log::info(&match &result {
                Ok(dbs) => format!("WAMP's databases imported: {}", dbs.iter().map(|(db, _)| db.label("")).collect::<Vec<_>>().join(", ")),
                Err(e) => format!("WAMP's databases not imported: {e}"),
            });
            *done.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.wamp = Some(job);
    }

    /// Whether WAMP has databases not imported yet (read again every few seconds).
    pub(super) fn wamp_to_import(&mut self) -> bool {
        if self.wamp_checked.is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(5)) {
            let pending = projects::wamp_data().iter().any(|w| !projects::has_data(&Db { kind: w.kind, version: w.version.clone() }));
            self.wamp_checked = Some((Instant::now(), pending));
        }
        self.wamp_checked.is_some_and(|(_, pending)| pending)
    }

    /// What the import does now, and how far ("Downloading MySQL 8.4.7…", 0.4).
    pub(super) fn wamp_import_state(&self) -> Option<(String, Option<f32>)> {
        let job = self.wamp.as_ref()?;
        let p = *job.progress.lock().unwrap();
        let fraction = (p.total > 0 && !p.unpacking && p.received < p.total).then(|| p.received as f32 / p.total as f32);
        Some((job.step.lock().unwrap().clone(), fraction))
    }

    /// Collects the end of the import: a connection to each server, the projects set on the server
    /// holding their database.
    pub(super) fn wamp_import_poll(&mut self) {
        let Some(result) = self.wamp.as_ref().and_then(|job| job.done.lock().unwrap().take()) else { return };
        self.wamp = None;
        self.envs.refresh();
        let t = self.t();
        let dbs = match result {
            Ok(dbs) => dbs,
            Err(e) => {
                self.error = Some(format!("{} : {e}", t.wamp_failed));
                return;
            }
        };
        for (db, port) in &dbs {
            let exists = self.config.databases.iter().any(|c| c.ssh.is_none() && c.port == *port && matches!(c.host.as_str(), "127.0.0.1" | "localhost"));
            if !exists {
                let mut connection = config::DbConnection::new();
                connection.name = format!("{} {}", db.kind.name(), db.version);
                connection.host = "127.0.0.1".into();
                connection.port = *port;
                connection.user = "root".into();
                self.config.databases.push(connection);
            }
        }
        let mut set = 0;
        for project in self.config.projects.iter_mut().filter(|p| p.db.is_none()) {
            let Some(name) = projects::detect(&project.path).database else { continue };
            if let Some((db, _)) = dbs.iter().find(|(db, _)| projects::holds_database(db, &name)) {
                project.db = Some(db.clone());
                set += 1;
            }
        }
        self.save_config();
        let names = dbs.iter().map(|(db, _)| db.label(t.env_system)).collect::<Vec<_>>().join(", ");
        let body = t.wamp_done_body.replace("{n}", &set.to_string());
        self.push_toast(Toast { ok: true, title: t.wamp_done.replace("{names}", &names), body, tab: self.active, at: Instant::now(), mail: None, notice: 0, lead: None });
    }
}

/// The import itself (off the interface's thread): each server's version installed, WAMP's services
/// stopped if its data is to be copied, its data and settings copied; each server and its port.
fn wamp_import(wamps: &[WampData], t: &'static Strings, step: &Arc<Mutex<String>>, progress: &Arc<Mutex<Progress>>, ctx: &egui::Context) -> anyhow::Result<Vec<(Db, u16)>> {
    let say = |text: String| {
        *step.lock().unwrap() = text;
        ctx.request_repaint();
    };
    let dbs: Vec<Db> = wamps.iter().map(|w| Db { kind: w.kind, version: w.version.clone() }).collect();
    for db in &dbs {
        if !envs::is_installed(db.kind, &db.version) {
            say(t.wamp_step_download.replace("{name}", &db.label("")));
            *progress.lock().unwrap() = Progress::default();
            envs::install(db.kind, &envs::release_of(db.kind, &db.version)?, progress, ctx)?;
        }
    }
    *progress.lock().unwrap() = Progress::default();
    if dbs.iter().any(|db| !projects::has_data(db)) {
        say(t.wamp_step_stop.to_owned());
        projects::stop_wamp_services(wamps)?;
    }
    for (wamp, db) in wamps.iter().zip(&dbs) {
        say(t.wamp_step_copy.replace("{name}", &db.label("")));
        projects::import_wamp(wamp, db)?;
    }
    Ok(wamps.iter().zip(dbs).map(|(wamp, db)| (db, wamp.port)).collect())
}
