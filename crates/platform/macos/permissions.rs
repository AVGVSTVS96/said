use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{ffi, objc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicrophonePermission {
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub microphone: MicrophonePermission,
}
impl Permissions {
    pub fn check() -> Self {
        let microphone = objc::autorelease(|| unsafe {
            match objc::send1::<_, isize>(
                objc::class(c"AVCaptureDevice"),
                c"authorizationStatusForMediaType:",
                ffi::AVMediaTypeAudio,
            ) {
                3 => MicrophonePermission::Authorized,
                2 => MicrophonePermission::Denied,
                1 => MicrophonePermission::Restricted,
                _ => MicrophonePermission::NotDetermined,
            }
        });
        Self {
            accessibility: unsafe { ffi::AXIsProcessTrusted() },
            input_monitoring: unsafe { ffi::CGPreflightListenEventAccess() },
            microphone,
        }
    }
    pub fn recovery(self) -> Vec<&'static str> {
        let mut steps = Vec::new();
        if !self.accessibility {
            steps.push("Turn on this app in System Settings > Privacy & Security > Accessibility. Shortcuts and paste start as soon as it is on.");
        }
        match self.microphone {
            MicrophonePermission::NotDetermined => {
                steps.push("Press Allow next to Microphone, then choose Allow.")
            }
            MicrophonePermission::Denied => steps
                .push("Turn on this app in System Settings > Privacy & Security > Microphone."),
            MicrophonePermission::Restricted => steps.push(
                "Microphone access is restricted by system policy. Ask the device administrator to allow it.",
            ),
            MicrophonePermission::Authorized => {}
        }
        steps
    }
}

static ACCESSIBILITY_PROMPTED: AtomicBool = AtomicBool::new(false);
static INPUT_MONITORING_PROMPTED: AtomicBool = AtomicBool::new(false);

/// Shows the system Accessibility prompt once per launch; false means it was already shown.
pub fn prompt_accessibility() -> bool {
    if ACCESSIBILITY_PROMPTED.swap(true, Ordering::AcqRel) {
        return false;
    }
    unsafe {
        let keys = [ffi::kAXTrustedCheckOptionPrompt];
        let values = [ffi::kCFBooleanTrue];
        if let Ok(options) = ffi::Owned::new(ffi::CFDictionaryCreate(
            null_mut(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &raw const ffi::kCFTypeDictionaryKeyCallBacks,
            &raw const ffi::kCFTypeDictionaryValueCallBacks,
        )) {
            ffi::AXIsProcessTrustedWithOptions(options.0);
        }
    }
    true
}
pub fn accessibility_prompted() -> bool {
    ACCESSIBILITY_PROMPTED.load(Ordering::Acquire)
}
/// Lists the app under Input Monitoring once per launch; false means it was already requested.
pub fn prompt_input_monitoring() -> bool {
    if INPUT_MONITORING_PROMPTED.swap(true, Ordering::AcqRel) {
        return false;
    }
    unsafe { ffi::CGRequestListenEventAccess() };
    true
}

pub fn secure_input_enabled() -> bool {
    unsafe { ffi::IsSecureEventInputEnabled() != 0 }
}

pub fn session_locked() -> bool {
    let session = unsafe { ffi::CGSessionCopyCurrentDictionary() };
    if session.is_null() {
        return true;
    }
    let session = ffi::Owned(session);
    session_unavailable(|key| unsafe {
        let key = ffi::Owned::string(key);
        let value = ffi::CFDictionaryGetValue(session.0, key.0);
        if value.is_null() || ffi::CFGetTypeID(value) != ffi::CFBooleanGetTypeID() {
            None
        } else {
            Some(ffi::CFBooleanGetValue(value))
        }
    })
}

fn session_unavailable(mut value: impl FnMut(&str) -> Option<bool>) -> bool {
    // The public macro kCGSessionOnConsoleKey expands to this differently named CFString.
    value("CGSSessionScreenIsLocked") == Some(true)
        || value("kCGSSessionOnConsoleKey") == Some(false)
        || value("kCGSessionLoginDoneKey") == Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_logout_and_console_switch_each_make_input_unavailable() {
        assert!(!session_unavailable(|_| None));
        for (key, state) in [
            ("CGSSessionScreenIsLocked", true),
            ("kCGSSessionOnConsoleKey", false),
            ("kCGSessionLoginDoneKey", false),
        ] {
            assert!(session_unavailable(|name| (name == key).then_some(state)));
        }
    }
    #[test]
    fn recovery_names_only_missing_grants_and_never_asks_for_a_relaunch() {
        let mut permissions = Permissions {
            accessibility: true,
            input_monitoring: false,
            microphone: MicrophonePermission::Authorized,
        };
        assert!(permissions.recovery().is_empty());
        permissions.microphone = MicrophonePermission::Denied;
        assert!(permissions.recovery()[0].contains("System Settings"));
        permissions.microphone = MicrophonePermission::Restricted;
        assert!(permissions.recovery()[0].contains("administrator"));
        permissions.microphone = MicrophonePermission::NotDetermined;
        assert!(permissions.recovery()[0].contains("Press Allow"));
        permissions.accessibility = false;
        assert!(permissions.recovery()[0].contains("Accessibility"));
        assert!(
            permissions
                .recovery()
                .iter()
                .all(|step| !step.contains("reopen") && !step.contains("Input Monitoring"))
        );
    }
    #[test]
    #[ignore = "read-only diagnostic of the current process's grants"]
    fn permission_report() {
        println!("{:?}", Permissions::check());
        println!(
            "secure_input={} session_locked={}",
            secure_input_enabled(),
            session_locked()
        );
    }
}
