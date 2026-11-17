//! Windows: the new Teams (a WebView2 page) read through UI Automation, whose elements carry the page's
//! ids as their AutomationId ("new-message-…", "chat-pane-list"…) and its classes in their ClassName.
//!
//! The places: UI Automation gives pixels (the process is made aware of each monitor's scale at start,
//! as winit would), turned into points by the scale of the monitor they are on, as the desktop effects
//! do: the overlay's window is placed in points.
//!
//! The draft is put back by pasting: Teams's window brought to the front, the box focused, Ctrl A then
//! Ctrl V sent (virtual keys: Windows maps them to the keyboard's layout, A is A on any keyboard), the
//! clipboard's text given back after; else the box's value set through UI Automation.
//!
//! COM: every thread using UI Automation joins the multithreaded apartment and has its own
//! `IUIAutomation` (`uia`).

use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

use egui::{Pos2, Rect};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationValuePattern, PropertyConditionFlags, PropertyConditionFlags_MatchSubstring, PropertyConditionFlags_None, TreeScope_Children,
    TreeScope_Descendants, UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::core::BSTR;
use windows_sys::Win32::Foundation::{CloseHandle, POINT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CONTROL};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow};

use super::common::{Field, Front, MESSAGE_CHARS, debug, is_tool, last_messages, leftmost};

/// Teams's window and its process.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct App {
    window: isize,
    pid: u32,
}

/// An element of Teams's page.
#[derive(Clone)]
pub(super) struct El(IUIAutomationElement);

// SAFETY: UI Automation's client objects are free-threaded; the threads calling into them have joined the
// multithreaded apartment (`uia`), the panel's thread only keeps, clones and drops them.
unsafe impl Send for El {}
unsafe impl Sync for El {}

impl PartialEq for El {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: two live elements.
        uia().is_some_and(|a| unsafe { a.CompareElements(&self.0, &other.0) }.is_ok_and(|same| same.as_bool()))
    }
}

thread_local! {
    /// This thread's UI Automation, the thread in the multithreaded apartment first.
    static UIA: Option<IUIAutomation> = {
        // SAFETY: plain COM calls; an apartment already joined (the window's thread) is kept as it is.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
        }
    };
}

fn uia() -> Option<IUIAutomation> {
    UIA.with(|u| u.clone())
}

/// A text as a VARIANT (given back to `free`).
fn text_variant(text: &str) -> VARIANT {
    let mut v = VARIANT::default();
    // SAFETY: the VARIANT's own fields, its type set with the value.
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_BSTR;
        inner.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(text));
    }
    v
}

fn number_variant(n: i32) -> VARIANT {
    let mut v = VARIANT::default();
    // SAFETY: the VARIANT's own fields, its type set with the value.
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = n;
    }
    v
}

/// A VARIANT's text given back.
fn free(mut v: VARIANT) {
    // SAFETY: a text only when its type says so, dropped once.
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        if inner.vt == VT_BSTR {
            ManuallyDrop::drop(&mut inner.Anonymous.bstrVal);
        }
    }
}

/// The elements whose page id is `id`, or holds it (`part`).
fn id_condition(a: &IUIAutomation, id: &str, part: bool) -> Option<IUIAutomationCondition> {
    let v = text_variant(id);
    let flags: PropertyConditionFlags = if part { PropertyConditionFlags_MatchSubstring } else { PropertyConditionFlags_None };
    // SAFETY: a live automation, a VARIANT alive during the call (copied by it).
    let condition = unsafe { a.CreatePropertyConditionEx(UIA_AutomationIdPropertyId, &v, flags) }.ok();
    free(v);
    condition
}

fn text(r: windows::core::Result<BSTR>) -> String {
    r.map(|b| b.to_string()).unwrap_or_default()
}

/// Pixels to points: over the scale of the monitor at `p`.
fn scale_at(p: POINT) -> f32 {
    // SAFETY: plain Win32 calls.
    unsafe {
        let m = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if !m.is_null() && GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) == 0 { dx as f32 / 96.0 } else { 1.0 }
    }
}

impl El {
    fn id(&self) -> String {
        // SAFETY: a live element.
        text(unsafe { self.0.CurrentAutomationId() })
    }

    fn class(&self) -> String {
        // SAFETY: a live element.
        text(unsafe { self.0.CurrentClassName() })
    }

    fn name(&self) -> String {
        // SAFETY: a live element.
        text(unsafe { self.0.CurrentName() })
    }

    /// On the screens, in points.
    fn rect(&self) -> Option<Rect> {
        // SAFETY: a live element.
        let r = unsafe { self.0.CurrentBoundingRectangle() }.ok()?;
        if r.right <= r.left || r.bottom <= r.top {
            return None;
        }
        let s = scale_at(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 });
        Some(Rect::from_min_max(Pos2::new(r.left as f32 / s, r.top as f32 / s), Pos2::new(r.right as f32 / s, r.bottom as f32 / s)))
    }

    fn first(&self, condition: &IUIAutomationCondition) -> Option<El> {
        // SAFETY: a live element and condition.
        unsafe { self.0.FindFirst(TreeScope_Descendants, condition) }.ok().map(El)
    }

    fn all(&self, children_only: bool, condition: &IUIAutomationCondition) -> Vec<El> {
        let scope = if children_only { TreeScope_Children } else { TreeScope_Descendants };
        // SAFETY: a live element and condition; the array read within its length.
        unsafe {
            let Ok(found) = self.0.FindAll(scope, condition) else { return Vec::new() };
            (0..found.Length().unwrap_or(0)).filter_map(|i| found.GetElement(i).ok().map(El)).collect()
        }
    }
}

/// The conversation's message box in Teams's window `root`: the element named "new-message-…" in the
/// page (whatever the user's language).
fn find_box(a: &IUIAutomation, root: &El) -> Option<El> {
    root.first(&id_condition(a, "new-message", true)?)
}

/// The leftmost of the buttons of the box's tools (Teams's formatting one): looked for in the box's
/// nearest container that has them.
fn first_tool(a: &IUIAutomation, el: &El, rect: Rect) -> Option<Rect> {
    let v = number_variant(UIA_ButtonControlTypeId.0);
    // SAFETY: a live automation, a VARIANT alive during the call.
    let buttons = unsafe { a.CreatePropertyCondition(UIA_ControlTypePropertyId, &v) }.ok();
    free(v);
    let buttons = buttons?;
    // SAFETY: a live automation.
    let walker = unsafe { a.ControlViewWalker() }.ok()?;
    let mut container = el.0.clone();
    for _ in 0..4 {
        // SAFETY: a live walker and element.
        container = unsafe { walker.GetParentElement(&container) }.ok()?;
        let found: Vec<Rect> = El(container.clone()).all(false, &buttons).iter().filter_map(El::rect).filter(|r| is_tool(*r, rect)).collect();
        if let Some(first) = leftmost(found) {
            return Some(first);
        }
    }
    None
}

/// What an element says: its name, else the names under it put together.
fn text_of(a: &IUIAutomation, el: &El) -> String {
    fn texts(el: &El, every: &IUIAutomationCondition, depth: u32, out: &mut Vec<String>) {
        if depth > 8 {
            return;
        }
        for child in el.all(true, every) {
            let name = child.name();
            if !name.trim().is_empty() {
                out.push(name.trim().to_owned());
            } else {
                texts(&child, every, depth + 1, out);
            }
        }
    }
    let name = el.name();
    if !name.trim().is_empty() {
        return name.trim().to_owned();
    }
    // SAFETY: a live automation.
    let Ok(every) = (unsafe { a.CreateTrueCondition() }) else { return String::new() };
    let mut out = Vec::new();
    texts(el, &every, 0, &mut out);
    out.join(" ")
}

fn process_name(pid: u32) -> String {
    // SAFETY: a handle opened, read into a buffer of its size, closed.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return String::new();
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) != 0;
        CloseHandle(handle);
        if ok { String::from_utf16_lossy(&buffer[..size as usize]) } else { String::new() }
    }
}

fn title(window: isize) -> String {
    let mut buffer = [0u16; 512];
    // SAFETY: a buffer of its length.
    let n = unsafe { GetWindowTextW(window as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}

/// The window in front, and its process.
fn foreground() -> Option<(isize, u32)> {
    // SAFETY: plain Win32 calls.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        Some((window as isize, pid))
    }
}

/// What is in front and Teams's message box, looked at again and again (by the watching thread).
pub(super) struct Watcher {
    uia: IUIAutomation,
    me: u32,
    /// Whether each process seen in front is Teams (ms-teams.exe).
    teams: HashMap<u32, bool>,
    /// Teams's message box last found, where it was and its first tool there.
    last: Option<(El, Rect, Option<Rect>)>,
    /// When it was last looked for (no box known).
    searched: Option<Instant>,
    window: isize,
    pid: u32,
}

impl Watcher {
    pub(super) fn new() -> Option<Watcher> {
        Some(Watcher { uia: uia()?, me: std::process::id(), teams: HashMap::new(), last: None, searched: None, window: 0, pid: 0 })
    }

    pub(super) fn look(&mut self) -> Front<App, El> {
        let Some((window, pid)) = foreground() else { return Front::Other };
        (self.window, self.pid) = (window, pid);
        if pid == self.me {
            return Front::Ours;
        }
        let teams = *self.teams.entry(pid).or_insert_with(|| process_name(pid).to_lowercase().ends_with("ms-teams.exe"));
        if !teams && !title(window).to_lowercase().contains("microsoft teams") {
            return Front::Other;
        }
        let app = App { window, pid };
        // The box last found while it is still there; else, once a second, looked for in the window.
        let rect = self.last.as_ref().and_then(|(el, _, _)| el.rect()).filter(|r| r.width() > 40.0 && r.height() > 8.0);
        let Some(rect) = rect else {
            self.last = None;
            if self.searched.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                self.searched = Some(Instant::now());
                // SAFETY: a live automation, the window in front.
                let root = unsafe { self.uia.ElementFromHandle(windows::Win32::Foundation::HWND(window as _)) }.ok().map(El);
                self.last = root.and_then(|root| find_box(&self.uia, &root)).and_then(|el| {
                    let rect = el.rect()?;
                    let tool = first_tool(&self.uia, &el, rect);
                    Some((el, rect, tool))
                });
            }
            return Front::Teams { app, field: self.last.as_ref().map(|(el, rect, tool)| Field { el: el.clone(), rect: *rect, tool: *tool }) };
        };
        // Moved or grown: its tools looked for again.
        if let Some((el, at, tool)) = self.last.as_mut()
            && *at != rect
        {
            *at = rect;
            *tool = first_tool(&self.uia, el, rect);
        }
        let field = self.last.as_ref().map(|(el, rect, tool)| Field { el: el.clone(), rect: *rect, tool: *tool });
        Front::Teams { app, field }
    }

    /// What was in front at the last look, for `RONNIE_TEAMS_DEBUG`.
    pub(super) fn who(&self) -> String {
        format!("window {:#x}, process {}", self.window, self.pid)
    }
}

/// Aware of each monitor's scale before any thread asks UI Automation for places (else they would be
/// scaled for a process unaware of it); winit asks the same of the process afterwards.
pub(super) fn prepare() {
    // SAFETY: plain Win32 call.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// The draft in the box: its text, else its value, else its name.
pub(super) fn draft(el: &El) -> String {
    // SAFETY: a live element and the patterns it gives.
    unsafe {
        if let Ok(pattern) = el.0.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            && let Ok(range) = pattern.DocumentRange()
            && let Ok(t) = range.GetText(-1)
        {
            return t.to_string();
        }
        if let Ok(pattern) = el.0.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            && let Ok(v) = pattern.CurrentValue()
        {
            return v.to_string();
        }
    }
    el.name()
}

/// The messages of the conversation shown in Teams (those its page holds: the last ones), the oldest
/// first: who wrote it (None: the user), and what.
pub(super) fn conversation(app: &App) -> Vec<(Option<String>, String)> {
    let Some(a) = uia() else { return Vec::new() };
    // SAFETY: a live automation, Teams's window.
    let Some(root) = (unsafe { a.ElementFromHandle(windows::Win32::Foundation::HWND(app.window as _)) }).ok().map(El) else { return Vec::new() };
    let Some(list) = id_condition(&a, "chat-pane-list", false).and_then(|c| root.first(&c)) else { return Vec::new() };
    let (Some(bodies), Some(authors)) = (id_condition(&a, "message-body-", true), id_condition(&a, "author-", true)) else { return Vec::new() };
    // SAFETY: a live automation and conditions.
    let Ok(either) = (unsafe { a.CreateOrCondition(&bodies, &authors) }) else { return Vec::new() };
    let mut out = Vec::new();
    let mut author: Option<String> = None;
    // In the page's order.
    for el in list.all(false, &either) {
        let id = el.id();
        if id.starts_with("author-") {
            author = Some(text_of(&a, &el)).filter(|t| !t.is_empty());
        } else if let Some(key) = id.strip_prefix("message-body-") {
            let mine = el.class().contains("ChatMyMessage");
            let content = id_condition(&a, &format!("content-{key}"), false).and_then(|c| el.first(&c));
            let text: String = content.map_or_else(|| text_of(&a, &el), |c| text_of(&a, &c)).chars().take(MESSAGE_CHARS).collect();
            if !text.is_empty() {
                out.push(((!mine).then(|| author.clone().unwrap_or_else(|| "?".into())), text));
            }
        }
    }
    last_messages(out)
}

/// Ctrl and `key` (a virtual key: A is 0x41, V 0x56), sent to the window in front.
fn control(key: u16) -> bool {
    let input = |vk: u16, up: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } } };
    let inputs = [input(VK_CONTROL, false), input(key, false), input(key, true), input(VK_CONTROL, true)];
    // SAFETY: an array of its length, of INPUTs.
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    sent == inputs.len() as u32
}

/// `text` in place of the draft in Teams's box: Teams's window brought to the front, the box focused,
/// Ctrl A and Ctrl V sent; the clipboard's text given back after. Else the box's value set.
pub(super) fn paste_into(app: App, el: El, text: String) {
    // This thread in the multithreaded apartment before the element is used.
    let _ = uia();
    // SAFETY: plain Win32 call (our process is in front: the user just clicked in the panel).
    unsafe { SetForegroundWindow(app.window as _) };
    std::thread::sleep(Duration::from_millis(250));
    // SAFETY: a live element.
    let _ = unsafe { el.0.SetFocus() };
    std::thread::sleep(Duration::from_millis(100));
    let mut clipboard = arboard::Clipboard::new().ok();
    let before = clipboard.as_mut().and_then(|c| c.get_text().ok());
    let copied = clipboard.as_mut().is_some_and(|c| c.set_text(text.clone()).is_ok());
    if copied && control(0x41) {
        std::thread::sleep(Duration::from_millis(60));
        if control(0x56) {
            debug("pasted with Ctrl V");
            std::thread::sleep(Duration::from_millis(600));
            if let (Some(c), Some(before)) = (clipboard.as_mut(), before) {
                let _ = c.set_text(before);
            }
            return;
        }
    }
    // SAFETY: a live element and the pattern it gives.
    let done = unsafe { el.0.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).and_then(|p| p.SetValue(&BSTR::from(text.as_str()))) }.is_ok();
    debug(if done { "set through UI Automation" } else { "the draft couldn't be replaced" });
}

/// Teams's window in front again.
pub(super) fn back_to(app: &App) {
    // SAFETY: plain Win32 call.
    unsafe { SetForegroundWindow(app.window as _) };
}

/// The work area of the screen showing `at` (points), in points.
pub(super) fn screen_of(at: Pos2) -> Option<Rect> {
    fn work(m: HMONITOR) -> Option<Rect> {
        // SAFETY: plain Win32 call on a struct owned here.
        unsafe {
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if m.is_null() || GetMonitorInfoW(m, &mut info) == 0 {
                return None;
            }
            let r = info.rcWork;
            Some(Rect::from_min_max(Pos2::new(r.left as f32, r.top as f32), Pos2::new(r.right as f32, r.bottom as f32)))
        }
    }
    // Back to pixels: the screens' scales tried until one holds the point.
    for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0] {
        let p = POINT { x: (at.x * scale) as i32, y: (at.y * scale) as i32 };
        // SAFETY: plain Win32 call.
        let m = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONULL) };
        if (scale_at(p) - scale).abs() < 0.01
            && let Some(r) = work(m)
        {
            return Some(Rect::from_min_max((r.min.to_vec2() / scale).to_pos2(), (r.max.to_vec2() / scale).to_pos2()));
        }
    }
    None
}

/// Our window in front (asked here, on the panel's thread).
pub(super) fn active() -> bool {
    foreground().is_some_and(|(_, pid)| pid == std::process::id())
}
