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

use crossbeam_channel::{Receiver, unbounded};
use voice_core::{Hotkey, Result, Target};

use crate::Platform;
use control::{ControlCommand, ControlRequest, ControlSocket};
use gtk::prelude::*;
use input::{Chord, InputListener};
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
    pub chord: Chord,
    pub read_input: bool,
    pub portal_trigger: Option<String>,
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
            chord: Chord::default(),
            read_input: true,
            portal_trigger: Some("CTRL+ALT+space".into()),
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
        let (portal, portal_unavailable_reason) = match (&niri, &config.portal_trigger) {
            (Some(_), Some(trigger)) => match PortalHotkeys::start(trigger.clone(), sender.clone())
            {
                Ok(portal) => (Some(portal), None),
                Err(error) => (None, Some(error.to_string())),
            },
            _ => (None, None),
        };
        let (input, input_unavailable_reason) =
            if niri.is_some() && portal.is_none() && config.read_input {
                match InputListener::start(
                    std::path::Path::new("/dev/input"),
                    config.chord.clone(),
                    sender.clone(),
                ) {
                    Ok(input) => (Some(input), None),
                    Err(error) => (None, Some(error.to_string())),
                }
            } else {
                (None, None)
            };
        Ok(Self {
            config,
            niri,
            control,
            keyboard,
            input,
            portal,
            portal_unavailable_reason,
            input_unavailable_reason,
            hotkeys,
        })
    }

    pub fn input_unavailable_reason(&self) -> Option<&str> {
        self.input_unavailable_reason.as_deref()
    }

    pub fn mode(&self) -> Mode {
        if self.niri.is_none() {
            Mode::Clipboard
        } else if self.portal.as_ref().is_some_and(PortalHotkeys::available) {
            Mode::NiriPortal
        } else if self.input.as_ref().is_some_and(InputListener::available) {
            Mode::NiriHold
        } else {
            Mode::NiriToggle
        }
    }

    pub fn portal_unavailable_reason(&self) -> Option<&str> {
        self.portal_unavailable_reason.as_deref()
    }

    pub fn control_requests(&self) -> Receiver<ControlRequest> {
        self.control.requests()
    }
    pub fn niri(&self) -> Option<&Niri> {
        self.niri.as_ref()
    }

    pub fn set_input_enabled(&mut self, enabled: bool) {
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
            ControlCommand::Status => return None,
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
                eprintln!("voice overlay: {error}");
            }
        }) {
            eprintln!("voice overlay: {error}");
        }
    }
}
