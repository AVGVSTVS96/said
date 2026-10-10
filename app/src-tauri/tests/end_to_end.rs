use std::{fs, process::Command};
use voice_core::{EngineEvent, History, UtteranceId, history::SqliteHistory};
use voice_engine::{
    manifest::{Manifest, model_cache},
    wav::Wav,
};

#[test]
#[ignore = "requires hash-verified Moonshine runtime/model and public synthetic engine-proof WAVs"]
fn moonshine_worker_host_history_and_paste_preserve_exact_bytes() {
    let cache = model_cache().unwrap();
    let root = cache
        .join("app-proof")
        .join(UtteranceId::random().0.to_string());
    fs::create_dir_all(&root).unwrap();
    let manifest = Manifest::bundled().unwrap();
    let model = manifest.default_model().unwrap();
    for clip in ["negation", "repair", "boundaries", "silence"] {
        let input = cache.join("engine-proof").join(format!("{clip}.wav"));
        let (mut recognizer, output) = manifest
            .create(UtteranceId::random(), model, &cache)
            .unwrap();
        recognizer.start(&[]).unwrap();
        let mut wav = Wav::open(&input).unwrap();
        while let Some(samples) = wav.read().unwrap() {
            recognizer.feed(&samples);
        }
        recognizer.finish();
        let mut expected = String::new();
        let mut finals = 0;
        for event in output.try_iter() {
            match event.event {
                EngineEvent::Committed(text) => expected.push_str(&text),
                EngineEvent::Final => finals += 1,
                EngineEvent::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
        assert_eq!(finals, 1);
        drop(recognizer);
        let data = root.join(clip);
        let result = Command::new(env!("CARGO_BIN_EXE_voice"))
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
