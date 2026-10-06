use dispatch2::DispatchQueue;
use objc2::ffi::object_setClass;
use objc2::runtime::AnyObject;
use objc2::ClassType;
use objc2_app_kit::{NSPanel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};

/// NSStatusWindowLevel: above other apps' windows, full-screen ones included.
const STATUS_LEVEL: isize = 25;

unsafe fn window<'a>(raw: isize) -> &'a NSWindow {
    &*(raw as *const NSWindow)
}

/// The overlay must never take focus, or the paste would land in it instead of the user's field: it becomes a
/// non-activating panel (a click on it does not activate Scribe). Called once at setup, on the main thread.
pub fn prepare_overlay(raw: isize) {
    unsafe {
        object_setClass(raw as *mut AnyObject, NSPanel::class());
        let w = window(raw);
        w.setStyleMask(w.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        w.setLevel(STATUS_LEVEL);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        // Scribe is never the active app: a panel hidden on deactivation would never show.
        w.setHidesOnDeactivate(false);
    }
}

// show/hide run with the `Overlay` lock held, from any thread, while AppKit wants the main thread: post, never wait.
pub fn show_overlay(raw: isize) {
    DispatchQueue::main().exec_async(move || unsafe { window(raw).orderFrontRegardless() });
}

pub fn hide_overlay(raw: isize) {
    DispatchQueue::main().exec_async(move || unsafe { window(raw).orderOut(None) });
}
