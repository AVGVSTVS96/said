use crate::runtime::HostPlatform;
use crossbeam_channel::{Receiver, never};
use said_core::{Hotkey, Result, SessionEvent, Settings, Target};
use said_platform::Platform;
use std::collections::BTreeMap;

#[cfg(target_os = "linux")]
use said_platform::linux::{self, LinuxPlatform};
#[cfg(target_os = "macos")]
use said_platform::macos::{self, MacosPlatform};

#[derive(Default)]
pub struct Desktop {
    #[cfg(target_os = "linux")]
    native: Option<LinuxPlatform>,
    #[cfg(target_os = "linux")]
    login: Option<crate::login::Login>,
    #[cfg(target_os = "macos")]
    native: Option<MacosPlatform<tauri::Window>>,
    error: Option<String>,
    hotkey: String,
    settings: Option<Settings>,
    #[cfg(target_os = "linux")]
    input_status: Vec<String>,
}
impl Desktop {
    #[cfg(target_os = "linux")]
    pub fn control_requests(&self) -> Receiver<linux::control::ControlRequest> {
        self.native
            .as_ref()
            .map(LinuxPlatform::control_requests)
            .unwrap_or_else(never)
    }
    #[cfg(target_os = "linux")]
    pub fn control_hotkey(
        &self,
        command: linux::control::ControlCommand,
    ) -> Option<(Hotkey, bool)> {
        self.native.as_ref()?.control_hotkey(command)
    }
}
impl Platform for Desktop {
    type Window = tauri::Window;
    fn hotkeys(&self) -> Receiver<Hotkey> {
        self.native
            .as_ref()
            .map(Platform::hotkeys)
            .unwrap_or_else(never)
    }
    fn focused(&self) -> Option<Target> {
        self.native.as_ref()?.focused()
    }
    fn focus(&self, target: &Target) -> Result<()> {
        self.native
            .as_ref()
            .ok_or("platform unavailable")?
            .focus(target)
    }
    fn paste(&self, target: &Target, text: &str, send: bool, automatic: bool) -> Result<()> {
        self.native
            .as_ref()
            .ok_or("platform unavailable")?
            .paste(target, text, send, automatic)
    }
    fn make_overlay(&self, window: &tauri::Window) {
        if let Some(native) = &self.native {
            native.make_overlay(window);
        }
    }
}
impl HostPlatform for Desktop {
    fn is_self(&self, target: &Target) -> bool {
        target.app_id == env!("PRODUCT_BUNDLE_ID")
            || cfg!(target_os = "linux") && target.app_id == env!("PRODUCT_STORAGE_KEY")
            || target.app_id == format!("pid:{}", std::process::id())
    }
    fn copy(&self, text: &str) -> Result<()> {
        self.native
            .as_ref()
            .ok_or("platform unavailable")?
            .copy(text)
    }
    fn configure(&mut self, settings: &Settings) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let shortcuts = linux::input::Shortcuts::parse(
                &settings.hotkey,
                &settings.interactive_hotkey,
                &settings.insert_last_hotkey,
            )?;
            let paste_chords = settings
                .apps
                .iter()
                .map(|(app, rule)| {
                    let chord = linux::paste::PasteChord::parse(&rule.paste)?;
                    Ok((app.clone(), chord))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            if let Some(native) = &mut self.native {
                native.configure(shortcuts, paste_chords);
            } else {
                let mut config = linux::Config::from_environment(env!("PRODUCT_STORAGE_KEY"))?;
                if self.login.is_none() {
                    match crate::login::Login::start() {
                        Ok(login) => self.login = Some(login),
                        Err(error) => {
                            eprintln!("said session status: {error}");
                            self.error = Some(
                                "Session status unavailable. Hold-to-talk is disabled.".into(),
                            );
                        }
                    }
                }
                config.read_input = self.login.as_ref().is_some_and(crate::login::Login::active);
                config.shortcuts = shortcuts;
                config.paste_chords = paste_chords;
                let native = LinuxPlatform::new(config)?;
                self.native = Some(native);
            }
        }
        #[cfg(target_os = "macos")]
        {
            let shortcuts = macos::Shortcuts {
                dictation: shortcut(&settings.hotkey)?,
                interactive: shortcut(&settings.interactive_hotkey)?,
                insert_last: shortcut(&settings.insert_last_hotkey)?,
            };
            shortcuts.validate()?;
            if settings
                .apps
                .values()
                .any(|rule| !rule.paste.eq_ignore_ascii_case("cmd+v"))
            {
                return Err("macOS paste shortcut is cmd+v".into());
            }
            if let Some(native) = &mut self.native {
                if self.settings.as_ref().is_none_or(|previous| {
                    previous.hotkey != settings.hotkey
                        || previous.interactive_hotkey != settings.interactive_hotkey
                        || previous.insert_last_hotkey != settings.insert_last_hotkey
                }) {
                    native.set_shortcuts(shortcuts)?;
                }
            } else {
                match MacosPlatform::start(shortcuts, configure_overlay) {
                    Ok(native) => {
                        self.native = Some(native);
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        }
        self.hotkey = settings.hotkey.clone();
        self.settings = Some(settings.clone());
        Ok(())
    }
    fn release_observable(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.native.as_ref().is_some_and(|native| {
                matches!(
                    native.mode(),
                    linux::Mode::NiriHold | linux::Mode::NiriPortal
                )
            })
        }
        #[cfg(target_os = "macos")]
        {
            true
        }
    }
    fn availability(&mut self, available: bool) {
        #[cfg(target_os = "linux")]
        if let Some(native) = &mut self.native {
            native.set_input_enabled(available);
        }
        #[cfg(target_os = "macos")]
        let _ = available;
    }
    fn recording(&mut self, active: bool) {
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            native.set_recording(active);
        }
        #[cfg(target_os = "linux")]
        let _ = active;
    }
    fn poll(&mut self) -> Vec<SessionEvent> {
        let mut events = vec![];
        #[cfg(target_os = "linux")]
        if let Some(login) = &mut self.login {
            events.extend(login.poll());
        }
        #[cfg(target_os = "linux")]
        if let Some(native) = &self.native {
            events.extend(
                native
                    .focus_changes()
                    .into_iter()
                    .map(|target| SessionEvent::FocusChanged { target }),
            );
        }
        #[cfg(target_os = "linux")]
        {
            let instructions = self.instructions();
            if instructions != self.input_status {
                self.input_status = instructions;
                events.push(
                    if self.login.as_ref().is_some_and(crate::login::Login::active) {
                        SessionEvent::SessionAvailable
                    } else if self.login.is_none() {
                        SessionEvent::FocusChanged {
                            target: self.focused(),
                        }
                    } else {
                        SessionEvent::SessionUnavailable {
                            reason: "Session locked, inactive or logind disconnected".into(),
                        }
                    },
                );
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            events.extend(
                native
                    .focus_changes()
                    .into_iter()
                    .map(|target| SessionEvent::FocusChanged { target }),
            );
            for status in native.input_statuses().try_iter() {
                events.push(match status {
                    macos::InputStatus::Available => SessionEvent::SessionAvailable,
                    status => SessionEvent::SessionUnavailable {
                        reason: match status {
                            macos::InputStatus::SecureInput => "Secure Input is enabled",
                            macos::InputStatus::SessionLocked => "Session is locked",
                            macos::InputStatus::PermissionRequired => {
                                "Enable Accessibility and Input Monitoring in System Settings"
                            }
                            macos::InputStatus::TapInterrupted => "Hotkey listener was interrupted",
                            macos::InputStatus::Available => unreachable!(),
                        }
                        .into(),
                    },
                });
            }
            for error in native.overlay_errors().try_iter() {
                self.error = Some(error);
            }
        }
        events
    }
    fn permissions(&self) -> BTreeMap<String, String> {
        #[cfg(target_os = "linux")]
        {
            BTreeMap::new()
        }
        #[cfg(target_os = "macos")]
        {
            let permission = macos::permissions::Permissions::check();
            BTreeMap::from([
                (
                    "microphone".into(),
                    format!("{:?}", permission.microphone).to_ascii_lowercase(),
                ),
                (
                    "accessibility".into(),
                    if permission.accessibility && permission.event_posting {
                        "authorized"
                    } else {
                        "denied"
                    }
                    .into(),
                ),
                (
                    "input_monitoring".into(),
                    if permission.input_monitoring {
                        "authorized"
                    } else {
                        "denied"
                    }
                    .into(),
                ),
            ])
        }
    }
    fn instructions(&self) -> Vec<String> {
        let mut steps = vec![];
        #[cfg(target_os = "linux")]
        {
            if let Some(settings) = &self.settings
                && let Ok(shortcuts) = linux::input::Shortcuts::parse(
                    &settings.hotkey,
                    &settings.interactive_hotkey,
                    &settings.insert_last_hotkey,
                )
            {
                steps.push(shortcuts.niri_binds());
            }
            if let Some(native) = &self.native {
                if native.mode() == linux::Mode::NiriToggle {
                    steps.push("For hold-to-talk: sudo usermod -aG input \"$USER\"; then sign out and back in. These are the same keys as the Niri toggle binds.".into());
                }
                steps.insert(0, match native.mode() {
                    linux::Mode::NiriHold => "Hold-to-talk available. Hold a shortcut to talk; tap to toggle.",
                    linux::Mode::NiriPortal => "Hold-to-talk available through GlobalShortcuts. Hold a shortcut to talk; tap to toggle.",
                    linux::Mode::NiriToggle => "Toggle mode. Press a shortcut to start; press it again to stop. Hold-to-talk needs input access or a working GlobalShortcuts portal.",
                    linux::Mode::Clipboard => "Clipboard mode. Dictation is copied for manual paste; automatic insertion needs Niri.",
                }.into());
                if let Some(reason) = native.input_unavailable_reason() {
                    steps.push(reason.into());
                }
                if let Some(reason) = native.portal_unavailable_reason() {
                    steps.push(reason);
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            steps.push(if self.native.is_some() {
                "Hold-to-talk available. Hold a shortcut to talk; tap to toggle."
            } else {
                "Shortcuts unavailable. Allow Accessibility and Input Monitoring, then reopen the app."
            }.into());
            steps.extend(
                macos::permissions::Permissions::check()
                    .recovery()
                    .into_iter()
                    .map(str::to_owned),
            );
        }
        if let Some(error) = &self.error {
            steps.push(error.clone());
        }
        steps
    }
}

#[cfg(target_os = "macos")]
fn shortcut(value: &str) -> Result<macos::Shortcut> {
    let mut parts = value.split('+');
    let key = parts.next_back().ok_or("shortcut needs a key")?;
    let keycode = match key.to_ascii_lowercase().as_str() {
        "space" => 49,
        "v" => 9,
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "o" => 31,
        "u" => 32,
        "i" => 34,
        "p" => 35,
        "l" => 37,
        "j" => 38,
        "k" => 40,
        "n" => 45,
        "m" => 46,
        _ => {
            return Err("choose Space or a letter".into());
        }
    };
    let mut modifiers = 0;
    for part in parts {
        modifiers |= match part.to_ascii_lowercase().as_str() {
            "ctrl" => macos::CONTROL,
            "alt" => macos::OPTION,
            "cmd" => macos::COMMAND,
            "shift" => macos::SHIFT,
            _ => return Err("supported modifiers are ctrl, alt, cmd and shift".into()),
        };
    }
    if modifiers == 0 {
        return Err("shortcut needs a modifier".into());
    }
    Ok(macos::Shortcut { keycode, modifiers })
}
#[cfg(target_os = "macos")]
fn configure_overlay(window: &tauri::Window) -> Result<()> {
    let pointer = std::ptr::NonNull::new(window.ns_window()?).ok_or("missing native window")?;
    unsafe {
        macos::overlay::configure(pointer)?;
    }
    crate::permissions::position_overlay(pointer);
    Ok(())
}
pub fn show_overlay(window: &tauri::Window) -> Result<()> {
    #[cfg(target_os = "linux")]
    window.show()?;
    #[cfg(target_os = "macos")]
    {
        let window = window.clone();
        window.clone().run_on_main_thread(move || {
            if let Ok(pointer) = window.ns_window()
                && let Some(pointer) = std::ptr::NonNull::new(pointer)
            {
                crate::permissions::position_overlay(pointer);
                let _ = unsafe { macos::overlay::show(pointer) };
            }
        })?;
    }
    Ok(())
}
pub fn request_permission(permission: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        if permission != "microphone" {
            return Err("no such permission on Linux".into());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        if permission == "microphone" {
            crate::permissions::microphone();
            return Ok(());
        }
        let pane = match permission {
            "accessibility" => "Privacy_Accessibility",
            "input_monitoring" => "Privacy_ListenEvent",
            _ => return Err("unknown permission".into()),
        };
        std::process::Command::new("open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{pane}"
            ))
            .spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_app_id_is_never_an_external_paste_target() {
        let app_id = if cfg!(target_os = "linux") {
            env!("PRODUCT_STORAGE_KEY")
        } else {
            env!("PRODUCT_BUNDLE_ID")
        };
        assert!(Desktop::default().is_self(&Target {
            app_id: app_id.into(),
            window_id: "native-setup".into(),
            name: "Setup".into(),
        }));
    }
}
