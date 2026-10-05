//! OS integration: keyboard hook, focus detection, key injection, clipboard, microphone.
//!
//! The OS entry points (`start_keyboard_hook`, `focus_detector`, `key_sender`, `*_overlay`) are re-exported
//! from `windows` on Windows and from `fallback` elsewhere: this file holds no dispatch logic of its own.
pub mod audio_capture;
pub mod clipboard;
mod device;
pub mod fallback;
pub mod focus_rules;
pub mod key_filter;
#[cfg(windows)]
mod windows;

use std::sync::atomic::{AtomicBool, AtomicU32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKey {
    pub vk: u32,
    pub down: bool,
    pub t_ms: u64,
}

pub struct HookConfig {
    pub trigger_vk: AtomicU32,
    /// 0 = no lock key.
    pub lock_vk: AtomicU32,
    pub paused: AtomicBool,
}

impl HookConfig {
    pub fn new(trigger_vk: u32, lock_vk: u32) -> Self {
        Self { trigger_vk: AtomicU32::new(trigger_vk), lock_vk: AtomicU32::new(lock_vk), paused: AtomicBool::new(false) }
    }
}

pub type KeyCallback = Box<dyn Fn(RawKey) + Send + Sync>;

#[cfg(not(windows))]
pub use fallback::{focus_detector, hide_overlay, key_sender, prepare_overlay, show_overlay, start_keyboard_hook, HookHandle};
#[cfg(windows)]
pub use windows::{focus_detector, hide_overlay, key_sender, prepare_overlay, show_overlay, start_keyboard_hook, HookHandle};
