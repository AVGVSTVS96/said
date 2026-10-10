use block2::RcBlock;
use objc2::{
    msg_send,
    runtime::{AnyClass, Bool},
};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}
pub fn microphone() {
    let completion = RcBlock::new(|_: Bool| {});
    let media = NSString::from_str("soun");
    unsafe {
        let class = AnyClass::get(c"AVCaptureDevice").expect("AVFoundation is linked");
        let _: () =
            msg_send![class, requestAccessForMediaType: &*media, completionHandler: &*completion];
    }
}

pub fn position_overlay(pointer: std::ptr::NonNull<std::ffi::c_void>) {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSRect};
    unsafe {
        let class = AnyClass::get(c"NSScreen").expect("AppKit is linked");
        let screen: *mut AnyObject = msg_send![class, mainScreen];
        if screen.is_null() {
            return;
        }
        let visible: NSRect = msg_send![screen, visibleFrame];
        let panel = pointer.as_ptr().cast::<AnyObject>();
        let frame: NSRect = msg_send![panel, frame];
        let origin = NSPoint::new(
            visible.origin.x + (visible.size.width - frame.size.width) / 2.0,
            visible.origin.y + 20.0,
        );
        let _: () = msg_send![panel, setFrameOrigin: origin];
    }
}
