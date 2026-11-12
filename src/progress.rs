//! The player's progress in the games (the bests, the fishing's collection, what was unlocked), kept
//! on floor with the account (`/v1/progress`) so that every machine signed in to it has the same.
//!
//! Each machine keeps the last progress it agreed on with floor (its base, in the sealed rewards,
//! one per account). A sync merges three ways: what changed here since the base and what changed on
//! floor since, both kept. The bests take the best of the two, an unlock made on one side counts on
//! both, the counters (cards hired, sold, baits, budget) add what each side did; so that a bait used
//! here isn't brought back by the other side, nor duplicates sold twice. With no base (a machine's
//! first sync), each side counts from nothing: two machines played apart add up.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use crate::config::{FloorAccount, Settings, REWARD_KEYS, TYPING_SCORES};
use crate::floor::{agent, url, USER_AGENT};

/// Kept to each machine: the hires in a row with no new card (its own luck).
const LOCAL_ONLY: [&str; 2] = ["fish_dry", "progress_bases"];

/// The keys of the settings that travel: the rewards, but those of the machine alone.
pub fn keys() -> impl Iterator<Item = &'static str> {
    REWARD_KEYS.into_iter().filter(|k| !LOCAL_ONLY.contains(k))
}

/// The account's key among the bases: its id (not floor's address, which may change: the base lost,
/// the counters would be counted twice).
pub fn account_key(account: &FloorAccount) -> String {
    account.id.clone()
}

/// The progress in `settings`: the keys that travel, as JSON.
pub fn snapshot(settings: &Settings) -> Value {
    let all = serde_json::to_value(settings).unwrap_or_default();
    Value::Object(keys().filter_map(|k| all.get(k).map(|v| (k.to_owned(), v.clone()))).collect())
}

/// `merged` (as `snapshot` gives it) written into `settings`.
pub fn apply(settings: &mut Settings, merged: &Value) {
    let Ok(mut all) = serde_json::to_value(&*settings) else { return };
    if let (Some(map), Some(from)) = (all.as_object_mut(), merged.as_object()) {
        for key in keys() {
            if let Some(v) = from.get(key) {
                map.insert(key.to_owned(), v.clone());
            }
        }
    }
    match serde_json::from_value(all) {
        Ok(back) => *settings = back,
        Err(e) => crate::log::error(&format!("progress: not applied: {e}")),
    }
}

fn int(v: Option<&Value>) -> i64 {
    v.and_then(Value::as_i64).unwrap_or(0)
}

/// What both sides did to a counter since `base`.
fn counter(base: Option<&Value>, local: Option<&Value>, remote: Option<&Value>) -> Value {
    let (b, l, r) = (int(base), int(local), int(remote));
    Value::from((l + r - b).max(0))
}

fn best(local: Option<&Value>, remote: Option<&Value>) -> Value {
    Value::from(int(local).max(int(remote)))
}

/// The smallest that isn't 0 (0: none yet).
fn fastest(local: Option<&Value>, remote: Option<&Value>) -> Value {
    let (l, r) = (int(local), int(remote));
    Value::from(match (l, r) {
        (0, x) | (x, 0) => x,
        _ => l.min(r),
    })
}

fn either(local: Option<&Value>, remote: Option<&Value>) -> Value {
    let yes = |v: Option<&Value>| v.and_then(Value::as_bool).unwrap_or(false);
    Value::Bool(yes(local) || yes(remote))
}

/// A best per level: each one's best.
fn bests(local: Option<&Value>, remote: Option<&Value>) -> Value {
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let (l, r) = (list(local), list(remote));
    Value::Array((0..l.len().max(r.len())).map(|i| best(l.get(i), r.get(i))).collect())
}

/// The typing game's rounds: the best of both, each round once.
fn rounds(local: Option<&Value>, remote: Option<&Value>) -> Value {
    let mut all: Vec<Value> = Vec::new();
    for v in [local, remote].into_iter().flatten().filter_map(Value::as_array).flatten() {
        if !all.contains(v) {
            all.push(v.clone());
        }
    }
    all.sort_by_key(|r| std::cmp::Reverse(int(r.get("letters"))));
    all.truncate(TYPING_SCORES);
    Value::Array(all)
}

fn object(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

/// The fishing's cards: counted on both sides, the biggest, the first caught.
fn cards(base: Option<&Value>, local: Option<&Value>, remote: Option<&Value>) -> Value {
    let (b, l, r) = (object(base), object(local), object(remote));
    let ids: BTreeSet<&String> = l.keys().chain(r.keys()).collect();
    let mut out = Map::new();
    for id in ids {
        let (cb, cl, cr) = (b.get(id), l.get(id), r.get(id));
        let field = |c: Option<&Value>, f: &str| c.and_then(|c| c.get(f)).cloned();
        let mut card = object(cl.or(cr));
        for f in ["count", "shiny", "sold"] {
            card.insert(f.to_owned(), counter(field(cb, f).as_ref(), field(cl, f).as_ref(), field(cr, f).as_ref()));
        }
        card.insert("best_cm".to_owned(), best(field(cl, "best_cm").as_ref(), field(cr, "best_cm").as_ref()));
        card.insert("first_at".to_owned(), fastest(field(cl, "first_at").as_ref(), field(cr, "first_at").as_ref()));
        // Duplicates sold: never more than the duplicates (a card itself is kept).
        let count = int(card.get("count"));
        let sold = int(card.get("sold")).min((count - 1).max(0));
        if sold == 0 {
            card.remove("sold");
        } else {
            card.insert("sold".to_owned(), Value::from(sold));
        }
        if count > 0 {
            out.insert(id.clone(), Value::Object(card));
        }
    }
    Value::Object(out)
}

/// The baits: what each side won and used.
fn baits(base: Option<&Value>, local: Option<&Value>, remote: Option<&Value>) -> Value {
    let (b, l, r) = (object(base), object(local), object(remote));
    let ids: BTreeSet<&String> = l.keys().chain(r.keys()).chain(b.keys()).collect();
    Value::Object(ids.into_iter().map(|id| (id.clone(), counter(b.get(id), l.get(id), r.get(id)))).filter(|(_, n)| int(Some(n)) > 0).collect())
}

/// Three ways: `base` (the last agreed, `Value::Null` when none), `local`, `remote` (floor's; Null
/// when none). Each keys that travel, merged by its rule.
pub fn merge(base: &Value, local: &Value, remote: &Value) -> Value {
    let mut out = Map::new();
    for key in keys() {
        let (b, l, r) = (base.get(key), local.get(key), remote.get(key));
        let merged = match key {
            "typing_scores" => rounds(l, r),
            "mines_best" => fastest(l, r),
            "riff_bests" | "coffee_bests" => bests(l, r),
            "fish_cards" => cards(b, l, r),
            "fish_baits" => baits(b, l, r),
            "fish_budget" => counter(b, l, r),
            // The days in a row: the last machine opened tells them, carrying on the other's streak
            // when that one was opened the day before (it counts its day from its own last opening,
            // before floor answers: a day on the other machine would otherwise break the streak).
            "open_streak" | "open_last" => {
                let day = |v: &Value| v.get("open_last").and_then(Value::as_str).unwrap_or_default().to_owned();
                let streak = |v: &Value| int(v.get("open_streak"));
                let (later, earlier) = match day(local).cmp(&day(remote)) {
                    std::cmp::Ordering::Less => (remote, local),
                    std::cmp::Ordering::Greater => (local, remote),
                    std::cmp::Ordering::Equal if streak(remote) > streak(local) => (remote, local),
                    std::cmp::Ordering::Equal => (local, remote),
                };
                if key == "open_streak" {
                    let eve = chrono::NaiveDate::parse_from_str(&day(later), "%Y-%m-%d").ok().and_then(|d| d.pred_opt()).map(|d| d.format("%Y-%m-%d").to_string());
                    let carried = if eve.is_some_and(|eve| eve == day(earlier)) { streak(earlier) + 1 } else { 0 };
                    if later.get(key).is_none() && carried == 0 {
                        match l {
                            Some(v) => v.clone(),
                            None => continue,
                        }
                    } else {
                        Value::from(streak(later).max(carried))
                    }
                } else {
                    match later.get(key).or(l) {
                        Some(v) => v.clone(),
                        None => continue,
                    }
                }
            }
            _ => match l.or(r) {
                Some(Value::Bool(_)) => either(l, r),
                Some(Value::Number(_)) => best(l, r),
                // Another kind (added later, unknown here): what changed here, else floor's.
                Some(_) => if l != b { l } else { r.or(l) }.cloned().unwrap_or_default(),
                None => continue,
            },
        };
        out.insert(key.to_owned(), merged);
    }
    Value::Object(out)
}

/// A sync's end.
pub struct Synced {
    pub account: String,
    /// What was sent (the base the app merges its changes made since with).
    pub sent: Value,
    /// floor's progress now.
    pub merged: Value,
}

/// A sync's end taken into `settings`: floor's progress, with what was played here meanwhile on top;
/// it becomes the base.
pub fn absorb(settings: &mut Settings, done: Synced) {
    let now = snapshot(settings);
    let merged = merge(&done.sent, &now, &done.merged);
    if merged != now {
        apply(settings, &merged);
    }
    settings.progress_bases.insert(done.account, done.merged);
}

/// A sync done in the calling thread (what `Progress::sync` does in the background).
#[cfg(test)]
fn sync_now(settings: &mut Settings, account: &FloorAccount) {
    let key = account_key(account);
    let base = settings.progress_bases.get(&key).cloned().unwrap_or_default();
    let (sent, merged) = exchange(&account.url, &account.token, &base, snapshot(settings)).unwrap();
    absorb(settings, Synced { account: key, sent, merged });
}

#[derive(Default)]
struct Shared {
    busy: bool,
    done: Option<Synced>,
    /// The last sync failed (floor out of reach, offline): the next waits `EVERY`.
    failed: bool,
}

/// Kept by the app: the sync running, when the last one was.
#[derive(Default)]
pub struct Progress {
    shared: Arc<Mutex<Shared>>,
    last: Option<f64>,
    /// When the local progress was last compared with its base.
    looked: Option<f64>,
    /// The window had the focus at the last look.
    focused: bool,
}

/// floor asked again, in case another machine played meanwhile.
const EVERY: f64 = 30.0;
/// How often the local progress is compared with the base (a change is sent then).
const LOOK: f64 = 3.0;

impl Progress {
    /// A sync ended: what to merge into the settings.
    pub fn poll(&mut self) -> Option<Synced> {
        self.shared.lock().ok()?.done.take()
    }

    /// Whether to sync now: the first time, the window back in front (the other machine may have
    /// played meanwhile), a change made here, or `EVERY` seconds since the last.
    pub fn due(&mut self, now: f64, focused: bool, local: impl FnOnce() -> bool) -> bool {
        // While a sync runs, the window coming back is seen once it ended.
        let Ok(shared) = self.shared.lock() else { return false };
        if shared.busy {
            return false;
        }
        let failed = shared.failed;
        drop(shared);
        // (Noted every time: a window that lost the focus must be seen coming back.)
        let was = std::mem::replace(&mut self.focused, focused);
        let back = focused && !was;
        if self.last.is_none_or(|last| now - last >= EVERY) {
            return true;
        }
        // floor out of reach: not asked again before `EVERY`, whatever happens here.
        if failed {
            return false;
        }
        if back {
            return true;
        }
        if self.looked.is_some_and(|looked| now - looked < LOOK) {
            return false;
        }
        self.looked = Some(now);
        local()
    }

    /// Merges `local` with floor's in the background (`base`: the last agreed with this account).
    pub fn sync(&mut self, ctx: &egui::Context, now: f64, account: &FloorAccount, base: Value, local: Value) {
        let Some(url) = url().filter(|u| *u == account.url) else { return };
        {
            let Ok(mut shared) = self.shared.lock() else { return };
            if shared.busy {
                return;
            }
            shared.busy = true;
        }
        self.last = Some(now);
        let (shared, ctx, token, key) = (self.shared.clone(), ctx.clone(), account.token.clone(), account_key(account));
        std::thread::spawn(move || {
            let done = exchange(&url, &token, &base, local).map_err(|e| crate::log::info(&format!("progress: {e}"))).ok();
            if let Ok(mut shared) = shared.lock() {
                shared.failed = done.is_none();
                shared.done = done.map(|(sent, merged)| Synced { account: key, sent, merged });
                shared.busy = false;
            }
            ctx.request_repaint();
        });
    }
}

#[derive(serde::Deserialize)]
struct Remote {
    data: Option<Value>,
    rev: i64,
}

/// floor's progress fetched, merged with `local`, sent back if it changed (fetched again and merged
/// again when another machine sent meanwhile). (What was merged, floor's now.)
fn exchange(url: &str, token: &str, base: &Value, local: Value) -> Result<(Value, Value), String> {
    let agent = agent();
    let auth = format!("Bearer {token}");
    let mut remote: Remote = agent.get(format!("{url}/v1/progress")).header("User-Agent", USER_AGENT).header("Authorization", &auth).call().map_err(|e| e.to_string())?.body_mut().read_json().map_err(|e| e.to_string())?;
    let put = crate::floor::account_agent();
    for _ in 0..3 {
        let theirs = remote.data.clone().unwrap_or(Value::Null);
        let merged = merge(base, &local, &theirs);
        if merged == theirs {
            return Ok((local, merged));
        }
        let body = serde_json::json!({ "data": merged, "rev": remote.rev });
        let mut res = put.put(format!("{url}/v1/progress")).header("User-Agent", USER_AGENT).header("Authorization", &auth).send_json(&body).map_err(|e| e.to_string())?;
        match res.status().as_u16() {
            200 => return Ok((local, merged)),
            // Another machine was faster: its progress, merged in turn.
            409 => remote = res.body_mut().read_json().map_err(|e| e.to_string())?,
            status => return Err(format!("floor answered {status}")),
        }
    }
    Err("floor kept changing".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> Settings {
        crate::config::Config::default().settings
    }

    #[test]
    fn two_machines_played_apart_add_up() {
        // The work machine: a run, cards; home: a better run, two shiny ones, a Jurassic theme.
        let mut work = settings();
        work.runner_best = 800;
        work.mines_best = 90_000;
        work.fish_cards.insert("stagiaire".into(), crate::config::FishCard { count: 3, best_cm: 40, shiny: 0, first_at: 200, sold: 1 });
        work.fish_budget = 1;
        work.fish_baits.insert("cafe".into(), 2);
        work.riff_bests = vec![100, 0];
        work.fish_dry = 4;
        let mut home = settings();
        home.runner_best = 900;
        home.mines_best = 120_000;
        home.fish_cards.insert("stagiaire".into(), crate::config::FishCard { count: 2, best_cm: 55, shiny: 2, first_at: 100, sold: 0 });
        home.fish_cards.insert("directeur".into(), crate::config::FishCard { count: 1, best_cm: 80, shiny: 0, first_at: 300, sold: 0 });
        home.jurassic_unlocked = true;
        home.riff_bests = vec![50, 70, 10];

        // Work syncs first (floor empty), then home, then work again.
        let floor = merge(&Value::Null, &snapshot(&work), &Value::Null);
        assert_eq!(floor, snapshot(&work));
        let floor = merge(&Value::Null, &snapshot(&home), &floor);
        let mut back = work.clone();
        apply(&mut back, &merge(&snapshot(&work), &snapshot(&work), &floor));
        assert_eq!(back.runner_best, 900);
        assert_eq!(back.mines_best, 90_000);
        assert!(back.jurassic_unlocked);
        assert_eq!(back.riff_bests, vec![100, 70, 10]);
        let card = &back.fish_cards["stagiaire"];
        assert_eq!((card.count, card.best_cm, card.shiny, card.first_at, card.sold), (5, 55, 2, 100, 1));
        assert_eq!(back.fish_cards["directeur"].count, 1);
        assert_eq!((back.fish_budget, back.fish_baits["cafe"]), (1, 2));
        assert_eq!(back.fish_dry, 4, "each machine's own luck");
    }

    #[test]
    fn what_one_side_spent_isnt_brought_back() {
        let mut s = settings();
        s.fish_baits.insert("cafe".into(), 3);
        s.fish_budget = 50;
        s.fish_cards.insert("stagiaire".into(), crate::config::FishCard { count: 4, sold: 0, ..Default::default() });
        let agreed = snapshot(&s);
        // Here a bait used and two duplicates sold; on floor (the other machine) a bait used too.
        let mut here = s.clone();
        here.fish_baits.insert("cafe".into(), 2);
        here.fish_cards.get_mut("stagiaire").unwrap().sold = 2;
        here.fish_budget = 52;
        let mut there = s.clone();
        there.fish_baits.insert("cafe".into(), 2);
        there.fish_budget = 20;
        there.fish_rod = 1;
        let merged = merge(&agreed, &snapshot(&here), &snapshot(&there));
        apply(&mut s, &merged);
        assert_eq!(s.fish_baits["cafe"], 1);
        assert_eq!(s.fish_cards["stagiaire"].sold, 2);
        assert_eq!((s.fish_budget, s.fish_rod), (22, 1));
        // All used: the bait goes.
        let merged = merge(&merged, &json!({ "fish_baits": {} }), &merged);
        assert!(merged["fish_baits"].as_object().unwrap().is_empty());
    }

    #[test]
    fn nothing_new_changes_nothing() {
        let mut s = settings();
        s.backstage_best = 1200;
        s.open_streak = 3;
        s.open_last = "2026-11-10".into();
        let snap = snapshot(&s);
        assert_eq!(merge(&snap, &snap, &snap), snap);
    }

    #[test]
    fn a_merge_applied_reads_back_the_same() {
        // Otherwise the next look would see a change, and send again every few seconds.
        let mut a = settings();
        a.fish_cards.insert("stagiaire".into(), crate::config::FishCard { count: 3, best_cm: 40, shiny: 1, first_at: 200, sold: 0 });
        a.fish_baits.insert("cafe".into(), 1);
        a.hyperspace_unlocked = true;
        a.coffee_bests = vec![3, 0, 7];
        let mut b = settings();
        b.fish_cards.insert("directeur".into(), crate::config::FishCard { count: 2, best_cm: 80, shiny: 0, first_at: 300, sold: 1 });
        b.typing_scores = vec![crate::config::TypingScore { letters: 250, words: 50, wpm: 50, accuracy: 98, combo: 12, at: 3 }];
        let merged = merge(&Value::Null, &snapshot(&a), &snapshot(&b));
        apply(&mut a, &merged);
        assert_eq!(snapshot(&a), merged);
        assert_eq!(merge(&merged, &snapshot(&a), &merged), merged);
    }

    #[test]
    fn the_typing_rounds_are_each_kept_once() {
        let round = |letters: u32, at: i64| crate::config::TypingScore { letters, words: 1, wpm: 1, accuracy: 100, combo: 1, at };
        let mut a = settings();
        a.typing_scores = vec![round(300, 1), round(200, 2)];
        let mut b = settings();
        b.typing_scores = vec![round(250, 3), round(200, 2)];
        let merged = merge(&Value::Null, &snapshot(&a), &snapshot(&b));
        apply(&mut a, &merged);
        assert_eq!(a.typing_scores.iter().map(|r| r.letters).collect::<Vec<_>>(), vec![300, 250, 200]);
    }

    /// Two machines on one account, through a floor running (RONNIE_FLOOR_URL): each one's progress
    /// played apart ends on both; a bait used at work stays used at home; both sending at once, the
    /// second merges the first's.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn two_machines_share_their_progress() {
        use crate::floor::{account_ask, AccountAsk, AccountDone};
        let url = url().expect("RONNIE_FLOOR_URL");
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let name = format!("Pecheur{stamp}");
        let account = |ask| match account_ask(&url, None, ask) {
            Ok(AccountDone::Account(a)) => a,
            other => panic!("{other:?}"),
        };
        let work_account = account(AccountAsk::SignUp { name: name.clone(), password: "holy diver".into() });
        let home_account = account(AccountAsk::SignIn { name, password: "holy diver".into() });
        let card = |count, shiny| crate::config::FishCard { count, best_cm: 30 + count, shiny, first_at: 100, sold: 0 };

        // Yesterday at home: two shiny cards, a record. Today at work: other fish, a bait.
        let mut home = settings();
        home.fish_cards.insert("stagiaire".into(), card(2, 2));
        home.runner_best = 1500;
        let mut work = settings();
        work.fish_cards.insert("stagiaire".into(), card(3, 0));
        work.fish_cards.insert("directeur".into(), card(1, 0));
        work.fish_baits.insert("cafe".into(), 2);
        work.runner_best = 700;

        sync_now(&mut work, &work_account);
        sync_now(&mut home, &home_account);
        sync_now(&mut work, &work_account);
        for s in [&work, &home] {
            assert_eq!((s.fish_cards["stagiaire"].count, s.fish_cards["stagiaire"].shiny), (5, 2));
            assert_eq!(s.fish_cards["directeur"].count, 1);
            assert_eq!((s.runner_best, s.fish_baits["cafe"]), (1500, 2));
        }

        // A bait used at work, a catch at home: neither comes back nor goes.
        work.fish_baits.insert("cafe".into(), 1);
        home.fish_cards.get_mut("directeur").unwrap().count = 2;
        sync_now(&mut work, &work_account);
        sync_now(&mut home, &home_account);
        sync_now(&mut work, &work_account);
        for s in [&work, &home] {
            assert_eq!((s.fish_baits["cafe"], s.fish_cards["directeur"].count), (1, 2));
        }
        // Nothing new: nothing sent (floor's version stays).
        let before = home.progress_bases.clone();
        sync_now(&mut home, &home_account);
        assert_eq!(home.progress_bases, before);
    }

    #[test]
    fn floor_is_asked_at_the_right_times() {
        let mut p = Progress::default();
        assert!(p.due(0.0, true, || false), "the first time");
        (p.last, p.looked) = (Some(0.0), Some(0.0));
        assert!(!p.due(1.0, true, || true), "a change is looked for every few seconds");
        assert!(p.due(4.0, true, || true), "a change made here");
        assert!(!p.due(8.0, true, || false), "nothing new");
        assert!(!p.due(9.0, false, || false));
        assert!(p.due(10.0, true, || false), "the window back in front");
        assert!(p.due(31.0, false, || false), "now and then");
        p.last = Some(31.0);
        // floor out of reach: only now and then.
        p.shared.lock().unwrap().failed = true;
        assert!(!p.due(32.0, false, || false));
        assert!(!p.due(36.0, true, || true));
        assert!(p.due(61.0, false, || false));
        p.shared.lock().unwrap().busy = true;
        assert!(!p.due(200.0, true, || true), "one at a time");
    }

    #[test]
    fn the_last_machine_opened_tells_the_streak() {
        let mut a = settings();
        (a.open_streak, a.open_last) = (5, "2026-11-09".into());
        let mut b = settings();
        (b.open_streak, b.open_last) = (2, "2026-11-08".into());
        let merged = merge(&Value::Null, &snapshot(&a), &snapshot(&b));
        apply(&mut a, &merged);
        assert_eq!((a.open_streak, a.open_last.as_str()), (5, "2026-11-09"), "a gap of a day: the later one's");
        (b.open_streak, b.open_last) = (2, "2026-11-10".into());
        let merged = merge(&Value::Null, &snapshot(&a), &snapshot(&b));
        apply(&mut a, &merged);
        assert_eq!((a.open_streak, a.open_last.as_str()), (6, "2026-11-10"), "the eve: carried on");
    }

    #[test]
    fn a_day_on_the_other_machine_carries_the_streak_on() {
        // Home, opened yesterday after 4 days in a row; work, today, counted 1 before floor answered.
        let mut work = settings();
        (work.open_streak, work.open_last) = (1, "2026-11-12".into());
        let mut home = settings();
        (home.open_streak, home.open_last) = (4, "2026-11-11".into());
        let merged = merge(&Value::Null, &snapshot(&work), &snapshot(&home));
        apply(&mut work, &merged);
        assert_eq!((work.open_streak, work.open_last.as_str()), (5, "2026-11-12"));
        // The other way round: floor's day is the later one.
        let merged = merge(&Value::Null, &snapshot(&home), &snapshot(&work));
        apply(&mut home, &merged);
        assert_eq!((home.open_streak, home.open_last.as_str()), (5, "2026-11-12"));
        // Already carried (the later side counted on from the same eve): not counted twice.
        let mut a = settings();
        (a.open_streak, a.open_last) = (5, "2026-11-12".into());
        let mut b = settings();
        (b.open_streak, b.open_last) = (4, "2026-11-11".into());
        let merged = merge(&Value::Null, &snapshot(&a), &snapshot(&b));
        apply(&mut a, &merged);
        assert_eq!(a.open_streak, 5);
    }
}
