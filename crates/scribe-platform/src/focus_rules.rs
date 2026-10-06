//! Pure classification of UI Automation facts, kept OS-independent so it is unit-testable everywhere.
use scribe_core::focus::FocusState;

pub const BUTTON: i32 = 50000;
pub const COMBO_BOX: i32 = 50003;
pub const EDIT: i32 = 50004;
pub const LIST_ITEM: i32 = 50007;
pub const LIST: i32 = 50008;
pub const MENU_ITEM: i32 = 50011;
pub const TAB: i32 = 50018;
pub const TAB_ITEM: i32 = 50019;
pub const TREE: i32 = 50023;
pub const TREE_ITEM: i32 = 50024;
pub const TEXT: i32 = 50020;
pub const DOCUMENT: i32 = 50030;
pub const PANE: i32 = 50033;

const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd"];
const NAVIGATION: &[i32] = &[BUTTON, LIST_ITEM, LIST, MENU_ITEM, TAB, TAB_ITEM, TREE, TREE_ITEM];
/// Terminals take typed and pasted text, but expose it as plain text with no writable value: Windows Terminal's
/// text area (a `TermControl` element) and the classic console window (`ConsoleWindowClass`).
const TERMINAL_ELEMENTS: &[&str] = &["TermControl"];
const TERMINAL_WINDOWS: &[&str] = &["ConsoleWindowClass"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiaFacts {
    pub window_class: String,
    /// Class name of the focused element itself (empty when unknown).
    pub element_class: String,
    pub control_type: Option<i32>,
    pub value_read_only: Option<bool>,
}

pub fn classify(f: &UiaFacts) -> FocusState {
    if SHELL_CLASSES.contains(&f.window_class.as_str()) {
        return FocusState::NotEditable;
    }
    if TERMINAL_ELEMENTS.contains(&f.element_class.as_str()) || TERMINAL_WINDOWS.contains(&f.window_class.as_str()) {
        return FocusState::Editable;
    }
    match f.control_type {
        Some(EDIT) => {
            if f.value_read_only == Some(true) { FocusState::NotEditable } else { FocusState::Editable }
        }
        Some(DOCUMENT) | Some(COMBO_BOX) => {
            if f.value_read_only == Some(false) { FocusState::Editable } else { FocusState::Unknown }
        }
        Some(ct) if NAVIGATION.contains(&ct) => FocusState::NotEditable,
        _ => FocusState::Unknown,
    }
}

/// Text of a UTF-16 buffer filled by a Win32 call that returned `len` (negative or too large = clamped).
pub fn utf16_prefix(buf: &[u16], len: i64) -> String {
    let n = len.clamp(0, buf.len() as i64) as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// Application name shown in the history: the executable's file stem (`C:\…\Code.exe` → `Code`).
pub fn process_stem(image_path: &str) -> Option<String> {
    let file = image_path.rsplit(['\\', '/']).next().unwrap_or(image_path);
    let stem = match file.rfind('.') {
        Some(i) if i > 0 => &file[..i],
        _ => file,
    };
    (!stem.is_empty()).then(|| stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(class: &str, ct: Option<i32>, ro: Option<bool>) -> UiaFacts {
        UiaFacts { window_class: class.into(), element_class: String::new(), control_type: ct, value_read_only: ro }
    }

    #[test]
    fn terminals_are_editable() {
        // Windows Terminal's text area: a Text control with a text pattern but no value.
        let wt = UiaFacts {
            window_class: "CASCADIA_HOSTING_WINDOW_CLASS".into(),
            element_class: "TermControl".into(),
            control_type: Some(TEXT),
            value_read_only: None,
        };
        assert_eq!(classify(&wt), FocusState::Editable);
        assert_eq!(classify(&facts("ConsoleWindowClass", None, None)), FocusState::Editable);
        assert_eq!(classify(&facts("X", Some(TEXT), None)), FocusState::Unknown, "other text controls stay unknown");
    }

    #[test]
    fn desktop_and_taskbar_are_not_editable() {
        for class in ["Progman", "WorkerW", "Shell_TrayWnd"] {
            assert_eq!(classify(&facts(class, Some(EDIT), Some(false))), FocusState::NotEditable);
        }
    }

    #[test]
    fn edit_controls_depend_on_read_only() {
        assert_eq!(classify(&facts("Notepad", Some(EDIT), Some(false))), FocusState::Editable);
        assert_eq!(classify(&facts("Notepad", Some(EDIT), None)), FocusState::Editable);
        assert_eq!(classify(&facts("Notepad", Some(EDIT), Some(true))), FocusState::NotEditable);
    }

    #[test]
    fn documents_are_editable_only_when_value_is_writable() {
        assert_eq!(classify(&facts("Chrome_WidgetWin_1", Some(DOCUMENT), Some(false))), FocusState::Editable);
        assert_eq!(classify(&facts("Chrome_WidgetWin_1", Some(DOCUMENT), None)), FocusState::Unknown);
        assert_eq!(classify(&facts("Chrome_WidgetWin_1", Some(DOCUMENT), Some(true))), FocusState::Unknown);
    }

    #[test]
    fn navigation_controls_are_not_editable_and_the_rest_is_unknown() {
        assert_eq!(classify(&facts("CabinetWClass", Some(LIST_ITEM), None)), FocusState::NotEditable);
        assert_eq!(classify(&facts("X", Some(BUTTON), None)), FocusState::NotEditable);
        assert_eq!(classify(&facts("CASCADIA_HOSTING_WINDOW_CLASS", Some(PANE), None)), FocusState::Unknown);
        assert_eq!(classify(&facts("X", None, None)), FocusState::Unknown);
        assert_eq!(classify(&facts("X", Some(COMBO_BOX), Some(false))), FocusState::Editable);
    }

    #[test]
    fn utf16_prefix_clamps_the_length() {
        let buf: Vec<u16> = "Notepad\0\0".encode_utf16().collect();
        assert_eq!(utf16_prefix(&buf, 7), "Notepad");
        assert_eq!(utf16_prefix(&buf, 0), "");
        assert_eq!(utf16_prefix(&buf, -1), "", "a failed call returns 0 or less");
        assert_eq!(utf16_prefix(&buf[..3], 50), "Not", "never reads past the buffer");
    }

    #[test]
    fn process_stem_keeps_the_file_name_without_extension() {
        assert_eq!(process_stem(r"C:\Program Files\Microsoft VS Code\Code.exe").as_deref(), Some("Code"));
        assert_eq!(process_stem(r"C:\Windows\notepad.EXE").as_deref(), Some("notepad"));
        assert_eq!(process_stem(r"C:\a.b\my.app.exe").as_deref(), Some("my.app"));
        assert_eq!(process_stem("/usr/bin/kate").as_deref(), Some("kate"));
        assert_eq!(process_stem(r"C:\tools\.hidden").as_deref(), Some(".hidden"));
        assert_eq!(process_stem("plain").as_deref(), Some("plain"));
        assert_eq!(process_stem(""), None);
        assert_eq!(process_stem(r"C:\dir\"), None);
    }
}
