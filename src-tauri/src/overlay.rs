use std::sync::atomic::{AtomicU8, Ordering};

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

const KIND_IDLE: u8 = 0;
const KIND_RECORDING: u8 = 1;
const KIND_PROCESSING: u8 = 2;
const KIND_TOAST: u8 = 3;

/// Kind of the last event emitted to the overlay, so that a late dismiss coming
/// from the overlay (toast auto-hide timer) cannot hide a newer recording pill.
static LAST_KIND: AtomicU8 = AtomicU8::new(KIND_IDLE);

fn kind_code(ev: &OverlayEvent) -> u8 {
    match ev {
        OverlayEvent::Idle => KIND_IDLE,
        OverlayEvent::Recording { .. } => KIND_RECORDING,
        OverlayEvent::Processing => KIND_PROCESSING,
        OverlayEvent::Toast { .. } => KIND_TOAST,
    }
}

/// A dismiss requested by the overlay only applies to a toast (or an already idle overlay).
fn dismiss_allowed(last_kind: u8) -> bool {
    last_kind == KIND_TOAST || last_kind == KIND_IDLE
}

/// Hides the overlay on behalf of the user/overlay, unless the backend has since
/// moved on to a recording or processing state.
pub fn dismiss(app: &AppHandle) {
    // Atomically turn a toast into idle; anything else (recording, processing) is left alone.
    let allowed = match LAST_KIND.compare_exchange(KIND_TOAST, KIND_IDLE, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => true,
        Err(current) => dismiss_allowed(current),
    };
    if allowed {
        hide(app);
    }
}

pub fn emit(app: &AppHandle, ev: OverlayEvent) {
    let idle = matches!(ev, OverlayEvent::Idle);
    LAST_KIND.store(kind_code(&ev), Ordering::SeqCst);
    let _ = app.emit_to("overlay", "overlay", &ev);
    if idle { hide(app) } else { show(app) }
}

pub fn toast(app: &AppHandle, level: ToastLevel, message: impl Into<String>, preview: Option<String>, dictation_id: Option<i64>) {
    emit(app, OverlayEvent::Toast { level, message: message.into(), preview, dictation_id });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_dismiss_does_not_hide_recording_or_processing() {
        assert!(!dismiss_allowed(kind_code(&OverlayEvent::Recording { locked: false })));
        assert!(!dismiss_allowed(kind_code(&OverlayEvent::Recording { locked: true })));
        assert!(!dismiss_allowed(kind_code(&OverlayEvent::Processing)));
    }

    #[test]
    fn dismiss_hides_toast_and_idle() {
        let toast = OverlayEvent::Toast { level: ToastLevel::Copied, message: "x".into(), preview: None, dictation_id: Some(1) };
        assert!(dismiss_allowed(kind_code(&toast)));
        assert!(dismiss_allowed(kind_code(&OverlayEvent::Idle)));
    }
}
