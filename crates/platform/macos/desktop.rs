use std::ptr::null_mut;
use std::time::{Duration, Instant};

use said_core::{Result, Target};

use super::decoder::COMMAND;
use super::ffi::{self, Owned, Ref};
use super::focus::Focus;
use super::{objc, permissions};

const HID_EVENT_TAP: u32 = 0;
const COMMAND_KEY: u16 = 55;
const RETURN_KEY: u16 = 36;

#[derive(Default)]
pub struct Desktop {
    focus: Focus,
}

impl Desktop {
    pub fn observing(&self) -> bool {
        self.focus.observing()
    }
    pub fn observe(&self) -> Result<()> {
        objc::main_thread(|| objc::autorelease(|| self.focus.observe()))
    }
    pub fn focused(&self) -> Result<Target> {
        objc::main_thread(|| objc::autorelease(|| self.focus.focused()))
    }
    pub fn focus_changes(&self) -> Vec<Option<Target>> {
        self.focus.events()
    }
    fn check_focus(&self, target: &Target) -> Result<()> {
        if self.focus.focused()?.same_window(target) {
            Ok(())
        } else {
            Err("Focus moved. Text is saved; use Insert last or paste manually.".into())
        }
    }
    fn ready(&self, target: &Target) -> Result<()> {
        if permissions::secure_input_enabled() || permissions::session_locked() {
            return Err("Input unavailable while Secure Input or session lock is active. Text is saved; paste after unlocking.".into());
        }
        self.check_focus(target)
    }
    pub fn focus(&self, target: &Target) -> Result<()> {
        objc::main_thread(|| {
            objc::autorelease(|| unsafe {
                if !ffi::AXIsProcessTrusted() {
                    return Err("Accessibility is required to refocus the target window".into());
                }
                let (pid, window, running) = self
                    .focus
                    .window(target)
                    .ok_or("Target window is no longer known")?;
                if running.is_null() || objc::send::<bool>(running, c"isTerminated") {
                    return Err("Target app closed".into());
                }
                let application = Owned::new(ffi::AXUIElementCreateApplication(pid))?;
                ffi::AXUIElementSetMessagingTimeout(application.0, 0.2);
                let focused = Owned::string("AXFocusedWindow");
                let raise = Owned::string("AXRaise");
                if ffi::AXUIElementPerformAction(window, raise.0) != 0
                    || ffi::AXUIElementSetAttributeValue(application.0, focused.0, window) != 0
                {
                    return Err(
                        "Cannot refocus the saved window. Select it manually, then insert again."
                            .into(),
                    );
                }
                if !objc::send1::<_, bool>(running, c"activateWithOptions:", 2_usize) {
                    return Err("Target app refused activation".into());
                }
                let deadline = Instant::now() + Duration::from_millis(500);
                loop {
                    if self.check_focus(target).is_ok() {
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        return Err("Target window did not gain focus. Select it manually, then insert again.".into());
                    }
                    // Activation completes asynchronously; service the main loop until actual window focus changes.
                    ffi::CFRunLoopRunInMode(ffi::kCFRunLoopDefaultMode, 0.01, true);
                }
            })
        })
    }
    pub fn copy(&self, text: &str) -> Result<()> {
        objc::main_thread(|| objc::autorelease(|| write_clipboard(text)))
    }
    pub fn paste(&self, target: &Target, text: &str, send: bool, automatic: bool) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let key = objc::main_thread(|| {
            objc::autorelease(|| {
                self.ready(target)?;
                let key = command_v_keycode()?;
                write_clipboard(text)?;
                Result::Ok(key)
            })
        })?;
        std::thread::sleep(Duration::from_millis(50));
        objc::main_thread(|| objc::autorelease(|| self.ready(target)))
            .map_err(|error| format!("{error} Text remains on the clipboard."))?;
        self.focus.post(target, automatic, || {
            post(&[
                keyboard_event(COMMAND_KEY, true, COMMAND)?,
                keyboard_event(key, true, COMMAND)?,
                keyboard_event(key, false, COMMAND)?,
            ]);
            // Hold Command briefly like a person; some apps read the modifier state after V.
            std::thread::sleep(Duration::from_millis(100));
            post(&[keyboard_event(COMMAND_KEY, false, 0)?]);
            Ok(())
        })?;
        if send {
            std::thread::sleep(Duration::from_millis(50));
            objc::main_thread(|| objc::autorelease(|| self.ready(target)))
                .map_err(|error| format!("Paste posted; Enter withheld. {error}"))?;
            post(&[
                keyboard_event(RETURN_KEY, true, 0)?,
                keyboard_event(RETURN_KEY, false, 0)?,
            ]);
        }
        Ok(())
    }
}

fn post(events: &[Owned]) {
    for event in events {
        unsafe { ffi::CGEventPost(HID_EVENT_TAP, event.0) };
    }
}

fn write_clipboard(text: &str) -> Result<()> {
    unsafe {
        let clipboard = objc::send::<Ref>(objc::class(c"NSPasteboard"), c"generalPasteboard");
        let payload = Owned::string(text);
        let plain_text = Owned::string("public.utf8-plain-text");
        objc::send::<isize>(clipboard, c"clearContents");
        if objc::send2::<_, _, bool>(clipboard, c"setString:forType:", payload.0, plain_text.0) {
            Ok(())
        } else {
            Err("Cannot write clipboard. Text is saved; use Copy again.".into())
        }
    }
}

fn keyboard_event(key: u16, down: bool, flags: u64) -> Result<Owned> {
    let event = Owned::new(unsafe { ffi::CGEventCreateKeyboardEvent(null_mut(), key, down) })?;
    unsafe { ffi::CGEventSetFlags(event.0, flags) };
    Ok(event)
}

fn find_command_v(mut translate: impl FnMut(u16) -> Option<char>) -> Result<u16> {
    (0..128).find(|&key| translate(key) == Some('v')).ok_or_else(|| "Cannot map Cmd+V in the current keyboard layout. Text is saved; use Copy and paste manually.".into())
}
fn command_v_keycode() -> Result<u16> {
    unsafe {
        let source = Owned::new(ffi::TISCopyCurrentKeyboardLayoutInputSource())?;
        let data = ffi::TISGetInputSourceProperty(source.0, ffi::kTISPropertyUnicodeKeyLayoutData);
        if data.is_null() {
            return Err(
                "Current keyboard layout has no Unicode mapping. Use Copy and paste manually."
                    .into(),
            );
        }
        let layout = ffi::CFDataGetBytePtr(data);
        if layout.is_null() {
            return Err("Current keyboard layout mapping is empty".into());
        }
        // Command is Carbon bit 8, shifted right by 8 for UCKeyTranslate. This handles Dvorak and non-Latin command layouts.
        let keyboard = u32::from(ffi::LMGetKbdType());
        find_command_v(|key| {
            let mut dead = 0;
            let mut length = 0;
            let mut chars = [0_u16; 4];
            let status = ffi::UCKeyTranslate(
                layout,
                key,
                3,
                1,
                keyboard,
                1,
                &mut dead,
                chars.len(),
                &mut length,
                chars.as_mut_ptr(),
            );
            if status == 0 && length == 1 {
                char::from_u32(u32::from(chars[0]))
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paste_key_comes_from_the_command_layout_and_missing_mapping_fails() {
        assert_eq!(
            find_command_v(|key| (key == 47).then_some('v')).unwrap(),
            47
        );
        assert!(find_command_v(|_| None).is_err());
        assert!(find_command_v(|_| Some('V')).is_err());
    }
    #[test]
    fn paste_event_flags_are_explicit_and_enter_is_unmodified() {
        let event = keyboard_event(47, true, COMMAND).unwrap();
        assert_eq!(unsafe { ffi::CGEventGetIntegerValueField(event.0, 9) }, 47);
        assert_eq!(unsafe { ffi::CGEventGetFlags(event.0) }, COMMAND);
        let enter = keyboard_event(36, true, 0).unwrap();
        assert_eq!(unsafe { ffi::CGEventGetFlags(enter.0) }, 0);
    }
}
