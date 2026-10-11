use std::path::PathBuf;
use std::time::Duration;

use said_core::{EngineEvent, UtteranceId};
use said_engine::{
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

struct AlignedTail(PathBuf);

impl AlignedTail {
    fn new(bench: &std::path::Path) -> Self {
        let mut wav = Wav::open(&bench.join("one-word.wav")).unwrap();
        let mut speech = Vec::new();
        while let Some(block) = wav.read().unwrap() {
            speech.extend(block);
        }
        let end = speech
            .iter()
            .rposition(|sample| sample.unsigned_abs() > 64)
            .unwrap();
        speech.truncate((end + 161).min(speech.len()));
        let mut samples = vec![0_i16; 8 * 16000 - speech.len()];
        samples.extend(speech);
        let data: Vec<_> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(16000_u32.to_le_bytes());
        bytes.extend(32000_u32.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        let directory = model_cache().unwrap().join("engine-tests");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{}.wav", UtteranceId::random().0));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for AlignedTail {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
#[ignore = "requires Parakeet and the public benchmark cache"]
fn quiet_meeting_speech_survives_previous_streams_and_long_dictation_finishes() {
    let cache = model_cache().unwrap();
    let bench = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/clips");
    for model in ["parakeet-v3-fp32"] {
        let client = WorkerClient::warm(WorkerConfig {
            executable: env!("CARGO_BIN_EXE_said-engine").into(),
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
        let tail = AlignedTail::new(&bench);
        let text = recognize(&client, tail.0.clone()).0;
        assert_eq!(
            text.split(|c: char| !c.is_alphabetic())
                .filter(|word| word.eq_ignore_ascii_case("stop"))
                .count(),
            1,
            "frame-aligned release must flush the spoken last word: {text}"
        );
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
