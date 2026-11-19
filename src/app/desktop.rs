//! The rewards over the whole screen, outside Ronnie too: the pointer's trail, the pointer chosen (the
//! wand, the lightsaber, the sword, Ronnie Métal's arrow, the stapler), the sheep, the droid, Gérard and the house-elf walking along the foot of the screen,
//! the unicorn galloping by.
//!
//! They live in a process of their own (`ronnie desktop-fx`), started by Ronnie while one of them is
//! chosen: a transparent window over the screen where the pointer is, letting every click through, out of
//! the Dock and the task bar, above the other windows (full-screen ones too, on macOS). It keeps going
//! while Ronnie's window is minimized or behind others, and ends with Ronnie: what to show comes on its
//! standard input, one JSON line at each change, and the input closing (Ronnie gone) ends it. The
//! pointer is read about 60 times a second; the window is only drawn again when something moves.
//!
//! With a pointer of Ronnie's, the system's pointer is hidden everywhere on macOS (it comes back by
//! itself when the process ends, even crashing); on Windows it stays, the wand drawn at its tip. macOS
//! and Windows; Linux through X11 (XWayland under Wayland), with a compositor.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::config::Pointer;

use super::*;

/// What the effects' process shows.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct Fx {
    trail: bool,
    sheep: bool,
    droid: bool,
    /// Lundi matin's horde, at 10.
    #[serde(default)]
    horde: bool,
    /// Gérard, Lundi matin's kind zombie, strolling now and then.
    #[serde(default)]
    gerard: bool,
    /// The horde run once right now, to see it (switched on): when, in Unix milliseconds.
    #[serde(default)]
    horde_demo: Option<i64>,
    /// The zombies met in Lundi matin (places in its bestiary): the horde is made of them.
    #[serde(default)]
    horde_kinds: Vec<u8>,
    /// Mathéo, Riff Hero's green plant, at the foot of the screen (to drag elsewhere).
    #[serde(default)]
    matheo: bool,
    /// His advice in a bubble now and then (water, stretches, steps).
    #[serde(default)]
    matheo_tips: bool,
    /// The language his advice is in.
    #[serde(default)]
    lang: crate::i18n::Lang,
    #[serde(default)]
    elf: bool,
    #[serde(default)]
    unicorn: bool,
    #[serde(default)]
    wolf: bool,
    /// The trail in bank notes (Ronnie Corp's).
    #[serde(default)]
    bills: bool,
    pointer: Option<Pointer>,
    /// The theme whose colors the trail takes.
    theme: String,
}


impl Fx {
    fn any(&self) -> bool {
        self.trail || self.sheep || self.droid || self.horde || self.gerard || self.matheo || self.elf || self.unicorn || self.wolf || self.pointer.is_some()
    }

    /// The pointer is followed (else it needn't be): Mathéo too, to be grabbed.
    fn follows(&self) -> bool {
        self.trail || self.matheo || self.pointer.is_some()
    }
}

/// The systems where it works: Linux, with an X11 display (XWayland under Wayland).
pub(super) fn supported() -> bool {
    cfg!(any(target_os = "macos", windows)) || (cfg!(unix) && std::env::var_os("DISPLAY").is_some())
}

/// The effects' process, from Ronnie's side.
#[derive(Default)]
pub(super) struct Desktop {
    child: Option<(std::process::Child, std::process::ChildStdin)>,
    /// What it was told last.
    sent: Option<Fx>,
    /// Starts that ended at once, in a row: past a few, not started again (until settings change).
    failures: u32,
    started: Option<Instant>,
    /// The horde just switched on: run once now (Unix milliseconds).
    pub(super) horde_demo: Option<i64>,
}

impl Drop for Desktop {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Desktop {
    fn stop(&mut self) {
        if let Some((mut child, stdin)) = self.child.take() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }
        self.sent = None;
    }
}

impl App {
    /// What the settings ask to show over the screen (each reward still to be won, left out; nothing
    /// unless switched on outside Ronnie).
    fn desktop_wanted(&self) -> Fx {
        if !self.config.settings.desktop_fx.on {
            return Fx::default();
        }
        self.companions()
    }

    /// Every companion, the trail and the pointer as the settings choose them (each won), wherever they
    /// show: over every screen, or only in Ronnie's window.
    fn companions(&self) -> Fx {
        let s = &self.config.settings;
        let d = s.desktop_fx;
        let official = crate::config::OFFICIAL;
        let worn = trail::worn(s);
        let pointer = Some(trail::pointer(s)).filter(|p| *p != Pointer::System);
        Fx { trail: worn != crate::config::Trail::Off, sheep: s.sheep && office::sheep_unlocked(s), droid: d.droid && (!official || s.hyperspace_unlocked), horde: d.horde && (!official || s.monday_horde), gerard: d.gerard && (!official || s.monday_gerard), horde_demo: self.desktop.horde_demo.filter(|_| d.horde), horde_kinds: super::zombie::met_kinds(s), matheo: d.matheo && (!official || s.riff_games >= super::riff::MATHEO_GAMES), matheo_tips: d.matheo_tips, lang: s.language, elf: d.elf && (!official || s.wizard_unlocked), unicorn: d.unicorn && wallstreet::unicorn_unlocked(s), wolf: d.wolf && (!official || crate::coffee::thrones_won(&s.coffee_bests)), bills: worn == crate::config::Trail::Bills, pointer, theme: s.theme.clone() }
    }

    /// The pointer's effects are drawn by the process over the screen (Ronnie doesn't draw its own:
    /// they would show twice).
    pub(super) fn desktop_draws(&self) -> (bool, bool, bool) {
        let fx = self.desktop.sent.as_ref().filter(|_| self.desktop.child.is_some());
        (fx.is_some_and(|f| f.trail), fx.is_some_and(|f| f.pointer.is_some()), fx.is_some_and(|f| f.sheep))
    }

    /// Not over every screen (switched off there, or the process not running): the companions walk along
    /// the foot of the terminals, in Ronnie's window (the sheep, its own way: `office::sheep_walk`), the
    /// horde runs across them at 10, Mathéo stands at their right.
    pub(super) fn companions_walk(&mut self, ctx: &egui::Context) {
        if self.desktop.child.is_some() && self.desktop.sent.as_ref().is_some_and(Fx::any) {
            return;
        }
        let Some(area) = self.terminals_rect else { return };
        let fx = Fx { sheep: false, trail: false, pointer: None, ..self.companions() };
        if !(fx.droid || fx.gerard || fx.elf || fx.unicorn || fx.wolf || fx.horde || fx.matheo) {
            return;
        }
        let now = ctx.input(|i| i.time);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("companions-in-ronnie")));
        let strip = Rect::from_min_max(Pos2::new(area.min.x + 6.0, area.max.y - 34.0), Pos2::new(area.max.x - 6.0, area.max.y - 2.0));
        let (busy, mut next) = paint_walkers(&painter, strip, area, now, 0, 1, &fx);
        if fx.matheo {
            paint_matheo(&painter, Pos2::new(area.max.x - 46.0, area.max.y - 4.0), 56.0, now, false);
            next = next.min(0.1);
        }
        if busy {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(next.clamp(0.02, 1.0)));
        }
    }

    /// The process started, told what changed, or stopped, as the settings say.
    pub(super) fn desktop_tick(&mut self) {
        if !supported() {
            return;
        }
        let fx = self.desktop_wanted();
        // Ended by itself: noted, started again below (not after failing again and again).
        if let Some((child, _)) = self.desktop.child.as_mut()
            && !matches!(child.try_wait(), Ok(None))
        {
            let quick = self.desktop.started.is_some_and(|t| t.elapsed() < Duration::from_secs(10));
            self.desktop.failures = if quick { self.desktop.failures + 1 } else { 0 };
            crate::log::info("desktop effects: the process ended");
            self.desktop.child = None;
            self.desktop.sent = None;
        }
        if !fx.any() {
            self.desktop.stop();
            self.desktop.failures = 0;
            return;
        }
        if self.desktop.sent.as_ref() == Some(&fx) {
            return;
        }
        if self.desktop.child.is_none() {
            if self.desktop.failures >= 3 {
                return;
            }
            let Ok(exe) = std::env::current_exe() else { return };
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("desktop-fx").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            // Linux: through X11 (XWayland under Wayland), where a window places itself and stays above,
            // in pixels (as the screens are read).
            #[cfg(all(unix, not(target_os = "macos")))]
            cmd.env_remove("WAYLAND_DISPLAY").env("WINIT_UNIX_BACKEND", "x11").env("WINIT_X11_SCALE_FACTOR", "1");
            match cmd.spawn() {
                Ok(mut child) => {
                    let Some(stdin) = child.stdin.take() else { return };
                    self.desktop.child = Some((child, stdin));
                    self.desktop.started = Some(Instant::now());
                }
                Err(e) => {
                    crate::log::error(&format!("desktop effects: {e}"));
                    self.desktop.failures += 1;
                    return;
                }
            }
        }
        let line = serde_json::to_string(&fx).unwrap_or_default();
        let ok = self.desktop.child.as_mut().is_some_and(|(_, stdin)| writeln!(stdin, "{line}").and_then(|()| stdin.flush()).is_ok());
        if ok {
            self.desktop.sent = Some(fx);
        } else {
            self.desktop.stop();
        }
    }
}

/// The pointer as last read: where (in points, the screens' space), the main button down.
#[derive(Clone, Copy, Default, PartialEq)]
struct Mouse {
    at: Option<Pos2>,
    pressed: bool,
}

/// The effects' window.
struct Overlay {
    fx: Arc<Mutex<Option<Fx>>>,
    mouse: Arc<Mutex<Mouse>>,
    shown: Fx,
    theme: Theme,
    /// The screen covered and its part left by the Dock, the menu bar or the task bar (points).
    screen: Rect,
    work: Rect,
    trail: trail::Trail,
    wand: wizard::Wand,
    pressed: bool,
    last: Option<f64>,
    /// The system's pointer is the plain arrow (else a hand, a beam, a resize...: left as it is), and
    /// when last looked at.
    plain: (bool, Option<Instant>),
    /// The screens another app fills (a film, a full-screen window): nothing shown there; and when
    /// last looked at.
    full: (Vec<Rect>, Option<Instant>),
    matheo: Plant,
    /// The windows over the other screens, by screen: kept a while once nothing goes along them
    /// (hidden), not made again at each walk.
    others: std::collections::HashMap<usize, Other>,
}

/// A window over another screen than the pointer's.
struct Other {
    /// Frames drawn in it: it shows from the second on (before, a new window is a white square on
    /// Windows, not yet its size, its place nor see-through).
    frames: u32,
    /// When something last went along its screen.
    needed: Instant,
}

/// How long a window over another screen is kept, hidden, once nothing goes along it.
const OTHER_KEPT: Duration = Duration::from_secs(30);

/// Mathéo where he stands (his pot's foot, screen points), falling, or held.
struct Plant {
    at: Option<Pos2>,
    vy: f32,
    /// Held by the mouse: from the pointer to his foot.
    grab: Option<Vec2>,
    /// The window lets the mouse through (not over him).
    pass: bool,
    /// Where the press that grabbed him was: let go without moving, a click (his advice).
    press: Option<Pos2>,
    /// Thrown: his speed across (points a second), his tilt (radians) and how fast it turns.
    vx: f32,
    tilt: f32,
    spin: f32,
    /// Held: where he was the last frames (time, place), for his swing and his speed when let go.
    track: std::collections::VecDeque<(f64, Pos2)>,
    /// Held: when he was last swung back the other way, and which way he went (shaken: quiet).
    flips: Vec<f64>,
    way: f32,
    /// Quiet until then (Unix seconds): shaken, he keeps his advice to himself for 6 hours.
    quiet: Option<u64>,
    /// His head spinning until then (shaken, or banged against a wall).
    dizzy: Option<Instant>,
    /// When he last hit a wall, the ceiling or the floor hard (squashed a moment).
    bonk: Option<Instant>,
    /// What his bubble says and since when.
    tip: Option<(Said, Instant)>,
    /// The watering can beside him (with each reminder to drink): since when, and since when it pours.
    can: Option<(Instant, Option<Instant>)>,
    /// Watered: glad until then.
    glad: Option<Instant>,
    /// Reminders to drink left without watering him (he wilts), up to `THIRST_MAX`.
    thirst: u32,
    /// Late in the evening, as last looked at (each minute).
    tired: (bool, Option<Instant>),
    /// Friday afternoon, as last looked at (with `tired`).
    friday: bool,
    /// At the screen since then without a break (the pointer never still `BREAK` long), and when he
    /// last asked for one.
    active_since: Instant,
    break_said: Option<Instant>,
    /// Waving at Gérard (towards his side), and whether this walk of his was greeted.
    wave: f32,
    greeted: bool,
    /// Hiding from the horde (0 to 1), and whether he was.
    hide: f32,
    hiding: bool,
    /// How he grew: the day, waterings that day, days well watered (`GOOD_DAY` waterings).
    growth: (String, u32, u32),
    /// When the next one is due, and the number of the next.
    next_tip: Instant,
    told: usize,
    /// The pointer's last place and when it last moved: nobody there, no advice.
    moved: (Option<Pos2>, Instant),
    /// His bubble on the screens (points), as last drawn: a click there closes it.
    bubble: Option<Rect>,
}

impl Default for Plant {
    fn default() -> Self {
        // Not the same advice first at each start (water first all the same).
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 60) as usize;
        let quiet = matheo_quiet_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| t.trim().parse().ok());
        Self { at: matheo_load(), vy: 0.0, grab: None, pass: true, press: None, vx: 0.0, tilt: 0.0, spin: 0.0, track: Default::default(), flips: Vec::new(), way: 0.0, quiet, dizzy: None, bonk: None, tip: None, can: None, glad: None, thirst: matheo_thirst_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| t.trim().parse().ok()).unwrap_or(0), tired: (false, None), friday: false, active_since: Instant::now(), break_said: None, wave: 0.0, greeted: false, hide: 0.0, hiding: false, growth: matheo_growth_load(), next_tip: Instant::now() + TIP_FIRST, told: seed % 60 * 2, moved: (None, Instant::now()), bubble: None }
    }
}

impl Plant {
    /// His next advice in his bubble; with a reminder to drink, the watering can beside him.
    /// Late in the evening, off to bed instead (no can then); thirsty, he says so.
    fn advise(&mut self) {
        let k = self.told;
        self.told += 1;
        self.next_tip = Instant::now() + TIP_EVERY;
        if late() {
            self.tip = Some((Said::Bed(k / 2), Instant::now()));
            return;
        }
        let water = k.is_multiple_of(2);
        let long = self.active_since.elapsed() >= LONG_WORK && self.break_said.is_none_or(|t| t.elapsed() >= TIP_EVERY);
        let said = if long {
            self.break_said = Some(Instant::now());
            Said::Break(k / 2)
        } else if water && self.thirst >= 2 {
            Said::Thirsty
        } else if !water && friday() {
            // Friday afternoon: the weekend in place of moving.
            Said::Weekend(k / 2)
        } else {
            Said::Tip(k)
        };
        self.tip = Some((said, Instant::now()));
        if water && self.can.is_none_or(|(_, pour)| pour.is_none()) {
            self.can = Some((Instant::now(), None));
        }
    }

    /// Thirstier (a can left unused) or watered (0): kept beside the settings.
    fn set_thirst(&mut self, thirst: u32) {
        self.thirst = thirst.min(THIRST_MAX);
        if let Some(f) = matheo_thirst_file() {
            let _ = std::fs::write(f, self.thirst.to_string());
        }
    }

    /// Watered once more: a day with `GOOD_DAY` waterings counts as well watered (kept).
    fn watered(&mut self) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if self.growth.0 != today {
            self.growth = (today, 0, self.growth.2);
        }
        self.growth.1 += 1;
        if self.growth.1 == GOOD_DAY {
            self.growth.2 += 1;
        }
        if let Some(f) = matheo_growth_file() {
            let _ = std::fs::write(f, serde_json::to_string(&self.growth).unwrap_or_default());
        }
    }

    /// How wilted he looks (0 to 1).
    fn wilt(&self) -> f32 {
        self.thirst as f32 / THIRST_MAX as f32
    }
}

/// The pointer still that long: a break (two hours of work on end, he insists).
const BREAK: Duration = Duration::from_secs(5 * 60);
const LONG_WORK: Duration = Duration::from_secs(2 * 3600);
/// Waterings in a day for it to count as well watered; such days for a leaf more (up to 3), and
/// for his flower.
const GOOD_DAY: u32 = 3;
const DAYS_A_LEAF: u32 = 5;
const DAYS_A_FLOWER: u32 = 20;

fn matheo_growth_file() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|d| d.join("matheo-growth.json"))
}

fn matheo_growth_load() -> (String, u32, u32) {
    matheo_growth_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Reminders to drink left without watering him, at most: then fully wilted.
const THIRST_MAX: u32 = 3;

fn matheo_thirst_file() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|d| d.join("matheo-thirst.json"))
}

/// Late in the evening (from 11 pm to 5 am): he's tired, and sends you to bed.
fn late() -> bool {
    use chrono::Timelike;
    if let Some(forced) = forced_clock() {
        return forced == "late";
    }
    let h = chrono::Local::now().hour();
    !(5..23).contains(&h)
}

/// Friday afternoon (from 2 pm, until he's tired): sunglasses on, the weekend is near.
fn friday() -> bool {
    use chrono::{Datelike, Timelike};
    if let Some(forced) = forced_clock() {
        return forced == "friday";
    }
    let now = chrono::Local::now();
    now.weekday() == chrono::Weekday::Fri && (14..23).contains(&now.hour())
}

/// Debug builds: `RONNIE_MATHEO_GALLERY=1` shows each of his looks side by side in the middle of
/// the screen, alive, to see them all at once.
fn paint_matheo_gallery(painter: &egui::Painter, area: Rect, now: f64) {
    let base = Look::default();
    let pour = 0.3 + (now % 3.6) as f32;
    let looks: [(&str, Look, Option<f32>); 10] = [
        ("Normal", base, None),
        ("Tenu", Look { held: true, ..base }, None),
        ("Lancé", Look { tilt: (now as f32 * 6.0) % std::f32::consts::TAU, ..base }, None),
        ("Contre un mur", Look { squash: 0.3 * (1.0 - ((now % 1.2) as f32 / 0.25).min(1.0)), dizzy: true, ..base }, None),
        ("Secoué (6 h)", Look { dizzy: true, ..base }, None),
        ("L'arrosoir", base, Some(-1.0)),
        ("Arrosé", Look { glad: pour > POUR_FROM + 0.3, ..base }, Some(pour)),
        ("Soif (1/3)", Look { wilt: 1.0 / 3.0, ..base }, None),
        ("Dépéri (3/3)", Look { wilt: 1.0, ..base }, None),
        ("Le soir, 23 h", Look { tired: true, ..base }, None),
    ];
    let more = [
        ("Vendredi aprèm", Look { shades: true, ..base }, None),
        ("2 h sans pause", Look { hop: { let e = (now % 3.0) as f32; if e < 2.0 { (e * 9.0).sin().abs() * 8.0 * (1.0 - e / 2.0) } else { 0.0 } }, ..base }, None),
        ("Salut Gérard", Look { wave: 1.0, ..base }, None),
        ("La horde, 10 h", Look { hide: 1.0, ..base }, None),
        ("Grandi, 1 mois", Look { extra: 3, flower: true, ..base }, None),
    ];
    let all: Vec<_> = looks.into_iter().chain(more).collect();
    let step = (area.width() / all.len() as f32).min(130.0);
    let left = area.center().x - step * (all.len() as f32 - 1.0) / 2.0;
    let y = area.center().y + 60.0;
    painter.rect_filled(Rect::from_min_max(Pos2::new(left - step / 2.0, y - 190.0), Pos2::new(left + step * (all.len() as f32 - 0.5), y + 40.0)), 14.0, Color32::from_rgba_unmultiplied(30, 32, 40, 200));
    for (i, (label, look, can)) in all.into_iter().enumerate() {
        let foot = Pos2::new(left + step * i as f32, y);
        paint_matheo_as(painter, foot, MATHEO_H, now, look);
        if let Some(t) = can {
            let pouring = (t >= 0.0).then(|| Instant::now() - Duration::from_secs_f32(t));
            let v = can_view(foot, 1.0, Instant::now() - Duration::from_secs(5), pouring);
            paint_can(painter, v, Vec2::ZERO);
        }
        painter.text(foot + Vec2::new(0.0, 22.0), egui::Align2::CENTER_CENTER, label, FontId::proportional(13.0), Color32::from_rgb(235, 235, 240));
    }
}

/// Debug builds: `RONNIE_MATHEO_CLOCK=late|friday` makes it so, to see him.
fn forced_clock() -> Option<String> {
    if cfg!(debug_assertions) { std::env::var("RONNIE_MATHEO_CLOCK").ok() } else { None }
}

impl Plant {
    /// His look's own part: held, tilted, dizzy, squashed, glad, tired, wilted.
    fn look(&self, tired: bool, shades: bool) -> Look {
        Look {
            held: self.grab.is_some(),
            tilt: self.tilt,
            dizzy: self.dizzy.is_some_and(|d| Instant::now() < d),
            squash: self.bonk.map_or(0.0, |b| (1.0 - b.elapsed().as_secs_f32() / 0.25).max(0.0) * 0.3),
            glad: self.glad.is_some_and(|g| Instant::now() < g) || self.can.and_then(|c| c.1).is_some_and(|p| p.elapsed().as_secs_f32() > POUR_FROM + 0.3),
            tired,
            shades: shades && !tired,
            wave: self.wave,
            hide: self.hide,
            hop: match self.tip {
                // Asking for a break: hopping a moment every ten seconds.
                Some((Said::Break(_), since)) => {
                    let e = since.elapsed().as_secs_f32() % 10.0;
                    if e < 2.0 { (e * 9.0).sin().abs() * 8.0 * (1.0 - e / 2.0) } else { 0.0 }
                }
                _ => 0.0,
            },
            extra: (self.growth.2 / DAYS_A_LEAF).min(3) as usize,
            flower: self.growth.2 >= DAYS_A_FLOWER,
            wilt: self.wilt(),
        }
    }
}

/// Mathéo's first advice after the start, then one every half hour (water and moving in turns).
const TIP_FIRST: Duration = Duration::from_secs(20 * 60);
const TIP_EVERY: Duration = Duration::from_secs(30 * 60);
/// How long his bubble stays.
const TIP_SHOWN: Duration = Duration::from_secs(45);
/// The pointer still that long: nobody at the screen, the advice waits.
const TIP_IDLE: Duration = Duration::from_secs(3 * 60);
/// Away that long (a break, a meeting): back, a quarter of an hour before the next.
const TIP_AWAY: Duration = Duration::from_secs(10 * 60);

/// Shaken that long, he's quiet.
const QUIET_SECS: u64 = 6 * 3600;
/// Swung back the other way that many times within `SHAKE_WITHIN` seconds: shaken.
const SHAKE_FLIPS: usize = 6;
const SHAKE_WITHIN: f64 = 1.6;

/// What Mathéo's bubble says.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Said {
    /// His advice number `k` (see `matheo_tip`).
    Tip(usize),
    /// Being shaken.
    Shaking,
    /// Shaken enough: quiet for 6 hours.
    Quiet,
    /// Banged against the edge of the screen.
    Ouch,
    /// Watered.
    Thanks,
    /// Late in the evening, his advice number `k`: off to bed.
    Bed(usize),
    /// Not watered for a while: thirsty, both of you.
    Thirsty,
    /// Friday afternoon, his advice number `k`: the weekend is near.
    Weekend(usize),
    /// Two hours on end without a break: number `k`, he insists (hopping).
    Break(usize),
    /// Gérard passing by.
    Hello,
    /// The 10 o'clock horde running across: hiding.
    Hiding,
    /// The horde gone.
    Gone,
}

impl Said {
    /// How long the bubble stays.
    fn lasts(self) -> Duration {
        match self {
            Said::Tip(_) => TIP_SHOWN,
            Said::Shaking => Duration::from_millis(700),
            Said::Quiet => Duration::from_secs(7),
            Said::Ouch => Duration::from_millis(1200),
            Said::Thanks => Duration::from_millis(3500),
            Said::Bed(_) | Said::Thirsty | Said::Weekend(_) => TIP_SHOWN,
            Said::Break(_) => TIP_SHOWN * 2,
            Said::Hello | Said::Gone => Duration::from_secs(4),
            Said::Hiding => Duration::from_millis(1500),
        }
    }

    fn text(self, lang: crate::i18n::Lang) -> &'static str {
        let t = lang.strings();
        match self {
            Said::Tip(k) => matheo_tip(lang, k),
            Said::Shaking => t.matheo_shaking,
            Said::Quiet => t.matheo_quiet,
            Said::Ouch => t.matheo_ouch,
            Said::Thanks => t.matheo_thanks,
            Said::Bed(k) => t.matheo_bed.get(k % t.matheo_bed.len().max(1)).copied().unwrap_or_default(),
            Said::Thirsty => t.matheo_thirsty,
            Said::Weekend(k) => t.matheo_weekend.get(k % t.matheo_weekend.len().max(1)).copied().unwrap_or_default(),
            Said::Break(k) => t.matheo_break.get(k % t.matheo_break.len().max(1)).copied().unwrap_or_default(),
            Said::Hello => t.matheo_hello,
            Said::Hiding => t.matheo_hiding,
            Said::Gone => t.matheo_gone,
        }
    }
}

/// Mathéo's advice number `k`: water for the even ones, moving for the odd ones, each list in turn.
fn matheo_tip(lang: crate::i18n::Lang, k: usize) -> &'static str {
    let t = lang.strings();
    let list = if k.is_multiple_of(2) { t.matheo_water } else { t.matheo_moves };
    list.get(k / 2 % list.len().max(1)).copied().unwrap_or_default()
}

/// The watering can waits that long to be used, then goes.
const CAN_WAITS: Duration = Duration::from_secs(3 * 60);
/// Its watering, from the click (seconds): up over his leaves, pouring, back down, gone.
const POUR_UP: f32 = 0.5;
const POUR_FROM: f32 = 0.6;
const POUR_TO: f32 = 2.25;
const POUR_END: f32 = 2.9;

/// The side of Mathéo where the can stands (1 his right; his left near the screen's right edge).
fn can_side(foot: Pos2, works: &[Rect]) -> f32 {
    let right = works.iter().find(|w| (w.min.x..w.max.x).contains(&foot.x)).map_or(f32::INFINITY, |w| w.max.x);
    if foot.x + 90.0 > right { -1.0 } else { 1.0 }
}

/// Where the can stands beside him, to be clicked (screen points).
fn can_rect(foot: Pos2, side: f32) -> Rect {
    Rect::from_center_size(foot + Vec2::new(side * 48.0, -17.0), Vec2::new(50.0, 40.0))
}

/// The watering can as it shows: its bottom's middle, its turn, which way its spout points, how
/// opaque, and how long it has been pouring (seconds since the click).
#[derive(Clone, Copy)]
struct CanView {
    origin: Pos2,
    angle: f32,
    facing: f32,
    alpha: f32,
    pour: Option<f32>,
    /// Mathéo's foot, for the drops to land on his soil.
    foot: Pos2,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn can_view(foot: Pos2, side: f32, since: Instant, pour: Option<Instant>) -> CanView {
    let rest = foot + Vec2::new(side * 48.0, 0.0);
    let above = foot + Vec2::new(side * 50.0, -99.0);
    let facing = -side;
    match pour.map(|p| p.elapsed().as_secs_f32()) {
        None => {
            // It hops in, then wiggles every few seconds to be noticed.
            let t = since.elapsed().as_secs_f32();
            let hop = (smooth(t / 0.35) * std::f32::consts::PI).sin() * 14.0;
            let k = t % 3.0;
            let wiggle = if t > 0.4 && k < 0.6 { (k * 28.0).sin() * 0.12 * (1.0 - k / 0.6) } else { 0.0 };
            CanView { origin: rest - Vec2::new(0.0, hop), angle: wiggle, facing, alpha: smooth(t / 0.2), pour: None, foot }
        }
        Some(t) => {
            let e = smooth(t / POUR_UP) * (1.0 - smooth((t - POUR_TO) / (POUR_END - POUR_TO - 0.15)));
            let arc = (e * std::f32::consts::PI).sin() * 16.0;
            let tilt = 1.15 * smooth((t - 0.2) / 0.4) * (1.0 - smooth((t - 2.2) / 0.4)) + if (POUR_FROM..POUR_TO).contains(&t) { (t * 9.0).sin() * 0.04 } else { 0.0 };
            CanView { origin: rest.lerp(above, e) + Vec2::new(side * arc, 0.0), angle: tilt * facing, facing, alpha: 1.0 - smooth((t - (POUR_END - 0.3)) / 0.3), pour: Some(t), foot }
        }
    }
}

/// The watering can: green enamel, a little daisy on its side, its arched handle at the back, a long
/// spout ending in a brass rose; pouring, drops raining from the rose onto Mathéo's soil.
fn paint_can(painter: &egui::Painter, v: CanView, shift: Vec2) {
    let o = v.origin - shift;
    let (sin, cos) = v.angle.sin_cos();
    let turn = |x: f32, y: f32| {
        let p = Vec2::new(x * v.facing, y);
        Vec2::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos)
    };
    let at = |x: f32, y: f32| o + turn(x, y);
    let c = |r: u8, g: u8, b: u8| Color32::from_rgb(r, g, b).gamma_multiply(v.alpha);
    let (body, dark, light, rim) = (c(52, 146, 98), c(33, 101, 68), c(140, 214, 170), c(28, 82, 55));
    let (brass, brass_dark) = (c(218, 176, 78), c(150, 110, 42));
    let poly = |pts: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        painter.add(egui::Shape::convex_polygon(pts.iter().map(|&(x, y)| at(x, y)).collect(), fill, stroke));
    };
    let curve = |pts: Vec<Pos2>, stroke: Stroke| {
        painter.add(egui::Shape::line(pts, stroke));
    };
    // Its shadow on the floor, standing.
    if v.pour.is_none() {
        painter.add(egui::Shape::ellipse_filled(o, Vec2::new(16.0, 3.0), Color32::from_black_alpha((50.0 * v.alpha) as u8)));
    }
    // The handle, arched from the top's back down to the back of the body.
    let bezier = |p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)| -> Vec<Pos2> {
        (0..=14)
            .map(|i| {
                let t = i as f32 / 14.0;
                let m = 1.0 - t;
                at(m * m * p0.0 + 2.0 * m * t * p1.0 + t * t * p2.0, m * m * p0.1 + 2.0 * m * t * p1.1 + t * t * p2.1)
            })
            .collect()
    };
    curve(bezier((-4.0, -27.0), (-27.0, -44.0), (-12.0, -9.0)), Stroke::new(3.6, rim));
    curve(bezier((-4.0, -27.5), (-26.0, -43.0), (-12.5, -9.5)), Stroke::new(1.2, body));
    // The spout, from the body's front low down, up to the rose.
    let (from, to) = ((8.0f32, -9.0f32), (32.0f32, -28.5f32));
    poly(&[(8.0, -5.0), (8.0, -13.0), (31.0, -30.0), (33.0, -27.0)], body, Stroke::new(1.0, rim));
    curve(vec![at(9.0, -11.0), at(31.0, -29.2)], Stroke::new(0.8, light));
    // The rose: a brass neck, then its round face full of holes, across the spout.
    let len = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
    let d = ((to.0 - from.0) / len, (to.1 - from.1) / len);
    let n = (-d.1, d.0);
    let neck = (to.0 + d.0 * 2.0, to.1 + d.1 * 2.0);
    poly(&[(to.0 - n.0 * 1.6, to.1 - n.1 * 1.6), (to.0 + n.0 * 1.6, to.1 + n.1 * 1.6), (neck.0 + n.0 * 3.2, neck.1 + n.1 * 3.2), (neck.0 - n.0 * 3.2, neck.1 - n.1 * 3.2)], brass_dark, Stroke::NONE);
    let face = (neck.0 + d.0 * 1.2, neck.1 + d.1 * 1.2);
    let disc: Vec<Pos2> = (0..20).map(|i| {
        let s = i as f32 / 20.0 * std::f32::consts::TAU;
        at(face.0 + n.0 * 5.0 * s.cos() + d.0 * 1.7 * s.sin(), face.1 + n.1 * 5.0 * s.cos() + d.1 * 1.7 * s.sin())
    }).collect();
    painter.add(egui::Shape::convex_polygon(disc, brass, Stroke::new(0.8, brass_dark)));
    for k in [-2.6f32, -0.9, 0.9, 2.6] {
        painter.circle_filled(at(face.0 + n.0 * k + d.0 * 0.9, face.1 + n.1 * k + d.1 * 0.9), 0.55, brass_dark);
    }
    // The body, a little narrower at the top: shaded on its far side, a highlight on the near one.
    poly(&[(-13.0, 0.0), (13.0, 0.0), (11.0, -27.0), (-11.0, -27.0)], body, Stroke::NONE);
    poly(&[(7.5, 0.0), (13.0, 0.0), (11.0, -27.0), (6.5, -27.0)], dark, Stroke::NONE);
    poly(&[(-8.5, -2.5), (-6.3, -2.5), (-5.6, -24.5), (-7.6, -24.5)], light.gamma_multiply(0.7), Stroke::NONE);
    poly(&[(-13.4, 0.0), (13.4, 0.0), (13.1, -3.2), (-13.1, -3.2)], rim, Stroke::NONE);
    // A little daisy painted on its side.
    let daisy = (-1.0f32, -14.0f32);
    for k in 0..6 {
        let a = k as f32 * std::f32::consts::TAU / 6.0;
        painter.circle_filled(at(daisy.0 + a.cos() * 2.6, daisy.1 + a.sin() * 2.6), 1.7, c(250, 250, 244));
    }
    painter.circle_filled(at(daisy.0, daisy.1), 1.5, c(250, 200, 60));
    // Its open top: the rim, the dark inside.
    let ring = |rx: f32, ry: f32| -> Vec<Pos2> { (0..24).map(|i| { let s = i as f32 / 24.0 * std::f32::consts::TAU; at(rx * s.cos(), -27.0 + ry * s.sin()) }).collect() };
    painter.add(egui::Shape::convex_polygon(ring(11.6, 2.8), rim, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(ring(9.6, 1.9), c(18, 48, 33), Stroke::NONE));
    // Pouring: drops from the rose, falling onto his soil, a little splash where they land.
    let Some(t) = v.pour else {
        return;
    };
    let rose = at(face.0 + d.0 * 1.8, face.1 + d.1 * 1.8);
    let dir = turn(d.0, d.1);
    let side = turn(n.0, n.1);
    let soil = v.foot.y - shift.y - 31.0;
    let water = Color32::from_rgba_unmultiplied(110, 182, 255, (230.0 * v.alpha) as u8);
    let mut i = 0u32;
    loop {
        let born = POUR_FROM + i as f32 * 0.04;
        if born > t.min(POUR_TO) {
            break;
        }
        let age = t - born;
        let j = ((i.wrapping_mul(2654435761) >> 16) % 1000) as f32 / 500.0 - 1.0;
        let v0 = dir * (70.0 + j.abs() * 30.0) + side * j * 2.2 * 12.0;
        let pos = rose + side * j * 3.5 + v0 * age + Vec2::new(0.0, 550.0 * age * age);
        if pos.y < soil {
            let vel = (v0 + Vec2::new(0.0, 1100.0 * age)).normalized();
            painter.line_segment([pos, pos - vel * 4.0], Stroke::new(1.3, water));
            painter.circle_filled(pos, 1.7, water);
        } else {
            // Landed: a ring spreading out for a moment.
            let hit = ((pos.y - soil) / 1100.0f32.max(1.0)).sqrt();
            let k = (hit * 6.0).min(1.0);
            if k < 1.0 {
                painter.add(egui::Shape::ellipse_stroke(Pos2::new(pos.x, soil), Vec2::new(2.0 + k * 4.0, 0.8 + k * 1.2), Stroke::new(1.0, water.gamma_multiply(1.0 - k))));
            }
        }
        i += 1;
    }
}

/// Until when Mathéo keeps quiet, kept beside the settings (a restart doesn't wake him).
fn matheo_quiet_file() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|d| d.join("matheo-quiet.json"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Mathéo's speech bubble above his leaves (his pot's foot at `foot`), kept within `bounds`: where it is.
fn paint_matheo_tip(painter: &egui::Painter, foot: Pos2, text: &str, bounds: Rect) -> Rect {
    let ink = Color32::from_rgb(46, 64, 36);
    let fill = Color32::from_rgb(250, 248, 234);
    let edge = Stroke::new(1.5, Color32::from_rgb(104, 156, 84));
    let galley = painter.layout(text.to_owned(), FontId::proportional(14.0), ink, 230.0);
    let size = galley.size() + Vec2::new(26.0, 18.0);
    let bottom = foot.y - MATHEO_H * 1.2;
    let x = foot.x.clamp(bounds.min.x + size.x / 2.0 + 6.0, (bounds.max.x - size.x / 2.0 - 6.0).max(bounds.min.x + size.x / 2.0 + 6.0));
    let top = (bottom - size.y).max(bounds.min.y + 6.0);
    let r = Rect::from_min_size(Pos2::new(x - size.x / 2.0, top), size);
    // Its tail towards his leaves: outlined, then the bubble over it, then filled again to join them.
    let tail_x = foot.x.clamp(r.min.x + 16.0, r.max.x - 16.0);
    let point = Pos2::new(foot.x, r.max.y + 12.0);
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(tail_x - 8.0, r.max.y - 1.0), Pos2::new(tail_x + 8.0, r.max.y - 1.0), point], fill, edge));
    painter.rect(r, 10.0, fill, edge, egui::StrokeKind::Inside);
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(tail_x - 6.5, r.max.y - 3.0), Pos2::new(tail_x + 6.5, r.max.y - 3.0), Pos2::new(point.x + (tail_x - point.x) * 0.15, point.y - 2.5)], fill, Stroke::NONE));
    painter.galley(r.min + Vec2::new(13.0, 9.0), galley, ink);
    r
}

/// Where a window of ours really is, its content's top left on the screens (points); before it is
/// known, where it was asked to be.
fn window_origin(ctx: &egui::Context, asked: Pos2) -> Pos2 {
    ctx.input(|i| i.viewport().inner_rect).map_or(asked, |r| r.min)
}

/// Mathéo's height on screen (points).
const MATHEO_H: f32 = 70.0;

/// Where Mathéo was left, kept beside the settings.
fn matheo_file() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|d| d.join("matheo.json"))
}

fn matheo_load() -> Option<Pos2> {
    let text = std::fs::read_to_string(matheo_file()?).ok()?;
    let v: (f32, f32) = serde_json::from_str(&text).ok()?;
    Some(Pos2::new(v.0, v.1))
}

fn matheo_save(at: Pos2) {
    if let Some(path) = matheo_file() {
        let _ = std::fs::write(path, serde_json::to_string(&(at.x, at.y)).unwrap_or_default());
    }
}

/// Mathéo's place among the screens' foot (`grounds`): where he lands (the bottom of the screen under
/// him; on the Dock, `dock`, where it is), back on a screen if left between two.
fn matheo_floor(at: Pos2, grounds: &[Rect], dock: Option<Rect>) -> (f32, f32) {
    // The screens in his column: the one he's over, else the first floor below him, else the lowest;
    // none, the nearest screen. (Screens one above the other: the first one in his column took him
    // back to the screen above.)
    let column = || grounds.iter().filter(|w| (w.min.x..w.max.x).contains(&at.x));
    let Some(ground) = column()
        .find(|w| (w.min.y..=w.max.y).contains(&at.y))
        .or_else(|| column().filter(|w| w.max.y >= at.y).min_by(|a, b| a.max.y.total_cmp(&b.max.y)))
        .or_else(|| column().max_by(|a, b| a.max.y.total_cmp(&b.max.y)))
        .or_else(|| grounds.iter().min_by(|a, b| a.center().distance(at).total_cmp(&b.center().distance(at))))
    else {
        return (at.x, at.y);
    };
    let half = MATHEO_H * 0.4;
    let x = at.x.clamp(ground.min.x + half, ground.max.x - half);
    // Standing on the Dock: his pot's middle over it.
    let on_dock = dock.filter(|d| ground.contains(d.center()) && (d.min.x..d.max.x).contains(&x));
    (x, on_dock.map_or(ground.max.y, |d| d.min.y))
}

/// The screens' foot Mathéo stands on: the Dock known (its frame, from macOS' accessibility), the very
/// bottom of each screen (on the Dock where it is, beside it at the foot); else above the Dock or the
/// task bar, all along.
fn matheo_grounds(screens: &[(Rect, Rect)], works: &[Rect], dock: Option<Rect>) -> Vec<Rect> {
    match dock {
        Some(_) if !screens.is_empty() => screens.iter().map(|(s, w)| Rect::from_min_max(w.min, Pos2::new(w.max.x, s.max.y))).collect(),
        _ => works.to_vec(),
    }
}

/// Mathéo, Riff Hero's green plant, his pot's foot at `foot`, `size` tall: a terracotta pot with a
/// face, his big leaves swaying; held, his eyes wide and his leaves flapping.
pub(super) fn paint_matheo(painter: &egui::Painter, foot: Pos2, size: f32, now: f64, held: bool) {
    paint_matheo_as(painter, foot, size, now, Look { held, ..Look::default() });
}

/// How Mathéo looks: held, tipped over (radians, turning around his middle), his head spinning,
/// squashed (0 to 1) against a wall.
#[derive(Clone, Copy, Default)]
struct Look {
    held: bool,
    tilt: f32,
    dizzy: bool,
    squash: f32,
    /// Watered: his eyes closed with pleasure, drops shining on his leaves.
    glad: bool,
    /// Late in the evening: heavy eyelids, yawns, leaves hanging a little, z's floating up.
    tired: bool,
    /// Friday afternoon: sunglasses.
    shades: bool,
    /// Waving a leaf at Gérard passing by: towards which side (1 right, -1 left, 0 not).
    wave: f32,
    /// Hiding from the 10 o'clock horde (0 to 1): leaves pulled in, trembling, eyes wide.
    hide: f32,
    /// Hopping to be noticed (points off the floor).
    hop: f32,
    /// Grown, well watered: leaves more (up to 3), and a flower.
    extra: usize,
    flower: bool,
    /// Not watered (0 to 1): his leaves hang and yellow, he sulks.
    wilt: f32,
}

fn paint_matheo_as(painter: &egui::Painter, foot: Pos2, size: f32, now: f64, look: Look) {
    let held = look.held;
    let u = size / 70.0;
    let ground = foot;
    let tremble = if look.hide > 0.3 { (now * 47.0).sin() as f32 * 0.9 * look.hide } else { 0.0 };
    let foot = foot - Vec2::new(-tremble, look.hop);
    let pivot = foot - Vec2::new(0.0, 20.0 * u);
    let (sin, cos) = look.tilt.sin_cos();
    let at = |x: f32, y: f32| {
        let v = Vec2::new(x * (1.0 + look.squash * 0.6), (y + 20.0) * (1.0 - look.squash)) * u;
        pivot + Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
    };
    let quad = |(x0, y0): (f32, f32), (x1, y1): (f32, f32)| vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    let ink = Color32::from_rgb(60, 36, 24);
    // The shadow (upright only), the pot (wider at the top), its rim, the soil.
    if look.tilt.abs() < 0.3 {
        painter.add(egui::Shape::ellipse_filled(ground, Vec2::new(20.0, 4.0) * u * (1.0 - look.hop / 40.0), Color32::from_black_alpha(60)));
    }
    painter.add(egui::Shape::convex_polygon(vec![at(-14.0, 0.0), at(14.0, 0.0), at(18.0, -26.0), at(-18.0, -26.0)], Color32::from_rgb(196, 104, 62), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(quad((-21.0, -32.0), (21.0, -24.0)), Color32::from_rgb(214, 122, 76), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(quad((-19.0, -33.0), (19.0, -31.0)), Color32::from_rgb(92, 62, 40), Stroke::NONE));
    // His face on the pot: his eyes in crosses when dizzy, wide when held.
    let blink = (now % 4.0) < 0.12 && !held;
    for s in [-1.0, 1.0] {
        let (x, y) = (s * 6.0, -15.0);
        if look.dizzy {
            let line = Stroke::new(1.6 * u, ink);
            painter.line_segment([at(x - 2.3, y - 2.3), at(x + 2.3, y + 2.3)], line);
            painter.line_segment([at(x - 2.3, y + 2.3), at(x + 2.3, y - 2.3)], line);
        } else if look.glad {
            let arch: Vec<Pos2> = (0..=6).map(|k| { let a = k as f32 / 6.0 * std::f32::consts::PI; at(x - 2.4 * a.cos(), y - 1.6 * a.sin()) }).collect();
            painter.add(egui::Shape::line(arch, Stroke::new(1.5 * u, ink)));
        } else if held || look.hide > 0.5 {
            painter.circle_filled(at(x, y), 3.2 * u, Color32::WHITE);
            painter.circle_filled(at(x, y), 1.8 * u, Color32::from_rgb(40, 30, 24));
        } else if look.tired {
            // Heavy eyelids: a line, half an eye under it.
            painter.line_segment([at(x - 2.6, y), at(x + 2.6, y)], Stroke::new(1.5 * u, ink));
            let half: Vec<Pos2> = (0..=6).map(|k| { let a = k as f32 / 6.0 * std::f32::consts::PI; at(x + 2.0 * a.cos(), y + 1.8 * a.sin()) }).collect();
            painter.add(egui::Shape::convex_polygon(half, ink, Stroke::NONE));
        } else if blink {
            painter.line_segment([at(x - 2.0, y), at(x + 2.0, y)], Stroke::new(1.5 * u, ink));
        } else {
            painter.circle_filled(at(x, y), 2.0 * u, ink);
        }
    }
    if look.dizzy {
        let wave: Vec<Pos2> = (0..=8).map(|k| at(-5.0 + k as f32 * 10.0 / 8.0, -8.0 + (k as f32 * 1.6 + now as f32 * 8.0).sin() * 1.2)).collect();
        painter.add(egui::Shape::line(wave, Stroke::new(1.5 * u, ink)));
    } else if held {
        painter.circle_stroke(at(0.0, -8.0), 2.5 * u, Stroke::new(1.5 * u, ink));
    } else if look.glad {
        let grin: Vec<Pos2> = (0..=8).map(|k| { let a = k as f32 / 8.0 * std::f32::consts::PI; at(-4.5 * a.cos(), -9.5 + 3.2 * a.sin()) }).collect();
        painter.add(egui::Shape::convex_polygon(grin, Color32::from_rgb(120, 50, 40), Stroke::new(1.2 * u, ink)));
    } else if look.tired {
        // A big yawn every few seconds, else a flat mouth.
        let k = (now % 7.0) as f32;
        let open = if k < 1.6 { (k / 1.6 * std::f32::consts::PI).sin() } else { 0.0 };
        if open > 0.1 {
            let mouth: Vec<Pos2> = (0..16).map(|i| { let a = i as f32 / 16.0 * std::f32::consts::TAU; at(2.4 * open * a.cos(), -8.0 + 3.4 * open * a.sin()) }).collect();
            painter.add(egui::Shape::convex_polygon(mouth, Color32::from_rgb(110, 46, 36), Stroke::new(1.2 * u, ink)));
        } else {
            painter.line_segment([at(-3.0, -8.0), at(3.0, -8.0)], Stroke::new(1.5 * u, ink));
        }
    } else if look.wilt > 0.4 {
        let frown: Vec<Pos2> = (0..=6).map(|k| at(-4.0 + k as f32 * 8.0 / 6.0, -7.0 - (1.0 - ((k as f32 - 3.0) / 3.0).powi(2)) * 2.0)).collect();
        painter.add(egui::Shape::line(frown, Stroke::new(1.5 * u, ink)));
    } else {
        let smile: Vec<Pos2> = (0..=6).map(|k| at(-4.0 + k as f32 * 8.0 / 6.0, -9.0 + ((k as f32 - 3.0) / 3.0).powi(2) * -2.0 + 2.0)).collect();
        painter.add(egui::Shape::line(smile, Stroke::new(1.5 * u, ink)));
    }
    painter.circle_filled(at(-10.0, -11.0), 2.2 * u, Color32::from_rgba_unmultiplied(255, 140, 140, 90));
    painter.circle_filled(at(10.0, -11.0), 2.2 * u, Color32::from_rgba_unmultiplied(255, 140, 140, 90));
    // Friday afternoon: black sunglasses, a glint on each lens, the arms going round the pot.
    if look.shades {
        let frame = Color32::from_rgb(12, 12, 16);
        painter.line_segment([at(-10.4, -16.4), at(-17.6, -18.0)], Stroke::new(1.4 * u, frame));
        painter.line_segment([at(10.4, -16.4), at(17.6, -18.0)], Stroke::new(1.4 * u, frame));
        let bridge: Vec<Pos2> = (0..=6).map(|k| { let a = k as f32 / 6.0 * std::f32::consts::PI; at(-1.6 * a.cos(), -16.6 - 0.9 * a.sin()) }).collect();
        painter.add(egui::Shape::line(bridge, Stroke::new(1.3 * u, frame)));
        for x in [-6.0f32, 6.0] {
            let lens: Vec<Pos2> = (0..24)
                .map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    at(x + 4.9 * a.cos(), -15.0 + (3.9 * a.sin()).max(-2.6))
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(lens, Color32::from_rgb(28, 30, 38), Stroke::new(1.1 * u, frame)));
            painter.line_segment([at(x - 3.0, -15.6), at(x - 1.2, -17.0)], Stroke::new(1.0 * u, Color32::from_rgba_unmultiplied(255, 255, 255, 150)));
            painter.circle_filled(at(x + 2.6, -13.2), 0.6 * u, Color32::from_rgba_unmultiplied(255, 255, 255, 110));
        }
    }
    // His leaves: stems from the soil, big leaves swaying (flapping, held).
    // Hanging when thirsty (and yellowing), a little when tired.
    let speed = if held { 9.0 } else if look.tired { 0.6 } else { 1.3 };
    let droop = look.wilt.max(if look.tired { 0.3 } else { 0.0 });
    let fade = |c: Color32, to: (u8, u8, u8)| {
        let w = look.wilt.clamp(0.0, 1.0) * 0.8;
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * w) as u8;
        Color32::from_rgb(m(c.r(), to.0), m(c.g(), to.1), m(c.b(), to.2))
    };
    // Grown: younger leaves between the first ones, drawn behind them.
    let leaves: Vec<(f32, f32)> = [(-0.75f32, 26.0f32), (0.35, 33.0), (0.85, 25.0)].into_iter().take(look.extra.min(3)).chain([(-1.0, 30.0), (-0.45, 38.0), (0.1, 42.0), (0.6, 36.0), (1.1, 28.0)]).collect();
    let first = leaves.len() - 5;
    // Waving at Gérard: the outer leaf on his side.
    let waving = if look.wave > 0.0 { Some(leaves.len() - 1) } else if look.wave < 0.0 { Some(first) } else { None };
    for (k, &(angle, len)) in leaves.iter().enumerate() {
        let sway = (now * speed + k as f64 * 0.9).sin() as f32 * if held { 0.25 } else { 0.08 };
        let len = len * (1.0 - 0.15 * look.wilt) * (1.0 - 0.7 * look.hide);
        let bend = if angle.abs() < 0.3 { droop * 0.6 } else { 0.0 };
        let mut a = -std::f32::consts::FRAC_PI_2 + angle * 0.8 * (1.0 + droop * 0.9) * (1.0 - 0.6 * look.hide) + bend + sway + look.tilt;
        if waving == Some(k) {
            a = -std::f32::consts::FRAC_PI_2 + look.wave * (0.75 + (now * 12.0).sin() as f32 * 0.35) + look.tilt;
        }
        let base = at(angle * 6.0, -32.0);
        let tip = base + Vec2::angled(a) * len * u;
        let mid = base.lerp(tip, 0.55);
        painter.line_segment([base, mid], Stroke::new(2.0 * u, fade(Color32::from_rgb(60, 120, 50), (120, 100, 50))));
        let dir = Vec2::angled(a);
        let side = Vec2::new(-dir.y, dir.x);
        let leaf: Vec<Pos2> = (0..=12)
            .map(|i| {
                let f = i as f32 / 12.0;
                let w = (f * std::f32::consts::PI).sin() * 9.0 * u;
                mid + dir * f * (len * 0.6) * u + side * w
            })
            .chain((0..=12).rev().map(|i| {
                let f = i as f32 / 12.0;
                let w = (f * std::f32::consts::PI).sin() * 9.0 * u;
                mid + dir * f * (len * 0.6) * u - side * w
            }))
            .collect();
        let young = k < first;
        let green = if young { Color32::from_rgb(118, 196, 96) } else { [Color32::from_rgb(72, 160, 70), Color32::from_rgb(90, 180, 80)][k % 2] };
        painter.add(egui::Shape::convex_polygon(leaf, fade(green, (186, 160, 72)), Stroke::new(1.0, fade(Color32::from_rgb(40, 100, 40), (120, 96, 44)))));
        painter.line_segment([mid, mid + dir * len * 0.55 * u], Stroke::new(1.0, fade(Color32::from_rgb(140, 210, 120), (214, 196, 120))));
        if look.glad {
            // Drops shining on his leaves.
            let drop = mid + dir * len * (0.25 + k as f32 * 0.06) * u + side * 3.0 * u;
            painter.circle_filled(drop, 1.9 * u, Color32::from_rgba_unmultiplied(150, 205, 255, 220));
            painter.circle_filled(drop - Vec2::new(0.6, 0.6) * u, 0.7 * u, Color32::WHITE);
        }
    }
    // Grown for a month: a pink flower on a stem of its own, nodding (hanging when thirsty).
    if look.flower {
        let sway = (now * speed * 0.8).sin() as f32 * 0.06;
        let a = -std::f32::consts::FRAC_PI_2 + 0.12 + droop * 0.9 + sway + look.tilt;
        let base = at(1.0, -32.0);
        let top = base + Vec2::angled(a) * 50.0 * u * (1.0 - 0.6 * look.hide);
        let bend = base.lerp(top, 0.5) + Vec2::angled(a + 1.4) * 3.0 * u;
        let stem: Vec<Pos2> = (0..=10).map(|i| { let t = i as f32 / 10.0; let m = 1.0 - t; (base.to_vec2() * m * m + bend.to_vec2() * 2.0 * m * t + top.to_vec2() * t * t).to_pos2() }).collect();
        painter.add(egui::Shape::line(stem, Stroke::new(1.6 * u, fade(Color32::from_rgb(60, 130, 50), (120, 100, 50)))));
        let petal = fade(Color32::from_rgb(244, 128, 170), (196, 150, 130));
        for i in 0..6 {
            let p = top + Vec2::angled(i as f32 * std::f32::consts::TAU / 6.0 + now as f32 * 0.2) * 4.2 * u;
            painter.circle_filled(p, 3.4 * u, petal);
            painter.circle_stroke(p, 3.4 * u, Stroke::new(0.6 * u, Color32::from_rgb(200, 80, 120)));
        }
        painter.circle_filled(top, 2.8 * u, Color32::from_rgb(255, 206, 64));
        painter.circle_filled(top - Vec2::new(0.8, 0.8) * u, 0.9 * u, Color32::from_rgb(255, 240, 170));
    }
    // Tired: z's floating up from his leaves.
    if look.tired && !look.dizzy && !held {
        for k in 0..3 {
            let f = ((now as f32 / 2.4) + k as f32 / 3.0).fract();
            let p = at(14.0 + f * 16.0 + (f * 9.0).sin() * 2.0, -60.0 - f * 34.0);
            let alpha = (f * std::f32::consts::PI).sin();
            painter.text(p, egui::Align2::CENTER_CENTER, "z", FontId::proportional((8.0 + f * 8.0) * u), Color32::from_rgba_unmultiplied(90, 110, 170, (230.0 * alpha) as u8));
        }
    }
    // Stars turning over his leaves.
    if look.dizzy {
        for k in 0..3 {
            let turn = now as f32 * 5.0 + k as f32 * std::f32::consts::TAU / 3.0;
            let c = at(0.0, -82.0) + Vec2::new(turn.cos() * 17.0, turn.sin() * 5.0) * u;
            let star: Vec<Pos2> = (0..10).map(|i| c + Vec2::angled(i as f32 * std::f32::consts::PI / 5.0 - std::f32::consts::FRAC_PI_2) * if i % 2 == 0 { 4.5 } else { 2.0 } * u).collect();
            painter.add(egui::Shape::Path(egui::epaint::PathShape::closed_line(star.clone(), Stroke::new(1.0 * u, Color32::from_rgb(200, 150, 20)))));
            painter.circle_filled(c, 2.0 * u, Color32::from_rgb(255, 214, 60));
        }
    }
}

/// `ronnie desktop-fx`: the effects' window, until its standard input closes.
pub(crate) fn run() {
    // Without a compositor, the transparent window would hide the screen in black: nothing then.
    #[cfg(all(unix, not(target_os = "macos")))]
    if !os::composited() {
        crate::log::info("desktop effects: no compositor, nothing shown");
        return;
    }
    let fx: Arc<Mutex<Option<Fx>>> = Arc::new(Mutex::new(None));
    let mouse = Arc::new(Mutex::new(Mouse::default()));
    let follows = Arc::new(AtomicBool::new(false));
    let ctx_slot: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    // What to show: a line at each change; the end, Ronnie gone.
    {
        let (fx, follows, ctx_slot) = (fx.clone(), follows.clone(), ctx_slot.clone());
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Ok(new) = serde_json::from_str::<Fx>(&line) {
                    follows.store(new.follows(), Ordering::Relaxed);
                    if let Ok(mut fx) = fx.lock() {
                        *fx = Some(new);
                    }
                    if let Some(ctx) = ctx_slot.lock().ok().and_then(|c| c.clone()) {
                        ctx.request_repaint();
                    }
                }
            }
            std::process::exit(0);
        });
    }
    // The pointer, read about 60 times a second; a move draws again (only when it is followed).
    {
        let (mouse, follows, ctx_slot) = (mouse.clone(), follows.clone(), ctx_slot.clone());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(16));
                let now = os::mouse();
                let changed = mouse.lock().map(|mut m| std::mem::replace(&mut *m, now) != now).unwrap_or(false);
                if changed
                    && follows.load(Ordering::Relaxed)
                    && let Some(ctx) = ctx_slot.lock().ok().and_then(|c| c.clone())
                {
                    ctx.request_repaint();
                }
            }
        });
    }

    let start = os::mouse().at.and_then(os::screen_at).map(|(screen, _)| screen).unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
    let viewport = egui::ViewportBuilder::default()
        .with_title("Ronnie FX")
        .with_decorations(false)
        .with_transparent(true)
        .with_mouse_passthrough(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_taskbar(false)
        .with_active(false)
        .with_has_shadow(false)
        .with_resizable(false)
        .with_position(start.min)
        .with_inner_size(start.size());
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_mut))]
    let mut options = eframe::NativeOptions { viewport, persist_window: false, ..Default::default() };
    // Out of the Dock and of ⌘ Tab.
    #[cfg(target_os = "macos")]
    {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }));
    }
    // Windows: a transparent window needs DirectX 12 through DirectComposition.
    #[cfg(windows)]
    {
        let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
        setup.instance_descriptor.backend_options.dx12.presentation_system = eframe::wgpu::Dx12SwapchainKind::DxgiFromVisual;
        options.wgpu_options.wgpu_setup = setup.into();
    }
    let result = eframe::run_native(
        "ronnie-fx",
        options,
        Box::new(move |cc| {
            os::tune(cc);
            if let Ok(mut slot) = ctx_slot.lock() {
                *slot = Some(cc.egui_ctx.clone());
            }
            let work = os::screen_at(start.center()).map_or(start, |(_, work)| work);
            let theme = crate::theme::Preset::find(crate::theme::DEFAULT_THEME).theme();
            Ok(Box::new(Overlay { fx, mouse, shown: Fx::default(), theme, screen: start, work, trail: Default::default(), wand: Default::default(), pressed: false, last: None, plain: (true, None), full: (Vec::new(), None), matheo: Plant::default(), others: Default::default() }))
        }),
    );
    if let Err(e) = result {
        crate::log::error(&format!("desktop effects: {e}"));
    }
}

/// The horde crosses the screen this long, from 10:00:00 on working days.
const HORDE_RUN: f64 = 14.0;

/// Lundi matin's horde, at 10 o'clock: the zombies running across the screen, shouting for the break.
/// Whether it shows, and when to look again. (`RONNIE_HORDE=1`, in a build of one's own: every minute.)
fn horde_run(painter: &egui::Painter, screen: Rect, demo: Option<i64>, kinds: &[u8]) -> (bool, f32) {
    match horde_since(demo) {
        Ok(since) => {
            paint_horde(painter, screen, since / HORDE_RUN, 14, kinds);
            (true, 0.0)
        }
        Err(again) => (false, again),
    }
}

/// How far into its run the horde is (seconds); or, not running, when to look again.
fn horde_since(demo: Option<i64>) -> Result<f64, f32> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    // Just switched on: once, right now.
    let shown = demo.map(|at| (now.timestamp_millis() - at) as f64 / 1000.0).filter(|t| (0.0..HORDE_RUN).contains(t));
    let since = if let Some(t) = shown {
        t
    } else if !crate::config::OFFICIAL && std::env::var("RONNIE_HORDE").is_ok_and(|v| v == "1") {
        f64::from(now.second()) + f64::from(now.nanosecond()) / 1e9
    } else {
        let weekday = now.weekday().num_days_from_monday() < 5;
        let t = f64::from(now.num_seconds_from_midnight()) + f64::from(now.nanosecond()) / 1e9 - 10.0 * 3600.0;
        if weekday { t } else { -1.0 }
    };
    if !(0.0..HORDE_RUN).contains(&since) {
        // Woken up again a moment before it starts (at most every second).
        return Err(1.0);
    }
    Ok(since)
}

/// The horde at `k` of its run (0 to 1): `count` zombies from the left edge of `area` to the right,
/// each on its own line and at its own pace, "PAUSE !" over their heads.
/// The zombies, Lundi matin's met ones (`kinds`, places in its bestiary) seen from above, or, none met,
/// runners seen from the side.
pub(super) fn paint_horde(painter: &egui::Painter, area: Rect, k: f64, count: u32, kinds: &[u8]) {
    let size = (area.height() / 14.0).clamp(10.0, 26.0);
    for n in 0..count {
        let h = n.wrapping_mul(2_654_435_761);
        let lane = 0.12 + 0.76 * ((h % 1000) as f32 / 1000.0);
        let delay = ((h >> 10) % 100) as f64 / 100.0 * 0.3;
        let pace = 0.75 + ((h >> 17) % 100) as f64 / 100.0 * 0.25;
        let along = ((k - delay) / pace).clamp(0.0, 1.0) as f32;
        if along <= 0.0 || along >= 1.0 {
            continue;
        }
        let x = area.min.x - size * 2.0 + (area.width() + size * 4.0) * along;
        let y = area.min.y + area.height() * lane;
        let step = (k * 40.0 + f64::from(n)).sin() as f32;
        if kinds.is_empty() {
            paint_runner(painter, Pos2::new(x, y + step.abs() * size * 0.15), size, step);
        } else {
            let kind = super::zombie::bestiary_kind(kinds[(h >> 5) as usize % kinds.len()]);
            super::zombie::paint_zombie(painter, Pos2::new(x, y), size / 20.0, kind, 0.0, n, (k * 60.0) as f32 + n as f32, 1.0, super::zombie::Fx::default());
        }
        let bubble = Pos2::new(x, y - size * 1.9);
        let font = FontId::proportional(size * 0.75);
        let galley = painter.layout_no_wrap("PAUSE !".to_owned(), font, Color32::from_rgb(61, 42, 99));
        let r = Rect::from_center_size(bubble, galley.size() + Vec2::new(size * 0.6, size * 0.3));
        painter.rect_filled(r, size * 0.3, Color32::from_rgb(255, 245, 210));
        painter.galley(r.center() - galley.size() / 2.0, galley, Color32::from_rgb(61, 42, 99));
    }
}

/// A zombie seen from the side, running to the right: arms ahead, legs swinging.
fn paint_runner(painter: &egui::Painter, at: Pos2, size: f32, step: f32) {
    let skin = Color32::from_rgb(143, 209, 106);
    let shirt = Color32::from_rgb(90, 110, 150);
    let legs = Stroke::new(size * 0.22, Color32::from_rgb(50, 54, 70));
    let hip = at + Vec2::new(0.0, size * 0.55);
    painter.line_segment([hip, hip + Vec2::new(step * size * 0.4, size * 0.7)], legs);
    painter.line_segment([hip, hip + Vec2::new(-step * size * 0.4, size * 0.7)], legs);
    painter.rect_filled(Rect::from_center_size(at + Vec2::new(0.0, size * 0.15), Vec2::new(size * 0.7, size * 0.9)), size * 0.15, shirt);
    let arms = Stroke::new(size * 0.18, skin);
    painter.line_segment([at, at + Vec2::new(size * 0.9, -size * 0.1 + step * size * 0.08)], arms);
    painter.line_segment([at + Vec2::new(0.0, size * 0.1), at + Vec2::new(size * 0.85, size * 0.05 - step * size * 0.08)], arms);
    let head = at - Vec2::new(-size * 0.05, size * 0.55);
    painter.circle_filled(head, size * 0.42, skin);
    painter.circle_filled(head + Vec2::new(size * 0.18, -size * 0.06), size * 0.07, Color32::from_rgb(45, 74, 34));
    painter.line_segment([head + Vec2::new(size * 0.08, size * 0.18), head + Vec2::new(size * 0.32, size * 0.16)], Stroke::new(size * 0.06, Color32::from_rgb(45, 74, 34)));
}

/// How often Gérard strolls by, how long it takes him (he's in no hurry).
const GERARD_EVERY: f64 = 600.0;
const GERARD_STROLL: f64 = 34.0;

/// Gérard strolling along `strip` now and then, one way then the other, a coffee in his hand: halfway, he
/// stops to wave and say hello. Whether he shows, and when to draw again.
fn gerard_stroll(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    let (along, rightward, waving) = match gerard_at(now) {
        Ok(at) => at,
        Err(again) => return (false, again),
    };
    let x = strip.min.x - 30.0 + (strip.width() + 60.0) * along;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    paint_gerard(painter, Pos2::new(x, strip.max.y), 40.0, if rightward { 1.0 } else { -1.0 }, now, waving);
    (true, 0.0)
}

/// Where Gérard is at `now`: how far along (0 to 1), going right, waving; or, away, the seconds until
/// he comes back.
/// How often Gérard comes. In a build of one's own, `RONNIE_GERARD=1`: every 45 s, to see him.
fn gerard_every() -> f64 {
    if !crate::config::OFFICIAL && std::env::var("RONNIE_GERARD").is_ok_and(|v| v == "1") { 45.0 } else { GERARD_EVERY }
}

fn gerard_at(now: f64) -> Result<(f32, bool, bool), f32> {
    let every = gerard_every();
    // Not at the sheep's time, nor the droid's.
    let t = (now + 95.0) % every;
    if t > GERARD_STROLL {
        return Err((every - t) as f32);
    }
    let rightward = ((now + 95.0) / every).floor() as i64 % 2 == 0;
    // Walking 0-14 s, waving 14-20 s, walking 20-34 s.
    let (along, waving) = if t < 14.0 {
        (t / 14.0 * 0.5, false)
    } else if t < 20.0 {
        (0.5, true)
    } else {
        (0.5 + (t - 20.0) / 14.0 * 0.5, false)
    };
    Ok((along as f32, rightward, waving))
}

/// Gérard seen from the side, his feet at `foot`, `size` tall, facing `dir` (1 right, -1 left): green
/// but kind, his brown jacket, his moustache, a coffee for Agnès; waving, "Bonjour !" and a heart.
pub(super) fn paint_gerard(painter: &egui::Painter, foot: Pos2, size: f32, dir: f32, now: f64, waving: bool) {
    let u = size / 40.0;
    let at = |x: f32, y: f32| foot + Vec2::new(x * dir, y) * u;
    let skin = Color32::from_rgb(170, 214, 140);
    let jacket = Color32::from_rgb(120, 86, 60);
    let dark = Color32::from_rgb(60, 50, 44);
    let step = if waving { 0.0 } else { (now * 5.0).sin() as f32 };
    // Legs, a slow step.
    let hip = at(0.0, -14.0);
    painter.line_segment([hip, at(step * 4.0, 0.0)], Stroke::new(3.2 * u, dark));
    painter.line_segment([hip, at(-step * 4.0, 0.0)], Stroke::new(3.2 * u, dark));
    // The jacket, the tie.
    painter.rect_filled(Rect::from_center_size(at(0.0, -21.0), Vec2::new(12.0, 16.0) * u), 3.0 * u, jacket);
    painter.line_segment([at(1.5, -28.0), at(1.5, -20.0)], Stroke::new(1.6 * u, Color32::from_rgb(200, 70, 70)));
    // The arm ahead holds the coffee; the other waves, or swings.
    let cup = at(9.0, -20.0);
    painter.line_segment([at(2.0, -25.0), cup], Stroke::new(2.6 * u, jacket));
    painter.rect_filled(Rect::from_center_size(cup + Vec2::new(2.0 * dir, -1.0) * u, Vec2::new(5.0, 6.0) * u), 1.0, Color32::from_rgb(240, 236, 226));
    let lift = if waving { -14.0 + (now * 8.0).sin() as f32 * 3.0 } else { -16.0 + step * 2.0 };
    let hand = at(-6.0 + if waving { 4.0 } else { 0.0 }, if waving { -40.0 } else { lift });
    painter.line_segment([at(-2.0, -25.0), hand], Stroke::new(2.6 * u, jacket));
    painter.circle_filled(hand, 2.0 * u, skin);
    // The head: green, a little grey hair, the moustache, a smile.
    let head = at(0.0, -34.0);
    painter.circle_filled(head, 6.5 * u, skin);
    painter.circle_filled(head + Vec2::new(-2.0 * dir, -4.5) * u, 3.5 * u, Color32::from_rgb(190, 190, 190));
    painter.circle_filled(head + Vec2::new(-2.0 * dir, -4.5) * u + Vec2::new(0.0, 2.0) * u, 3.0 * u, skin);
    painter.circle_filled(head + Vec2::new(3.0 * dir, -1.5) * u, 0.9 * u, dark);
    painter.line_segment([head + Vec2::new(2.0 * dir, 2.0) * u, head + Vec2::new(6.0 * dir, 2.0) * u], Stroke::new(1.6 * u, Color32::from_rgb(90, 70, 50)));
    if waving {
        let font = FontId::proportional(13.0 * u);
        let ink = Color32::from_rgb(61, 42, 99);
        let galley = painter.layout_no_wrap("Bonjour !".to_owned(), font, ink);
        let r = Rect::from_center_size(head - Vec2::new(0.0, 22.0 * u), galley.size() + Vec2::new(12.0, 6.0) * u);
        painter.rect_filled(r, 6.0 * u, Color32::from_rgb(255, 245, 210));
        painter.galley(r.center() - galley.size() / 2.0, galley, ink);
    }
    // A heart rising now and then (he thinks of her).
    let rise = (now * 0.4 % 1.0) as f32;
    painter.text(head + Vec2::new(-8.0 * dir, -8.0 - rise * 18.0) * u, Align2::CENTER_CENTER, "♥", FontId::proportional(10.0 * u), Color32::from_rgb(255, 122, 182).gamma_multiply(1.0 - rise));
}

/// How often the droid rolls by, how long it takes.
const DROID_EVERY: f64 = 420.0;
const DROID_ROLL: f64 = 12.0;

/// The droid rolling along `strip`, every so often, one way then the other: whether it shows, and when to
/// draw again.
fn droid_walk(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    // Not at the sheep's time.
    let t = (now + 55.0) % DROID_EVERY;
    if t > DROID_ROLL {
        return (false, (DROID_EVERY - t) as f32);
    }
    let rightward = ((now + 55.0) / DROID_EVERY).floor() as i64 % 2 == 0;
    let along = (t / DROID_ROLL) as f32;
    let x = strip.min.x - 30.0 + (strip.width() + 60.0) * along;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    // Every few seconds, a little hop on his thrusters: up, a moment in the air, down.
    let hop = (t % 3.2) as f32;
    let lift = if hop < 0.9 { (hop / 0.9 * std::f32::consts::PI).sin() * 34.0 } else { 0.0 };
    let foot = Pos2::new(x, strip.max.y - 2.0 - lift);
    if lift > 0.5 {
        // The thrusters' flames under his feet, flickering, bigger on the way up.
        let push = if hop < 0.45 { 1.0 } else { 0.55 };
        for dx in [-9.0f32, 9.0] {
            let base = foot + Vec2::new(dx, 0.0);
            let flick = 0.8 + 0.4 * ((now * 40.0 + f64::from(dx)).sin() as f32 * 0.5 + 0.5);
            let len = 14.0 * push * flick;
            painter.add(egui::Shape::convex_polygon(vec![base - Vec2::new(4.0, 0.0), base + Vec2::new(4.0, 0.0), base + Vec2::new(0.0, len)], Color32::from_rgb(255, 150, 40), Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![base - Vec2::new(2.0, 0.0), base + Vec2::new(2.0, 0.0), base + Vec2::new(0.0, len * 0.55)], Color32::from_rgb(255, 240, 160), Stroke::NONE));
        }
        // His shadow on the ground, smaller the higher.
        let k = 1.0 - lift / 50.0;
        painter.add(egui::Shape::ellipse_filled(Pos2::new(x, strip.max.y - 1.0), Vec2::new(14.0 * k, 3.0 * k), Color32::from_black_alpha(60)));
    }
    hyperspace::droid(painter, foot, 34.0, now);
    (true, 0.0)
}

/// Which of `n` screens a walk goes along: the same all through it (`lap`), each walker its own.
fn walk_screen(lap: f64, salt: u64, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let h = (lap as i64 as u64).wrapping_add(salt).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((h >> 33) % n as u64) as usize
}

/// The sheep's, the droid's, Gérard's, the house-elf's, the unicorn's and the direwolf cub's walks under
/// way, and the screen each goes along (any of the `n`).
fn walks(fx: &Fx, now: f64, n: usize) -> [(bool, usize); 6] {
    let gerard = gerard_every();
    let at = |lap: f64, salt: u64| walk_screen(lap, salt, n);
    [
        (fx.sheep && (now + 20.0) % office::SHEEP_EVERY <= 16.0, at(((now + 20.0) / office::SHEEP_EVERY).floor(), 1)),
        (fx.droid && (now + 55.0) % DROID_EVERY <= DROID_ROLL, at(((now + 55.0) / DROID_EVERY).floor(), 2)),
        (fx.gerard && gerard_at(now).is_ok(), at(((now + 95.0) / gerard).floor(), 3)),
        (fx.elf && elf_at(now).is_ok(), at(((now + 75.0) / ELF_EVERY).floor(), 4)),
        (fx.unicorn && wallstreet::unicorn_at(now).is_ok(), at(wallstreet::unicorn_lap(now), 5)),
        (fx.wolf && wolf_at(now).is_ok(), at(((now + 40.0) / WOLF_EVERY).floor(), 6)),
    ]
}

/// Something to show on screen `here` now: a walker going along it, the horde (on every screen).
fn walkers_on(fx: &Fx, now: f64, here: usize, n: usize) -> bool {
    (fx.horde && horde_since(fx.horde_demo).is_ok()) || walks(fx, now, n).iter().any(|&(on, at)| on && at == here)
}

/// On screen `here` (of `n`): the walkers going along it (`strip`, its foot) and
/// the horde (`whole`). Something shown, and when to look again.
fn paint_walkers(painter: &egui::Painter, strip: Rect, whole: Rect, now: f64, here: usize, n: usize, fx: &Fx) -> (bool, f32) {
    let [sheep, droid, gerard, elf, unicorn, wolf] = walks(fx, now, n);
    let (mut busy, mut next) = (false, 1.0f32);
    if fx.sheep && sheep.1 == here {
        let (shown, again) = office::sheep(painter, strip, now);
        busy |= shown.is_some();
        next = next.min(again);
    }
    if fx.droid && droid.1 == here {
        let (shown, again) = droid_walk(painter, strip, now);
        busy |= shown;
        next = next.min(again);
    }
    if fx.gerard && gerard.1 == here {
        let (shown, again) = gerard_stroll(painter, strip, now);
        busy |= shown;
        next = next.min(again);
    }
    if fx.elf && elf.1 == here {
        let (shown, again) = elf_walk(painter, strip, now);
        busy |= shown;
        next = next.min(again);
    }
    if fx.unicorn && unicorn.1 == here {
        let (shown, again) = wallstreet::unicorn_run(painter, strip, now);
        busy |= shown;
        next = next.min(again);
    }
    if fx.wolf && wolf.1 == here {
        let (shown, again) = wolf_walk(painter, strip, now);
        busy |= shown;
        next = next.min(again);
    }
    if fx.horde {
        let (shown, again) = horde_run(painter, whole, fx.horde_demo, &fx.horde_kinds);
        busy |= shown;
        next = next.min(again);
    }
    (busy, next)
}

/// How often the house-elf walks by, how long it takes (a stop halfway, waving his sock).
const ELF_EVERY: f64 = 480.0;
const ELF_WALK: f64 = 15.0;

/// Where the house-elf is in his walk at `now`: how far along (0 to 1), rightward, waving; or, out of
/// sight, the seconds before he comes.
fn elf_at(now: f64) -> Result<(f64, bool, bool), f32> {
    // Mostly not at the sheep’s or the droid’s time.
    let t = (now + 75.0) % ELF_EVERY;
    if t > ELF_WALK {
        return Err((ELF_EVERY - t) as f32);
    }
    let rightward = ((now + 75.0) / ELF_EVERY).floor() as i64 % 2 == 0;
    // Walking 0-6 s, waving 6-9 s, walking 9-15 s.
    let (along, waving) = if t < 6.0 {
        (t / 6.0 * 0.5, false)
    } else if t < 9.0 {
        (0.5, true)
    } else {
        (0.5 + (t - 9.0) / 6.0 * 0.5, false)
    };
    Ok((along, rightward, waving))
}

/// The house-elf walking along `strip`, every so often, one way then the other, a stop halfway to wave the
/// sock he was given (« Dobby est libre ! »): whether he shows, and when to draw again.
fn elf_walk(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    let (along, rightward, waving) = match elf_at(now) {
        Ok(at) => at,
        Err(again) => return (false, again),
    };
    let x = strip.min.x - 30.0 + (strip.width() + 60.0) * along as f32;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    let foot = Pos2::new(x, strip.max.y - 2.0);
    wizard::paint_elf(painter, foot, 36.0, if rightward { 1.0 } else { -1.0 }, (!waving).then_some(now as f32), now);
    if waving {
        let galley = painter.layout_no_wrap(wizard::ELF_FREE.to_owned(), FontId::proportional(14.0), Color32::from_rgb(0x2e, 0x24, 0x10));
        let r = Rect::from_center_size(foot + Vec2::new(0.0, -64.0), galley.size() + Vec2::new(18.0, 10.0));
        painter.rect(r, 8.0, Color32::from_rgb(0xfb, 0xf1, 0xd6), Stroke::new(1.0, wizard::GOLD), egui::StrokeKind::Inside);
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(r.center().x - 5.0, r.max.y), Pos2::new(r.center().x + 5.0, r.max.y), Pos2::new(r.center().x, r.max.y + 7.0)], Color32::from_rgb(0xfb, 0xf1, 0xd6), Stroke::NONE));
        painter.galley(r.center() - galley.size() / 2.0, galley, Color32::from_rgb(0x2e, 0x24, 0x10));
    }
    (true, 0.0)
}

/// How often the direwolf cub trots by, how long it takes (sitting halfway to howl).
const WOLF_EVERY: f64 = 110.0;
const WOLF_TROT: f64 = 16.0;

/// Where the direwolf cub is in its walk at `now`: how far along (0 to 1), rightward, howling; or, out
/// of sight, the seconds before it comes.
fn wolf_at(now: f64) -> Result<(f64, bool, bool), f32> {
    // Mostly not at the others' time.
    let t = (now + 40.0) % WOLF_EVERY;
    if t > WOLF_TROT {
        return Err((WOLF_EVERY - t) as f32);
    }
    let rightward = ((now + 40.0) / WOLF_EVERY).floor() as i64 % 2 == 0;
    // Trotting 0-6 s, sitting and howling 6-10 s, trotting 10-16 s.
    let (along, howling) = if t < 6.0 {
        (t / 6.0 * 0.5, false)
    } else if t < 10.0 {
        (0.5, true)
    } else {
        (0.5 + (t - 10.0) / 6.0 * 0.5, false)
    };
    Ok((along, rightward, howling))
}

/// The white direwolf cub trotting along `strip`, every so often, one way then the other, sitting down
/// halfway to howl: whether it shows, and when to draw again.
fn wolf_walk(painter: &egui::Painter, strip: Rect, now: f64) -> (bool, f32) {
    let (along, rightward, howling) = match wolf_at(now) {
        Ok(at) => at,
        Err(again) => return (false, again),
    };
    let x = strip.min.x - 30.0 + (strip.width() + 60.0) * along as f32;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    let foot = Pos2::new(x, strip.max.y - 2.0);
    westeros::paint_wolf(painter, foot, 34.0, if rightward { 1.0 } else { -1.0 }, (!howling).then_some(now as f32), now);
    if howling {
        westeros::paint_howl(painter, foot);
    }
    (true, 0.0)
}

impl eframe::App for Overlay {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let dt = self.last.map_or(0.016, |l| (now - l) as f32).clamp(0.001, 0.05);
        self.last = Some(now);
        if let Some(fx) = self.fx.lock().ok().and_then(|f| f.clone())
            && fx != self.shown
        {
            if fx.theme != self.shown.theme {
                self.theme = crate::theme::Preset::find(&fx.theme).theme();
            }
            self.shown = fx;
        }
        let mouse = self.mouse.lock().map(|m| *m).unwrap_or_default();
        // Another app full screen (a film...): nothing over it, the pointer its own; on the other
        // screens either. Looked at each second.
        if self.full.1.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            let all = os::screens();
            let screens: Vec<Rect> = if all.is_empty() { vec![self.screen] } else { all.into_iter().map(|s| s.0).collect() };
            self.full = (screens.into_iter().filter(|s| os::full_screen(*s)).collect(), Some(Instant::now()));
            // Linux: the window manager told about our windows once it lists them (Windows: their
            // frame taken off again, should one have slipped through).
            #[cfg(any(windows, all(unix, not(target_os = "macos"))))]
            os::tune_all();
        }
        let covered = |full: &[Rect], screen: Rect| full.iter().any(|f| (f.min - screen.min).length() < 1.0 && (f.max - screen.max).length() < 1.0);
        if covered(&self.full.0, self.screen) {
            os::hide_pointer(false);
            self.trail = Default::default();
            ctx.request_repaint_after(Duration::from_secs(1));
            return;
        }
        // The pointer went to another screen: the window follows it.
        if let Some(at) = mouse.at
            && !self.screen.contains(at)
            && let Some((screen, work)) = os::screen_at(at)
        {
            self.screen = screen;
            self.work = work;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(screen.min));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(screen.size()));
            self.trail = Default::default();
        }
        // Where this window really is: macOS may have slid it under the menu bar (and cut it to fit), so
        // everything is drawn from its true top left, not the screen's.
        let origin = window_origin(&ctx, self.screen.min);
        let local = mouse.at.filter(|p| self.screen.contains(*p)).map(|p| (p - origin).to_pos2());
        let click = mouse.pressed && !self.pressed;
        self.pressed = mouse.pressed;
        let screens = os::screens();
        let works: Vec<Rect> = if screens.is_empty() { vec![self.work] } else { screens.iter().map(|s| s.1).collect() };
        let dock = os::dock();
        let grounds = matheo_grounds(&screens, &works, dock);
        // Mathéo: grabbed with the mouse (the window takes the click over him only), dragged, dropped:
        // he falls back to the foot of the screen under him.
        let mut plant_busy = false;
        if self.shown.matheo {
            let plant = &mut self.matheo;
            let at = *plant.at.get_or_insert_with(|| Pos2::new(self.work.max.x - 160.0, self.work.max.y));
            let body = Rect::from_center_size(at - Vec2::new(0.0, MATHEO_H * 0.5), Vec2::new(MATHEO_H * 0.7, MATHEO_H));
            let on_bubble = plant.tip.is_some() && mouse.at.zip(plant.bubble).is_some_and(|(p, r)| r.contains(p));
            let side = can_side(at, &works);
            let on_can = plant.can.is_some_and(|(_, pour)| pour.is_none()) && plant.grab.is_none() && mouse.at.is_some_and(|p| can_rect(at, side).contains(p));
            let over = mouse.at.is_some_and(|p| body.contains(p)) || on_bubble || on_can;
            if plant.quiet.is_some_and(|until| unix_now() >= until) {
                plant.quiet = None;
            }
            if click && on_can {
                // Watered: the can lifts over his leaves and pours.
                if let Some(can) = &mut plant.can {
                    can.1 = Some(Instant::now());
                }
                if matches!(plant.tip, Some((Said::Tip(_), _))) {
                    plant.tip = None;
                }
            } else if click && on_bubble {
                plant.tip = None;
            } else if click && over {
                // Caught, even in the air.
                plant.grab = mouse.at.map(|p| at - p);
                plant.press = mouse.at;
                plant.track.clear();
                plant.flips.clear();
                (plant.vx, plant.vy, plant.spin) = (0.0, 0.0, 0.0);
            }
            match (plant.grab, mouse.at) {
                (Some(grab), Some(p)) if mouse.pressed => {
                    let to = p + grab;
                    // Swung back the other way, fast: a shake.
                    if let Some(&(then, from)) = plant.track.back()
                        && now > then
                    {
                        let speed = (to.x - from.x) / (now - then) as f32;
                        if speed.abs() > 700.0 {
                            if plant.way != 0.0 && speed.signum() != plant.way {
                                plant.flips.push(now);
                            }
                            plant.way = speed.signum();
                        }
                    }
                    plant.track.push_back((now, to));
                    while plant.track.len() > 8 {
                        plant.track.pop_front();
                    }
                    plant.flips.retain(|t| now - t < SHAKE_WITHIN);
                    // He leans with the swing.
                    let swing = plant.track.front().filter(|(then, _)| now > *then).map_or(0.0, |(then, from)| (to.x - from.x) / (now - then) as f32);
                    plant.tilt += ((swing / 2500.0).clamp(-0.7, 0.7) - plant.tilt) * (1.0 - (-14.0 * dt).exp());
                    if plant.flips.len() >= SHAKE_FLIPS {
                        plant.flips.clear();
                        plant.press = None;
                        let until = unix_now() + QUIET_SECS;
                        plant.quiet = Some(until);
                        if let Some(f) = matheo_quiet_file() {
                            let _ = std::fs::write(f, until.to_string());
                        }
                        plant.dizzy = Some(Instant::now() + Duration::from_secs(5));
                        plant.tip = Some((Said::Quiet, Instant::now()));
                    } else if plant.flips.len() >= 2 && !matches!(plant.tip, Some((Said::Quiet, _))) {
                        plant.tip = Some((Said::Shaking, Instant::now()));
                    }
                    plant.at = Some(to);
                    plant_busy = true;
                }
                (Some(_), _) => {
                    plant.grab = None;
                    if plant.press.take().zip(mouse.at).is_some_and(|(a, b)| a.distance(b) < 4.0) {
                        // Let go where he was taken: a click, his advice at once (or none, if one
                        // shows); he speaks again if he was quiet.
                        if self.shown.matheo_tips {
                            if matches!(plant.tip, Some((Said::Tip(_), _))) {
                                plant.tip = None;
                            } else {
                                plant.quiet = None;
                                if let Some(f) = matheo_quiet_file() {
                                    let _ = std::fs::remove_file(f);
                                }
                                plant.advise();
                            }
                        }
                    } else if let (Some(&(t0, p0)), Some(&(t1, p1))) = (plant.track.front(), plant.track.back())
                        && t1 > t0
                    {
                        // Let go moving: thrown, turning on himself.
                        let v = (p1 - p0) / (t1 - t0) as f32;
                        let v = if v.length() > 4500.0 { v.normalized() * 4500.0 } else { v };
                        (plant.vx, plant.vy, plant.spin) = (v.x, v.y, v.x * 0.004);
                    }
                    plant.track.clear();
                    plant_busy = true;
                }
                _ => {
                    // Falling, flying, bouncing off the screens' edges, standing up again on the floor.
                    let half = MATHEO_H * 0.4;
                    let left = works.iter().map(|w| w.min.x).fold(f32::INFINITY, f32::min) + half;
                    let right = works.iter().map(|w| w.max.x).fold(f32::NEG_INFINITY, f32::max) - half;
                    let top = works.iter().map(|w| w.min.y).fold(f32::INFINITY, f32::min) + MATHEO_H;
                    let (rest_x, floor) = matheo_floor(at, &grounds, dock);
                    let upright = plant.tilt.abs() < 0.005;
                    if at.y < floor - 0.5 || at.y > floor + 0.5 || plant.vy != 0.0 || plant.vx != 0.0 || !upright || rest_x != at.x {
                        let mut p = at;
                        let mut bonk = false;
                        plant.vy += 2600.0 * dt;
                        p += Vec2::new(plant.vx, plant.vy) * dt;
                        if p.x < left || p.x > right {
                            p.x = p.x.clamp(left, right.max(left));
                            bonk = plant.vx.abs() > 300.0;
                            plant.vx = -plant.vx * 0.55;
                            plant.spin = -plant.spin * 0.7;
                        }
                        if p.y < top && plant.vy < 0.0 {
                            p.y = top;
                            bonk = plant.vy < -300.0;
                            plant.vy = -plant.vy * 0.4;
                        }
                        let (_, floor) = matheo_floor(p, &grounds, dock);
                        let grounded = p.y >= floor;
                        if grounded {
                            p.y = floor;
                            if plant.vy > 1400.0 {
                                plant.bonk = Some(Instant::now());
                            }
                            plant.vy = if plant.vy > 500.0 { -plant.vy * 0.35 } else { 0.0 };
                            plant.vx *= (-6.0 * dt).exp();
                            if plant.vx.abs() < 20.0 {
                                plant.vx = 0.0;
                            }
                            plant.spin *= (-10.0 * dt).exp();
                            // Back on his foot, the nearest way up.
                            let upright = (plant.tilt / std::f32::consts::TAU).round() * std::f32::consts::TAU;
                            plant.tilt += (upright - plant.tilt) * (1.0 - (-10.0 * dt).exp());
                            if (upright - plant.tilt).abs() < 0.005 && plant.vx == 0.0 {
                                plant.tilt = 0.0;
                            }
                        } else {
                            plant.tilt += plant.spin * dt;
                        }
                        if bonk {
                            plant.bonk = Some(Instant::now());
                            plant.dizzy = Some(plant.dizzy.map_or(Instant::now(), |d| d.max(Instant::now())) + Duration::from_millis(1500));
                            if !matches!(plant.tip, Some((Said::Quiet, _))) {
                                plant.tip = Some((Said::Ouch, Instant::now()));
                            }
                        }
                        if grounded && plant.vx == 0.0 && plant.vy == 0.0 {
                            p.x = matheo_floor(p, &grounds, dock).0;
                            if plant.tilt == 0.0 {
                                matheo_save(p);
                            }
                        }
                        plant.at = Some(p);
                        plant_busy = true;
                    }
                }
            }
            // His advice, every half hour while someone is at the screen (back from a break, a while
            // first); shown a little while.
            let moment = Instant::now();
            if mouse.at != plant.moved.0 {
                if plant.moved.1.elapsed() >= TIP_AWAY {
                    plant.next_tip = plant.next_tip.max(moment + TIP_EVERY / 2);
                }
                if plant.moved.1.elapsed() >= BREAK {
                    plant.active_since = moment;
                }
                plant.moved = (mouse.at, moment);
            }
            if plant.tip.is_some_and(|(said, since)| since.elapsed() >= said.lasts() || (!self.shown.matheo_tips && matches!(said, Said::Tip(_)))) {
                plant.tip = None;
            } else if self.shown.matheo_tips && plant.quiet.is_none() && plant.tip.is_none() && moment >= plant.next_tip && plant.moved.1.elapsed() < TIP_IDLE && plant.grab.is_none() {
                plant.advise();
            }
            match plant.can {
                Some((since, None)) if since.elapsed() >= CAN_WAITS => {
                    plant.can = None;
                    plant.set_thirst(plant.thirst + 1);
                }
                Some((_, Some(pour))) if pour.elapsed().as_secs_f32() >= POUR_END => {
                    plant.can = None;
                    plant.set_thirst(0);
                    plant.watered();
                    plant.glad = Some(Instant::now() + Duration::from_secs(4));
                    plant.tip = Some((Said::Thanks, Instant::now()));
                }
                Some(_) => plant_busy = true,
                None => {}
            }
            // Gérard passing near him on his screen: a leaf waved, « Salut Gérard ! » once a walk.
            let n = screens.len().max(1);
            let mine = screens.iter().position(|(s, _)| s.contains(at - Vec2::new(0.0, 1.0))).unwrap_or(0);
            let (walking, on) = walks(&self.shown, now, n)[2];
            plant.wave = 0.0;
            if walking && on == mine
                && let (Ok((along, rightward, _)), Some(work)) = (gerard_at(now), works.get(mine))
            {
                let (from, to) = (work.min.x + 6.0, work.max.x - 6.0);
                let x = from - 30.0 + (to - from + 60.0) * along;
                let x = if rightward { x } else { from + to - x };
                if (x - at.x).abs() < 150.0 && plant.grab.is_none() {
                    plant.wave = (x - at.x).signum();
                    if !plant.greeted && plant.tip.is_none_or(|(said, _)| matches!(said, Said::Ouch | Said::Gone)) {
                        plant.tip = Some((Said::Hello, Instant::now()));
                    }
                    plant.greeted = true;
                }
                plant_busy = true;
            } else {
                plant.greeted = false;
            }
            // The 10 o'clock horde: he hides in his pot, trembling, and peeks out once it's gone.
            let horde = self.shown.horde && horde_since(self.shown.horde_demo).is_ok();
            plant.hide = (plant.hide + (if horde { 4.0 } else { -2.0 }) * dt).clamp(0.0, 1.0);
            if horde {
                if !matches!(plant.tip, Some((Said::Quiet, _))) {
                    plant.tip = Some((Said::Hiding, Instant::now()));
                }
                plant.hiding = true;
            } else if plant.hiding {
                plant.hiding = false;
                plant.tip = Some((Said::Gone, Instant::now()));
            }
            if plant.hide > 0.0 {
                plant_busy = true;
            }
            let pass = !(over || plant.grab.is_some());
            if pass != plant.pass {
                plant.pass = pass;
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(pass));
            }
        } else if !self.matheo.pass {
            self.matheo.pass = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        }
        if self.matheo.tired.1.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
            self.matheo.tired = (late(), Some(Instant::now()));
            self.matheo.friday = friday();
        }
        let m = &self.matheo;
        let look = m.look(m.tired.0, m.friday);
        let can = m.at.zip(m.can).map(|(at, (since, pour))| can_view(at, can_side(at, &works), since, pour));
        let plant = m.at.filter(|_| self.shown.matheo).map(|at| (at, look));
        let tip = m.tip.filter(|_| self.shown.matheo).map(|(said, _)| said.text(self.shown.lang));

        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("desktop-fx")));
        let mut busy = false;
        let mut next: f32 = 1.0;
        // Along the foot of the screen, above the Dock or the task bar.
        let foot = self.work.translate(-origin.to_vec2());
        let strip = Rect::from_min_max(Pos2::new(foot.min.x + 6.0, foot.max.y - 34.0), Pos2::new(foot.max.x - 6.0, foot.max.y));
        // The screens: each walker along one of them (the same all through its walk), the horde along
        // all of them at once. This window covers the pointer's; the others get one each while needed.
        let n = screens.len().max(1);
        let here = screens.iter().position(|(s, _)| (s.min - self.screen.min).length() < 1.0).unwrap_or(0);
        let (shown, again) = paint_walkers(&painter, strip, self.screen.translate(-origin.to_vec2()), now, here, n, &self.shown);
        busy |= shown || plant_busy;
        next = next.min(again);
        // Mathéo on the screen he's on (this window's, or another's).
        let plant_on = plant.map(|(at, _)| screens.iter().position(|(s, _)| s.contains(at - Vec2::new(0.0, 1.0))).unwrap_or(here));
        if let (Some((at, held)), true) = (plant, plant_on == Some(here)) {
            paint_matheo_as(&painter, at - origin.to_vec2(), MATHEO_H, now, held);
            if let Some(can) = can {
                paint_can(&painter, can, origin.to_vec2());
            }
            if let Some(text) = tip {
                let bounds = self.screen.translate(-origin.to_vec2());
                self.matheo.bubble = Some(paint_matheo_tip(&painter, at - origin.to_vec2(), text, bounds).translate(origin.to_vec2()));
            }
            next = next.min(0.1);
        }
        // A window over another screen is made hidden, shown once drawn (else, on Windows, a white
        // square first), hidden again when nothing goes along it, closed a while after.
        for (i, (screen, work)) in screens.iter().enumerate() {
            let walking = walkers_on(&self.shown, now, i, n);
            let needed = walking || plant_on == Some(i);
            if i == here || covered(&self.full.0, *screen) {
                self.others.remove(&i);
                continue;
            }
            if needed {
                self.others.entry(i).or_insert(Other { frames: 0, needed: Instant::now() }).needed = Instant::now();
            }
            let Some(other) = self.others.get_mut(&i).filter(|o| o.needed.elapsed() < OTHER_KEPT) else {
                self.others.remove(&i);
                continue;
            };
            let visible = needed && other.frames >= 2;
            other.frames = other.frames.saturating_add(1);
            if needed && !visible {
                // Drawn again at once, to be shown.
                next = next.min(0.02);
            }
            let (screen, work, fx) = (*screen, *work, self.shown.clone());
            let builder = egui::ViewportBuilder::default()
                .with_visible(visible)
                .with_title("Ronnie FX")
                .with_decorations(false)
                .with_transparent(true)
                .with_mouse_passthrough(true)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_taskbar(false)
                .with_active(false)
                .with_has_shadow(false)
                .with_resizable(false)
                .with_position(screen.min)
                .with_inner_size(screen.size());
            let bubble = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("fx-screen", i)), builder, |ctx, _| {
                // Windows places a new window with the scale of the screen it was made on: put back
                // over its screen, with that screen's own scale (as the main window is when it moves).
                if cfg!(windows)
                    && let Some(r) = ctx.input(|i| i.viewport().outer_rect)
                    && ((r.min - screen.min).length() > 1.0 || (r.size() - screen.size()).length() > 1.0)
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(screen.min));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(screen.size()));
                }
                if !needed {
                    return None;
                }
                // Drawn from where the window really is (macOS slides a new one under the menu bar).
                let origin = window_origin(ctx, screen.min);
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("desktop-fx")));
                let foot = work.translate(-origin.to_vec2());
                let strip = Rect::from_min_max(Pos2::new(foot.min.x + 6.0, foot.max.y - 34.0), Pos2::new(foot.max.x - 6.0, foot.max.y));
                paint_walkers(&painter, strip, screen.translate(-origin.to_vec2()), now, i, n, &fx);
                let (Some((at, held)), true) = (plant, plant_on == Some(i)) else {
                    return None;
                };
                paint_matheo_as(&painter, at - origin.to_vec2(), MATHEO_H, now, held);
                if let Some(can) = can {
                    paint_can(&painter, can, origin.to_vec2());
                }
                tip.map(|text| paint_matheo_tip(&painter, at - origin.to_vec2(), text, screen.translate(-origin.to_vec2())).translate(origin.to_vec2()))
            });
            if bubble.is_some() {
                self.matheo.bubble = bubble;
            }
            os::tune_all();
            if walking {
                busy = true;
            } else if needed {
                // Mathéo alone there: his leaves sway gently.
                next = next.min(0.1);
            }
        }
        if cfg!(debug_assertions) && std::env::var_os("RONNIE_MATHEO_GALLERY").is_some() {
            paint_matheo_gallery(&painter, self.screen.translate(-origin.to_vec2()), now);
            busy = true;
        }
        if self.shown.trail {
            self.trail.bills = self.shown.bills;
            self.trail.step(local, dt);
            self.trail.paint(&painter, &self.theme);
            busy |= self.trail.alive();
        }
        // The wand or the lightsaber in place of the system's pointer, only for the plain arrow (a
        // window's edge, a link, a text keep theirs). Looked at ten times a second.
        if self.plain.1.is_none_or(|t| t.elapsed() >= Duration::from_millis(100)) {
            self.plain = (os::plain_pointer(), Some(Instant::now()));
        }
        let local = local.filter(|_| self.plain.0 || self.shown.pointer.is_none());
        os::hide_pointer(self.shown.pointer.is_some() && self.plain.0);
        if self.shown.pointer.is_some() {
            // Woken up in time to hide it again.
            next = next.min(0.5);
        }
        match self.shown.pointer {
            Some(Pointer::Wand) => {
                self.wand.step(local, click, now, dt);
                busy |= self.wand.paint(&painter, local, now);
            }
            Some(Pointer::Saber) => {
                if let Some(at) = local {
                    hyperspace::pointer_saber(&painter, at, crate::theme::Preset::find(crate::theme::HYPERSPACE_THEME).theme().accent);
                }
            }
            Some(Pointer::Sword) => {
                if let Some(at) = local {
                    thrones::pointer_sword(&painter, at);
                }
            }
            Some(Pointer::Metal) => {
                if let Some(at) = local {
                    super::metal_arrow(&painter, at, &crate::theme::Preset::find(crate::theme::METAL_THEME).theme());
                }
            }
            Some(Pointer::Stapler) => {
                if let Some(at) = local {
                    monday_theme::pointer_stapler(&painter, at);
                }
            }
            Some(Pointer::System) | None => {}
        }
        if busy {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs_f32(next.clamp(0.02, 1.0)));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}

#[cfg(target_os = "macos")]
mod os {
    use std::ffi::c_void;

    use egui::{Pos2, Rect, Vec2};

    use super::Mouse;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
        static kCFBooleanTrue: *const c_void;
    }

    // The window server's own calls (private, used by the pointer utilities): with this property, a
    // process hides the pointer even while another one is in front.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn _CGSDefaultConnection() -> i32;
        fn CGSSetConnectionProperty(cid: i32, target: i32, key: *const c_void, value: *const c_void) -> i32;
        fn CGDisplayHideCursor(display: u32) -> i32;
        fn CGDisplayShowCursor(display: u32) -> i32;
    }

    thread_local! {
        /// How many times the pointer was hidden, and when last.
        static HIDDEN: std::cell::Cell<(u32, Option<std::time::Instant>)> = const { std::cell::Cell::new((0, None)) };
    }

    /// The system's pointer hidden (`hide`) or shown again. Hidden once, it stays in sight on recent
    /// macOS: it is hidden again every half second while it should be, and shown again as many times
    /// as it was hidden (the window server counts them).
    pub(super) fn hide_pointer(hide: bool) {
        static ALLOWED: std::sync::Once = std::sync::Once::new();
        let (count, last) = HIDDEN.get();
        // SAFETY: the window server's calls on our own connection; the key is a CFString kept alive.
        unsafe {
            if hide {
                ALLOWED.call_once(|| {
                    let key = objc2_foundation::NSString::from_str("SetsCursorInBackground");
                    let cid = _CGSDefaultConnection();
                    CGSSetConnectionProperty(cid, cid, objc2::rc::Retained::as_ptr(&key).cast(), kCFBooleanTrue);
                });
                if last.is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(500)) {
                    CGDisplayHideCursor(0);
                    HIDDEN.set((count + 1, Some(std::time::Instant::now())));
                }
            } else if count > 0 {
                for _ in 0..count {
                    CGDisplayShowCursor(0);
                }
                HIDDEN.set((0, None));
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
        fn CGRectMakeWithDictionaryRepresentation(dict: *const c_void, rect: *mut CGRect) -> bool;
        static kCGWindowLayer: *const c_void;
        static kCGWindowBounds: *const c_void;
        static kCGWindowOwnerPID: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFNumberGetValue(number: *const c_void, kind: isize, out: *mut c_void) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetDisplaysWithPoint(point: CGPoint, max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> *const c_void;
        fn CGSCopyManagedDisplaySpaces(cid: i32) -> *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFUUIDCreateString(alloc: *const c_void, uuid: *const c_void) -> *const c_void;
        fn CFEqual(a: *const c_void, b: *const c_void) -> bool;
    }

    /// `screen` (points, top left origin) shows a film, a game, a window in full screen: its desktop
    /// is a full-screen one (an app's own, the menu bar hidden; on a screen with a notch the window
    /// stops under it, so its size alone wouldn't tell), or the app in front covers it whole. Not when
    /// that app is Ronnie.
    pub(super) fn full_screen(screen: Rect) -> bool {
        let Some(front) = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication() else { return false };
        let pid = front.processIdentifier();
        if pid as u32 == std::os::unix::process::parent_id() || pid as u32 == std::process::id() {
            return false;
        }
        covered(screen, pid, full_space(screen))
    }

    /// The desktop shown on `screen` is an app's full-screen one (the window server's own list, read
    /// as the window utilities do: type 4).
    fn full_space(screen: Rect) -> bool {
        let key = |s: &str| objc2_foundation::NSString::from_str(s);
        let (display_key, current_key, type_key, main) = (key("Display Identifier"), key("Current Space"), key("type"), key("Main"));
        let ptr = |s: &objc2::rc::Retained<objc2_foundation::NSString>| objc2::rc::Retained::as_ptr(s).cast::<c_void>();
        // SAFETY: CoreGraphics' and the window server's lists, read then released; every value checked.
        unsafe {
            let mut display = 0u32;
            let mut count = 0u32;
            let center = CGPoint { x: f64::from(screen.center().x), y: f64::from(screen.center().y) };
            if CGGetDisplaysWithPoint(center, 1, &mut display, &mut count) != 0 || count == 0 {
                return false;
            }
            let uuid = CGDisplayCreateUUIDFromDisplayID(display);
            if uuid.is_null() {
                return false;
            }
            let name = CFUUIDCreateString(std::ptr::null(), uuid);
            CFRelease(uuid);
            if name.is_null() {
                return false;
            }
            let list = CGSCopyManagedDisplaySpaces(_CGSDefaultConnection());
            let mut full = false;
            if !list.is_null() {
                for k in 0..CFArrayGetCount(list) {
                    let d = CFArrayGetValueAtIndex(list, k);
                    let id = CFDictionaryGetValue(d, ptr(&display_key));
                    // « Main »: the screens share their desktops.
                    if id.is_null() || !(CFEqual(id, name) || CFEqual(id, ptr(&main))) {
                        continue;
                    }
                    let current = CFDictionaryGetValue(d, ptr(&current_key));
                    if !current.is_null() {
                        let kind = CFDictionaryGetValue(current, ptr(&type_key));
                        let mut v: i64 = 0;
                        full = !kind.is_null() && CFNumberGetValue(kind, 4, (&raw mut v).cast()) && v == 4;
                    }
                    break;
                }
                CFRelease(list);
            }
            CFRelease(name);
            full
        }
    }

    /// The app `pid` has a window covering the whole of `screen`; on a full-screen desktop
    /// (`in_space`), its top may stop under the notch.
    fn covered(screen: Rect, pid: i32, in_space: bool) -> bool {
        // SAFETY: the window server's list, read then released; every value checked before use.
        unsafe {
            // On screen only (1), without the desktop's (16).
            let list = CGWindowListCopyWindowInfo(1 | 16, 0);
            if list.is_null() {
                return false;
            }
            let number = |d: *const c_void, key: *const c_void| {
                let n = CFDictionaryGetValue(d, key);
                let mut v: i64 = 0;
                // kCFNumberSInt64Type.
                (!n.is_null() && CFNumberGetValue(n, 4, (&raw mut v).cast())).then_some(v)
            };
            let mut full = false;
            for k in 0..CFArrayGetCount(list) {
                let d = CFArrayGetValueAtIndex(list, k);
                if number(d, kCGWindowOwnerPID) != Some(i64::from(pid)) || number(d, kCGWindowLayer) != Some(0) {
                    continue;
                }
                let mut b = CGRect::default();
                let bounds = CFDictionaryGetValue(d, kCGWindowBounds);
                if !bounds.is_null() && CGRectMakeWithDictionaryRepresentation(bounds, &mut b) {
                    let r = Rect::from_min_size(Pos2::new(b.x as f32, b.y as f32), Vec2::new(b.w as f32, b.h as f32));
                    let sides = (r.min.x - screen.min.x).abs() < 2.0 && (r.max - screen.max).length() < 2.0;
                    let top = (r.min.y - screen.min.y).abs() < 2.0 || (in_space && r.min.y >= screen.min.y && r.min.y - screen.min.y < 60.0);
                    if sides && top {
                        full = true;
                        break;
                    }
                }
            }
            CFRelease(list);
            full
        }
    }

    /// The pointer shown by the system (whichever app set it) is the plain arrow: same hot spot, same
    /// size (the arrow-like ones too: the contextual menu's, the copy's...).
    pub(super) fn plain_pointer() -> bool {
        use objc2_app_kit::NSCursor;
        // Deprecated for screen captures, but the only way to see another app's pointer.
        #[allow(deprecated)]
        let Some(current) = NSCursor::currentSystemCursor() else { return true };
        let arrow = NSCursor::arrowCursor();
        let (a, b) = (current.hotSpot(), arrow.hotSpot());
        let (sa, sb) = (current.image().size(), arrow.image().size());
        (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5 && (sa.width - sb.width).abs() < 0.5 && (sa.height - sb.height).abs() < 0.5
    }

    /// The pointer, in points from the main screen's top left (as the windows are placed).
    pub(super) fn mouse() -> Mouse {
        // SAFETY: an event made from nothing, read and released; CoreGraphics' calls are thread-safe.
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return Mouse::default();
            }
            let p = CGEventGetLocation(event);
            CFRelease(event);
            // The session's state (0), the left button (0).
            Mouse { at: Some(Pos2::new(p.x as f32, p.y as f32)), pressed: CGEventSourceButtonState(0, 0) }
        }
    }

    /// The screen under `at`, and its part left by the Dock and the menu bar (points, top left origin).
    pub(super) fn screen_at(at: Pos2) -> Option<(Rect, Rect)> {
        let mtm = objc2::MainThreadMarker::new()?;
        let screens = objc2_app_kit::NSScreen::screens(mtm);
        // AppKit counts from the bottom of the main screen, upwards.
        let height = screens.firstObject()?.frame().size.height;
        let flip = |r: objc2_foundation::NSRect| Rect::from_min_size(Pos2::new(r.origin.x as f32, (height - r.origin.y - r.size.height) as f32), Vec2::new(r.size.width as f32, r.size.height as f32));
        screens.iter().map(|s| (flip(s.frame()), flip(s.visibleFrame()))).find(|(frame, _)| frame.contains(at))
    }

    thread_local! {
        /// The Dock's frame as last read, and when.
        static DOCK: std::cell::Cell<(Option<Rect>, Option<std::time::Instant>)> = const { std::cell::Cell::new((None, None)) };
    }

    /// The Dock's frame (points, top left origin), along the foot of a screen: read through the
    /// accessibility (allowed to the app), every two seconds. Not allowed, hidden or on a side: none.
    pub(super) fn dock() -> Option<Rect> {
        DOCK.with(|cell| {
            let (known, at) = cell.get();
            if at.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2)) {
                return known;
            }
            let dock = dock_now();
            cell.set((dock, Some(std::time::Instant::now())));
            dock
        })
    }

    fn dock_now() -> Option<Rect> {
        #[repr(C)]
        #[derive(Default)]
        struct CGSize {
            width: f64,
            height: f64,
        }
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn AXIsProcessTrusted() -> bool;
            fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
            fn AXUIElementCopyAttributeValue(element: *const c_void, attribute: *const c_void, value: *mut *const c_void) -> i32;
            fn AXValueGetValue(value: *const c_void, kind: u32, out: *mut c_void) -> bool;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFArrayGetCount(array: *const c_void) -> isize;
            fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        }
        let bundle = objc2_foundation::NSString::from_str("com.apple.dock");
        let pid = objc2_app_kit::NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle).firstObject()?.processIdentifier();
        // SAFETY: CoreFoundation objects made by the copying calls, each released once; AXValues read
        // into structs of their own type (1: CGPoint, 2: CGSize).
        unsafe {
            if !AXIsProcessTrusted() {
                return None;
            }
            let app = AXUIElementCreateApplication(pid);
            if app.is_null() {
                return None;
            }
            let copy = |element: *const c_void, name: &str| -> *const c_void {
                let key = objc2_foundation::NSString::from_str(name);
                let mut value = std::ptr::null();
                if AXUIElementCopyAttributeValue(element, objc2::rc::Retained::as_ptr(&key).cast(), &mut value) == 0 { value } else { std::ptr::null() }
            };
            let children = copy(app, "AXChildren");
            let mut found = None;
            if !children.is_null() {
                for i in 0..CFArrayGetCount(children) {
                    let child = CFArrayGetValueAtIndex(children, i);
                    let (pos, size) = (copy(child, "AXPosition"), copy(child, "AXSize"));
                    let (mut p, mut s) = (CGPoint { x: 0.0, y: 0.0 }, CGSize::default());
                    if !pos.is_null() && !size.is_null() && AXValueGetValue(pos, 1, (&raw mut p).cast()) && AXValueGetValue(size, 2, (&raw mut s).cast()) {
                        found = Some(Rect::from_min_size(Pos2::new(p.x as f32, p.y as f32), Vec2::new(s.width as f32, s.height as f32)));
                    }
                    for v in [pos, size] {
                        if !v.is_null() {
                            CFRelease(v);
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                CFRelease(children);
            }
            CFRelease(app);
            // Along the foot, in sight (not hidden below the screen, not on a side).
            found.filter(|r| r.width() > r.height() && r.height() > 4.0 && screens().iter().any(|(s, _)| s.contains(r.center()) && r.min.y < s.max.y - 4.0))
        }
    }

    /// Every screen and its part left by the Dock and the menu bar (points, top left origin), left to
    /// right.
    pub(super) fn screens() -> Vec<(Rect, Rect)> {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return Vec::new() };
        let screens = objc2_app_kit::NSScreen::screens(mtm);
        let Some(main) = screens.firstObject() else { return Vec::new() };
        let height = main.frame().size.height;
        let flip = |r: objc2_foundation::NSRect| Rect::from_min_size(Pos2::new(r.origin.x as f32, (height - r.origin.y - r.size.height) as f32), Vec2::new(r.size.width as f32, r.size.height as f32));
        let mut list: Vec<(Rect, Rect)> = screens.iter().map(|s| (flip(s.frame()), flip(s.visibleFrame()))).collect();
        list.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x).then(a.0.min.y.total_cmp(&b.0.min.y)));
        list
    }

    /// Every window of ours (the other screens' too) above everything, on every desktop, letting the
    /// mouse through.
    pub(super) fn tune_all() {
        use objc2_app_kit::NSWindowCollectionBehavior as B;
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        for window in objc2_app_kit::NSApplication::sharedApplication(mtm).windows().iter() {
            if window.level() != 1000 {
                window.setLevel(1000);
                window.setCollectionBehavior(B::CanJoinAllSpaces | B::Stationary | B::FullScreenAuxiliary | B::IgnoresCycle);
                window.setHasShadow(false);
                window.setIgnoresMouseEvents(true);
            }
        }
    }

    /// Above every window, the full-screen ones too, on every desktop, out of the window cycle; never
    /// put to sleep by App Nap.
    pub(super) fn tune(cc: &eframe::CreationContext<'_>) {
        use objc2_app_kit::NSWindowCollectionBehavior as B;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = cc.window_handle()
            && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
        {
            // SAFETY: the handle points to the live NSView of our window, on the main thread.
            let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };
            if let Some(window) = view.window() {
                // NSScreenSaverWindowLevel.
                window.setLevel(1000);
                window.setCollectionBehavior(B::CanJoinAllSpaces | B::Stationary | B::FullScreenAuxiliary | B::IgnoresCycle);
                window.setHasShadow(false);
                window.setIgnoresMouseEvents(true);
            }
        }
        let info = objc2_foundation::NSProcessInfo::processInfo();
        let reason = objc2_foundation::NSString::from_str("Ronnie's rewards over the screen");
        let activity = info.beginActivityWithOptions_reason(objc2_foundation::NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason);
        // Kept for the process's life.
        std::mem::forget(activity);
    }
}

#[cfg(windows)]
mod os {
    use egui::{Pos2, Rect};
    use windows_sys::Win32::Foundation::{LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    use super::Mouse;

    /// The monitor at `p` (pixels): its rect and work area in pixels, its scale.
    fn monitor(p: POINT) -> Option<(Rect, Rect, f32)> {
        // SAFETY: a plain Win32 call.
        monitor_info(unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONULL) })
    }

    /// Monitor `m`: its rect and work area in pixels, its scale.
    fn monitor_info(m: HMONITOR) -> Option<(Rect, Rect, f32)> {
        if m.is_null() {
            return None;
        }
        // SAFETY: plain Win32 calls on values owned here.
        unsafe {
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(m, &mut info) == 0 {
                return None;
            }
            let (mut dx, mut dy) = (96u32, 96u32);
            let scale = if GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) == 0 { dx as f32 / 96.0 } else { 1.0 };
            let rect = |r: windows_sys::Win32::Foundation::RECT| Rect::from_min_max(Pos2::new(r.left as f32, r.top as f32), Pos2::new(r.right as f32, r.bottom as f32));
            Some((rect(info.rcMonitor), rect(info.rcWork), scale))
        }
    }

    /// The pointer, in points (pixels over its monitor's scale).
    pub(super) fn mouse() -> Mouse {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: plain Win32 calls.
        let (ok, pressed) = unsafe { (GetCursorPos(&mut p) != 0, (GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000) != 0) };
        if !ok {
            return Mouse::default();
        }
        let scale = monitor(p).map_or(1.0, |(_, _, s)| s);
        Mouse { at: Some(Pos2::new(p.x as f32 / scale, p.y as f32 / scale)), pressed }
    }

    /// Windows keeps its pointer (replacing it system-wide would outlive a crash).
    pub(super) fn hide_pointer(_hide: bool) {}

    /// The window in front (not the desktop's) covers its whole monitor: a film, a game.
    pub(super) fn full_screen(_screen: Rect) -> bool {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromWindow};
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect};
        // SAFETY: plain Win32 calls on values owned here.
        unsafe {
            let front = GetForegroundWindow();
            if front.is_null() || front == GetDesktopWindow() || front == GetShellWindow() {
                return false;
            }
            let mut r: RECT = std::mem::zeroed();
            if GetWindowRect(front, &mut r) == 0 {
                return false;
            }
            let m = MonitorFromWindow(front, MONITOR_DEFAULTTONULL);
            if m.is_null() {
                return false;
            }
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(m, &mut info) != 0 && r.left <= info.rcMonitor.left && r.top <= info.rcMonitor.top && r.right >= info.rcMonitor.right && r.bottom >= info.rcMonitor.bottom
        }
    }

    /// The system's pointer is the plain arrow.
    pub(super) fn plain_pointer() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{CURSORINFO, GetCursorInfo, IDC_ARROW, LoadCursorW};
        // SAFETY: plain Win32 calls on values owned here.
        unsafe {
            let mut info: CURSORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
            GetCursorInfo(&mut info) == 0 || info.hCursor == LoadCursorW(std::ptr::null_mut(), IDC_ARROW)
        }
    }

    /// The screen under `at` (points), and its work area.
    pub(super) fn screen_at(at: Pos2) -> Option<(Rect, Rect)> {
        // Back to pixels: the scales of the screens are tried until one holds the point.
        for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0] {
            let p = POINT { x: (at.x * scale) as i32, y: (at.y * scale) as i32 };
            if let Some((rect, work, s)) = monitor(p).filter(|(_, _, s)| (s - scale).abs() < 0.01) {
                let pts = |r: Rect| Rect::from_min_max((r.min.to_vec2() / s).to_pos2(), (r.max.to_vec2() / s).to_pos2());
                return Some((pts(rect), pts(work)));
            }
        }
        None
    }

    pub(super) fn tune(_cc: &eframe::CreationContext<'_>) {
        tune_all();
    }

    /// The Dock's frame: macOS only (the task bar spans the whole width).
    pub(super) fn dock() -> Option<Rect> {
        None
    }

    /// Every screen and its work area (points: each monitor's pixels over its own scale, as
    /// `screen_at` gives them), left to right.
    pub(super) fn screens() -> Vec<(Rect, Rect)> {
        unsafe extern "system" fn each(m: HMONITOR, _dc: HDC, _r: *mut RECT, list: LPARAM) -> BOOL {
            // SAFETY: `list` is the Vec handed to EnumDisplayMonitors below, alive during the call.
            let list = unsafe { &mut *(list as *mut Vec<HMONITOR>) };
            list.push(m);
            1
        }
        let mut found: Vec<HMONITOR> = Vec::new();
        // SAFETY: the callback only pushes into `found`, which outlives the call.
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &raw mut found as LPARAM) };
        let pts = |r: Rect, s: f32| Rect::from_min_max((r.min.to_vec2() / s).to_pos2(), (r.max.to_vec2() / s).to_pos2());
        let mut list: Vec<(Rect, Rect)> = found.into_iter().filter_map(monitor_info).map(|(rect, work, s)| (pts(rect, s), pts(work, s))).collect();
        list.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x).then(a.0.min.y.total_cmp(&b.0.min.y)));
        list
    }

    /// Windows 11 draws a frame (a thin line, rounded corners) around every window, ours too though
    /// they have no decorations: over a transparent window as big as the screen, only that frame
    /// showed. Taken off each of this process's windows, once (a window is made hidden, so before
    /// it shows).
    pub(super) fn tune_all() {
        use std::collections::HashSet;
        use std::sync::Mutex;

        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::Graphics::Dwm::{DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute};
        use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};

        /// No colour for the frame (`DWMWA_COLOR_NONE`).
        const COLOR_NONE: u32 = 0xFFFF_FFFE;
        static DONE: Mutex<Option<HashSet<usize>>> = Mutex::new(None);

        unsafe extern "system" fn each(w: HWND, list: LPARAM) -> BOOL {
            // SAFETY: `list` is the Vec handed to EnumWindows below, alive during the call.
            let list = unsafe { &mut *(list as *mut Vec<HWND>) };
            let mut pid = 0u32;
            // SAFETY: a plain Win32 call on a window handed by EnumWindows.
            unsafe { GetWindowThreadProcessId(w, &mut pid) };
            if pid == std::process::id() {
                list.push(w);
            }
            1
        }
        let mut ours: Vec<HWND> = Vec::new();
        // SAFETY: the callback only pushes into `ours`, which outlives the call.
        unsafe { EnumWindows(Some(each), &raw mut ours as LPARAM) };
        let Ok(mut done) = DONE.lock() else { return };
        let done = done.get_or_insert_with(HashSet::new);
        // Windows gone: their handles may come back for new ones.
        done.retain(|h| ours.iter().any(|w| *w as usize == *h));
        for w in ours {
            if !done.insert(w as usize) {
                continue;
            }
            let set = |attr: i32, value: u32| {
                // SAFETY: a plain Win32 call, `value` living through it (refused before Windows 11: nothing then).
                unsafe { DwmSetWindowAttribute(w, attr as u32, (&raw const value).cast(), std::mem::size_of::<u32>() as u32) };
            };
            set(DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED as u32);
            set(DWMWA_BORDER_COLOR, COLOR_NONE);
            set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND as u32);
        }
    }
}

/// Linux: over X11 (under Wayland, through XWayland: a Wayland window can neither place itself nor stay
/// above the others), in pixels (the process is started at scale 1). Under Wayland the pointer is only
/// known while it is over an X11 window: the walkers and the horde show, the trail and the wand hardly.
#[cfg(all(unix, not(target_os = "macos")))]
mod os {
    use std::sync::OnceLock;

    use egui::{Pos2, Rect, Vec2};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, KeyButMask, Window};
    use x11rb::rust_connection::RustConnection;

    use super::Mouse;

    /// The display's connection and its root window, opened once.
    fn x() -> Option<&'static (RustConnection, Window)> {
        static X: OnceLock<Option<(RustConnection, Window)>> = OnceLock::new();
        X.get_or_init(|| {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            Some((conn, root))
        })
        .as_ref()
    }

    fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
        Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom)
    }

    /// A property of 32-bit values.
    fn values(conn: &RustConnection, window: Window, name: &str) -> Vec<u32> {
        let Some(prop) = atom(conn, name) else { return Vec::new() };
        conn.get_property(false, window, prop, AtomEnum::ANY, 0, 4096).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(Iterator::collect)).unwrap_or_default()
    }

    /// A compositor draws the windows: without one, the transparent window would be black.
    pub(super) fn composited() -> bool {
        let Some((conn, _)) = x() else { return false };
        let screen = std::env::var("DISPLAY").ok().and_then(|d| d.rsplit('.').next().filter(|s| !s.contains(':')).and_then(|s| s.parse::<u32>().ok())).unwrap_or(0);
        atom(conn, &format!("_NET_WM_CM_S{screen}")).and_then(|a| conn.get_selection_owner(a).ok()?.reply().ok()).is_some_and(|r| r.owner != x11rb::NONE)
    }

    pub(super) fn mouse() -> Mouse {
        let Some((conn, root)) = x() else { return Mouse::default() };
        match conn.query_pointer(*root).ok().and_then(|c| c.reply().ok()) {
            Some(p) => Mouse { at: Some(Pos2::new(f32::from(p.root_x), f32::from(p.root_y))), pressed: p.mask.contains(KeyButMask::BUTTON1) },
            None => Mouse::default(),
        }
    }

    /// The system keeps its pointer (the wand drawn at its tip).
    pub(super) fn hide_pointer(_hide: bool) {}

    pub(super) fn plain_pointer() -> bool {
        true
    }

    /// The active window (not ours, nor Ronnie's) is in full screen.
    pub(super) fn full_screen(_screen: Rect) -> bool {
        let Some((conn, root)) = x() else { return false };
        let Some(&front) = values(conn, *root, "_NET_ACTIVE_WINDOW").first() else { return false };
        if front == 0 {
            return false;
        }
        let pid = values(conn, front, "_NET_WM_PID").first().copied();
        if pid.is_some_and(|p| p == std::process::id() || p == std::os::unix::process::parent_id()) {
            return false;
        }
        atom(conn, "_NET_WM_STATE_FULLSCREEN").is_some_and(|full| values(conn, front, "_NET_WM_STATE").contains(&full))
    }

    pub(super) fn screen_at(at: Pos2) -> Option<(Rect, Rect)> {
        screens().into_iter().find(|(s, _)| s.contains(at))
    }

    /// The Dock's frame: macOS only (the task bar spans the whole width).
    pub(super) fn dock() -> Option<Rect> {
        None
    }

    /// Every monitor and its part left by the panels (the desktop's work area cut to it), left to right.
    pub(super) fn screens() -> Vec<(Rect, Rect)> {
        let Some((conn, root)) = x() else { return Vec::new() };
        let Some(reply) = conn.randr_get_monitors(*root, true).ok().and_then(|c| c.reply().ok()) else { return Vec::new() };
        let area = values(conn, *root, "_NET_WORKAREA");
        let area = (area.len() >= 4).then(|| Rect::from_min_size(Pos2::new(area[0] as f32, area[1] as f32), Vec2::new(area[2] as f32, area[3] as f32)));
        let mut list: Vec<(Rect, Rect)> = reply
            .monitors
            .iter()
            .map(|m| {
                let s = Rect::from_min_size(Pos2::new(f32::from(m.x), f32::from(m.y)), Vec2::new(f32::from(m.width), f32::from(m.height)));
                let work = area.map(|a| a.intersect(s)).filter(|w| w.width() > 100.0 && w.height() > 100.0).unwrap_or(s);
                (s, work)
            })
            .collect();
        list.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x).then(a.0.min.y.total_cmp(&b.0.min.y)));
        list
    }

    pub(super) fn tune(_cc: &eframe::CreationContext<'_>) {}

    /// Every window of ours out of the task bar and the pager, on every desktop (asked of the window
    /// manager once each, when it lists them).
    pub(super) fn tune_all() {
        static DONE: std::sync::Mutex<Vec<Window>> = std::sync::Mutex::new(Vec::new());
        let Some((conn, root)) = x() else { return };
        let me = std::process::id();
        let (Some(state), Some(desktop), Some(skip_bar), Some(skip_pager), Some(sticky)) = (atom(conn, "_NET_WM_STATE"), atom(conn, "_NET_WM_DESKTOP"), atom(conn, "_NET_WM_STATE_SKIP_TASKBAR"), atom(conn, "_NET_WM_STATE_SKIP_PAGER"), atom(conn, "_NET_WM_STATE_STICKY")) else { return };
        let Ok(mut done) = DONE.lock() else { return };
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        for w in values(conn, *root, "_NET_CLIENT_LIST") {
            if done.contains(&w) || values(conn, w, "_NET_WM_PID").first() != Some(&me) {
                continue;
            }
            // 1: add; then the source, a normal application.
            let _ = conn.send_event(false, *root, mask, ClientMessageEvent::new(32, w, state, [1, skip_bar, skip_pager, 1, 0]));
            let _ = conn.send_event(false, *root, mask, ClientMessageEvent::new(32, w, state, [1, sticky, 0, 1, 0]));
            let _ = conn.send_event(false, *root, mask, ClientMessageEvent::new(32, w, desktop, [0xFFFF_FFFF, 1, 0, 0, 0]));
            done.push(w);
        }
        let _ = conn.flush();
    }
}

#[cfg(not(any(target_os = "macos", windows, unix)))]
mod os {
    use egui::{Pos2, Rect};

    use super::Mouse;

    pub(super) fn mouse() -> Mouse {
        Mouse::default()
    }

    pub(super) fn hide_pointer(_hide: bool) {}

    pub(super) fn plain_pointer() -> bool {
        true
    }

    pub(super) fn full_screen(_screen: Rect) -> bool {
        false
    }

    pub(super) fn screen_at(_at: Pos2) -> Option<(Rect, Rect)> {
        None
    }

    pub(super) fn tune(_cc: &eframe::CreationContext<'_>) {}

    /// The Dock's frame: macOS only (the task bar spans the whole width).
    pub(super) fn dock() -> Option<Rect> {
        None
    }

    pub(super) fn screens() -> Vec<(Rect, Rect)> {
        Vec::new()
    }

    pub(super) fn tune_all() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_droid_rolls_now_and_then() {
        let ctx = egui::Context::default();
        let painter = egui::Painter::new(ctx, egui::LayerId::background(), Rect::EVERYTHING);
        let strip = Rect::from_min_max(Pos2::new(0.0, 800.0), Pos2::new(1400.0, 834.0));
        let shown = (0..4200).filter(|k| droid_walk(&painter, strip, *k as f64 / 10.0).0).count();
        // 12 s out of 420.
        assert!((110..=130).contains(&shown), "{shown}");
        let (_, again) = droid_walk(&painter, strip, 30.0);
        assert!(again > 0.0);
    }

    #[test]
    fn gerard_strolls_now_and_then() {
        let shown = (0..6000).filter(|k: &i32| gerard_at(f64::from(*k) / 10.0).is_ok()).count();
        // 34 s out of 600, waving 6 s of them, halfway.
        assert!((330..=350).contains(&shown), "{shown}");
        let waving = (0..6000).filter_map(|k: i32| gerard_at(f64::from(k) / 10.0).ok()).filter(|w| w.2).count();
        assert!((55..=65).contains(&waving), "{waving}");
        assert!(gerard_at(0.0).is_err_and(|again| again > 0.0));
    }

    #[test]
    fn matheo_lands_at_the_foot_of_his_screen() {
        let works = [Rect::from_min_max(Pos2::new(0.0, 25.0), Pos2::new(1440.0, 860.0)), Rect::from_min_max(Pos2::new(1440.0, 0.0), Pos2::new(3360.0, 1040.0))];
        // On the second screen: its foot. Between two (past the last): back on the nearest one.
        assert_eq!(matheo_floor(Pos2::new(2000.0, 300.0), &works, None), (2000.0, 1040.0));
        assert_eq!(matheo_floor(Pos2::new(700.0, 100.0), &works, None), (700.0, 860.0));
        let (x, y) = matheo_floor(Pos2::new(3500.0, 100.0), &works, None);
        assert!(x < 3360.0 && y == 1040.0);
    }

    #[test]
    fn matheo_lands_on_the_screen_under_him_when_screens_are_stacked() {
        // A laptop under a bigger screen.
        let top = Rect::from_min_max(Pos2::new(-200.0, -1080.0), Pos2::new(1720.0, 0.0));
        let bottom = Rect::from_min_max(Pos2::new(0.0, 25.0), Pos2::new(1512.0, 982.0));
        let grounds = [top, bottom];
        assert_eq!(matheo_floor(Pos2::new(700.0, 500.0), &grounds, None), (700.0, 982.0));
        assert_eq!(matheo_floor(Pos2::new(700.0, -500.0), &grounds, None), (700.0, 0.0));
        // Between the two (the lower one's menu bar): onto the lower one.
        assert_eq!(matheo_floor(Pos2::new(700.0, 10.0), &grounds, None), (700.0, 982.0));
    }

    #[test]
    fn matheo_stands_on_the_dock_and_beside_it_at_the_very_foot() {
        // A screen of 1710 × 1107, the Dock 762 wide in its middle, 49 high, 55 left by it.
        let screens = [(Rect::from_min_max(Pos2::ZERO, Pos2::new(1710.0, 1107.0)), Rect::from_min_max(Pos2::new(0.0, 34.0), Pos2::new(1710.0, 1052.0)))];
        let works = [screens[0].1];
        let dock = Some(Rect::from_min_size(Pos2::new(474.0, 1048.0), Vec2::new(762.0, 49.0)));
        let grounds = matheo_grounds(&screens, &works, dock);
        assert_eq!(matheo_floor(Pos2::new(800.0, 300.0), &grounds, dock), (800.0, 1048.0));
        assert_eq!(matheo_floor(Pos2::new(1500.0, 300.0), &grounds, dock), (1500.0, 1107.0));
        assert_eq!(matheo_floor(Pos2::new(200.0, 300.0), &grounds, dock), (200.0, 1107.0));
        // The Dock unknown: above it, all along.
        assert_eq!(matheo_floor(Pos2::new(1500.0, 300.0), &matheo_grounds(&screens, &works, None), None), (1500.0, 1052.0));
    }

    #[test]
    fn each_walk_on_one_screen() {
        // One screen: always it. Several: the same all through a walk, every screen in turn.
        assert!((0..50).all(|lap| walk_screen(f64::from(lap), 1, 1) == 0));
        let seen: std::collections::HashSet<usize> = (0..60).map(|lap| walk_screen(f64::from(lap), 2, 3)).collect();
        assert_eq!(seen.len(), 3);
        let fx = Fx { sheep: true, ..Default::default() };
        let at = walks(&fx, 282.0, 3)[0];
        assert!(at.0 && (0..30).all(|k| walks(&fx, 282.0 + f64::from(k) * 0.4, 3)[0] == at));
    }

    #[test]
    fn the_wolf_trots_by_now_and_then() {
        let shown = (0..1100).filter(|k| wolf_at(*k as f64 / 10.0).is_ok()).count();
        // 16 s out of 110, 4 of them howling.
        assert!((150..=170).contains(&shown), "{shown}");
        let howling = (0..1100).filter(|k| wolf_at(*k as f64 / 10.0).is_ok_and(|a| a.2)).count();
        assert!((35..=45).contains(&howling), "{howling}");
        assert!(wolf_at(30.0).is_err_and(|again| again > 0.0));
    }

    #[test]
    fn the_elf_walks_by_now_and_then() {
        let shown = (0..4800).filter(|k| elf_at(*k as f64 / 10.0).is_ok()).count();
        // 15 s out of 480, 3 of them waving.
        assert!((140..=160).contains(&shown), "{shown}");
        let waving = (0..4800).filter(|k| elf_at(*k as f64 / 10.0).is_ok_and(|a| a.2)).count();
        assert!((25..=35).contains(&waving), "{waving}");
        assert!(elf_at(40.0).is_err_and(|again| again > 0.0));
    }

    #[test]
    fn the_line_sent_reads_back() {
        let fx = Fx { trail: true, sheep: false, droid: true, horde: true, gerard: true, horde_demo: None, horde_kinds: vec![1, 4], matheo: true, matheo_tips: true, lang: crate::i18n::Lang::En, elf: true, unicorn: true, wolf: true, bills: true, pointer: Some(Pointer::Wand), theme: "gryffondor".into() };
        let back: Fx = serde_json::from_str(&serde_json::to_string(&fx).unwrap()).unwrap();
        assert_eq!(back, fx);
        assert!(fx.any() && fx.follows());
        assert!(!Fx::default().any());
    }

    #[test]
    fn mathéo_says_water_and_moving_in_turns() {
        for lang in crate::i18n::Lang::ALL {
            let t = lang.strings();
            assert!(t.matheo_water.contains(&matheo_tip(lang, 0)));
            assert!(t.matheo_moves.contains(&matheo_tip(lang, 1)));
            for said in [Said::Tip(7), Said::Shaking, Said::Quiet, Said::Ouch, Said::Thanks, Said::Bed(12), Said::Thirsty, Said::Weekend(3), Said::Break(4), Said::Hello, Said::Hiding, Said::Gone] {
                assert!(!said.text(lang).is_empty(), "{said:?}");
            }
        }
        // Every one of each list comes, in turn.
        let lang = crate::i18n::Lang::Fr;
        let waters: std::collections::HashSet<_> = (0..40).step_by(2).map(|k| matheo_tip(lang, k)).collect();
        assert_eq!(waters.len(), lang.strings().matheo_water.len());
    }

    #[test]
    fn the_can_stands_on_the_free_side_and_comes_back_down() {
        let screen = [Rect::from_min_max(Pos2::ZERO, Pos2::new(1000.0, 800.0))];
        assert_eq!(can_side(Pos2::new(500.0, 800.0), &screen), 1.0);
        assert_eq!(can_side(Pos2::new(950.0, 800.0), &screen), -1.0);
        let foot = Pos2::new(500.0, 800.0);
        let rest = can_view(foot, 1.0, Instant::now() - Duration::from_secs(5), None);
        assert!((rest.origin - (foot + Vec2::new(48.0, 0.0))).length() < 0.5);
        // Up over his leaves, tipped towards him, while pouring.
        let pouring = can_view(foot, 1.0, Instant::now(), Some(Instant::now() - Duration::from_secs(1)));
        assert!(pouring.origin.y < foot.y - 80.0 && pouring.angle < -1.0, "{:?} {}", pouring.origin, pouring.angle);
        assert!(can_rect(foot, 1.0).contains(rest.origin - Vec2::new(0.0, 10.0)));
    }

    #[test]
    fn mathéo_wilts_with_thirst() {
        let mut plant = Plant { thirst: 0, ..Plant::default() };
        assert_eq!(plant.wilt(), 0.0);
        plant.thirst = THIRST_MAX + 4;
        plant.thirst = plant.thirst.min(THIRST_MAX);
        assert_eq!(plant.wilt(), 1.0);
    }
}
