//! A project's logs in a window of their own (the button on its card): the errors of its site
//! (nginx), those of its PHP version (shared by the projects on it) and its requests, the end of each
//! read again every second, the errors in red. A dot on the card's button when errors came since.

use std::time::SystemTime;

use super::*;
use crate::projects::{self, Project};

/// How much of the end of a log is read.
const TAIL_BYTES: u64 = 256 * 1024;
/// A log is read again so often while shown.
const READ_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Which {
    #[default]
    Site,
    Php,
    Requests,
}

/// The window open: whose logs, which one, its end as last read.
pub(super) struct LogsView {
    id: Uuid,
    which: Which,
    text: String,
    read: Option<Instant>,
}

/// The end of a file, from the start of a line: at most `TAIL_BYTES`.
fn tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(TAIL_BYTES);
    if file.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    match (from > 0, text.find('\n')) {
        (true, Some(cut)) => text[cut + 1..].to_owned(),
        _ => text,
    }
}

/// Whether a line of a log tells of an error (not a notice nor a request answered well).
fn is_error(line: &str) -> bool {
    let lower = line.to_lowercase();
    ["fatal", "[error]", "[crit]", "[alert]", "[emerg]", "parse error", "uncaught", "php warning"].iter().any(|w| lower.contains(w))
}

/// A project's log file.
fn log_path(project: &Project, which: Which) -> Option<PathBuf> {
    match which {
        Which::Site => projects::site_logs(project).ok().map(|(errors, _)| errors),
        Which::Requests => projects::site_logs(project).ok().map(|(_, requests)| requests),
        Which::Php => projects::php_log(&project.php_version()?).ok(),
    }
}

/// When a file was last written.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl App {
    /// Whether something came in a project's logs (its site's errors, its PHP's) since they were last
    /// shown, or since Ronnie started.
    pub(super) fn project_logs_new(&self, project: &Project) -> bool {
        let seen = self.projects.logs_seen.get(&project.id).copied().unwrap_or(self.projects.logs_since);
        [Which::Site, Which::Php].into_iter().filter_map(|w| log_path(project, w)).filter_map(|p| modified(&p)).any(|at| at > seen)
    }

    /// The window of a project's logs opened.
    pub(super) fn project_logs_open(&mut self, id: Uuid) {
        self.projects.logs = Some(LogsView { id, which: Which::Site, text: String::new(), read: None });
    }

    /// The window of a project's logs, if open.
    pub(super) fn project_logs_window(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let theme = self.theme.clone();
        let Some(view) = &self.projects.logs else { return };
        let Some(project) = self.project(view.id).cloned() else {
            self.projects.logs = None;
            return;
        };
        let path = log_path(&project, view.which);
        let view = self.projects.logs.as_mut().unwrap();
        if view.read.is_none_or(|at| at.elapsed() >= READ_EVERY) {
            view.text = path.as_deref().map(tail).unwrap_or_default();
            view.read = Some(Instant::now());
        }
        ctx.request_repaint_after(READ_EVERY);
        // Shown: what came so far is seen.
        self.projects.logs_seen.insert(project.id, SystemTime::now());
        let view = self.projects.logs.as_mut().unwrap();
        let (mut close, mut clear, mut folder) = (false, false, false);
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let modal = egui::Modal::new(egui::Id::new("project-logs")).frame(frame).show(ctx, |ui| {
            let size = ctx.content_rect().size();
            ui.set_width((size.x - 120.0).clamp(420.0, 1000.0));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.projects_logs_title.replace("{name}", &project.name)).size(17.0).strong().color(theme.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let php = project.php_version().map(|v| t.projects_logs_php.replace("{version}", &v));
                let tabs = [(Which::Site, Some(t.projects_logs_site.to_owned())), (Which::Php, php), (Which::Requests, Some(t.projects_logs_requests.to_owned()))];
                for (which, label) in tabs {
                    let Some(label) = label else { continue };
                    if ui.selectable_label(view.which == which, label).clicked() && view.which != which {
                        view.which = which;
                        view.read = None;
                    }
                }
            });
            if view.which == Which::Php {
                ui.label(egui::RichText::new(t.projects_logs_php_shared).size(11.5).color(theme.text_muted));
            }
            ui.add_space(6.0);
            let height = (size.y - 260.0).clamp(200.0, 640.0);
            Frame::new().fill(theme.bg).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_min_height(height);
                egui::ScrollArea::both().max_height(height).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    if view.text.trim().is_empty() {
                        ui.label(egui::RichText::new(t.projects_logs_empty).size(12.5).color(theme.text_muted));
                    }
                    // The last lines only: a long log stays quick to draw.
                    let lines: Vec<&str> = view.text.lines().collect();
                    for line in &lines[lines.len().saturating_sub(1500)..] {
                        let color = if is_error(line) { theme.ansi[1] } else { theme.text };
                        ui.label(egui::RichText::new(*line).monospace().size(11.5).color(color));
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(format!("🗑  {}", t.projects_logs_clear)).clicked() {
                    clear = true;
                }
                if ui.button(format!("📁  {}", t.projects_open_logs)).clicked() {
                    folder = true;
                }
                if let Some(path) = &path {
                    ui.label(egui::RichText::new(path.display().to_string()).size(11.0).color(theme.text_muted));
                }
            });
        });
        if clear {
            // Emptied, not removed: nginx and PHP keep writing in it.
            if let Some(path) = &path {
                let _ = std::fs::OpenOptions::new().write(true).truncate(true).open(path);
            }
            view.read = None;
        }
        if folder && let Ok(logs) = projects::logs_dir() {
            let _ = std::fs::create_dir_all(&logs);
            config::open_folder(&logs);
        }
        if close || modal.should_close() {
            self.projects.logs = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_errors_are_told_from_the_rest() {
        assert!(is_error("2026/11/05 10:00:00 [error] 1234#0: *1 FastCGI sent in stderr: \"PHP message: ...\""));
        assert!(is_error("[05-Nov-2026 10:00:00 Europe/Paris] PHP Fatal error:  Uncaught Error: Call to undefined function"));
        assert!(is_error("PHP Warning:  Undefined variable $x in /site/index.php on line 3"));
        assert!(!is_error("[05-Nov-2026 10:00:00 Europe/Paris] PHP Deprecated:  strftime() is deprecated"));
        assert!(!is_error("127.0.0.1 - - [05/Nov/2026:10:00:00 +0100] \"GET / HTTP/1.1\" 200 512"));
    }

    #[test]
    fn the_end_of_a_long_log_starts_at_a_line() {
        let path = std::env::temp_dir().join(format!("ronnie-tail-{}.log", std::process::id()));
        let line = "a line of the log, long enough to fill it quickly\n";
        let text = line.repeat((TAIL_BYTES as usize / line.len()) + 100);
        std::fs::write(&path, &text).unwrap();
        let end = tail(&path);
        assert!(end.len() as u64 <= TAIL_BYTES);
        assert!(end.starts_with("a line"));
        assert!(end.lines().all(|l| l == line.trim_end()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(tail(&path), "");
    }
}
