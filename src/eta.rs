//! How long a task still takes ("encore 3 min"), from how far it went and for how long: the time
//! spent so far shared out over what is left, once enough of it went by to tell.

use std::time::{Duration, Instant};

use crate::i18n::Strings;

/// Below it, too early to tell.
const TOO_EARLY: Duration = Duration::from_secs(2);

/// Follows one task: fed with the fraction done, it says the time left. A fraction going back (the
/// next step of the task) starts it over.
pub struct Eta {
    started: Instant,
    last: f32,
}

impl Default for Eta {
    fn default() -> Self {
        Self { started: Instant::now(), last: 0.0 }
    }
}

impl Eta {
    /// The time left, the fraction now done being `fraction` (0 to 1).
    pub fn update(&mut self, fraction: f32) -> Option<Duration> {
        if fraction + 0.001 < self.last {
            self.started = Instant::now();
        }
        self.last = fraction;
        left(fraction, self.started.elapsed())
    }

    /// The time left, said ("encore 3 min"), empty while too early to tell.
    pub fn said(&mut self, fraction: f32, t: &Strings) -> String {
        self.update(fraction).map(|d| say(d, t)).unwrap_or_default()
    }
}

/// The time left once `fraction` of a task is done in `elapsed`, if enough went by to tell.
pub fn left(fraction: f32, elapsed: Duration) -> Option<Duration> {
    if elapsed < TOO_EARLY || !(0.005..1.0).contains(&fraction) {
        return None;
    }
    let fraction = f64::from(fraction);
    Some(Duration::from_secs_f64(elapsed.as_secs_f64() * (1.0 - fraction) / fraction))
}

/// A time left, said: "encore quelques secondes", "encore 40 s", "encore 12 min", "encore 1 h 05".
pub fn say(left: Duration, t: &Strings) -> String {
    let secs = left.as_secs();
    let time = if secs < 10 {
        t.eta_seconds.to_owned()
    } else if secs < 60 {
        // By 5 s: it doesn't flicker.
        format!("{} s", secs.div_ceil(5) * 5)
    } else if secs < 3600 {
        format!("{} min", secs.div_ceil(60))
    } else {
        let minutes = secs.div_ceil(60);
        format!("{} h {:02}", minutes / 60, minutes % 60)
    };
    t.eta_left.replace("{time}", &time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_early_or_done_says_nothing() {
        assert_eq!(left(0.5, Duration::from_secs(1)), None);
        assert_eq!(left(0.0, Duration::from_secs(30)), None);
        assert_eq!(left(1.0, Duration::from_secs(30)), None);
    }

    #[test]
    fn the_time_spent_shared_out_over_what_is_left() {
        assert_eq!(left(0.25, Duration::from_secs(60)), Some(Duration::from_secs(180)));
        assert_eq!(left(0.5, Duration::from_secs(10)), Some(Duration::from_secs(10)));
    }

    #[test]
    fn said_in_seconds_minutes_hours() {
        let t = &crate::i18n::FR;
        assert_eq!(say(Duration::from_secs(4), t), "encore quelques secondes");
        assert_eq!(say(Duration::from_secs(37), t), "encore 40 s");
        assert_eq!(say(Duration::from_secs(61), t), "encore 2 min");
        assert_eq!(say(Duration::from_secs(3900), t), "encore 1 h 05");
    }

    #[test]
    fn a_new_step_starts_over() {
        let mut eta = Eta { started: Instant::now() - Duration::from_secs(100), last: 0.9 };
        assert_eq!(eta.update(0.1), None);
    }
}
