use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastLevel {
    Info,
    Uncertain,
    Copied,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    Idle,
    Recording { locked: bool },
    Processing,
    Toast { level: ToastLevel, message: String, preview: Option<String>, dictation_id: Option<i64> },
}

fn raw_hwnd(app: &AppHandle) -> Option<isize> {
    #[cfg(windows)]
    {
        app.get_webview_window("overlay").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Bottom-centre of the primary monitor, non-activating.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window("overlay") else { return Ok(()) };
    if let Some(monitor) = win.primary_monitor()? {
        let size = win.outer_size()?;
        let area = monitor.size();
        let margin = (80.0 * monitor.scale_factor()) as i32;
        let x = monitor.position().x + (area.width as i32 - size.width as i32) / 2;
        let y = monitor.position().y + area.height as i32 - size.height as i32 - margin;
        win.set_position(PhysicalPosition::new(x, y))?;
    }
    if let Some(h) = raw_hwnd(app) {
        scribe_platform::prepare_overlay(h);
    }
    Ok(())
}

fn show(app: &AppHandle) {
    match raw_hwnd(app) {
        Some(h) => scribe_platform::show_overlay(h),
        None => {
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.show();
            }
        }
    }
}

pub fn hide(app: &AppHandle) {
    match raw_hwnd(app) {
        Some(h) => scribe_platform::hide_overlay(h),
        None => {
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.hide();
            }
        }
    }
}

pub fn emit(app: &AppHandle, ev: OverlayEvent) {
    let idle = matches!(ev, OverlayEvent::Idle);
    let _ = app.emit_to("overlay", "overlay", &ev);
    if idle { hide(app) } else { show(app) }
}

pub fn toast(app: &AppHandle, level: ToastLevel, message: impl Into<String>, preview: Option<String>, dictation_id: Option<i64>) {
    emit(app, OverlayEvent::Toast { level, message: message.into(), preview, dictation_id });
}
