use std::sync::{mpsc, Arc};
use std::time::Duration;

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

/// Mutes the computer's audio outputs while dictating, so speakers do not disturb the transcription. Never fails:
/// an output that cannot be read or changed is skipped and logged.
pub trait SystemMute: Send + Sync {
    /// Mutes every active output that is not muted yet; returns the stable ids of those it muted (an output
    /// already muted by the user is left alone and not returned).
    fn mute_all(&self) -> Vec<String>;
    /// Unmutes the given outputs (ids from `mute_all`) that are still muted. An output that is gone or was
    /// unmuted meanwhile is left alone.
    fn restore(&self, ids: &[String]);
}

pub trait KeySender: Send + Sync {
    /// Simulates Ctrl+V (Cmd+V on macOS).
    fn send_paste(&self) -> Result<(), String>;
}

/// Reads the text of the focused field, to check that a paste really landed. None when it cannot be read
/// (no text or value exposed, a password field, an accessibility error). The text is only compared in memory.
pub trait FieldReader: Send + Sync {
    fn focused_text(&self) -> Option<String>;
}

/// Whether the dictated text appeared in the focused field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Inserted,
    /// The field is readable and did not change: the paste did not land.
    NotInserted,
    /// The field cannot be read, or changed in a way that does not show the text (an app that shows a long
    /// paste as a placeholder, an auto-format): fall back on the focus prediction.
    Unknown,
}

/// Letters and digits only: what survives an app's rendering of the pasted text (a terminal wraps lines and
/// draws box borders around them, an editor normalises line endings and spaces).
fn fingerprint(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Below this many letters and digits the text is too common to be recognised: a « 1 » or « ok » shows up in
/// any terminal output that scrolls by.
pub const MIN_VERIFIABLE_CHARS: usize = 4;

/// Compares the focused field before and after the paste. One more occurrence of the text than before means
/// it landed (dictating the same sentence twice still counts).
pub fn verify(before: Option<&str>, after: Option<&str>, text: &str) -> Verdict {
    let needle = fingerprint(text);
    let (Some(before), Some(after)) = (before, after) else {
        return Verdict::Unknown;
    };
    if needle.chars().count() < MIN_VERIFIABLE_CHARS {
        return Verdict::Unknown;
    }
    if before == after {
        // Only a field that shows some text proves the paste missed. Web editors (VS Code's terminal and Monaco,
        // Google Docs) take the paste in a hidden, empty helper field and apply it to their own model: it stays
        // empty although the text landed.
        return if fingerprint(before).is_empty() { Verdict::Unknown } else { Verdict::NotInserted };
    }
    // Changed: the text shows up, or the app rendered it otherwise (a placeholder for a long paste, a spacing or
    // punctuation fix): only the first case is a proof.
    if occurrences(&fingerprint(after), &needle) > occurrences(&fingerprint(before), &needle) {
        Verdict::Inserted
    } else {
        Verdict::Unknown
    }
}

/// Bounds every read: UI Automation calls into the target app and can block for seconds when it is hung. A read
/// that does not answer in time counts as unreadable (the thread is left to finish on its own).
pub struct TimedFieldReader {
    inner: Arc<dyn FieldReader>,
    timeout: Duration,
}

impl TimedFieldReader {
    pub fn new(inner: Arc<dyn FieldReader>, timeout_ms: u64) -> Self {
        Self { inner, timeout: Duration::from_millis(timeout_ms) }
    }
}

impl FieldReader for TimedFieldReader {
    fn focused_text(&self) -> Option<String> {
        let (tx, rx) = mpsc::channel();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let _ = tx.send(inner.focused_text());
        });
        rx.recv_timeout(self.timeout).ok().flatten()
    }
}

/// How often and how many times the field is re-read after Ctrl+V: apps apply a paste asynchronously.
pub const VERIFY_INTERVAL_MS: u64 = 100;
pub const VERIFY_ATTEMPTS: u64 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertResult {
    Pasted,
    PastedUncertain,
    ClipboardOnly,
    PasteFailed,
    /// Ctrl+V was sent but the field did not change: the text stays in the clipboard.
    NotPasted,
    ClipboardFailed,
}

impl InsertResult {
    pub fn outcome(&self) -> Outcome {
        match self {
            InsertResult::Pasted => Outcome::Pasted,
            InsertResult::PastedUncertain => Outcome::PastedUncertain,
            InsertResult::ClipboardOnly | InsertResult::PasteFailed | InsertResult::NotPasted => Outcome::Clipboard,
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
    field: &dyn FieldReader,
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
            let before = field.focused_text();
            if clipboard.write_text(text).is_err() {
                return InsertResult::ClipboardFailed;
            }
            if keys.send_paste().is_err() {
                return InsertResult::PasteFailed;
            }
            // Only a field readable before the paste can be compared. Re-read it while it has not changed (the app
            // may apply the paste late); stop as soon as it changed or can no longer be read.
            let mut verdict = Verdict::Unknown;
            let mut waited = 0;
            if before.is_some() {
                for _ in 0..VERIFY_ATTEMPTS {
                    sleep(VERIFY_INTERVAL_MS);
                    waited += VERIFY_INTERVAL_MS;
                    let Some(after) = field.focused_text() else {
                        verdict = Verdict::Unknown;
                        break;
                    };
                    verdict = verify(before.as_deref(), Some(&after), text);
                    if verdict != Verdict::NotInserted {
                        break;
                    }
                }
            }
            if verdict == Verdict::NotInserted {
                // Keep the text in the clipboard: the user pastes it by hand.
                return InsertResult::NotPasted;
            }
            // Target apps read the clipboard asynchronously after Ctrl+V; the reading time counts.
            if restore_delay_ms > waited {
                sleep(restore_delay_ms - waited);
            }
            // If the user copied something else meanwhile, leave it alone.
            if previous != ClipboardContent::Unsupported && is_our_text(&clipboard.read(), text) {
                let _ = clipboard.restore(&previous);
            }
            match verdict {
                Verdict::Inserted => InsertResult::Pasted,
                _ if uncertain => InsertResult::PastedUncertain,
                _ => InsertResult::Pasted,
            }
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

    /// A field that cannot be read (no accessibility text): the paste is not verified.
    struct NoField;
    impl FieldReader for NoField {
        fn focused_text(&self) -> Option<String> {
            None
        }
    }

    /// Returns the scripted readings in order, then repeats the last one.
    struct FakeField(Mutex<Vec<Option<String>>>);
    impl FakeField {
        fn new(readings: &[Option<&str>]) -> Self {
            Self(Mutex::new(readings.iter().rev().map(|r| r.map(String::from)).collect()))
        }
    }
    impl FieldReader for FakeField {
        fn focused_text(&self) -> Option<String> {
            let mut v = self.0.lock().unwrap();
            if v.len() > 1 { v.pop().unwrap() } else { v[0].clone() }
        }
    }

    /// The sleeps `perform` asked for, in order.
    #[derive(Default)]
    struct Delays(Mutex<Vec<u64>>);
    impl Delays {
        fn push(&self, ms: u64) {
            self.0.lock().unwrap().push(ms);
        }
        fn get(&self) -> Vec<u64> {
            self.0.lock().unwrap().clone()
        }
    }

    #[test]
    fn verify_counts_the_text_in_the_field_before_and_after() {
        let some = Some;
        assert_eq!(verify(some("Bonjour"), some("Bonjour, à demain."), "à demain."), Verdict::Inserted);
        assert_eq!(verify(some("à demain"), some("à demain"), "À demain"), Verdict::NotInserted, "an unchanged field");
        assert_eq!(verify(some("à demain"), some("à demain\nà demain"), "à demain"), Verdict::Inserted, "same text twice");
        assert_eq!(verify(some("abc"), some("abc"), "à demain"), Verdict::NotInserted);
        assert_eq!(verify(some("a,b c"), some("a, b c."), "a, b, c"), Verdict::Unknown, "too short to recognise");
        assert_eq!(verify(some("abc"), some("abc [Pasted text #1 +3 lines]"), "à demain"), Verdict::Unknown);
        assert_eq!(verify(None, some("à demain"), "à demain"), Verdict::Unknown, "nothing to compare with");
        assert_eq!(verify(some(""), None, "à demain"), Verdict::Unknown);
        assert_eq!(verify(some(""), some("..."), "..."), Verdict::Unknown, "no letter to look for");
    }

    #[test]
    fn an_empty_field_that_stays_empty_does_not_prove_a_missed_paste() {
        // xterm.js, Monaco, Google Docs: the focused element is a hidden helper field the page empties itself.
        assert_eq!(verify(Some(""), Some(""), "Bonjour Scribe."), Verdict::Unknown);
        assert_eq!(verify(Some(" \n"), Some(" \n"), "Bonjour Scribe."), Verdict::Unknown);
    }

    #[test]
    fn a_change_that_does_not_show_the_text_proves_nothing() {
        // Same letters, other spacing or punctuation (a selection replaced by its own text, an auto-format).
        assert_eq!(verify(Some("Bonjour Scribe"), Some("Bonjour, Scribe."), "Bonjour, Scribe."), Verdict::Unknown);
    }

    #[test]
    fn short_texts_are_not_verified() {
        // A terminal printing « 12 » or « oui » would otherwise confirm a paste of « 12 » or « oui ».
        for text in ["12", "oui", "à", "a b c"] {
            assert_eq!(verify(Some("$ "), Some("$ 12 oui à a b c"), text), Verdict::Unknown, "{text}");
        }
        assert_eq!(verify(Some("$ "), Some("$ 1234"), "1234"), Verdict::Inserted, "four digits are enough");
    }

    #[test]
    fn verify_sees_the_text_through_a_terminal_rendering() {
        let before = "│ > │\n";
        let after = "│ > Ceci est un test de mon     │\n│   application, sur deux lignes. │\n";
        assert_eq!(
            verify(Some(before), Some(after), "Ceci est un test de mon application, sur deux lignes."),
            Verdict::Inserted
        );
    }

    #[test]
    fn a_paste_seen_in_the_field_is_confirmed_even_when_the_focus_was_uncertain() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let field = FakeField::new(&[Some("début"), Some("début"), Some("début dicté")]);
        let delays = Delays::default();
        let r = perform(InsertPlan::Paste { uncertain: true }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| delays.push(ms));
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(delays.get(), vec![100, 100], "stops reading once seen; the restore delay already elapsed");
        assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn a_paste_that_did_not_land_leaves_the_text_in_the_clipboard() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let delays = Delays::default();
        let field = FakeField::new(&[Some("début")]);
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| delays.push(ms));
        assert_eq!(r, InsertResult::NotPasted);
        assert_eq!(r.outcome(), Outcome::Clipboard);
        assert_eq!(delays.get().len() as u64, VERIFY_ATTEMPTS, "waits for a slow app before giving up");
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn an_unclear_change_falls_back_on_the_focus_prediction() {
        for (uncertain, expected) in [(false, InsertResult::Pasted), (true, InsertResult::PastedUncertain)] {
            let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
            let field = FakeField::new(&[Some("> "), Some("> [Pasted text #1 +3 lines]")]);
            let delays = Delays::default();
            let r = perform(InsertPlan::Paste { uncertain }, "dicté", &cb, &FakeKeys::new(true), &field, 1000, &|ms| delays.push(ms));
            assert_eq!(r, expected);
            let delays = delays.get();
            assert_eq!(delays.iter().sum::<u64>(), 1000, "the restore delay counts the reading time: {delays:?}");
            assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
        }
    }

    #[test]
    fn a_field_that_becomes_unreadable_stops_the_verification() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let delays = Delays::default();
        let field = FakeField::new(&[Some("début"), None, Some("début")]);
        let r = perform(InsertPlan::Paste { uncertain: true }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| delays.push(ms));
        assert_eq!(r, InsertResult::PastedUncertain, "falls back on the prediction");
        assert_eq!(delays.get(), vec![100, 50], "one read, then the rest of the restore delay");
        assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
    }

    struct SlowField(u64);
    impl FieldReader for SlowField {
        fn focused_text(&self) -> Option<String> {
            std::thread::sleep(Duration::from_millis(self.0));
            Some("texte".into())
        }
    }

    #[test]
    fn a_hung_app_cannot_hold_the_read() {
        let fast = TimedFieldReader::new(Arc::new(SlowField(0)), 2_000);
        assert_eq!(fast.focused_text().as_deref(), Some("texte"));
        let slow = TimedFieldReader::new(Arc::new(SlowField(2_000)), 20);
        let start = std::time::Instant::now();
        assert_eq!(slow.focused_text(), None);
        assert!(start.elapsed() < Duration::from_millis(1_000), "{:?}", start.elapsed());
    }

    #[test]
    fn a_field_unreadable_before_the_paste_is_not_verified() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let delays = Delays::default();
        let field = FakeField::new(&[None, Some("dicté")]);
        let r = perform(InsertPlan::Paste { uncertain: true }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| delays.push(ms));
        assert_eq!(r, InsertResult::PastedUncertain);
        assert_eq!(delays.get(), vec![150]);
    }

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
        let delays = Delays::default();
        let r = perform(InsertPlan::Paste { uncertain: false }, "nouveau", &cb, &keys, &NoField, 150, &|ms| delays.push(ms));
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(*keys.sent.lock().unwrap(), 1);
        assert_eq!(delays.get(), vec![150]);
        assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn paste_restores_previous_image_and_multiline_text_with_crlf() {
        let img = ClipboardContent::Image { width: 1, height: 1, rgba: vec![1, 2, 3, 4] };
        let cb = FakeClipboard { content: Mutex::new(Some(img.clone())), crlf: true, fail_write: false };
        let r = perform(InsertPlan::Paste { uncertain: true }, "ligne 1\nligne 2", &cb, &FakeKeys::new(true), &NoField, 0, &no_sleep);
        assert_eq!(r, InsertResult::PastedUncertain);
        assert_eq!(cb.get(), img);
    }

    #[test]
    fn user_copy_during_paste_is_not_overwritten() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), &NoField, 150, &|_| {
            cb.write_text("copié par l'utilisateur").unwrap();
        });
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(cb.get(), ClipboardContent::Text("copié par l'utilisateur".into()));
    }

    #[test]
    fn unsupported_previous_content_is_not_restored() {
        let cb = FakeClipboard::with(ClipboardContent::Unsupported);
        perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), &NoField, 0, &no_sleep);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn paste_key_failure_leaves_text_in_clipboard() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(false), &NoField, 0, &no_sleep);
        assert_eq!(r, InsertResult::PasteFailed);
        assert_eq!(r.outcome(), Outcome::Clipboard);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn clipboard_only_never_sends_keys() {
        let cb = FakeClipboard::default();
        let keys = FakeKeys::new(true);
        assert_eq!(perform(InsertPlan::ClipboardOnly, "dicté", &cb, &keys, &NoField, 0, &no_sleep), InsertResult::ClipboardOnly);
        assert_eq!(*keys.sent.lock().unwrap(), 0);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn clipboard_write_failure_is_reported() {
        let cb = FakeClipboard { fail_write: true, ..Default::default() };
        let r = perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), &NoField, 0, &no_sleep);
        assert_eq!(r, InsertResult::ClipboardFailed);
        assert_eq!(r.outcome(), Outcome::Error);
    }

    #[test]
    fn insert_results_map_to_stored_outcomes() {
        assert_eq!(InsertResult::Pasted.outcome(), Outcome::Pasted);
        assert_eq!(InsertResult::PastedUncertain.outcome(), Outcome::PastedUncertain);
        assert_eq!(InsertResult::ClipboardOnly.outcome(), Outcome::Clipboard);
        assert_eq!(InsertResult::PasteFailed.outcome(), Outcome::Clipboard);
        assert_eq!(InsertResult::NotPasted.outcome(), Outcome::Clipboard);
        assert_eq!(InsertResult::ClipboardFailed.outcome(), Outcome::Error);
    }

    #[test]
    fn clipboard_only_write_failure_is_reported_without_keys() {
        let cb = FakeClipboard { fail_write: true, ..Default::default() };
        let keys = FakeKeys::new(true);
        assert_eq!(perform(InsertPlan::ClipboardOnly, "dicté", &cb, &keys, &NoField, 0, &no_sleep), InsertResult::ClipboardFailed);
        assert_eq!(*keys.sent.lock().unwrap(), 0);
    }
}
