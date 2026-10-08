//! OS integration: keyboard hook, focus detection, key injection, clipboard, microphone, audio outputs.
//!
//! The OS entry points (`start_keyboard_hook`, `focus_detector`, `key_sender`, `system_mute`, `boot_time_ms`,
//! `exclusive_input`, `*_overlay`) are
//! re-exported from `windows` on Windows, `macos` on macOS and `fallback` elsewhere: this file holds no dispatch
//! logic of its own.
pub mod audio_capture;
pub mod clipboard;
mod device;
pub mod exclusive_mic;
pub mod fallback;
pub mod focus_rules;
pub mod key_filter;
pub mod mac_keys;
pub mod output_mute;
pub mod wasapi_rules;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use scribe_core::chord;
use scribe_core::gesture::KeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKey {
    pub vk: u32,
    pub down: bool,
    /// Auto-repeat key-down (the key was already down).
    pub repeat: bool,
    pub t_ms: u64,
}

/// A real key event and what it means for the gesture detector (trigger combination / lock key edges).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub key: RawKey,
    pub gestures: Vec<KeyEvent>,
}

pub struct HookConfig {
    /// The trigger combination, packed by `scribe_core::chord::pack`.
    pub trigger: AtomicU32,
    /// 0 = no lock key.
    pub lock_vk: AtomicU32,
    pub paused: AtomicBool,
    /// A hotkey capture is running: every key is swallowed (so Win or Alt open no menu, letters type nothing).
    pub capturing: AtomicBool,
}

impl HookConfig {
    pub fn new(trigger_keys: &[u32], lock_vk: u32) -> Self {
        Self {
            trigger: AtomicU32::new(chord::pack(trigger_keys)),
            lock_vk: AtomicU32::new(lock_vk),
            paused: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
        }
    }

    pub fn set_trigger(&self, keys: &[u32]) {
        self.trigger.store(chord::pack(keys), Ordering::Relaxed);
    }
}

pub type KeyCallback = Box<dyn Fn(HookEvent) + Send + Sync>;

#[cfg(not(any(windows, target_os = "macos")))]
pub use fallback::{
    exclusive_input, field_reader, focus_detector, hide_overlay, is_key_pressed, key_sender, permissions, prepare_overlay, show_overlay,
    boot_time_ms, start_keyboard_hook, system_mute, HookHandle,
};
#[cfg(windows)]
pub use windows::{
    exclusive_input, field_reader, focus_detector, hide_overlay, is_key_pressed, key_sender, permissions, prepare_overlay, show_overlay,
    boot_time_ms, start_keyboard_hook, system_mute, HookHandle,
};
#[cfg(target_os = "macos")]
pub use macos::{
    exclusive_input, field_reader, focus_detector, hide_overlay, is_key_pressed, key_sender, permissions, prepare_overlay, show_overlay,
    boot_time_ms, start_keyboard_hook, system_mute, HookHandle,
};
