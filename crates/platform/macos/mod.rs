mod decoder;
mod desktop;
mod ffi;
mod focus;
mod input;
mod objc;
pub mod overlay;
pub mod permissions;

use crossbeam_channel::Receiver;
use said_core::{Hotkey, Result, Target};

pub use decoder::{COMMAND, CONTROL, OPTION, SHIFT, Shortcut, Shortcuts};
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
}
impl<W> MacosPlatform<W> {
    pub fn start(shortcuts: Shortcuts, configure_overlay: fn(&W) -> Result<()>) -> Result<Self> {
        let (overlay_errors, errors) = crossbeam_channel::unbounded();
        Ok(Self {
            input: Input::start(shortcuts)?,
            desktop: Desktop::default(),
            overlay: configure_overlay,
            overlay_errors,
            errors,
        })
    }
    pub fn set_shortcuts(&mut self, shortcuts: Shortcuts) -> Result<()> {
        self.input = Input::start(shortcuts)?;
        Ok(())
    }
    pub fn focus_changes(&self) -> Vec<Option<Target>> {
        self.desktop.focus_changes()
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
    pub fn copy(&self, text: &str) -> Result<()> {
        self.desktop.copy(text)
    }
}
impl<W: Sync> Platform for MacosPlatform<W> {
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
    fn paste(&self, target: &Target, text: &str, send: bool, automatic: bool) -> Result<()> {
        self.desktop.paste(target, text, send, automatic)
    }
    fn make_overlay(&self, window: &W) {
        if let Err(error) = objc::main_thread(|| (self.overlay)(window)) {
            let _ = self.overlay_errors.send(error.to_string());
        }
    }
}
