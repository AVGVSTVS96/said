use std::ffi::{CStr, c_void};
use std::mem::transmute;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use super::ffi::{self, Ref};

pub struct Retained(pub Ref);
impl Retained {
    pub unsafe fn new(object: Ref) -> Self {
        Self(unsafe { send(object, c"retain") })
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        unsafe { send::<()>(self.0, c"release") };
    }
}

pub unsafe fn class(name: &CStr) -> Ref {
    unsafe { ffi::objc_getClass(name.as_ptr()) }
}
pub unsafe fn send<R>(object: Ref, selector: &CStr) -> R {
    let function: unsafe extern "C" fn(Ref, Ref) -> R =
        unsafe { transmute(ffi::objc_msgSend as *const ()) };
    unsafe { function(object, ffi::sel_registerName(selector.as_ptr())) }
}
pub unsafe fn send1<A, R>(object: Ref, selector: &CStr, a: A) -> R {
    let function: unsafe extern "C" fn(Ref, Ref, A) -> R =
        unsafe { transmute(ffi::objc_msgSend as *const ()) };
    unsafe { function(object, ffi::sel_registerName(selector.as_ptr()), a) }
}
pub unsafe fn send2<A, B, R>(object: Ref, selector: &CStr, a: A, b: B) -> R {
    let function: unsafe extern "C" fn(Ref, Ref, A, B) -> R =
        unsafe { transmute(ffi::objc_msgSend as *const ()) };
    unsafe { function(object, ffi::sel_registerName(selector.as_ptr()), a, b) }
}
pub unsafe fn send4<A, B, C, D, R>(object: Ref, selector: &CStr, a: A, b: B, c: C, d: D) -> R {
    let function: unsafe extern "C" fn(Ref, Ref, A, B, C, D) -> R =
        unsafe { transmute(ffi::objc_msgSend as *const ()) };
    unsafe { function(object, ffi::sel_registerName(selector.as_ptr()), a, b, c, d) }
}
pub unsafe fn string(object: Ref) -> Option<String> {
    if object.is_null() {
        return None;
    }
    let bytes: *const std::ffi::c_char = unsafe { send(object, c"UTF8String") };
    if bytes.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(bytes) }.to_str().ok()?.to_owned())
    }
}

pub fn main_thread<F: FnOnce() -> R + Send, R: Send>(work: F) -> R {
    if unsafe { ffi::pthread_main_np() } != 0 {
        return work();
    }
    struct Job<F, R> {
        work: Option<F>,
        result: Option<std::thread::Result<R>>,
    }
    unsafe extern "C" fn run<F: FnOnce() -> R, R>(context: Ref) {
        let job = unsafe { &mut *context.cast::<Job<F, R>>() };
        job.result = Some(catch_unwind(AssertUnwindSafe(job.work.take().unwrap())));
    }
    let mut job = Job {
        work: Some(work),
        result: None,
    };
    unsafe {
        ffi::dispatch_sync_f(
            // dispatch_get_main_queue is an inline C accessor for this queue object.
            (&raw const ffi::_dispatch_main_q).cast_mut(),
            (&mut job as *mut Job<_, R>).cast::<c_void>(),
            run::<F, R>,
        );
    }
    match job.result.unwrap() {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    }
}

pub fn autorelease<R>(work: impl FnOnce() -> R) -> R {
    let pool = unsafe { send::<Ref>(class(c"NSAutoreleasePool"), c"new") };
    struct Pool(Ref);
    impl Drop for Pool {
        fn drop(&mut self) {
            unsafe { send::<()>(self.0, c"drain") };
        }
    }
    let _pool = Pool(pool);
    work()
}
