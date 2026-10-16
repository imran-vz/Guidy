//! OS permission checks (replacement for tauri-plugin-macos-permissions).
//!
//! macOS: Accessibility (needed to synthesize paste keystrokes and observe
//! global keys) and Microphone. Other platforms report granted; Windows
//! microphone privacy is handled in `commands::audio`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Denied,
    /// Not asked yet (macOS microphone).
    NotDetermined,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::PermissionState;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::{NSDictionary, NSNumber, NSString};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    pub fn accessibility() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Shows the system prompt that deep-links to Privacy & Security ›
    /// Accessibility (no-op once granted).
    pub fn request_accessibility() -> bool {
        let key = NSString::from_str("AXTrustedCheckOptionPrompt");
        let value = NSNumber::new_bool(true);
        let dict: Retained<NSDictionary<NSString, NSNumber>> =
            NSDictionary::from_retained_objects(&[&*key], &[value]);
        unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&dict) as *const _) }
    }

    fn capture_device() -> Option<&'static AnyClass> {
        AnyClass::get(c"AVCaptureDevice")
    }

    pub fn microphone() -> PermissionState {
        let Some(cls) = capture_device() else {
            return PermissionState::Granted;
        };
        let media = NSString::from_str("soun"); // AVMediaTypeAudio
        let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: &*media] };
        match status {
            3 => PermissionState::Granted,
            0 => PermissionState::NotDetermined,
            _ => PermissionState::Denied,
        }
    }

    pub fn request_microphone() {
        let Some(cls) = capture_device() else {
            return;
        };
        let media = NSString::from_str("soun");
        let handler = block2::RcBlock::new(|granted: Bool| {
            log::info!(
                "Microphone permission request finished: granted={}",
                granted.as_bool()
            );
        });
        let _: () = unsafe {
            msg_send![cls, requestAccessForMediaType: &*media, completionHandler: &*handler]
        };
        let _ = std::ptr::null::<AnyObject>();
    }
}

/// Whether the process may control the computer (Accessibility).
pub fn check_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    return imp::accessibility();
    #[cfg(not(target_os = "macos"))]
    true
}

/// Prompt for Accessibility. Returns the state after prompting.
pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    return imp::request_accessibility();
    #[cfg(not(target_os = "macos"))]
    true
}

pub fn microphone_permission_state() -> PermissionState {
    #[cfg(target_os = "macos")]
    return imp::microphone();
    #[cfg(not(target_os = "macos"))]
    PermissionState::Granted
}

pub fn check_microphone_permission() -> bool {
    microphone_permission_state() == PermissionState::Granted
}

/// Ask for microphone access. When the user already denied it, the system
/// won't prompt again, so open the privacy pane instead.
pub fn request_microphone_permission() {
    #[cfg(target_os = "macos")]
    {
        match imp::microphone() {
            PermissionState::NotDetermined => imp::request_microphone(),
            PermissionState::Denied => {
                let _ = open::that_detached(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
                );
            }
            PermissionState::Granted => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_state_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&PermissionState::NotDetermined).unwrap(),
            "\"not_determined\""
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_mac_platforms_report_granted() {
        assert!(check_accessibility_permission());
        assert!(check_microphone_permission());
    }
}
