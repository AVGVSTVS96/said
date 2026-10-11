use gtk::prelude::*;
use gtk_layer_shell::LayerShell;
use std::{fs, path::PathBuf, process::Command, time::Duration};
use said_core::{Hotkey, Target};
use said_platform::{
    Platform,
    linux::{Config, LinuxPlatform, Mode, control::ControlCommand, overlay},
};

fn main() -> said_core::Result<()> {
    let root = PathBuf::from(std::env::var("SAID_TEST_ROOT")?);
    let mut config = Config::from_environment("said")?;
    config.read_input = false;
    config.portal_enabled = false;
    let platform = LinuxPlatform::new(config)?;
    assert_eq!(platform.mode(), Mode::NiriToggle);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
    let parent_display = std::env::var_os("SAID_TEST_PARENT_DISPLAY").unwrap();
    let mut bind_keyboard = said_platform::linux::keyboard::VirtualKeyboard::connect(
        std::path::Path::new(&runtime),
        &parent_display,
    )?;
    let controls = platform.control_requests();
    let subscription = platform.niri().unwrap().focus_changes()?;
    let niri_events = subscription.events();
    let windows: serde_json::Value = serde_json::from_slice(
        &Command::new("niri")
            .args(["msg", "-j", "windows"])
            .output()?
            .stdout,
    )?;
    let targets: Vec<Target> = windows
        .as_array()
        .unwrap()
        .iter()
        .map(|window| Target {
            app_id: window["app_id"].as_str().unwrap().into(),
            window_id: window["id"].to_string(),
            name: window["title"].as_str().unwrap().into(),
        })
        .collect();
    let text = "  no no, uh actually don't change foo_bar --dry-run /tmp/example.rs. café 🦀\nsecond line\n  ";
    for (name, app_id) in [
        ("gtk", "said-gtk-scratch"),
        ("terminal", "com.mitchellh.ghostty"),
        ("electron", "electron"),
    ] {
        let target = targets
            .iter()
            .find(|target| target.app_id == app_id)
            .ok_or("missing scratch target")?;
        platform.focus(target)?;
        platform.paste(target, text, false)?;
        let path = root.join(format!("{name}.txt"));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while fs::read(&path).ok().as_deref() != Some(text.as_bytes()) {
            if std::time::Instant::now() > deadline {
                return Err(format!("exact paste failed for {name}: {:?}", fs::read(&path)).into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        println!(
            "PASS exact UTF-8 paste into {name}: {} bytes, no trimming or rewriting",
            text.len()
        );
    }
    let target = targets
        .iter()
        .find(|target| target.app_id == "said-gtk-scratch")
        .unwrap();
    platform.focus(target)?;
    gtk::init()?;
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.add(&gtk::Label::new(Some("Listening — scratch overlay")));
    overlay::configure(&window, true, None)?;
    window.show_all();
    for _ in 0..30 {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(platform.focused().unwrap().same_window(target));
    println!("PASS nonfocusing layer-shell overlay");
    assert_eq!(window.keyboard_mode(), gtk_layer_shell::KeyboardMode::None);
    assert_eq!(window.exclusive_zone(), 0);
    platform.paste(target, "OVERLAY", false)?;
    window.close();
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
    let other = targets
        .iter()
        .find(|other| !other.same_window(target))
        .unwrap();
    platform.focus(other)?;
    assert!(platform.paste(target, "MUST NOT PASTE", false).is_err());
    println!("PASS focus mismatch rejects injection");
    let mut observed = false;
    while let Ok(event) = niri_events.recv_timeout(Duration::from_millis(100)) {
        if event.as_ref().is_some_and(|event| event.same_window(other)) {
            observed = true;
            break;
        }
    }
    assert!(observed);
    println!("PASS Niri focus event stream");
    bind_keyboard.physical_chord(&[29, 56, 57])?;
    let request = controls.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(request.command, ControlCommand::Hotkey);
    assert_eq!(
        platform.control_hotkey(request.command),
        Some((Hotkey::Down, false))
    );
    request.reply.send(Ok("toggle".into()))?;
    println!("PASS Niri bind launches said hotkey over owner-only control socket");
    let mut config = Config::from_environment("said-clipboard-test")?;
    config.niri_socket = None;
    config.read_input = false;
    config.wayland_display = parent_display.clone();
    let clipboard = LinuxPlatform::new(config)?;
    assert_eq!(clipboard.mode(), Mode::Clipboard);
    assert!(clipboard.focused().is_none());
    clipboard.copy(text)?;
    let copied = Command::new("wl-paste")
        .env("WAYLAND_DISPLAY", &parent_display)
        .args(["--no-newline", "--type", "text/plain;charset=utf-8"])
        .output()?;
    assert_eq!(copied.stdout, text.as_bytes());
    assert!(clipboard.paste(other, "MUST NOT INJECT", true).is_err());
    println!("PASS clipboard-only fallback preserves bytes and refuses input");
    for name in ["gtk", "electron", "terminal"] {
        let keys = fs::read(root.join(format!("{name}.keys"))).unwrap_or_default();
        match name {
            "gtk" => assert!(
                !String::from_utf8_lossy(&keys)
                    .lines()
                    .any(|key| key == "65293")
            ),
            "electron" => assert!(
                !String::from_utf8_lossy(&keys)
                    .lines()
                    .any(|key| key == "Enter")
            ),
            _ => {
                assert!(!keys.contains(&b'\r'));
                assert!(!keys.contains(&b'\n'));
            }
        }
    }
    println!("PASS no stray Enter into any scratch target");
    platform.focus(target)?;
    platform.paste(target, "EXPLICIT SEND", true)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !fs::read_to_string(root.join("gtk.keys"))
        .unwrap_or_default()
        .lines()
        .any(|key| key == "65293")
    {
        if std::time::Instant::now() > deadline {
            return Err("explicit send did not post Enter".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("PASS explicit send posts Enter only when requested");
    Ok(())
}
