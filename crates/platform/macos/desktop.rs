use std::ptr::null_mut;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use said_core::{Result, Target};

use super::decoder::COMMAND;
use super::ffi::{self, Owned, Ref};
use super::{objc, permissions};

struct WindowIdentity {
    pid: i32,
    target: Target,
    element: Owned,
    application: objc::Retained,
}
// AX references may cross threads; all registry and AppKit work is serialized on the main queue.
unsafe impl Send for WindowIdentity {}
#[derive(Default)]
struct Registry {
    next: u64,
    windows: Vec<WindowIdentity>,
}
#[derive(Default)]
pub struct Desktop {
    registry: Mutex<Registry>,
}

fn attribute(element: Ref, name: &str) -> Result<Owned> {
    let name = Owned::string(name);
    let mut value = null_mut();
    let status = unsafe { ffi::AXUIElementCopyAttributeValue(element, name.0, &mut value) };
    if status != 0 {
        return Err(format!("Cannot read focused-window identity (Accessibility error {status}). Enable Accessibility or use Copy and paste manually.").into());
    }
    Owned::new(value)
}

impl Desktop {
    pub fn focused(&self) -> Result<Target> {
        objc::main_thread(|| objc::autorelease(|| self.focused_main()))
    }
    fn focused_main(&self) -> Result<Target> {
        if !unsafe { ffi::AXIsProcessTrusted() } {
            return Err("Enable this signed app in System Settings > Privacy & Security > Accessibility, then quit and reopen it.".into());
        }
        unsafe {
            let workspace = objc::send::<Ref>(objc::class(c"NSWorkspace"), c"sharedWorkspace");
            let app = objc::send::<Ref>(workspace, c"frontmostApplication");
            if app.is_null() {
                return Err("No focused application".into());
            }
            let pid: i32 = objc::send(app, c"processIdentifier");
            let app_id = objc::string(objc::send(app, c"bundleIdentifier"))
                .unwrap_or_else(|| format!("pid:{pid}"));
            let name = objc::string(objc::send(app, c"localizedName"))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| app_id.clone());
            let application = Owned::new(ffi::AXUIElementCreateApplication(pid))?;
            ffi::AXUIElementSetMessagingTimeout(application.0, 0.2);
            let window = attribute(application.0, "AXFocusedWindow")?;
            let mut registry = self.registry.lock().unwrap();
            if let Some(existing) = registry.windows.iter().find(|entry| {
                entry.pid == pid
                    && !objc::send::<bool>(entry.application.0, c"isTerminated")
                    && entry.target.app_id == app_id
                    && ffi::CFEqual(entry.element.0, window.0)
            }) {
                return Ok(Target {
                    name,
                    ..existing.target.clone()
                });
            }
            registry.next += 1;
            let target = Target {
                app_id,
                window_id: format!("{pid}:{}", registry.next),
                name,
            };
            registry.windows.push(WindowIdentity {
                pid,
                target: target.clone(),
                element: window,
                application: objc::Retained::new(app),
            });
            Ok(target)
        }
    }
    fn pid(&self, target: &Target) -> Result<i32> {
        self.registry
            .lock()
            .unwrap()
            .windows
            .iter()
            .find(|entry| entry.target.same_window(target))
            .map(|entry| entry.pid)
            .ok_or_else(|| {
                "Target window is no longer known; insert into the current app explicitly.".into()
            })
    }
    fn check_focus(&self, target: &Target) -> Result<()> {
        if self.focused_main()?.same_window(target) {
            Ok(())
        } else {
            Err("Focus moved. Text is saved; use Insert last or paste manually.".into())
        }
    }
    pub fn focus(&self, target: &Target) -> Result<()> {
        objc::main_thread(|| {
            objc::autorelease(|| unsafe {
                let (pid, window, running) = {
                    let registry = self.registry.lock().unwrap();
                    let entry = registry
                        .windows
                        .iter()
                        .find(|entry| entry.target.same_window(target))
                        .ok_or("Target window is no longer known")?;
                    (entry.pid, entry.element.0, entry.application.0)
                };
                if !ffi::AXIsProcessTrusted() {
                    return Err("Accessibility is required to refocus the target window".into());
                }
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
    pub fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        objc::main_thread(|| {
            objc::autorelease(|| unsafe {
                if permissions::secure_input_enabled() || permissions::session_locked() {
                    return Err("Input unavailable while Secure Input or session lock is active. Text is saved; paste after unlocking.".into());
                }
                if !ffi::AXIsProcessTrusted() || !ffi::CGPreflightPostEventAccess() {
                    return Err("Enable this signed app in System Settings > Privacy & Security > Accessibility, then quit and reopen it.".into());
                }
                self.check_focus(target)?;
                let pid = self.pid(target)?;
                let key = command_v_keycode()?;
                let down = keyboard_event(key, true, COMMAND)?;
                let up = keyboard_event(key, false, COMMAND)?;
                let enter = if send {
                    Some((keyboard_event(36, true, 0)?, keyboard_event(36, false, 0)?))
                } else {
                    None
                };
                write_clipboard(text)?;
                self.check_focus(target)?;
                if permissions::secure_input_enabled() || permissions::session_locked() {
                    return Err("Input became unavailable. Text remains on the clipboard.".into());
                }
                ffi::CGEventPostToPid(pid, down.0);
                ffi::CGEventPostToPid(pid, up.0);
                if let Some((down, up)) = enter {
                    std::thread::sleep(Duration::from_millis(50));
                    self.check_focus(target)?;
                    if permissions::secure_input_enabled() || permissions::session_locked() {
                        return Err(
                            "Paste posted; Enter withheld because input became unavailable.".into(),
                        );
                    }
                    ffi::CGEventPostToPid(pid, down.0);
                    ffi::CGEventPostToPid(pid, up.0);
                }
                Ok(())
            })
        })
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
