//! Talent fishing's collection kept on floor (its `/v1/pond/save`), the same on all the player's
//! machines. A machine sends only what changed on it since its last sync (`Step`: the hires, the
//! sales, the baits, the budget), floor adds it to what the others sent and answers the whole: hires
//! made on two machines add up instead of one overwriting the other. The first sync of a machine
//! sends all it has, so what each one had before is put together. Only with a pseudo.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::config::{FishCard, FloorAccount, Settings};

/// Seconds between two syncs when nothing changed (others may have fished on another machine).
const EVERY: f64 = 30.0;

/// A card, as floor keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Card {
    pub count: u32,
    pub shiny: u32,
    pub sold: u32,
    /// The biggest one (cm).
    pub best: u32,
    /// First caught (Unix seconds; 0: unknown).
    pub first: i64,
}

/// The collection: cards, baits, budget (k€), rod.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Save {
    pub cards: BTreeMap<String, Card>,
    pub baits: BTreeMap<String, u32>,
    pub budget: u32,
    pub rod: u32,
}

/// What changed since a sync: added (the counters), or the new value (best, first, rod).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Step {
    pub cards: BTreeMap<String, Card>,
    pub baits: BTreeMap<String, i64>,
    pub budget: i64,
    pub rod: u32,
}

/// The last collection floor answered, for the account it belongs to: what the next sync starts from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Synced {
    /// floor's address and the player's id.
    pub account: String,
    pub save: Save,
}

impl Save {
    /// The collection in the settings.
    pub fn of(s: &Settings) -> Self {
        let cards = s.fish_cards.iter().map(|(id, c)| (id.clone(), Card { count: c.count, shiny: c.shiny, sold: c.sold, best: c.best_cm, first: c.first_at })).collect();
        Save { cards, baits: s.fish_baits.clone(), budget: s.fish_budget, rod: s.fish_rod }
    }

    /// Puts it in the settings (the bait on the hook taken off if none is left).
    pub fn put(&self, s: &mut Settings) {
        s.fish_cards = self.cards.iter().map(|(id, c)| (id.clone(), FishCard { count: c.count, shiny: c.shiny, sold: c.sold, best_cm: c.best, first_at: c.first })).collect();
        s.fish_baits = self.baits.clone();
        s.fish_budget = self.budget;
        s.fish_rod = self.rod;
        if s.fish_bait.as_ref().is_some_and(|b| !s.fish_baits.contains_key(b)) {
            s.fish_bait = None;
        }
    }

    /// What changed from `base` to this one.
    pub fn since(&self, base: &Save) -> Step {
        let mut cards = BTreeMap::new();
        for (id, c) in &self.cards {
            let b = base.cards.get(id).copied().unwrap_or_default();
            let step = Card { count: c.count.saturating_sub(b.count), shiny: c.shiny.saturating_sub(b.shiny), sold: c.sold.saturating_sub(b.sold), best: c.best, first: c.first };
            if step.count + step.shiny + step.sold > 0 || c.best != b.best || c.first != b.first {
                cards.insert(id.clone(), step);
            }
        }
        let mut baits = BTreeMap::new();
        for id in self.baits.keys().chain(base.baits.keys()) {
            let n = i64::from(self.baits.get(id).copied().unwrap_or(0)) - i64::from(base.baits.get(id).copied().unwrap_or(0));
            if n != 0 {
                baits.insert(id.clone(), n);
            }
        }
        Step { cards, baits, budget: i64::from(self.budget) - i64::from(base.budget), rod: self.rod }
    }

    /// `step` added (as floor does it).
    pub fn plus(&self, step: &Step) -> Save {
        let mut save = self.clone();
        for (id, c) in &step.cards {
            let had = save.cards.get(id).copied().unwrap_or_default();
            let count = had.count + c.count;
            let first = if had.first > 0 && c.first > 0 { had.first.min(c.first) } else { had.first.max(c.first) };
            let card = Card { count, shiny: (had.shiny + c.shiny).min(count), sold: (had.sold + c.sold).min(count), best: had.best.max(c.best), first };
            if count > 0 {
                save.cards.insert(id.clone(), card);
            } else {
                save.cards.remove(id);
            }
        }
        for (id, n) in &step.baits {
            let left = (i64::from(save.baits.get(id).copied().unwrap_or(0)) + n).max(0) as u32;
            if left > 0 {
                save.baits.insert(id.clone(), left);
            } else {
                save.baits.remove(id);
            }
        }
        save.budget = (i64::from(save.budget) + step.budget).max(0) as u32;
        save.rod = save.rod.max(step.rod);
        save
    }
}

/// The account a collection belongs to.
fn owner(account: &FloorAccount) -> String {
    format!("{}#{}", account.url, account.id)
}

/// A sync done: the account, the collection sent, floor's answer (None: not reached or refused).
pub struct Done {
    pub account: String,
    pub sent: Save,
    pub answer: Option<Save>,
}

#[derive(Default)]
pub struct FishSync {
    shared: Arc<Mutex<Option<Done>>>,
    busy: bool,
    last: Option<f64>,
    /// Something changed here: synced at once.
    pub dirty: bool,
}

impl FishSync {
    /// A sync, when something changed here or the last one is older than `EVERY`.
    pub fn tick(&mut self, ctx: &egui::Context, now: f64, settings: &Settings) {
        if self.busy || (!self.dirty && self.last.is_some_and(|last| now - last < EVERY)) {
            return;
        }
        let Some(url) = crate::floor::url() else { return };
        let Some(account) = settings.floor.as_ref().filter(|a| a.url == url && a.name.is_some()) else { return };
        let owner = owner(account);
        let base = settings.fish_synced.as_ref().filter(|s| s.account == owner).map(|s| s.save.clone()).unwrap_or_default();
        let sent = Save::of(settings);
        let step = sent.since(&base);
        let token = account.token.clone();
        self.busy = true;
        self.dirty = false;
        self.last = Some(now);
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let answer = crate::floor::agent()
                .post(format!("{url}/v1/pond/save"))
                .header("User-Agent", crate::floor::USER_AGENT)
                .header("Authorization", format!("Bearer {token}"))
                .send_json(&step)
                .and_then(|mut res| res.body_mut().read_json::<Save>())
                .map_err(|e| crate::log::info(&format!("floor: the fishing collection: {e}")))
                .ok();
            if let Ok(mut shared) = shared.lock() {
                *shared = Some(Done { account: owner, sent, answer });
            }
            ctx.request_repaint();
        });
    }

    /// Takes floor's answer and puts it in `settings`, with what changed here meanwhile. True: the
    /// settings changed (to save).
    pub fn poll(&mut self, settings: &mut Settings) -> bool {
        let Some(done) = self.shared.lock().ok().and_then(|mut s| s.take()) else { return false };
        self.busy = false;
        let Some(answer) = done.answer else { return false };
        // The player signed in elsewhere meanwhile: not theirs.
        if settings.floor.as_ref().map(owner).as_ref() != Some(&done.account) {
            return false;
        }
        let merged = answer.plus(&Save::of(settings).since(&done.sent));
        // Changed here meanwhile: sent next.
        self.dirty |= merged != answer;
        merged.put(settings);
        settings.fish_synced = Some(Synced { account: done.account, save: answer });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(count: u32, shiny: u32, sold: u32, best: u32, first: i64) -> Card {
        Card { count, shiny, sold, best, first }
    }

    #[test]
    fn two_machines_add_up() {
        // floor has 3 interns; the Mac synced then; the PC never did and has 2 interns and a rod.
        let floor = Save { cards: [("stagiaire".into(), card(3, 1, 0, 22, 500))].into(), baits: [("salaire".into(), 2)].into(), budget: 10, rod: 0 };
        let pc = Save { cards: [("stagiaire".into(), card(2, 0, 0, 25, 400))].into(), budget: 0, rod: 1, ..Default::default() };
        let after = floor.plus(&pc.since(&Save::default()));
        assert_eq!(after.cards["stagiaire"], card(5, 1, 0, 25, 400));
        assert_eq!((after.budget, after.rod, after.baits["salaire"]), (10, 1, 2));

        // The Mac, from what it synced: one more intern hired, 2 sold for 2 k€, a bait used.
        let mut mac = floor.clone();
        mac.cards.get_mut("stagiaire").unwrap().count += 1;
        mac.cards.get_mut("stagiaire").unwrap().sold += 2;
        mac.budget += 2;
        mac.baits.insert("salaire".into(), 1);
        let step = mac.since(&floor);
        assert_eq!(step.cards["stagiaire"], card(1, 0, 2, 22, 500));
        assert_eq!((step.budget, step.baits["salaire"]), (2, -1));
        let after = after.plus(&step);
        assert_eq!(after.cards["stagiaire"], card(6, 1, 2, 25, 400));
        assert_eq!((after.budget, after.baits["salaire"]), (12, 1));
    }

    #[test]
    fn nothing_changed_nothing_sent() {
        let save = Save { cards: [("stagiaire".into(), card(3, 1, 0, 22, 500))].into(), baits: [("salaire".into(), 2)].into(), budget: 10, rod: 2 };
        let step = save.since(&save);
        assert_eq!(save.plus(&step), save);
        assert!(step.baits.is_empty() && step.budget == 0);
    }

    #[test]
    fn a_rod_bought_spends_the_budget() {
        let base = Save { budget: 40, ..Default::default() };
        let now = Save { budget: 10, rod: 1, ..Default::default() };
        let after = base.plus(&now.since(&base));
        assert_eq!((after.budget, after.rod), (10, 1));
    }
}
