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
