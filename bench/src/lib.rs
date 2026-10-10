pub mod corpus;
mod resources;
pub mod runner;
pub mod score;

use std::path::PathBuf;
use voice_core::Result;
use voice_engine::{
    fake::{FakeEngine, Trace},
    manifest::{Manifest, model_cache, worker_main},
};

pub fn cli(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("--worker") {
        let model = args.get(1).ok_or("worker model missing")?;
        let flag = |name: &str| {
            args.windows(2)
                .find(|p| p[0] == name)
                .map(|p| p[1].as_str())
        };
        if let Some(path) = flag("--trace") {
            let trace: Trace = serde_json::from_reader(std::fs::File::open(path)?)?;
            return voice_engine::worker::run(
                std::io::stdin().lock(),
                std::io::stdout().lock(),
                |id| {
                    let (engine, output) = FakeEngine::new(id, trace.clone());
                    Ok((Box::new(engine), output))
                },
            );
        }
        let manifest = match flag("--model-manifest") {
            Some(path) => Manifest::read(std::path::Path::new(path))?,
            None => Manifest::bundled()?,
        };
        let cache = flag("--model-cache")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(model_cache)?;
        return worker_main(Some(model), &manifest, &cache);
    }
    if args.first().map(String::as_str) != Some("bench") {
        return Err("usage: voice bench <manifest.jsonl> [--engine NAME|--all] [--ci] [--audio-root DIR] [--output DIR] [--trace FILE] [--model-manifest FILE] [--model-cache DIR]".into());
    }
    runner::run(runner::Options::parse(&args[1..])?)
}
