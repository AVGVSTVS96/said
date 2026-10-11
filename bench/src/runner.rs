use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use said_core::{EngineEvent, Result, UtteranceId};
use said_engine::{
    Engine,
    host::{WorkerClient, WorkerConfig},
    manifest::{Manifest, model_cache},
    wav::Wav,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    corpus,
    resources::Memory,
    score::{self, Score, percentile},
};

pub struct Options {
    pub manifest: PathBuf,
    pub engines: Vec<String>,
    pub all: bool,
    pub ci: bool,
    pub unpaced: bool,
    pub audio_root: Option<PathBuf>,
    pub output: PathBuf,
    pub trace: Option<PathBuf>,
    pub model_manifest: Option<PathBuf>,
    pub model_cache: PathBuf,
    pub startup: Duration,
    pub audio: Duration,
    pub finish: Duration,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let mut options = Self {
            manifest: args.first().ok_or("manifest path missing")?.into(),
            engines: Vec::new(),
            all: false,
            ci: false,
            unpaced: false,
            audio_root: None,
            output: model_cache()?.join(format!("bench-runs/{stamp}-{}", std::process::id())),
            trace: None,
            model_manifest: None,
            model_cache: model_cache()?,
            startup: Duration::from_secs(60),
            audio: Duration::from_secs(10),
            finish: Duration::from_secs(30),
        };
        let mut args = args[1..].iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ci" => options.ci = true,
                "--unpaced" => options.unpaced = true,
                "--all" => options.all = true,
                "--engine" => options
                    .engines
                    .push(args.next().ok_or("engine missing")?.clone()),
                "--audio-root" => {
                    options.audio_root = Some(args.next().ok_or("audio root missing")?.into())
                }
                "--output" => {
                    options.output = args.next().ok_or("output directory missing")?.into()
                }
                "--trace" => {
                    options.trace = Some(fs::canonicalize(args.next().ok_or("trace missing")?)?)
                }
                "--model-manifest" => {
                    options.model_manifest = Some(fs::canonicalize(
                        args.next().ok_or("model manifest missing")?,
                    )?)
                }
                "--model-cache" => {
                    options.model_cache =
                        fs::canonicalize(args.next().ok_or("model cache missing")?)?
                }
                "--startup-seconds" | "--audio-seconds" | "--finish-seconds" => {
                    let seconds: u64 = args.next().ok_or("deadline missing")?.parse()?;
                    if seconds == 0 {
                        return Err("deadline must be positive".into());
                    }
                    let duration = Duration::from_secs(seconds);
                    match arg.as_str() {
                        "--startup-seconds" => options.startup = duration,
                        "--audio-seconds" => options.audio = duration,
                        _ => options.finish = duration,
                    }
                }
                _ => return Err(format!("unknown option: {arg}").into()),
            }
        }
        if options.all && !options.engines.is_empty() {
            return Err("choose --all or --engine".into());
        }
        Ok(options)
    }
}

#[derive(Serialize)]
struct Recognition {
    text: String,
    first_partial_ms: Option<f64>,
    first_text_ms: Option<f64>,
    release_to_final_ms: f64,
    replay_lag_ms: f64,
    duration_ms: f64,
    wall_ms: f64,
    events: Vec<Value>,
}

fn recognize(client: &WorkerClient, path: &Path, unpaced: bool) -> Result<Recognition> {
    let (mut engine, receiver) = client.engine(UtteranceId::random())?;
    let mut wav = Wav::open(path)?;
    let release = Arc::new(Mutex::new(None));
    let stopped = Arc::new(AtomicBool::new(false));
    let began = Instant::now();
    let mut result = Recognition {
        text: String::new(),
        first_partial_ms: None,
        first_text_ms: None,
        release_to_final_ms: 0.0,
        replay_lag_ms: 0.0,
        duration_ms: 0.0,
        wall_ms: 0.0,
        events: Vec::new(),
    };
    thread::scope(|scope| -> Result<()> {
        let release_signal = release.clone();
        let stop_signal = stopped.clone();
        let feeder = scope.spawn(move || -> Result<(said_engine::host::WorkerEngine, u64)> {
            engine.start()?;
            let mut samples = 0_u64;
            while let Some(block) = wav.read()? {
                if stop_signal.load(Ordering::Relaxed) {
                    engine.cancel();
                    return Err("replay stopped after engine failure".into());
                }
                samples += block.len() as u64;
                if !unpaced {
                    thread::sleep(
                        (began + Duration::from_secs_f64(samples as f64 / 16000.0))
                            .saturating_duration_since(Instant::now()),
                    );
                }
                engine.feed(&block);
            }
            *release_signal
                .lock()
                .map_err(|_| "release clock poisoned")? = Some(Instant::now());
            engine.finish();
            Ok((engine, samples))
        });
        let mut error = None;
        loop {
            let output = match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(output) => output,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    let released = *release.lock().map_err(|_| "release clock poisoned")?;
                    if feeder.is_finished() && released.is_none() {
                        error = Some("audio feeder failed before release".into());
                        break;
                    }
                    continue;
                }
                Err(failure) => {
                    error = Some(format!("worker event wait: {failure}"));
                    break;
                }
            };
            let elapsed = began.elapsed().as_secs_f64() * 1000.0;
            result
                .events
                .push(json!({"ms": elapsed, "event": &output.event}));
            match output.event {
                EngineEvent::Partial(ref chunk) | EngineEvent::Committed(ref chunk)
                    if !chunk.is_empty() =>
                {
                    result.first_text_ms.get_or_insert(elapsed);
                    if matches!(output.event, EngineEvent::Partial(_)) {
                        result.first_partial_ms.get_or_insert(elapsed);
                    } else {
                        result.text.push_str(chunk);
                    }
                }
                EngineEvent::Committed(chunk) => result.text.push_str(&chunk),
                EngineEvent::Partial(_) => {}
                EngineEvent::Final => {
                    let released = release
                        .lock()
                        .map_err(|_| "release clock poisoned")?
                        .ok_or("Final before release")?;
                    result.release_to_final_ms = released.elapsed().as_secs_f64() * 1000.0;
                    break;
                }
                EngineEvent::Error(message) => {
                    error = Some(message);
                    break;
                }
            }
        }
        if error.is_some() {
            stopped.store(true, Ordering::Relaxed);
        }
        let feeding = feeder.join().map_err(|_| "audio replay thread failed")?;
        if let Some(error) = error {
            return Err(error.into());
        }
        let (_engine, samples) = feeding?;
        result.duration_ms = samples as f64 / 16.0;
        result.wall_ms = began.elapsed().as_secs_f64() * 1000.0;
        if let Some(released) = *release.lock().map_err(|_| "release clock poisoned")? {
            result.replay_lag_ms = (released.duration_since(began).as_secs_f64() * 1000.0
                - result.duration_ms)
                .max(0.0);
        }
        Ok(())
    })?;
    Ok(result)
}

fn summary(scores: &[Score], timings: &[Recognition]) -> Value {
    let mut total = Score::default();
    for score in scores {
        total.literal.add(&score.literal);
        total.normalized.add(&score.normalized);
        total.negation.add(&score.negation);
        total.filler.add(&score.filler);
        total.repetition.add(&score.repetition);
        total.technical.add(&score.technical);
        total.identifiers.add(&score.identifiers);
        total.negations_dropped += score.negations_dropped;
        total.negations_added += score.negations_added;
        total.negations_changed += score.negations_changed;
        total.hallucinated_words += score.hallucinated_words;
    }
    let first: Vec<_> = timings.iter().filter_map(|t| t.first_partial_ms).collect();
    let text: Vec<_> = timings.iter().filter_map(|t| t.first_text_ms).collect();
    let release: Vec<_> = timings.iter().map(|t| t.release_to_final_ms).collect();
    let mut value = serde_json::to_value(&total).unwrap();
    value["literal_wer"] = json!(total.literal.wer());
    value["normalized_wer"] = json!(total.normalized.wer());
    for (name, retention) in [
        ("negation", &total.negation),
        ("filler", &total.filler),
        ("repetition", &total.repetition),
        ("technical", &total.technical),
        ("identifiers", &total.identifiers),
    ] {
        value[name]["recall"] = json!(retention.recall());
        value[name]["precision"] = json!(retention.precision());
    }
    let release_budget: Vec<_> = timings
        .iter()
        .filter(|t| (5000.0..=20000.0).contains(&t.duration_ms))
        .map(|t| t.release_to_final_ms)
        .collect();
    value["latency"] = json!({
        "release_5_to_20s_samples": release_budget.len(),
        "release_5_to_20s_p50_ms": percentile(&release_budget, 0.5),
        "release_5_to_20s_p95_ms": percentile(&release_budget, 0.95),
        "first_partial_samples": first.len(), "first_partial_missing": timings.len() - first.len(),
        "first_partial_p50_ms": percentile(&first, 0.5), "first_partial_p95_ms": percentile(&first, 0.95),
        "first_text_p50_ms": percentile(&text, 0.5), "first_text_p95_ms": percentile(&text, 0.95),
        "release_to_final_p50_ms": percentile(&release, 0.5), "release_to_final_p95_ms": percentile(&release, 0.95),
        "max_replay_lag_ms": timings.iter().map(|t| t.replay_lag_ms).reduce(f64::max),
        "audio_seconds": timings.iter().map(|t| t.duration_ms / 1000.0).sum::<f64>(),
    });
    value
}

fn create_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub fn run(mut options: Options) -> Result<()> {
    let manifest = match &options.model_manifest {
        Some(path) => Manifest::read(path)?,
        None => Manifest::bundled()?,
    };
    if options.all {
        options.engines = manifest.models.keys().cloned().collect();
    }
    if options.engines.is_empty() {
        options.engines.push(manifest.default_model()?.into());
    }
    if options.trace.is_some() && options.engines.iter().any(|model| model != "fake") {
        return Err(
            "--trace requires --engine fake; synthetic output cannot be labeled as a real engine"
                .into(),
        );
    }
    let clips = corpus::read(&options.manifest, options.ci)?;
    corpus::verify(&clips, &options.manifest, options.audio_root.as_deref())?;
    if options.output.exists() {
        return Err("output directory already exists; choose a new run directory".into());
    }
    fs::create_dir_all(&options.output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&options.output, fs::Permissions::from_mode(0o700))?;
    }
    let mut records = create_file(&options.output.join("clips.jsonl"))?;
    let mut report = json!({
        "schema": 1, "started_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        "corpus_sha256": corpus::hash(&options.manifest)?, "clips": clips.len(),
        "mode": if options.unpaced { "unpaced" } else { "real_time" },
        "ci": options.ci, "postprocessing": "none",
        "requested_engines": options.engines,
        "model_manifest_sha256": options.model_manifest.as_ref().map(|p| corpus::hash(p)).transpose()?.unwrap_or_else(|| format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(include_bytes!("../../crates/engine/models.json")))),
        "build": {"rust": std::process::Command::new("rustc").arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()),
        "dependency_lock_sha256": format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(include_bytes!("../../Cargo.lock")))},
        "host": host(), "rss_method": "20ms sampled parent plus direct engine child resident set; shared pages may be counted twice",
        "engine_crate": env!("CARGO_PKG_VERSION"), "runtime_version": manifest.runtime.version,
        "binary_sha256": corpus::hash(&std::env::current_exe()?)?,
        "watchdog": {"policy": "acknowledgment progress; max(seed floor, 4 * max(recent median busy RTF * min(uncommitted seconds, 30), recent peak busy RTF * min(uncommitted seconds, 0.5)))"},
        "worker_deadlines_seconds": {"startup": options.startup.as_secs(), "audio": options.audio.as_secs(), "finish": options.finish.as_secs()},
        "reference_statuses": clips.iter().fold(BTreeMap::new(), |mut counts, c| { *counts.entry(&c.reference_status).or_insert(0) += 1; counts }),
        "engines": {},
    });
    let mut failed = false;
    for model in &options.engines {
        if !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err("invalid model name".into());
        }
        let mut pins = verify_model(
            &manifest,
            model,
            &options.model_cache,
            options.trace.is_some(),
        )?;
        if let Some(path) = &options.trace {
            pins["trace_sha256"] = json!(corpus::hash(path)?);
        }
        let mut args = vec![
            "--worker".into(),
            model.clone(),
            "--model-cache".into(),
            options.model_cache.to_string_lossy().into(),
        ];
        if let Some(path) = &options.model_manifest {
            args.extend(["--model-manifest".into(), path.to_string_lossy().into()]);
        }
        if let Some(path) = &options.trace {
            args.extend(["--trace".into(), path.to_string_lossy().into()]);
        }
        let config = WorkerConfig {
            executable: std::env::current_exe()?,
            args,
            startup_timeout: options.startup,
            audio_timeout: options.audio,
            finish_timeout: options.finish,
            queue_frames: 256,
        };
        let memory = Memory::start();
        let began = Instant::now();
        let client = WorkerClient::warm(config);
        let cold_ms = began.elapsed().as_secs_f64() * 1000.0;
        let mut measured_rtf = None;
        let mut scores = Vec::new();
        let mut timings = Vec::new();
        let mut errors = 0;
        let mut tags = Vec::new();
        if let Ok(client) = client {
            for clip in &clips {
                eprintln!("{model}: {}", clip.id);
                let path = clip.path(&options.manifest, options.audio_root.as_deref());
                match recognize(&client, &path, options.unpaced) {
                    Ok(recognition) => {
                        let a = score::normalized_tokens(&clip.reference)
                            .len()
                            .max(clip.reference.split_whitespace().count());
                        let b = score::normalized_tokens(&recognition.text)
                            .len()
                            .max(recognition.text.split_whitespace().count());
                        if (a + 1)
                            .checked_mul(b + 1)
                            .is_none_or(|cells| cells > 64_000_000)
                        {
                            return Err("alignment exceeds 64 million cells; split the reference into auditable clips".into());
                        }
                        let scored = score::score(
                            &clip.reference,
                            &recognition.text,
                            &clip.terms,
                            clip.non_speech(),
                        );
                        writeln!(
                            records,
                            "{}",
                            json!({"engine": model, "id": clip.id, "audio_sha256": clip.sha256,
                            "reference": clip.reference, "reference_status": clip.reference_status, "tags": clip.tags,
                            "pair": clip.pair, "polarity": clip.polarity, "status": "ok", "recognition": recognition, "score": scored})
                        )?;
                        tags.push(clip.tags.clone());
                        scores.push(scored);
                        timings.push(recognition);
                    }
                    Err(error) => {
                        errors += 1;
                        scores.push(score::score(
                            &clip.reference,
                            "",
                            &clip.terms,
                            clip.non_speech(),
                        ));
                        tags.push(clip.tags.clone());
                        writeln!(
                            records,
                            "{}",
                            json!({"engine": model, "id": clip.id, "status": "error", "error": error.to_string()})
                        )?;
                    }
                }
                records.flush()?;
            }
            measured_rtf = client.measured_real_time_factor();
        } else if let Err(error) = client {
            errors = clips.len();
            for clip in &clips {
                scores.push(score::score(
                    &clip.reference,
                    "",
                    &clip.terms,
                    clip.non_speech(),
                ));
                tags.push(clip.tags.clone());
            }
            writeln!(
                records,
                "{}",
                json!({"engine": model, "status": "load_error", "error": error.to_string()})
            )?;
        }
        let mut result = summary(&scores, &timings);
        let mut strata = serde_json::Map::new();
        for tag in [
            "natural",
            "negation",
            "filler",
            "repetition",
            "identifier",
            "path",
            "flag",
            "technical",
            "long",
            "silence",
            "noise",
        ] {
            let chosen: Vec<_> = tags
                .iter()
                .enumerate()
                .filter(|(_, tags)| tags.iter().any(|t| t == tag))
                .map(|(i, _)| i)
                .collect();
            strata.insert(tag.into(), json!({"clips": chosen.len(),
                "normalized_errors": chosen.iter().map(|&i| scores[i].normalized.substitutions + scores[i].normalized.deletions + scores[i].normalized.insertions).sum::<usize>(),
                "reference_words": chosen.iter().map(|&i| scores[i].normalized.reference_words).sum::<usize>(),
                "negations_dropped": chosen.iter().map(|&i| scores[i].negations_dropped).sum::<usize>(),
                "negations_added": chosen.iter().map(|&i| scores[i].negations_added).sum::<usize>()}));
        }
        result["strata"] = Value::Object(strata);
        result["cold_load_ms"] = json!(cold_ms);
        result["measured_busy_real_time_factor"] = json!(measured_rtf);
        result["peak_rss_bytes"] = json!(memory.finish());
        result["successful_clips"] = json!(timings.len());
        result["failed_clips"] = json!(errors);
        result["pins"] = pins;
        report["engines"][model] = result;
        failed |= errors > 0;
        write_report(&options.output, &report)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    eprintln!("Results: {}", options.output.display());
    if failed {
        return Err("benchmark has failed clips; results are incomplete".into());
    }
    Ok(())
}

fn write_report(output: &Path, report: &Value) -> Result<()> {
    let temp = output.join("summary.pending");
    let mut file = create_file(&temp)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temp, output.join("summary.json"))?;
    Ok(())
}

fn verify_model(manifest: &Manifest, model: &str, cache: &Path, trace: bool) -> Result<Value> {
    if model == "fake" && trace {
        return Ok(json!({"adapter": "fake", "ranking_candidate": false}));
    }
    let config = manifest
        .models
        .get(model)
        .ok_or("engine absent from model manifest")?;
    let mut files = BTreeMap::new();
    for (name, expected) in &config.files {
        let actual = corpus::hash(&cache.join(&config.directory).join(name))?;
        if actual != *expected {
            return Err(format!("{model}: model hash mismatch for {name}").into());
        }
        files.insert(name, actual);
    }
    let runtime = &manifest.runtime;
    let library_hash = runtime
        .libraries
        .get(std::env::consts::OS)
        .map(|library| corpus::hash(&cache.join(library)))
        .transpose()?;
    let mut runtime_files = BTreeMap::new();
    if let Some(library) = runtime.libraries.get(std::env::consts::OS) {
        let directory = cache
            .join(library)
            .parent()
            .ok_or("runtime directory missing")?
            .to_owned();
        for file in fs::read_dir(directory)? {
            let file = file?;
            let name = file.file_name().to_string_lossy().into_owned();
            if file.path().is_file() && (name.contains(".so") || name.ends_with(".dylib")) {
                runtime_files.insert(name, corpus::hash(&file.path())?);
            }
        }
    }
    Ok(
        json!({"adapter": config.adapter, "ranking_candidate": config.adapter != "fake", "runtime_files_sha256": runtime_files, "runtime_version": runtime.version, "runtime_source_revision": runtime.source.revision, "runtime_source_sha256": runtime.source.sha256, "threads": config.threads, "provider": config.provider, "chunk_seconds": if config.adapter == "parakeet" { Some(30) } else { None }, "overlap_seconds": if config.adapter == "parakeet" { Some(5) } else { None }, "update_samples": config.update_samples,
        "source": config.source, "license": config.license, "files": files, "runtime_library_sha256": library_hash}),
    )
}

fn host() -> Value {
    let cpu = if cfg!(target_os = "linux") {
        fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .map(|l| l.split_once(':').unwrap().1.trim().to_owned())
        })
    } else {
        std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    json!({"os": std::env::consts::OS, "architecture": std::env::consts::ARCH, "cpu": cpu,
        "logical_cpus": thread::available_parallelism().ok().map(|n| n.get()),
        "kernel": std::process::Command::new("uname").arg("-r").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())})
}
