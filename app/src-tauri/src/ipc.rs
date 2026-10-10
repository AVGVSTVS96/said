use crate::{
    desktop::Desktop,
    runtime::{Host, HostPlatform},
    storage::Paths,
};
use crossbeam_channel::{Sender, bounded};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, ipc::Channel};
use voice_audio::CpalRecorder;
use voice_core::History;
use voice_core::{Result, *};
use voice_platform::Platform;

pub enum Request {
    Subscribe(
        Channel<HostSnapshot>,
        Sender<std::result::Result<HostSnapshot, String>>,
    ),
    Ui(UiAction, Sender<std::result::Result<HostSnapshot, String>>),
    Action(
        HostAction,
        Sender<std::result::Result<HostSnapshot, String>>,
    ),
    Audio(UtteranceId, Sender<std::result::Result<PathBuf, String>>),
    Shutdown,
}
#[derive(Clone)]
pub struct Connection(pub Sender<Request>);
impl Connection {
    fn response(
        &self,
        request: impl FnOnce(Sender<std::result::Result<HostSnapshot, String>>) -> Request,
    ) -> std::result::Result<HostSnapshot, String> {
        let (sender, receiver) = bounded(1);
        self.0.send(request(sender)).map_err(|e| e.to_string())?;
        receiver.recv().map_err(|e| e.to_string())?
    }
}
#[tauri::command]
pub async fn connect(
    connection: tauri::State<'_, Connection>,
    updates: Channel<HostSnapshot>,
) -> std::result::Result<HostSnapshot, String> {
    let connection = connection.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        connection.response(|reply| Request::Subscribe(updates, reply))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn ui_action(
    connection: tauri::State<'_, Connection>,
    action: UiAction,
) -> std::result::Result<HostSnapshot, String> {
    let connection = connection.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        connection.response(|reply| Request::Ui(action, reply))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn host_action(
    connection: tauri::State<'_, Connection>,
    action: HostAction,
) -> std::result::Result<HostSnapshot, String> {
    let connection = connection.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        connection.response(|reply| Request::Action(action, reply))
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn spawn(app: tauri::AppHandle, paths: Paths) -> Result<Connection> {
    let (sender, requests) = bounded(32);
    let (ready, started) = bounded(1);
    std::thread::spawn(move || {
        let result: Result<()> = (|| {
            let recorder = CpalRecorder::new(paths.data.join("audio"));
            let mut host = Host::open(Desktop::default(), recorder, paths)?;
            host.install()?;
            ready.send(Ok(())).map_err(|_| "app startup canceled")?;
            if let Some(overlay) = app
                .get_webview_window("overlay")
                .map(|view| view.as_ref().window().clone())
            {
                host.platform.make_overlay(&overlay);
            }
            let mut subscribers = vec![];
            let mut overlay_until: Option<Instant> = None;
            let mut overlay_id = None;
            let mut overlay_terminal = false;
            loop {
                let mut changed = host.poll()?;
                for hotkey in host.platform.hotkeys().try_iter() {
                    if let Err(error) = host.hotkey(hotkey, host.platform.release_observable()) {
                        eprintln!("{error}");
                    }
                    changed = true;
                }
                #[cfg(target_os = "linux")]
                for request in host.platform.control_requests().try_iter() {
                    use voice_platform::linux::control::ControlCommand as C;
                    let result = if matches!(
                        request.command,
                        C::Hotkey | C::HotkeyInteractive | C::HotkeyInsertLast
                    ) {
                        match host.platform.control_hotkey(request.command) {
                            Some((hotkey, observable)) => {
                                host.hotkey(hotkey, observable).map(|_| "applied".into())
                            }
                            None => Ok("handled by native key edges".into()),
                        }
                    } else {
                        crate::cli::control(&mut host, request.command)
                    }
                    .map_err(|e| e.to_string());
                    let _ = request.reply.send(result);
                    changed = true;
                }
                let mut reply = None;
                match requests.recv_timeout(Duration::from_millis(10)) {
                    Ok(Request::Subscribe(channel, response)) => {
                        subscribers.push(channel);
                        reply = Some((response, Ok(())));
                        changed = true;
                    }
                    Ok(Request::Ui(action, response)) => {
                        reply = Some((response, host.ui(action)));
                        changed = true;
                    }
                    Ok(Request::Action(action, response)) => {
                        reply = Some((response, host.action(action)));
                        changed = true;
                    }
                    Ok(Request::Audio(id, response)) => {
                        let result = host
                            .history
                            .get(id)
                            .and_then(|row| row.ok_or_else(|| "unknown audio entry".into()))
                            .map(|row| host.history.audio_path(row.audio_id))
                            .map_err(|e| e.to_string());
                        let _ = response.send(result);
                    }
                    Ok(Request::Shutdown)
                    | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        host.stop()?;
                        break;
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                }
                if overlay_until.is_some_and(|deadline| Instant::now() >= deadline) {
                    if let Some(window) = app
                        .get_webview_window("overlay")
                        .map(|view| view.as_ref().window().clone())
                    {
                        let _ = window.hide();
                    }
                    overlay_until = None;
                }
                if changed {
                    let snapshot = host.snapshot()?;
                    subscribers.retain(|channel: &Channel<HostSnapshot>| {
                        channel.send(snapshot.clone()).is_ok()
                    });
                    if let Some((sender, result)) = reply {
                        let _ = sender
                            .send(result.map(|_| snapshot.clone()).map_err(|e| e.to_string()));
                    }
                    if let Some(view) = snapshot.sessions.last() {
                        if snapshot.host.scratch != Some(view.id) {
                            if view.mode == Mode::Interactive {
                                if let Some(window) = app
                                    .get_webview_window("interactive")
                                    .map(|view| view.as_ref().window().clone())
                                {
                                    let _ = window.show();
                                }
                            } else if let Some(window) = app
                                .get_webview_window("overlay")
                                .map(|view| view.as_ref().window().clone())
                            {
                                let terminal = matches!(
                                    view.phase,
                                    Phase::Pasted | Phase::Saved | Phase::Failed
                                );
                                if overlay_id != Some(view.id) {
                                    #[cfg(target_os = "linux")]
                                    host.platform.make_overlay(&window);
                                    let _ = crate::desktop::show_overlay(&window);
                                }
                                if terminal && (!overlay_terminal || overlay_id != Some(view.id)) {
                                    overlay_until =
                                        Some(
                                            Instant::now()
                                                + Duration::from_secs(
                                                    if view.phase == Phase::Pasted { 1 } else { 3 },
                                                ),
                                        );
                                }
                                if !terminal {
                                    overlay_until = None;
                                }
                                overlay_terminal = terminal;
                                overlay_id = Some(view.id);
                            }
                        }
                    }
                    if let Some(reason) = &snapshot.host.unavailable {
                        let _ = app.emit("input-unavailable", reason);
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = ready.send(Err(error.to_string()));
            eprintln!("host stopped: {error}");
            let _ = app.emit("host-error", error.to_string());
        }
    });
    started
        .recv()
        .map_err(|_| "host startup thread exited")?
        .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { error.into() })?;
    Ok(Connection(sender))
}
pub fn audio(
    connection: &Connection,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let result = (|| -> Result<_> {
        let id = UtteranceId(request.uri().path().trim_start_matches('/').parse()?);
        let (sender, receiver) = bounded(1);
        connection.0.send(Request::Audio(id, sender))?;
        let path = receiver
            .recv_timeout(Duration::from_secs(5))?
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let range = request.headers().get("range").and_then(|v| v.to_str().ok());
        let (start, end) = match range {
            Some(value) => {
                let value = value.strip_prefix("bytes=").ok_or("invalid audio range")?;
                let (start, end) = value.split_once('-').ok_or("invalid audio range")?;
                if start.is_empty() {
                    let tail: u64 = end.parse()?;
                    (size.saturating_sub(tail), size.saturating_sub(1))
                } else {
                    (
                        start.parse()?,
                        if end.is_empty() {
                            size.saturating_sub(1)
                        } else {
                            end.parse()?
                        },
                    )
                }
            }
            None => (0, size.saturating_sub(1)),
        };
        if start >= size || end < start {
            return Err("audio range outside file".into());
        }
        let end = end.min(size - 1).min(start + 1024 * 1024 - 1);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0; (end - start + 1) as usize];
        file.read_exact(&mut bytes)?;
        Ok(tauri::http::Response::builder()
            .status(206)
            .header("Content-Type", "audio/wav")
            .header("Accept-Ranges", "bytes")
            .header("Content-Range", format!("bytes {start}-{end}/{size}"))
            .header("Content-Length", bytes.len())
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes)?)
    })();
    result.unwrap_or_else(|error| {
        tauri::http::Response::builder()
            .status(416)
            .body(error.to_string().into_bytes())
            .unwrap()
    })
}
