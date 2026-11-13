//! The rewards over the whole screen, outside Ronnie too: the pointer's trail, the wand, the lightsaber
//! or the sword at the pointer, the sheep, the droid and Gérard walking along the foot of the screen.
//!
//! They live in a process of their own (`ronnie desktop-fx`), started by Ronnie while one of them is
//! chosen: a transparent window over the screen where the pointer is, letting every click through, out of
//! the Dock and the task bar, above the other windows (full-screen ones too, on macOS). It keeps going
//! while Ronnie's window is minimized or behind others, and ends with Ronnie: what to show comes on its
//! standard input, one JSON line at each change, and the input closing (Ronnie gone) ends it. The
//! pointer is read about 60 times a second; the window is only drawn again when something moves.
//!
//! With the wand or the lightsaber, the system's pointer is hidden everywhere on macOS (it comes back by
//! itself when the process ends, even crashing); on Windows it stays, the wand drawn at its tip. macOS
//! and Windows; not Linux (no way there to stay above everything for X11 and Wayland both).

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

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
    pointer: Option<Pointer>,
    /// The theme whose colors the trail takes.
    theme: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
enum Pointer {
    Wand,
    Saber,
    Sword,
}

impl Fx {
    fn any(&self) -> bool {
        self.trail || self.sheep || self.droid || self.horde || self.gerard || self.matheo || self.pointer.is_some()
    }

    /// The pointer is followed (else it needn't be): Mathéo too, to be grabbed.
    fn follows(&self) -> bool {
        self.trail || self.matheo || self.pointer.is_some()
    }
}

/// The systems where it works.
pub(super) fn supported() -> bool {
    cfg!(any(target_os = "macos", windows))
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
    /// What the settings ask to show over the screen (each reward still to be won, left out).
    fn desktop_wanted(&self) -> Fx {
        let s = &self.config.settings;
        let d = s.desktop_fx;
        let official = crate::config::OFFICIAL;
        let star_wars = !official || s.hyperspace_unlocked;
        let pointer = if d.wand && (!official || s.wizard_unlocked) {
            Some(Pointer::Wand)
        } else if d.saber && star_wars {
            Some(Pointer::Saber)
        } else if d.sword && (!official || crate::coffee::thrones_won(&s.coffee_bests)) {
            Some(Pointer::Sword)
        } else {
            None
        };
        Fx { trail: d.trail && s.cursor_trail && trail::unlocked(s.runner_best), sheep: d.sheep && s.sheep && office::sheep_unlocked(s), droid: d.droid && star_wars, horde: d.horde && (!official || s.monday_horde), gerard: d.gerard && (!official || s.monday_gerard), horde_demo: self.desktop.horde_demo.filter(|_| d.horde), horde_kinds: super::zombie::met_kinds(s), matheo: d.matheo && (!official || s.riff_games >= super::riff::MATHEO_GAMES), pointer, theme: s.theme.clone() }
    }

    /// The pointer's effects are drawn by the process over the screen (Ronnie doesn't draw its own:
    /// they would show twice).
    pub(super) fn desktop_draws(&self) -> (bool, bool, bool) {
        let fx = self.desktop.sent.as_ref().filter(|_| self.desktop.child.is_some());
        (fx.is_some_and(|f| f.trail), fx.is_some_and(|f| f.pointer.is_some()), fx.is_some_and(|f| f.sheep))
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
    /// Another app fills the screen (a film, a full-screen window): nothing shown then; and when
    /// last looked at.
    full: (bool, Option<Instant>),
    matheo: Plant,
}

/// Mathéo where he stands (his pot's foot, screen points), falling, or held.
struct Plant {
    at: Option<Pos2>,
    vy: f32,
    /// Held by the mouse: from the pointer to his foot.
    grab: Option<Vec2>,
    /// The window lets the mouse through (not over him).
    pass: bool,
}

impl Default for Plant {
    fn default() -> Self {
        Self { at: matheo_load(), vy: 0.0, grab: None, pass: true }
    }
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

/// Mathéo's place among the screens' foot: where he lands (the bottom of the screen under him, above
/// the Dock), back on a screen if left between two.
fn matheo_floor(at: Pos2, works: &[Rect]) -> (f32, f32) {
    let Some(work) = works.iter().find(|w| (w.min.x..w.max.x).contains(&at.x)).or_else(|| works.iter().min_by(|a, b| a.center().distance(at).total_cmp(&b.center().distance(at)))) else {
        return (at.x, at.y);
    };
    let half = MATHEO_H * 0.4;
    (at.x.clamp(work.min.x + half, work.max.x - half), work.max.y)
}

/// Mathéo, Riff Hero's green plant, his pot's foot at `foot`, `size` tall: a terracotta pot with a
/// face, his big leaves swaying; held, his eyes wide and his leaves flapping.
pub(super) fn paint_matheo(painter: &egui::Painter, foot: Pos2, size: f32, now: f64, held: bool) {
    let u = size / 70.0;
    let at = |x: f32, y: f32| foot + Vec2::new(x, y) * u;
    // The shadow, the pot (wider at the top), its rim.
    painter.add(egui::Shape::ellipse_filled(at(0.0, 0.0), Vec2::new(20.0, 4.0) * u, Color32::from_black_alpha(60)));
    painter.add(egui::Shape::convex_polygon(vec![at(-14.0, 0.0), at(14.0, 0.0), at(18.0, -26.0), at(-18.0, -26.0)], Color32::from_rgb(196, 104, 62), Stroke::NONE));
    painter.rect_filled(Rect::from_min_max(at(-21.0, -32.0), at(21.0, -24.0)), 2.0 * u, Color32::from_rgb(214, 122, 76));
    painter.rect_filled(Rect::from_min_max(at(-19.0, -33.0), at(19.0, -31.0)), 1.0, Color32::from_rgb(92, 62, 40));
    // His face on the pot.
    let blink = (now % 4.0) < 0.12 && !held;
    for s in [-1.0, 1.0] {
        let eye = at(s * 6.0, -15.0);
        if held {
            painter.circle_filled(eye, 3.2 * u, Color32::WHITE);
            painter.circle_filled(eye, 1.8 * u, Color32::from_rgb(40, 30, 24));
        } else if blink {
            painter.line_segment([eye - Vec2::new(2.0, 0.0) * u, eye + Vec2::new(2.0, 0.0) * u], Stroke::new(1.5 * u, Color32::from_rgb(60, 36, 24)));
        } else {
            painter.circle_filled(eye, 2.0 * u, Color32::from_rgb(60, 36, 24));
        }
    }
    if held {
        painter.circle_stroke(at(0.0, -8.0), 2.5 * u, Stroke::new(1.5 * u, Color32::from_rgb(60, 36, 24)));
    } else {
        let smile: Vec<Pos2> = (0..=6).map(|k| at(-4.0 + k as f32 * 8.0 / 6.0, -9.0 + ((k as f32 - 3.0) / 3.0).powi(2) * -2.0 + 2.0)).collect();
        painter.add(egui::Shape::line(smile, Stroke::new(1.5 * u, Color32::from_rgb(60, 36, 24))));
    }
    painter.circle_filled(at(-10.0, -11.0), 2.2 * u, Color32::from_rgba_unmultiplied(255, 140, 140, 90));
    painter.circle_filled(at(10.0, -11.0), 2.2 * u, Color32::from_rgba_unmultiplied(255, 140, 140, 90));
    // His leaves: stems from the soil, big leaves swaying (flapping, held).
    let speed = if held { 9.0 } else { 1.3 };
    for (k, (angle, len)) in [(-1.0f32, 30.0f32), (-0.45, 38.0), (0.1, 42.0), (0.6, 36.0), (1.1, 28.0)].into_iter().enumerate() {
        let sway = (now * speed + k as f64 * 0.9).sin() as f32 * if held { 0.25 } else { 0.08 };
        let a = -std::f32::consts::FRAC_PI_2 + angle * 0.8 + sway;
        let base = at((k as f32 - 2.0) * 3.0, -32.0);
        let tip = base + Vec2::angled(a) * len * u;
        let mid = base.lerp(tip, 0.55);
        painter.line_segment([base, mid], Stroke::new(2.0 * u, Color32::from_rgb(60, 120, 50)));
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
        painter.add(egui::Shape::convex_polygon(leaf, [Color32::from_rgb(72, 160, 70), Color32::from_rgb(90, 180, 80)][k % 2], Stroke::new(1.0, Color32::from_rgb(40, 100, 40))));
        painter.line_segment([mid, mid + dir * len * 0.55 * u], Stroke::new(1.0, Color32::from_rgb(140, 210, 120)));
    }
}

/// `ronnie desktop-fx`: the effects' window, until its standard input closes.
pub(crate) fn run() {
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
            Ok(Box::new(Overlay { fx, mouse, shown: Fx::default(), theme, screen: start, work, trail: Default::default(), wand: Default::default(), pressed: false, last: None, plain: (true, None), full: (false, None), matheo: Plant::default() }))
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
const GERARD_EVERY: f64 = 150.0;
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
const DROID_EVERY: f64 = 70.0;
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

/// The sheep's, the droid's and Gérard's walks under way, and the screen each goes along.
fn walks(fx: &Fx, now: f64, n: usize) -> [(bool, usize); 3] {
    let gerard = gerard_every();
    [
        (fx.sheep && (now + 20.0) % 50.0 <= 16.0, walk_screen(((now + 20.0) / 50.0).floor(), 1, n)),
        (fx.droid && (now + 55.0) % DROID_EVERY <= DROID_ROLL, walk_screen(((now + 55.0) / DROID_EVERY).floor(), 2, n)),
        (fx.gerard && gerard_at(now).is_ok(), walk_screen(((now + 95.0) / gerard).floor(), 3, n)),
    ]
}

/// Something to show on screen `here` now: a walker going along it, the horde.
fn walkers_on(fx: &Fx, now: f64, here: usize, n: usize) -> bool {
    (fx.horde && horde_since(fx.horde_demo).is_ok()) || walks(fx, now, n).iter().any(|&(on, at)| on && at == here)
}

/// On screen `here` (of `n`): the walkers going along it (`strip`, its foot) and the horde (`whole`).
/// Something shown, and when to look again.
fn paint_walkers(painter: &egui::Painter, strip: Rect, whole: Rect, now: f64, here: usize, n: usize, fx: &Fx) -> (bool, f32) {
    let [sheep, droid, gerard] = walks(fx, now, n);
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
    if fx.horde {
        let (shown, again) = horde_run(painter, whole, fx.horde_demo, &fx.horde_kinds);
        busy |= shown;
        next = next.min(again);
    }
    (busy, next)
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
        // Another app full screen (a film...): nothing over it, the pointer its own. Looked at each second.
        if self.full.1.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            self.full = (os::full_screen(self.screen), Some(Instant::now()));
        }
        if self.full.0 {
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
        // This window too, put back on its screen's top left (macOS slid it under the menu bar when it
        // was made, before it was raised above everything: all drawn too low).
        if ctx.input(|i| i.viewport().outer_rect).is_some_and(|r| (r.min - self.screen.min).length() > 1.0) {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.screen.min));
        }
        let local = mouse.at.filter(|p| self.screen.contains(*p)).map(|p| (p - self.screen.min).to_pos2());
        let click = mouse.pressed && !self.pressed;
        self.pressed = mouse.pressed;
        let screens = os::screens();
        let works: Vec<Rect> = if screens.is_empty() { vec![self.work] } else { screens.iter().map(|s| s.1).collect() };
        // Mathéo: grabbed with the mouse (the window takes the click over him only), dragged, dropped:
        // he falls back to the foot of the screen under him.
        let mut plant_busy = false;
        if self.shown.matheo {
            let plant = &mut self.matheo;
            let at = *plant.at.get_or_insert_with(|| Pos2::new(self.work.max.x - 160.0, self.work.max.y));
            let body = Rect::from_center_size(at - Vec2::new(0.0, MATHEO_H * 0.5), Vec2::new(MATHEO_H * 0.7, MATHEO_H));
            let over = mouse.at.is_some_and(|p| body.contains(p));
            if click && over {
                plant.grab = mouse.at.map(|p| at - p);
            }
            match (plant.grab, mouse.at) {
                (Some(grab), Some(p)) if mouse.pressed => {
                    plant.at = Some(p + grab);
                    plant.vy = 0.0;
                    plant_busy = true;
                }
                (Some(_), _) => plant.grab = None,
                _ => {
                    let (x, floor) = matheo_floor(at, &works);
                    if at.y < floor - 0.5 || plant.vy != 0.0 || x != at.x {
                        plant.vy += 2600.0 * dt;
                        let mut y = at.y + plant.vy * dt;
                        if y >= floor {
                            y = floor;
                            plant.vy = if plant.vy > 500.0 { -plant.vy * 0.3 } else { 0.0 };
                            if plant.vy == 0.0 {
                                matheo_save(Pos2::new(x, y));
                            }
                        }
                        plant.at = Some(Pos2::new(x, y));
                        plant_busy = true;
                    }
                }
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
        let plant = self.matheo.at.filter(|_| self.shown.matheo).map(|at| (at, self.matheo.grab.is_some()));

        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("desktop-fx")));
        let mut busy = false;
        let mut next: f32 = 1.0;
        // Along the foot of the screen, above the Dock or the task bar.
        let foot = self.work.translate(-self.screen.min.to_vec2());
        let strip = Rect::from_min_max(Pos2::new(foot.min.x + 6.0, foot.max.y - 34.0), Pos2::new(foot.max.x - 6.0, foot.max.y));
        // The screens: each walker along one of them (the same all through its walk), the horde along
        // all of them at once. This window covers the pointer's; the others get one each while needed.
        let n = screens.len().max(1);
        let here = screens.iter().position(|(s, _)| (s.min - self.screen.min).length() < 1.0).unwrap_or(0);
        let (shown, again) = paint_walkers(&painter, strip, Rect::from_min_size(Pos2::ZERO, self.screen.size()), now, here, n, &self.shown);
        busy |= shown || plant_busy;
        next = next.min(again);
        // Mathéo on the screen he's on (this window's, or another's).
        let plant_on = plant.map(|(at, _)| screens.iter().position(|(s, _)| s.contains(at - Vec2::new(0.0, 1.0))).unwrap_or(here));
        if let (Some((at, held)), true) = (plant, plant_on == Some(here)) {
            paint_matheo(&painter, at - self.screen.min.to_vec2(), MATHEO_H, now, held);
            next = next.min(0.1);
        }
        for (i, (screen, work)) in screens.iter().enumerate() {
            if i == here || !(walkers_on(&self.shown, now, i, n) || plant_on == Some(i)) {
                continue;
            }
            let (screen, work, fx) = (*screen, *work, self.shown.clone());
            let builder = egui::ViewportBuilder::default()
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
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("fx-screen", i)), builder, |ctx, _| {
                // Put back on its screen's top left: macOS slides a new window under the menu bar
                // (before it is raised above everything), everything in it then drawn too low.
                if ctx.input(|i| i.viewport().outer_rect).is_some_and(|r| (r.min - screen.min).length() > 1.0) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(screen.min));
                }
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("desktop-fx")));
                let foot = work.translate(-screen.min.to_vec2());
                let strip = Rect::from_min_max(Pos2::new(foot.min.x + 6.0, foot.max.y - 34.0), Pos2::new(foot.max.x - 6.0, foot.max.y));
                paint_walkers(&painter, strip, Rect::from_min_size(Pos2::ZERO, screen.size()), now, i, n, &fx);
                if let (Some((at, held)), true) = (plant, plant_on == Some(i)) {
                    paint_matheo(&painter, at - screen.min.to_vec2(), MATHEO_H, now, held);
                }
            });
            os::tune_all();
            if walkers_on(&self.shown, now, i, n) {
                busy = true;
            } else {
                // Mathéo alone there: his leaves sway gently.
                next = next.min(0.1);
            }
        }
        if self.shown.trail {
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
                    hyperspace::pointer_saber(&painter, at, self.theme.accent);
                }
            }
            Some(Pointer::Sword) => {
                if let Some(at) = local {
                    thrones::pointer_sword(&painter, at);
                }
            }
            None => {}
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
        fn CFNumberGetValue(number: *const c_void, kind: i32, out: *mut c_void) -> bool;
    }

    /// The app in front (not Ronnie) has a window covering the whole of `screen` (points, top left
    /// origin): a film, a game, a window in full screen.
    pub(super) fn full_screen(screen: Rect) -> bool {
        let Some(front) = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication() else { return false };
        let pid = front.processIdentifier();
        if pid as u32 == std::os::unix::process::parent_id() || pid as u32 == std::process::id() {
            return false;
        }
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
                    if (r.min - screen.min).length() < 2.0 && (r.max - screen.max).length() < 2.0 {
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
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    use super::Mouse;

    /// The monitor at `p` (pixels): its rect and work area in pixels, its scale.
    fn monitor(p: POINT) -> Option<(Rect, Rect, f32)> {
        // SAFETY: plain Win32 calls on values owned here.
        unsafe {
            let m = MonitorFromPoint(p, MONITOR_DEFAULTTONULL);
            if m.is_null() {
                return None;
            }
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

    pub(super) fn tune(_cc: &eframe::CreationContext<'_>) {}

    /// One screen only here (the pointer's).
    pub(super) fn screens() -> Vec<(Rect, Rect)> {
        Vec::new()
    }

    pub(super) fn tune_all() {}
}

#[cfg(not(any(target_os = "macos", windows)))]
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
        let shown = (0..700).filter(|k| droid_walk(&painter, strip, *k as f64 / 10.0).0).count();
        // 12 s out of 70.
        assert!((110..=130).contains(&shown), "{shown}");
        let (_, again) = droid_walk(&painter, strip, 30.0);
        assert!(again > 0.0);
    }

    #[test]
    fn gerard_strolls_now_and_then() {
        let shown = (0..1500).filter(|k: &i32| gerard_at(f64::from(*k) / 10.0).is_ok()).count();
        // 34 s out of 150, waving 6 s of them, halfway.
        assert!((330..=350).contains(&shown), "{shown}");
        let waving = (0..1500).filter_map(|k: i32| gerard_at(f64::from(k) / 10.0).ok()).filter(|w| w.2).count();
        assert!((55..=65).contains(&waving), "{waving}");
        assert!(gerard_at(0.0).is_err_and(|again| again > 0.0));
    }

    #[test]
    fn matheo_lands_at_the_foot_of_his_screen() {
        let works = [Rect::from_min_max(Pos2::new(0.0, 25.0), Pos2::new(1440.0, 860.0)), Rect::from_min_max(Pos2::new(1440.0, 0.0), Pos2::new(3360.0, 1040.0))];
        // On the second screen: its foot. Between two (past the last): back on the nearest one.
        assert_eq!(matheo_floor(Pos2::new(2000.0, 300.0), &works), (2000.0, 1040.0));
        assert_eq!(matheo_floor(Pos2::new(700.0, 100.0), &works), (700.0, 860.0));
        let (x, y) = matheo_floor(Pos2::new(3500.0, 100.0), &works);
        assert!(x < 3360.0 && y == 1040.0);
    }

    #[test]
    fn each_walk_on_one_screen() {
        // One screen: always it. Several: the same all through a walk, every screen in turn.
        assert!((0..50).all(|lap| walk_screen(f64::from(lap), 1, 1) == 0));
        let seen: std::collections::HashSet<usize> = (0..60).map(|lap| walk_screen(f64::from(lap), 2, 3)).collect();
        assert_eq!(seen.len(), 3);
        let fx = Fx { sheep: true, ..Default::default() };
        let at = walks(&fx, 32.0, 3)[0];
        assert!(at.0 && (0..30).all(|k| walks(&fx, 32.0 + f64::from(k) * 0.4, 3)[0] == at));
    }

    #[test]
    fn the_line_sent_reads_back() {
        let fx = Fx { trail: true, sheep: false, droid: true, horde: true, gerard: true, horde_demo: None, horde_kinds: vec![1, 4], matheo: true, pointer: Some(Pointer::Wand), theme: "gryffondor".into() };
        let back: Fx = serde_json::from_str(&serde_json::to_string(&fx).unwrap()).unwrap();
        assert_eq!(back, fx);
        assert!(fx.any() && fx.follows());
        assert!(!Fx::default().any());
    }
}
