use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select};
use voice_core::{EngineEvent, EngineOutput, Result, UtteranceId, WorkerRequest, WorkerResponse};

use crate::{Engine, protocol, spool::Spool, watchdog::Watchdog, wav::Wav};

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub startup_timeout: Duration,
    pub audio_timeout: Duration,
    pub finish_timeout: Duration,
    pub queue_frames: usize,
}

impl WorkerConfig {
    fn validate(&self) -> Result<()> {
        if self.queue_frames == 0
            || self.queue_frames > 256
            || self.startup_timeout.is_zero()
            || self.audio_timeout.is_zero()
            || self.finish_timeout.is_zero()
        {
            return Err("worker queue and benchmark-derived deadlines must be positive".into());
        }
        Ok(())
    }
}

enum Input {
    Start(Vec<String>),
    AudioAvailable,
    Finish,
}

type IdleWorker = Arc<Mutex<Option<Transport>>>;

pub struct WorkerClient {
    config: WorkerConfig,
    idle: IdleWorker,
}

impl WorkerClient {
    pub fn warm(config: WorkerConfig) -> Result<Self> {
        config.validate()?;
        let worker = Transport::spawn(&config)?;
        let id = UtteranceId::random();
        worker.send(
            WorkerRequest::Start {
                id,
                vocabulary: vec![],
            },
            None,
        )?;
        worker.send(WorkerRequest::Audio { id, sequence: 0 }, Some(vec![]))?;
        let deadline = Instant::now() + config.startup_timeout;
        loop {
            match receive(&worker, deadline, None)? {
                WorkerResponse::AudioAck {
                    id: current,
                    sequence: 0,
                } if current == id => break,
                WorkerResponse::Event {
                    id: current,
                    event: EngineEvent::Partial(_),
                } if current == id => {}
                _ => return Err("unexpected response while warming worker".into()),
            }
        }
        worker.send(WorkerRequest::Cancel { id }, None)?;
        Ok(Self {
            config,
            idle: Arc::new(Mutex::new(Some(worker))),
        })
    }

    pub fn measured_real_time_factor(&self) -> Option<f64> {
        self.idle
            .lock()
            .ok()?
            .as_ref()
            .map(|worker| worker.watchdog.real_time_factor())
    }

    pub fn engine(&self, id: UtteranceId) -> Result<(WorkerEngine, Receiver<EngineOutput>)> {
        WorkerEngine::launch(id, self.config.clone(), Some(self.idle.clone()))
    }
}

pub struct WorkerEngine {
    input: Sender<Input>,
    failures: Sender<String>,
    spool: Arc<Spool>,
    started: bool,
    used: bool,
    cancel_signal: Sender<()>,
    cancelled: Arc<AtomicBool>,
}

impl WorkerEngine {
    pub fn new(id: UtteranceId, config: WorkerConfig) -> Result<(Self, Receiver<EngineOutput>)> {
        Self::launch(id, config, None)
    }

    fn launch(
        id: UtteranceId,
        config: WorkerConfig,
        idle: Option<IdleWorker>,
    ) -> Result<(Self, Receiver<EngineOutput>)> {
        config.validate()?;
        let (input, requests) = bounded(config.queue_frames);
        let (failures, errors) = bounded(1);
        let spool = Arc::new(Spool::new(id)?);
        let audio = spool.clone();
        let (events, output) = bounded(64);
        let (cancel_signal, cancellation) = bounded(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_flag = cancelled.clone();
        thread::Builder::new()
            .name("voice-engine".into())
            .spawn(move || {
                supervise(
                    id,
                    config,
                    requests,
                    errors,
                    events,
                    audio,
                    (cancellation, cancel_flag, idle),
                );
            })?;
        Ok((
            Self {
                input,
                failures,
                spool,
                started: false,
                used: false,
                cancel_signal,
                cancelled,
            },
            output,
        ))
    }
}

impl Engine for WorkerEngine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()> {
        if self.used {
            return Err("worker instances are scoped to one utterance".into());
        }
        self.input
            .try_send(Input::Start(vocabulary.to_vec()))
            .map_err(|_| "worker unavailable")?;
        self.started = true;
        self.used = true;
        Ok(())
    }
    fn feed(&mut self, pcm16k: &[i16]) {
        if !self.started || pcm16k.is_empty() {
            return;
        }
        for samples in pcm16k.chunks(1600) {
            if let Err(error) = self.spool.append(samples) {
                let _ = self.failures.try_send(format!(
                    "audio spool failed: {error}; capture remains on disk"
                ));
                return;
            }
        }
        let _ = self.input.try_send(Input::AudioAvailable);
    }
    fn finish(&mut self) {
        if self.started {
            let _ = self.input.send(Input::Finish);
            self.started = false;
        }
    }
    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.cancel_signal.try_send(());
        self.started = false;
    }
}

struct Transport {
    child: Child,
    input: Sender<(WorkerRequest, Option<Vec<i16>>)>,
    output: Receiver<std::result::Result<WorkerResponse, String>>,
    watchdog: Watchdog,
    inference_ready: bool,
}

impl Transport {
    fn spawn(config: &WorkerConfig) -> Result<Self> {
        let mut child = Command::new(&config.executable)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or("worker stdin unavailable")?;
        let mut stdout = child.stdout.take().ok_or("worker stdout unavailable")?;
        let (input, requests) =
            bounded::<(WorkerRequest, Option<Vec<i16>>)>(config.queue_frames + 2);
        let (responses, output) = bounded(64);
        let errors = responses.clone();
        thread::spawn(move || {
            for (message, pcm) in requests {
                if let Err(error) = protocol::write_request(&mut stdin, &message, pcm.as_deref()) {
                    let _ = errors.send(Err(error.to_string()));
                    break;
                }
            }
        });
        thread::spawn(move || {
            loop {
                let response = match protocol::read_response(&mut stdout) {
                    Ok(Some(response)) => Ok(response),
                    Ok(None) => Err("recognizer exited".into()),
                    Err(error) => Err(error.to_string()),
                };
                let failed = response.is_err();
                if responses.send(response).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            output,
            watchdog: Watchdog::default(),
            inference_ready: false,
        })
    }

    fn send(&self, request: WorkerRequest, pcm: Option<Vec<i16>>) -> Result<()> {
        self.input
            .try_send((request, pcm))
            .map_err(|error| match error {
                TrySendError::Full(_) => "worker input queue full",
                TrySendError::Disconnected(_) => "worker writer closed",
            })?;
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn emit(events: &Sender<EngineOutput>, id: UtteranceId, event: EngineEvent) -> bool {
    events.try_send(EngineOutput { id, event }).is_ok()
}

fn supervise(
    id: UtteranceId,
    config: WorkerConfig,
    requests: Receiver<Input>,
    failures: Receiver<String>,
    events: Sender<EngineOutput>,
    spool: Arc<Spool>,
    cancellation: (Receiver<()>, Arc<AtomicBool>, Option<IdleWorker>),
) {
    let (cancellation, cancelled, idle) = cancellation;
    let Ok(Input::Start(vocabulary)) = requests.recv() else {
        return;
    };
    let existing = idle.as_ref().and_then(|idle| idle.lock().ok()?.take());
    let existing_is_warm = existing.is_some();
    let mut transport = existing
        .map(Ok)
        .unwrap_or_else(|| Transport::spawn(&config))
        .and_then(|worker| {
            worker.send(
                WorkerRequest::Start {
                    id,
                    vocabulary: vocabulary.clone(),
                },
                None,
            )?;
            Ok(worker)
        });
    let mut pending = VecDeque::new();
    let mut sequence = 0;
    let mut finishing = None;
    let mut finish_requested = false;
    let mut offset = 0;
    let mut loaded = existing_is_warm;
    let mut progress = None;
    let mut uncommitted_samples = 0_u64;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = &transport {
            if !emit(
                &events,
                id,
                EngineEvent::Error(format!("Recognizer interrupted: {error}")),
            ) {
                return;
            }
            return;
        }
        let worker = transport.as_mut().unwrap();
        if pending.len() < config.queue_frames && !spool.consumed(offset) {
            match spool.read(&mut offset) {
                Ok(Some(pcm)) => {
                    if pending.is_empty() {
                        progress = Some(Instant::now());
                    }
                    uncommitted_samples += pcm.len() as u64;
                    pending.push_back((sequence, pcm.len() as u64));
                    if let Err(error) =
                        worker.send(WorkerRequest::Audio { id, sequence }, Some(pcm))
                    {
                        transport = Err(error);
                    }
                    sequence += 1;
                }
                Ok(None) => {}
                Err(error) => {
                    transport = Err(error);
                }
            }
            continue;
        }
        if finish_requested && finishing.is_none() && pending.is_empty() && spool.consumed(offset) {
            finishing = Some(());
            progress = Some(Instant::now());
            if let Err(error) = worker.send(WorkerRequest::Finish { id }, None) {
                transport = Err(error);
                continue;
            }
        }
        let timeout = if !loaded || !worker.inference_ready {
            config.startup_timeout
        } else if pending.is_empty() {
            worker
                .watchdog
                .budget(config.finish_timeout, uncommitted_samples)
        } else {
            worker
                .watchdog
                .budget(config.audio_timeout, uncommitted_samples)
        };
        let wait = progress
            .map(|since: Instant| timeout.saturating_sub(since.elapsed()))
            .unwrap_or(Duration::from_secs(3600));
        select! {
            recv(cancellation) -> _ => return,
            recv(failures) -> failure => {
                if let Ok(error) = failure { transport = Err(error.into()); }
                else { return; }
            }
            recv(requests) -> input => match input {
                Ok(Input::AudioAvailable) => {}
                Ok(Input::Finish) => { finish_requested = true; }
                Err(_) => { return; }
                Ok(Input::Start(_)) => { transport = Err("duplicate Start".into()); }
            },
            recv(worker.output) -> response => match response {
                Ok(Ok(WorkerResponse::AudioAck { id: current, sequence: ack })) => {
                    let completed = pending.pop_front();
                    if current != id || completed.map(|(sequence, _)| sequence) != Some(ack) {
                        transport = Err("worker acknowledgment mismatch".into());
                    } else {
                        if loaded && let Some(since) = progress {
                            worker.watchdog.observe(completed.unwrap().1, since.elapsed());
                        }
                        loaded = true;
                        progress = (!pending.is_empty() || finishing.is_some()).then(Instant::now);
                    }
                }
                Ok(Ok(WorkerResponse::Event { id: current, event })) => {
                    if current != id { transport = Err("worker event identity mismatch".into()); continue; }
                    if matches!(&event, EngineEvent::Partial(text) | EngineEvent::Committed(text) if !text.is_empty()) {
                        worker.inference_ready = true;
                    }
                    match &event {

                        EngineEvent::Error(error) => { transport = Err(error.clone().into()); continue; }
                        EngineEvent::Final => {
                            if finishing.is_none() || !pending.is_empty() { transport = Err("worker Final before all audio was acknowledged".into()); continue; }
                            if let (Some(idle), Ok(worker)) = (&idle, transport)
                                && let Ok(mut slot) = idle.lock()
                                && slot.is_none() {
                                *slot = Some(worker);
                            }
                            emit(&events, id, event);
                            return;
                        }
                        EngineEvent::Committed(_) => {
                            uncommitted_samples = pending.iter().map(|(_, samples)| samples).sum();
                            if finishing.is_some() && pending.is_empty() { progress = Some(Instant::now()); }
                        }
                        _ => {}
                    }
                    if !emit(&events, id, event) { transport = Err("host event queue overflow".into()); }
                }
                Ok(Err(error)) => { transport = Err(error.into()); }
                Err(_) => { transport = Err("worker reader closed".into()); }
            },
            default(wait) => {
                transport = Err(format!("worker made no acknowledgment/final progress for {:.3}s (measured RTF {:.3}, {} uncommitted samples)", timeout.as_secs_f64(), worker.watchdog.real_time_factor(), uncommitted_samples).into());
            }
        }
    }
}

pub fn recognize_file(
    id: UtteranceId,
    config: &WorkerConfig,
    path: &std::path::Path,
    vocabulary: &[String],
) -> Result<String> {
    config.validate()?;
    replay(id, config, path, vocabulary, None)
}

fn replay(
    id: UtteranceId,
    config: &WorkerConfig,
    path: &std::path::Path,
    vocabulary: &[String],
    cancellation: Option<&Receiver<()>>,
) -> Result<String> {
    let mut worker = Transport::spawn(config)?;
    worker.send(
        WorkerRequest::Start {
            id,
            vocabulary: vocabulary.to_vec(),
        },
        None,
    )?;
    let mut wav = Wav::open(path)?;
    let mut text = String::new();
    let mut sequence = 0;
    let mut first = true;
    let mut uncommitted_samples = 0;
    while let Some(samples) = wav.read()? {
        let sample_count = samples.len() as u64;
        let began = Instant::now();
        worker.send(WorkerRequest::Audio { id, sequence }, Some(samples))?;
        uncommitted_samples += sample_count;
        let deadline = Instant::now()
            + if first || !worker.inference_ready {
                config.startup_timeout
            } else {
                worker
                    .watchdog
                    .budget(config.audio_timeout, uncommitted_samples)
            };
        let cold = first;
        first = false;
        loop {
            let response = receive(&worker, deadline, cancellation)?;
            match response {
                WorkerResponse::AudioAck {
                    id: current,
                    sequence: ack,
                } if current == id && ack == sequence => {
                    if !cold {
                        worker.watchdog.observe(sample_count, began.elapsed());
                    }
                    break;
                }
                WorkerResponse::Event { id: current, event } if current == id => match event {
                    EngineEvent::Committed(chunk) => {
                        worker.inference_ready |= !chunk.is_empty();
                        text.push_str(&chunk);
                        uncommitted_samples = 0;
                    }
                    EngineEvent::Partial(text) => {
                        worker.inference_ready |= !text.is_empty();
                    }
                    EngineEvent::Error(error) => return Err(error.into()),
                    EngineEvent::Final => return Err("Final before Finish during replay".into()),
                },
                _ => return Err("replay acknowledgment or identity mismatch".into()),
            }
        }
        sequence += 1;
    }
    worker.send(WorkerRequest::Finish { id }, None)?;
    let deadline = Instant::now()
        + if first || !worker.inference_ready {
            config.startup_timeout
        } else {
            worker
                .watchdog
                .budget(config.finish_timeout, uncommitted_samples)
        };
    loop {
        match receive(&worker, deadline, cancellation)? {
            WorkerResponse::Event { id: current, event } if current == id => match event {
                EngineEvent::Committed(chunk) => text.push_str(&chunk),
                EngineEvent::Partial(_) => {}
                EngineEvent::Final => return Ok(text),
                EngineEvent::Error(error) => return Err(error.into()),
            },
            _ => return Err("unexpected response during replay finish".into()),
        }
    }
}

fn receive(
    worker: &Transport,
    deadline: Instant,
    cancellation: Option<&Receiver<()>>,
) -> Result<WorkerResponse> {
    let never = crossbeam_channel::never();
    let cancellation = cancellation.unwrap_or(&never);
    select! {
        recv(cancellation) -> _ => Err("recognition canceled".into()),
        recv(worker.output) -> response => response.map_err(|_| "worker reader closed")?.map_err(Into::into),
        default(deadline.saturating_duration_since(Instant::now())) => Err("worker deadline exceeded".into()),
    }
}
