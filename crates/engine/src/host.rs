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

use crate::{Engine, protocol, wav::Wav};

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
    Audio(Vec<i16>),
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

    pub fn engine(&self, id: UtteranceId) -> Result<(WorkerEngine, Receiver<EngineOutput>)> {
        WorkerEngine::launch(id, self.config.clone(), Some(self.idle.clone()))
    }
}

pub struct WorkerEngine {
    input: Sender<Input>,
    overflow: Sender<()>,
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
        let (overflow, failures) = bounded(1);
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
                    failures,
                    events,
                    (cancellation, cancel_flag, idle),
                );
            })?;
        Ok((
            Self {
                input,
                overflow,
                started: false,
                used: false,
                cancel_signal,
                cancelled,
            },
            output,
        ))
    }

    fn send(&self, input: Input) {
        if self.input.try_send(input).is_err() {
            let _ = self.overflow.try_send(());
        }
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
        if self.started {
            if pcm16k.is_empty() {
                self.send(Input::Audio(vec![]));
            }
            for chunk in pcm16k.chunks(1600) {
                self.send(Input::Audio(chunk.to_vec()));
            }
        }
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
        let (input, requests) = bounded::<(WorkerRequest, Option<Vec<i16>>)>(config.queue_frames);
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
    failures: Receiver<()>,
    events: Sender<EngineOutput>,
    cancellation: (Receiver<()>, Arc<AtomicBool>, Option<IdleWorker>),
) {
    let (cancellation, cancelled, idle) = cancellation;
    let Ok(Input::Start(vocabulary)) = requests.recv() else {
        return;
    };
    let existing = idle.as_ref().and_then(|idle| idle.lock().ok()?.take());
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
    let mut loaded = false;
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
        let worker = transport.as_ref().unwrap();
        let deadline = pending
            .front()
            .map(|(_, deadline)| *deadline)
            .into_iter()
            .chain(finishing)
            .min();
        let wait = deadline
            .map(|deadline: Instant| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        select! {
            recv(cancellation) -> _ => return,
            recv(failures) -> failure => {
                if failure.is_ok() { transport = Err("host input queue overflow; audio remains on disk".into()); }
                else { return; }
            }
            recv(requests) -> input => match input {
                Ok(Input::Audio(pcm)) => {
                    let timeout = if !loaded { config.startup_timeout } else { config.audio_timeout };
                    pending.push_back((sequence, Instant::now() + timeout));
                    if let Err(error) = worker.send(WorkerRequest::Audio { id, sequence }, Some(pcm)) { transport = Err(error); }
                    sequence += 1;
                }
                Ok(Input::Finish) => {
                    finishing = Some(Instant::now() + if !loaded { config.startup_timeout } else { config.finish_timeout });
                    if let Err(error) = worker.send(WorkerRequest::Finish { id }, None) { transport = Err(error); }
                }
                Err(_) => { return; }
                Ok(Input::Start(_)) => { transport = Err("duplicate Start".into()); }
            },
            recv(worker.output) -> response => match response {
                Ok(Ok(WorkerResponse::AudioAck { id: current, sequence: ack })) => {
                    loaded = true;
                    if current != id || pending.pop_front().map(|(sequence, _)| sequence) != Some(ack) {
                        transport = Err("worker acknowledgment mismatch".into());
                    } else if pending.is_empty() && finishing.is_some() {
                        finishing = Some(Instant::now() + config.finish_timeout);
                    }
                }
                Ok(Ok(WorkerResponse::Event { id: current, event })) => {
                    if current != id { transport = Err("worker event identity mismatch".into()); continue; }
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
                        _ => {}
                    }
                    if !emit(&events, id, event) { transport = Err("host event queue overflow".into()); }
                }
                Ok(Err(error)) => { transport = Err(error.into()); }
                Err(_) => { transport = Err("worker reader closed".into()); }
            },
            default(wait) => { transport = Err("worker acknowledgment or final deadline exceeded".into()); }
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
    let worker = Transport::spawn(config)?;
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
    while let Some(samples) = wav.read()? {
        worker.send(WorkerRequest::Audio { id, sequence }, Some(samples))?;
        let deadline = Instant::now()
            + if first {
                config.startup_timeout
            } else {
                config.audio_timeout
            };
        first = false;
        loop {
            let response = receive(&worker, deadline, cancellation)?;
            match response {
                WorkerResponse::AudioAck {
                    id: current,
                    sequence: ack,
                } if current == id && ack == sequence => break,
                WorkerResponse::Event { id: current, event } if current == id => match event {
                    EngineEvent::Committed(chunk) => text.push_str(&chunk),
                    EngineEvent::Partial(_) => {}
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
        + if first {
            config.startup_timeout
        } else {
            config.finish_timeout
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
