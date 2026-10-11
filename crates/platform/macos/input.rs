use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use said_core::{Hotkey, Result};

use super::decoder::{Decoder, KeyEvent, Shortcuts};
use super::ffi::{self, Owned, Ref};
use super::permissions::{Permissions, secure_input_enabled, session_locked};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputStatus {
    Available,
    SecureInput,
    SessionLocked,
    PermissionRequired,
    TapInterrupted,
}

pub struct Input {
    hotkeys: Receiver<Hotkey>,
    statuses: Receiver<InputStatus>,
    recording: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
struct Tap {
    decoder: Decoder,
    hotkeys: Sender<Hotkey>,
    statuses: Sender<InputStatus>,
    recording: Arc<AtomicBool>,
    status: InputStatus,
}
impl Tap {
    fn status(&mut self, status: InputStatus) {
        if self.status != status {
            self.status = status;
            if status != InputStatus::Available {
                self.decoder.reset();
            }
            let _ = self.statuses.send(status);
        }
    }
}

unsafe fn key_event(kind: u32, event: Ref) -> KeyEvent {
    KeyEvent {
        kind,
        keycode: unsafe { ffi::CGEventGetIntegerValueField(event, 9) } as u16,
        flags: unsafe { ffi::CGEventGetFlags(event) },
        repeat: unsafe { ffi::CGEventGetIntegerValueField(event, 8) } != 0,
    }
}
unsafe extern "C" fn callback(_: Ref, kind: u32, event: Ref, context: Ref) -> Ref {
    let tap = unsafe { &mut *context.cast::<Tap>() };
    if kind == u32::MAX || kind == u32::MAX - 1 {
        tap.status(InputStatus::TapInterrupted);
        return event;
    }
    if tap.status != InputStatus::Available {
        return event;
    }
    let result = tap.decoder.decode(
        unsafe { key_event(kind, event) },
        tap.recording.load(Ordering::Acquire),
    );
    if let Some(hotkey) = result.hotkey {
        let _ = tap.hotkeys.send(hotkey);
    }
    if result.suppress {
        std::ptr::null_mut()
    } else {
        event
    }
}

impl Input {
    pub fn start(shortcuts: Shortcuts) -> Result<Self> {
        shortcuts.validate()?;
        let permissions = Permissions::check();
        if !permissions.accessibility || !permissions.input_monitoring {
            return Err(permissions.recovery().join(" ").into());
        }
        let (sender, hotkeys) = unbounded();
        let (status_sender, statuses) = unbounded();
        let (ready_sender, ready) = bounded(1);
        let recording = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_recording = recording.clone();
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("macos-hotkey".into())
            .spawn(move || {
                let result = run(
                    shortcuts,
                    sender,
                    status_sender,
                    worker_recording,
                    worker_stop,
                    &ready_sender,
                );
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error.to_string()));
                }
            })?;
        match ready.recv()? {
            Ok(()) => Ok(Self {
                hotkeys,
                statuses,
                recording,
                stop,
                worker: Some(worker),
            }),
            Err(error) => {
                let _ = worker.join();
                Err(error.into())
            }
        }
    }
    pub fn hotkeys(&self) -> Receiver<Hotkey> {
        self.hotkeys.clone()
    }
    pub fn statuses(&self) -> Receiver<InputStatus> {
        self.statuses.clone()
    }
    pub fn set_recording(&self, recording: bool) {
        self.recording.store(recording, Ordering::Release);
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    shortcuts: Shortcuts,
    hotkeys: Sender<Hotkey>,
    statuses: Sender<InputStatus>,
    recording: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    ready: &Sender<std::result::Result<(), String>>,
) -> Result<()> {
    let mut tap = Tap {
        decoder: Decoder::new(shortcuts),
        hotkeys,
        statuses,
        recording,
        status: InputStatus::TapInterrupted,
    };
    // A filtering session tap suppresses the shortcut without requiring a root HID tap.
    let port = Owned::new(unsafe { ffi::CGEventTapCreate(1, 0, 0, (1 << 10) | (1 << 11) | (1 << 12), callback, (&mut tap as *mut Tap).cast()) })
        .map_err(|_| "Cannot install the hotkey tap. Enable Accessibility and Input Monitoring for this signed app, then quit and reopen it.")?;
    let source = match Owned::new(unsafe {
        ffi::CFMachPortCreateRunLoopSource(std::ptr::null_mut(), port.0, 0)
    }) {
        Ok(source) => source,
        Err(error) => {
            unsafe { ffi::CFMachPortInvalidate(port.0) };
            return Err(error);
        }
    };
    unsafe {
        ffi::CFRunLoopAddSource(
            ffi::CFRunLoopGetCurrent(),
            source.0,
            ffi::kCFRunLoopDefaultMode,
        );
    }
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Acquire) {
        let status = if secure_input_enabled() {
            InputStatus::SecureInput
        } else if session_locked() {
            InputStatus::SessionLocked
        } else if !unsafe { ffi::AXIsProcessTrusted() && ffi::CGPreflightListenEventAccess() } {
            InputStatus::PermissionRequired
        } else {
            InputStatus::Available
        };
        tap.status(status);
        if status == InputStatus::Available && !unsafe { ffi::CGEventTapIsEnabled(port.0) } {
            unsafe { ffi::CGEventTapEnable(port.0, true) };
            if !unsafe { ffi::CGEventTapIsEnabled(port.0) } {
                tap.status(InputStatus::TapInterrupted);
            }
        }
        // Secure Input has no public change notification; each poll reads actual OS state.
        unsafe {
            ffi::CFRunLoopRunInMode(
                ffi::kCFRunLoopDefaultMode,
                Duration::from_millis(100).as_secs_f64(),
                false,
            );
        }
    }
    unsafe {
        ffi::CGEventTapEnable(port.0, false);
        ffi::CFMachPortInvalidate(port.0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::decoder::{CONTROL, OPTION};
    use super::*;
    #[test]
    fn quartz_events_run_the_production_callback_without_installing_a_tap() {
        let (hotkeys, receiver) = unbounded();
        let (statuses, _) = unbounded();
        let mut tap = Tap {
            decoder: Decoder::new(Shortcuts {
                dictation: super::super::decoder::Shortcut {
                    keycode: 49,
                    modifiers: super::super::decoder::CONTROL | super::super::decoder::OPTION,
                },
                ..Shortcuts::default()
            }),
            hotkeys,
            statuses,
            recording: Arc::new(AtomicBool::new(false)),
            status: InputStatus::Available,
        };
        let context = (&mut tap as *mut Tap).cast();
        let down =
            Owned::new(unsafe { ffi::CGEventCreateKeyboardEvent(std::ptr::null_mut(), 49, true) })
                .unwrap();
        unsafe {
            ffi::CGEventSetFlags(down.0, CONTROL | OPTION);
        }
        assert!(unsafe { callback(std::ptr::null_mut(), 10, down.0, context) }.is_null());
        assert_eq!(receiver.try_recv().unwrap(), Hotkey::Down);
        unsafe {
            ffi::CGEventSetIntegerValueField(down.0, 8, 1);
        }
        assert!(unsafe { callback(std::ptr::null_mut(), 10, down.0, context) }.is_null());
        assert!(receiver.is_empty());
        let up =
            Owned::new(unsafe { ffi::CGEventCreateKeyboardEvent(std::ptr::null_mut(), 49, false) })
                .unwrap();
        unsafe {
            ffi::CGEventSetFlags(up.0, 0);
        }
        assert!(unsafe { callback(std::ptr::null_mut(), 11, up.0, context) }.is_null());
        assert_eq!(receiver.try_recv().unwrap(), Hotkey::Up);
        let escape =
            Owned::new(unsafe { ffi::CGEventCreateKeyboardEvent(std::ptr::null_mut(), 53, true) })
                .unwrap();
        assert_eq!(
            unsafe { callback(std::ptr::null_mut(), 10, escape.0, context) },
            escape.0
        );
        assert!(receiver.is_empty());
        tap.recording.store(true, Ordering::Release);
        assert!(unsafe { callback(std::ptr::null_mut(), 10, escape.0, context) }.is_null());
        assert_eq!(receiver.try_recv().unwrap(), Hotkey::Cancel);
    }
    #[test]
    fn interrupted_tap_publishes_unavailability_and_resets_edges() {
        let (hotkeys, receiver) = unbounded();
        let (statuses, status_receiver) = unbounded();
        let mut tap = Tap {
            decoder: Decoder::new(Shortcuts {
                dictation: super::super::decoder::Shortcut {
                    keycode: 49,
                    modifiers: super::super::decoder::CONTROL | super::super::decoder::OPTION,
                },
                ..Shortcuts::default()
            }),
            hotkeys,
            statuses,
            recording: Arc::new(AtomicBool::new(false)),
            status: InputStatus::Available,
        };
        tap.decoder.decode(
            KeyEvent {
                kind: 10,
                keycode: 49,
                flags: CONTROL | OPTION,
                repeat: false,
            },
            false,
        );
        unsafe {
            callback(
                std::ptr::null_mut(),
                u32::MAX - 1,
                std::ptr::null_mut(),
                (&mut tap as *mut Tap).cast(),
            );
        }
        assert_eq!(
            status_receiver.try_recv().unwrap(),
            InputStatus::TapInterrupted
        );
        assert!(receiver.is_empty());
        assert_eq!(
            tap.decoder
                .decode(
                    KeyEvent {
                        kind: 11,
                        keycode: 49,
                        flags: 0,
                        repeat: false
                    },
                    false
                )
                .hotkey,
            None
        );
    }
}
