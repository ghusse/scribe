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
pub const DOCUMENT: i32 = 50030;
pub const PANE: i32 = 50033;

const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd"];
const NAVIGATION: &[i32] = &[BUTTON, LIST_ITEM, LIST, MENU_ITEM, TAB, TAB_ITEM, TREE, TREE_ITEM];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiaFacts {
    pub window_class: String,
    pub control_type: Option<i32>,
    pub value_read_only: Option<bool>,
}

pub fn classify(f: &UiaFacts) -> FocusState {
    if SHELL_CLASSES.contains(&f.window_class.as_str()) {
        return FocusState::NotEditable;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(class: &str, ct: Option<i32>, ro: Option<bool>) -> UiaFacts {
        UiaFacts { window_class: class.into(), control_type: ct, value_read_only: ro }
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
}
