//! Overlay state machine: what the pill shows, and whether a dismiss from the overlay may hide it.
//! The window itself is behind `OverlayWindow` (Tauri webview + Win32 in the app, a fake in tests).
use std::sync::{Arc, Mutex};

use scribe_core::lang::Language;
use scribe_core::pipeline::ProviderError;
use scribe_core::style::{self, Style};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastLevel {
    Info,
    Uncertain,
    Copied,
    Error,
}

/// What a toast says; the overlay translates the code (`Notice` in src/lib/api.ts). `correction`: why the text was
/// not corrected; `detail`: untranslated technical text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Notice {
    /// Clean paste of a dictation with a keyword style: the toast hosts its badge.
    Pasted,
    PastedWithoutCorrection {
        correction: ProviderError,
    },
    PastedUncertain {
        correction: Option<ProviderError>,
    },
    Copied {
        correction: Option<ProviderError>,
    },
    NotPasted {
        correction: Option<ProviderError>,
    },
    ClipboardUnavailable {
        correction: Option<ProviderError>,
    },
    TranscriptionFailed {
        error: ProviderError,
    },
    AudioEncodingFailed {
        detail: String,
    },
    MicrophoneUnavailable {
        detail: String,
    },
    RecordingLost {
        detail: String,
    },
    HookUnavailable {
        detail: String,
    },
    UpdateAvailable {
        version: String,
    },
    /// A style switch replaced the pasted text in place.
    Restyled,
    /// A style switch could not replace the pasted text: the new text is in the clipboard.
    RestyleCopied,
    /// A style switch could not rewrite the text; nothing changed.
    RestyleFailed {
        error: ProviderError,
    },
}

/// Shown under the pill for the whole recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingWarning {
    /// Exclusive access to the microphone was asked for but not obtained.
    SharedMicrophone,
}

/// `style`: the keyword style of the dictation, when one was detected (never the default style).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    Idle,
    Recording {
        locked: bool,
        warning: Option<RecordingWarning>,
        style: Option<StyleBadge>,
    },
    Processing {
        style: Option<StyleBadge>,
    },
    Toast {
        level: ToastLevel,
        message: Notice,
        preview: Option<String>,
        dictation_id: Option<i64>,
        style: Option<StyleBadge>,
    },
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

/// A style as the overlay and the style pickers show it: its id and its name in the UI language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StyleBadge {
    pub id: String,
    pub name: String,
}

impl StyleBadge {
    pub fn of(style: &Style, language: Language) -> Self {
        Self { id: style.id.clone(), name: style::display_name(style, language) }
    }

    /// The badge of a keyword style; None for the default style or an unknown id.
    pub fn keyword(styles: &[Style], id: Option<&str>, language: Language) -> Option<Self> {
        style::find(styles, id?).filter(|s| s.keyword.is_some()).map(|s| Self::of(s, language))
    }
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
        OverlayEvent::Processing { .. } => Kind::Processing,
        OverlayEvent::Toast { .. } => Kind::Toast,
    }
}

pub struct Overlay {
    window: Arc<dyn OverlayWindow>,
    /// Kind of the last event emitted and, for a toast, its dictation. The lock is held across the window calls,
    /// so a dismiss and an emit never interleave: a toast expiring while the user starts speaking cannot hide the
    /// new pill.
    last: Mutex<(Kind, Option<i64>)>,
}

impl Overlay {
    pub fn new(window: Arc<dyn OverlayWindow>) -> Self {
        Self { window, last: Mutex::new((Kind::Idle, None)) }
    }

    pub fn emit(&self, ev: OverlayEvent) {
        self.show(&mut self.last.lock().unwrap(), ev);
    }

    /// Emits a late result about `dictation_id` only while the overlay is idle or still shows that dictation's
    /// toast: it never covers a recording, a processing state or another dictation's toast. Returns whether it did.
    pub fn emit_for_dictation(&self, dictation_id: i64, ev: OverlayEvent) -> bool {
        let mut last = self.last.lock().unwrap();
        let free = Self::free_for(*last, dictation_id);
        if free {
            self.show(&mut last, ev);
        }
        free
    }

    /// Whether [`Overlay::emit_for_dictation`] would show a result about `dictation_id` now.
    pub fn free_for_dictation(&self, dictation_id: i64) -> bool {
        Self::free_for(*self.last.lock().unwrap(), dictation_id)
    }

    fn free_for(last: (Kind, Option<i64>), dictation_id: i64) -> bool {
        match last {
            (Kind::Idle, _) => true,
            (Kind::Toast, shown) => shown == Some(dictation_id),
            _ => false,
        }
    }

    fn show(&self, last: &mut (Kind, Option<i64>), ev: OverlayEvent) {
        let dictation_id = match &ev {
            OverlayEvent::Toast { dictation_id, .. } => *dictation_id,
            _ => None,
        };
        *last = (kind_of(&ev), dictation_id);
        self.window.send(&ev);
        if last.0 == Kind::Idle {
            self.window.hide()
        } else {
            self.window.show()
        }
    }

    pub fn toast(&self, level: ToastLevel, message: Notice, preview: Option<String>, dictation_id: Option<i64>) {
        self.emit(OverlayEvent::Toast { level, message, preview, dictation_id, style: None });
    }

    /// Hides the overlay on behalf of the user/overlay (toast closed or expired), unless the backend
    /// has since moved on to a recording or processing state.
    pub fn dismiss(&self) {
        let mut last = self.last.lock().unwrap();
        if matches!(last.0, Kind::Toast | Kind::Idle) {
            *last = (Kind::Idle, None);
            self.window.hide();
        }
    }
}

/// Bottom-centre of the monitor, `margin_px` (logical) above its bottom edge, in physical pixels.
pub fn overlay_position(
    monitor_pos: (i32, i32),
    monitor_size: (u32, u32),
    window_size: (u32, u32),
    scale: f64,
    margin_px: f64,
) -> (i32, i32) {
    let margin = (margin_px * scale) as i32;
    let x = monitor_pos.0 + (monitor_size.0 as i32 - window_size.0 as i32) / 2;
    let y = monitor_pos.1 + monitor_size.1 as i32 - window_size.1 as i32 - margin;
    (x, y)
}

/// `overlay-pointer` event (`OverlayPointer` in src/overlay/model.ts): where the native tracking sees the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OverlayPointer {
    pub inside: bool,
    pub button: Option<usize>,
}

impl From<scribe_platform::overlay_pointer::PointerState> for OverlayPointer {
    fn from(s: scribe_platform::overlay_pointer::PointerState) -> Self {
        Self { inside: s.inside, button: s.over }
    }
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

    use serde_json::json;

    use super::fake::{Call, FakeWindow};
    use super::*;

    fn copied() -> Notice {
        Notice::Copied { correction: None }
    }

    fn slack() -> StyleBadge {
        StyleBadge { id: "slack".into(), name: "Slack".into() }
    }

    #[test]
    fn badges_only_for_keyword_styles_named_in_the_ui_language() {
        let styles = style::catalog();
        let mail = |l| StyleBadge::keyword(&styles, Some("mail"), l);
        assert_eq!(mail(Language::En), Some(StyleBadge { id: "mail".into(), name: "Email".into() }));
        assert_eq!(mail(Language::Fr), Some(StyleBadge { id: "mail".into(), name: "Mail".into() }));
        assert_eq!(StyleBadge::keyword(&styles, Some(style::DEFAULT_STYLE_ID), Language::En), None);
        assert_eq!(StyleBadge::keyword(&styles, Some("gone"), Language::En), None);
        assert_eq!(StyleBadge::keyword(&styles, None, Language::En), None);
    }

    fn overlay() -> (Arc<FakeWindow>, Overlay) {
        let w = Arc::new(FakeWindow::default());
        (w.clone(), Overlay::new(w))
    }

    #[test]
    fn idle_hides_and_everything_else_shows() {
        let (w, o) = overlay();
        o.emit(OverlayEvent::Recording { locked: false, warning: None, style: None });
        assert!(w.visible());
        o.emit(OverlayEvent::Processing { style: None });
        assert!(w.visible());
        o.emit(OverlayEvent::Idle);
        assert!(!w.visible());
        o.toast(ToastLevel::Copied, copied(), Some("texte".into()), Some(4));
        assert!(w.visible());
        assert_eq!(
            w.last_event(),
            Some(OverlayEvent::Toast {
                level: ToastLevel::Copied,
                message: copied(),
                preview: Some("texte".into()),
                dictation_id: Some(4),
                style: None
            })
        );
        assert_eq!(
            w.calls.lock().unwrap()[0],
            Call::Send(OverlayEvent::Recording { locked: false, warning: None, style: None })
        );
    }

    #[test]
    fn dismiss_hides_a_toast_or_idle_overlay() {
        let (w, o) = overlay();
        o.toast(ToastLevel::Info, copied(), None, None);
        o.dismiss();
        assert!(!w.visible());
        // Now idle: a second dismiss hides again (harmless) and keeps the state idle.
        o.dismiss();
        assert_eq!(w.calls.lock().unwrap().iter().filter(|c| **c == Call::Hide).count(), 2);
    }

    #[test]
    fn late_dismiss_does_not_hide_recording_or_processing() {
        for ev in [
            OverlayEvent::Recording { locked: false, warning: None, style: None },
            OverlayEvent::Recording { locked: true, warning: None, style: None },
            OverlayEvent::Processing { style: None },
        ] {
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
        o.toast(ToastLevel::Info, copied(), None, None);
        let (in_hide_tx, in_hide_rx) = mpsc::channel();
        *w.on_hide.lock().unwrap() = Some(Box::new(move || {
            let _ = in_hide_tx.send(());
            std::thread::sleep(Duration::from_millis(50));
        }));
        let o2 = o.clone();
        let dismiss = std::thread::spawn(move || o2.dismiss());
        in_hide_rx.recv().unwrap();
        o.emit(OverlayEvent::Recording { locked: false, warning: None, style: None });
        dismiss.join().unwrap();
        assert!(w.visible(), "{:?}", w.calls.lock().unwrap());
        let calls = w.calls.lock().unwrap();
        let n = calls.len();
        assert_eq!(
            calls[n - 3..],
            [Call::Hide, Call::Send(OverlayEvent::Recording { locked: false, warning: None, style: None }), Call::Show]
        );
    }

    fn restyled(id: i64) -> OverlayEvent {
        OverlayEvent::Toast {
            level: ToastLevel::Info,
            message: Notice::Restyled,
            preview: None,
            dictation_id: Some(id),
            style: None,
        }
    }

    #[test]
    fn a_late_dictation_result_shows_only_on_idle_or_on_that_dictations_toast() {
        let (w, o) = overlay();
        assert!(o.emit_for_dictation(4, restyled(4)), "idle");
        assert_eq!(w.last_event(), Some(restyled(4)));
        assert!(w.visible());
        assert!(o.emit_for_dictation(4, restyled(4)), "its own toast");
        assert!(o.free_for_dictation(4));
        o.toast(ToastLevel::Copied, copied(), None, Some(5));
        assert!(!o.free_for_dictation(4));
        assert!(!o.emit_for_dictation(4, restyled(4)), "a newer dictation's toast");
        o.toast(ToastLevel::Error, copied(), None, None);
        assert!(!o.emit_for_dictation(4, restyled(4)), "a toast of no dictation");
        for ev in [
            OverlayEvent::Recording { locked: false, warning: None, style: None },
            OverlayEvent::Processing { style: None },
        ] {
            o.emit(ev.clone());
            assert!(!o.emit_for_dictation(4, restyled(4)));
            assert_eq!(w.last_event(), Some(ev));
            assert!(w.visible());
        }
        o.toast(ToastLevel::Copied, copied(), None, Some(4));
        o.dismiss();
        assert!(o.emit_for_dictation(4, restyled(4)), "dismissed: idle again");
    }

    #[test]
    fn events_serialize_for_the_overlay_webview() {
        let ev = OverlayEvent::Toast {
            level: ToastLevel::Uncertain,
            message: Notice::PastedUncertain { correction: Some(ProviderError::Timeout) },
            preview: None,
            dictation_id: Some(2),
            style: Some(slack()),
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            json!({
                "kind": "toast",
                "level": "uncertain",
                "message": {"code": "pasted_uncertain", "correction": {"code": "timeout"}},
                "preview": null,
                "dictation_id": 2,
                "style": {"id": "slack", "name": "Slack"}
            })
        );
        assert_eq!(
            serde_json::to_value(OverlayEvent::Recording { locked: true, warning: None, style: None }).unwrap(),
            json!({"kind": "recording", "locked": true, "warning": null, "style": null})
        );
        assert_eq!(
            serde_json::to_value(OverlayEvent::Recording {
                locked: false,
                warning: Some(RecordingWarning::SharedMicrophone),
                style: Some(slack()),
            })
            .unwrap(),
            json!({
                "kind": "recording",
                "locked": false,
                "warning": "shared_microphone",
                "style": {"id": "slack", "name": "Slack"}
            })
        );
        assert_eq!(
            serde_json::to_value(OverlayEvent::Processing { style: Some(slack()) }).unwrap(),
            json!({"kind": "processing", "style": {"id": "slack", "name": "Slack"}})
        );
        assert_eq!(
            serde_json::to_value(OverlayEvent::Processing { style: None }).unwrap(),
            json!({"kind": "processing", "style": null})
        );
    }

    #[test]
    fn pointer_state_serializes_for_the_overlay_webview() {
        use scribe_platform::overlay_pointer::PointerState;
        let pointer = |inside, over| serde_json::to_value(OverlayPointer::from(PointerState { inside, over })).unwrap();
        assert_eq!(pointer(true, Some(2)), json!({"inside": true, "button": 2}));
        assert_eq!(pointer(false, None), json!({"inside": false, "button": null}));
    }

    #[test]
    fn notices_serialize_as_codes_for_the_overlay() {
        let auth = || ProviderError::Auth;
        let cases = [
            (Notice::Pasted, json!({"code": "pasted"})),
            (
                Notice::PastedWithoutCorrection { correction: auth() },
                json!({"code": "pasted_without_correction", "correction": {"code": "auth"}}),
            ),
            (Notice::Copied { correction: None }, json!({"code": "copied", "correction": null})),
            (
                Notice::NotPasted { correction: Some(auth()) },
                json!({"code": "not_pasted", "correction": {"code": "auth"}}),
            ),
            (
                Notice::ClipboardUnavailable { correction: None },
                json!({"code": "clipboard_unavailable", "correction": null}),
            ),
            (
                Notice::TranscriptionFailed { error: auth() },
                json!({"code": "transcription_failed", "error": {"code": "auth"}}),
            ),
            (
                Notice::AudioEncodingFailed { detail: "d".into() },
                json!({"code": "audio_encoding_failed", "detail": "d"}),
            ),
            (
                Notice::MicrophoneUnavailable { detail: "d".into() },
                json!({"code": "microphone_unavailable", "detail": "d"}),
            ),
            (Notice::RecordingLost { detail: "d".into() }, json!({"code": "recording_lost", "detail": "d"})),
            (Notice::HookUnavailable { detail: "d".into() }, json!({"code": "hook_unavailable", "detail": "d"})),
            (
                Notice::UpdateAvailable { version: "1.2.0".into() },
                json!({"code": "update_available", "version": "1.2.0"}),
            ),
            (Notice::Restyled, json!({"code": "restyled"})),
            (Notice::RestyleCopied, json!({"code": "restyle_copied"})),
            (Notice::RestyleFailed { error: auth() }, json!({"code": "restyle_failed", "error": {"code": "auth"}})),
        ];
        for (notice, expected) in cases {
            assert_eq!(serde_json::to_value(&notice).unwrap(), expected);
        }
    }

    #[test]
    fn overlay_sits_bottom_centre_above_the_margin() {
        assert_eq!(overlay_position((0, 0), (1920, 1080), (320, 80), 1.0, 80.0), (800, 920));
        // Scaled margin, secondary-monitor origin.
        assert_eq!(overlay_position((-1920, 100), (1920, 1080), (320, 80), 1.5, 80.0), (-1120, 980));
    }
}
