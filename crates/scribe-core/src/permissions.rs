//! OS permissions Scribe needs to work (macOS privacy settings). Windows asks for none.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Global shortcut, simulated paste and reading the focused field.
    Accessibility,
    Microphone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    /// The OS can still show its prompt (`SystemPermissions::request`).
    NotDetermined,
    /// Refused, or only grantable in the OS settings.
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PermissionStatus {
    pub permission: Permission,
    pub state: PermissionState,
}

pub trait SystemPermissions: Send + Sync {
    /// The permissions this OS asks for, with their current state (empty when it asks for none).
    fn status(&self) -> Vec<PermissionStatus>;
    /// Shows the OS prompt when it still can; the new state is read again with `status`.
    fn request(&self, permission: Permission);
    /// Opens the OS settings page where the permission is granted.
    fn open_settings(&self, permission: Permission) -> Result<(), String>;
}

pub fn any_missing(statuses: &[PermissionStatus]) -> bool {
    statuses.iter().any(|s| s.state != PermissionState::Granted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(permission: Permission, state: PermissionState) -> PermissionStatus {
        PermissionStatus { permission, state }
    }

    #[test]
    fn missing_means_any_permission_not_granted() {
        assert!(!any_missing(&[]), "an OS that asks for nothing");
        assert!(!any_missing(&[status(Permission::Accessibility, PermissionState::Granted)]));
        assert!(any_missing(&[
            status(Permission::Accessibility, PermissionState::Granted),
            status(Permission::Microphone, PermissionState::NotDetermined),
        ]));
        assert!(any_missing(&[status(Permission::Accessibility, PermissionState::Denied)]));
    }

    #[test]
    fn serializes_for_the_ui() {
        let json = serde_json::to_string(&status(Permission::Microphone, PermissionState::NotDetermined)).unwrap();
        assert_eq!(json, r#"{"permission":"microphone","state":"not_determined"}"#);
        assert_eq!(serde_json::from_str::<Permission>(r#""accessibility""#).unwrap(), Permission::Accessibility);
    }
}
