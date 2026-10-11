use crossbeam_channel::Receiver;
use said_app::{
    runtime::{Host, HostPlatform},
    storage::Paths,
};
use said_audio::fake::FakeRecorder;
use said_core::{History, Hotkey, Mode, Result, Settings, Target, UiAction, UtteranceId};
use said_engine::host::{WorkerClient, WorkerConfig};
use said_platform::{
    Platform,
    fake::{FakePlatform, PlatformCall},
};
use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = PathBuf::from(std::env::var_os("HOME").unwrap())
            .join(".cache/said-models/host-tests")
            .join(UtteranceId::random().0.to_string());
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn host(&self) -> Host<TestPlatform, FakeRecorder> {
        let trace = self.0.join("trace.json");
        fs::write(
            &trace,
            serde_json::json!({"start":[], "feeds":[], "finish":[
                {"type":"committed", "text":"do delete the file"}, {"type":"final"}
            ]})
            .to_string(),
        )
        .unwrap();
        let client = WorkerClient::warm(WorkerConfig {
            executable: env!("CARGO_BIN_EXE_said").into(),
            args: vec![
                "--worker".into(),
                "fake".into(),
                "--trace".into(),
                trace.display().to_string(),
            ],
            startup_timeout: Duration::from_secs(5),
            audio_timeout: Duration::from_secs(5),
            finish_timeout: Duration::from_secs(5),
            queue_frames: 1,
        })
        .unwrap();
        let mut host = Host::open(
            TestPlatform {
                inner: FakePlatform::new(Some(Target {
                    app_id: "scratch".into(),
                    window_id: "1".into(),
                    name: "Scratch".into(),
                })),
                copies: RefCell::new(vec![]),
                instructions: RefCell::new(vec![]),
            },
            FakeRecorder::new(vec![(vec![1, 2], 0.2)], 1),
            Paths::new(self.0.clone(), self.0.join("config")),
        )
        .unwrap();
        host.set_client(client);
        host
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct TestPlatform {
    inner: FakePlatform,
    copies: RefCell<Vec<String>>,
    instructions: RefCell<Vec<String>>,
}
impl Platform for TestPlatform {
    type Window = ();
    fn hotkeys(&self) -> Receiver<Hotkey> {
        self.inner.hotkeys()
    }
    fn focused(&self) -> Option<Target> {
        self.inner.focused()
    }
    fn focus(&self, target: &Target) -> Result<()> {
        self.inner.focus(target)
    }
    fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        self.inner.paste(target, text, send)
    }
    fn make_overlay(&self, window: &()) {
        self.inner.make_overlay(window);
    }
}
impl HostPlatform for TestPlatform {
    fn instructions(&self) -> Vec<String> {
        self.instructions.borrow().clone()
    }
    fn copy(&self, text: &str) -> Result<()> {
        self.copies.borrow_mut().push(text.to_owned());
        Ok(())
    }
    fn configure(&mut self, _: &Settings) -> Result<()> {
        Ok(())
    }
}
fn complete(host: &mut Host<TestPlatform, FakeRecorder>, mode: Mode) -> UtteranceId {
    host.start(mode, false).unwrap();
    let id = host.session.views().last().unwrap().id;
    host.stop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        host.poll().unwrap();
        if host.history.get(id).unwrap().unwrap().raw_text.is_some() {
            return id;
        }
        assert!(
            Instant::now() < deadline,
            "synthetic recognition must finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn assert_paste(host: &Host<TestPlatform, FakeRecorder>, text: &str) {
    assert!(
        matches!(host.platform.inner.calls.borrow().last(), Some(PlatformCall::Paste {text: actual, send: false, ..}) if actual.as_bytes()==text.as_bytes())
    );
}

#[test]
fn completed_history_edits_change_negation_and_whitespace_in_every_delivery_path() {
    let directory = Directory::new();
    let mut host = directory.host();
    let edited = "  don't delete the file\t\n\n";
    let quick = complete(&mut host, Mode::Quick);
    host.ui(UiAction::Edit {
        id: quick,
        text: edited.into(),
    })
    .unwrap();
    assert_eq!(
        host.history
            .get(quick)
            .unwrap()
            .unwrap()
            .raw_text
            .as_deref(),
        Some("do delete the file")
    );
    assert_eq!(
        host.history.get(quick).unwrap().unwrap().text.as_deref(),
        Some(edited)
    );
    assert_eq!(
        host.session
            .views()
            .find(|v| v.id == quick)
            .unwrap()
            .edited
            .as_deref(),
        Some(edited)
    );
    host.ui(UiAction::Copy { id: quick }).unwrap();
    assert_eq!(host.platform.copies.borrow().last().unwrap(), edited);
    host.ui(UiAction::Insert { id: quick }).unwrap();
    assert_paste(&host, edited);
    host.ui(UiAction::InsertLast).unwrap();
    assert_paste(&host, edited);
    let newer = complete(&mut host, Mode::Quick);
    assert!(!host.session.views().any(|v| v.id == quick));
    let older_edit = "\n don't delete the old file  ";
    host.ui(UiAction::Edit {
        id: quick,
        text: older_edit.into(),
    })
    .unwrap();
    host.ui(UiAction::Insert { id: quick }).unwrap();
    assert_paste(&host, older_edit);
    host.ui(UiAction::Edit {
        id: newer,
        text: edited.into(),
    })
    .unwrap();
    host.ui(UiAction::InsertLast).unwrap();
    assert_paste(&host, edited);
    let interactive = complete(&mut host, Mode::Interactive);
    host.ui(UiAction::Edit {
        id: interactive,
        text: edited.into(),
    })
    .unwrap();
    host.ui(UiAction::Insert { id: interactive }).unwrap();
    assert_paste(&host, edited);
    host.ui(UiAction::Copy { id: interactive }).unwrap();
    assert_eq!(host.platform.copies.borrow().last().unwrap(), edited);
    assert_eq!(
        host.history
            .get(interactive)
            .unwrap()
            .unwrap()
            .raw_text
            .as_deref(),
        Some("do delete the file")
    );
}

#[test]
fn invalid_history_edits_reject_and_failed_saves_leave_retained_text_unchanged() {
    let directory = Directory::new();
    let mut host = directory.host();
    assert!(
        host.ui(UiAction::Edit {
            id: UtteranceId::random(),
            text: "unknown".into()
        })
        .is_err()
    );
    host.start(Mode::Quick, false).unwrap();
    let active = host.session.views().last().unwrap().id;
    assert!(
        host.ui(UiAction::Edit {
            id: active,
            text: "pending".into()
        })
        .is_err()
    );
    host.stop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while host
        .history
        .get(active)
        .unwrap()
        .unwrap()
        .raw_text
        .is_none()
    {
        host.poll().unwrap();
        assert!(Instant::now() < deadline);
    }
    let db = rusqlite::Connection::open(directory.0.join("history.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_edit BEFORE UPDATE OF text ON utterances BEGIN SELECT RAISE(ABORT, 'synthetic save failure'); END;").unwrap();
    assert!(
        host.ui(UiAction::Edit {
            id: active,
            text: "don't delete the file".into()
        })
        .is_err()
    );
    assert_eq!(
        host.history.get(active).unwrap().unwrap().text.as_deref(),
        Some("do delete the file")
    );
    assert!(
        host.session
            .views()
            .find(|v| v.id == active)
            .unwrap()
            .edited
            .is_none()
    );
}

#[test]
fn optional_disk_metrics_fail_without_stopping_capture_or_losing_the_last_value() {
    use std::os::unix::fs::PermissionsExt;
    let directory = Directory::new();
    let mut host = directory.host();
    let previous = host.snapshot().unwrap().host.disk_bytes;
    let denied = directory.0.join("statistics");
    fs::create_dir(&denied).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    let snapshot = host.snapshot().unwrap();
    assert_eq!(snapshot.host.disk_bytes, previous);
    assert!(
        snapshot
            .host
            .disk_error
            .as_deref()
            .unwrap()
            .contains("Permission denied")
    );
    let id = complete(&mut host, Mode::Quick);
    assert!(host.history.get(id).unwrap().unwrap().pasted_at.is_some());
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(host.snapshot().unwrap().host.disk_error.is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn expired_control_requests_cannot_start_or_toggle_a_recording() {
    use said_platform::linux::control::{ControlCommand, ControlRequest};
    let directory = Directory::new();
    let mut host = directory.host();
    let (reply, response) = crossbeam_channel::bounded(1);
    said_app::ipc::reply_control(
        ControlRequest {
            command: ControlCommand::Toggle,
            reply,
            deadline: Instant::now(),
        },
        |command| said_app::cli::control(&mut host, command),
    );
    assert!(response.recv().unwrap().unwrap_err().contains("expired"));
    assert_eq!(host.session.views().count(), 0);
    assert!(host.history.search("").unwrap().is_empty());
    let (reply, response) = crossbeam_channel::bounded(1);
    said_app::ipc::reply_control(
        ControlRequest {
            command: ControlCommand::Toggle,
            reply,
            deadline: Instant::now() + Duration::from_secs(3),
        },
        |command| said_app::cli::control(&mut host, command),
    );
    assert_eq!(response.recv().unwrap().unwrap(), "applied");
    assert_eq!(host.session.views().count(), 1);
    host.stop().unwrap();
}

#[test]
fn actor_startup_returns_while_initialization_waits_for_the_main_thread() {
    let (initialize, initialization) = crossbeam_channel::bounded(1);
    let (returned, connection) = crossbeam_channel::bounded(1);
    let (failed, failure) = crossbeam_channel::bounded(1);
    let starting = std::thread::spawn(move || {
        let actor = said_app::ipc::Connection::start(
            move |requests| {
                initialization.recv()?;
                assert!(matches!(requests.recv()?, said_app::ipc::Request::Shutdown));
                Ok(())
            },
            move |error| {
                failed.send(error).unwrap();
            },
        )
        .unwrap();
        returned.send(actor).unwrap();
    });
    let result = connection.recv_timeout(Duration::from_millis(200));
    initialize.send(()).unwrap();
    let actor = result.expect(
        "setup must receive the connection before main-thread-dependent initialization finishes",
    );
    actor.0.send(said_app::ipc::Request::Shutdown).unwrap();
    starting.join().unwrap();
    assert!(failure.recv_timeout(Duration::from_secs(1)).is_err());
}

#[test]
fn actor_initialization_failure_and_panic_both_publish_terminal_errors() {
    for panic in [false, true] {
        let (failed, failure) = crossbeam_channel::bounded(1);
        let actor = said_app::ipc::Connection::start(
            move |_| {
                assert!(!panic, "synthetic actor panic");
                Err("synthetic initialization failure".into())
            },
            move |error| {
                failed.send(error).unwrap();
            },
        )
        .unwrap();
        assert_eq!(
            failure.recv_timeout(Duration::from_secs(1)).unwrap(),
            if panic {
                "host actor panicked"
            } else {
                "synthetic initialization failure"
            }
        );
        assert!(actor.0.send(said_app::ipc::Request::Shutdown).is_err());
    }
}

#[test]
fn tray_recording_art_differs_from_the_idle_icon() {
    let idle = said_app::ipc::tray_icon(false);
    let recording = said_app::ipc::tray_icon(true);
    assert_eq!(
        (idle.width(), idle.height()),
        (recording.width(), recording.height())
    );
    assert_ne!(idle.rgba(), recording.rgba());
}

#[test]
fn snapshot_refreshes_capabilities_without_a_settings_or_session_change() {
    let directory = Directory::new();
    let mut host = directory.host();
    *host.platform.instructions.borrow_mut() = vec!["Shortcut registration pending".into()];
    assert_eq!(
        host.snapshot().unwrap().host.instructions,
        ["Shortcut registration pending"]
    );
    *host.platform.instructions.borrow_mut() = vec!["Shortcut release available".into()];
    assert_eq!(
        host.snapshot().unwrap().host.instructions,
        ["Shortcut release available"]
    );
}
