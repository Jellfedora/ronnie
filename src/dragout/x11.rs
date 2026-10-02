//! X11: the XDND protocol, spoken from a connection of our own. The window's connection (winit's)
//! keeps the pointer grabbed while the button is down, so the pointer is followed by asking the
//! server where it is, instead of grabbing it; the drop happens when the button is let go.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, KeyButMask, PropMode, SelectionNotifyEvent, SelectionRequestEvent,
    Window, WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::NONE;

type Error = Box<dyn std::error::Error>;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        XdndAware,
        XdndEnter,
        XdndPosition,
        XdndStatus,
        XdndLeave,
        XdndDrop,
        XdndFinished,
        XdndSelection,
        XdndActionCopy,
        TARGETS,
        _NET_WM_PID,
        URI_LIST: b"text/uri-list",
    }
}

/// The window's own window: never a drop target.
static MAIN: OnceLock<Window> = OnceLock::new();
/// A connection for asking about the mouse button, opened once.
static QUERY: Mutex<Option<(RustConnection, Window)>> = Mutex::new(None);

pub fn init(window: raw_window_handle::RawWindowHandle) {
    let id = match window {
        raw_window_handle::RawWindowHandle::Xlib(h) => h.window as Window,
        raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
        _ => return,
    };
    let _ = MAIN.set(id);
}

pub fn button_down() -> bool {
    let mut query = QUERY.lock().unwrap();
    if query.is_none() {
        *query = RustConnection::connect(None).ok().map(|(conn, screen)| {
            let root = conn.setup().roots[screen].root;
            (conn, root)
        });
    }
    let Some((conn, root)) = query.as_ref() else { return false };
    match conn.query_pointer(*root).map_err(Error::from).and_then(|c| c.reply().map_err(Error::from)) {
        Ok(pointer) => pointer.mask.contains(KeyButMask::BUTTON1),
        Err(_) => {
            *query = None;
            false
        }
    }
}

/// The drag goes on in the background, until the files are dropped (or not).
pub fn start(paths: &[PathBuf]) -> bool {
    let data = super::uri_list(paths);
    std::thread::Builder::new()
        .name("drag-out".into())
        .spawn(move || {
            if let Err(e) = run(&data) {
                crate::log::error(&format!("drag out: {e}"));
            }
        })
        .is_ok()
}

fn run(data: &[u8]) -> Result<(), Error> {
    let (conn, screen) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let atoms = Atoms::new(&conn)?.reply()?;
    let source = conn.generate_id()?;
    let aux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::PROPERTY_CHANGE);
    conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, source, root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, x11rb::COPY_FROM_PARENT, &aux)?;
    // A time from the server (selections want one): what a property change on our window says.
    conn.change_property8(PropMode::APPEND, source, AtomEnum::WM_NAME, AtomEnum::STRING, &[])?;
    conn.flush()?;
    let time = loop {
        if let Event::PropertyNotify(e) = conn.wait_for_event()? {
            break e.time;
        }
    };
    conn.set_selection_owner(source, atoms.XdndSelection, time)?;

    let result = follow(&conn, &atoms, root, source, time, data);
    let _ = conn.destroy_window(source);
    let _ = conn.flush();
    result
}

/// Follows the pointer, telling the windows under it, until the button is let go.
fn follow(conn: &RustConnection, atoms: &Atoms, root: Window, source: Window, time: u32, data: &[u8]) -> Result<(), Error> {
    // The window under the pointer that takes drops, and its XDND version.
    let mut target: Option<(Window, u32)> = None;
    let mut accepted = false;
    // A position sent, its status not yet back (one at a time, as XDND wants).
    let mut status_due: Option<Instant> = None;
    let mut last = (i16::MIN, i16::MIN);
    let mut dropped: Option<Instant> = None;
    loop {
        while let Some(event) = conn.poll_for_event()? {
            match event {
                Event::ClientMessage(e) if e.type_ == atoms.XdndStatus => {
                    let d = e.data.as_data32();
                    if target.is_some_and(|(w, _)| w == d[0]) {
                        accepted = d[1] & 1 != 0;
                        status_due = None;
                    }
                }
                Event::ClientMessage(e) if e.type_ == atoms.XdndFinished && dropped.is_some() => return Ok(()),
                Event::SelectionRequest(e) => answer(conn, atoms, &e, data)?,
                _ => {}
            }
        }
        // Dropped: the target asks for the files, then says it's done (or never does).
        if let Some(at) = dropped {
            if at.elapsed() > Duration::from_secs(60) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }

        let pointer = conn.query_pointer(root)?.reply()?;
        if !pointer.mask.contains(KeyButMask::BUTTON1) {
            match target {
                Some((w, _)) if accepted => {
                    message(conn, w, atoms.XdndDrop, [source, 0, time, 0, 0])?;
                    conn.flush()?;
                    dropped = Some(Instant::now());
                    continue;
                }
                Some((w, _)) => {
                    message(conn, w, atoms.XdndLeave, [source, 0, 0, 0, 0])?;
                    conn.flush()?;
                }
                None => {}
            }
            return Ok(());
        }

        let under = drop_target(conn, atoms, root, pointer.root_x, pointer.root_y)?;
        if under.map(|u| u.0) != target.map(|t| t.0) {
            if let Some((w, _)) = target {
                message(conn, w, atoms.XdndLeave, [source, 0, 0, 0, 0])?;
            }
            target = under;
            (accepted, status_due, last) = (false, None, (i16::MIN, i16::MIN));
            if let Some((w, version)) = target {
                message(conn, w, atoms.XdndEnter, [source, version.min(5) << 24, atoms.URI_LIST, 0, 0])?;
            }
        }
        // A target that never answers doesn't stop the drag.
        if status_due.is_some_and(|at| at.elapsed() > Duration::from_millis(500)) {
            status_due = None;
        }
        if let Some((w, _)) = target
            && status_due.is_none()
            && (pointer.root_x, pointer.root_y) != last
        {
            let at = ((pointer.root_x as u16 as u32) << 16) | pointer.root_y as u16 as u32;
            message(conn, w, atoms.XdndPosition, [source, 0, at, time, atoms.XdndActionCopy])?;
            status_due = Some(Instant::now());
            last = (pointer.root_x, pointer.root_y);
        }
        conn.flush()?;
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// The window at (x, y) taking drops (XdndAware), unless it is one of ours.
fn drop_target(conn: &RustConnection, atoms: &Atoms, root: Window, x: i16, y: i16) -> Result<Option<(Window, u32)>, Error> {
    let mut window = root;
    loop {
        let child = conn.translate_coordinates(root, window, x, y)?.reply()?.child;
        if child == NONE {
            return Ok(None);
        }
        window = child;
        let aware = conn.get_property(false, window, atoms.XdndAware, AtomEnum::ATOM, 0, 1)?.reply()?;
        if let Some(version) = aware.value32().and_then(|mut v| v.next()) {
            if Some(&window) == MAIN.get() || is_ours(conn, atoms, window)? {
                return Ok(None);
            }
            return Ok(Some((window, version)));
        }
    }
}

/// A window of this process (another of its windows): dropped there, the files would come back in.
fn is_ours(conn: &RustConnection, atoms: &Atoms, window: Window) -> Result<bool, Error> {
    let pid = conn.get_property(false, window, atoms._NET_WM_PID, AtomEnum::CARDINAL, 0, 1)?.reply()?;
    Ok(pid.value32().and_then(|mut v| v.next()) == Some(std::process::id()))
}

fn message(conn: &RustConnection, to: Window, kind: Atom, data: [u32; 5]) -> Result<(), Error> {
    conn.send_event(false, to, EventMask::NO_EVENT, ClientMessageEvent::new(32, to, kind, data))?;
    Ok(())
}

/// Gives the files (text/uri-list) to the window asking for them.
fn answer(conn: &RustConnection, atoms: &Atoms, e: &SelectionRequestEvent, data: &[u8]) -> Result<(), Error> {
    let property = if e.property == NONE { e.target } else { e.property };
    let given = if e.target == atoms.TARGETS {
        conn.change_property32(PropMode::REPLACE, e.requestor, property, AtomEnum::ATOM, &[atoms.TARGETS, atoms.URI_LIST])?;
        true
    } else if e.target == atoms.URI_LIST {
        conn.change_property8(PropMode::REPLACE, e.requestor, property, atoms.URI_LIST, data)?;
        true
    } else {
        false
    };
    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: e.time,
        requestor: e.requestor,
        selection: e.selection,
        target: e.target,
        property: if given { property } else { NONE },
    };
    conn.send_event(false, e.requestor, EventMask::NO_EVENT, notify)?;
    conn.flush()?;
    Ok(())
}
