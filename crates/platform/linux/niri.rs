use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gtk::gio::{DesktopAppInfo, prelude::AppInfoExt};
use said_core::{Result, Target};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Niri {
    socket: PathBuf,
}

impl Niri {
    pub fn new(socket: impl Into<PathBuf>) -> Result<Self> {
        let niri = Self {
            socket: socket.into(),
        };
        niri.request(json!("Version"))?;
        Ok(niri)
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn focused(&self) -> Result<Option<Target>> {
        let reply = self.request(json!("FocusedWindow"))?;
        let window = &reply["FocusedWindow"];
        if window.is_null() {
            return Ok(None);
        }
        target(window).map(Some)
    }

    pub fn focus(&self, target: &Target) -> Result<()> {
        let id: u64 = target.window_id.parse()?;
        let windows = self.request(json!("Windows"))?;
        let exists = windows["Windows"].as_array().is_some_and(|windows| {
            windows.iter().any(|window| {
                window["id"].as_u64() == Some(id)
                    && window["app_id"].as_str() == Some(&target.app_id)
            })
        });
        if !exists {
            return Err("target window closed or its app identity changed".into());
        }
        let changes = self.focus_changes()?;
        self.request(json!({"Action": {"FocusWindow": {"id": id}}}))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !self
            .focused()?
            .is_some_and(|focused| focused.same_window(target))
        {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            changes
                .events()
                .recv_timeout(remaining)
                .map_err(|_| "Niri did not focus the requested window")?;
        }
        Ok(())
    }

    pub fn focused_output_position(&self) -> Result<Option<(i32, i32)>> {
        let reply = self.request(json!("FocusedOutput"))?;
        let logical = &reply["FocusedOutput"]["logical"];
        if logical.is_null() {
            return Ok(None);
        }
        Ok(Some((
            logical["x"].as_i64().ok_or("output has no x position")? as i32,
            logical["y"].as_i64().ok_or("output has no y position")? as i32,
        )))
    }

    pub fn events(&self) -> Result<BufReader<UnixStream>> {
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.write_all(b"\"EventStream\"\n")?;
        let mut reader = BufReader::new(stream);
        read_reply(&mut reader)?;
        Ok(reader)
    }

    fn request(&self, request: Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        serde_json::to_writer(&mut stream, &request)?;
        stream.write_all(b"\n")?;
        read_reply(&mut BufReader::new(stream))
    }
}

fn read_reply(reader: &mut impl BufRead) -> Result<Value> {
    let mut line = String::new();
    reader.take(4 * 1024 * 1024).read_line(&mut line)?;
    let reply: Value = serde_json::from_str(&line)?;
    if let Some(error) = reply.get("Err") {
        return Err(format!("Niri IPC: {error}").into());
    }
    reply
        .get("Ok")
        .cloned()
        .ok_or_else(|| "invalid Niri reply".into())
}

fn target(window: &Value) -> Result<Target> {
    let app_id = window["app_id"].as_str().ok_or("window has no app_id")?;
    let desktop_id = if app_id.ends_with(".desktop") {
        app_id.to_owned()
    } else {
        format!("{app_id}.desktop")
    };
    let name = DesktopAppInfo::new(&desktop_id)
        .map(|app| app.name().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| app_id.to_owned());
    Ok(Target {
        window_id: window["id"]
            .as_u64()
            .ok_or("window has no numeric id")?
            .to_string(),
        app_id: app_id.into(),
        name,
    })
}

pub struct FocusChanges {
    events: crossbeam_channel::Receiver<Option<Target>>,
    pub queue: std::sync::Arc<crate::focus::FocusQueue>,
    stream: UnixStream,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FocusChanges {
    pub fn events(&self) -> crossbeam_channel::Receiver<Option<Target>> {
        self.events.clone()
    }
}

impl Drop for FocusChanges {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Niri {
    pub fn focus_changes(&self) -> Result<FocusChanges> {
        let mut reader = self.events()?;
        let stream = reader.get_ref().try_clone()?;
        let (sender, events) = crossbeam_channel::unbounded();
        let queue = std::sync::Arc::new(crate::focus::FocusQueue::default());
        let observed = queue.clone();
        let thread = std::thread::spawn(move || {
            let mut windows = std::collections::BTreeMap::new();
            let mut focused: Option<Target> = None;
            let mut line = String::new();
            loop {
                line.clear();
                if reader
                    .read_line(&mut line)
                    .ok()
                    .filter(|size| *size > 0)
                    .is_none()
                {
                    break;
                }
                let Ok(event) = serde_json::from_str::<Value>(&line) else {
                    break;
                };
                let mut changed = false;
                if let Some(event) = event.get("WindowsChanged") {
                    windows.clear();
                    focused = None;
                    if let Some(items) = event["windows"].as_array() {
                        for window in items {
                            if let Ok(window_target) = target(window) {
                                if window["is_focused"].as_bool() == Some(true) {
                                    focused = Some(window_target.clone());
                                }
                                windows.insert(window_target.window_id.clone(), window_target);
                            }
                        }
                    }
                    changed = true;
                } else if let Some(event) = event.get("WindowOpenedOrChanged") {
                    let window = &event["window"];
                    if window["is_focused"].as_bool() == Some(true) {
                        focused = target(window).ok();
                        changed = true;
                    }
                    if let Ok(window_target) = target(window) {
                        windows.insert(window_target.window_id.clone(), window_target);
                    }
                } else if let Some(event) = event.get("WindowFocusChanged") {
                    focused = event["id"]
                        .as_u64()
                        .and_then(|id| windows.get(&id.to_string()).cloned());
                    changed = true;
                } else if let Some(event) = event.get("WindowClosed")
                    && let Some(id) = event["id"].as_u64()
                {
                    windows.remove(&id.to_string());
                    if focused
                        .as_ref()
                        .is_some_and(|target| target.window_id == id.to_string())
                    {
                        focused = None;
                        changed = true;
                    }
                }
                if changed {
                    observed.push(focused.clone());
                    if sender.send(focused.clone()).is_err() {
                        return;
                    }
                }
            }
            observed.push(None);
            let _ = sender.send(None);
        });
        Ok(FocusChanges {
            events,
            queue,
            stream,
            thread: Some(thread),
        })
    }
}
