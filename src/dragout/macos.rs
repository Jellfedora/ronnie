//! macOS: an AppKit dragging session from the key window's view. Local files are dragged as file
//! URLs; remote ones as file promises, written (downloaded) once the Finder says where.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent, NSEventModifierFlags,
    NSEventType, NSFilePromiseProvider, NSFilePromiseProviderDelegate, NSImage, NSWorkspace,
};
use objc2_foundation::{NSArray, NSError, NSNumber, NSOperationQueue, NSPoint, NSRect, NSSize, NSString, NSURL};

use super::{Ask, RemoteItem, Wake};

pub enum Payload<'a> {
    Files(&'a [PathBuf]),
    Promises { items: Vec<RemoteItem>, asks: Sender<Ask>, wake: Wake },
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RonnieDragSource"]
    struct DragSource;

    unsafe impl NSObjectProtocol for DragSource {}

    unsafe impl NSDraggingSource for DragSource {
        // Copied out of the window only: dropped back on it, the files would be imported again.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(&self, _session: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
            if context == NSDraggingContext::OutsideApplication {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }
    }
);

impl DragSource {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

struct PromiseIvars {
    items: Vec<RemoteItem>,
    asks: Mutex<Sender<Ask>>,
    wake: Wake,
    /// Promises are written there: waiting for a download must not block the main thread.
    queue: Retained<NSOperationQueue>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class doesn't implement Drop. Its
    // ivars are only read (or behind a mutex): the promise is written on another thread.
    #[unsafe(super(NSObject))]
    #[name = "RonniePromiseDelegate"]
    #[ivars = PromiseIvars]
    struct PromiseDelegate;

    unsafe impl NSObjectProtocol for PromiseDelegate {}

    unsafe impl NSFilePromiseProviderDelegate for PromiseDelegate {
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
        fn file_name(&self, provider: &NSFilePromiseProvider, _file_type: &NSString) -> Retained<NSString> {
            NSString::from_str(self.item(provider).map_or("", |(_, item)| item.name.as_str()))
        }

        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
        fn write_promise(&self, provider: &NSFilePromiseProvider, url: &NSURL, completion: &block2::DynBlock<dyn Fn(*mut NSError)>) {
            match self.download(provider, url) {
                Ok(()) => completion.call((std::ptr::null_mut(),)),
                Err(e) => {
                    crate::log::error(&format!("drag out: {e}"));
                    let error = NSError::new(1, &NSString::from_str("Ronnie"));
                    completion.call((Retained::as_ptr(&error).cast_mut(),));
                }
            }
        }

        #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
        fn operation_queue(&self, _provider: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
            self.ivars().queue.clone()
        }
    }
);

impl PromiseDelegate {
    fn new(items: Vec<RemoteItem>, asks: Sender<Ask>, wake: Wake) -> Retained<Self> {
        let queue = NSOperationQueue::new();
        let this = Self::alloc().set_ivars(PromiseIvars { items, asks: Mutex::new(asks), wake, queue });
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }

    /// The item a provider promises (its index is the provider's user info).
    fn item(&self, provider: &NSFilePromiseProvider) -> Option<(usize, &RemoteItem)> {
        let index = provider.userInfo()?.downcast::<NSNumber>().ok()?.unsignedIntegerValue();
        self.ivars().items.get(index).map(|item| (index, item))
    }

    /// Has the file manager download the item where the Finder wants it, and waits until it's there.
    fn download(&self, provider: &NSFilePromiseProvider, url: &NSURL) -> Result<(), String> {
        let (index, _) = self.item(provider).ok_or("unknown item")?;
        let dir = url.to_file_path().and_then(|p| p.parent().map(PathBuf::from)).ok_or("not a folder")?;
        let (done, wait) = std::sync::mpsc::channel();
        self.ivars().asks.lock().unwrap().send(Ask::Download { index, dir, done }).map_err(|_| "the file manager is closed")?;
        (self.ivars().wake)();
        wait.recv().map_err(|_| "the file manager is closed".to_owned())?
    }
}

/// A drag's source, and the delegate of its promises.
type Kept = (Retained<DragSource>, Option<Retained<PromiseDelegate>>);

thread_local! {
    /// AppKit keeps neither the source nor the promises' delegate: kept here for the drags to come
    /// to an end (a promise is written after the drop).
    static KEPT: RefCell<Vec<Kept>> = const { RefCell::new(Vec::new()) };
}

pub fn button_down() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

pub fn start(payload: Payload) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = app.keyWindow() else { return false };
    let Some(view) = window.contentView() else { return false };
    let in_window = window.mouseLocationOutsideOfEventStream();
    let at = view.convertPoint_fromView(in_window, None);
    let workspace = NSWorkspace::sharedWorkspace();

    let mut items = Vec::new();
    let mut delegate = None;
    match payload {
        Payload::Files(paths) => {
            for path in paths {
                let path = NSString::from_str(&path.to_string_lossy());
                let url = NSURL::fileURLWithPath(&path);
                let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*url));
                place(&item, at, items.len(), &workspace.iconForFile(&path));
                items.push(item);
            }
        }
        Payload::Promises { items: remote, asks, wake } => {
            let promised = PromiseDelegate::new(remote.clone(), asks, wake);
            for (i, entry) in remote.iter().enumerate() {
                let file_type = NSString::from_str(if entry.is_dir { "public.folder" } else { "public.data" });
                let provider = NSFilePromiseProvider::initWithFileType_delegate(NSFilePromiseProvider::alloc(), &file_type, ProtocolObject::from_ref(&*promised));
                // SAFETY: any object can be the user info; read back by `PromiseDelegate::item`.
                unsafe { provider.setUserInfo(Some(&NSNumber::new_usize(i))) };
                let extension = entry.name.rsplit_once('.').map_or("", |(_, e)| e);
                // Deprecated for iconForContentType:, which needs UniformTypeIdentifiers.
                #[allow(deprecated)]
                let icon = workspace.iconForFileType(&NSString::from_str(if entry.is_dir { "public.folder" } else { extension }));
                let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*provider));
                place(&item, at, i, &icon);
                items.push(item);
            }
            delegate = Some(promised);
        }
    }
    if items.is_empty() {
        return false;
    }

    // The session starts from a mouse event: the drag going on, made up where the pointer is.
    let time = app.currentEvent().map_or(0.0, |e| e.timestamp());
    let Some(event) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDragged,
        in_window,
        NSEventModifierFlags::empty(),
        time,
        window.windowNumber(),
        None,
        0,
        1,
        1.0,
    ) else {
        return false;
    };
    let source = DragSource::new(mtm);
    let _session = view.beginDraggingSessionWithItems_event_source(&NSArray::from_retained_slice(&items), &event, ProtocolObject::from_ref(&*source));
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        if kept.len() >= 16 {
            kept.remove(0);
        }
        kept.push((source, delegate));
    });
    super::took_button();
    true
}

/// The item's icon under the pointer, the next ones slightly offset.
fn place(item: &NSDraggingItem, at: NSPoint, index: usize, icon: &NSImage) {
    let offset = 6.0 * index.min(4) as f64;
    let frame = NSRect::new(NSPoint::new(at.x - 16.0 + offset, at.y - 16.0 + offset), NSSize::new(32.0, 32.0));
    let contents: &AnyObject = icon;
    // SAFETY: an NSImage is what the contents may be.
    unsafe { item.setDraggingFrame_contents(frame, Some(contents)) };
}
