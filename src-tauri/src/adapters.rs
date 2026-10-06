//! Thin adapters from the app's seams to Tauri, Win32 and cpal. No decisions here: every rule lives
//! in a tested module (`overlay::Overlay`, `overlay::overlay_position`, `controller`, ...).
use scribe_core::audio::AudioClip;
use scribe_platform::audio_capture::{self, LevelCallback, RecordingHandle};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::bootstrap::MAIN_WINDOW;
use crate::controller::{Recorder, Recording};
use crate::overlay::{self, OverlayEvent, OverlayWindow};
use crate::services::{LaunchAtLogin, UiSink};

const OVERLAY_WINDOW: &str = "overlay";
/// Logical pixels between the overlay and the bottom of the screen.
const OVERLAY_MARGIN: f64 = 80.0;

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Launch at login through tauri-plugin-autostart (registered in `main.rs` with `--minimized`).
pub struct TauriAutostart(pub AppHandle);

impl LaunchAtLogin for TauriAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt;
        self.0.autolaunch().is_enabled().map_err(|e| e.to_string())
    }
    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = self.0.autolaunch();
        if enabled { launcher.enable() } else { launcher.disable() }.map_err(|e| e.to_string())
    }
}

pub struct TauriUi(pub AppHandle);

impl UiSink for TauriUi {
    fn audio_level(&self, rms: f32) {
        let _ = self.0.emit_to(OVERLAY_WINDOW, "audio-level", rms);
    }
    fn history_changed(&self) {
        let _ = self.0.emit_to(MAIN_WINDOW, "history-changed", ());
    }
    fn focus_dictation(&self, id: i64) {
        let _ = self.0.emit_to(MAIN_WINDOW, "focus-dictation", id);
    }
    fn show_main(&self) {
        show_main(&self.0);
    }
}

fn raw_hwnd(app: &AppHandle) -> Option<isize> {
    #[cfg(windows)]
    {
        app.get_webview_window(OVERLAY_WINDOW).and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// The overlay webview, shown and hidden without activation through Win32 when available.
///
/// The HWND is resolved once, at construction on the main thread: `WebviewWindow::hwnd()` is a
/// synchronous round-trip to the event loop, and `show`/`hide` run with the `Overlay` lock held,
/// so they must only post (Win32 calls or Tauri's fire-and-forget `show`/`hide`), never wait.
pub struct TauriOverlayWindow {
    app: AppHandle,
    hwnd: Option<isize>,
}

impl TauriOverlayWindow {
    /// Call on the main thread (the setup hook), after the overlay window exists.
    pub fn new(app: AppHandle) -> Self {
        let hwnd = raw_hwnd(&app);
        Self { app, hwnd }
    }
}

impl OverlayWindow for TauriOverlayWindow {
    fn send(&self, ev: &OverlayEvent) {
        let _ = self.app.emit_to(OVERLAY_WINDOW, "overlay", ev);
    }
    fn show(&self) {
        match self.hwnd {
            Some(h) => scribe_platform::show_overlay(h),
            None => {
                if let Some(w) = self.app.get_webview_window(OVERLAY_WINDOW) {
                    let _ = w.show();
                }
            }
        }
    }
    fn hide(&self) {
        match self.hwnd {
            Some(h) => scribe_platform::hide_overlay(h),
            None => {
                if let Some(w) = self.app.get_webview_window(OVERLAY_WINDOW) {
                    let _ = w.hide();
                }
            }
        }
    }
}

/// Places the overlay (see `overlay::overlay_position`) and makes it non-activating.
pub fn setup_overlay(app: &AppHandle) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window(OVERLAY_WINDOW) else { return Ok(()) };
    if let Some(monitor) = win.primary_monitor()? {
        let size = win.outer_size()?;
        let (x, y) = overlay::overlay_position(
            (monitor.position().x, monitor.position().y),
            (monitor.size().width, monitor.size().height),
            (size.width, size.height),
            monitor.scale_factor(),
            OVERLAY_MARGIN,
        );
        win.set_position(PhysicalPosition::new(x, y))?;
    }
    if let Some(h) = raw_hwnd(app) {
        scribe_platform::prepare_overlay(h);
    }
    Ok(())
}

/// The default microphone through cpal.
pub struct CpalRecorder;

struct CpalRecording(RecordingHandle);

impl Recording for CpalRecording {
    fn stop(self: Box<Self>) -> Result<AudioClip, String> {
        self.0.stop()
    }
}

impl Recorder for CpalRecorder {
    fn start(&self, on_level: LevelCallback) -> Result<Box<dyn Recording>, String> {
        audio_capture::start_recording(on_level).map(|h| Box::new(CpalRecording(h)) as Box<dyn Recording>)
    }
}
