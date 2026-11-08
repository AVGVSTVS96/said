pub mod fake;
pub mod focus;

#[cfg(all(target_os = "linux", feature = "native"))]
#[allow(unsafe_code, reason = "libc FFI: flock, inotify, memfd")]
#[path = "../linux/mod.rs"]
pub mod linux;

#[cfg(all(target_os = "macos", feature = "native"))]
#[allow(unsafe_code, reason = "AppKit, Accessibility and CoreGraphics FFI")]
#[path = "../macos/mod.rs"]
pub mod macos;

use crossbeam_channel::Receiver;
use said_core::{Hotkey, Result, Target};

pub trait Platform {
    type Window;

    fn hotkeys(&self) -> Receiver<Hotkey>;
    fn focused(&self) -> Option<Target>;
    fn focus(&self, target: &Target) -> Result<()>;
    fn paste(&self, target: &Target, text: &str, send: bool, automatic: bool) -> Result<()>;
    fn make_overlay(&self, window: &Self::Window);
}
