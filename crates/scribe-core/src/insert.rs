use std::sync::{mpsc, Arc};
use std::time::Duration;

use unicode_segmentation::UnicodeSegmentation;

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
        FocusState::Editable | FocusState::Terminal => InsertPlan::Paste { uncertain: false },
        FocusState::Unknown => InsertPlan::Paste { uncertain: true },
        FocusState::NotEditable => InsertPlan::ClipboardOnly,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardContent {
    Empty,
    Text(String),
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Left,
    Right,
}

pub trait KeySender: Send + Sync {
    /// Simulates Ctrl+V (Cmd+V on macOS).
    fn send_paste(&self) -> Result<(), String>;

    /// Presses an arrow key `times` times, with Shift held when `shift`. Only used to select when the field cannot
    /// be selected through accessibility.
    fn press(&self, _key: NavKey, _shift: bool, _times: usize) -> Result<(), String> {
        Err("arrow keys not supported".into())
    }
}

/// Reads the text of the focused field, to check that a paste really landed. None when it cannot be read
/// (no text or value exposed, a password field, an accessibility error). The text is only compared in memory.
pub trait FieldReader: Send + Sync {
    fn focused_text(&self) -> Option<String>;

    /// The field's text up to the caret; None with a selection or when the caret cannot be read.
    fn before_caret(&self) -> Option<String> {
        None
    }

    fn selected_text(&self) -> Option<String> {
        None
    }

    /// Selects `suffix`, which ends at the caret.
    fn select_before_caret(&self, _suffix: &str) -> Selection {
        Selection::Unsupported
    }
}

/// What [`FieldReader::select_before_caret`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Selected,
    /// The field cannot be selected through accessibility.
    Unsupported,
    /// No answer in time: the selection may still land later.
    TimedOut,
}

impl Selection {
    /// What a reader answers: whether it selected.
    pub fn of(selected: bool) -> Self {
        if selected {
            Selection::Selected
        } else {
            Selection::Unsupported
        }
    }
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

    fn timed<T: Send + 'static>(&self, read: impl FnOnce(&dyn FieldReader) -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read(inner.as_ref()));
        });
        rx.recv_timeout(self.timeout).ok()
    }
}

impl FieldReader for TimedFieldReader {
    fn focused_text(&self) -> Option<String> {
        self.timed(|f| f.focused_text()).flatten()
    }

    fn before_caret(&self) -> Option<String> {
        self.timed(|f| f.before_caret()).flatten()
    }

    fn selected_text(&self) -> Option<String> {
        self.timed(|f| f.selected_text()).flatten()
    }

    fn select_before_caret(&self, suffix: &str) -> Selection {
        let suffix = suffix.to_string();
        self.timed(move |f| f.select_before_caret(&suffix)).unwrap_or(Selection::TimedOut)
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

/// Every line break as LF: CRLF (Win32 edits) and a lone CR (RichEdit).
fn eol(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_our_text(content: &ClipboardContent, text: &str) -> bool {
    matches!(content, ClipboardContent::Text(t) if eol(t) == eol(text))
}

/// Re-reads the field after Ctrl+V while it has not changed (the app may apply the paste late); stops as soon as
/// it changed or can no longer be read. Only a field readable before the paste can be compared. Returns the
/// verdict and the time waited.
fn await_paste(
    field: &dyn FieldReader,
    before: Option<&str>,
    sleep: &dyn Fn(u64),
    judge: impl Fn(&str, &str) -> Verdict,
) -> (Verdict, u64) {
    let Some(before) = before else {
        return (Verdict::Unknown, 0);
    };
    let mut waited = 0;
    for _ in 0..VERIFY_ATTEMPTS {
        sleep(VERIFY_INTERVAL_MS);
        waited += VERIFY_INTERVAL_MS;
        let Some(after) = field.focused_text() else {
            return (Verdict::Unknown, waited);
        };
        let verdict = judge(before, &after);
        if verdict != Verdict::NotInserted {
            return (verdict, waited);
        }
    }
    (Verdict::NotInserted, waited)
}

/// Puts `previous` back once the target app had time to read the clipboard, unless the user copied something
/// else meanwhile.
fn restore_clipboard(
    clipboard: &dyn Clipboard,
    previous: &ClipboardContent,
    text: &str,
    restore_delay_ms: u64,
    waited: u64,
    sleep: &dyn Fn(u64),
) {
    if restore_delay_ms > waited {
        sleep(restore_delay_ms - waited);
    }
    if *previous != ClipboardContent::Unsupported && is_our_text(&clipboard.read(), text) {
        let _ = clipboard.restore(previous);
    }
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
            let (verdict, waited) = await_paste(field, before.as_deref(), sleep, |b, a| verify(Some(b), Some(a), text));
            if verdict == Verdict::NotInserted {
                // Keep the text in the clipboard: the user pastes it by hand.
                return InsertResult::NotPasted;
            }
            restore_clipboard(clipboard, &previous, text, restore_delay_ms, waited, sleep);
            match verdict {
                Verdict::Inserted => InsertResult::Pasted,
                _ if uncertain => InsertResult::PastedUncertain,
                _ => InsertResult::Pasted,
            }
        }
    }
}

/// The last dictation pasted by Scribe, and where: what a style switch may replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedText {
    pub dictation_id: i64,
    pub focus: FocusSnapshot,
    pub text: String,
}

/// Why a style switch copies the new text instead of replacing the pasted one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceRefusal {
    /// A recording or its processing is in flight.
    Busy,
    NotLastPaste,
    FocusChanged,
    /// A terminal, an unknown or a non-editable field.
    NotEditable,
    CaretUnreadable,
    /// The pasted text no longer ends at the caret.
    TextMoved,
    TooLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacePlan {
    /// `suffix` is the field's own text (line endings as the field has them), `graphemes` its caret steps.
    Replace {
        suffix: String,
        graphemes: usize,
    },
    CopyOnly(ReplaceRefusal),
}

/// Beyond this many caret steps a Shift+Left burst takes too long and an app is likelier to drop some.
pub const MAX_SELECT_GRAPHEMES: usize = 2000;

/// How far the caret moves over `suffix`: UTF-16 units (accessibility ranges) and graphemes (arrow presses; a
/// CRLF is one step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretUnits {
    pub utf16: usize,
    pub graphemes: usize,
}

pub fn caret_units(suffix: &str) -> CaretUnits {
    CaretUnits { utf16: suffix.encode_utf16().count(), graphemes: suffix.graphemes(true).count() }
}

/// The end of `field` that reads as `text` once line breaks become LFs, without splitting a CRLF.
fn field_suffix<'a>(field: &'a str, text: &str) -> Option<&'a str> {
    let text = eol(text);
    if !eol(field).ends_with(&text) {
        return None;
    }
    let mut remaining = text.len();
    let mut start = field.len();
    while remaining > 0 {
        let (i, c) = field[..start].char_indices().next_back()?;
        if !(c == '\r' && field[start..].starts_with('\n')) {
            remaining -= c.len_utf8();
        }
        start = i;
    }
    if field[start..].starts_with('\n') && field[..start].ends_with('\r') {
        start -= 1;
    }
    Some(&field[start..])
}

/// Whether the text Scribe pasted last can be selected and replaced in place: same window and app, a field known
/// to be editable, nothing in flight, and the pasted text still right before the caret.
pub fn plan_replace(
    last: &PastedText,
    now: &FocusSnapshot,
    before_caret: Option<&str>,
    session_idle: bool,
    max_graphemes: usize,
) -> ReplacePlan {
    if !session_idle {
        return ReplacePlan::CopyOnly(ReplaceRefusal::Busy);
    }
    if last.focus.window_id.is_none() || last.focus.window_id != now.window_id || last.focus.app_name != now.app_name {
        return ReplacePlan::CopyOnly(ReplaceRefusal::FocusChanged);
    }
    if now.state != FocusState::Editable {
        return ReplacePlan::CopyOnly(ReplaceRefusal::NotEditable);
    }
    let Some(before_caret) = before_caret else {
        return ReplacePlan::CopyOnly(ReplaceRefusal::CaretUnreadable);
    };
    let Some(suffix) = field_suffix(before_caret, &last.text) else {
        return ReplacePlan::CopyOnly(ReplaceRefusal::TextMoved);
    };
    let graphemes = caret_units(suffix).graphemes;
    if graphemes > max_graphemes {
        return ReplacePlan::CopyOnly(ReplaceRefusal::TooLong);
    }
    ReplacePlan::Replace { suffix: suffix.to_string(), graphemes }
}

/// Compares the focused field before and after pasting `new` over the selected `old`.
pub fn verify_replace(before: Option<&str>, after: Option<&str>, old: &str, new: &str) -> Verdict {
    let (Some(before), Some(after)) = (before, after) else {
        return Verdict::Unknown;
    };
    if before == after {
        return Verdict::NotInserted;
    }
    let (old, new) = (fingerprint(old), fingerprint(new));
    if new.chars().count() < MIN_VERIFIABLE_CHARS {
        return Verdict::Unknown;
    }
    let (before, after) = (fingerprint(before), fingerprint(after));
    let new_rose = occurrences(&after, &new) > occurrences(&before, &new);
    // A new text that contains the old one (« Salut » → « Salut, à demain ») keeps the old count.
    let old_gone = occurrences(&after, &old) < occurrences(&before, &old) || new.contains(&old);
    if new_rose && old_gone {
        Verdict::Inserted
    } else {
        Verdict::Unknown
    }
}

/// Waits for the app to show `suffix` selected: injected arrows are applied asynchronously.
fn await_selection(field: &dyn FieldReader, suffix: &str, sleep: &dyn Fn(u64)) -> bool {
    let suffix = eol(suffix);
    (0..VERIFY_ATTEMPTS).any(|_| {
        sleep(VERIFY_INTERVAL_MS);
        field.selected_text().is_some_and(|s| eol(&s) == suffix)
    })
}

/// Replaces `suffix` (from [`plan_replace`]), right before the caret, with `text`. Whenever the replacement cannot
/// be made or seen, `text` stays in the clipboard.
#[allow(clippy::too_many_arguments)]
pub fn replace(
    suffix: &str,
    graphemes: usize,
    text: &str,
    clipboard: &dyn Clipboard,
    keys: &dyn KeySender,
    field: &dyn FieldReader,
    restore_delay_ms: u64,
    sleep: &dyn Fn(u64),
) -> InsertResult {
    let previous = clipboard.read();
    if clipboard.write_text(text).is_err() {
        return InsertResult::ClipboardFailed;
    }
    // A timed-out selection may still land: arrows on top of it would shrink it.
    if field.select_before_caret(suffix) == Selection::Unsupported && keys.press(NavKey::Left, true, graphemes).is_err()
    {
        return InsertResult::NotPasted;
    }
    // Also catches caret steps that are not graphemes in this app, and a modifier the user still holds.
    if !await_selection(field, suffix, sleep) {
        if field.selected_text().is_some_and(|s| !s.is_empty()) {
            let _ = keys.press(NavKey::Right, false, 1);
        }
        return InsertResult::NotPasted;
    }
    let before = field.focused_text();
    if keys.send_paste().is_err() {
        return InsertResult::PasteFailed;
    }
    let (verdict, waited) =
        await_paste(field, before.as_deref(), sleep, |b, a| verify_replace(Some(b), Some(a), suffix, text));
    if verdict == Verdict::NotInserted {
        // The old text stays selected: pasting by hand replaces it, and a late paste lands over it.
        return InsertResult::NotPasted;
    }
    restore_clipboard(clipboard, &previous, text, restore_delay_ms, waited, sleep);
    InsertResult::Pasted
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
            if self.ok {
                Ok(())
            } else {
                Err("bloqué".into())
            }
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
            if v.len() > 1 {
                v.pop().unwrap()
            } else {
                v[0].clone()
            }
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
        assert_eq!(
            verify(some("à demain"), some("à demain\nà demain"), "à demain"),
            Verdict::Inserted,
            "same text twice"
        );
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
        let r =
            perform(InsertPlan::Paste { uncertain: true }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| {
                delays.push(ms)
            });
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(delays.get(), vec![100, 100], "stops reading once seen; the restore delay already elapsed");
        assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn a_paste_that_did_not_land_leaves_the_text_in_the_clipboard() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let delays = Delays::default();
        let field = FakeField::new(&[Some("début")]);
        let r =
            perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| {
                delays.push(ms)
            });
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
            let r = perform(InsertPlan::Paste { uncertain }, "dicté", &cb, &FakeKeys::new(true), &field, 1000, &|ms| {
                delays.push(ms)
            });
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
        let r =
            perform(InsertPlan::Paste { uncertain: true }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| {
                delays.push(ms)
            });
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
        let r =
            perform(InsertPlan::Paste { uncertain: true }, "dicté", &cb, &FakeKeys::new(true), &field, 150, &|ms| {
                delays.push(ms)
            });
        assert_eq!(r, InsertResult::PastedUncertain);
        assert_eq!(delays.get(), vec![150]);
    }

    #[test]
    fn decide_by_focus_state() {
        assert_eq!(
            decide(&snap(1, FocusState::Unknown), &snap(1, FocusState::Editable)),
            InsertPlan::Paste { uncertain: false }
        );
        assert_eq!(
            decide(&snap(1, FocusState::Editable), &snap(1, FocusState::Unknown)),
            InsertPlan::Paste { uncertain: true }
        );
        assert_eq!(
            decide(&snap(1, FocusState::Editable), &snap(1, FocusState::NotEditable)),
            InsertPlan::ClipboardOnly
        );
        assert_eq!(
            decide(&snap(1, FocusState::Editable), &snap(1, FocusState::Terminal)),
            InsertPlan::Paste { uncertain: false },
            "a terminal is pasted into like an editable field"
        );
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
        let r = perform(InsertPlan::Paste { uncertain: false }, "nouveau", &cb, &keys, &NoField, 150, &|ms| {
            delays.push(ms)
        });
        assert_eq!(r, InsertResult::Pasted);
        assert_eq!(*keys.sent.lock().unwrap(), 1);
        assert_eq!(delays.get(), vec![150]);
        assert_eq!(cb.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn paste_restores_previous_image_and_multiline_text_with_crlf() {
        let img = ClipboardContent::Image { width: 1, height: 1, rgba: vec![1, 2, 3, 4] };
        let cb = FakeClipboard { content: Mutex::new(Some(img.clone())), crlf: true, fail_write: false };
        let r = perform(
            InsertPlan::Paste { uncertain: true },
            "ligne 1\nligne 2",
            &cb,
            &FakeKeys::new(true),
            &NoField,
            0,
            &no_sleep,
        );
        assert_eq!(r, InsertResult::PastedUncertain);
        assert_eq!(cb.get(), img);
    }

    #[test]
    fn user_copy_during_paste_is_not_overwritten() {
        let cb = FakeClipboard::with(ClipboardContent::Text("ancien".into()));
        let r = perform(
            InsertPlan::Paste { uncertain: false },
            "dicté",
            &cb,
            &FakeKeys::new(true),
            &NoField,
            150,
            &|_| {
                cb.write_text("copié par l'utilisateur").unwrap();
            },
        );
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
        let r = perform(
            InsertPlan::Paste { uncertain: false },
            "dicté",
            &cb,
            &FakeKeys::new(false),
            &NoField,
            0,
            &no_sleep,
        );
        assert_eq!(r, InsertResult::PasteFailed);
        assert_eq!(r.outcome(), Outcome::Clipboard);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn clipboard_only_never_sends_keys() {
        let cb = FakeClipboard::default();
        let keys = FakeKeys::new(true);
        assert_eq!(
            perform(InsertPlan::ClipboardOnly, "dicté", &cb, &keys, &NoField, 0, &no_sleep),
            InsertResult::ClipboardOnly
        );
        assert_eq!(*keys.sent.lock().unwrap(), 0);
        assert_eq!(cb.get(), ClipboardContent::Text("dicté".into()));
    }

    #[test]
    fn clipboard_write_failure_is_reported() {
        let cb = FakeClipboard { fail_write: true, ..Default::default() };
        let r =
            perform(InsertPlan::Paste { uncertain: false }, "dicté", &cb, &FakeKeys::new(true), &NoField, 0, &no_sleep);
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
        assert_eq!(
            perform(InsertPlan::ClipboardOnly, "dicté", &cb, &keys, &NoField, 0, &no_sleep),
            InsertResult::ClipboardFailed
        );
        assert_eq!(*keys.sent.lock().unwrap(), 0);
    }

    fn pasted(text: &str) -> PastedText {
        PastedText { dictation_id: 3, focus: snap(1, FocusState::Editable), text: text.into() }
    }

    fn replace_with(suffix: &str) -> ReplacePlan {
        ReplacePlan::Replace { suffix: suffix.into(), graphemes: caret_units(suffix).graphemes }
    }

    #[test]
    fn plan_replace_needs_the_same_editable_field_with_the_text_before_the_caret() {
        use ReplaceRefusal::*;
        let last = pasted("Salut, à demain.");
        let here = snap(1, FocusState::Editable);
        let plan = |now: &FocusSnapshot, before: Option<&str>, idle: bool| {
            plan_replace(&last, now, before, idle, MAX_SELECT_GRAPHEMES)
        };
        let field = Some("Bonjour. Salut, à demain.");
        assert_eq!(plan(&here, field, true), replace_with("Salut, à demain."));
        assert_eq!(plan(&here, field, false), ReplacePlan::CopyOnly(Busy));
        assert_eq!(plan(&snap(2, FocusState::Editable), field, true), ReplacePlan::CopyOnly(FocusChanged));
        let other_app = FocusSnapshot { app_name: Some("Other".into()), ..here.clone() };
        assert_eq!(plan(&other_app, field, true), ReplacePlan::CopyOnly(FocusChanged));
        for state in [FocusState::NotEditable, FocusState::Unknown, FocusState::Terminal] {
            assert_eq!(plan(&snap(1, state), field, true), ReplacePlan::CopyOnly(NotEditable), "{state:?}");
        }
        assert_eq!(plan(&here, None, true), ReplacePlan::CopyOnly(CaretUnreadable));
        assert_eq!(plan(&here, Some("Salut, à demain. Et"), true), ReplacePlan::CopyOnly(TextMoved), "typed after");
        assert_eq!(plan(&here, Some("demain."), true), ReplacePlan::CopyOnly(TextMoved), "partly deleted");
        assert_eq!(plan(&here, Some(""), true), ReplacePlan::CopyOnly(TextMoved));
    }

    #[test]
    fn plan_replace_refuses_a_paste_whose_window_was_unknown() {
        let mut last = pasted("Salut, à demain.");
        last.focus.window_id = None;
        let now = FocusSnapshot { window_id: None, ..snap(1, FocusState::Editable) };
        assert_eq!(
            plan_replace(&last, &now, Some("Salut, à demain."), true, MAX_SELECT_GRAPHEMES),
            ReplacePlan::CopyOnly(ReplaceRefusal::FocusChanged)
        );
    }

    #[test]
    fn plan_replace_selects_the_field_text_with_its_crlf() {
        let last = pasted("Ligne 1\nLigne 2");
        let now = snap(1, FocusState::Editable);
        assert_eq!(
            plan_replace(&last, &now, Some("Avant\r\nLigne 1\r\nLigne 2"), true, MAX_SELECT_GRAPHEMES),
            ReplacePlan::Replace { suffix: "Ligne 1\r\nLigne 2".into(), graphemes: 15 },
            "a CRLF is one caret step"
        );
        let last = pasted("\nSuite");
        assert_eq!(
            plan_replace(&last, &now, Some("Avant\r\nSuite"), true, MAX_SELECT_GRAPHEMES),
            ReplacePlan::Replace { suffix: "\r\nSuite".into(), graphemes: 6 },
            "never splits a CRLF"
        );
        let last = pasted("Ligne 1\r\nLigne 2");
        assert_eq!(
            plan_replace(&last, &now, Some("Ligne 1\nLigne 2"), true, MAX_SELECT_GRAPHEMES),
            replace_with("Ligne 1\nLigne 2")
        );
    }

    #[test]
    fn plan_replace_reads_a_lone_cr_as_a_line_break() {
        // RichEdit (Notepad, WordPad, Outlook) reports its line breaks as a bare CR.
        let last = pasted("Bonjour\nÀ demain");
        let now = snap(1, FocusState::Editable);
        assert_eq!(
            plan_replace(&last, &now, Some("Objet\rBonjour\rÀ demain"), true, MAX_SELECT_GRAPHEMES),
            ReplacePlan::Replace { suffix: "Bonjour\rÀ demain".into(), graphemes: 16 }
        );
    }

    #[test]
    fn plan_replace_counts_graphemes_and_refuses_a_long_text() {
        let now = snap(1, FocusState::Editable);
        let last = pasted("Bravo 👍🏽 👨‍👩‍👧 é");
        assert_eq!(
            plan_replace(&last, &now, Some("x Bravo 👍🏽 👨‍👩‍👧 é"), true, MAX_SELECT_GRAPHEMES),
            ReplacePlan::Replace { suffix: "Bravo 👍🏽 👨‍👩‍👧 é".into(), graphemes: 11 }
        );
        assert_eq!(plan_replace(&last, &now, Some("Bravo 👍🏽 👨‍👩‍👧 é"), true, 11), replace_with("Bravo 👍🏽 👨‍👩‍👧 é"));
        assert_eq!(
            plan_replace(&last, &now, Some("Bravo 👍🏽 👨‍👩‍👧 é"), true, 10),
            ReplacePlan::CopyOnly(ReplaceRefusal::TooLong)
        );
    }

    #[test]
    fn caret_units_count_utf16_and_graphemes() {
        assert_eq!(caret_units(""), CaretUnits { utf16: 0, graphemes: 0 });
        assert_eq!(caret_units("é"), CaretUnits { utf16: 1, graphemes: 1 });
        assert_eq!(caret_units("e\u{301}"), CaretUnits { utf16: 2, graphemes: 1 });
        assert_eq!(caret_units("👍🏽"), CaretUnits { utf16: 4, graphemes: 1 });
        assert_eq!(caret_units("a\r\nb"), CaretUnits { utf16: 4, graphemes: 3 });
    }

    #[test]
    fn verify_replace_wants_the_new_text_in_and_the_old_one_out() {
        let v = |before: &str, after: &str, old: &str, new: &str| verify_replace(Some(before), Some(after), old, new);
        assert_eq!(v("Bonjour salut", "Bonjour Hello there", "salut", "Hello there"), Verdict::Inserted);
        assert_eq!(v("Salut", "Salut, à demain", "Salut", "Salut, à demain"), Verdict::Inserted, "new contains old");
        assert_eq!(v("Salut", "Salut", "Salut", "Hello there"), Verdict::NotInserted, "unchanged");
        assert_eq!(
            v("Bonjour salut", "Bonjour salutHello there", "salut", "Hello there"),
            Verdict::Unknown,
            "pasted after the old text: the selection was lost"
        );
        assert_eq!(v("Bonjour salut", "Bonjour", "salut", "Hello there"), Verdict::Unknown, "new text not seen");
        assert_eq!(v("Bonjour salut", "Bonjour ok", "salut", "ok"), Verdict::Unknown, "too short to recognise");
        assert_eq!(verify_replace(None, Some("x"), "salut", "Hello there"), Verdict::Unknown);
        assert_eq!(verify_replace(Some("x"), None, "salut", "Hello there"), Verdict::Unknown);
    }

    /// What the fakes saw, in order.
    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);
    impl Log {
        fn push(&self, step: impl Into<String>) {
            self.0.lock().unwrap().push(step.into());
        }
        fn get(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    struct LoggedClipboard<'a> {
        inner: FakeClipboard,
        log: &'a Log,
    }
    impl Clipboard for LoggedClipboard<'_> {
        fn read(&self) -> ClipboardContent {
            self.inner.read()
        }
        fn write_text(&self, text: &str) -> Result<(), String> {
            self.log.push(format!("write {text}"));
            self.inner.write_text(text)
        }
        fn restore(&self, c: &ClipboardContent) -> Result<(), String> {
            self.log.push("restore");
            self.inner.restore(c)
        }
    }

    struct LoggedKeys<'a> {
        log: &'a Log,
        paste_ok: bool,
        press_ok: bool,
    }
    impl KeySender for LoggedKeys<'_> {
        fn send_paste(&self) -> Result<(), String> {
            self.log.push("paste");
            if self.paste_ok {
                Ok(())
            } else {
                Err("bloqué".into())
            }
        }
        fn press(&self, key: NavKey, shift: bool, times: usize) -> Result<(), String> {
            self.log.push(format!("press {key:?} shift={shift} x{times}"));
            if self.press_ok {
                Ok(())
            } else {
                Err("bloqué".into())
            }
        }
    }

    struct SelectField<'a> {
        log: &'a Log,
        select: Selection,
        selections: FakeField,
        texts: FakeField,
    }
    impl FieldReader for SelectField<'_> {
        fn focused_text(&self) -> Option<String> {
            self.texts.focused_text()
        }
        fn selected_text(&self) -> Option<String> {
            self.selections.focused_text()
        }
        fn select_before_caret(&self, suffix: &str) -> Selection {
            self.log.push(format!("select {suffix}"));
            self.select
        }
    }

    struct Replacing<'a> {
        clipboard: LoggedClipboard<'a>,
        keys: LoggedKeys<'a>,
        field: SelectField<'a>,
        delays: Delays,
    }
    impl<'a> Replacing<'a> {
        fn new(log: &'a Log) -> Self {
            Self {
                clipboard: LoggedClipboard { inner: FakeClipboard::with(ClipboardContent::Text("ancien".into())), log },
                keys: LoggedKeys { log, paste_ok: true, press_ok: true },
                field: SelectField {
                    log,
                    select: Selection::Selected,
                    selections: FakeField::new(&[Some("salut ça va")]),
                    texts: FakeField::new(&[Some("Bonjour salut ça va"), Some("Bonjour Hello there")]),
                },
                delays: Delays::default(),
            }
        }
        fn run(&self, suffix: &str) -> InsertResult {
            replace(
                suffix,
                caret_units(suffix).graphemes,
                "Hello there",
                &self.clipboard,
                &self.keys,
                &self.field,
                150,
                &|ms| self.delays.push(ms),
            )
        }
    }

    #[test]
    fn replace_selects_then_pastes_then_restores_the_clipboard() {
        let log = Log::default();
        let r = Replacing::new(&log);
        assert_eq!(r.run("salut ça va"), InsertResult::Pasted);
        assert_eq!(log.get(), ["write Hello there", "select salut ça va", "paste", "restore"]);
        assert_eq!(r.delays.get(), vec![100, 100, 50], "selection poll, paste seen, rest of the restore delay");
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn replace_falls_back_on_shift_left_when_the_field_cannot_select() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.select = Selection::Unsupported;
        assert_eq!(r.run("salut ça va"), InsertResult::Pasted);
        assert_eq!(
            log.get(),
            ["write Hello there", "select salut ça va", "press Left shift=true x11", "paste", "restore"]
        );
    }

    #[test]
    fn replace_waits_for_the_selection_to_reach_the_suffix() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.selections = FakeField::new(&[None, Some("va"), Some("salut ça va")]);
        assert_eq!(r.run("salut ça va"), InsertResult::Pasted);
        assert_eq!(r.delays.get(), vec![100, 100, 100, 100, 50]);
    }

    #[test]
    fn replace_compares_the_selection_without_line_endings() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.selections = FakeField::new(&[Some("salut\nça va")]);
        r.field.texts = FakeField::new(&[Some("Bonjour salut\r\nça va"), Some("Bonjour Hello there")]);
        assert_eq!(r.run("salut\r\nça va"), InsertResult::Pasted);
    }

    #[test]
    fn a_selection_that_never_matches_is_collapsed_and_the_text_copied() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.selections = FakeField::new(&[Some("ça va")]);
        assert_eq!(r.run("salut ça va"), InsertResult::NotPasted);
        assert_eq!(log.get(), ["write Hello there", "select salut ça va", "press Right shift=false x1"]);
        assert_eq!(r.delays.get().len() as u64, VERIFY_ATTEMPTS);
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("Hello there".into()));
    }

    #[test]
    fn an_empty_selection_is_not_collapsed() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        for selection in [Some(""), None] {
            r.field.selections = FakeField::new(&[selection]);
            assert_eq!(r.run("salut ça va"), InsertResult::NotPasted);
        }
        assert!(!log.get().iter().any(|step| step.starts_with("press")), "{:?}", log.get());
    }

    #[test]
    fn a_timed_out_selection_is_awaited_without_arrows() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.select = Selection::TimedOut;
        r.field.selections = FakeField::new(&[None, Some("salut ça va")]);
        assert_eq!(r.run("salut ça va"), InsertResult::Pasted, "it landed late");
        assert_eq!(log.get(), ["write Hello there", "select salut ça va", "paste", "restore"]);

        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.select = Selection::TimedOut;
        r.field.selections = FakeField::new(&[None]);
        assert_eq!(r.run("salut ça va"), InsertResult::NotPasted);
        assert_eq!(log.get(), ["write Hello there", "select salut ça va"]);
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("Hello there".into()));
    }

    #[test]
    fn replace_without_any_way_to_select_copies_the_text() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.select = Selection::Unsupported;
        r.keys.press_ok = false;
        assert_eq!(r.run("salut ça va"), InsertResult::NotPasted);
        assert_eq!(log.get(), ["write Hello there", "select salut ça va", "press Left shift=true x11"]);
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("Hello there".into()));
    }

    #[test]
    fn a_replacement_that_did_not_land_stays_selected_and_copied() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.texts = FakeField::new(&[Some("Bonjour salut ça va")]);
        assert_eq!(r.run("salut ça va"), InsertResult::NotPasted);
        assert_eq!(log.get(), ["write Hello there", "select salut ça va", "paste"], "a paste by hand replaces it");
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("Hello there".into()));
    }

    #[test]
    fn an_unclear_replacement_counts_as_pasted() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.field.texts = FakeField::new(&[Some("Bonjour salut ça va"), None]);
        assert_eq!(r.run("salut ça va"), InsertResult::Pasted);
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("ancien".into()));
    }

    #[test]
    fn replace_reports_clipboard_and_paste_failures() {
        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.clipboard.inner.fail_write = true;
        assert_eq!(r.run("salut ça va"), InsertResult::ClipboardFailed);
        assert_eq!(log.get(), ["write Hello there"], "nothing selected");

        let log = Log::default();
        let mut r = Replacing::new(&log);
        r.keys.paste_ok = false;
        assert_eq!(r.run("salut ça va"), InsertResult::PasteFailed);
        assert_eq!(log.get(), ["write Hello there", "select salut ça va", "paste"], "still selected");
        assert_eq!(r.clipboard.inner.get(), ClipboardContent::Text("Hello there".into()));
    }

    struct PasteOnly;
    impl KeySender for PasteOnly {
        fn send_paste(&self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn seams_default_to_unsupported() {
        assert_eq!(NoField.before_caret(), None);
        assert_eq!(NoField.selected_text(), None);
        assert_eq!(NoField.select_before_caret("x"), Selection::Unsupported);
        assert_eq!((Selection::of(true), Selection::of(false)), (Selection::Selected, Selection::Unsupported));
        assert!(PasteOnly.press(NavKey::Left, true, 1).is_err());
        assert!(PasteOnly.send_paste().is_ok());
    }

    /// Answers every read after `delay` ms.
    struct SlowSelect(u64);
    impl FieldReader for SlowSelect {
        fn focused_text(&self) -> Option<String> {
            std::thread::sleep(Duration::from_millis(self.0));
            Some("texte".into())
        }
        fn before_caret(&self) -> Option<String> {
            std::thread::sleep(Duration::from_millis(self.0));
            Some("avant".into())
        }
        fn selected_text(&self) -> Option<String> {
            std::thread::sleep(Duration::from_millis(self.0));
            Some("choisi".into())
        }
        fn select_before_caret(&self, suffix: &str) -> Selection {
            std::thread::sleep(Duration::from_millis(self.0));
            if suffix == "ok" {
                Selection::Selected
            } else {
                Selection::Unsupported
            }
        }
    }

    #[test]
    fn the_timed_reader_forwards_every_read_and_bounds_it() {
        let fast = TimedFieldReader::new(Arc::new(SlowSelect(0)), 2_000);
        assert_eq!(fast.focused_text().as_deref(), Some("texte"));
        assert_eq!(fast.before_caret().as_deref(), Some("avant"));
        assert_eq!(fast.selected_text().as_deref(), Some("choisi"));
        assert_eq!(fast.select_before_caret("ok"), Selection::Selected);
        assert_eq!(fast.select_before_caret("non"), Selection::Unsupported);
        let slow = TimedFieldReader::new(Arc::new(SlowSelect(2_000)), 20);
        let start = std::time::Instant::now();
        assert_eq!(slow.before_caret(), None);
        assert_eq!(slow.selected_text(), None);
        assert_eq!(slow.select_before_caret("ok"), Selection::TimedOut);
        assert!(start.elapsed() < Duration::from_millis(1_500), "{:?}", start.elapsed());
    }
}
