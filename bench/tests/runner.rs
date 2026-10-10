use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use voice_bench::corpus;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/fixtures")
            .join(voice_core::UtteranceId::random().0.to_string());
        fs::create_dir_all(&path).unwrap();
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(676_u32.to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16_u32.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(16000_u32.to_le_bytes());
        wav.extend(32000_u32.to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(640_u32.to_le_bytes());
        wav.extend([0_u8; 640]);
        fs::write(path.join("audio.wav"), wav).unwrap();
        fs::write(path.join("trace.json"), json!({"start": [], "feeds": [], "finish": [
            {"type": "committed", "text": "  uh uh DON'T"}, {"type": "committed", "text": "   change it.\n"}, {"type": "final"}]}).to_string()).unwrap();
        Self(path)
    }
    fn manifest(&self, hash: &str) {
        fs::write(self.0.join("manifest.jsonl"), json!({"id":"external-synthetic", "audio": self.0.join("audio.wav"),
            "sha256":hash,"reference":"uh uh DON'T change it.", "tags": ["filler", "repetition", "negation"],
            "license":"CC0-1.0","source":"synthetic fixture", "reference_status":"synthetic"}).to_string()).unwrap();
    }
    fn run(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_voice"))
            .arg("bench")
            .arg(self.0.join("manifest.jsonl"))
            .args(["--engine", "fake", "--trace"])
            .arg(self.0.join("trace.json"))
            .arg("--model-cache")
            .arg(&self.0)
            .arg("--output")
            .arg(self.0.join("run"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn external_manifest_uses_engine_worker_and_preserves_committed_bytes() {
    let fixture = Fixture::new();
    fixture.manifest(&corpus::hash(&fixture.0.join("audio.wav")).unwrap());
    let result = fixture.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let records = fs::read_to_string(fixture.0.join("run/clips.jsonl")).unwrap();
    let output: serde_json::Value = serde_json::from_str(records.trim()).unwrap();
    assert_eq!(
        output["recognition"]["text"],
        "  uh uh DON'T   change it.\n"
    );
    assert_eq!(output["score"]["normalized"]["substitutions"], 0);
    assert_eq!(
        output["recognition"]["first_partial_ms"],
        serde_json::Value::Null
    );
    assert_eq!(
        output["recognition"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["event"]["type"] == "final")
            .count(),
        1
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(fixture.0.join("run/clips.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn wrong_audio_hash_stops_before_creating_results() {
    let fixture = Fixture::new();
    fixture.manifest(&"0".repeat(64));
    let result = fixture.run();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("audio hash mismatch"));
    assert!(!fixture.0.join("run").exists());
}
