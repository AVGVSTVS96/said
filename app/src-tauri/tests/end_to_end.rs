use said_core::{History, UtteranceId, history::SqliteHistory};
use said_engine::{
    host::{WorkerConfig, recognize_file},
    manifest::{Manifest, model_cache},
};
use std::{fs, process::Command, time::Duration};

#[test]
#[ignore = "requires hash-verified Parakeet runtime/model and public synthetic engine-proof WAVs"]
fn parakeet_worker_host_history_and_paste_preserve_exact_bytes() {
    let cache = model_cache().unwrap();
    let root = cache
        .join("app-proof")
        .join(UtteranceId::random().0.to_string());
    fs::create_dir_all(&root).unwrap();
    let manifest = Manifest::bundled().unwrap();
    let model = manifest.default_model().unwrap();
    for clip in ["negation", "repair", "boundaries", "silence"] {
        let input = cache.join("engine-proof").join(format!("{clip}.wav"));
        let expected = recognize_file(
            UtteranceId::random(),
            &WorkerConfig {
                executable: env!("CARGO_BIN_EXE_said").into(),
                args: vec![
                    "--worker".into(),
                    model.into(),
                    "--models".into(),
                    cache.to_string_lossy().into(),
                ],
                startup_timeout: Duration::from_secs(60),
                audio_timeout: Duration::from_secs(10),
                finish_timeout: Duration::from_secs(30),
                queue_frames: 256,
            },
            &input,
            &[],
        )
        .unwrap();
        let data = root.join(clip);
        let result = Command::new(env!("CARGO_BIN_EXE_said"))
            .args([
                "--replay",
                input.to_str().unwrap(),
                "--data",
                data.to_str().unwrap(),
                "--models",
                cache.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let raw = receipt["utterance"]["raw_text"].as_str().unwrap();
        assert_eq!(
            raw.as_bytes(),
            expected.as_bytes(),
            "recognizer to host: {clip}"
        );
        let id: UtteranceId = serde_json::from_value(receipt["utterance"]["id"].clone()).unwrap();
        let history = SqliteHistory::open(&data).unwrap();
        let saved = history.get(id).unwrap().unwrap();
        assert_eq!(
            saved.raw_text.as_deref().unwrap().as_bytes(),
            expected.as_bytes()
        );
        assert_eq!(saved.text, saved.raw_text);
        let audio = history.audio_path(saved.audio_id);
        let (sender, requests) = crossbeam_channel::bounded(1);
        let audio_path = audio.clone();
        let service = std::thread::spawn(move || match requests.recv().unwrap() {
            said_app::ipc::Request::Audio(requested, reply) => {
                assert_eq!(requested, id);
                reply.send(Ok(audio_path)).unwrap();
            }
            _ => panic!("unexpected playback request"),
        });
        let request = tauri::http::Request::builder()
            .uri(format!("said-audio://localhost/{}", id.0))
            .header("Range", "bytes=0-79")
            .body(Vec::new())
            .unwrap();
        let response = said_app::ipc::audio(&said_app::ipc::Connection(sender), request);
        assert_eq!(response.status(), 206);
        assert_eq!(response.headers()["Content-Type"], "audio/wav");
        assert_eq!(response.body(), &fs::read(audio).unwrap()[..80]);
        service.join().unwrap();
        if clip == "silence" {
            assert!(expected.is_empty());
            assert_eq!(receipt["paste"].as_array().unwrap().len(), 0);
        } else {
            assert!(!expected.is_empty());
            assert_eq!(receipt["paste"].as_array().unwrap().len(), 1);
            assert_eq!(
                receipt["paste"][0]["text"].as_str().unwrap().as_bytes(),
                expected.as_bytes(),
                "host to platform: {clip}"
            );
            assert_eq!(receipt["paste"][0]["send"], false);
            assert!(saved.pasted_at.is_some());
        }
        println!(
            "{clip}: exact raw/history/paste bytes; {} bytes",
            expected.len()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stop_drains_final_frames_before_the_next_recording_starts() {
    use crossbeam_channel::{Receiver, Sender, bounded};
    use said_app::{runtime::Host, storage::Paths};
    use said_audio::{Recorder, wav::WavWriter};
    use said_core::{AudioEvent, AudioFrame, InputDevice, Mode, Result, Target};
    use said_engine::host::{WorkerClient, WorkerConfig};
    use said_platform::fake::{FakePlatform, PlatformCall};
    use std::time::{Duration, Instant};

    struct TailRecorder {
        directory: std::path::PathBuf,
        active: Option<(UtteranceId, WavWriter, Sender<AudioEvent>)>,
    }
    impl Recorder for TailRecorder {
        fn list_devices(&self) -> Result<Vec<InputDevice>> {
            Ok(vec![])
        }
        fn start(&mut self, id: UtteranceId, _: Option<&str>) -> Result<Receiver<AudioEvent>> {
            let mut writer = WavWriter::create(&self.directory.join(format!("{}.wav", id.0)))?;
            writer.append(&[11, 22])?;
            let (sender, receiver) = bounded(4);
            sender.send(AudioEvent::Frame(AudioFrame {
                id,
                pcm: vec![11, 22],
                level: 0.1,
            }))?;
            self.active = Some((id, writer, sender));
            Ok(receiver)
        }
        fn stop(&mut self) -> Result<()> {
            let (id, mut writer, sender) = self.active.take().ok_or("already stopped")?;
            writer.append(&[33, 44])?;
            writer.finish()?;
            sender.send(AudioEvent::Frame(AudioFrame {
                id,
                pcm: vec![33, 44],
                level: 0.2,
            }))?;
            sender.send(AudioEvent::Stopped { id, duration_ms: 1 })?;
            Ok(())
        }
    }
    let root = model_cache()
        .unwrap()
        .join("app-fake-proof")
        .join(UtteranceId::random().0.to_string());
    fs::create_dir_all(&root).unwrap();
    let text = "  no no, um don't change café.\n  ";
    let trace = root.join("trace.json");
    fs::write(&trace,serde_json::json!({"start":[],"feeds":[],"finish":[{"type":"committed","text":text},{"type":"final"}]}).to_string()).unwrap();
    let client = WorkerClient::warm(WorkerConfig {
        executable: env!("CARGO_BIN_EXE_said").into(),
        args: vec![
            "--worker".into(),
            "fake".into(),
            "--trace".into(),
            trace.to_string_lossy().into(),
            "--model-cache".into(),
            root.to_string_lossy().into(),
        ],
        startup_timeout: Duration::from_secs(5),
        audio_timeout: Duration::from_secs(5),
        finish_timeout: Duration::from_secs(5),
        queue_frames: 16,
    })
    .unwrap();
    let platform = FakePlatform::new(Some(Target {
        app_id: "scratch".into(),
        window_id: "1".into(),
        name: "Scratch".into(),
    }));
    let recorder = TailRecorder {
        directory: root.join("audio"),
        active: None,
    };
    let mut host = Host::open(
        platform,
        recorder,
        Paths::new(root.clone(), root.join("config")),
    )
    .unwrap();
    host.set_client(client);
    host.start(Mode::Quick, false).unwrap();
    host.stop().unwrap();
    host.start(Mode::Quick, false).unwrap();
    host.stop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        host.poll().unwrap();
        if host
            .history
            .search("")
            .unwrap()
            .iter()
            .filter(|row| row.pasted_at.is_some())
            .count()
            == 2
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "both finishing utterances must be delivered"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    for row in host.history.search("").unwrap() {
        assert_eq!(row.raw_text.as_deref(), Some(text));
        let mut wav =
            said_audio::wav::WavReader::open(&host.history.audio_path(row.audio_id)).unwrap();
        let mut samples = [0; 4];
        assert_eq!(wav.read(&mut samples).unwrap(), 4);
        assert_eq!(samples, [11, 22, 33, 44]);
    }
    assert_eq!(host.platform.calls.borrow().iter().filter(|call|matches!(call,PlatformCall::Paste {text:received,send:false,..} if received.as_bytes()==text.as_bytes())).count(),2);
    drop(host);
    fs::remove_dir_all(root).unwrap();
}
