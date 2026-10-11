use crate::{
    desktop::Desktop,
    runtime::{Host, HostPlatform},
    storage::Paths,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use said_audio::CpalRecorder;
use said_core::History;
use said_core::{Result, *};
use said_platform::Platform;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, ipc::Channel};

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
    CloseInteractive,
    Shutdown,
}
#[derive(Clone)]
pub struct Connection(pub Sender<Request>);
impl Connection {
    pub fn start(
        run: impl FnOnce(Receiver<Request>) -> Result<()> + Send + 'static,
        failed: impl FnOnce(String) + Send + 'static,
    ) -> Result<Self> {
        let (sender, requests) = bounded(32);
        std::thread::Builder::new()
            .name("said-host".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(requests)))
                        .unwrap_or_else(|_| Err("host actor panicked".into()));
                if let Err(error) = result {
                    failed(error.to_string());
                }
            })?;
        Ok(Self(sender))
    }
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

#[cfg(target_os = "linux")]
pub fn reply_control(
    request: said_platform::linux::control::ControlRequest,
    apply: impl FnOnce(said_platform::linux::control::ControlCommand) -> Result<String>,
) {
    let result = if request.expired() {
        Err("control request expired".into())
    } else {
        apply(request.command)
    };
    let _ = request
        .reply
        .try_send(result.map_err(|error| error.to_string()));
}

pub fn tray_icon(recording: bool) -> tauri::image::Image<'static> {
    match (cfg!(target_os = "macos"), recording) {
        (true, false) => tauri::include_image!("icons/tray/iconTemplate@2x.png"),
        (true, true) => tauri::include_image!("icons/tray/iconRecordingTemplate@2x.png"),
        (false, false) => tauri::include_image!("icons/tray/said-tray-idle-paper-48.png"),
        (false, true) => tauri::include_image!("icons/tray/said-tray-recording-paper-48.png"),
    }
}

pub fn spawn(app: tauri::AppHandle, paths: Paths) -> Result<Connection> {
    let errors = app.clone();
    Connection::start(
        move |requests| {
            let recorder = CpalRecorder::new(paths.data.join("audio"));
            let mut host = Host::open(Desktop::default(), recorder, paths)?;
            host.install()?;
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
            let mut interactive_id = None;
            let mut interactive_closed = false;
            let mut tray_label = String::new();
            let mut tray_recording = None;
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
                    use said_platform::linux::control::ControlCommand as C;
                    reply_control(request, |command| {
                        if matches!(
                            command,
                            C::Hotkey | C::HotkeyInteractive | C::HotkeyInsertLast
                        ) {
                            match host.platform.control_hotkey(command) {
                                Some((hotkey, observable)) => {
                                    host.hotkey(hotkey, observable).map(|_| "applied".into())
                                }
                                None => Ok("handled by native key edges".into()),
                            }
                        } else {
                            crate::cli::control(&mut host, command)
                        }
                    });
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
                    Ok(Request::CloseInteractive) => {
                        interactive_closed = true;
                        changed = true;
                    }
                    Ok(Request::Shutdown)
                    | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        host.event(SessionEvent::SessionUnavailable {
                            reason: "App closing".into(),
                        })?;
                        host.stop()?;
                        app.exit(0);
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
                    if interactive_closed {
                        let ids: Vec<_> = host
                            .session
                            .views()
                            .filter(|view| {
                                view.mode == Mode::Interactive
                                    && !matches!(
                                        view.phase,
                                        Phase::Recording
                                            | Phase::Finishing
                                            | Phase::Saving
                                            | Phase::Pasting
                                    )
                            })
                            .map(|view| view.id)
                            .collect();
                        for id in ids {
                            host.session.dismiss(id);
                        }
                    }
                    let snapshot = host.snapshot()?;
                    let recording = host.recording();
                    if tray_recording != Some(recording)
                        && let Some(tray) = app.tray_by_id("main")
                    {
                        let _ = tray.set_icon(Some(tray_icon(recording)));
                        tray_recording = Some(recording);
                    }
                    let label = if let Some(reason) = &snapshot.host.unavailable {
                        format!("{} · {reason}", env!("PRODUCT_NAME"))
                    } else if snapshot
                        .host
                        .permissions
                        .values()
                        .any(|status| status != "authorized")
                    {
                        format!("{} · Grant permissions in Setup", env!("PRODUCT_NAME"))
                    } else if snapshot.host.download.state != "ready" {
                        format!(
                            "{} · Model {}",
                            env!("PRODUCT_NAME"),
                            snapshot.host.download.state
                        )
                    } else {
                        env!("PRODUCT_NAME").into()
                    };
                    if label != tray_label {
                        if let Some(tray) = app.tray_by_id("main") {
                            let _ = tray.set_tooltip(Some(&label));
                        }
                        tray_label = label;
                    }

                    subscribers.retain(|channel: &Channel<HostSnapshot>| {
                        channel.send(snapshot.clone()).is_ok()
                    });
                    if let Some((sender, result)) = reply {
                        let _ = sender
                            .send(result.map(|_| snapshot.clone()).map_err(|e| e.to_string()));
                    }
                    if let Some(view) = snapshot.sessions.last()
                        && snapshot.host.scratch != Some(view.id)
                    {
                        if view.mode == Mode::Interactive {
                            if let Some(window) = app.get_webview_window("overlay") {
                                let _ = window.hide();
                            }
                            if view.phase == Phase::Recording && interactive_id != Some(view.id) {
                                interactive_id = Some(view.id);
                                interactive_closed = false;
                                if let Some(window) = app.get_webview_window("interactive") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        } else if let Some(window) = app
                            .get_webview_window("overlay")
                            .map(|view| view.as_ref().window().clone())
                        {
                            let terminal =
                                matches!(view.phase, Phase::Pasted | Phase::Saved | Phase::Failed);
                            if overlay_id != Some(view.id) {
                                #[cfg(target_os = "linux")]
                                host.platform.make_overlay(&window);
                                let _ = crate::desktop::show_overlay(&window);
                            }
                            if terminal && (!overlay_terminal || overlay_id != Some(view.id)) {
                                overlay_until = Some(
                                    Instant::now()
                                        + Duration::from_secs(if view.phase == Phase::Pasted {
                                            1
                                        } else {
                                            3
                                        }),
                                );
                            }
                            if !terminal {
                                overlay_until = None;
                            }
                            overlay_terminal = terminal;
                            overlay_id = Some(view.id);
                        }
                    }
                    if let Some(reason) = &snapshot.host.unavailable {
                        let _ = app.emit("input-unavailable", reason);
                    }
                }
            }
            Ok(())
        },
        move |error| {
            eprintln!("host stopped: {error}");
            if let Some(tray) = errors.tray_by_id("main") {
                let _ = tray.set_icon(Some(tray_icon(false)));
                let _ = tray.set_tooltip(Some(format!(
                    "{} · Host stopped: {error}",
                    env!("PRODUCT_NAME")
                )));
            }
            let _ = errors.emit("host-error", error);
        },
    )
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
