use std::process::Command;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_application_services::{kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_core_foundation::{kCFBooleanTrue, CFDictionary};
use objc2_foundation::NSBundle;
use scribe_core::permissions::{Permission, PermissionState, PermissionStatus, SystemPermissions};

pub struct MacPermissions;

pub fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

fn microphone() -> PermissionState {
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else { return PermissionState::Denied };
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) } {
        AVAuthorizationStatus::Authorized => PermissionState::Granted,
        AVAuthorizationStatus::NotDetermined => PermissionState::NotDetermined,
        _ => PermissionState::Denied,
    }
}

impl SystemPermissions for MacPermissions {
    /// Accessibility is never « denied »: its prompt can always be shown again.
    fn status(&self) -> Vec<PermissionStatus> {
        let accessibility = if accessibility_granted() { PermissionState::Granted } else { PermissionState::NotDetermined };
        vec![
            PermissionStatus { permission: Permission::Accessibility, state: accessibility },
            PermissionStatus { permission: Permission::Microphone, state: microphone() },
        ]
    }

    fn request(&self, permission: Permission) {
        match permission {
            // The ad-hoc signature changes with every build: an entry granted to an older build stays checked but no
            // longer applies. Resetting it first, the prompt then adds Scribe again, unchecked, for this build.
            Permission::Accessibility => {
                if let Some(id) = NSBundle::mainBundle().bundleIdentifier() {
                    let _ = Command::new("tccutil").args(["reset", "Accessibility", &id.to_string()]).status();
                }
                let options = CFDictionary::from_slices(&[unsafe { kAXTrustedCheckOptionPrompt }], &[unsafe { kCFBooleanTrue }.unwrap()]);
                unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
            }
            Permission::Microphone => {
                if let Some(audio) = unsafe { AVMediaTypeAudio } {
                    let done = RcBlock::new(|_granted: Bool| {});
                    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &done) };
                }
            }
        }
    }

    fn open_settings(&self, permission: Permission) -> Result<(), String> {
        let pane = match permission {
            Permission::Accessibility => "Privacy_Accessibility",
            Permission::Microphone => "Privacy_Microphone",
        };
        Command::new("open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}"))
            .status()
            .map_err(|e| e.to_string())
            .and_then(|s| if s.success() { Ok(()) } else { Err(format!("ouverture des Réglages impossible ({s})")) })
    }
}
