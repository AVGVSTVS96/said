use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

use crossbeam_channel::{Receiver, Sender, unbounded};
use said_core::{Result, Target};

use super::ffi::{self, Owned, Ref};
use super::objc;

struct Watch {
    pid: i32,
    observer: Owned,
    _application: Owned,
}
// Native observer creation, callbacks and teardown all run on the main queue.
unsafe impl Send for Watch {}
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

struct State {
    watch: Mutex<Option<Watch>>,
    events: Sender<Option<Target>>,
}
impl State {
    fn changed(&self) {
        // A queued invalidation preserves a departure even when current focus has already returned.
        let _ = self.events.send(None);
    }
    fn observe(&self) -> Result<i32> {
        unsafe {
            let workspace = objc::send::<Ref>(objc::class(c"NSWorkspace"), c"sharedWorkspace");
            let app = objc::send::<Ref>(workspace, c"frontmostApplication");
            if app.is_null() {
                return Err("No focused application".into());
            }
            let pid: i32 = objc::send(app, c"processIdentifier");
            let mut watch = self.watch.lock().unwrap();
            if watch.as_ref().is_some_and(|watch| watch.pid == pid) {
                return Ok(pid);
            }
            *watch = None;
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
            *watch = Some(Watch {
                pid,
                observer,
                _application: application,
            });
            Ok(pid)
        }
    }
}
unsafe extern "C" fn window_changed(_: Ref, _: Ref, _: Ref, context: Ref) {
    unsafe { &*context.cast::<State>() }.changed();
}
unsafe extern "C" fn application_changed(object: Ref, _: Ref, _: Ref) {
    let mut context = null_mut();
    unsafe { ffi::object_getInstanceVariable(object, c"context".as_ptr(), &mut context) };
    if let Some(state) = unsafe { context.cast::<State>().as_ref() } {
        state.changed();
        let _ = state.observe();
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
            application_changed as *const (),
            c"v@:@".as_ptr()
        ));
        ffi::objc_registerClassPair(class);
        class as usize
    }) as Ref
}

pub struct FocusChanges {
    state: Box<State>,
    object: Mutex<Option<objc::Retained>>,
    events: Receiver<Option<Target>>,
}
impl Default for FocusChanges {
    fn default() -> Self {
        Self::new()
    }
}
// The retained notification object is only accessed through objc::main_thread.
unsafe impl Send for FocusChanges {}
unsafe impl Sync for FocusChanges {}
impl FocusChanges {
    pub fn new() -> Self {
        let (sender, events) = unbounded();
        Self {
            state: Box::new(State {
                watch: Mutex::new(None),
                events: sender,
            }),
            object: Mutex::new(None),
            events,
        }
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
        self.state.observe()?;
        Ok(())
    }
    pub fn events(&self) -> Receiver<Option<Target>> {
        self.events.clone()
    }
}
impl Drop for FocusChanges {
    fn drop(&mut self) {
        if self.object.get_mut().unwrap().is_none() {
            return;
        }
        objc::main_thread(|| unsafe {
            if let Some(object) = self.object.lock().unwrap().take() {
                let workspace = objc::send::<Ref>(objc::class(c"NSWorkspace"), c"sharedWorkspace");
                let center = objc::send::<Ref>(workspace, c"notificationCenter");
                objc::send1::<_, ()>(center, c"removeObserver:", object.0);
                ffi::object_setInstanceVariable(object.0, c"context".as_ptr(), null_mut());
                self.state.watch.lock().unwrap().take();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn departure_and_return_remain_queued_while_the_host_is_busy() {
        let changes = FocusChanges::new();
        let context = (&*changes.state as *const State).cast_mut().cast();
        unsafe {
            window_changed(null_mut(), null_mut(), null_mut(), context);
            window_changed(null_mut(), null_mut(), null_mut(), context);
        }
        assert_eq!(
            changes.events().try_iter().collect::<Vec<_>>(),
            [None, None]
        );
    }
}
