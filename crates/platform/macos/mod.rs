mod decoder;
mod desktop;
mod ffi;
mod focus;
mod input;
mod objc;
pub mod overlay;
pub mod permissions;

use crossbeam_channel::{Receiver, never};
use said_core::{Hotkey, Result, Target};

pub use decoder::{COMMAND, CONTROL, OPTION, SHIFT, Shortcut, Shortcuts};
pub use input::InputStatus;

use crate::Platform;
use desktop::Desktop;
use input::Input;
use permissions::Permissions;

pub struct MacosPlatform<W> {
    shortcuts: Shortcuts,
    input: Option<Input>,
    input_error: Option<String>,
    listen_access_required: bool,
    desktop: Desktop,
    overlay: fn(&W) -> Result<()>,
    overlay_errors: crossbeam_channel::Sender<String>,
    errors: Receiver<String>,
}
impl<W> MacosPlatform<W> {
    /// Clipboard and the overlay work immediately; focus, paste and shortcuts start once
    /// Accessibility is on. Call `refresh` after grants change.
    pub fn start(shortcuts: Shortcuts, configure_overlay: fn(&W) -> Result<()>) -> Result<Self> {
        shortcuts.validate()?;
        let (overlay_errors, errors) = crossbeam_channel::unbounded();
        let mut platform = Self {
            shortcuts,
            input: None,
            input_error: None,
            listen_access_required: false,
            desktop: Desktop::default(),
            overlay: configure_overlay,
            overlay_errors,
            errors,
        };
        platform.refresh(Permissions::check());
        Ok(platform)
    }
    pub fn set_shortcuts(&mut self, shortcuts: Shortcuts) -> Result<()> {
        shortcuts.validate()?;
        self.shortcuts = shortcuts;
        self.input = None;
        self.refresh(Permissions::check());
        Ok(())
    }
    /// Idempotently starts whatever the current grants allow and is not running yet.
    pub fn refresh(&mut self, permissions: Permissions) {
        if !permissions.accessibility {
            return;
        }
        if !self.desktop.observing() {
            let _ = self.desktop.observe();
        }
        if self.input.is_some() {
            return;
        }
        match Input::start(self.shortcuts) {
            Ok(input) => {
                self.input = Some(input);
                self.input_error = None;
                self.listen_access_required = false;
            }
            Err(error) => {
                self.input_error = Some(error.to_string());
                if !permissions.input_monitoring {
                    self.listen_access_required = true;
                    permissions::prompt_input_monitoring();
                }
            }
        }
    }
    pub fn shortcuts_running(&self) -> bool {
        self.input.is_some()
    }
    pub fn shortcuts_error(&self) -> Option<&str> {
        self.input_error.as_deref()
    }
    /// True only when Accessibility is on but macOS still refused the event tap.
    pub fn input_monitoring_required(&self) -> bool {
        self.input.is_none() && self.listen_access_required
    }
    pub fn focus_changes(&self) -> Vec<Option<Target>> {
        self.desktop.focus_changes()
    }
    pub fn input_statuses(&self) -> Receiver<InputStatus> {
        self.input
            .as_ref()
            .map(Input::statuses)
            .unwrap_or_else(never)
    }
    pub fn overlay_errors(&self) -> Receiver<String> {
        self.errors.clone()
    }
    pub fn set_recording(&self, recording: bool) {
        if let Some(input) = &self.input {
            input.set_recording(recording);
        }
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
        self.input
            .as_ref()
            .map(Input::hotkeys)
            .unwrap_or_else(never)
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
