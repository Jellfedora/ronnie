//! Whether a saved window position still lands on a connected screen (one unplugged since would leave
//! the window invisible).

use crate::config::WindowState;

/// A rectangle in points, from its top-left corner.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// False only when the screens are known and none shows enough of the window's top (its title bar) to
/// grab it: the position is then better left to the system.
pub fn on_screen(w: &WindowState) -> bool {
    match screens() {
        Some(screens) if !screens.is_empty() => reachable(w, &screens),
        _ => true,
    }
}

fn reachable(w: &WindowState, screens: &[Area]) -> bool {
    let top = Area { x: w.x, y: w.y, width: w.width.max(120.0), height: 30.0 };
    screens.iter().any(|s| {
        let across = (top.x + top.width).min(s.x + s.width) - top.x.max(s.x);
        let down = (top.y + top.height).min(s.y + s.height) - top.y.max(s.y);
        across >= 80.0 && down >= 20.0
    })
}

/// The screens, in the coordinates window positions are saved in (points, y down from the top of the main
/// screen).
#[cfg(target_os = "macos")]
fn screens() -> Option<Vec<Area>> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    // AppKit counts y up from the bottom of the main (first) screen; windows are placed from its top.
    let main_height = screens.iter().next()?.frame().size.height;
    Some(
        screens
            .iter()
            .map(|s| {
                let f = s.frame();
                Area { x: f.origin.x as f32, y: (main_height - f.origin.y - f.size.height) as f32, width: f.size.width as f32, height: f.size.height as f32 }
            })
            .collect(),
    )
}

/// The screens, each twice: as reported, and divided by its scale (in pixels or in points depending on
/// whether the process is DPI-aware yet; either reading is accepted).
#[cfg(windows)]
fn screens() -> Option<Vec<Area>> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> i32 {
        // SAFETY: `data` is the Vec passed below, alive for the whole enumeration; `rect` is given by Windows.
        let (areas, r) = unsafe { (&mut *(data as *mut Vec<Area>), *rect) };
        let raw = Area { x: r.left as f32, y: r.top as f32, width: (r.right - r.left) as f32, height: (r.bottom - r.top) as f32 };
        areas.push(raw);
        let (mut dpi, mut _dpi_y) = (0u32, 0u32);
        // SAFETY: plain query on a monitor handle Windows just gave us.
        if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y) } == 0 && dpi > 0 {
            let k = 96.0 / dpi as f32;
            areas.push(Area { x: raw.x * k, y: raw.y * k, width: raw.width * k, height: raw.height * k });
        }
        1
    }

    let mut areas: Vec<Area> = Vec::new();
    // SAFETY: the callback only writes into `areas`, which outlives the call.
    let ok = unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut areas as *mut _ as LPARAM) };
    (ok != 0).then_some(areas)
}

/// Unknown here: X11 window managers keep windows reachable, and Wayland ignores positions.
#[cfg(not(any(target_os = "macos", windows)))]
fn screens() -> Option<Vec<Area>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(x: f32, y: f32) -> WindowState {
        WindowState { x, y, width: 1100.0, height: 700.0, maximized: false, fullscreen: false }
    }

    #[test]
    fn keeps_windows_whose_title_bar_shows() {
        let screens = [Area { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 }, Area { x: 1440.0, y: -200.0, width: 2560.0, height: 1440.0 }];
        assert!(reachable(&window(100.0, 50.0), &screens));
        assert!(reachable(&window(2000.0, -150.0), &screens));
        // Mostly off the right edge, but the left of its title bar still shows.
        assert!(reachable(&window(3900.0, 100.0), &screens));
    }

    #[test]
    fn drops_windows_left_on_an_unplugged_screen() {
        let screens = [Area { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 }];
        assert!(!reachable(&window(1600.0, 100.0), &screens));
        assert!(!reachable(&window(-1200.0, 100.0), &screens));
        // Title bar above the top of the screen: the window cannot be dragged back.
        assert!(!reachable(&window(100.0, -500.0), &screens));
    }
}
