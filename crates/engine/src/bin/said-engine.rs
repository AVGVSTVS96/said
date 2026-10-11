use std::path::PathBuf;
use std::time::Duration;

use said_core::{Result, UtteranceId};
use said_engine::{
    fake::{FakeEngine, Trace},
    host::{WorkerConfig, recognize_file},
    manifest::{Manifest, model_cache, worker_main},
    worker,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--worker") {
        if args.get(1).map(String::as_str) == Some("--trace") {
            let trace: Trace = serde_json::from_reader(std::fs::File::open(
                args.get(2).ok_or("trace path missing")?,
            )?)?;
            return worker::run(std::io::stdin().lock(), std::io::stdout().lock(), |id| {
                let (engine, output) = FakeEngine::new(id, trace.clone());
                Ok((Box::new(engine), output))
            });
        }
        return worker_main(
            args.get(1).map(String::as_str),
            &Manifest::bundled()?,
            &model_cache()?,
        );
    }
    let path = args
        .first()
        .ok_or("usage: said-engine <16k-mono-i16.wav> [model], or --worker [model]")?;
    let model = args
        .get(1)
        .cloned()
        .unwrap_or(Manifest::bundled()?.default_model()?.into());
    let config = WorkerConfig {
        executable: std::env::current_exe()?,
        args: vec!["--worker".into(), model],
        startup_timeout: Duration::from_secs(60),
        audio_timeout: Duration::from_secs(10),
        finish_timeout: Duration::from_secs(30),
        queue_frames: 256,
    };
    println!(
        "{}",
        serde_json::json!({"text": recognize_file(UtteranceId::random(), &config, &PathBuf::from(path), &[])?})
    );
    Ok(())
}
