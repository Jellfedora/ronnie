//! Wayland: a data source dragged from the window's surface, on the window's own connection (a drag
//! must start from the button press that the compositor saw, by its serial). Our own pointer object
//! hears the presses as winit's does.

use std::io::Write;
use std::os::raw::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use wayland_backend::client::Backend;
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum};

const BTN_LEFT: u32 = 0x110;
const URI_LIST: &str = "text/uri-list";

struct Shared {
    conn: Connection,
    qh: QueueHandle<State>,
    seat: Mutex<Seat>,
}

#[derive(Default)]
struct Seat {
    manager: Option<WlDataDeviceManager>,
    seat: Option<WlSeat>,
    device: Option<WlDataDevice>,
    pointer: Option<WlPointer>,
    /// The surface the pointer last entered, where a drag starts from.
    surface: Option<WlSurface>,
    /// The last press of the left button, and whether it's still down.
    serial: u32,
    pressed: bool,
    /// A drop offered to the window (by a drag from elsewhere): ours to destroy once over.
    offer: Option<WlDataOffer>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// Our events are handled on a thread of their own, from the window's connection.
struct State;

pub fn init(display: *mut c_void) {
    // SAFETY: the display is winit's, alive as long as the window (the app).
    let backend = unsafe { Backend::from_foreign_display(display.cast()) };
    let conn = Connection::from_backend(backend);
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    if SHARED.set(Shared { conn: conn.clone(), qh, seat: Mutex::default() }).is_err() {
        return;
    }
    let _ = std::thread::Builder::new().name("drag-out".into()).spawn(move || while queue.blocking_dispatch(&mut State).is_ok() {});
}

/// On Wayland (rather than X11).
pub fn active() -> bool {
    SHARED.get().is_some()
}

pub fn button_down() -> bool {
    SHARED.get().is_some_and(|s| s.seat.lock().unwrap().pressed)
}

pub fn start(paths: &[PathBuf]) -> bool {
    let Some(shared) = SHARED.get() else { return false };
    let seat = shared.seat.lock().unwrap();
    let (Some(manager), Some(device), Some(surface), true) = (&seat.manager, &seat.device, &seat.surface, seat.pressed) else {
        return false;
    };
    let source = manager.create_data_source(&shared.qh, super::uri_list(paths));
    source.offer(URI_LIST.into());
    if manager.version() >= 3 {
        source.set_actions(DndAction::Copy);
    }
    device.start_drag(Some(&source), surface, None, seat.serial);
    let _ = shared.conn.flush();
    // The compositor holds the pointer during the drag: the window never hears the button let go.
    super::took_button();
    true
}

fn shared() -> &'static Shared {
    SHARED.get().expect("set before any event")
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(_: &mut Self, registry: &WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        let wl_registry::Event::Global { name, interface, version } = event else { return };
        let mut seat = shared().seat.lock().unwrap();
        match interface.as_str() {
            "wl_seat" if seat.seat.is_none() => seat.seat = Some(registry.bind(name, version.min(5), qh, ())),
            "wl_data_device_manager" if seat.manager.is_none() => seat.manager = Some(registry.bind(name, version.min(3), qh, ())),
            _ => return,
        }
        if let (Some(manager), Some(wl_seat), None) = (&seat.manager, &seat.seat, &seat.device) {
            seat.device = Some(manager.get_data_device(wl_seat, qh, ()));
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, wl_seat: &WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            let mut seat = shared().seat.lock().unwrap();
            if caps.contains(wl_seat::Capability::Pointer) && seat.pointer.is_none() {
                seat.pointer = Some(wl_seat.get_pointer(qh, ()));
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(_: &mut Self, _: &WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let mut seat = shared().seat.lock().unwrap();
        match event {
            wl_pointer::Event::Enter { surface, .. } => seat.surface = Some(surface),
            wl_pointer::Event::Button { serial, button: BTN_LEFT, state, .. } => {
                seat.pressed = state == WEnum::Value(wl_pointer::ButtonState::Pressed);
                if seat.pressed {
                    seat.serial = serial;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataDeviceManager, ()> for State {
    fn event(_: &mut Self, _: &WlDataDeviceManager, _: <WlDataDeviceManager as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// Offers come to every data device of the window (the clipboard's, drops from elsewhere): winit
// handles them on its own; ours are just destroyed.
impl Dispatch<WlDataDevice, ()> for State {
    fn event(_: &mut Self, _: &WlDataDevice, event: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let mut seat = shared().seat.lock().unwrap();
        match event {
            wl_data_device::Event::Selection { id: Some(offer) } => offer.destroy(),
            wl_data_device::Event::Enter { id, .. } => {
                if let Some(old) = std::mem::replace(&mut seat.offer, id) {
                    old.destroy();
                }
            }
            wl_data_device::Event::Leave | wl_data_device::Event::Drop => {
                if let Some(offer) = seat.offer.take() {
                    offer.destroy();
                }
            }
            _ => {}
        }
    }

    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

impl Dispatch<WlDataOffer, ()> for State {
    fn event(_: &mut Self, _: &WlDataOffer, _: <WlDataOffer as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// The source's data is the text/uri-list it gives.
impl Dispatch<WlDataSource, Vec<u8>> for State {
    fn event(_: &mut Self, source: &WlDataSource, event: wl_data_source::Event, data: &Vec<u8>, _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } if mime_type == URI_LIST => {
                let _ = std::fs::File::from(fd).write_all(data);
            }
            wl_data_source::Event::Cancelled | wl_data_source::Event::DndFinished => source.destroy(),
            _ => {}
        }
    }
}
