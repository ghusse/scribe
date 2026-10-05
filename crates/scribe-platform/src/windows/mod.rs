//! Windows entry points, re-exported by `lib.rs`. Wiring only: each function hands over to one Win32 module.
use std::sync::Arc;

use scribe_core::focus::FocusDetector;
use scribe_core::insert::KeySender;

pub mod focus;
pub mod hook;
pub mod keys;
pub mod window;

pub use hook::{start as start_keyboard_hook, HookHandle};
pub use window::{hide_overlay, prepare_overlay, show_overlay};

pub fn focus_detector() -> Arc<dyn FocusDetector> {
    Arc::new(focus::UiaFocusDetector)
}

pub fn key_sender() -> Arc<dyn KeySender> {
    Arc::new(keys::WinKeySender)
}
