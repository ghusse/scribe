//! Overlay state machine: what the pill shows, and whether a dismiss from the overlay may hide it.
//! The window itself is behind `OverlayWindow` (Tauri webview + Win32 in the app, a fake in tests).
use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastLevel {
    Info,
    Uncertain,
    Copied,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    Idle,
    /// `warning`: shown under the pill for the whole recording (e.g. the microphone could not be kept exclusive).
    Recording { locked: bool, warning: Option<String> },
    Processing,
    Toast { level: ToastLevel, message: String, preview: Option<String>, dictation_id: Option<i64> },
}

/// The overlay window: delivers events to its webview and shows/hides it without taking focus.
///
/// `Overlay` calls these methods with its lock held, from any thread (controller, processing tasks,
/// commands). Implementations must therefore never block waiting on another thread, in particular the
/// main thread that owns the window: post, don't send (Win32 `ShowWindowAsync`, `SWP_ASYNCWINDOWPOS`).
/// Otherwise a main-thread caller waiting for the lock and a holder waiting for the main thread deadlock.
pub trait OverlayWindow: Send + Sync {
    fn send(&self, ev: &OverlayEvent);
    fn show(&self);
    fn hide(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Idle,
    Recording,
    Processing,
    Toast,
}

fn kind_of(ev: &OverlayEvent) -> Kind {
    match ev {
        OverlayEvent::Idle => Kind::Idle,
        OverlayEvent::Recording { .. } => Kind::Recording,
        OverlayEvent::Processing => Kind::Processing,
        OverlayEvent::Toast { .. } => Kind::Toast,
    }
}

pub struct Overlay {
    window: Arc<dyn OverlayWindow>,
    /// Kind of the last event emitted. The lock is held across the window calls, so a dismiss and an
    /// emit never interleave: a toast expiring while the user starts speaking cannot hide the new pill.
    last: Mutex<Kind>,
}

impl Overlay {
    pub fn new(window: Arc<dyn OverlayWindow>) -> Self {
        Self { window, last: Mutex::new(Kind::Idle) }
    }

    pub fn emit(&self, ev: OverlayEvent) {
        let mut last = self.last.lock().unwrap();
        *last = kind_of(&ev);
        self.window.send(&ev);
        if *last == Kind::Idle {
            self.window.hide()
        } else {
            self.window.show()
        }
    }

    pub fn toast(&self, level: ToastLevel, message: impl Into<String>, preview: Option<String>, dictation_id: Option<i64>) {
        self.emit(OverlayEvent::Toast { level, message: message.into(), preview, dictation_id });
    }

    /// Hides the overlay on behalf of the user/overlay (toast closed or expired), unless the backend
    /// has since moved on to a recording or processing state.
    pub fn dismiss(&self) {
        let mut last = self.last.lock().unwrap();
        if matches!(*last, Kind::Toast | Kind::Idle) {
            *last = Kind::Idle;
            self.window.hide();
        }
    }
}

/// Bottom-centre of the monitor, `margin_px` (logical) above its bottom edge, in physical pixels.
pub fn overlay_position(monitor_pos: (i32, i32), monitor_size: (u32, u32), window_size: (u32, u32), scale: f64, margin_px: f64) -> (i32, i32) {
    let margin = (margin_px * scale) as i32;
    let x = monitor_pos.0 + (monitor_size.0 as i32 - window_size.0 as i32) / 2;
    let y = monitor_pos.1 + monitor_size.1 as i32 - window_size.1 as i32 - margin;
    (x, y)
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::{OverlayEvent, OverlayWindow};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Call {
        Send(OverlayEvent),
        Show,
        Hide,
    }

    /// Records every window call; `visible` is the resulting visibility.
    #[derive(Default)]
    pub struct FakeWindow {
        pub calls: Mutex<Vec<Call>>,
        /// Called at the start of `hide`, to widen race windows in tests.
        pub on_hide: Mutex<Option<Box<dyn Fn() + Send>>>,
    }

    impl FakeWindow {
        pub fn events(&self) -> Vec<OverlayEvent> {
            let calls = self.calls.lock().unwrap();
            calls.iter().filter_map(|c| if let Call::Send(ev) = c { Some(ev.clone()) } else { None }).collect()
        }
        pub fn last_event(&self) -> Option<OverlayEvent> {
            self.events().pop()
        }
        pub fn visible(&self) -> bool {
            let calls = self.calls.lock().unwrap();
            calls.iter().rev().find(|c| !matches!(c, Call::Send(_))) == Some(&Call::Show)
        }
    }

    impl OverlayWindow for FakeWindow {
        fn send(&self, ev: &OverlayEvent) {
            self.calls.lock().unwrap().push(Call::Send(ev.clone()));
        }
        fn show(&self) {
            self.calls.lock().unwrap().push(Call::Show);
        }
        fn hide(&self) {
            if let Some(f) = self.on_hide.lock().unwrap().as_ref() {
                f();
            }
            self.calls.lock().unwrap().push(Call::Hide);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::fake::{Call, FakeWindow};
    use super::*;

    fn overlay() -> (Arc<FakeWindow>, Overlay) {
        let w = Arc::new(FakeWindow::default());
        (w.clone(), Overlay::new(w))
    }

    #[test]
    fn idle_hides_and_everything_else_shows() {
        let (w, o) = overlay();
        o.emit(OverlayEvent::Recording { locked: false, warning: None });
        assert!(w.visible());
        o.emit(OverlayEvent::Processing);
        assert!(w.visible());
        o.emit(OverlayEvent::Idle);
        assert!(!w.visible());
        o.toast(ToastLevel::Copied, "copié", Some("texte".into()), Some(4));
        assert!(w.visible());
        assert_eq!(
            w.last_event(),
            Some(OverlayEvent::Toast { level: ToastLevel::Copied, message: "copié".into(), preview: Some("texte".into()), dictation_id: Some(4) })
        );
        assert_eq!(w.calls.lock().unwrap()[0], Call::Send(OverlayEvent::Recording { locked: false, warning: None }));
    }

    #[test]
    fn dismiss_hides_a_toast_or_idle_overlay() {
        let (w, o) = overlay();
        o.toast(ToastLevel::Info, "x", None, None);
        o.dismiss();
        assert!(!w.visible());
        // Now idle: a second dismiss hides again (harmless) and keeps the state idle.
        o.dismiss();
        assert_eq!(w.calls.lock().unwrap().iter().filter(|c| **c == Call::Hide).count(), 2);
    }

    #[test]
    fn late_dismiss_does_not_hide_recording_or_processing() {
        for ev in [OverlayEvent::Recording { locked: false, warning: None }, OverlayEvent::Recording { locked: true, warning: None }, OverlayEvent::Processing] {
            let (w, o) = overlay();
            o.emit(ev);
            o.dismiss();
            assert!(w.visible());
            assert!(!w.calls.lock().unwrap().contains(&Call::Hide));
        }
    }

    #[test]
    fn dismiss_racing_a_new_recording_never_hides_the_pill() {
        // The dismiss is inside hide() when the recording starts: the emit must wait for it, so the
        // recording's show() comes last and the pill stays visible.
        let (w, o) = overlay();
        let o = Arc::new(o);
        o.toast(ToastLevel::Info, "x", None, None);
        let (in_hide_tx, in_hide_rx) = mpsc::channel();
        *w.on_hide.lock().unwrap() = Some(Box::new(move || {
            let _ = in_hide_tx.send(());
            std::thread::sleep(Duration::from_millis(50));
        }));
        let o2 = o.clone();
        let dismiss = std::thread::spawn(move || o2.dismiss());
        in_hide_rx.recv().unwrap();
        o.emit(OverlayEvent::Recording { locked: false, warning: None });
        dismiss.join().unwrap();
        assert!(w.visible(), "{:?}", w.calls.lock().unwrap());
        let calls = w.calls.lock().unwrap();
        let n = calls.len();
        assert_eq!(calls[n - 3..], [Call::Hide, Call::Send(OverlayEvent::Recording { locked: false, warning: None }), Call::Show]);
    }

    #[test]
    fn events_serialize_for_the_overlay_webview() {
        let ev = OverlayEvent::Toast { level: ToastLevel::Uncertain, message: "m".into(), preview: None, dictation_id: Some(2) };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::json!({"kind": "toast", "level": "uncertain", "message": "m", "preview": null, "dictation_id": 2})
        );
        assert_eq!(serde_json::to_value(OverlayEvent::Recording { locked: true, warning: None }).unwrap(), serde_json::json!({"kind": "recording", "locked": true, "warning": null}));
    }

    #[test]
    fn overlay_sits_bottom_centre_above_the_margin() {
        assert_eq!(overlay_position((0, 0), (1920, 1080), (320, 80), 1.0, 80.0), (800, 920));
        // Scaled margin, secondary-monitor origin.
        assert_eq!(overlay_position((-1920, 100), (1920, 1080), (320, 80), 1.5, 80.0), (-1120, 980));
    }
}
