//! The rewards over the whole screen, outside Ronnie too: the pointer's trail, the wand or the
//! lightsaber at the pointer, the sheep and the droid walking along the foot of the screen.
//!
//! They live in a process of their own (`ronnie desktop-fx`), started by Ronnie while one of them is
//! chosen: a transparent window over the screen where the pointer is, letting every click through, out of
//! the Dock and the task bar, above the other windows (full-screen ones too, on macOS). It keeps going
//! while Ronnie's window is minimized or behind others, and ends with Ronnie: what to show comes on its
//! standard input, one JSON line at each change, and the input closing (Ronnie gone) ends it. The
//! pointer is read about 60 times a second; the window is only drawn again when something moves.
//!
//! The system's own pointer can't be replaced outside Ronnie: the wand and the lightsaber are drawn at
//! its tip. macOS and Windows; not Linux (no way there to stay above everything for X11 and Wayland both).

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
    pointer: Option<Pointer>,
    /// The theme whose colors the trail takes.
    theme: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
enum Pointer {
    Wand,
    Saber,
}

impl Fx {
    fn any(&self) -> bool {
        self.trail || self.sheep || self.droid || self.pointer.is_some()
    }

    /// The pointer is followed (else it needn't be).
    fn follows(&self) -> bool {
        self.trail || self.pointer.is_some()
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
        } else {
            None
        };
        Fx { trail: d.trail && s.cursor_trail && trail::unlocked(s.runner_best), sheep: d.sheep && s.sheep && office::sheep_unlocked(s), droid: d.droid && star_wars, pointer, theme: s.theme.clone() }
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
            Ok(Box::new(Overlay { fx, mouse, shown: Fx::default(), theme, screen: start, work, trail: Default::default(), wand: Default::default(), pressed: false, last: None }))
        }),
    );
    if let Err(e) = result {
        crate::log::error(&format!("desktop effects: {e}"));
    }
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
    hyperspace::droid(painter, Pos2::new(x, strip.max.y - 2.0), 34.0, now);
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
        let local = mouse.at.filter(|p| self.screen.contains(*p)).map(|p| (p - self.screen.min).to_pos2());
        let click = mouse.pressed && !self.pressed;
        self.pressed = mouse.pressed;

        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("desktop-fx")));
        let mut busy = false;
        let mut next: f32 = 1.0;
        // Along the foot of the screen, above the Dock or the task bar.
        let foot = self.work.translate(-self.screen.min.to_vec2());
        let strip = Rect::from_min_max(Pos2::new(foot.min.x + 6.0, foot.max.y - 34.0), Pos2::new(foot.max.x - 6.0, foot.max.y));
        if self.shown.sheep {
            let (shown, again) = office::sheep(&painter, strip, now);
            busy |= shown.is_some();
            next = next.min(again);
        }
        if self.shown.droid {
            let (shown, again) = droid_walk(&painter, strip, now);
            busy |= shown;
            next = next.min(again);
        }
        if self.shown.trail {
            self.trail.step(local, dt);
            self.trail.paint(&painter, &self.theme);
            busy |= self.trail.alive();
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
}

#[cfg(not(any(target_os = "macos", windows)))]
mod os {
    use egui::{Pos2, Rect};

    use super::Mouse;

    pub(super) fn mouse() -> Mouse {
        Mouse::default()
    }

    pub(super) fn screen_at(_at: Pos2) -> Option<(Rect, Rect)> {
        None
    }

    pub(super) fn tune(_cc: &eframe::CreationContext<'_>) {}
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
    fn the_line_sent_reads_back() {
        let fx = Fx { trail: true, sheep: false, droid: true, pointer: Some(Pointer::Wand), theme: "gryffondor".into() };
        let back: Fx = serde_json::from_str(&serde_json::to_string(&fx).unwrap()).unwrap();
        assert_eq!(back, fx);
        assert!(fx.any() && fx.follows());
        assert!(!Fx::default().any());
    }
}
