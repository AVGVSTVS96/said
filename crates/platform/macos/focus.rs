use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

use said_core::{Result, Target};

use super::ffi::{self, Owned, Ref};
use super::objc;

const AX_CANNOT_COMPLETE: i32 = -25204;
const AX_INVALID_ELEMENT: i32 = -25202;

struct WindowIdentity {
    pid: i32,
    target: Target,
    element: Owned,
    application: objc::Retained,
}
struct Watch {
    pid: i32,
    observer: Owned,
    _application: Owned,
}
impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            ffi::CFRunLoopRemoveSource(
                ffi::CFRunLoopGetMain(),
                ffi::AXObserverGetRunLoopSource(self.observer.0),
                ffi::kCFRunLoopCommonModes,
            );
        }
    }
}

#[derive(Default)]
struct Windows {
    next: u64,
    known: Vec<WindowIdentity>,
    watch: Option<Watch>,
}
// AX and AppKit references are created, used and released only on the main queue.
unsafe impl Send for Windows {}

#[derive(Default)]
struct State {
    windows: Mutex<Windows>,
    current: Mutex<Option<Target>>,
    queue: crate::focus::FocusQueue,
}

struct Frontmost {
    application: Ref,
    pid: i32,
    app_id: String,
    name: String,
}
unsafe fn frontmost() -> Option<Frontmost> {
    unsafe {
        let workspace = objc::send::<Ref>(objc::class(c"NSWorkspace"), c"sharedWorkspace");
        let application = objc::send::<Ref>(workspace, c"frontmostApplication");
        if application.is_null() {
            return None;
        }
        let pid: i32 = objc::send(application, c"processIdentifier");
        let app_id = objc::string(objc::send(application, c"bundleIdentifier"))
            .unwrap_or_else(|| format!("pid:{pid}"));
        let name = objc::string(objc::send(application, c"localizedName"))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| app_id.clone());
        Some(Frontmost {
            application,
            pid,
            app_id,
            name,
        })
    }
}
unsafe fn attribute(element: Ref, name: &str) -> std::result::Result<Owned, i32> {
    let name = Owned::string(name);
    let mut value = null_mut();
    match unsafe { ffi::AXUIElementCopyAttributeValue(element, name.0, &mut value) } {
        0 => Owned::new(value).map_err(|_| 0),
        status => Err(status),
    }
}

impl State {
    fn record(&self, target: Option<Target>) {
        let mut current = self.current.lock().unwrap();
        let same = match (&*current, &target) {
            (Some(current), Some(target)) => current.same_window(target),
            (None, None) => true,
            _ => false,
        };
        if !same {
            *current = target.clone();
            self.queue.push(target);
        }
    }
    unsafe fn window(&self, app: &Frontmost) -> std::result::Result<Target, i32> {
        unsafe {
            let application =
                Owned::new(ffi::AXUIElementCreateApplication(app.pid)).map_err(|_| 0)?;
            ffi::AXUIElementSetMessagingTimeout(application.0, 0.2);
            let window = attribute(application.0, "AXFocusedWindow")?;
            let mut windows = self.windows.lock().unwrap();
            if let Some(existing) = windows.known.iter().find(|entry| {
                entry.pid == app.pid
                    && !objc::send::<bool>(entry.application.0, c"isTerminated")
                    && entry.target.app_id == app.app_id
                    && ffi::CFEqual(entry.element.0, window.0)
            }) {
                return Ok(Target {
                    name: app.name.clone(),
                    ..existing.target.clone()
                });
            }
            windows.known.retain(|entry| {
                !objc::send::<bool>(entry.application.0, c"isTerminated")
                    && attribute(entry.element.0, "AXRole")
                        .err()
                        .is_none_or(|status| status != AX_INVALID_ELEMENT)
            });
            ffi::AXUIElementSetMessagingTimeout(window.0, 0.2);
            windows.next += 1;
            let target = Target {
                app_id: app.app_id.clone(),
                window_id: format!("{}:{}", app.pid, windows.next),
                name: app.name.clone(),
            };
            windows.known.push(WindowIdentity {
                pid: app.pid,
                target: target.clone(),
                element: window,
                application: objc::Retained::new(app.application),
            });
            Ok(target)
        }
    }
    fn focused(&self) -> Result<Target> {
        if !unsafe { ffi::AXIsProcessTrusted() } {
            return Err("Turn on this app in System Settings > Privacy & Security > Accessibility to insert text.".into());
        }
        let app = unsafe { frontmost() }.ok_or("No focused application")?;
        let target = unsafe { self.window(&app) }.map_err(|status| {
            format!("Cannot read the focused window (Accessibility error {status}). Text is saved; use Copy and paste manually.")
        })?;
        self.record(Some(target.clone()));
        Ok(target)
    }
    fn changed(&self) {
        let Some(app) = (unsafe { frontmost() }) else {
            return self.record(None);
        };
        let _ = self.watch(app.pid);
        match unsafe { self.window(&app) } {
            Ok(target) => self.record(Some(target)),
            // A busy app timed out; only a different frontmost app proves a departure.
            Err(AX_CANNOT_COMPLETE)
                if self
                    .current
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|current| current.app_id == app.app_id) => {}
            Err(_) => self.record(None),
        }
    }
    fn watch(&self, pid: i32) -> Result<()> {
        unsafe {
            let mut windows = self.windows.lock().unwrap();
            if windows.watch.as_ref().is_some_and(|watch| watch.pid == pid) {
                return Ok(());
            }
            windows.watch = None;
            let application = Owned::new(ffi::AXUIElementCreateApplication(pid))?;
            ffi::AXUIElementSetMessagingTimeout(application.0, 0.2);
            let mut observer = null_mut();
            let status = ffi::AXObserverCreate(pid, window_changed, &mut observer);
            if status != 0 {
                return Err(format!(
                    "Cannot observe focused windows (Accessibility error {status}); use Copy."
                )
                .into());
            }
            let observer = Owned::new(observer)?;
            let notification = Owned::string("AXFocusedWindowChanged");
            let status = ffi::AXObserverAddNotification(
                observer.0,
                application.0,
                notification.0,
                (self as *const Self).cast_mut().cast(),
            );
            if status != 0 {
                return Err(format!(
                    "Cannot observe focused windows (Accessibility error {status}); use Copy."
                )
                .into());
            }
            ffi::CFRunLoopAddSource(
                ffi::CFRunLoopGetMain(),
                ffi::AXObserverGetRunLoopSource(observer.0),
                ffi::kCFRunLoopCommonModes,
            );
            windows.watch = Some(Watch {
                pid,
                observer,
                _application: application,
            });
            Ok(())
        }
    }
}
unsafe extern "C" fn window_changed(_: Ref, _: Ref, _: Ref, context: Ref) {
    objc::autorelease(|| unsafe { &*context.cast::<State>() }.changed());
}
unsafe extern "C" fn application_changed(object: Ref, _: Ref, _: Ref) {
    let mut context = null_mut();
    unsafe { ffi::object_getInstanceVariable(object, c"context".as_ptr(), &mut context) };
    if let Some(state) = unsafe { context.cast::<State>().as_ref() } {
        objc::autorelease(|| state.changed());
    }
}

fn observer_class() -> Ref {
    static CLASS: OnceLock<usize> = OnceLock::new();
    *CLASS.get_or_init(|| unsafe {
        let class =
            ffi::objc_allocateClassPair(objc::class(c"NSObject"), c"SaidFocusObserver".as_ptr(), 0);
        assert!(!class.is_null());
        assert!(ffi::class_addIvar(
            class,
            c"context".as_ptr(),
            std::mem::size_of::<Ref>(),
            std::mem::align_of::<Ref>().trailing_zeros() as u8,
            c"^v".as_ptr()
        ));
        assert!(ffi::class_addMethod(
            class,
            ffi::sel_registerName(c"focusChanged:".as_ptr()),
            application_changed as *const std::ffi::c_void,
            c"v@:@".as_ptr()
        ));
        ffi::objc_registerClassPair(class);
        class as usize
    }) as Ref
}

/// Window identities plus notification-driven focus changes. Every method that touches AX
/// or AppKit must run on the main queue.
#[derive(Default)]
pub struct Focus {
    state: Box<State>,
    object: Mutex<Option<objc::Retained>>,
}
// The retained notification object and AX references are only used on the main queue.
unsafe impl Send for Focus {}
unsafe impl Sync for Focus {}
impl Focus {
    pub fn observing(&self) -> bool {
        self.object.lock().unwrap().is_some() && self.state.windows.lock().unwrap().watch.is_some()
    }
    pub fn observe(&self) -> Result<()> {
        let mut object = self.object.lock().unwrap();
        if object.is_none() {
            unsafe {
                let observer = objc::send::<Ref>(observer_class(), c"new");
                ffi::object_setInstanceVariable(
                    observer,
                    c"context".as_ptr(),
                    (&*self.state as *const State).cast_mut().cast(),
                );
                let workspace = objc::send::<Ref>(objc::class(c"NSWorkspace"), c"sharedWorkspace");
                let center = objc::send::<Ref>(workspace, c"notificationCenter");
                let name = Owned::string("NSWorkspaceDidActivateApplicationNotification");
                objc::send4::<_, _, _, _, ()>(
                    center,
                    c"addObserver:selector:name:object:",
                    observer,
                    ffi::sel_registerName(c"focusChanged:".as_ptr()),
                    name.0,
                    null_mut::<std::ffi::c_void>(),
                );
                *object = Some(objc::Retained(observer));
            }
        }
        drop(object);
        if !unsafe { ffi::AXIsProcessTrusted() } {
            return Ok(());
        }
        let app = unsafe { frontmost() }.ok_or("No focused application")?;
        self.state.watch(app.pid)
    }
    pub fn focused(&self) -> Result<Target> {
        self.observe()?;
        self.state.focused()
    }
    /// The remembered window's process, AX element and NSRunningApplication.
    pub fn window(&self, target: &Target) -> Option<(i32, Ref, Ref)> {
        self.state
            .windows
            .lock()
            .unwrap()
            .known
            .iter()
            .find(|entry| entry.target.same_window(target))
            .map(|entry| (entry.pid, entry.element.0, entry.application.0))
    }
    pub fn events(&self) -> Vec<Option<Target>> {
        self.state.queue.drain()
    }
    pub fn post(
        &self,
        target: &Target,
        automatic: bool,
        post: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.state.queue.post(target, automatic, post)
    }
}
impl Focus {
    fn remove(&self) {
        unsafe {
            if let Some(object) = self.object.lock().unwrap().take() {
                let workspace = objc::send::<Ref>(objc::class(c"NSWorkspace"), c"sharedWorkspace");
                let center = objc::send::<Ref>(workspace, c"notificationCenter");
                objc::send1::<_, ()>(center, c"removeObserver:", object.0);
                ffi::object_setInstanceVariable(object.0, c"context".as_ptr(), null_mut());
            }
            let mut windows = self.state.windows.lock().unwrap();
            windows.watch.take();
            windows.known.clear();
        }
    }
}
impl Drop for Focus {
    fn drop(&mut self) {
        let windows = self.state.windows.get_mut().unwrap();
        if self.object.get_mut().unwrap().is_none()
            && windows.watch.is_none()
            && windows.known.is_empty()
        {
            return;
        }
        objc::main_thread(|| self.remove());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window(id: &str) -> Option<Target> {
        Some(Target {
            app_id: "scratch".into(),
            window_id: id.into(),
            name: "Scratch".into(),
        })
    }
    #[test]
    fn only_actual_window_changes_queue_and_departure_then_return_both_remain() {
        let state = State::default();
        state.record(window("A"));
        state.record(window("A"));
        state.record(Some(Target {
            name: "Renamed".into(),
            ..window("A").unwrap()
        }));
        assert_eq!(state.queue.drain(), [window("A")]);
        state.record(window("B"));
        state.record(None);
        state.record(None);
        state.record(window("A"));
        assert_eq!(state.queue.drain(), [window("B"), None, window("A")]);
    }
}
