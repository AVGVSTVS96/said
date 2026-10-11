use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use said_core::{EngineEvent, Result, UtteranceId};
use said_engine::{
    Engine,
    host::{WorkerClient, WorkerConfig},
    wav::Wav,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: stream <worker> <wav> <model>".into());
    }
    let config = WorkerConfig {
        executable: PathBuf::from(&args[0]),
        args: vec!["--worker".into(), args[2].clone()],
        startup_timeout: Duration::from_secs(60),
        audio_timeout: Duration::from_secs(10),
        finish_timeout: Duration::from_secs(30),
        queue_frames: 256,
    };
    let client = WorkerClient::warm(config)?;
    let (mut engine, events) = client.engine(UtteranceId::random())?;
    let began = Instant::now();
    let receiver = thread::spawn(move || {
        let mut text = String::new();
        loop {
            let output = events
                .recv_timeout(Duration::from_secs(90))
                .expect("engine deadline");
            println!(
                "{}",
                serde_json::json!({"seconds": began.elapsed().as_secs_f64(), "event":output.event})
            );
            match output.event {
                EngineEvent::Committed(chunk) => text.push_str(&chunk),
                EngineEvent::Final => return text,
                EngineEvent::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
    });
    engine.start()?;
    let mut wav = Wav::open(&PathBuf::from(&args[1]))?;
    let mut samples = 0_u64;
    while let Some(block) = wav.read()? {
        samples += block.len() as u64;
        thread::sleep(
            (began + Duration::from_secs_f64(samples as f64 / 16000.0))
                .saturating_duration_since(Instant::now()),
        );
        engine.feed(&block);
    }
    engine.finish();
    println!(
        "{}",
        serde_json::json!({"text": receiver.join().map_err(|_| "receiver failed")?})
    );
    Ok(())
}
