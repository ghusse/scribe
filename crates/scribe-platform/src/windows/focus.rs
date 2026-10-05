use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationValuePattern, UIA_ValuePatternId};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};

use scribe_core::focus::{FocusDetector, FocusSnapshot};

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
    let (control_type, value_read_only) = uia_facts().unwrap_or((None, None));
    let state = classify(&UiaFacts { window_class, control_type, value_read_only });
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

unsafe fn uia_facts() -> windows::core::Result<(Option<i32>, Option<bool>)> {
    // Ignore the result: S_FALSE / RPC_E_CHANGED_MODE just mean COM is already initialised on this thread.
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let element = automation.GetFocusedElement()?;
    let control_type = element.CurrentControlType()?.0;
    let read_only = element
        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        .ok()
        .and_then(|p| p.CurrentIsReadOnly().ok())
        .map(|b| b.as_bool());
    Ok((Some(control_type), read_only))
}
