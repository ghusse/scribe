//! OS entry points on platforms without an integration yet (neither Windows nor macOS): the hook and key
//! injection report an error, focus is always unknown, the overlay calls do nothing. Compiled everywhere so
//! that it is tested everywhere; `lib.rs` re-exports it only off Windows and macOS.
#![cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
use std::sync::Arc;

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::{FieldReader, KeySender, SystemMute};
use scribe_core::permissions::{Permission, PermissionStatus, SystemPermissions};

use crate::output_mute::{OutputBackend, OutputMuter};
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

struct NoField;

impl FieldReader for NoField {
    fn focused_text(&self) -> Option<String> {
        None
    }
}

/// No accessibility API here: a paste is never verified.
pub fn field_reader() -> Arc<dyn FieldReader> {
    Arc::new(NoField)
}

/// Physical key state: unknown here, reported as up (there is no hook, so no trigger is ever held).
pub fn is_key_pressed(_vk: u32) -> bool {
    false
}

/// No audio API here: there is no output to mute.
pub struct NoOutputs;

impl OutputBackend for NoOutputs {
    fn active_outputs(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    fn is_muted(&self, id: &str) -> Result<bool, String> {
        Err(format!("sortie audio {id} inconnue"))
    }
    fn set_muted(&self, id: &str, _muted: bool) -> Result<(), String> {
        Err(format!("sortie audio {id} inconnue"))
    }
}

pub fn system_mute() -> Arc<dyn SystemMute> {
    Arc::new(OutputMuter(NoOutputs))
}

pub fn prepare_overlay(_raw_hwnd: isize) {}

struct NoPermissions;

impl SystemPermissions for NoPermissions {
    fn status(&self) -> Vec<PermissionStatus> {
        Vec::new()
    }
    fn request(&self, _permission: Permission) {}
    fn open_settings(&self, _permission: Permission) -> Result<(), String> {
        Err("aucun réglage d'autorisation sur cette plateforme".into())
    }
}

/// No OS permission to ask for (Windows grants the hook, the microphone and UI Automation to desktop apps).
pub fn permissions() -> Arc<dyn SystemPermissions> {
    Arc::new(NoPermissions)
}

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
    fn keys_are_reported_up() {
        assert!(!is_key_pressed(0xA3));
    }

    #[test]
    fn focus_is_always_unknown() {
        assert_eq!(focus_detector().snapshot(), FocusSnapshot::unknown());
    }

    #[test]
    fn a_paste_is_never_verified() {
        assert_eq!(field_reader().focused_text(), None);
    }

    #[test]
    fn paste_simulation_fails_so_the_text_stays_in_the_clipboard() {
        assert_eq!(key_sender().send_paste(), Err("simulation clavier non disponible sur cette plateforme".into()));
    }

    #[test]
    fn no_permission_is_asked() {
        let p = permissions();
        assert!(p.status().is_empty());
        p.request(Permission::Microphone);
        assert!(p.open_settings(Permission::Accessibility).is_err());
    }

    #[test]
    fn there_is_no_output_to_mute() {
        assert_eq!(NoOutputs.active_outputs(), Ok(Vec::new()));
        assert!(NoOutputs.is_muted("speakers").is_err());
        assert!(NoOutputs.set_muted("speakers", true).is_err());
        let mute = system_mute();
        assert!(mute.mute_all().is_empty());
        mute.restore(&["speakers".to_string()]);
    }

    #[test]
    fn overlay_calls_are_no_ops() {
        prepare_overlay(1);
        show_overlay(1);
        hide_overlay(1);
    }
}
