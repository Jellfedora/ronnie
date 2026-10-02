//! The activity simulator: while the user is away from the keyboard during the chosen hours, the system
//! is told the user is still there, so that chat apps (Teams...) don't show them as away.
//!
//! macOS: a user activity declared to the power manager, and a mouse moved by zero pixels (only posted
//! once Ronnie is allowed in Accessibility). Windows: a mouse moved by zero pixels. Linux: on X11 the
//! pointer nudged by one pixel and back (XTest); on Wayland, the desktop's screen saver told of activity
//! over D-Bus.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{Datelike, Timelike};

use crate::config::KeepActive;

/// Without input for this long, the user is pretended to be there.
const IDLE: f64 = 50.0;
/// How often the thread looks.
const TICK: Duration = Duration::from_secs(15);

static WANTED: Mutex<Option<KeepActive>> = Mutex::new(None);

/// Applies the settings (cheap when unchanged); the thread starts the first time.
pub fn configure(settings: &KeepActive) {
    let mut wanted = WANTED.lock().unwrap();
    if wanted.as_ref() == Some(settings) {
        return;
    }
    let first = wanted.is_none();
    *wanted = Some(settings.clone());
    if first {
        let _ = std::thread::Builder::new().name("awake".into()).spawn(run);
    }
}

/// Whether the simulator acts right now (on, and within its hours).
pub fn active_now(settings: &KeepActive) -> bool {
    let now = chrono::Local::now();
    settings.enabled && in_schedule(settings, now.weekday().num_days_from_monday() as usize, (now.hour() * 60 + now.minute()) as u16)
}

/// `day`: 0 for Monday; `minute`: since midnight.
fn in_schedule(settings: &KeepActive, day: usize, minute: u16) -> bool {
    settings.days.get(day).copied().unwrap_or(false) && settings.ranges.iter().any(|&(from, to)| from <= minute && minute < to)
}

fn run() {
    let mut system = System::default();
    loop {
        std::thread::sleep(TICK);
        let Some(settings) = WANTED.lock().unwrap().clone() else { continue };
        if !active_now(&settings) {
            continue;
        }
        // Unknown idle time (Wayland): told every time, it does no harm.
        if system.idle_seconds().is_none_or(|idle| idle >= IDLE) {
            system.nudge();
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct System {
    /// The user activity assertion, renewed each time.
    assertion: u32,
}

#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        pub fn IOPMAssertionDeclareUserActivity(name: *const c_void, kind: u32, id: *mut u32) -> i32;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        pub fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
        pub fn CGEventCreate(source: *const c_void) -> *mut c_void;
        pub fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        pub fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, at: CGPoint, button: u32) -> *mut c_void;
        pub fn CGEventPost(tap: u32, event: *mut c_void);
        pub fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFRelease(object: *const c_void);
    }
}

#[cfg(target_os = "macos")]
impl System {
    fn idle_seconds(&self) -> Option<f64> {
        // The hardware's state (1), any kind of input (!0).
        Some(unsafe { ffi::CGEventSourceSecondsSinceLastEventType(1, u32::MAX) })
    }

    fn nudge(&mut self) {
        let name = objc2_foundation::NSString::from_str("Ronnie");
        let name = objc2::rc::Retained::as_ptr(&name).cast();
        // kIOPMUserActiveLocal.
        let result = unsafe { ffi::IOPMAssertionDeclareUserActivity(name, 0, &mut self.assertion) };
        if result != 0 {
            crate::log::info(&format!("awake: user activity refused ({result:#x})"));
        }
        if !accessibility_allowed() {
            return;
        }
        // The mouse moved where it already is (kCGEventMouseMoved, on the HID tap).
        unsafe {
            let here = ffi::CGEventCreate(std::ptr::null());
            if here.is_null() {
                return;
            }
            let at = ffi::CGEventGetLocation(here);
            ffi::CFRelease(here);
            let event = ffi::CGEventCreateMouseEvent(std::ptr::null(), 5, at, 0);
            if !event.is_null() {
                ffi::CGEventPost(0, event);
                ffi::CFRelease(event);
            }
        }
    }
}

/// macOS: Ronnie may post input events (Accessibility); the other systems need nothing.
pub fn accessibility_allowed() -> bool {
    #[cfg(target_os = "macos")]
    return unsafe { ffi::AXIsProcessTrusted() };
    #[cfg(not(target_os = "macos"))]
    true
}

/// Opens the system settings where Ronnie can be allowed in Accessibility (macOS).
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn();
}

#[cfg(windows)]
#[derive(Default)]
struct System;

#[cfg(windows)]
impl System {
    fn idle_seconds(&self) -> Option<f64> {
        use windows_sys::Win32::System::SystemInformation::GetTickCount;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return None;
        }
        Some(unsafe { GetTickCount() }.wrapping_sub(info.dwTime) as f64 / 1000.0)
    }

    fn nudge(&mut self) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput};
        let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 } } };
        unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Default)]
struct System {
    /// X11, once connected (None: tried and failed, Wayland or no X server).
    x11: Option<Option<(x11rb::rust_connection::RustConnection, u32)>>,
}

#[cfg(all(unix, not(target_os = "macos")))]
impl System {
    fn x11(&mut self) -> Option<&(x11rb::rust_connection::RustConnection, u32)> {
        self.x11
            .get_or_insert_with(|| {
                // A Wayland session: its X server (XWayland) sees none of the user's input.
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    return None;
                }
                let (conn, screen) = x11rb::connect(None).ok()?;
                let root = x11rb::connection::Connection::setup(&conn).roots.get(screen)?.root;
                Some((conn, root))
            })
            .as_ref()
    }

    fn idle_seconds(&mut self) -> Option<f64> {
        use x11rb::protocol::screensaver::ConnectionExt;
        let (conn, root) = self.x11()?;
        let info = conn.screensaver_query_info(*root).ok()?.reply().ok()?;
        Some(info.ms_since_user_input as f64 / 1000.0)
    }

    fn nudge(&mut self) {
        if let Some((conn, root)) = self.x11() {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto::MOTION_NOTIFY_EVENT;
            use x11rb::protocol::xtest::ConnectionExt;
            // One pixel aside and back (relative moves: detail 1).
            for dx in [1i16, -1] {
                let _ = conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 1, x11rb::CURRENT_TIME, *root, dx, 0, 0);
            }
            let _ = conn.flush();
            return;
        }
        // Wayland: the screen saver of the desktop (KDE, GNOME...), as video players do.
        for (dest, path) in [("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver"), ("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver")] {
            let _ = std::process::Command::new("gdbus")
                .args(["call", "--session", "--dest", dest, "--object-path", path, "--method", &format!("{dest}.SimulateUserActivity")])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_schedule() {
        let s = KeepActive { enabled: true, ..KeepActive::default() };
        // Monday 8:29, 8:30, 11:59, 12:00, 14:00, 17:59, 18:00; Saturday 10:00.
        assert!(!in_schedule(&s, 0, 8 * 60 + 29));
        assert!(in_schedule(&s, 0, 8 * 60 + 30));
        assert!(in_schedule(&s, 0, 11 * 60 + 59));
        assert!(!in_schedule(&s, 0, 12 * 60));
        assert!(in_schedule(&s, 0, 14 * 60));
        assert!(in_schedule(&s, 4, 17 * 60 + 59));
        assert!(!in_schedule(&s, 4, 18 * 60));
        assert!(!in_schedule(&s, 5, 10 * 60));
    }
}
