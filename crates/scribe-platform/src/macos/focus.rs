use std::hash::{Hash, Hasher};
use std::ptr::NonNull;

use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFRetained, CFString, CFType};

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::FieldReader;

use crate::focus_rules::{classify_ax, process_stem, AxFacts, AX_SECURE_TEXT_FIELD};

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

fn value_settable(element: &AXUIElement) -> Option<bool> {
    let mut settable: u8 = 0;
    let err = unsafe { element.is_attribute_settable(&CFString::from_static_str("AXValue"), NonNull::from(&mut settable)) };
    (err == AXError::Success).then_some(settable != 0)
}

fn focused_app() -> Option<CFRetained<AXUIElement>> {
    let system = unsafe { AXUIElement::new_system_wide() };
    unsafe { system.set_messaging_timeout(MESSAGING_TIMEOUT_S) };
    let app = element_attr(&system, "AXFocusedApplication")?;
    unsafe { app.set_messaging_timeout(MESSAGING_TIMEOUT_S) };
    Some(app)
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
        let Some(app) = focused_app() else { return FocusSnapshot::unknown() };
        let pid = pid(&app);
        let facts = element_attr(&app, "AXFocusedUIElement").map(|el| AxFacts {
            role: string_attr(&el, "AXRole").unwrap_or_default(),
            subrole: string_attr(&el, "AXSubrole").unwrap_or_default(),
            value_settable: value_settable(&el),
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
        let app = focused_app()?;
        let element = element_attr(&app, "AXFocusedUIElement")?;
        if string_attr(&element, "AXSubrole").as_deref() == Some(AX_SECURE_TEXT_FIELD) {
            return None;
        }
        string_attr(&element, "AXValue")
    }
}
