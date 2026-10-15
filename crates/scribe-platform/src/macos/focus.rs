use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::ptr::NonNull;
use std::sync::Mutex;

use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFBoolean, CFIndex, CFRange, CFRetained, CFString, CFType};

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::{caret_units, FieldReader, Selection};

use crate::focus_rules::{classify_ax, prefix_utf16, process_stem, AxFacts, AX_SECURE_TEXT_FIELD};

/// An app that does not answer within this delay is treated as unknown (`snapshot_with_timeout` also bounds it).
const MESSAGING_TIMEOUT_S: f32 = 0.5;

fn attr(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    let err = unsafe { element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value)) };
    if err != AXError::Success {
        return None;
    }
    NonNull::new(value as *mut CFType).map(|v| unsafe { CFRetained::from_raw(v) })
}

fn element_attr(element: &AXUIElement, name: &'static str) -> Option<CFRetained<AXUIElement>> {
    attr(element, name)?.downcast::<AXUIElement>().ok()
}

fn string_attr(element: &AXUIElement, name: &'static str) -> Option<String> {
    Some(attr(element, name)?.downcast::<CFString>().ok()?.to_string())
}

fn settable(element: &AXUIElement, name: &'static str) -> Option<bool> {
    let mut settable: u8 = 0;
    let err = unsafe { element.is_attribute_settable(&CFString::from_static_str(name), NonNull::from(&mut settable)) };
    (err == AXError::Success).then_some(settable != 0)
}

/// Applications that accepted `AXManualAccessibility`.
static MANUAL_ACCESSIBILITY: Mutex<BTreeSet<libc::pid_t>> = Mutex::new(BTreeSet::new());

/// Chromium (Electron apps such as Slack) builds no accessibility tree for other apps until asked. Costly for the
/// app (and VS Code switches to its screen reader mode): only a style switch asks.
fn enable_manual_accessibility(app: &AXUIElement) {
    let Some(pid) = pid(app) else { return };
    if MANUAL_ACCESSIBILITY.lock().unwrap_or_else(|p| p.into_inner()).contains(&pid) {
        return;
    }
    let name = CFString::from_static_str("AXManualAccessibility");
    if unsafe { app.set_attribute_value(&name, CFBoolean::new(true)) } == AXError::Success {
        MANUAL_ACCESSIBILITY.lock().unwrap_or_else(|p| p.into_inner()).insert(pid);
    }
}

fn focused_app() -> Option<CFRetained<AXUIElement>> {
    let system = unsafe { AXUIElement::new_system_wide() };
    unsafe { system.set_messaging_timeout(MESSAGING_TIMEOUT_S) };
    let app = element_attr(&system, "AXFocusedApplication")?;
    unsafe { app.set_messaging_timeout(MESSAGING_TIMEOUT_S) };
    Some(app)
}

/// The app's focused element, unless it is a password field.
fn field_of(app: &AXUIElement) -> Option<CFRetained<AXUIElement>> {
    let element = element_attr(app, "AXFocusedUIElement")?;
    (string_attr(&element, "AXSubrole").as_deref() != Some(AX_SECURE_TEXT_FIELD)).then_some(element)
}

fn focused_field() -> Option<CFRetained<AXUIElement>> {
    field_of(&*focused_app()?)
}

/// The focused field for a style switch, which needs the caret even in an Electron app.
fn caret_field() -> Option<CFRetained<AXUIElement>> {
    let app = focused_app()?;
    enable_manual_accessibility(&app);
    field_of(&app)
}

/// The selection, in UTF-16 units.
fn selected_range(element: &AXUIElement) -> Option<CFRange> {
    let value = attr(element, "AXSelectedTextRange")?.downcast::<AXValue>().ok()?;
    let mut range = CFRange { location: 0, length: 0 };
    unsafe { value.value(AXValueType::CFRange, NonNull::from(&mut range).cast()) }.then_some(range)
}

/// The caret position when nothing is selected.
fn caret(element: &AXUIElement) -> Option<CFIndex> {
    selected_range(element).filter(|r| r.length == 0).map(|r| r.location)
}

fn pid(element: &AXUIElement) -> Option<libc::pid_t> {
    let mut pid: libc::pid_t = 0;
    (unsafe { element.pid(NonNull::from(&mut pid)) } == AXError::Success).then_some(pid)
}

fn process_name(pid: libc::pid_t) -> Option<String> {
    let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let path = std::str::from_utf8(buf.get(..usize::try_from(n).ok()?)?).ok()?;
    process_stem(path)
}

/// The focused window, as a number that changes when the user switches window (same app or another).
fn window_id(app: &AXUIElement, pid: libc::pid_t) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pid.hash(&mut hasher);
    if let Some(window) = attr(app, "AXFocusedWindow") {
        window.hash(&mut hasher);
    }
    hasher.finish()
}

pub struct AxFocusDetector;

impl FocusDetector for AxFocusDetector {
    fn snapshot(&self) -> FocusSnapshot {
        let Some(app) = focused_app() else {
            return FocusSnapshot::unknown();
        };
        let pid = pid(&app);
        let facts = element_attr(&app, "AXFocusedUIElement").map(|el| AxFacts {
            role: string_attr(&el, "AXRole").unwrap_or_default(),
            subrole: string_attr(&el, "AXSubrole").unwrap_or_default(),
            value_settable: settable(&el, "AXValue"),
        });
        FocusSnapshot {
            app_name: pid.and_then(process_name),
            window_id: pid.map(|p| window_id(&app, p)),
            state: facts.map_or(scribe_core::focus::FocusState::Unknown, |f| classify_ax(&f)),
        }
    }
}

/// Reads the focused field's value through the accessibility API. Never a password field.
pub struct AxFieldReader;

impl FieldReader for AxFieldReader {
    fn focused_text(&self) -> Option<String> {
        string_attr(&*focused_field()?, "AXValue")
    }

    fn before_caret(&self) -> Option<String> {
        let element = caret_field()?;
        let caret = usize::try_from(caret(&element)?).ok()?;
        prefix_utf16(&string_attr(&element, "AXValue")?, caret)
    }

    fn selected_text(&self) -> Option<String> {
        string_attr(&*focused_field()?, "AXSelectedText")
    }

    fn select_before_caret(&self, suffix: &str) -> Selection {
        Selection::of(select_before_caret(suffix).is_some())
    }
}

fn select_before_caret(suffix: &str) -> Option<()> {
    let element = caret_field()?;
    let caret = caret(&element)?;
    let units = CFIndex::try_from(caret_units(suffix).utf16).ok()?;
    if units > caret || settable(&element, "AXSelectedTextRange") != Some(true) {
        return None;
    }
    let mut range = CFRange { location: caret - units, length: units };
    let value = unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&mut range).cast()) }?;
    let err = unsafe { element.set_attribute_value(&CFString::from_static_str("AXSelectedTextRange"), &value) };
    (err == AXError::Success).then_some(())
}
