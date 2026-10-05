use crate::focus::{FocusSnapshot, FocusState};
use crate::model::Outcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPlan {
    Paste { uncertain: bool },
    ClipboardOnly,
}

/// `start` is captured when recording begins, `end` right before inserting.
pub fn decide(start: &FocusSnapshot, end: &FocusSnapshot) -> InsertPlan {
    if start.window_id.is_some() && start.window_id != end.window_id {
        return InsertPlan::ClipboardOnly;
    }
    match end.state {
        FocusState::Editable => InsertPlan::Paste { uncertain: false },
        FocusState::Unknown => InsertPlan::Paste { uncertain: true },
        FocusState::NotEditable => InsertPlan::ClipboardOnly,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardContent {
    Empty,
    Text(String),
    Image { width: usize, height: usize, rgba: Vec<u8> },
    /// Files or formats we cannot round-trip: never restored.
    Unsupported,
}

pub trait Clipboard: Send + Sync {
    fn read(&self) -> ClipboardContent;
    fn write_text(&self, text: &str) -> Result<(), String>;
    fn restore(&self, content: &ClipboardContent) -> Result<(), String>;
}

pub trait KeySender: Send + Sync {
    /// Simulates Ctrl+V (Cmd+V on macOS).
    fn send_paste(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertResult {
    Pasted,
    PastedUncertain,
    ClipboardOnly,
    PasteFailed,
    ClipboardFailed,
}

impl InsertResult {
    pub fn outcome(&self) -> Outcome {
        match self {
            InsertResult::Pasted => Outcome::Pasted,
            InsertResult::PastedUncertain => Outcome::PastedUncertain,
            InsertResult::ClipboardOnly | InsertResult::PasteFailed => Outcome::Clipboard,
            InsertResult::ClipboardFailed => Outcome::Error,
        }
    }
}

fn is_our_text(content: &ClipboardContent, text: &str) -> bool {
    matches!(content, ClipboardContent::Text(t) if t.replace("\r\n", "\n") == text.replace("\r\n", "\n"))
}

pub fn perform(
    plan: InsertPlan,
    text: &str,
    clipboard: &dyn Clipboard,
    keys: &dyn KeySender,
    restore_delay_ms: u64,
    sleep: &dyn Fn(u64),
) -> InsertResult {
    match plan {
        InsertPlan::ClipboardOnly => match clipboard.write_text(text) {
            Ok(()) => InsertResult::ClipboardOnly,
            Err(_) => InsertResult::ClipboardFailed,
        },
        InsertPlan::Paste { uncertain } => {
            let previous = clipboard.read();
            if clipboard.write_text(text).is_err() {
                return InsertResult::ClipboardFailed;
            }
            if keys.send_paste().is_err() {
                return InsertResult::PasteFailed;
            }
            // Target apps read the clipboard asynchronously after Ctrl+V.
            sleep(restore_delay_ms);
            // If the user copied something else meanwhile, leave it alone.
            if previous != ClipboardContent::Unsupported && is_our_text(&clipboard.read(), text) {
                let _ = clipboard.restore(&previous);
            }
            if uncertain { InsertResult::PastedUncertain } else { InsertResult::Pasted }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::{FocusSnapshot, FocusState};
    use std::sync::Mutex;

    fn snap(window: u64, state: FocusState) -> FocusSnapshot {
        FocusSnapshot { app_name: Some("App".into()), window_id: Some(window), state }
    }

    #[derive(Default)]
    struct FakeClipboard {
        content: Mutex<Option<ClipboardContent>>,
        /// Simulates the Windows clipboard returning CRLF line endings.
        crlf: bool,
        fail_write: bool,
    }
    impl FakeClipboard {
        fn with(c: ClipboardContent) -> Self {
            Self { content: Mutex::new(Some(c)), ..Default::default() }
        }
        fn get(&self) -> ClipboardContent {
            self.content.lock().unwrap().clone().unwrap_or(ClipboardContent::Empty)
        }
    }
    impl Clipboard for FakeClipboard {
        fn read(&self) -> ClipboardContent {
            match self.get() {
                ClipboardContent::Text(t) if self.crlf => ClipboardContent::Text(t.replace('\n', "\r\n")),
                c => c,
            }
        }
        fn write_text(&self, text: &str) -> Result<(), String> {
            if self.fail_write {
                return Err("verrouillé".into());
            }
            *self.content.lock().unwrap() = Some(ClipboardContent::Text(text.into()));
            Ok(())
        }
        fn restore(&self, c: &ClipboardContent) -> Result<(), String> {
            *self.content.lock().unwrap() = Some(c.clone());
            Ok(())
        }
    }

    struct FakeKeys {
        ok: bool,
        sent: Mutex<u32>,
    }
    impl FakeKeys {
        fn new(ok: bool) -> Self {
            Self { ok, sent: Mutex::new(0) }
        }
    }
    impl KeySender for FakeKeys {
        fn send_paste(&self) -> Result<(), String> {
            *self.sent.lock().unwrap() += 1;
            if self.ok { Ok(()) } else { Err("bloqué".into()) }
        }
    }

    fn no_sleep(_: u64) {}

    #[test]
    fn decide_by_focus_state() {
        assert_eq!(decide(&snap(1, FocusState::Unknown), &snap(1, FocusState::Editable)), InsertPlan::Paste { uncertain: false });
        assert_eq!(decide(&snap(1, FocusState::Editable), &snap(1, FocusState::Unknown)), InsertPlan::Paste { uncertain: true });
        assert_eq!(decide(&snap(1, FocusState::Editable), &snap(1, FocusState::NotEditable)), InsertPlan::ClipboardOnly);
    }

    #[test]
    fn window_changed_during_dictation_means_clipboard_only() {
        assert_eq!(decide(&snap(1, FocusState::Editable), &snap(2, FocusState::Editable)), InsertPlan::ClipboardOnly);
        let unknown_start = FocusSnapshot::unknown();
        assert_eq!(decide(&unknown_start, &snap(2, FocusState::Editable)), InsertPlan::Paste { uncertain: false });
    }

    #[test]
    fn paste_restores_previous_text_after_delay() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let keys = FakeKeys::new(true);
        let delays = Mutex::new(vec![]);
        let r = perform(InsertPlan::Paste { uncertain: false }, "nouveau", &cb, &keys, 150, &|ms| delays.lock().unwrap().push(ms));
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(*keys.sent.lock().unwrap(), 1);
        assert_eq!(*delays.lock().unwrap(), vec![150]);
        assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn paste_restores_previous_image_and_multiline_text_with_crlf() {
        let img = ClipboardContent::Image { width: 1, height: 1, rgba: vec![1, 2, 3, 4] };
        let cb = FakeClipboard { content: Mutex::new(Some(img.clone())), crlf: true, fail_write: false };
        let r = perform(InsertPlan::Paste { uncertain: true }, "ligne 1\nligne 2", &cb, &FakeKeys::new(true), 0, &no_sleep);
        assert_eq!(r, InsertResult::PastedUncertain);
        assert_eq!(cb.get(), img);
    }

    #[test]
    fn user_copy_during_paste_is_not_overwritten() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), 150, &|_| {
            cb.write_text("copié par l'utilisateur").unwrap();
        });
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(cb.get(), ClipboardContent::Text("copié par l'utilisateur".into()));
    }

    #[test]
    fn unsupported_previous_content_is_not_restored() {
        let cb = FakeClipboard::with(ClipboardContent::Unsupported);
        perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), 0, &no_sleep);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn paste_key_failure_leaves_text_in_clipboard() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(false), 0, &no_sleep);
        assert_eq!(r, InsertResult::PasteFailed);
        assert_eq!(r.outcome(), Outcome::Clipboard);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn clipboard_only_never_sends_keys() {
        let cb = FakeClipboard::default();
        let keys = FakeKeys::new(true);
        assert_eq!(perform(InsertPlan::ClipboardOnly, "dicté", &cb, &keys, 0, &no_sleep), InsertResult::ClipboardOnly);
        assert_eq!(*keys.sent.lock().unwrap(), 0);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn clipboard_write_failure_is_reported() {
        let cb = FakeClipboard { fail_write: true, ..Default::default() };
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), 0, &no_sleep);
        assert_eq!(r, InsertResult::ClipboardFailed);
        assert_eq!(r.outcome(), Outcome::Error);
    }

    #[test]
    fn insert_results_map_to_stored_outcomes() {
        assert_eq!(InsertResult::Pasted.outcome(), Outcome::Pasted);
        assert_eq!(InsertResult::PastedUncertain.outcome(), Outcome::PastedUncertain);
        assert_eq!(InsertResult::ClipboardOnly.outcome(), Outcome::Clipboard);
        assert_eq!(InsertResult::PasteFailed.outcome(), Outcome::Clipboard);
        assert_eq!(InsertResult::ClipboardFailed.outcome(), Outcome::Error);
    }

    #[test]
    fn clipboard_only_write_failure_is_reported_without_keys() {
        let cb = FakeClipboard { fail_write: true, ..Default::default() };
        let keys = FakeKeys::new(true);
        assert_eq!(perform(InsertPlan::ClipboardOnly, "dicté", &cb, &keys, 0, &no_sleep), InsertResult::ClipboardFailed);
        assert_eq!(*keys.sent.lock().unwrap(), 0);
    }
}
