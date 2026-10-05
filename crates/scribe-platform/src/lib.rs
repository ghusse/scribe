//! OS integration: keyboard hook, focus detection, key injection, clipboard, microphone.
pub mod audio_capture;
pub mod clipboard;
pub mod focus_rules;
#[cfg(windows)]
mod windows;

use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::KeySender;

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

#[cfg(windows)]
pub use windows::hook::HookHandle;
#[cfg(not(windows))]
pub struct HookHandle;

#[cfg(windows)]
pub fn start_keyboard_hook(cfg: Arc<HookConfig>, on_key: KeyCallback) -> Result<HookHandle, String> {
    windows::hook::start(cfg, on_key)
}
#[cfg(not(windows))]
pub fn start_keyboard_hook(_cfg: Arc<HookConfig>, _on_key: KeyCallback) -> Result<HookHandle, String> {
    Err("hook clavier non disponible sur cette plateforme (Plan 3)".into())
}

#[allow(dead_code)]
struct UnknownFocus;
impl FocusDetector for UnknownFocus {
    fn snapshot(&self) -> FocusSnapshot {
        FocusSnapshot::unknown()
    }
}

pub fn focus_detector() -> Arc<dyn FocusDetector> {
    #[cfg(windows)]
    return Arc::new(windows::focus::UiaFocusDetector);
    #[cfg(not(windows))]
    return Arc::new(UnknownFocus);
}

#[allow(dead_code)]
struct NoKeys;
impl KeySender for NoKeys {
    fn send_paste(&self) -> Result<(), String> {
        Err("simulation clavier non disponible sur cette plateforme".into())
    }
}

pub fn key_sender() -> Arc<dyn KeySender> {
    #[cfg(windows)]
    return Arc::new(windows::keys::WinKeySender);
    #[cfg(not(windows))]
    return Arc::new(NoKeys);
}

pub fn prepare_overlay(raw_hwnd: isize) {
    #[cfg(windows)]
    windows::window::prepare_overlay(raw_hwnd);
    #[cfg(not(windows))]
    let _ = raw_hwnd;
}

pub fn show_overlay(raw_hwnd: isize) {
    #[cfg(windows)]
    windows::window::show_overlay(raw_hwnd);
    #[cfg(not(windows))]
    let _ = raw_hwnd;
}

pub fn hide_overlay(raw_hwnd: isize) {
    #[cfg(windows)]
    windows::window::hide_overlay(raw_hwnd);
    #[cfg(not(windows))]
    let _ = raw_hwnd;
}
