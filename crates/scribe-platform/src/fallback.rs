//! OS entry points on platforms without an integration yet (everything but Windows): the hook and key
//! injection report an error, focus is always unknown, the overlay calls do nothing. Compiled everywhere so
//! that it is tested on Windows too; `lib.rs` re-exports it only off Windows.
#![cfg_attr(windows, allow(dead_code))]
use std::sync::Arc;

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::KeySender;

use crate::{HookConfig, KeyCallback};

pub struct HookHandle;

pub fn start_keyboard_hook(_cfg: Arc<HookConfig>, _on_key: KeyCallback) -> Result<HookHandle, String> {
    Err("hook clavier non disponible sur cette plateforme (Plan 3)".into())
}

struct UnknownFocus;

impl FocusDetector for UnknownFocus {
    fn snapshot(&self) -> FocusSnapshot {
        FocusSnapshot::unknown()
    }
}

pub fn focus_detector() -> Arc<dyn FocusDetector> {
    Arc::new(UnknownFocus)
}

struct NoKeys;

impl KeySender for NoKeys {
    fn send_paste(&self) -> Result<(), String> {
        Err("simulation clavier non disponible sur cette plateforme".into())
    }
}

pub fn key_sender() -> Arc<dyn KeySender> {
    Arc::new(NoKeys)
}

pub fn prepare_overlay(_raw_hwnd: isize) {}

pub fn show_overlay(_raw_hwnd: isize) {}

pub fn hide_overlay(_raw_hwnd: isize) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_hook_is_unavailable() {
        let r = start_keyboard_hook(Arc::new(HookConfig::new(&[0xA3], 0x20)), Box::new(|_| {}));
        assert_eq!(r.err(), Some("hook clavier non disponible sur cette plateforme (Plan 3)".to_string()));
    }

    #[test]
    fn focus_is_always_unknown() {
        assert_eq!(focus_detector().snapshot(), FocusSnapshot::unknown());
    }

    #[test]
    fn paste_simulation_fails_so_the_text_stays_in_the_clipboard() {
        assert_eq!(key_sender().send_paste(), Err("simulation clavier non disponible sur cette plateforme".into()));
    }

    #[test]
    fn overlay_calls_are_no_ops() {
        prepare_overlay(1);
        show_overlay(1);
        hide_overlay(1);
    }
}
