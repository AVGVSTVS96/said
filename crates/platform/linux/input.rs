use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};

use crossbeam_channel::Sender;
use evdev::{BusType, Device, EventType, InputEvent, KeyCode};
use said_core::{Hotkey, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: Vec<Vec<KeyCode>>,
    pub key: KeyCode,
}

impl Default for Chord {
    fn default() -> Self {
        Self::parse(&said_core::Settings::defaults("").hotkey).unwrap()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcuts {
    pub dictation: Chord,
    pub interactive: Chord,
    pub insert_last: Chord,
}

impl Default for Shortcuts {
    fn default() -> Self {
        let settings = said_core::Settings::defaults("");
        Self::parse(
            &settings.hotkey,
            &settings.interactive_hotkey,
            &settings.insert_last_hotkey,
        )
        .unwrap()
    }
}

impl Shortcuts {
    pub fn chords_for_portal(&self) -> [String; 3] {
        self.chords().map(Chord::portal_trigger)
    }
    pub fn parse(dictation: &str, interactive: &str, insert_last: &str) -> Result<Self> {
        let shortcuts = Self {
            dictation: Chord::parse(dictation)?,
            interactive: Chord::parse(interactive)?,
            insert_last: Chord::parse(insert_last)?,
        };
        if shortcuts.dictation == shortcuts.interactive
            || shortcuts.dictation == shortcuts.insert_last
            || shortcuts.interactive == shortcuts.insert_last
        {
            return Err("shortcuts must be distinct".into());
        }
        Ok(shortcuts)
    }

    pub fn niri_binds(&self) -> String {
        let mut result = "binds {\n".to_owned();
        for (chord, action, title) in [
            (&self.dictation, "\"hotkey\"", "Dictate"),
            (
                &self.interactive,
                "\"hotkey\" \"interactive\"",
                "Review dictation",
            ),
            (
                &self.insert_last,
                "\"hotkey\" \"insert-last\"",
                "Insert last dictation",
            ),
        ] {
            result.push_str(&format!("    {} repeat=false allow-inhibiting=false hotkey-overlay-title=\"{title}: said\" {{ spawn \"said\" {action}; }}\n", chord.niri()));
        }
        result.push('}');
        result
    }

    fn chords(&self) -> [&Chord; 3] {
        [&self.dictation, &self.interactive, &self.insert_last]
    }
}

impl Chord {
    pub fn niri(&self) -> String {
        let mut parts = self
            .modifiers
            .iter()
            .map(|keys| match keys[0] {
                KeyCode::KEY_LEFTCTRL => "Ctrl",
                KeyCode::KEY_LEFTALT => "Alt",
                KeyCode::KEY_LEFTSHIFT => "Shift",
                KeyCode::KEY_LEFTMETA => "Super",
                _ => unreachable!(),
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let key = match self.key {
            KeyCode::KEY_SPACE => "Space".into(),
            KeyCode::KEY_ESC => "Escape".into(),
            KeyCode::KEY_ENTER => "Return".into(),
            KeyCode::KEY_GRAVE => "grave".into(),
            KeyCode::KEY_LEFTBRACE => "bracketleft".into(),
            KeyCode::KEY_RIGHTBRACE => "bracketright".into(),
            KeyCode::KEY_SEMICOLON => "semicolon".into(),
            KeyCode::KEY_APOSTROPHE => "apostrophe".into(),
            KeyCode::KEY_COMMA => "comma".into(),
            KeyCode::KEY_DOT => "period".into(),
            KeyCode::KEY_SLASH => "slash".into(),
            KeyCode::KEY_BACKSLASH => "backslash".into(),
            _ => format!("{:?}", self.key).trim_start_matches("KEY_").into(),
        };
        parts.push(key);
        parts.join("+")
    }

    pub fn portal_trigger(&self) -> String {
        self.niri()
            .split('+')
            .map(|part| match part {
                "Super" => "LOGO".into(),
                "Ctrl" | "Alt" | "Shift" => part.to_ascii_uppercase(),
                _ => part.to_ascii_lowercase(),
            })
            .collect::<Vec<_>>()
            .join("+")
    }

    pub fn parse(binding: &str) -> Result<Self> {
        let mut parts = binding.split('+');
        let key = parts.next_back().ok_or("hotkey needs a key")?;
        let key: KeyCode = format!("KEY_{}", key.to_ascii_uppercase())
            .parse()
            .map_err(|_| "unknown hotkey key")?;
        let mut modifiers = Vec::new();
        for modifier in parts {
            let alternatives = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => vec![KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL],
                "alt" => vec![KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT],
                "shift" => vec![KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_RIGHTSHIFT],
                "super" | "meta" => vec![KeyCode::KEY_LEFTMETA, KeyCode::KEY_RIGHTMETA],
                _ => return Err("unknown hotkey modifier".into()),
            };
            if !modifiers.contains(&alternatives) {
                modifiers.push(alternatives);
            }
        }
        if modifiers.is_empty() || modifiers.iter().flatten().any(|modifier| *modifier == key) {
            return Err("hotkey needs modifiers and a distinct trigger key".into());
        }
        modifiers.sort_by_key(|keys| keys[0].0);
        Ok(Self { modifiers, key })
    }
}

pub struct KeyTracker {
    shortcuts: Shortcuts,
    devices: BTreeMap<u64, BTreeSet<KeyCode>>,
    active: Option<usize>,
}

impl KeyTracker {
    pub fn new(shortcuts: Shortcuts) -> Self {
        Self {
            shortcuts,
            devices: BTreeMap::new(),
            active: None,
        }
    }

    pub fn event(&mut self, device: u64, event: InputEvent) -> Vec<Hotkey> {
        if event.event_type() != EventType::KEY || event.value() == 2 {
            return Vec::new();
        }
        let key = KeyCode(event.code());
        if !self.tracked(key) {
            return Vec::new();
        }
        let pressed = event.value() == 1;
        let keys = self.devices.entry(device).or_default();
        let changed = if pressed {
            keys.insert(key)
        } else {
            keys.remove(&key)
        };
        if !changed {
            return Vec::new();
        }
        if pressed && key == KeyCode::KEY_ESC {
            return vec![Hotkey::Cancel];
        }
        let mut events = self.transition();
        if pressed {
            if self.matches(&self.shortcuts.insert_last) && key == self.shortcuts.insert_last.key {
                events.push(Hotkey::InsertLast);
            } else if self.active.is_none() {
                for (index, event) in [
                    (&self.shortcuts.dictation, Hotkey::Down),
                    (&self.shortcuts.interactive, Hotkey::Interactive),
                ]
                .into_iter()
                .enumerate()
                {
                    let (chord, event) = event;
                    if key == chord.key && self.matches(chord) {
                        self.active = Some(index);
                        events.push(event);
                        break;
                    }
                }
            }
        }
        events
    }

    pub fn remove(&mut self, device: u64) -> Vec<Hotkey> {
        self.devices.remove(&device);
        self.transition()
    }

    pub fn reset(&mut self) -> Vec<Hotkey> {
        self.devices.clear();
        self.transition()
    }

    fn tracked(&self, key: KeyCode) -> bool {
        key == KeyCode::KEY_ESC
            || self.shortcuts.chords().iter().any(|chord| {
                chord.key == key
                    || chord
                        .modifiers
                        .iter()
                        .flatten()
                        .any(|modifier| *modifier == key)
            })
            || [
                KeyCode::KEY_LEFTCTRL,
                KeyCode::KEY_RIGHTCTRL,
                KeyCode::KEY_LEFTALT,
                KeyCode::KEY_RIGHTALT,
                KeyCode::KEY_LEFTSHIFT,
                KeyCode::KEY_RIGHTSHIFT,
                KeyCode::KEY_LEFTMETA,
                KeyCode::KEY_RIGHTMETA,
            ]
            .contains(&key)
    }

    fn down(&self, key: KeyCode) -> bool {
        self.devices.values().any(|keys| keys.contains(&key))
    }

    fn matches(&self, chord: &Chord) -> bool {
        self.down(chord.key)
            && [
                [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL],
                [KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT],
                [KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_RIGHTSHIFT],
                [KeyCode::KEY_LEFTMETA, KeyCode::KEY_RIGHTMETA],
            ]
            .iter()
            .all(|keys| {
                keys.iter().any(|key| self.down(*key))
                    == chord.modifiers.iter().any(|group| group.as_slice() == keys)
            })
    }

    fn transition(&mut self) -> Vec<Hotkey> {
        if let Some(index) = self.active {
            let chord = self.shortcuts.chords()[index];
            if !self.matches(chord) {
                self.active = None;
                return vec![Hotkey::Up];
            }
        }
        Vec::new()
    }
}

pub struct InputListener {
    stop: UnixStream,
    enabled: Arc<AtomicBool>,
    available: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl InputListener {
    pub fn start(directory: &Path, shortcuts: Shortcuts, sender: Sender<Hotkey>) -> Result<Self> {
        let directory = directory.to_path_buf();
        let watch = watch_directory(&directory)?;
        let devices = keyboards(&directory);
        if devices.is_empty() {
            return Err("no readable physical keyboards; use the Niri toggle bind or grant input-group access".into());
        }
        let (stop, stopped) = UnixStream::pair()?;
        let enabled = Arc::new(AtomicBool::new(true));
        let available = Arc::new(AtomicBool::new(true));
        let thread_enabled = enabled.clone();
        let thread_available = available.clone();
        let thread = thread::spawn(move || {
            listen(
                directory,
                watch,
                devices,
                stopped,
                shortcuts,
                sender,
                (thread_enabled, thread_available),
            );
        });
        Ok(Self {
            stop,
            enabled,
            available,
            thread: Some(thread),
        })
    }

    pub fn available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled.swap(enabled, Ordering::AcqRel) != enabled {
            let _ = self.stop.write_all(&[0]);
        }
    }
}

impl Drop for InputListener {
    fn drop(&mut self) {
        let _ = self.stop.write_all(&[1]);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn keyboards(directory: &Path) -> BTreeMap<PathBuf, Device> {
    let Ok(entries) = fs::read_dir(directory) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if !path.file_name()?.to_str()?.starts_with("event") {
                return None;
            }
            let device = Device::open(&path).ok()?;
            if device.input_id().bus_type() == BusType::BUS_VIRTUAL {
                return None;
            }
            let keys = device.supported_keys()?;
            if ![KeyCode::KEY_A, KeyCode::KEY_Z, KeyCode::KEY_ENTER]
                .iter()
                .all(|key| keys.contains(*key))
            {
                return None;
            }
            device.set_nonblocking(true).ok()?;
            Some((path, device))
        })
        .collect()
}

fn watch_directory(directory: &Path) -> Result<OwnedFd> {
    let descriptor = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let watch = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let path = CString::new(directory.as_os_str().as_encoded_bytes())?;
    if unsafe {
        libc::inotify_add_watch(
            watch.as_raw_fd(),
            path.as_ptr(),
            libc::IN_CREATE
                | libc::IN_DELETE
                | libc::IN_ATTRIB
                | libc::IN_MOVED_TO
                | libc::IN_MOVED_FROM,
        )
    } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(watch)
}

fn listen(
    directory: PathBuf,
    watch: OwnedFd,
    mut devices: BTreeMap<PathBuf, Device>,
    mut stopped: UnixStream,
    shortcuts: Shortcuts,
    sender: Sender<Hotkey>,
    state: (Arc<AtomicBool>, Arc<AtomicBool>),
) {
    let (enabled, available) = state;
    let mut tracker = KeyTracker::new(shortcuts);
    loop {
        let paths: Vec<_> = devices.keys().cloned().collect();
        let mut descriptors = vec![
            libc::pollfd {
                fd: stopped.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: watch.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        descriptors.extend(devices.values().map(|device| libc::pollfd {
            fd: device.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }));
        if unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                -1,
            )
        } < 0
        {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if descriptors[0].revents != 0 {
            let mut commands = [0; 64];
            let Ok(count) = stopped.read(&mut commands) else {
                break;
            };
            emit(&sender, tracker.reset());
            if count == 0 || commands[..count].contains(&1) {
                break;
            }
        }
        for (index, path) in paths.iter().enumerate() {
            if descriptors[index + 2].revents == 0 {
                continue;
            }
            let Some(device) = devices.get_mut(path) else {
                continue;
            };
            let id = device.as_raw_fd() as u64;
            let error = device
                .fetch_events()
                .map(|events| {
                    process_events(
                        &mut tracker,
                        id,
                        events,
                        &sender,
                        enabled.load(Ordering::Acquire),
                    )
                })
                .err();
            if error.is_some_and(|error| error.kind() != io::ErrorKind::WouldBlock) {
                emit(&sender, tracker.remove(id));
                devices.remove(path);
            }
        }
        if descriptors[1].revents != 0 {
            let mut buffer = [0_u8; 4096];
            while unsafe { libc::read(watch.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) }
                > 0
            {}
            let mut detected = keyboards(&directory);
            devices.retain(|path, device| {
                let present = detected.remove(path).is_some();
                if !present {
                    emit(&sender, tracker.remove(device.as_raw_fd() as u64));
                }
                present
            });
            devices.extend(detected);
        }
        available.store(!devices.is_empty(), Ordering::Release);
    }
    emit(&sender, tracker.reset());
    available.store(false, Ordering::Release);
}

fn process_events(
    tracker: &mut KeyTracker,
    device: u64,
    events: impl Iterator<Item = InputEvent>,
    sender: &Sender<Hotkey>,
    enabled: bool,
) {
    for event in events {
        if enabled {
            emit(sender, tracker.event(device, event));
        }
    }
}

fn emit(sender: &Sender<Hotkey>, events: Vec<Hotkey>) {
    for event in events {
        let _ = sender.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_niri_binds_match_the_settings_and_all_actions() {
        assert_eq!(
            Shortcuts::default().niri_binds(),
            include_str!("../../../packaging/linux/niri.kdl").trim()
        );
        let configured = Shortcuts::parse("meta+control+space", "super+g", "alt+a").unwrap();
        let binds = configured.niri_binds();
        assert!(binds.starts_with("binds {\n"));
        assert!(binds.contains("Ctrl+Super+Space"));
        assert!(binds.contains("\"hotkey\" \"interactive\""));
        assert!(binds.contains("\"hotkey\" \"insert-last\""));
        assert_eq!(configured.dictation.portal_trigger(), "CTRL+LOGO+space");
    }

    #[test]
    fn configured_actions_use_exact_modifiers_and_pair_their_releases() {
        let mut tracker = KeyTracker::new(Shortcuts::default());
        key(&mut tracker, 1, KeyCode::KEY_LEFTMETA, 1);
        assert_eq!(
            key(&mut tracker, 1, KeyCode::KEY_X, 1),
            [Hotkey::Interactive]
        );
        assert_eq!(key(&mut tracker, 1, KeyCode::KEY_X, 0), [Hotkey::Up]);
        assert_eq!(
            key(&mut tracker, 1, KeyCode::KEY_A, 1),
            [Hotkey::InsertLast]
        );
        assert!(key(&mut tracker, 1, KeyCode::KEY_A, 2).is_empty());
        assert!(key(&mut tracker, 1, KeyCode::KEY_A, 0).is_empty());
        key(&mut tracker, 1, KeyCode::KEY_LEFTSHIFT, 1);
        assert!(key(&mut tracker, 1, KeyCode::KEY_Z, 1).is_empty());
        key(&mut tracker, 1, KeyCode::KEY_Z, 0);
        key(&mut tracker, 1, KeyCode::KEY_LEFTSHIFT, 0);
        assert_eq!(key(&mut tracker, 1, KeyCode::KEY_Z, 1), [Hotkey::Down]);
        assert_eq!(key(&mut tracker, 1, KeyCode::KEY_LEFTMETA, 0), [Hotkey::Up]);
    }

    fn key(tracker: &mut KeyTracker, device: u64, code: KeyCode, value: i32) -> Vec<Hotkey> {
        tracker.event(device, InputEvent::new(EventType::KEY.0, code.0, value))
    }

    #[test]
    fn fake_event_stream_runs_the_listener_dispatch_without_input_devices() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut tracker = KeyTracker::new(
            Shortcuts::parse("ctrl+alt+space", "ctrl+alt+shift+space", "ctrl+alt+v").unwrap(),
        );
        let stream = [
            (KeyCode::KEY_LEFTCTRL, 1),
            (KeyCode::KEY_LEFTALT, 1),
            (KeyCode::KEY_SPACE, 1),
            (KeyCode::KEY_SPACE, 2),
            (KeyCode::KEY_A, 1),
            (KeyCode::KEY_SPACE, 0),
        ]
        .into_iter()
        .map(|(key, value)| InputEvent::new(EventType::KEY.0, key.0, value));
        process_events(&mut tracker, 1, stream, &sender, true);
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            [Hotkey::Down, Hotkey::Up]
        );
        let stream = [KeyCode::KEY_SPACE]
            .into_iter()
            .map(|key| InputEvent::new(EventType::KEY.0, key.0, 1));
        process_events(&mut tracker, 1, stream, &sender, false);
        assert!(receiver.is_empty());
    }

    #[test]
    fn fake_devices_hold_repeats_modifier_release_and_unplug() {
        let mut tracker = KeyTracker::new(
            Shortcuts::parse("ctrl+alt+space", "ctrl+alt+shift+space", "ctrl+alt+v").unwrap(),
        );
        assert!(key(&mut tracker, 1, KeyCode::KEY_A, 1).is_empty());
        assert!(key(&mut tracker, 1, KeyCode::KEY_LEFTCTRL, 1).is_empty());
        assert!(key(&mut tracker, 2, KeyCode::KEY_RIGHTALT, 1).is_empty());
        assert_eq!(key(&mut tracker, 2, KeyCode::KEY_SPACE, 1), [Hotkey::Down]);
        assert!(key(&mut tracker, 2, KeyCode::KEY_SPACE, 2).is_empty());
        assert_eq!(key(&mut tracker, 1, KeyCode::KEY_LEFTCTRL, 0), [Hotkey::Up]);
        assert!(key(&mut tracker, 1, KeyCode::KEY_LEFTCTRL, 1).is_empty());
        assert!(key(&mut tracker, 2, KeyCode::KEY_SPACE, 0).is_empty());
        assert_eq!(key(&mut tracker, 2, KeyCode::KEY_SPACE, 1), [Hotkey::Down]);
        assert_eq!(tracker.remove(2), [Hotkey::Up]);
    }

    #[test]
    fn fake_stream_interactive_insert_cancel_and_reset() {
        let mut tracker = KeyTracker::new(
            Shortcuts::parse("ctrl+alt+space", "ctrl+alt+shift+space", "ctrl+alt+v").unwrap(),
        );
        for code in [
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_RIGHTSHIFT,
        ] {
            key(&mut tracker, 1, code, 1);
        }
        assert_eq!(
            key(&mut tracker, 1, KeyCode::KEY_SPACE, 1),
            [Hotkey::Interactive]
        );
        assert_eq!(tracker.reset(), [Hotkey::Up]);
        assert_eq!(key(&mut tracker, 1, KeyCode::KEY_ESC, 1), [Hotkey::Cancel]);
        key(&mut tracker, 1, KeyCode::KEY_LEFTCTRL, 1);
        key(&mut tracker, 1, KeyCode::KEY_LEFTALT, 1);
        assert_eq!(
            key(&mut tracker, 1, KeyCode::KEY_V, 1),
            [Hotkey::InsertLast]
        );
    }
}
