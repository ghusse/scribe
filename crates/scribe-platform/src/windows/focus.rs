use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextRange,
    IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
    UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};

use scribe_core::focus::{FocusDetector, FocusSnapshot};
use scribe_core::insert::{caret_units, FieldReader, Selection};

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

    fn before_caret(&self) -> Option<String> {
        unsafe { before_caret().ok().flatten() }
    }

    fn selected_text(&self) -> Option<String> {
        unsafe { selected_text().ok().flatten() }
    }

    fn select_before_caret(&self, suffix: &str) -> Selection {
        Selection::of(unsafe { select_before_caret(suffix).unwrap_or(false) })
    }
}

/// The focused field's text pattern (`None`: a password field or a field without one, such as a value-only field).
unsafe fn text_pattern() -> windows::core::Result<Option<IUIAutomationTextPattern>> {
    let element = focused_element()?;
    if element.CurrentIsPassword()?.as_bool() {
        return Ok(None);
    }
    Ok(element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok())
}

unsafe fn selection(pattern: &IUIAutomationTextPattern) -> windows::core::Result<IUIAutomationTextRange> {
    pattern.GetSelection()?.GetElement(0)
}

/// The caret as an empty range (`None` while text is selected).
unsafe fn caret(pattern: &IUIAutomationTextPattern) -> windows::core::Result<Option<IUIAutomationTextRange>> {
    let sel = selection(pattern)?;
    let empty = sel.CompareEndpoints(TextPatternRangeEndpoint_Start, &sel, TextPatternRangeEndpoint_End)? == 0;
    Ok(empty.then_some(sel))
}

unsafe fn before_caret() -> windows::core::Result<Option<String>> {
    let Some(pattern) = text_pattern()? else { return Ok(None) };
    let Some(caret) = caret(&pattern)? else { return Ok(None) };
    let range = pattern.DocumentRange()?;
    range.MoveEndpointByRange(TextPatternRangeEndpoint_End, &caret, TextPatternRangeEndpoint_Start)?;
    Ok(Some(range.GetText(-1)?.to_string()))
}

unsafe fn selected_text() -> windows::core::Result<Option<String>> {
    let Some(pattern) = text_pattern()? else { return Ok(None) };
    Ok(Some(selection(&pattern)?.GetText(-1)?.to_string()))
}

/// Characters are the provider's own (graphemes, code points or UTF-16 units): the caller checks the selection.
unsafe fn select_before_caret(suffix: &str) -> windows::core::Result<bool> {
    let Some(pattern) = text_pattern()? else { return Ok(false) };
    let Some(caret) = caret(&pattern)? else { return Ok(false) };
    let Ok(n) = i32::try_from(caret_units(suffix).graphemes) else { return Ok(false) };
    let range = caret.Clone()?;
    if range.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -n)? != -n {
        return Ok(false);
    }
    range.Select()?;
    Ok(true)
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
