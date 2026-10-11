use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;

use said_core::Result;

#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

pub struct Library(*mut c_void);

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes())?;
        let handle = unsafe { dlopen(path.as_ptr(), 2) };
        if handle.is_null() {
            return Err(error().into());
        }
        Ok(Self(handle))
    }

    pub fn symbol(&self, name: &CStr) -> Result<*mut c_void> {
        let pointer = unsafe { dlsym(self.0, name.as_ptr()) };
        if pointer.is_null() {
            return Err(error().into());
        }
        Ok(pointer)
    }
}

fn error() -> String {
    let message = unsafe { dlerror() };
    if message.is_null() {
        "native library loading failed".into()
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { dlclose(self.0) };
    }
}
