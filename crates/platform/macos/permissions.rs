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
    pub event_posting: bool,
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
            event_posting: unsafe { ffi::CGPreflightPostEventAccess() },
            microphone,
        }
    }
    pub fn recovery(self) -> Vec<&'static str> {
        let mut steps = Vec::new();
        if !self.accessibility || !self.event_posting {
            steps.push("Enable this signed app in System Settings > Privacy & Security > Accessibility, then quit and reopen it.");
        }
        if !self.input_monitoring {
            steps.push("Enable this signed app in System Settings > Privacy & Security > Input Monitoring, then quit and reopen it.");
        }
        match self.microphone {
            MicrophonePermission::NotDetermined => steps.push("Use the app's Grant Microphone action once and choose Allow."),
            MicrophonePermission::Denied => steps.push("Enable this signed app in System Settings > Privacy & Security > Microphone, then quit and reopen it."),
            MicrophonePermission::Restricted => steps.push("Microphone access is restricted by system policy. Ask the device administrator to allow it."),
            MicrophonePermission::Authorized => {}
        }
        steps
    }
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
    fn recovery_distinguishes_denial_restriction_and_first_use() {
        let mut permissions = Permissions {
            accessibility: true,
            input_monitoring: true,
            event_posting: true,
            microphone: MicrophonePermission::Authorized,
        };
        assert!(permissions.recovery().is_empty());
        permissions.microphone = MicrophonePermission::Denied;
        assert!(permissions.recovery()[0].contains("System Settings"));
        permissions.microphone = MicrophonePermission::Restricted;
        assert!(permissions.recovery()[0].contains("administrator"));
        permissions.microphone = MicrophonePermission::NotDetermined;
        assert!(permissions.recovery()[0].contains("Grant Microphone"));
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
