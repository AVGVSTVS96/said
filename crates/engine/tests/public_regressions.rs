use std::path::PathBuf;
use std::time::Duration;

use voice_core::{EngineEvent, UtteranceId};
use voice_engine::{
    Engine,
    host::{WorkerClient, WorkerConfig},
    manifest::model_cache,
    wav::Wav,
};

fn recognize(client: &WorkerClient, path: PathBuf) -> (String, usize) {
    let (mut engine, output) = client.engine(UtteranceId::random()).unwrap();
    let mut text = String::new();
    let mut commits = 0;
    std::thread::scope(|scope| {
        let feeder = scope.spawn(move || {
            engine.start(&[]).unwrap();
            let mut wav = Wav::open(&path).unwrap();
            while let Some(samples) = wav.read().unwrap() {
                engine.feed(&samples);
                std::thread::sleep(Duration::from_millis(100));
            }
            engine.finish();
            engine
        });
        for event in output.iter() {
            match event.event {
                EngineEvent::Committed(chunk) => {
                    text.push_str(&chunk);
                    commits += 1;
                }
                EngineEvent::Final => break,
                EngineEvent::Error(error) => panic!("{error}"),
                EngineEvent::Partial(_) => {}
            }
        }
        feeder.join().unwrap();
    });
    (text, commits)
}

#[test]
#[ignore = "requires both patched native models and the public benchmark cache"]
fn quiet_meeting_speech_survives_previous_streams_and_long_dictation_finishes() {
    let cache = model_cache().unwrap();
    let bench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/clips");
    for model in ["moonshine-small-en", "moonshine-medium-en"] {
        let client = WorkerClient::warm(WorkerConfig {
            executable: env!("CARGO_BIN_EXE_voice-engine").into(),
            args: vec!["--worker".into(), model.into()],
            startup_timeout: Duration::from_secs(60),
            audio_timeout: Duration::from_secs(10),
            finish_timeout: Duration::from_secs(30),
            queue_frames: 256,
        })
        .unwrap();
        for name in [
            "dont-delete",
            "do-delete",
            "never-rewrite",
            "always-rewrite",
            "fillers-repeats",
            "repair",
            "identifiers",
            "paths-flags",
        ] {
            assert!(
                !recognize(&client, bench.join(format!("{name}.wav")))
                    .0
                    .is_empty()
            );
        }
        let first = recognize(&client, bench.join("ami-project.wav")).0;
        let second = recognize(&client, bench.join("ami-project.wav")).0;
        assert!(!first.is_empty());
        assert_eq!(
            first, second,
            "earlier utterances must not affect VAD or decoding"
        );
        let (text, commits) =
            recognize(&client, cache.join("bench-public/clips/long-monologue.wav"));
        assert!(
            commits >= 4,
            "a long recording must commit bounded speech segments"
        );
        let words: Vec<_> = text
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .collect();
        for anchor in ["parser", "summary", "worker", "false", "true"] {
            assert_eq!(
                words
                    .iter()
                    .filter(|word| word.eq_ignore_ascii_case(anchor))
                    .count(),
                4,
                "a repeated passage was lost or duplicated: {anchor}: {text}"
            );
        }
        println!("{model}: {commits} commits, {} words", words.len());
    }
}
