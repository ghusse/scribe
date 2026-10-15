use std::sync::{Mutex, OnceLock};

use dispatch2::DispatchQueue;
use objc2::ffi::object_setClass;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSCursor, NSEvent, NSPanel, NSResponder, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSObjectProtocol, NSRect};

use crate::overlay_pointer::{Cursor, OverlayAreas, PointerChange, PointerState, PointerTracker};

/// NSStatusWindowLevel: above other apps' windows, full-screen ones included.
const STATUS_LEVEL: isize = 25;

/// NSTrackingMouseEnteredAndExited | MouseMoved | ActiveAlways | InVisibleRect.
const TRACKING_OPTIONS: usize = 0x01 | 0x02 | 0x80 | 0x200;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(cid: i32, target: i32, key: *const AnyObject, value: *const AnyObject) -> i32;
}

static POINTER: Mutex<PointerTracker> = Mutex::new(PointerTracker::new());
static ON_POINTER: OnceLock<Box<dyn Fn(PointerState) + Send + Sync>> = OnceLock::new();

fn apply(change: Option<PointerChange>) {
    let Some(change) = change else { return };
    if let Some(cursor) = change.cursor {
        DispatchQueue::main().exec_async(move || set_cursor(cursor));
    }
    if let Some(f) = ON_POINTER.get() {
        f(change.state);
    }
}

fn set_cursor(cursor: Cursor) {
    let cursor = match cursor {
        Cursor::PointingHand => NSCursor::pointingHandCursor(),
        Cursor::Arrow => NSCursor::arrowCursor(),
    };
    cursor.set();
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class implements no Drop.
    #[unsafe(super(NSObject))]
    struct PointerOwner;

    impl PointerOwner {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) {
            moved(event);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            moved(event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let change = POINTER.lock().unwrap().left();
            apply(change);
        }
    }
);

define_class!(
    // SAFETY: NSPanel has no subclassing requirements and this class adds no ivar and implements no Drop.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ScribeOverlayPanel"]
    struct OverlayPanel;

    impl OverlayPanel {
        /// A non-activating panel still becomes key on a click: the restyle keystrokes would then reach the
        /// overlay instead of the target app.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            false
        }
    }
);

/// Converts the event to overlay coordinates (CSS pixels from the top-left of the web view, which fills the window).
fn moved(event: &NSEvent) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(content) = event.window(mtm).and_then(|w| w.contentView()) else { return };
    let at = event.locationInWindow();
    let (change, state) = {
        let mut p = POINTER.lock().unwrap();
        let change = p.moved(at.x, content.frame().size.height - at.y);
        (change, p.state())
    };
    // The active app may reset the cursor on its own events: assert ours while over a button.
    if change.is_none() && state.over.is_some() {
        set_cursor(Cursor::PointingHand);
    }
    apply(change);
}

/// WebKit gets no mouse event in a window of an inactive app, and Scribe never is active: an always-active
/// tracking area tells `overlay_pointer` where the pointer is.
unsafe fn track_pointer(w: &NSWindow) {
    let Some(content) = w.contentView() else { return };
    let owner: Retained<PointerOwner> = msg_send![PointerOwner::alloc(), init];
    let area: *mut AnyObject = msg_send![AnyClass::get(c"NSTrackingArea").unwrap(), alloc];
    let area: *mut AnyObject = msg_send![area, initWithRect: NSRect::ZERO, options: TRACKING_OPTIONS, owner: &*owner, userInfo: std::ptr::null::<AnyObject>()];
    let _: () = msg_send![&content, addTrackingArea: area];
    // A tracking area does not retain its owner.
    std::mem::forget(owner);
}

/// Lets the overlay set the cursor while another app is active (private SkyLight property, as used by
/// background utilities).
unsafe fn set_cursor_in_background() {
    let key: *const AnyObject =
        msg_send![AnyClass::get(c"NSString").unwrap(), stringWithUTF8String: c"SetsCursorInBackground".as_ptr()];
    let yes: *const AnyObject = msg_send![AnyClass::get(c"NSNumber").unwrap(), numberWithBool: Bool::YES];
    let cid = _CGSDefaultConnection();
    CGSSetConnectionProperty(cid, cid, key, yes);
}

/// Called whenever the pointer enters or leaves the overlay or one of its buttons.
pub fn on_overlay_pointer(f: Box<dyn Fn(PointerState) + Send + Sync>) {
    let _ = ON_POINTER.set(f);
}

/// The overlay's visible surface and buttons, sent by its web view after each render.
pub fn set_overlay_hit_areas(areas: OverlayAreas) {
    let change = POINTER.lock().unwrap().set_areas(areas);
    apply(change);
}

unsafe fn window<'a>(raw: isize) -> &'a NSWindow {
    &*(raw as *const NSWindow)
}

/// The overlay must never take focus, or the paste would land in it instead of the user's field: it becomes a
/// non-activating panel that never becomes key (a click on it does not activate Scribe). Called once at setup, on
/// the main thread.
pub fn prepare_overlay(raw: isize) {
    unsafe {
        object_setClass(raw as *mut AnyObject, OverlayPanel::class());
        let w = window(raw);
        w.setStyleMask(w.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        // AppKit ignores NonactivatingPanel set after the window exists: without the WindowServer tag, a click
        // activates Scribe and macOS jumps to the Space of its main window.
        if w.respondsToSelector(sel!(_setPreventsActivation:)) {
            let _: () = msg_send![w, _setPreventsActivation: Bool::YES];
        }
        w.setLevel(STATUS_LEVEL);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        // Scribe is never the active app: a panel hidden on deactivation would never show.
        w.setHidesOnDeactivate(false);
        w.setAcceptsMouseMovedEvents(true);
        track_pointer(w);
        set_cursor_in_background();
    }
}

// show/hide run with the `Overlay` lock held, from any thread, while AppKit wants the main thread: post, never wait.
pub fn show_overlay(raw: isize) {
    DispatchQueue::main().exec_async(move || unsafe { window(raw).orderFrontRegardless() });
}

pub fn hide_overlay(raw: isize) {
    DispatchQueue::main().exec_async(move || {
        unsafe { window(raw).orderOut(None) };
        // A window ordered out gets no mouseExited: a kept position would hover the next layout.
        let change = POINTER.lock().unwrap().left();
        apply(change);
    });
}
