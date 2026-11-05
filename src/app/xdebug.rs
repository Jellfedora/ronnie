//! Xdebug for a project (its settings): downloaded from xdebug.org for its PHP version the first time
//! (Windows), then the project restarted on PHP processes of its own that load it, so that the other
//! projects on the version aren't slowed down. A debugger (VS Code's PHP Debug, PhpStorm) listening on
//! port 9003 is called at each request of it.

use std::sync::{Arc, Mutex};

use super::*;
use crate::envs::{self, Progress};

/// Xdebug being downloaded for a project's PHP version.
pub(super) struct XdebugInstall {
    id: Uuid,
    php: String,
    progress: Arc<Mutex<Progress>>,
    done: Arc<Mutex<Option<Result<(), String>>>>,
    eta: crate::eta::Eta,
}

/// Where a project stands with Xdebug, for its settings.
pub(super) enum XdebugState {
    /// Not on this system (or no PHP version yet).
    Unavailable,
    Off,
    On,
    /// Being downloaded: what it says.
    Downloading(String),
}

impl App {
    /// Where project `id` stands with Xdebug.
    pub(super) fn project_xdebug_state(&mut self, id: Uuid) -> XdebugState {
        let t = self.t();
        if let Some(job) = self.projects.xdebug.as_mut().filter(|j| j.id == id) {
            let p = *job.progress.lock().unwrap();
            let mut text = t.projects_xdebug_downloading.to_owned();
            if p.total > 0 && p.received < p.total {
                let fraction = p.received as f32 / p.total as f32;
                text += &format!("  ·  {:.0} %", fraction * 100.0);
                let left = job.eta.said(fraction, t);
                if !left.is_empty() {
                    text += &format!("  ·  {left}");
                }
            }
            return XdebugState::Downloading(text);
        }
        let Some(project) = self.project(id) else { return XdebugState::Unavailable };
        match project.php_version() {
            Some(php) if envs::xdebug_path(&php).is_some() => {
                if project.xdebug {
                    XdebugState::On
                } else {
                    XdebugState::Off
                }
            }
            _ => XdebugState::Unavailable,
        }
    }

    /// Xdebug turned on or off for a project: downloaded first if its PHP version has none; the
    /// project restarted if it runs.
    pub(super) fn project_xdebug_set(&mut self, id: Uuid, on: bool) {
        let Some(php) = self.project(id).and_then(|p| p.php_version()) else { return };
        if on && !envs::has_xdebug(&php) {
            if self.projects.xdebug.is_some() {
                return;
            }
            let job = XdebugInstall { id, php: php.clone(), progress: Arc::new(Mutex::new(Progress::default())), done: Arc::new(Mutex::new(None)), eta: Default::default() };
            let (progress, done, ctx) = (job.progress.clone(), job.done.clone(), self.ctx.clone());
            let _ = std::thread::Builder::new().name("xdebug-install".into()).spawn(move || {
                let result = envs::install_xdebug(&php, &progress, &ctx).map_err(|e| format!("{e:#}"));
                crate::log::info(&match &result {
                    Ok(()) => format!("Xdebug installed for PHP {php}"),
                    Err(e) => format!("Xdebug not installed for PHP {php}: {e}"),
                });
                *done.lock().unwrap() = Some(result);
                ctx.request_repaint();
            });
            self.projects.xdebug = Some(job);
            return;
        }
        if let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) {
            project.xdebug = on;
        }
        self.save_config();
        self.project_restart(id);
    }

    /// Collects the end of Xdebug's download: turned on for its project, or why not.
    pub(super) fn project_xdebug_poll(&mut self) {
        let Some(result) = self.projects.xdebug.as_ref().and_then(|job| job.done.lock().unwrap().take()) else { return };
        let Some(job) = self.projects.xdebug.take() else { return };
        match result {
            Ok(()) if envs::has_xdebug(&job.php) => self.project_xdebug_set(job.id, true),
            Ok(()) => {}
            Err(e) => self.error = Some(format!("{} : {e}", self.t().projects_xdebug_failed)),
        }
    }
}
