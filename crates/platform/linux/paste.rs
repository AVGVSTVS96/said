use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use voice_core::{Result, Target};

use super::{Config, keyboard::VirtualKeyboard, niri::Niri};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PasteChord {
    #[default]
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

impl PasteChord {
    fn post(self, keyboard: &mut VirtualKeyboard) -> Result<()> {
        match self {
            Self::CtrlV => keyboard.tap(47, true, false, false),
            Self::CtrlShiftV => keyboard.tap(47, true, true, false),
            Self::ShiftInsert => keyboard.tap(110, false, true, false),
        }
    }
}

pub fn default_chords() -> BTreeMap<String, PasteChord> {
    [
        "com.mitchellh.ghostty",
        "Alacritty",
        "kitty",
        "foot",
        "footclient",
        "org.gnome.Terminal",
        "org.gnome.Console",
        "konsole",
        "org.kde.konsole",
        "wezterm",
        "org.wezfurlong.wezterm",
        "rio",
    ]
    .into_iter()
    .map(|app| (app.into(), PasteChord::CtrlShiftV))
    .collect()
}

pub fn copy(runtime: &Path, display: &OsStr, text: &str) -> Result<()> {
    let mut child = command("wl-copy", runtime, display)
        .args(["--type", "text/plain;charset=utf-8"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let written = child
        .stdin
        .take()
        .ok_or("wl-copy stdin unavailable")?
        .write_all(text.as_bytes());
    let status = child.wait()?;
    written?;
    if !status.success() {
        return Err(format!("wl-copy failed: {status}").into());
    }
    Ok(())
}

pub fn paste(
    niri: &Niri,
    config: &Config,
    target: &Target,
    text: &str,
    send: bool,
    keyboard: &mut VirtualKeyboard,
) -> Result<()> {
    check_focus(niri, target)?;
    if text.is_empty() {
        return Ok(());
    }
    copy(&config.runtime_dir, &config.wayland_display, text)?;
    check_focus(niri, target)?;
    config
        .paste_chords
        .get(&target.app_id)
        .copied()
        .unwrap_or_default()
        .post(keyboard)?;
    if send {
        thread::sleep(Duration::from_millis(50));
        check_focus(niri, target)?;
        keyboard.tap(28, false, false, false)?;
    }
    Ok(())
}

fn check_focus(niri: &Niri, target: &Target) -> Result<()> {
    if !niri
        .focused()?
        .is_some_and(|focused| focused.same_window(target))
    {
        return Err("focus changed; no further input was posted".into());
    }
    Ok(())
}

fn command(program: &str, runtime: &Path, display: &OsStr) -> Command {
    let mut command = Command::new(program);
    command
        .env("XDG_RUNTIME_DIR", runtime)
        .env("WAYLAND_DISPLAY", display)
        .env_remove("WAYLAND_SOCKET")
        .env_remove("DISPLAY");
    command
}
