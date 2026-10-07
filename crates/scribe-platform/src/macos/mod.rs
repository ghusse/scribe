//! macOS entry points, re-exported by `lib.rs`. Wiring only: each function hands over to one module.
use std::sync::Arc;

use scribe_core::focus::FocusDetector;
use scribe_core::insert::{FieldReader, KeySender, SystemMute};
use scribe_core::permissions::SystemPermissions;

use crate::output_mute::OutputMuter;

pub mod audio_output;
pub mod focus;
pub mod hook;
pub mod keys;
pub mod permissions;
pub mod window;

pub use hook::{start as start_keyboard_hook, HookHandle};
pub use audio_output::boot_time_ms;
pub use keys::is_key_pressed;
pub use window::{hide_overlay, prepare_overlay, show_overlay};

pub fn focus_detector() -> Arc<dyn FocusDetector> {
    Arc::new(focus::AxFocusDetector)
}

pub fn key_sender() -> Arc<dyn KeySender> {
    Arc::new(keys::MacKeySender)
}

pub fn field_reader() -> Arc<dyn FieldReader> {
    Arc::new(focus::AxFieldReader)
}

pub fn system_mute() -> Arc<dyn SystemMute> {
    Arc::new(OutputMuter(audio_output::CoreAudioOutputs))
}

pub fn permissions() -> Arc<dyn SystemPermissions> {
    Arc::new(permissions::MacPermissions)
}
