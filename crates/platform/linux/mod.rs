pub mod control;
pub mod input;
pub mod keyboard;
pub mod niri;
pub mod overlay;
pub mod paste;
pub mod portal;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use crossbeam_channel::{Receiver, Sender, unbounded};
use said_core::{Hotkey, Result, Target};

use crate::Platform;
use control::{ControlCommand, ControlRequest, ControlSocket};
use gtk::prelude::*;
use input::{InputListener, Shortcuts};
use keyboard::VirtualKeyboard;
use niri::Niri;
use paste::PasteChord;
use portal::PortalHotkeys;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    NiriToggle,
    NiriHold,
    NiriPortal,
    Clipboard,
}

pub struct Config {
    pub runtime_dir: PathBuf,
    pub wayland_display: OsString,
    pub niri_socket: Option<PathBuf>,
    pub storage_key: String,
    pub shortcuts: Shortcuts,
    pub read_input: bool,
    pub portal_enabled: bool,
    pub paste_chords: BTreeMap<String, PasteChord>,
}

impl Config {
    pub fn from_environment(storage_key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            runtime_dir: std::env::var_os("XDG_RUNTIME_DIR")
                .ok_or("XDG_RUNTIME_DIR is required")?
                .into(),
            wayland_display: std::env::var_os("WAYLAND_DISPLAY")
                .ok_or("a Wayland session is required")?,
            niri_socket: std::env::var_os("NIRI_SOCKET").map(Into::into),
            storage_key: storage_key.into(),
            shortcuts: Shortcuts::default(),
            read_input: true,
            portal_enabled: true,
            paste_chords: paste::default_chords(),
        })
    }
}

pub struct LinuxPlatform {
    config: Config,
    niri: Option<Niri>,
    control: ControlSocket,
    keyboard: Option<Mutex<VirtualKeyboard>>,
    input: Option<InputListener>,
    portal: Option<PortalHotkeys>,
    portal_unavailable_reason: Option<String>,
    input_unavailable_reason: Option<String>,
    hotkeys: Receiver<Hotkey>,
    sender: Sender<Hotkey>,
}

impl LinuxPlatform {
    pub fn new(config: Config) -> Result<Self> {
        let niri = config.niri_socket.as_ref().map(Niri::new).transpose()?;
        let control = ControlSocket::bind(&config.runtime_dir, &config.storage_key)?;
        let keyboard = niri
            .as_ref()
            .map(|_| {
                VirtualKeyboard::connect(&config.runtime_dir, &config.wayland_display)
                    .map(Mutex::new)
            })
            .transpose()?;
        let (sender, hotkeys) = unbounded();
        let read_input = config.read_input;
        let mut platform = Self {
            config,
            niri,
            control,
            keyboard,
            input: None,
            portal: None,
            portal_unavailable_reason: None,
            input_unavailable_reason: None,
            hotkeys,
            sender,
        };
        platform.set_input_enabled(read_input);
        Ok(platform)
    }

    pub fn input_unavailable_reason(&self) -> Option<&str> {
        self.input_unavailable_reason.as_deref()
    }

    pub fn mode(&self) -> Mode {
        if self.niri.is_none() {
            Mode::Clipboard
        } else if self.input.as_ref().is_some_and(InputListener::available) {
            Mode::NiriHold
        } else if self.portal.as_ref().is_some_and(PortalHotkeys::available) {
            Mode::NiriPortal
        } else {
            Mode::NiriToggle
        }
    }

    pub fn portal_unavailable_reason(&self) -> Option<String> {
        self.portal_unavailable_reason.clone().or_else(|| {
            self.portal
                .as_ref()
                .and_then(PortalHotkeys::unavailable_reason)
        })
    }

    pub fn configure(&mut self, shortcuts: Shortcuts, paste_chords: BTreeMap<String, PasteChord>) {
        if self.config.shortcuts != shortcuts {
            self.input = None;
            self.portal = None;
            self.config.shortcuts = shortcuts;
            self.input_unavailable_reason = None;
            self.portal_unavailable_reason = None;
            self.set_input_enabled(self.config.read_input);
        }
        self.config.paste_chords = paste_chords;
    }

    pub fn control_requests(&self) -> Receiver<ControlRequest> {
        self.control.requests()
    }
    pub fn niri(&self) -> Option<&Niri> {
        self.niri.as_ref()
    }

    pub fn set_input_enabled(&mut self, enabled: bool) {
        self.config.read_input = enabled;
        if enabled && self.niri.is_some() && self.input.is_none() && self.portal.is_none() {
            match InputListener::start(
                std::path::Path::new("/dev/input"),
                self.config.shortcuts.clone(),
                self.sender.clone(),
            ) {
                Ok(input) => {
                    self.input = Some(input);
                    self.input_unavailable_reason = None;
                }
                Err(error) => self.input_unavailable_reason = Some(error.to_string()),
            }
            if self.input.is_none() && self.config.portal_enabled {
                match PortalHotkeys::start(
                    self.config.shortcuts.chords_for_portal(),
                    self.sender.clone(),
                ) {
                    Ok(portal) => self.portal = Some(portal),
                    Err(error) => self.portal_unavailable_reason = Some(error.to_string()),
                }
            }
        }
        if let Some(portal) = &self.portal {
            portal.set_enabled(enabled);
        }
        if let Some(input) = &mut self.input {
            input.set_enabled(enabled);
        }
    }

    pub fn control_hotkey(&self, command: ControlCommand) -> Option<(Hotkey, bool)> {
        let event = match command {
            ControlCommand::Hotkey
            | ControlCommand::HotkeyInteractive
            | ControlCommand::HotkeyInsertLast
                if matches!(self.mode(), Mode::NiriHold | Mode::NiriPortal) =>
            {
                return None;
            }
            ControlCommand::Hotkey | ControlCommand::Toggle => Hotkey::Down,
            ControlCommand::HotkeyInteractive | ControlCommand::Interactive => Hotkey::Interactive,
            ControlCommand::HotkeyInsertLast | ControlCommand::InsertLast => Hotkey::InsertLast,
            ControlCommand::Cancel => Hotkey::Cancel,
            ControlCommand::Status
            | ControlCommand::Start
            | ControlCommand::Stop
            | ControlCommand::HistorySearch { .. } => return None,
        };
        Some((event, false))
    }

    pub fn copy(&self, text: &str) -> Result<()> {
        paste::copy(&self.config.runtime_dir, &self.config.wayland_display, text)
    }
}

impl Platform for LinuxPlatform {
    type Window = tauri::Window;

    fn hotkeys(&self) -> Receiver<Hotkey> {
        self.hotkeys.clone()
    }

    fn focused(&self) -> Option<Target> {
        self.niri.as_ref()?.focused().ok().flatten()
    }

    fn focus(&self, target: &Target) -> Result<()> {
        self.niri
            .as_ref()
            .ok_or("clipboard mode cannot focus another app")?
            .focus(target)
    }

    fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        let niri = self
            .niri
            .as_ref()
            .ok_or("clipboard mode: use copy; no paste chord was posted")?;
        let mut keyboard = self
            .keyboard
            .as_ref()
            .ok_or("no virtual keyboard")?
            .lock()
            .map_err(|_| "virtual keyboard lock poisoned")?;
        paste::paste(niri, &self.config, target, text, send, &mut keyboard)
    }

    fn make_overlay(&self, window: &Self::Window) {
        let niri = self.niri.clone();
        let overlay = window.clone();
        if let Err(error) = window.run_on_main_thread(move || {
            let result = (|| -> Result<()> {
                let window = overlay.gtk_window()?;
                let monitor = niri
                    .as_ref()
                    .and_then(|niri| niri.focused_output_position().ok().flatten())
                    .and_then(|(x, y)| {
                        let display = gtk::prelude::WidgetExt::display(&window);
                        (0..display.n_monitors())
                            .filter_map(|index| display.monitor(index))
                            .find(|monitor| {
                                let geometry = monitor.geometry();
                                geometry.x() == x && geometry.y() == y
                            })
                    });
                overlay::configure(&window, niri.is_some(), monitor.as_ref())
            })();
            if let Err(error) = result {
                eprintln!("said overlay: {error}");
            }
        }) {
            eprintln!("said overlay: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input::Chord;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn settings_updates_keep_the_control_socket_and_focus_adapter_alive() {
        let directory = std::env::temp_dir().join(format!("said-configure-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut platform = LinuxPlatform::new(Config {
            runtime_dir: directory.clone(),
            wayland_display: "unused-test-display".into(),
            niri_socket: None,
            storage_key: "configure".into(),
            shortcuts: Shortcuts::default(),
            read_input: false,
            portal_enabled: false,
            paste_chords: BTreeMap::new(),
        })
        .unwrap();
        let socket = platform.control.path().to_path_buf();
        let inode = std::fs::metadata(&socket).unwrap().ino();
        platform.configure(
            Shortcuts::parse("super+d", "super+g", "super+i").unwrap(),
            BTreeMap::from([("synthetic-app".into(), PasteChord::CtrlV)]),
        );
        assert_eq!(std::fs::metadata(&socket).unwrap().ino(), inode);
        assert_eq!(
            platform.config.shortcuts.dictation,
            Chord::parse("super+d").unwrap()
        );
        assert_eq!(
            platform.config.paste_chords["synthetic-app"],
            PasteChord::CtrlV
        );
        drop(platform);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
