use std::ffi::{c_char, c_int, c_void};

pub type Ref = *mut c_void;
pub type TapCallback = unsafe extern "C" fn(Ref, u32, Ref, Ref) -> Ref;

#[cfg(test)]
#[repr(C)]
pub struct Range {
    pub location: isize,
    pub length: isize,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRelease(value: Ref);
    pub fn CFEqual(a: Ref, b: Ref) -> bool;
    pub fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    pub fn CFBooleanGetValue(value: Ref) -> bool;
    pub fn CFGetTypeID(value: Ref) -> usize;
    pub fn CFBooleanGetTypeID() -> usize;
    #[cfg(test)]
    pub fn CFStringGetLength(value: Ref) -> isize;
    #[cfg(test)]
    pub fn CFStringGetBytes(
        value: Ref,
        range: Range,
        encoding: u32,
        loss: u8,
        external: bool,
        buffer: *mut u8,
        capacity: isize,
        used: *mut isize,
    ) -> isize;
    pub fn CFStringCreateWithBytes(
        allocator: Ref,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        external: bool,
    ) -> Ref;
    pub fn CFDataGetBytePtr(data: Ref) -> *const u8;
    pub fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    pub fn CFMachPortInvalidate(port: Ref);
    pub fn CFRunLoopGetCurrent() -> Ref;
    pub fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    pub fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
    pub static kCFRunLoopDefaultMode: Ref;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
    pub fn CGSessionCopyCurrentDictionary() -> Ref;
    pub fn AXUIElementCreateApplication(pid: c_int) -> Ref;
    pub fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
    pub fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
    pub fn AXUIElementPerformAction(element: Ref, action: Ref) -> i32;
    pub fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGPreflightPostEventAccess() -> bool;
    pub fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: TapCallback,
        data: Ref,
    ) -> Ref;
    pub fn CGEventTapEnable(tap: Ref, enable: bool);
    pub fn CGEventTapIsEnabled(tap: Ref) -> bool;
    pub fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    pub fn CGEventGetFlags(event: Ref) -> u64;
    pub fn CGEventSetFlags(event: Ref, flags: u64);
    pub fn CGEventSetIntegerValueField(event: Ref, field: u32, value: i64);
    pub fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
    pub fn CGEventPostToPid(pid: c_int, event: Ref);
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub fn IsSecureEventInputEnabled() -> u8;
    pub fn TISCopyCurrentKeyboardLayoutInputSource() -> Ref;
    pub fn TISGetInputSourceProperty(source: Ref, property: Ref) -> Ref;
    pub static kTISPropertyUnicodeKeyLayoutData: Ref;
    pub fn LMGetKbdType() -> u8;
    pub fn UCKeyTranslate(
        layout: *const u8,
        key: u16,
        action: u16,
        modifiers: u32,
        keyboard: u32,
        options: u32,
        dead: *mut u32,
        max: usize,
        length: *mut usize,
        output: *mut u16,
    ) -> i32;
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    pub static AVMediaTypeAudio: Ref;
}

#[link(name = "objc")]
unsafe extern "C" {
    pub fn objc_getClass(name: *const c_char) -> Ref;
    pub fn sel_registerName(name: *const c_char) -> Ref;
    pub fn objc_msgSend();
    pub fn objc_allocateClassPair(superclass: Ref, name: *const c_char, extra: usize) -> Ref;
    pub fn objc_registerClassPair(class: Ref);
    pub fn class_addMethod(
        class: Ref,
        selector: Ref,
        implementation: *const c_void,
        types: *const c_char,
    ) -> bool;
    pub fn class_getInstanceSize(class: Ref) -> usize;
    pub fn object_setClass(object: Ref, class: Ref) -> Ref;
}

unsafe extern "C" {
    pub fn pthread_main_np() -> c_int;
    pub static _dispatch_main_q: c_void;
    pub fn dispatch_sync_f(queue: Ref, context: Ref, work: unsafe extern "C" fn(Ref));
}

pub struct Owned(pub Ref);
impl Owned {
    pub fn new(value: Ref) -> voice_core::Result<Self> {
        if value.is_null() {
            Err("macOS returned a null native object".into())
        } else {
            Ok(Self(value))
        }
    }
    pub fn string(value: &str) -> Self {
        Self(unsafe {
            CFStringCreateWithBytes(
                std::ptr::null_mut(),
                value.as_ptr(),
                value.len() as isize,
                0x08000100,
                false,
            )
        })
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_string_conversion_preserves_every_utf8_byte() {
        let text = "  no no, um actually no\t--no-verify\n\n猫 e\u{301}\0  ";
        let value = Owned::string(text);
        let mut bytes = vec![0; text.len()];
        let mut used = 0;
        let length = unsafe { CFStringGetLength(value.0) };
        assert_eq!(
            unsafe {
                CFStringGetBytes(
                    value.0,
                    Range {
                        location: 0,
                        length,
                    },
                    0x08000100,
                    0,
                    false,
                    bytes.as_mut_ptr(),
                    bytes.len() as isize,
                    &mut used,
                )
            },
            length
        );
        assert_eq!(used as usize, text.len());
        assert_eq!(bytes, text.as_bytes());
    }
}
