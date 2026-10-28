//! The games during working hours: one game of Speed Metal, one of Puissance 4, one of the
//! minesweeper, one run of Ronnie Run, one song of Riff Hero and one show of Backstage a morning and an afternoon, and 10 minutes of Ronnie.io. Free outside them (before 8:30, 12:00 to 14:00, after
//! 18:00 — 17:20 on Fridays — and at the weekend). Not a setting: Ronnie is for work first.
//!
//! What was played is kept in the config file, sealed (an HMAC of this machine and user): edited
//! during working hours, it counts as all played for the half-day. A record never written (the first
//! start of a version with the limit) starts afresh.

use chrono::{Datelike, NaiveDateTime, NaiveTime, Weekday};
use sha2::{Digest, Sha256};

use crate::config::GameUsage;

/// Seconds of Ronnie.io a half-day.
pub const BLOB_SECS: u32 = 10 * 60;

/// A half-day of work.
#[derive(Clone, Debug, PartialEq)]
pub struct Period {
    /// Tells it from the others ("2026-10-12 am"): what was played is counted for it.
    pub key: String,
    pub morning: bool,
    /// When the games are free again.
    pub end: NaiveTime,
}

fn at(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

/// The half-day of work `now` is in, if any.
pub fn period(now: NaiveDateTime) -> Option<Period> {
    let day = now.weekday();
    if matches!(day, Weekday::Sat | Weekday::Sun) {
        return None;
    }
    let evening = if day == Weekday::Fri { at(17, 20) } else { at(18, 0) };
    let time = now.time();
    let (morning, end) = if (at(8, 30)..at(12, 0)).contains(&time) {
        (true, at(12, 0))
    } else if (at(14, 0)..evening).contains(&time) {
        (false, evening)
    } else {
        return None;
    };
    Some(Period { key: format!("{} {}", now.date(), if morning { "am" } else { "pm" }), morning, end })
}

/// What was played in the half-day `now` is in, the record checked: a new half-day starts afresh, a
/// record edited counts as all played. Sealed again when changed.
pub fn checked(usage: &mut GameUsage, now: NaiveDateTime) -> Option<Period> {
    let p = period(now);
    // Never written: nothing played yet (it locked every game at the first start, unplayed).
    let valid = usage.seal == seal(usage) || *usage == GameUsage::default();
    let fresh = |period: &str| GameUsage { period: period.to_owned(), ..GameUsage::default() };
    let wanted = match &p {
        // Out of working hours nothing is limited: a record to trust again from here.
        None if !valid => Some(fresh("")),
        Some(p) if !valid => Some(GameUsage { period: p.key.clone(), speed_metal: true, four: true, blob_secs: BLOB_SECS, mines: true, runner: true, riff: true, backstage: true, seal: String::new() }),
        Some(p) if usage.period != p.key => Some(fresh(&p.key)),
        _ => None,
    };
    if let Some(wanted) = wanted {
        *usage = wanted;
        reseal(usage);
    }
    p
}

/// To call after changing `usage`.
pub fn reseal(usage: &mut GameUsage) {
    usage.seal = seal(usage);
}

fn seal(usage: &GameUsage) -> String {
    let who = format!("{}|{}", std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default(), std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default());
    let mut message = format!("{}|{}|{}|{}", usage.period, usage.speed_metal, usage.four, usage.blob_secs);
    // Only once played: the records sealed before these games stay valid.
    if usage.mines {
        message.push_str("|mines");
    }
    if usage.runner {
        message.push_str("|runner");
    }
    if usage.riff {
        message.push_str("|riff");
    }
    if usage.backstage {
        message.push_str("|backstage");
    }
    hmac(format!("ronnie-playtime-7f3a9c|{who}").as_bytes(), message.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// HMAC-SHA256 (RFC 2104).
fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(message).finalize();
    Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn hmac_matches_rfc_4231() {
        let out = hmac(b"Jefe", b"what do ya want for nothing?");
        let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn edited_counts_as_played() {
        let monday = on(2026, 10, 12, 9, 0);
        let mut usage = GameUsage::default();
        // Never written (the first start with the limit): nothing played yet.
        checked(&mut usage, monday);
        assert!(!usage.speed_metal && !usage.four && usage.blob_secs == 0 && usage.period == "2026-10-12 am");
        // Edited by hand without a seal: all played.
        let mut usage = GameUsage { period: "2026-10-12 am".into(), ..GameUsage::default() };
        checked(&mut usage, monday);
        assert!(usage.speed_metal && usage.four && usage.blob_secs == BLOB_SECS);
        // Trusted again out of working hours, fresh at the next half-day.
        let mut usage = GameUsage::default();
        checked(&mut usage, on(2026, 10, 12, 12, 30));
        checked(&mut usage, on(2026, 10, 12, 14, 0));
        assert!(!usage.speed_metal && usage.period == "2026-10-12 pm");
        // Played, then edited by hand: all played.
        usage.speed_metal = true;
        reseal(&mut usage);
        usage.speed_metal = false;
        checked(&mut usage, on(2026, 10, 12, 15, 0));
        assert!(usage.speed_metal && usage.four && usage.mines && usage.runner && usage.riff && usage.backstage);
    }

    #[test]
    fn a_seal_made_before_the_minesweeper_holds() {
        let monday = on(2026, 10, 12, 9, 0);
        let mut usage = GameUsage::default();
        checked(&mut usage, monday);
        usage.four = true;
        reseal(&mut usage);
        let before = usage.seal.clone();
        // The seal a version without the minesweeper made of this record.
        let who = format!("{}|{}", std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default(), std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default());
        let old: String = hmac(format!("ronnie-playtime-7f3a9c|{who}").as_bytes(), format!("{}|false|true|0", usage.period).as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(before, old);
        checked(&mut usage, monday);
        assert!(usage.four && !usage.speed_metal && !usage.mines);
        usage.mines = true;
        reseal(&mut usage);
        assert_ne!(usage.seal, before);
    }

    fn on(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }

    #[test]
    fn working_hours() {
        // Monday 12 October 2026.
        assert_eq!(period(on(2026, 10, 12, 8, 29)), None);
        let morning = period(on(2026, 10, 12, 8, 30)).unwrap();
        assert!(morning.morning);
        assert_eq!(morning.end, at(12, 0));
        assert_eq!(morning.key, "2026-10-12 am");
        assert_eq!(period(on(2026, 10, 12, 12, 0)), None);
        assert_eq!(period(on(2026, 10, 12, 13, 59)), None);
        let afternoon = period(on(2026, 10, 12, 17, 59)).unwrap();
        assert!(!afternoon.morning);
        assert_eq!(afternoon.end, at(18, 0));
        assert_ne!(afternoon.key, morning.key);
        assert_eq!(period(on(2026, 10, 12, 18, 0)), None);
    }

    #[test]
    fn friday_ends_early_weekend_free() {
        // Friday 16 October 2026.
        assert_eq!(period(on(2026, 10, 16, 17, 19)).unwrap().end, at(17, 20));
        assert_eq!(period(on(2026, 10, 16, 17, 20)), None);
        assert_eq!(period(on(2026, 10, 17, 10, 0)), None);
        assert_eq!(period(on(2026, 10, 18, 15, 0)), None);
    }
}
