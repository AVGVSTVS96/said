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

#[derive(Clone, Debug)]
pub struct Chord {
    pub modifiers: Vec<Vec<KeyCode>>,
    pub key: KeyCode,
}

impl Default for Chord {
    fn default() -> Self {
        Self {
            modifiers: vec![
                vec![KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL],
                vec![KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT],
            ],
            key: KeyCode::KEY_SPACE,
        }
    }
}

impl Chord {
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
        Ok(Self { modifiers, key })
    }
}

pub struct KeyTracker {
    chord: Chord,
    devices: BTreeMap<u64, BTreeSet<KeyCode>>,
    active: bool,
}

impl KeyTracker {
    pub fn new(chord: Chord) -> Self {
        Self {
            chord,
            devices: BTreeMap::new(),
            active: false,
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
        if pressed && key == KeyCode::KEY_V && self.default_modifiers() {
            return vec![Hotkey::InsertLast];
        }
        self.transition(pressed && key == self.chord.key)
    }

    pub fn remove(&mut self, device: u64) -> Vec<Hotkey> {
        self.devices.remove(&device);
        self.transition(false)
    }

    pub fn reset(&mut self) -> Vec<Hotkey> {
        self.devices.clear();
        self.transition(false)
    }

    fn tracked(&self, key: KeyCode) -> bool {
        key == self.chord.key
            || self
                .chord
                .modifiers
                .iter()
                .flatten()
                .any(|modifier| *modifier == key)
            || [
                KeyCode::KEY_LEFTCTRL,
                KeyCode::KEY_RIGHTCTRL,
                KeyCode::KEY_LEFTALT,
                KeyCode::KEY_RIGHTALT,
                KeyCode::KEY_LEFTSHIFT,
                KeyCode::KEY_RIGHTSHIFT,
                KeyCode::KEY_V,
                KeyCode::KEY_ESC,
            ]
            .contains(&key)
    }

    fn down(&self, key: KeyCode) -> bool {
        self.devices.values().any(|keys| keys.contains(&key))
    }

    fn default_modifiers(&self) -> bool {
        (self.down(KeyCode::KEY_LEFTCTRL) || self.down(KeyCode::KEY_RIGHTCTRL))
            && (self.down(KeyCode::KEY_LEFTALT) || self.down(KeyCode::KEY_RIGHTALT))
    }

    fn transition(&mut self, trigger: bool) -> Vec<Hotkey> {
        let chord = self.down(self.chord.key)
            && self
                .chord
                .modifiers
                .iter()
                .all(|alternatives| alternatives.iter().any(|key| self.down(*key)));
        if self.active && !chord {
            self.active = false;
            return vec![Hotkey::Up];
        }
        if !self.active && chord && trigger {
            self.active = true;
            return vec![
                if self.down(KeyCode::KEY_LEFTSHIFT) || self.down(KeyCode::KEY_RIGHTSHIFT) {
                    Hotkey::Interactive
                } else {
                    Hotkey::Down
                },
            ];
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
    pub fn start(directory: &Path, chord: Chord, sender: Sender<Hotkey>) -> Result<Self> {
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
                chord,
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
        self.enabled.store(enabled, Ordering::Release);
        let _ = self.stop.write_all(&[0]);
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
    chord: Chord,
    sender: Sender<Hotkey>,
    state: (Arc<AtomicBool>, Arc<AtomicBool>),
) {
    let (enabled, available) = state;
    let mut tracker = KeyTracker::new(chord);
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

    fn key(tracker: &mut KeyTracker, device: u64, code: KeyCode, value: i32) -> Vec<Hotkey> {
        tracker.event(device, InputEvent::new(EventType::KEY.0, code.0, value))
    }

    #[test]
    fn fake_event_stream_runs_the_listener_dispatch_without_input_devices() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut tracker = KeyTracker::new(Chord::default());
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
        let mut tracker = KeyTracker::new(Chord::parse("ctrl+alt+space").unwrap());
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
        let mut tracker = KeyTracker::new(Chord::default());
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
