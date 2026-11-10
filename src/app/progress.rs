//! The games' progress kept with the floor account (see `crate::progress`): synced in the
//! background, merged into the settings when floor answers.

use super::App;
use crate::progress;

impl App {
    pub(super) fn progress_tick(&mut self, ctx: &egui::Context) {
        if let Some(done) = self.progress.poll() {
            progress::absorb(&mut self.config.settings, done);
            self.save_config();
        }
        // An account with a pseudo, on the floor this build talks to.
        let Some(account) = self.config.settings.floor.clone().filter(|a| a.name.is_some() && crate::floor::url().as_deref() == Some(a.url.as_str())) else { return };
        let key = progress::account_key(&account);
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        let s = &self.config.settings;
        let base = s.progress_bases.get(&key).cloned();
        if !self.progress.due(now, focused, || base.as_ref() != Some(&progress::snapshot(s))) {
            return;
        }
        let local = progress::snapshot(s);
        self.progress.sync(ctx, now, &account, base.unwrap_or_default(), local);
        // Asked again in a while even with nothing to show (another machine may play).
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }
}
