//! macOS: Teams read and driven through the system's accessibility (Ronnie allowed in Privacy & Security ›
//! Accessibility). The draft is put back through Teams's own "Select All" and "Paste" menu items: no key
//! typed (a key's number is a different letter from one keyboard to another, ⌘ A typing ⌘ Q on a French
//! one).

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, Vec2};
use objc2_foundation::NSString;

use super::common::{Field, Front, MESSAGE_CHARS, debug, is_tool, last_messages, leftmost};

/// Teams's process.
pub(super) type App = i32;
/// An element of Teams's page.
pub(super) type El = Cf;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> *const c_void;
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCopyAttributeValue(element: *const c_void, attribute: *const c_void, value: *mut *const c_void) -> i32;
    fn AXUIElementSetAttributeValue(element: *const c_void, attribute: *const c_void, value: *const c_void) -> i32;
    fn AXUIElementGetPid(element: *const c_void, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, seconds: f32) -> i32;
    fn AXValueGetValue(value: *const c_void, kind: u32, out: *mut c_void) -> bool;
    fn AXValueCreate(kind: u32, value: *const c_void) -> *const c_void;
    fn AXUIElementPerformAction(element: *const c_void, action: *const c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRetain(cf: *const c_void) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFEqual(a: *const c_void, b: *const c_void) -> bool;
    fn CFGetTypeID(cf: *const c_void) -> usize;
    fn CFStringGetTypeID() -> usize;
    static kCFBooleanTrue: *const c_void;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(boolean: *const c_void) -> bool;
    fn CFNumberGetValue(number: *const c_void, kind: isize, out: *mut c_void) -> bool;
}

/// An accessibility element (or another object of Core Foundation's), kept while held.
pub(super) struct Cf(*const c_void);

// SAFETY: accessibility elements and Core Foundation values may be used and released from any thread.
unsafe impl Send for Cf {}
unsafe impl Sync for Cf {}

impl Clone for Cf {
    fn clone(&self) -> Self {
        // SAFETY: a live object, retained once more for the copy.
        unsafe { CFRetain(self.0) };
        Cf(self.0)
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: each Cf owns one retain.
        unsafe { CFRelease(self.0) }
    }
}

impl PartialEq for Cf {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: two live objects.
        unsafe { CFEqual(self.0, other.0) }
    }
}

impl Cf {
    /// An owned object (a "Create" or "Copy" result), None when null.
    fn owned(p: *const c_void) -> Option<Cf> {
        (!p.is_null()).then_some(Cf(p))
    }

    fn app(pid: i32) -> Option<Cf> {
        // SAFETY: plain call, the result owned.
        Cf::owned(unsafe { AXUIElementCreateApplication(pid) })
    }

    fn get(&self, attribute: &str) -> Option<Cf> {
        let name = NSString::from_str(attribute);
        let mut value: *const c_void = std::ptr::null();
        // SAFETY: an element, an NSString (toll-free a CFString), the value owned when given.
        let err = unsafe { AXUIElementCopyAttributeValue(self.0, objc2::rc::Retained::as_ptr(&name).cast(), &mut value) };
        if err != 0 {
            return None;
        }
        Cf::owned(value)
    }

    fn string(&self, attribute: &str) -> Option<String> {
        let value = self.get(attribute)?;
        // SAFETY: a CFString checked, toll-free an NSString.
        unsafe { (CFGetTypeID(value.0) == CFStringGetTypeID()).then(|| (*value.0.cast::<NSString>()).to_string()) }
    }

    /// A point (kind 1) or a size (kind 2).
    fn pair(&self, attribute: &str, kind: u32) -> Option<(f64, f64)> {
        let value = self.get(attribute)?;
        let mut out = [0f64; 2];
        // SAFETY: CGPoint and CGSize are two f64.
        unsafe { AXValueGetValue(value.0, kind, out.as_mut_ptr().cast()) }.then_some((out[0], out[1]))
    }

    /// On the screens, in points from the main screen's top left (as the windows are placed).
    fn rect(&self) -> Option<Rect> {
        let (x, y) = self.pair("AXPosition", 1)?;
        let (w, h) = self.pair("AXSize", 2)?;
        Some(Rect::from_min_size(Pos2::new(x as f32, y as f32), Vec2::new(w as f32, h as f32)))
    }

    /// A list of texts (the page's classes of an element).
    fn strings(&self, attribute: &str) -> Vec<String> {
        let Some(list) = self.get(attribute) else { return Vec::new() };
        // SAFETY: a CFArray checked, each value a CFString checked (toll-free an NSString).
        unsafe {
            if CFGetTypeID(list.0) != CFArrayGetTypeID() {
                return Vec::new();
            }
            (0..CFArrayGetCount(list.0))
                .map(|i| CFArrayGetValueAtIndex(list.0, i))
                .filter(|v| !v.is_null() && CFGetTypeID(*v) == CFStringGetTypeID())
                .map(|v| (*v.cast::<NSString>()).to_string())
                .collect()
        }
    }

    fn children(&self) -> Vec<Cf> {
        let Some(list) = self.get("AXChildren") else { return Vec::new() };
        // SAFETY: a CFArray checked; each value retained for its Cf.
        unsafe {
            if CFGetTypeID(list.0) != CFArrayGetTypeID() {
                return Vec::new();
            }
            (0..CFArrayGetCount(list.0)).filter_map(|i| Cf::owned(CFRetain(CFArrayGetValueAtIndex(list.0, i)))).collect()
        }
    }

    /// A number, or a boolean as 0 or 1.
    fn number(&self, attribute: &str) -> Option<i64> {
        let value = self.get(attribute)?;
        // SAFETY: the type checked before reading.
        unsafe {
            let kind = CFGetTypeID(value.0);
            if kind == CFBooleanGetTypeID() {
                return Some(CFBooleanGetValue(value.0) as i64);
            }
            let mut out = 0i64;
            // kCFNumberSInt64Type.
            (kind == CFNumberGetTypeID() && CFNumberGetValue(value.0, 4, (&mut out as *mut i64).cast())).then_some(out)
        }
    }

    fn press(&self) -> bool {
        let action = NSString::from_str("AXPress");
        // SAFETY: an element, a CFString.
        unsafe { AXUIElementPerformAction(self.0, objc2::rc::Retained::as_ptr(&action).cast()) == 0 }
    }

    fn set(&self, attribute: &str, value: *const c_void) -> bool {
        let name = NSString::from_str(attribute);
        // SAFETY: an element, a CFString, a live value.
        unsafe { AXUIElementSetAttributeValue(self.0, objc2::rc::Retained::as_ptr(&name).cast(), value) == 0 }
    }

    fn set_string(&self, attribute: &str, text: &str) -> bool {
        let text = NSString::from_str(text);
        self.set(attribute, objc2::rc::Retained::as_ptr(&text).cast())
    }

    /// A range of text (UTF-16 units).
    fn set_range(&self, attribute: &str, location: usize, length: usize) -> bool {
        let range = [location as isize, length as isize];
        // SAFETY: a CFRange is two CFIndex; kAXValueCFRangeType.
        let Some(value) = Cf::owned(unsafe { AXValueCreate(4, range.as_ptr().cast()) }) else { return false };
        self.set(attribute, value.0)
    }

    fn pid(&self) -> Option<i32> {
        let mut pid = 0;
        // SAFETY: an element, an out integer.
        (unsafe { AXUIElementGetPid(self.0, &mut pid) } == 0).then_some(pid)
    }

    fn set_true(&self, attribute: &str) -> bool {
        let name = NSString::from_str(attribute);
        // SAFETY: an element, a CFString, a CFBoolean.
        unsafe { AXUIElementSetAttributeValue(self.0, objc2::rc::Retained::as_ptr(&name).cast(), kCFBooleanTrue) == 0 }
    }
}

fn is_teams(pid: i32) -> bool {
    objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid).and_then(|app| app.bundleIdentifier()).is_some_and(|id| id.to_string().starts_with("com.microsoft.teams"))
}

/// What is in front and Teams's message box, looked at again and again (by the watching thread).
pub(super) struct Watcher {
    system: Cf,
    me: i32,
    /// Whether each process seen in front is Teams.
    teams: HashMap<i32, bool>,
    /// Teams's message box last found.
    last: Option<Cf>,
    /// When it was last looked for through Teams's window (no box known).
    searched: Option<Instant>,
    /// The process in front at the last look.
    pid: i32,
}

impl Watcher {
    pub(super) fn new() -> Option<Watcher> {
        // SAFETY: plain call, the result owned.
        let system = Cf::owned(unsafe { AXUIElementCreateSystemWide() })?;
        // SAFETY: the system-wide element: every call's time limit.
        unsafe { AXUIElementSetMessagingTimeout(system.0, 0.5) };
        Some(Watcher { system, me: std::process::id() as i32, teams: HashMap::new(), last: None, searched: None, pid: 0 })
    }

    /// The process in front: the one with the keyboard's focus, through the accessibility; else (ours: the
    /// accessibility can't ask our own process from here) the workspace's, which can be late.
    fn front_pid(&self) -> Option<i32> {
        self.system.get("AXFocusedApplication").and_then(|app| app.pid()).or_else(|| objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier()))
    }

    pub(super) fn look(&mut self) -> Front<App, El> {
        let Some(pid) = self.front_pid() else { return Front::Other };
        self.pid = pid;
        if pid == self.me {
            return Front::Ours;
        }
        let Some(app) = Cf::app(pid) else { return Front::Other };
        if !*self.teams.entry(pid).or_insert_with(|| is_teams(pid)) {
            return Front::Other;
        }
        let is_box = |el: &Cf| el.string("AXRole").as_deref() == Some("AXTextArea");
        // The box with the focus; else the last one found, while it is still there; else, once a second,
        // the conversation's box looked for in the window (shown before it is clicked in).
        if let Some(el) = app.get("AXFocusedUIElement").filter(is_box) {
            self.last = Some(el);
        }
        let shown = |el: &Cf| el.rect().filter(|r| r.width() > 40.0 && r.height() > 8.0);
        if self.last.as_ref().is_none_or(|el| !is_box(el) || shown(el).is_none()) {
            self.last = None;
            if self.searched.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                self.searched = Some(Instant::now());
                self.last = find_box(&app);
            }
        }
        let field = self.last.as_ref().and_then(|el| {
            let rect = shown(el)?;
            Some(Field { el: el.clone(), rect, tool: first_tool(el, rect) })
        });
        Front::Teams { app: pid, field }
    }

    /// What was in front at the last look, for `RONNIE_TEAMS_DEBUG`.
    pub(super) fn who(&self) -> String {
        let app = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(self.pid).and_then(|a| a.bundleIdentifier()).map(|b| b.to_string()).unwrap_or_default();
        format!("process {} {app}", self.pid)
    }
}

/// Logged when Ronnie isn't allowed in Accessibility (nothing can be read).
pub(super) fn prepare() {
    // SAFETY: plain call.
    if !unsafe { AXIsProcessTrusted() } {
        crate::log::info("teams claude: Ronnie isn't allowed in Accessibility");
    }
}

/// The draft in the box.
pub(super) fn draft(el: &El) -> String {
    el.string("AXValue").unwrap_or_default()
}

/// The conversation's message box in Teams's window: the text area named "new-message-…" in the page
/// (whatever the user's language).
fn find_box(app: &Cf) -> Option<Cf> {
    let window = app.get("AXFocusedWindow")?;
    find(&window, &|el| el.string("AXRole").as_deref() == Some("AXTextArea") && el.string("AXDOMIdentifier").is_some_and(|id| id.starts_with("new-message")))
}

/// The first element under `el` (itself included) that `wanted` takes, depth first.
fn find(el: &Cf, wanted: &dyn Fn(&Cf) -> bool) -> Option<Cf> {
    fn walk(el: &Cf, wanted: &dyn Fn(&Cf) -> bool, depth: u32, seen: &mut u32) -> Option<Cf> {
        *seen += 1;
        if depth > 45 || *seen > 5000 {
            return None;
        }
        if wanted(el) {
            return Some(el.clone());
        }
        el.children().iter().find_map(|child| walk(child, wanted, depth + 1, seen))
    }
    walk(el, wanted, 0, &mut 0)
}

/// What an element says: its description, else the texts under it put together.
fn text_of(el: &Cf) -> String {
    fn texts(el: &Cf, depth: u32, out: &mut Vec<String>) {
        if depth > 8 {
            return;
        }
        if el.string("AXRole").as_deref() == Some("AXStaticText")
            && let Some(v) = el.string("AXValue").filter(|v| !v.trim().is_empty())
        {
            out.push(v.trim().to_owned());
        }
        for child in el.children() {
            texts(&child, depth + 1, out);
        }
    }
    if let Some(d) = el.string("AXDescription").filter(|d| !d.trim().is_empty()) {
        return d.trim().to_owned();
    }
    let mut out = Vec::new();
    texts(el, 0, &mut out);
    out.join(" ")
}

/// The messages of the conversation shown in Teams (those its page holds: the last ones), the oldest
/// first: who wrote it (None: the user), and what.
pub(super) fn conversation(pid: &App) -> Vec<(Option<String>, String)> {
    let Some(window) = Cf::app(*pid).and_then(|app| app.get("AXFocusedWindow")) else { return Vec::new() };
    let Some(list) = find(&window, &|el| el.string("AXDOMIdentifier").as_deref() == Some("chat-pane-list")) else { return Vec::new() };
    fn walk(el: &Cf, depth: u32, author: &mut Option<String>, out: &mut Vec<(Option<String>, String)>) {
        if depth > 10 {
            return;
        }
        let id = el.string("AXDOMIdentifier").unwrap_or_default();
        if id.starts_with("author-") {
            *author = Some(text_of(el)).filter(|a| !a.is_empty());
            return;
        }
        if let Some(key) = id.strip_prefix("message-body-") {
            let mine = el.strings("AXDOMClassList").iter().any(|c| c.contains("ChatMyMessage"));
            let content = find(el, &|c| c.string("AXDOMIdentifier").is_some_and(|i| i == format!("content-{key}")));
            let text: String = content.as_ref().map_or_else(|| text_of(el), text_of).chars().take(MESSAGE_CHARS).collect();
            if !text.is_empty() {
                out.push(((!mine).then(|| author.clone().unwrap_or_else(|| "?".into())), text));
            }
            return;
        }
        for child in el.children() {
            walk(&child, depth + 1, author, out);
        }
    }
    let mut out = Vec::new();
    walk(&list, 0, &mut None, &mut out);
    last_messages(out)
}

/// The leftmost of the buttons in the row under the box (Teams's formatting one, whatever its name in
/// the user's language): looked for in the box's nearest container that has them.
fn first_tool(field: &Cf, rect: Rect) -> Option<Rect> {
    fn buttons(el: &Cf, rect: Rect, depth: u32, out: &mut Vec<Rect>) {
        if depth > 8 {
            return;
        }
        for child in el.children() {
            let role = child.string("AXRole").unwrap_or_default();
            if matches!(role.as_str(), "AXButton" | "AXPopUpButton" | "AXMenuButton") {
                if let Some(r) = child.rect().filter(|r| is_tool(*r, rect)) {
                    out.push(r);
                }
            } else {
                buttons(&child, rect, depth + 1, out);
            }
        }
    }
    let mut el = field.clone();
    for _ in 0..4 {
        el = el.get("AXParent")?;
        let mut found = Vec::new();
        buttons(&el, rect, 0, &mut found);
        if let Some(first) = leftmost(found) {
            return Some(first);
        }
    }
    None
}

/// The item of the app's menus whose shortcut is ⌘ and `key` alone (whatever the menus' language), when
/// it can be chosen now.
fn menu_item(app: &Cf, key: &str) -> Option<Cf> {
    let bar = app.get("AXMenuBar")?;
    for top in bar.children() {
        for menu in top.children() {
            for item in menu.children() {
                if item.string("AXMenuItemCmdChar").as_deref() == Some(key) && item.number("AXMenuItemCmdModifiers").unwrap_or(0) == 0 && item.number("AXEnabled") != Some(0) {
                    return Some(item);
                }
            }
        }
    }
    None
}

/// `text` in place of the draft in Teams's box: Teams brought to the front, the box focused, then
/// Teams's own "Select All" and "Paste" chosen in its menus; the clipboard's text given back after.
/// Without those menus, the selection and the text set through the box's accessibility.
pub(super) fn paste_into(pid: App, field: El, text: String) {
    let Some(app) = Cf::app(pid) else { return };
    app.set_true("AXFrontmost");
    std::thread::sleep(Duration::from_millis(250));
    field.set_true("AXFocused");
    std::thread::sleep(Duration::from_millis(100));
    let selected = match menu_item(&app, "A") {
        Some(item) => item.press(),
        None => false,
    } || {
        let length = field.string("AXValue").map_or(0, |v| v.encode_utf16().count());
        field.set_range("AXSelectedTextRange", 0, length)
    };
    if !selected {
        debug("the draft couldn't be selected");
        return;
    }
    std::thread::sleep(Duration::from_millis(80));
    if let Some(paste) = menu_item(&app, "V") {
        let mut clipboard = arboard::Clipboard::new().ok();
        let before = clipboard.as_mut().and_then(|c| c.get_text().ok());
        if clipboard.as_mut().is_some_and(|c| c.set_text(text.clone()).is_ok()) {
            let pasted = paste.press();
            debug(if pasted { "pasted through Teams's menu" } else { "Teams's Paste refused" });
            std::thread::sleep(Duration::from_millis(600));
            if let (Some(c), Some(before)) = (clipboard.as_mut(), before) {
                let _ = c.set_text(before);
            }
            if pasted {
                return;
            }
        }
    }
    let done = field.set_string("AXSelectedText", &text);
    debug(if done { "set through the accessibility" } else { "the draft couldn't be replaced" });
}

/// Teams in front again.
pub(super) fn back_to(pid: &App) {
    if let Some(app) = Cf::app(*pid) {
        app.set_true("AXFrontmost");
    }
}

/// The screen showing `at` (points, from the main screen's top left), on the main thread.
pub(super) fn screen_of(at: Pos2) -> Option<Rect> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    let height = screens.firstObject()?.frame().size.height;
    screens.iter().map(|s| s.frame()).map(|r| Rect::from_min_size(Pos2::new(r.origin.x as f32, (height - r.origin.y - r.size.height) as f32), Vec2::new(r.size.width as f32, r.size.height as f32))).find(|r| r.contains(at))
}

/// Our window is the active one (asked on the main thread): whatever the watching thread saw last.
pub(super) fn active() -> bool {
    objc2::MainThreadMarker::new().is_some_and(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).isActive())
}

/// Out of the Dock and of ⌘ Tab.
pub(super) fn options(options: &mut eframe::NativeOptions) {
    options.event_loop_builder = Some(Box::new(|builder| {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }));
}

/// Over the full-screen windows too, on every desktop, out of the window cycle; never put to sleep by
/// App Nap.
pub(super) fn tune(cc: &eframe::CreationContext<'_>) {
    use objc2_app_kit::NSWindowCollectionBehavior as B;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = cc.window_handle()
        && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
    {
        // SAFETY: the handle points to the live NSView of our window, on the main thread.
        let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };
        if let Some(window) = view.window() {
            window.setCollectionBehavior(B::CanJoinAllSpaces | B::FullScreenAuxiliary | B::IgnoresCycle);
        }
    }
    let info = objc2_foundation::NSProcessInfo::processInfo();
    let reason = NSString::from_str("Ronnie's Claude button over Teams");
    let activity = info.beginActivityWithOptions_reason(objc2_foundation::NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason);
    // Kept for the process's life.
    std::mem::forget(activity);
}
