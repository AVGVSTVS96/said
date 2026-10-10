use std::time::Duration;

use voice_core::{EngineEvent, UtteranceId};
use voice_engine::{
    host::{WorkerConfig, recognize_file},
    manifest::{Manifest, model_cache},
    wav::Wav,
};

#[test]
#[ignore = "requires fetch-model.sh and synthetic engine-proof clips in the model cache"]
fn native_adapter_and_worker_preserve_identical_raw_transcripts() {
    let manifest = Manifest::bundled().unwrap();
    let cache = model_cache().unwrap();
    let model = manifest.default_model().unwrap();
    let config = WorkerConfig {
        executable: env!("CARGO_BIN_EXE_voice-engine").into(),
        args: vec!["--worker".into(), model.into()],
        startup_timeout: Duration::from_secs(60),
        audio_timeout: Duration::from_secs(10),
        finish_timeout: Duration::from_secs(30),
        queue_frames: 16,
    };
    for name in ["negation", "repair", "repeats", "silence"] {
        let path = cache.join("engine-proof").join(format!("{name}.wav"));
        let id = UtteranceId::random();
        let (mut engine, events) = manifest.create(id, model, &cache).unwrap();
        engine.start(&[]).unwrap();
        let mut wav = Wav::open(&path).unwrap();
        while let Some(samples) = wav.read().unwrap() {
            engine.feed(&samples);
        }
        engine.finish();
        let mut text = String::new();
        let mut final_count = 0;
        for output in events.try_iter() {
            assert_eq!(output.id, id);
            assert_eq!(final_count, 0, "event after Final");
            match output.event {
                EngineEvent::Committed(chunk) => text.push_str(&chunk),
                EngineEvent::Final => final_count += 1,
                EngineEvent::Partial(_) => {}
                EngineEvent::Error(error) => panic!("{error}"),
            }
        }
        assert_eq!(final_count, 1);
        drop(engine);
        let worker_text = recognize_file(UtteranceId::random(), &config, &path, &[]).unwrap();
        assert_eq!(worker_text.as_bytes(), text.as_bytes());
        if name == "silence" {
            assert!(text.is_empty());
        } else {
            assert!(!text.is_empty());
        }
        println!(
            "{}",
            serde_json::json!({"model":model, "clip":name, "text":text})
        );
    }
}
