//! The notes kept with the floor account (see `crate::notesync`), when the user turned it on: synced in
//! the background, what floor sent merged into the notes.

use super::App;

impl App {
    pub(super) fn notesync_tick(&mut self, ctx: &egui::Context) {
        if let Some(done) = self.notesync.poll() {
            self.notes.absorb(done.items, done.files);
        }
        let now = ctx.input(|i| i.time);
        if self.notes.take_saved() {
            self.notesync.touched(now);
        }
        if !self.config.settings.notes_sync || self.read_only {
            return;
        }
        // An account with a pseudo, on the floor this build talks to.
        let Some(account) = self.config.settings.floor.clone().filter(|a| a.name.is_some() && crate::floor::url().as_deref() == Some(a.url.as_str())) else { return };
        // Without the key on this computer (the password not given here yet): nothing goes.
        if !self.notesync.keyed(&account) {
            return;
        }
        let focused = ctx.input(|i| i.focused);
        if !self.notesync.due(now, focused) {
            return;
        }
        let (items, cited) = self.notes.sync_items();
        self.notesync.sync(ctx, now, &account, items, cited);
        // Asked again in a while even with nothing changed here (another machine may write).
        ctx.request_repaint_after(std::time::Duration::from_secs(60));
    }
}
