use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

use said_core::Result;

use super::{ffi, objc};

unsafe extern "C" fn never_focus(_: ffi::Ref, _: ffi::Ref) -> bool {
    false
}

/// Convert a hidden native window before its first presentation.
///
/// # Safety
/// `window` must point to a live AppKit NSWindow owned by the caller. It must
/// remain alive through the call; invoke on the AppKit main thread.
pub unsafe fn configure(window: NonNull<c_void>) -> Result<()> {
    if unsafe { ffi::pthread_main_np() } == 0 {
        return Err("Configure the overlay on the AppKit main thread".into());
    }
    unsafe {
        let window = window.as_ptr();
        if objc::send::<bool>(window, c"isVisible") {
            return Err("Convert the overlay to NSPanel before showing it".into());
        }
        let panel = objc::class(c"NSPanel");
        let activation = ffi::sel_registerName(c"_setPreventsActivation:".as_ptr());
        if !objc::send1::<_, bool>(window, c"respondsToSelector:", activation) {
            return Err(
                "This macOS version cannot safely convert the overlay; keep it hidden".into(),
            );
        }
        if !objc::send1::<_, bool>(window, c"isKindOfClass:", panel) {
            if !objc::send1::<_, bool>(window, c"isKindOfClass:", objc::class(c"NSWindow")) {
                return Err("Overlay handle is not an NSWindow".into());
            }
            // Like tauri-nspanel, conversion changes the hidden window's runtime class.
            static PANEL: OnceLock<usize> = OnceLock::new();
            let class = *PANEL.get_or_init(|| {
                let class =
                    ffi::objc_allocateClassPair(panel, c"saidNonactivatingPanel".as_ptr(), 0);
                assert!(!class.is_null(), "cannot register overlay panel class");
                for selector in [c"canBecomeKeyWindow", c"canBecomeMainWindow"] {
                    assert!(ffi::class_addMethod(
                        class,
                        ffi::sel_registerName(selector.as_ptr()),
                        never_focus as *const c_void,
                        c"B@:".as_ptr()
                    ));
                }
                ffi::objc_registerClassPair(class);
                class as usize
            }) as ffi::Ref;
            let original = objc::send::<ffi::Ref>(window, c"class");
            if ffi::class_getInstanceSize(class) > ffi::class_getInstanceSize(original) {
                return Err("NSPanel cannot fit the native window allocation".into());
            }
            ffi::object_setClass(window, class);
        } else if objc::send::<bool>(window, c"canBecomeKeyWindow")
            || objc::send::<bool>(window, c"canBecomeMainWindow")
        {
            return Err("Existing NSPanel must override canBecomeKeyWindow and canBecomeMainWindow to false".into());
        }
        objc::send1::<_, ()>(window, c"setStyleMask:", 1_usize << 7);
        // Conversion skips NSPanel initialization; setStyleMask does not update WindowServer activation tags.
        // https://github.com/ahkohd/tauri-nspanel/blob/v2.1/src/panel.rs
        objc::send1::<_, ()>(window, c"_setPreventsActivation:", true);
        objc::send1::<_, ()>(window, c"setReleasedWhenClosed:", false);
        objc::send1::<_, ()>(window, c"setLevel:", 25_isize);
        objc::send1::<_, ()>(window, c"setCollectionBehavior:", (1_usize << 0) | (1 << 8));
        objc::send1::<_, ()>(window, c"setBecomesKeyOnlyIfNeeded:", true);
        objc::send1::<_, ()>(window, c"setHidesOnDeactivate:", false);
        objc::send1::<_, ()>(window, c"setWorksWhenModal:", true);
        Ok(())
    }
}

/// # Safety
/// `window` must be a live panel previously configured by `configure`, on the main thread.
pub unsafe fn show(window: NonNull<c_void>) -> Result<()> {
    if unsafe { ffi::pthread_main_np() } == 0 {
        return Err("Show the overlay on the AppKit main thread".into());
    }
    unsafe {
        objc::send::<()>(window.as_ptr(), c"orderFrontRegardless");
    }
    Ok(())
}
