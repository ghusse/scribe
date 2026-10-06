use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationValuePattern,
    UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::FieldReader;

use crate::focus_rules::{classify, process_stem, utf16_prefix, UiaFacts};

pub struct UiaFocusDetector;

impl FocusDetector for UiaFocusDetector {
    fn snapshot(&self) -> FocusSnapshot {
        unsafe { snapshot_impl() }
    }
}

unsafe fn snapshot_impl() -> FocusSnapshot {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return FocusSnapshot::unknown();
    }
    let window_class = class_name(hwnd);
    let (element_class, control_type, value_read_only) = uia_facts().unwrap_or((String::new(), None, None));
    let state = classify(&UiaFacts { window_class, element_class, control_type, value_read_only });
    FocusSnapshot { app_name: process_name(hwnd), window_id: Some(hwnd.0 as usize as u64), state }
}

unsafe fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = GetClassNameW(hwnd, &mut buf);
    utf16_prefix(&buf, n as i64)
}

unsafe fn process_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let result = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
    let _ = CloseHandle(handle);
    result.ok()?;
    process_stem(&utf16_prefix(&buf, len as i64))
}

unsafe fn focused_element() -> windows::core::Result<IUIAutomationElement> {
    // Ignore the result: S_FALSE / RPC_E_CHANGED_MODE just mean COM is already initialised on this thread.
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    automation.GetFocusedElement()
}

unsafe fn uia_facts() -> windows::core::Result<(String, Option<i32>, Option<bool>)> {
    let element = focused_element()?;
    let element_class = element.CurrentClassName().map(|s| s.to_string()).unwrap_or_default();
    let control_type = element.CurrentControlType()?.0;
    let read_only = element
        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        .ok()
        .and_then(|p| p.CurrentIsReadOnly().ok())
        .map(|b| b.as_bool());
    Ok((element_class, Some(control_type), read_only))
}

/// Reads the focused field through UI Automation: its whole text (text pattern: edits, documents, terminals),
/// else its value. Never a password field.
pub struct UiaFieldReader;

impl FieldReader for UiaFieldReader {
    fn focused_text(&self) -> Option<String> {
        unsafe { focused_text().ok().flatten() }
    }
}

unsafe fn focused_text() -> windows::core::Result<Option<String>> {
    let element = focused_element()?;
    if element.CurrentIsPassword()?.as_bool() {
        return Ok(None);
    }
    if let Ok(text) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
        // -1: the whole text. A cap could hide a paste made past it, and both readings would look unchanged.
        return Ok(Some(text.DocumentRange()?.GetText(-1)?.to_string()));
    }
    if let Ok(value) = element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
        return Ok(Some(value.CurrentValue()?.to_string()));
    }
    Ok(None)
}
