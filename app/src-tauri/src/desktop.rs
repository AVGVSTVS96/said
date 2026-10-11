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
    changes: Option<linux::niri::FocusChanges>,
    #[cfg(target_os = "linux")]
    login: Option<crate::login::Login>,
    #[cfg(target_os = "macos")]
    native: Option<MacosPlatform<tauri::Window>>,
    error: Option<String>,
    hotkey: String,
    settings: Option<Settings>,
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
    fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        self.native
            .as_ref()
            .ok_or("platform unavailable")?
            .paste(target, text, send)
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
            let mut config = linux::Config::from_environment(env!("PRODUCT_STORAGE_KEY"))?;
            if self.login.is_none() {
                match crate::login::Login::start() {
                    Ok(login) => self.login = Some(login),
                    Err(error) => {
                        self.error = Some(format!(
                            "logind unavailable: {error}; physical input disabled"
                        ))
                    }
                }
            }
            config.read_input = self.login.as_ref().is_some_and(crate::login::Login::active);
            if !config.read_input {
                config.portal_trigger = None;
            }
            config.chord = linux::input::Chord::parse(&settings.hotkey)?;
            let reserved = linux::input::Chord::parse("ctrl+alt+v")?;
            if settings.hotkey.to_ascii_lowercase().contains("shift")
                || config.chord.key == reserved.key
                    && reserved
                        .modifiers
                        .iter()
                        .all(|modifier| config.chord.modifiers.contains(modifier))
            {
                return Err(
                    "Shift is reserved for interactive mode and Ctrl+Alt+V for insert last".into(),
                );
            }
            if config.read_input {
                let mut parts: Vec<_> = settings.hotkey.split('+').map(str::to_owned).collect();
                let last = parts.len() - 1;
                for part in &mut parts[..last] {
                    *part = part.to_ascii_uppercase();
                }
                parts[last] = parts[last].to_ascii_lowercase();
                config.portal_trigger = Some(parts.join("+"));
            }
            for (app, rule) in &settings.apps {
                let chord = match rule.paste.to_ascii_lowercase().as_str() {
                    "ctrl+v" => linux::paste::PasteChord::CtrlV,
                    "ctrl+shift+v" => linux::paste::PasteChord::CtrlShiftV,
                    "shift+insert" => linux::paste::PasteChord::ShiftInsert,
                    _ => {
                        return Err(
                            "supported paste shortcuts: ctrl+v, ctrl+shift+v, shift+insert".into(),
                        );
                    }
                };
                config.paste_chords.insert(app.clone(), chord);
            }
            self.changes = None;
            self.native = None;
            let native = LinuxPlatform::new(config)?;
            self.changes = native
                .niri()
                .map(linux::niri::Niri::focus_changes)
                .transpose()?;
            self.native = Some(native);
        }
        #[cfg(target_os = "macos")]
        {
            let shortcut = shortcut(&settings.hotkey)?;
            if settings
                .apps
                .values()
                .any(|rule| !rule.paste.eq_ignore_ascii_case("cmd+v"))
            {
                return Err("macOS paste shortcut is cmd+v".into());
            }
            if let Some(native) = &mut self.native {
                if settings.hotkey != self.hotkey {
                    native.set_shortcut(shortcut)?;
                }
            } else {
                match MacosPlatform::start(shortcut, configure_overlay) {
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
        if available
            && self
                .native
                .as_ref()
                .is_some_and(|native| native.mode() == linux::Mode::NiriToggle)
        {
            if let Some(settings) = self.settings.clone()
                && let Err(error) = self.configure(&settings)
            {
                self.error = Some(error.to_string());
            }
        } else if let Some(native) = &mut self.native {
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
        if let Some(changes) = &self.changes {
            events.extend(
                changes
                    .events()
                    .try_iter()
                    .map(|target| SessionEvent::FocusChanged { target }),
            );
        }
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
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
            BTreeMap::from([("microphone".into(), "authorized".into())])
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
            steps.push(format!(
                "Niri bind: {} {{ spawn \"said\" \"hotkey\"; }}",
                self.hotkey
                    .replace("ctrl", "Ctrl")
                    .replace("alt", "Alt")
                    .replace("space", "Space")
            ));
            steps.push("Without input access, each press toggles. For hold-to-talk: sudo usermod -aG input \"$USER\"; then sign out and back in.".into());
            if let Some(native) = &self.native {
                steps.push(format!("Input mode: {:?}", native.mode()));
                if let Some(reason) = native.input_unavailable_reason() {
                    steps.push(reason.into());
                }
                if let Some(reason) = native.portal_unavailable_reason() {
                    steps.push(reason.into());
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
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
            return Err(
                "choose Space or a letter other than V; Shift is reserved for interactive mode"
                    .into(),
            );
        }
    };
    let mut modifiers = 0;
    for part in parts {
        modifiers |= match part.to_ascii_lowercase().as_str() {
            "ctrl" => macos::CONTROL,
            "alt" => macos::OPTION,
            "cmd" => macos::COMMAND,
            _ => return Err("supported modifiers are ctrl, alt and cmd".into()),
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
