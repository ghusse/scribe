//! Thin adapters from the app's seams to Tauri, Win32/AppKit and cpal. No decisions here: every rule lives
//! in a tested module (`overlay::Overlay`, `overlay::overlay_position`, `controller`, ...).
use scribe_core::audio::AudioClip;
use scribe_platform::audio_capture::{self, LevelCallback, RecordingHandle};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::bootstrap::MAIN_WINDOW;
use crate::controller::{Recorder, Recording};
use crate::overlay::{self, OverlayEvent, OverlayWindow};
use crate::services::{AppUpdater, AvailableUpdate, LaunchAtLogin, UiSink};

const OVERLAY_WINDOW: &str = "overlay";
/// Logical pixels between the overlay and the bottom of the screen.
const OVERLAY_MARGIN: f64 = 80.0;

/// Builds the main window from its `tauri.conf.json` entry (`"create": false`). Called once the services are
/// managed: its page invokes commands as soon as it loads, which in a release build (embedded assets) can happen
/// while setup still runs (setup pumps the event loop), and a command would then find no `Services` state.
pub fn create_main_window(app: &AppHandle) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.iter().find(|w| w.label == MAIN_WINDOW) else { return Ok(()) };
    tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Updates through tauri-plugin-updater (endpoint and public key in tauri.conf.json).
pub struct TauriUpdater(pub AppHandle);

#[async_trait::async_trait]
impl AppUpdater for TauriUpdater {
    fn current_version(&self) -> String {
        self.0.package_info().version.to_string()
    }
    async fn check(&self) -> Result<Option<AvailableUpdate>, String> {
        use tauri_plugin_updater::UpdaterExt;
        let update = self.0.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
        Ok(update.map(|u| AvailableUpdate { version: u.version, notes: u.body }))
    }
    async fn install(&self) -> Result<(), String> {
        use tauri_plugin_updater::UpdaterExt;
        let update = self.0.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
        let Some(update) = update else {
            return Err("aucune mise à jour disponible".into());
        };
        // Windows: the installer closes Scribe and relaunches it. macOS: restart into the new bundle.
        update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        self.0.restart()
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

/// The overlay's native window: HWND on Windows, NSWindow on macOS.
fn raw_window(app: &AppHandle) -> Option<isize> {
    let window = app.get_webview_window(OVERLAY_WINDOW)?;
    #[cfg(windows)]
    {
        window.hwnd().ok().map(|h| h.0 as isize)
    }
    #[cfg(target_os = "macos")]
    {
        window.ns_window().ok().map(|w| w as isize)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = window;
        None
    }
}

/// The overlay webview, shown and hidden without activation through Win32/AppKit when available.
///
/// The native window is resolved once, at construction on the main thread: `WebviewWindow::hwnd()` and
/// `ns_window()` are synchronous round-trips to the event loop, and `show`/`hide` run with the `Overlay` lock
/// held, so they must only post (platform calls or Tauri's fire-and-forget `show`/`hide`), never wait.
pub struct TauriOverlayWindow {
    app: AppHandle,
    native: Option<isize>,
}

impl TauriOverlayWindow {
    /// Call on the main thread (the setup hook), after the overlay window exists.
    pub fn new(app: AppHandle) -> Self {
        let native = raw_window(&app);
        Self { app, native }
    }
}

impl OverlayWindow for TauriOverlayWindow {
    fn send(&self, ev: &OverlayEvent) {
        let _ = self.app.emit_to(OVERLAY_WINDOW, "overlay", ev);
    }
    fn show(&self) {
        match self.native {
            Some(h) => scribe_platform::show_overlay(h),
            None => {
                if let Some(w) = self.app.get_webview_window(OVERLAY_WINDOW) {
                    let _ = w.show();
                }
            }
        }
    }
    fn hide(&self) {
        match self.native {
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
    if let Some(h) = raw_window(app) {
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
