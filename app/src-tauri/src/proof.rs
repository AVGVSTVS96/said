use crate::runtime::HostPlatform;
use crossbeam_channel::Receiver;
use voice_core::{Hotkey, Result, Settings, Target};
use voice_platform::{Platform, fake::FakePlatform};

pub struct SavedPlatform {
    pub fake: FakePlatform,
    pub database: std::path::PathBuf,
}
impl Platform for SavedPlatform {
    type Window = ();
    fn hotkeys(&self) -> Receiver<Hotkey> {
        self.fake.hotkeys()
    }
    fn focused(&self) -> Option<Target> {
        self.fake.focused()
    }
    fn focus(&self, target: &Target) -> Result<()> {
        self.fake.focus(target)
    }
    fn make_overlay(&self, window: &()) {
        self.fake.make_overlay(window);
    }
    fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        let database = rusqlite::Connection::open_with_flags(
            &self.database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let (raw, saved): (String, String) = database.query_row(
            "SELECT raw_text,text FROM utterances ORDER BY rowid DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if raw.as_bytes() != text.as_bytes() || saved.as_bytes() != text.as_bytes() {
            return Err("paste was attempted before an exact durable save".into());
        }
        self.fake.paste(target, text, send)
    }
}
impl HostPlatform for SavedPlatform {
    fn copy(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn configure(&mut self, _: &Settings) -> Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub fn nested(wav: &str, models: &std::path::Path) -> Result<()> {
    use crate::{
        runtime::{Host, worker_config},
        storage::Paths,
    };
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        time::{Duration, Instant},
    };

    use voice_platform::linux::{Config, LinuxPlatform, control::ControlCommand};
    struct NestedPlatform {
        native: LinuxPlatform,
        changes: voice_platform::linux::niri::FocusChanges,
    }
    impl Platform for NestedPlatform {
        type Window = tauri::Window;
        fn hotkeys(&self) -> Receiver<Hotkey> {
            self.native.hotkeys()
        }
        fn focused(&self) -> Option<Target> {
            self.native.focused()
        }
        fn focus(&self, target: &Target) -> Result<()> {
            self.native.focus(target)
        }
        fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
            self.native.paste(target, text, send)
        }
        fn make_overlay(&self, window: &tauri::Window) {
            self.native.make_overlay(window);
        }
    }
    impl HostPlatform for NestedPlatform {
        fn copy(&self, text: &str) -> Result<()> {
            self.native.copy(text)
        }
        fn configure(&mut self, _: &Settings) -> Result<()> {
            Ok(())
        }
        fn poll(&mut self) -> Vec<voice_core::SessionEvent> {
            self.changes
                .events()
                .try_iter()
                .map(|target| voice_core::SessionEvent::FocusChanged { target })
                .collect()
        }
    }
    let root = PathBuf::from(std::env::var("VOICE_TEST_ROOT")?).canonicalize()?;
    let runtime = PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?).canonicalize()?;
    let niri = PathBuf::from(std::env::var("NIRI_SOCKET")?);
    if runtime != root.join("runtime")
        || niri.parent() != Some(runtime.as_path())
        || std::env::var("WAYLAND_DISPLAY")? == std::env::var("VOICE_TEST_PARENT_DISPLAY")?
    {
        return Err("proof requires the isolated nested Niri harness".into());
    }
    let mut keyboard = voice_platform::linux::keyboard::VirtualKeyboard::connect(
        &runtime,
        &std::env::var_os("VOICE_TEST_PARENT_DISPLAY").ok_or("parent display missing")?,
    )?;
    let mut config = Config::from_environment(env!("PRODUCT_STORAGE_KEY"))?;
    config.read_input = false;
    config.portal_trigger = None;
    let native = LinuxPlatform::new(config)?;
    let changes = native
        .niri()
        .ok_or("nested Niri missing")?
        .focus_changes()?;
    let windows: serde_json::Value = serde_json::from_slice(
        &Command::new("niri")
            .args(["msg", "-j", "windows"])
            .output()?
            .stdout,
    )?;
    let target = windows
        .as_array()
        .ok_or("invalid windows")?
        .iter()
        .find(|window| window["app_id"] == "com.mitchellh.ghostty")
        .ok_or("scratch terminal missing")?;
    let target = Target {
        app_id: target["app_id"].as_str().unwrap().into(),
        window_id: target["id"].to_string(),
        name: "Scratch terminal".into(),
    };
    native.focus(&target)?;
    let path = root.join("terminal.txt");
    let before = fs::read(&path)?;
    let keys = path.with_extension("keys");
    let previous_keys = fs::read(&keys).unwrap_or_default();
    let data = root.join("app-proof");
    let mut paths = Paths::new(data.clone(), data.join("config"));
    paths.models = models.into();
    let recorder = replay_recorder(std::path::Path::new(wav), &paths.data)?;
    let mut host = Host::open(NestedPlatform { native, changes }, recorder, paths)?;
    host.set_client(voice_engine::host::WorkerClient::warm(worker_config(
        models,
        &host.settings.engine,
    )?)?);
    host.poll()?;
    let controls = host.platform.native.control_requests();
    keyboard.physical_chord(&[29, 56, 57])?;
    let request = controls
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("waiting for Niri bind to app socket: {error}"))?;
    if request.command != ControlCommand::Hotkey {
        return Err("Niri bind did not reach the app CLI".into());
    }
    let result = crate::cli::control(&mut host, request.command).map_err(|error| error.to_string());
    request.reply.send(result)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let row = loop {
        host.poll()?;
        if let Some(row) = host.history.last()?
            && row.pasted_at.is_some()
        {
            break row;
        }
        if Instant::now() >= deadline {
            return Err("nested host recognition/paste timed out".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let raw = row.raw_text.as_ref().ok_or("raw text missing")?;
    if raw.is_empty() || row.text.as_ref() != Some(raw) {
        return Err("non-verbatim or empty recognition".into());
    }
    let mut expected = before;
    expected.extend_from_slice(raw.as_bytes());
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read(&path)? != expected {
        if Instant::now() >= deadline {
            return Err("scratch terminal bytes differ from saved recognition".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if fs::read(&keys).unwrap_or_default() != previous_keys {
        return Err("a key escaped bracketed paste, including Enter".into());
    }
    fs::write(
        root.join("app-receipt.json"),
        serde_json::to_vec_pretty(&row)?,
    )?;
    let cli = Command::new(std::env::current_exe()?)
        .args(["history", "search", "no"])
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let request = controls.recv_timeout(Duration::from_secs(5))?;
    let result = crate::cli::control(&mut host, request.command).map_err(|error| error.to_string());
    request.reply.send(result)?;
    let output = cli.wait_with_output()?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains(&row.id.0.to_string())
    {
        return Err("history search CLI did not return the persisted row".into());
    }
    println!(
        "PASS Niri bind → voice CLI → real host → same-binary Moonshine worker → durable history → scratch terminal: {} exact UTF-8 bytes, no Enter",
        raw.len()
    );
    println!("PASS persistent history search over the owner-only control socket");
    Ok(())
}

pub fn replay_recorder(
    input: &std::path::Path,
    data: &std::path::Path,
) -> Result<voice_audio::ReplayRecorder> {
    crate::storage::private_dir(data)?;
    let source = data.join("replay-source.wav");
    let mut input = voice_engine::wav::Wav::open(input)?;
    let mut writer = voice_audio::wav::WavWriter::create(&source)?;
    while let Some(samples) = input.read()? {
        writer.append(&samples)?;
    }
    writer.finish()?;
    Ok(voice_audio::ReplayRecorder::new(source, data.join("audio")))
}
