//! Linux, under X11: Teams in its web app (a window of Chrome or Edge), read through AT-SPI (the
//! accessibility's bus, through D-Bus), the window in front and the keys through X11.
//!
//! Chromium only fills its page's accessibility for a reader: the accessibility is turned on at start
//! (`org.a11y.Status.IsEnabled`); a Teams started before may have to be started again.
//!
//! The draft is put back by pasting: the box focused, Ctrl A then Ctrl V typed through XTest (the keys
//! found by their letter in the keyboard's layout, not by their place), the clipboard's text given back
//! after; else the box's text set through AT-SPI.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, Vec2};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use super::common::{Field, Front, MESSAGE_CHARS, debug, is_tool, last_messages, leftmost};

/// Teams's window: its X11 window, its process (the browser's), its title then.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct App {
    window: Window,
    pid: u32,
    title: String,
}

/// An object of AT-SPI: who holds it on the bus, and where.
#[derive(Clone, PartialEq, Debug)]
struct Node {
    bus: String,
    path: OwnedObjectPath,
}

/// An element of Teams's page, with its containers (the nearest first, a few), where its tools are
/// looked for.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct El {
    node: Node,
    above: Vec<Node>,
}

const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
/// AT-SPI's states (`AtspiStateType`): the window active.
const STATE_ACTIVE: u32 = 1;

/// The accessibility's bus, reached once (its address asked on the session's bus).
fn bus() -> Option<&'static Connection> {
    static BUS: OnceLock<Option<Connection>> = OnceLock::new();
    BUS.get_or_init(|| {
        let session = Connection::session().ok()?;
        let reply = session.call_method(Some("org.a11y.Bus"), "/org/a11y/bus", Some("org.a11y.Bus"), "GetAddress", &()).ok()?;
        let address: String = reply.body().deserialize().ok()?;
        let built = zbus::blocking::connection::Builder::address(address.as_str()).ok()?.method_timeout(Duration::from_millis(800)).build();
        if let Err(e) = &built {
            debug(&format!("the accessibility's bus can't be reached: {e}"));
        }
        built.ok()
    })
    .as_ref()
}

impl Node {
    fn new(bus: &str, path: &str) -> Option<Node> {
        Some(Node { bus: bus.to_owned(), path: OwnedObjectPath::try_from(path).ok()? })
    }

    fn call<B, R>(&self, interface: &str, method: &str, body: &B) -> Option<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
    {
        let reply = bus()?.call_method(Some(self.bus.as_str()), self.path.as_str(), Some(interface), method, body).ok()?;
        reply.body().deserialize().ok()
    }

    fn property(&self, interface: &str, name: &str) -> Option<OwnedValue> {
        self.call("org.freedesktop.DBus.Properties", "Get", &(interface, name))
    }

    fn children(&self) -> Vec<Node> {
        let list: Vec<(String, OwnedObjectPath)> = self.call(ACCESSIBLE, "GetChildren", &()).unwrap_or_default();
        list.into_iter().filter(|(_, path)| path.as_str() != "/org/a11y/atspi/null").map(|(bus, path)| Node { bus, path }).collect()
    }

    fn attributes(&self) -> HashMap<String, String> {
        self.call(ACCESSIBLE, "GetAttributes", &()).unwrap_or_default()
    }

    fn role(&self) -> String {
        self.call(ACCESSIBLE, "GetRoleName", &()).unwrap_or_default()
    }

    fn has_state(&self, state: u32) -> bool {
        let states: Vec<u32> = self.call(ACCESSIBLE, "GetState", &()).unwrap_or_default();
        states.get((state / 32) as usize).is_some_and(|word| word & (1 << (state % 32)) != 0)
    }

    fn name(&self) -> String {
        self.property(ACCESSIBLE, "Name").and_then(|v| String::try_from(v).ok()).unwrap_or_default()
    }

    fn description(&self) -> String {
        self.property(ACCESSIBLE, "Description").and_then(|v| String::try_from(v).ok()).unwrap_or_default()
    }

    /// The page's id of the element ("new-message-…").
    fn id(&self) -> String {
        self.attributes().remove("id").unwrap_or_default()
    }

    /// On the screens, in pixels.
    fn rect(&self) -> Option<Rect> {
        // ATSPI_COORD_TYPE_SCREEN.
        let (x, y, w, h): (i32, i32, i32, i32) = self.call("org.a11y.atspi.Component", "GetExtents", &(0u32,))?;
        (w > 0 && h > 0).then(|| Rect::from_min_size(Pos2::new(x as f32, y as f32), Vec2::new(w as f32, h as f32)))
    }

    fn grab_focus(&self) -> bool {
        self.call("org.a11y.atspi.Component", "GrabFocus", &()).unwrap_or(false)
    }

    fn text(&self) -> Option<String> {
        self.call("org.a11y.atspi.Text", "GetText", &(0i32, -1i32))
    }

    fn set_text(&self, text: &str) -> bool {
        self.call("org.a11y.atspi.EditableText", "SetTextContents", &(text,)).unwrap_or(false)
    }
}

/// The application (its root) of process `pid` on the accessibility's bus.
fn app_root(pid: u32) -> Option<Node> {
    let registry = Node::new("org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root")?;
    let dbus = Node::new("org.freedesktop.DBus", "/org/freedesktop/DBus")?;
    registry.children().into_iter().find(|app| dbus.call::<_, u32>("org.freedesktop.DBus", "GetConnectionUnixProcessID", &(app.bus.as_str(),)) == Some(pid))
}

/// Teams's window among the application's: the one of that title, else the active one.
fn frame(root: &Node, title: &str) -> Option<Node> {
    let frames = root.children();
    frames.iter().find(|f| !title.is_empty() && f.name() == title).or_else(|| frames.iter().find(|f| f.has_state(STATE_ACTIVE))).cloned()
}

/// The first element under `node` (itself included) that `wanted` takes, depth first, the last children
/// first (the message box is at the page's end), with its containers.
fn find(node: &Node, last_first: bool, wanted: &dyn Fn(&Node) -> bool) -> Option<El> {
    fn walk(node: &Node, last_first: bool, wanted: &dyn Fn(&Node) -> bool, path: &mut Vec<Node>, seen: &mut u32) -> Option<El> {
        *seen += 1;
        if path.len() > 45 || *seen > 6000 {
            return None;
        }
        if wanted(node) {
            let above = path.iter().rev().take(5).cloned().collect();
            return Some(El { node: node.clone(), above });
        }
        let mut children = node.children();
        if last_first {
            children.reverse();
        }
        path.push(node.clone());
        let found = children.iter().find_map(|child| walk(child, last_first, wanted, path, seen));
        path.pop();
        found
    }
    walk(node, last_first, wanted, &mut Vec::new(), &mut 0)
}

/// The conversation's message box in Teams's window: the element named "new-message-…" in the page
/// (whatever the user's language).
fn find_box(frame: &Node) -> Option<El> {
    find(frame, true, &|n| n.id().starts_with("new-message"))
}

/// The leftmost of the buttons in the row under the box (Teams's formatting one): looked for in the
/// box's nearest container that has them.
fn first_tool(el: &El, rect: Rect) -> Option<Rect> {
    fn buttons(node: &Node, rect: Rect, depth: u32, out: &mut Vec<Rect>) {
        if depth > 8 {
            return;
        }
        for child in node.children() {
            if child.role().contains("button") {
                if let Some(r) = child.rect().filter(|r| is_tool(*r, rect)) {
                    out.push(r);
                }
            } else {
                buttons(&child, rect, depth + 1, out);
            }
        }
    }
    el.above.iter().take(4).find_map(|container| {
        let mut found = Vec::new();
        buttons(container, rect, 0, &mut found);
        leftmost(found)
    })
}

/// What an element says: its name, else its description, else the names under it put together.
fn text_of(node: &Node) -> String {
    fn texts(node: &Node, depth: u32, out: &mut Vec<String>) {
        if depth > 8 {
            return;
        }
        for child in node.children() {
            let name = child.name();
            if !name.trim().is_empty() {
                out.push(name.trim().to_owned());
            } else {
                texts(&child, depth + 1, out);
            }
        }
    }
    for said in [node.name(), node.description()] {
        if !said.trim().is_empty() {
            return said.trim().to_owned();
        }
    }
    let mut out = Vec::new();
    texts(node, 0, &mut out);
    out.join(" ")
}

/// X11: the window in front, a window brought in front, keys typed.
struct X {
    conn: RustConnection,
    root: Window,
    active: u32,
    pid: u32,
    name: u32,
    utf8: u32,
}

impl X {
    fn new() -> Option<X> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| conn.intern_atom(false, name).ok()?.reply().ok().map(|r| r.atom);
        let (active, pid, name, utf8) = (atom(b"_NET_ACTIVE_WINDOW")?, atom(b"_NET_WM_PID")?, atom(b"_NET_WM_NAME")?, atom(b"UTF8_STRING")?);
        Some(X { conn, root, active, pid, name, utf8 })
    }

    fn number(&self, window: Window, property: u32, kind: AtomEnum) -> Option<u32> {
        let reply = self.conn.get_property(false, window, property, kind, 0, 1).ok()?.reply().ok()?;
        reply.value32()?.next()
    }

    fn active_window(&self) -> Option<Window> {
        self.number(self.root, self.active, AtomEnum::WINDOW).filter(|w| *w != 0)
    }

    fn pid_of(&self, window: Window) -> Option<u32> {
        self.number(window, self.pid, AtomEnum::CARDINAL)
    }

    fn title(&self, window: Window) -> String {
        let read = |property: u32, kind: u32| {
            let reply = self.conn.get_property(false, window, property, kind, 0, 1024).ok()?.reply().ok()?;
            Some(String::from_utf8_lossy(&reply.value).into_owned()).filter(|t| !t.is_empty())
        };
        read(self.name, self.utf8).or_else(|| read(AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())).unwrap_or_default()
    }

    /// `window` in front, asked of the window manager.
    fn activate(&self, window: Window) {
        // Source 2: a pager, as the user's own wish (the window managers honour it).
        let event = ClientMessageEvent::new(32, window, self.active, [2u32, 0, 0, 0, 0]);
        let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event);
        let _ = self.conn.flush();
    }

    /// The key typing `keysym` in the keyboard's layout.
    fn keycode(&self, keysym: u32) -> Option<u8> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let per = map.keysyms_per_keycode.max(1) as usize;
        let index = map.keysyms.iter().position(|k| *k == keysym)?;
        u8::try_from(index / per).ok().and_then(|i| min.checked_add(i))
    }

    fn key(&self, code: u8, down: bool) {
        let kind = if down { x11rb::protocol::xproto::KEY_PRESS_EVENT } else { x11rb::protocol::xproto::KEY_RELEASE_EVENT };
        let _ = self.conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, self.root, 0, 0, 0);
        let _ = self.conn.flush();
        std::thread::sleep(Duration::from_millis(15));
    }

    /// Ctrl and the key typing `keysym`; false when a key isn't on the keyboard.
    fn control(&self, keysym: u32) -> bool {
        // XK_Control_L.
        let (Some(control), Some(key)) = (self.keycode(0xffe3), self.keycode(keysym)) else { return false };
        self.key(control, true);
        self.key(key, true);
        self.key(key, false);
        self.key(control, false);
        true
    }
}

/// Teams's window: its title says so (the web app's: "… | Microsoft Teams").
fn is_teams(title: &str) -> bool {
    title.to_lowercase().contains("microsoft teams")
}

/// What is in front and Teams's message box, looked at again and again (by the watching thread).
pub(super) struct Watcher {
    x: X,
    me: u32,
    /// The application of each process on the accessibility's bus.
    roots: HashMap<u32, Node>,
    /// Teams's message box last found, where it was and its first tool there.
    last: Option<(El, Rect, Option<Rect>)>,
    /// When it was last looked for (no box known).
    searched: Option<Instant>,
    /// The window and process in front at the last look.
    window: Window,
    pid: u32,
}

impl Watcher {
    pub(super) fn new() -> Option<Watcher> {
        bus()?;
        Some(Watcher { x: X::new()?, me: std::process::id(), roots: HashMap::new(), last: None, searched: None, window: 0, pid: 0 })
    }

    pub(super) fn look(&mut self) -> Front<App, El> {
        let Some(window) = self.x.active_window() else { return Front::Other };
        let pid = self.x.pid_of(window).unwrap_or(0);
        (self.window, self.pid) = (window, pid);
        if pid == self.me {
            return Front::Ours;
        }
        let title = self.x.title(window);
        if !is_teams(&title) {
            return Front::Other;
        }
        let app = App { window, pid, title };
        // The box last found while it is still there; else, every 2 seconds, looked for in the window.
        let rect = self.last.as_ref().and_then(|(el, _, _)| el.node.rect()).filter(|r| r.width() > 40.0 && r.height() > 8.0);
        if rect.is_none() {
            self.last = None;
            if self.searched.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
                self.searched = Some(Instant::now());
                if !self.roots.contains_key(&pid)
                    && let Some(root) = app_root(pid)
                {
                    self.roots.insert(pid, root);
                }
                let found = self.roots.get(&pid).and_then(|root| frame(root, &app.title)).and_then(|f| find_box(&f));
                if found.is_none() {
                    // Teams started again: its application looked for again.
                    self.roots.remove(&pid);
                }
                self.last = found.and_then(|el| {
                    let rect = el.node.rect()?;
                    Some((el, rect, None))
                });
                if let Some((el, rect, tool)) = self.last.as_mut() {
                    *tool = first_tool(el, *rect);
                }
            }
            return Front::Teams { app, field: self.last.as_ref().map(|(el, rect, tool)| Field { el: el.clone(), rect: *rect, tool: *tool }) };
        }
        let rect = rect.unwrap_or(Rect::NOTHING);
        // Moved or grown: its tools looked for again.
        if let Some((el, at, tool)) = self.last.as_mut()
            && *at != rect
        {
            *at = rect;
            *tool = first_tool(el, rect);
        }
        let field = self.last.as_ref().map(|(el, rect, tool)| Field { el: el.clone(), rect: *rect, tool: *tool });
        Front::Teams { app, field }
    }

    /// What was in front at the last look, for `RONNIE_TEAMS_DEBUG`.
    pub(super) fn who(&self) -> String {
        format!("window {:#x}, process {}", self.window, self.pid)
    }
}

/// The accessibility turned on (Chromium fills its pages' only then).
pub(super) fn prepare() {
    let Ok(session) = Connection::session() else {
        debug("no session bus");
        return;
    };
    let on = zbus::zvariant::Value::from(true);
    let done = session.call_method(Some("org.a11y.Bus"), "/org/a11y/bus", Some("org.freedesktop.DBus.Properties"), "Set", &("org.a11y.Status", "IsEnabled", on));
    if let Err(e) = done {
        debug(&format!("the accessibility can't be turned on: {e}"));
    }
}

/// The draft in the box.
pub(super) fn draft(el: &El) -> String {
    el.node.text().unwrap_or_else(|| el.node.name())
}

/// The messages of the conversation shown in Teams (those its page holds: the last ones), the oldest
/// first: who wrote it (None: the user), and what.
pub(super) fn conversation(app: &App) -> Vec<(Option<String>, String)> {
    let Some(frame) = app_root(app.pid).and_then(|root| frame(&root, &app.title)) else { return Vec::new() };
    let Some(list) = find(&frame, false, &|n| n.id() == "chat-pane-list") else { return Vec::new() };
    fn walk(node: &Node, depth: u32, author: &mut Option<String>, out: &mut Vec<(Option<String>, String)>) {
        if depth > 10 {
            return;
        }
        let attributes = node.attributes();
        let id = attributes.get("id").cloned().unwrap_or_default();
        if id.starts_with("author-") {
            *author = Some(text_of(node)).filter(|a| !a.is_empty());
            return;
        }
        if let Some(key) = id.strip_prefix("message-body-") {
            let mine = attributes.get("class").is_some_and(|c| c.contains("ChatMyMessage"));
            let content = find(node, false, &|c| c.id() == format!("content-{key}"));
            let text: String = content.map_or_else(|| text_of(node), |c| text_of(&c.node)).chars().take(MESSAGE_CHARS).collect();
            if !text.is_empty() {
                out.push(((!mine).then(|| author.clone().unwrap_or_else(|| "?".into())), text));
            }
            return;
        }
        for child in node.children() {
            walk(&child, depth + 1, author, out);
        }
    }
    let mut out = Vec::new();
    walk(&list.node, 0, &mut None, &mut out);
    last_messages(out)
}

/// `text` in place of the draft in Teams's box: Teams's window brought to the front, the box focused,
/// Ctrl A and Ctrl V typed; the clipboard's text given back after. Without those keys, the box's text set
/// through AT-SPI.
pub(super) fn paste_into(app: App, el: El, text: String) {
    let Some(x) = X::new() else {
        let done = el.node.set_text(&text);
        debug(if done { "set through AT-SPI" } else { "the draft couldn't be replaced" });
        return;
    };
    x.activate(app.window);
    std::thread::sleep(Duration::from_millis(250));
    el.node.grab_focus();
    std::thread::sleep(Duration::from_millis(100));
    let mut clipboard = arboard::Clipboard::new().ok();
    let before = clipboard.as_mut().and_then(|c| c.get_text().ok());
    let copied = clipboard.as_mut().is_some_and(|c| c.set_text(text.clone()).is_ok());
    // a, then v.
    if copied && x.control(0x61) {
        std::thread::sleep(Duration::from_millis(60));
        if x.control(0x76) {
            debug("pasted with Ctrl V");
            std::thread::sleep(Duration::from_millis(600));
            if let (Some(c), Some(before)) = (clipboard.as_mut(), before) {
                let _ = c.set_text(before);
            }
            return;
        }
    }
    let done = el.node.set_text(&text);
    debug(if done { "set through AT-SPI" } else { "the draft couldn't be replaced" });
}

/// Teams's window in front again.
pub(super) fn back_to(app: &App) {
    if let Some(x) = X::new() {
        x.activate(app.window);
    }
}

/// The screens (all of X11's root window): the panel kept in.
pub(super) fn screen_of(_at: Pos2) -> Option<Rect> {
    static ROOT: OnceLock<Option<Rect>> = OnceLock::new();
    *ROOT.get_or_init(|| {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let s = conn.setup().roots.get(screen)?;
        Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(s.width_in_pixels as f32, s.height_in_pixels as f32)))
    })
}

/// Ours in front is told by the window in front (the watching thread).
pub(super) fn active() -> bool {
    false
}
