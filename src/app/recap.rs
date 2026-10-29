//! The week's recap (see `crate::recap`): the time counted where it goes while a window of Ronnie is
//! in front and in use, and each evening the day's section written into the week's note.

use std::collections::HashMap;
use std::thread::JoinHandle;
use std::time::Instant;

use super::*;
use crate::recap::{self, Journal, Place, Report};

/// Without a key pressed or the mouse moved for this long (seconds), the time no longer counts.
const IDLE: f64 = 5.0 * 60.0;
/// Time between two frames counted at most (seconds): the window is drawn again at least this often
/// while the recap is on.
const TICK: f64 = 30.0;
/// The time counted goes into the journal by this much (seconds), the place looked at then.
const FLUSH: f64 = 5.0;
/// The days due looked for, and the journal saved, this often.
const CHECK_EVERY: Duration = Duration::from_secs(60);

pub(super) struct Recap {
    journal: Journal,
    /// Each window's last frame (app time), and whether it was in front then.
    frames: HashMap<egui::ViewportId, (f64, bool)>,
    /// The last key or mouse move (app time).
    input: f64,
    /// Counted, not in the journal yet.
    pending: f64,
    dirty: bool,
    checked: Option<Instant>,
    /// A day's section being made (git and Claude read off the interface's thread).
    job: Option<JoinHandle<Report>>,
    /// The day whose section was asked from the settings: written now, again if it already was.
    forced: Option<chrono::NaiveDate>,
}

impl Default for Recap {
    fn default() -> Self {
        Self { journal: recap::load(), frames: HashMap::new(), input: 0.0, pending: 0.0, dirty: false, checked: None, job: None, forced: None }
    }
}

impl Recap {
    /// Today's section, now (from the settings).
    pub(super) fn write_now(&mut self) {
        self.forced = Some(chrono::Local::now().date_naive());
        self.checked = None;
    }
}

impl App {
    /// At each frame of a window: the time since its last one counted, if it was in front and used.
    pub(super) fn recap_track(&mut self, ui: &Ui) {
        // A second Ronnie (read only) leaves the journal and the notes to the first.
        if !self.config.settings.recap.on || self.read_only {
            // Turned on again later: counted from then, not from the last frame before.
            self.recap.frames.clear();
            self.recap.pending = 0.0;
            return;
        }
        let (now, focused, input) = ui.input(|i| (i.time, i.viewport().focused == Some(true), !i.events.is_empty()));
        let idle = now - self.recap.input;
        if input {
            self.recap.input = now;
        }
        let last = self.recap.frames.insert(self.viewport, (now, focused));
        ui.ctx().request_repaint_after(Duration::from_secs_f64(TICK));
        // Counted from a frame in front to one in front, used lately.
        let Some((then, was_focused)) = last else { return };
        if !(focused && was_focused) || idle > IDLE {
            return;
        }
        self.recap.pending += (now - then).clamp(0.0, TICK + 5.0);
        if self.recap.pending < FLUSH {
            return;
        }
        let seconds = self.recap.pending.round() as u32;
        self.recap.pending = 0.0;
        if let Some(place) = self.recap_place() {
            self.recap.journal.add(chrono::Local::now().date_naive(), &place, seconds);
            self.recap.dirty = true;
        }
    }

    /// Where this window's time goes: the games, the SSH server or the database of the tab shown, or
    /// the project of its focused pane's folder. None on the other pages.
    fn recap_place(&self) -> Option<Place> {
        if self.home.is_some() {
            return self.home_game.then_some(Place::Games);
        }
        if self.notes_page || self.music_page || self.dev_page || self.modules_page || self.procs_page {
            return None;
        }
        let tab = self.tabs.get(self.active)?;
        if let Some(host) = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)) {
            return Some(Place::Server(host.name.clone()));
        }
        if tab.show_db
            && let Some(db) = tab.db.and_then(|id| self.config.databases.iter().find(|d| d.id == id))
        {
            return Some(Place::Database(db.name.clone()));
        }
        let cwd = tab.panes.get(&tab.focused).and_then(Terminal::cwd).or_else(|| tab.cwds.get(&tab.focused).cloned())?;
        Some(Place::Project(crate::worklog::project_of(&cwd)))
    }

    /// Every minute: the journal saved, and the days due written into their week's note (one at a
    /// time, its section made in a thread).
    pub(super) fn recap_poll(&mut self, ctx: &egui::Context) {
        if self.recap.job.as_ref().is_some_and(|j| j.is_finished())
            && let Some(Ok(report)) = self.recap.job.take().map(JoinHandle::join)
        {
            self.recap_write(report);
            // The next day due right away (catching up after days closed).
            self.recap.checked = None;
            ctx.request_repaint();
        }
        if !self.config.settings.recap.on || self.read_only || self.recap.checked.is_some_and(|c| c.elapsed() < CHECK_EVERY) {
            return;
        }
        self.recap.checked = Some(Instant::now());
        ctx.request_repaint_after(CHECK_EVERY);
        let now = chrono::Local::now().naive_local();
        let today = now.date();
        self.recap.journal.seen(today);
        // Asked from the settings: today too, before the hour, again if it was written (until it is;
        // asked yesterday, before midnight: no more).
        if self.recap.forced.is_some_and(|d| d != today) {
            self.recap.forced = None;
        }
        let mut due = self.recap.journal.due(now, self.config.settings.recap.hour);
        if self.recap.forced.is_some() && !due.contains(&today) {
            due.push(today);
        }
        if self.recap.job.is_none()
            && let Some(date) = due.first().copied()
        {
            let day = self.recap.journal.days.get(&recap::key(date)).cloned().unwrap_or_default();
            let lang = self.config.settings.language;
            let ctx = ctx.clone();
            self.recap.job = std::thread::Builder::new()
                .name("recap".into())
                .spawn(move || {
                    let report = recap::report(date, &day, lang);
                    ctx.request_repaint();
                    report
                })
                .ok();
        }
        self.recap_save();
    }

    /// A day's section into its week's note (made if need be, in the "Récaps" folder), the week's
    /// total under it written again.
    fn recap_write(&mut self, report: Report) {
        let lang = self.config.settings.language;
        let week = recap::week_start(report.date);
        let week_key = recap::key(week);
        let day_key = recap::key(report.date);
        let now = chrono::Local::now();
        let asked = self.recap.forced == Some(report.date);
        if asked {
            self.recap.forced = None;
        }
        self.recap.dirty = true;
        let day = self.recap.journal.days.entry(day_key.clone()).or_default();
        day.facts = Some(report.facts);
        // Today asked from the settings before the hour: written again at the hour, with the rest of
        // the day.
        if report.date < now.date_naive() || chrono::Timelike::hour(&now) >= self.config.settings.recap.hour {
            day.written_seconds = Some(day.seconds());
            self.recap.journal.written.insert(day_key.clone());
        }
        // A day without anything done (Ronnie only open, a Saturday...) stays out of the note, unless
        // asked.
        if report.empty && !asked {
            self.recap_save();
            return;
        }
        let title = recap::title(week, lang);
        let total = recap::total(&self.recap.journal, week, lang);
        let id = self.recap.journal.notes.get(&week_key).copied();
        let (old_section, old_total) = (self.recap.journal.sections.get(&day_key).cloned(), self.recap.journal.totals.get(&week_key).cloned());
        let mut done = None;
        let id = self.notes.recap_note(id, self.t().recap_folder, |text| {
            let upserted = recap::upsert(text, &title, &report.section, old_section.as_deref(), &total, old_total.as_deref());
            let text = upserted.text.clone();
            done = Some(upserted);
            text
        });
        self.recap.journal.notes.insert(week_key.clone(), id);
        // Remembered as written, to be written again in place (what the user changed isn't).
        if let Some(done) = done {
            if done.section {
                self.recap.journal.sections.insert(day_key, report.section);
            }
            if done.total {
                self.recap.journal.totals.insert(week_key, total);
            }
        }
        self.recap_save();
    }

    pub(super) fn recap_save(&mut self) {
        if std::mem::take(&mut self.recap.dirty) && !self.read_only {
            recap::save(&self.recap.journal);
        }
    }
}
