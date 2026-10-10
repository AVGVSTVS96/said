mod decoder;
mod desktop;
mod ffi;
mod input;
mod objc;
pub mod overlay;
pub mod permissions;

use std::marker::PhantomData;

use crossbeam_channel::Receiver;
use voice_core::{Hotkey, Result, Target};

pub use decoder::{COMMAND, CONTROL, OPTION, SHIFT, Shortcut};
pub use input::InputStatus;

use crate::Platform;
use desktop::Desktop;
use input::Input;

pub struct MacosPlatform<W> {
    input: Input,
    desktop: Desktop,
    overlay: fn(&W) -> Result<()>,
    overlay_errors: crossbeam_channel::Sender<String>,
    errors: Receiver<String>,
    window: PhantomData<fn(&W)>,
}
impl<W> MacosPlatform<W> {
    pub fn start(shortcut: Shortcut, configure_overlay: fn(&W) -> Result<()>) -> Result<Self> {
        let (overlay_errors, errors) = crossbeam_channel::unbounded();
        Ok(Self {
            input: Input::start(shortcut)?,
            desktop: Desktop::default(),
            overlay: configure_overlay,
            overlay_errors,
            errors,
            window: PhantomData,
        })
    }
    pub fn input_statuses(&self) -> Receiver<InputStatus> {
        self.input.statuses()
    }
    pub fn overlay_errors(&self) -> Receiver<String> {
        self.errors.clone()
    }
    pub fn set_recording(&self, recording: bool) {
        self.input.set_recording(recording);
    }
    pub fn focused_target(&self) -> Result<Target> {
        self.desktop.focused()
    }
}
impl<W> Platform for MacosPlatform<W> {
    type Window = W;
    fn hotkeys(&self) -> Receiver<Hotkey> {
        self.input.hotkeys()
    }
    fn focused(&self) -> Option<Target> {
        self.focused_target().ok()
    }
    fn focus(&self, target: &Target) -> Result<()> {
        self.desktop.focus(target)
    }
    fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        self.desktop.paste(target, text, send)
    }
    fn make_overlay(&self, window: &W) {
        if let Err(error) = (self.overlay)(window) {
            let _ = self.overlay_errors.send(error.to_string());
        }
    }
}
