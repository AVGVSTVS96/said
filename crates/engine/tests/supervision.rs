use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use voice_core::{EngineEvent, UtteranceId};
use voice_engine::{
    Engine,
    fake::Trace,
    host::{WorkerConfig, WorkerEngine, recognize_file},
};

struct Fixture {
    directory: PathBuf,
    wav: PathBuf,
    trace: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = PathBuf::from(std::env::var_os("HOME").unwrap())
            .join(".cache/voice-models/engine-tests")
            .join(UtteranceId::random().0.to_string());
        fs::create_dir_all(&directory).unwrap();
        let wav = directory.join("synthetic.wav");
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(38_u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(16000_u32.to_le_bytes());
        bytes.extend(32000_u32.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(0_i16.to_le_bytes());
        fs::write(&wav, bytes).unwrap();
        let trace = directory.join("trace.json");
        fs::write(
            &trace,
            serde_json::to_vec(&Trace {
                finish: vec![
                    EngineEvent::Committed("no no  café\t\n".into()),
                    EngineEvent::Final,
                ],
                ..Trace::default()
            })
            .unwrap(),
        )
        .unwrap();
        Self {
            directory,
            wav,
            trace,
        }
    }
    fn config(&self) -> WorkerConfig {
        WorkerConfig {
            executable: PathBuf::from(env!("CARGO_BIN_EXE_voice-engine")),
            args: vec![
                "--worker".into(),
                "--trace".into(),
                self.trace.display().to_string(),
            ],
            startup_timeout: Duration::from_secs(2),
            audio_timeout: Duration::from_millis(250),
            finish_timeout: Duration::from_millis(250),
            queue_frames: 8,
        }
    }
    fn failing_once(&self, first: &str) -> WorkerConfig {
        let script = self.directory.join("worker.sh");
        fs::write(&script, format!("#!/bin/sh\nif mkdir '{}' 2>/dev/null; then\n{first}\nfi\nexec '{}' --worker --trace '{}'\n", self.directory.join("once").display(), env!("CARGO_BIN_EXE_voice-engine"), self.trace.display())).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        WorkerConfig {
            executable: script,
            args: vec![],
            ..self.config()
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn same_binary_worker_recognizes_file_without_changing_bytes() {
    let fixture = Fixture::new();
    assert_eq!(
        recognize_file(UtteranceId::random(), &fixture.config(), &fixture.wav, &[]).unwrap(),
        "no no  café\t\n"
    );
}

#[test]
fn crash_hang_and_bad_framing_restart_and_replay_durable_audio() {
    for first in ["exit 17", "exec sleep 10", "printf garbage; exit 0"] {
        let fixture = Fixture::new();
        let (mut engine, output) = WorkerEngine::new(
            UtteranceId::random(),
            fixture.failing_once(first),
            fixture.wav.clone(),
        )
        .unwrap();
        engine.start(&[]).unwrap();
        engine.feed(&[0]);
        engine.finish();
        let mut transcript = String::new();
        let mut failures = 0;
        loop {
            match output.recv_timeout(Duration::from_secs(5)).unwrap().event {
                EngineEvent::Error(_) => failures += 1,
                EngineEvent::Committed(chunk) => transcript.push_str(&chunk),
                EngineEvent::Final => break,
                _ => {}
            }
        }
        assert_eq!(failures, 1);
        assert_eq!(transcript, "no no  café\t\n");
    }
}

#[test]
fn hung_final_is_recovered_even_after_finish_was_sent() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.trace,
        serde_json::to_vec(&Trace::default()).unwrap(),
    )
    .unwrap();
    let recovered = fixture.directory.join("recovered.json");
    fs::write(
        &recovered,
        serde_json::to_vec(&Trace {
            finish: vec![EngineEvent::Committed("tail".into()), EngineEvent::Final],
            ..Trace::default()
        })
        .unwrap(),
    )
    .unwrap();
    let config = fixture.failing_once(&format!(
        "exec '{}' --worker --trace '{}'",
        env!("CARGO_BIN_EXE_voice-engine"),
        fixture.trace.display()
    ));
    let script = fs::read_to_string(&config.executable).unwrap().replace(
        &format!(
            "fi\nexec '{}' --worker --trace '{}'",
            env!("CARGO_BIN_EXE_voice-engine"),
            fixture.trace.display()
        ),
        &format!(
            "fi\nexec '{}' --worker --trace '{}'",
            env!("CARGO_BIN_EXE_voice-engine"),
            recovered.display()
        ),
    );
    fs::write(&config.executable, script).unwrap();
    let (mut engine, output) =
        WorkerEngine::new(UtteranceId::random(), config, fixture.wav.clone()).unwrap();
    engine.start(&[]).unwrap();
    engine.feed(&[0]);
    engine.finish();
    assert!(matches!(
        output.recv_timeout(Duration::from_secs(5)).unwrap().event,
        EngineEvent::Error(_)
    ));
    assert_eq!(
        output.recv_timeout(Duration::from_secs(5)).unwrap().event,
        EngineEvent::Committed("tail".into())
    );
    assert_eq!(
        output.recv_timeout(Duration::from_secs(5)).unwrap().event,
        EngineEvent::Final
    );
}

#[test]
fn cancellation_stops_a_hung_worker_without_late_usable_events() {
    let fixture = Fixture::new();
    let (mut engine, output) = WorkerEngine::new(
        UtteranceId::random(),
        fixture.failing_once("exec sleep 10"),
        fixture.wav.clone(),
    )
    .unwrap();
    engine.start(&[]).unwrap();
    engine.feed(&[0]);
    engine.cancel();
    assert!(engine.start(&[]).is_err());
    assert!(output.recv_timeout(Duration::from_secs(1)).is_err());
    assert_eq!(
        output.try_recv(),
        Err(crossbeam_channel::TryRecvError::Disconnected)
    );
}

#[test]
fn recovery_appends_the_uncommitted_suffix_without_deduping_repeats() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.trace,
        serde_json::to_vec(&Trace {
            feeds: std::collections::VecDeque::from([
                vec![EngineEvent::Committed("no ".into())],
                vec![EngineEvent::Error("synthetic crash".into())],
            ]),
            finish: vec![EngineEvent::Committed("no".into()), EngineEvent::Final],
            ..Trace::default()
        })
        .unwrap(),
    )
    .unwrap();
    let (mut engine, output) =
        WorkerEngine::new(UtteranceId::random(), fixture.config(), fixture.wav.clone()).unwrap();
    engine.start(&[]).unwrap();
    engine.feed(&[0]);
    engine.feed(&[0]);
    engine.finish();
    let mut text = String::new();
    loop {
        match output.recv_timeout(Duration::from_secs(5)).unwrap().event {
            EngineEvent::Committed(chunk) => text.push_str(&chunk),
            EngineEvent::Final => break,
            _ => {}
        }
    }
    assert_eq!(text, "no no");
}

#[test]
fn changed_recovery_prefix_fails_instead_of_rewriting_final_words() {
    let fixture = Fixture::new();
    let initial = fixture.directory.join("initial.json");
    fs::write(
        &initial,
        serde_json::to_vec(&Trace {
            feeds: std::collections::VecDeque::from([
                vec![EngineEvent::Committed("old ".into())],
                vec![EngineEvent::Error("synthetic crash".into())],
            ]),
            ..Trace::default()
        })
        .unwrap(),
    )
    .unwrap();
    let config = fixture.failing_once(&format!(
        "exec '{}' --worker --trace '{}'",
        env!("CARGO_BIN_EXE_voice-engine"),
        initial.display()
    ));
    let (mut engine, output) =
        WorkerEngine::new(UtteranceId::random(), config, fixture.wav.clone()).unwrap();
    engine.start(&[]).unwrap();
    engine.feed(&[0]);
    engine.feed(&[0]);
    engine.finish();
    let events: Vec<_> = output.iter().map(|output| output.event).collect();
    assert_eq!(events[0], EngineEvent::Committed("old ".into()));
    assert!(events.iter().any(|event| matches!(event, EngineEvent::Error(message) if message.contains("fresh utterance ID"))));
    assert!(!events.contains(&EngineEvent::Final));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, EngineEvent::Committed(_)))
            .count(),
        1
    );
}

#[test]
fn warm_client_reuses_the_same_worker_between_distinct_utterance_ids() {
    let fixture = Fixture::new();
    let script = fixture.directory.join("counted.sh");
    let starts = fixture.directory.join("starts");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nexec '{}' --worker --trace '{}'\n",
            starts.display(),
            env!("CARGO_BIN_EXE_voice-engine"),
            fixture.trace.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let client = voice_engine::host::WorkerClient::warm(WorkerConfig {
        executable: script,
        args: vec![],
        ..fixture.config()
    })
    .unwrap();
    for _ in 0..2 {
        let (mut engine, output) = client
            .engine(UtteranceId::random(), fixture.wav.clone())
            .unwrap();
        engine.start(&[]).unwrap();
        engine.feed(&[0]);
        engine.finish();
        assert_eq!(
            output.recv_timeout(Duration::from_secs(2)).unwrap().event,
            EngineEvent::Committed("no no  café\t\n".into())
        );
        assert_eq!(
            output.recv_timeout(Duration::from_secs(2)).unwrap().event,
            EngineEvent::Final
        );
    }
    assert_eq!(fs::read(starts).unwrap(), b"x");
}
