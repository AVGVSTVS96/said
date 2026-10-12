use crate::storage::{Paths, atomic_write, disk_bytes};
use crossbeam_channel::{Receiver, Sender, bounded};
use said_audio::Recorder;
use said_core::{history::SqliteHistory, *};
use said_engine::{
    Engine,
    host::{WorkerClient, WorkerConfig, WorkerEngine, recognize_file_with_cancel},
    manifest::Manifest,
};
use said_platform::Platform;
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub trait HostPlatform: Platform {
    fn copy(&self, text: &str) -> Result<()>;
    fn is_self(&self, _target: &Target) -> bool {
        false
    }
    fn configure(&mut self, settings: &Settings) -> Result<()>;
    fn recording(&mut self, _active: bool) {}
    fn availability(&mut self, _available: bool) {}
    fn poll(&mut self) -> Vec<SessionEvent> {
        vec![]
    }
    /// Rechecks grants and starts whatever they now allow; true when published status changed.
    fn refresh(&mut self) -> bool {
        false
    }
    fn release_observable(&self) -> bool {
        false
    }
    fn permissions(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    fn instructions(&self) -> Vec<String> {
        vec![]
    }
}
impl HostPlatform for said_platform::fake::FakePlatform {
    fn copy(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn configure(&mut self, _: &Settings) -> Result<()> {
        Ok(())
    }
}

pub enum Background {
    Download(DownloadState),
    Ready(Result<WorkerClient>),
    Replay {
        id: UtteranceId,
        result: Result<(u64, String)>,
    },
}
struct ActiveEngine {
    engine: WorkerEngine,
    events: Receiver<EngineOutput>,
}

pub struct Host<P: HostPlatform, R: Recorder> {
    pub platform: P,
    pub history: SqliteHistory,
    pub session: Session,
    pub settings: Settings,
    pub paths: Paths,
    pub state: HostState,
    recorder: R,
    capture: Option<(UtteranceId, Receiver<AudioEvent>)>,
    engines: BTreeMap<UtteranceId, ActiveEngine>,
    client: Option<WorkerClient>,
    model_idle_timeout: Duration,
    background: Receiver<Background>,
    sender: Sender<Background>,
    clock: Instant,
    sequence: u64,
    query: String,
    external: Option<Target>,
    replaying: BTreeMap<UtteranceId, Sender<()>>,
    _lock: fs::File,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn worker_config(models: &Path, model: &str) -> Result<WorkerConfig> {
    Ok(WorkerConfig {
        executable: std::env::current_exe()?,
        args: vec![
            "--worker".into(),
            model.into(),
            "--models".into(),
            models.to_string_lossy().into(),
        ],
        startup_timeout: Duration::from_secs(60),
        audio_timeout: Duration::from_secs(10),
        finish_timeout: Duration::from_secs(30),
        queue_frames: 256,
    })
}

impl<P: HostPlatform, R: Recorder> Drop for Host<P, R> {
    fn drop(&mut self) {
        if self.capture.take().is_some() {
            let _ = self.recorder.stop();
            self.platform.recording(false);
        }
        for active in self.engines.values_mut() {
            active.engine.cancel();
        }
        for cancellation in self.replaying.values() {
            let _ = cancellation.try_send(());
        }
    }
}
impl<P: HostPlatform, R: Recorder> Host<P, R> {
    pub fn open(mut platform: P, recorder: R, paths: Paths) -> Result<Self> {
        let lock = paths.lock()?;
        let manifest = Manifest::bundled()?;
        let settings = Settings::load(
            &paths.config.join("settings.toml"),
            manifest.default_model()?,
        )?;
        platform.configure(&settings)?;
        let permissions = platform.permissions();
        let instructions = platform.instructions();
        let external = platform
            .focused()
            .filter(|target| !platform.is_self(target));
        let (sender, background) = bounded(64);
        let state = HostState {
            models: manifest
                .models
                .iter()
                .map(|(id, model)| ModelInfo {
                    id: id.clone(),
                    license: model.license.clone(),
                    source: model.source.clone(),
                })
                .collect(),
            download: DownloadState {
                state: "missing".into(),
                file: String::new(),
                received: 0,
                total: None,
                error: None,
            },
            permissions,
            instructions,
            notifications: BTreeMap::new(),
            disk_bytes: 0,
            disk_error: None,
            unavailable: None,
            scratch: None,
            setup_complete: paths.config.join("setup-complete").is_file(),
        };
        let history = SqliteHistory::open(&paths.data)?;
        Ok(Self {
            platform,
            history,
            settings,
            paths,
            state,
            recorder,
            capture: None,
            engines: BTreeMap::new(),
            client: None,
            model_idle_timeout: Duration::from_secs(
                std::env::var("SAID_MODEL_IDLE_SECONDS")
                    .ok()
                    .map(|value| value.parse::<u64>())
                    .transpose()?
                    .unwrap_or(300),
            ),
            background,
            sender,
            clock: Instant::now(),
            sequence: 0,
            query: String::new(),
            external,
            replaying: Default::default(),
            _lock: lock,
            session: Session::default(),
        })
    }
    pub fn install(&mut self) -> Result<()> {
        if matches!(
            self.state.download.state.as_str(),
            "downloading" | "loading"
        ) {
            return Ok(());
        }
        if self.capture.is_some() || !self.engines.is_empty() || !self.replaying.is_empty() {
            return Err("finish recognition before verifying the model".into());
        }
        self.client = None;
        self.state.download.state = "downloading".into();
        self.state.download.error = None;
        let sender = self.sender.clone();
        let models = self.paths.models.clone();
        let model = self.settings.engine.clone();
        std::thread::spawn(move || {
            let result = (|| {
                crate::download::install(&Manifest::bundled()?, &model, &models, |progress| {
                    let _ = sender.try_send(Background::Download(progress));
                })?;
                let _ = sender.send(Background::Download(DownloadState {
                    state: "loading".into(),
                    file: String::new(),
                    received: 0,
                    total: None,
                    error: None,
                }));
                WorkerClient::warm(worker_config(&models, &model)?)
            })();
            let _ = sender.send(Background::Ready(result));
        });
        Ok(())
    }
    pub fn set_client(&mut self, client: WorkerClient) {
        self.client = Some(client);
        self.state.download.state = "ready".into();
        if let Ok(manifest) = Manifest::bundled()
            && let Some(model) = manifest.models.get(&self.settings.engine)
        {
            let size = model
                .files
                .keys()
                .filter_map(|name| {
                    fs::metadata(self.paths.models.join(&model.directory).join(name)).ok()
                })
                .map(|file| file.len())
                .sum();
            self.state.download.file = self.settings.engine.clone();
            self.state.download.received = size;
            self.state.download.total = Some(size);
        }
    }
    pub fn recover(&mut self) -> Result<()> {
        for utterance in self.history.recover()? {
            self.event(SessionEvent::Recover { utterance })?;
        }
        Ok(())
    }
    fn context(&self, mode: Mode, scratch: bool) -> StartContext {
        let target = if scratch {
            None
        } else {
            self.platform.focused()
        };
        let send = target
            .as_ref()
            .and_then(|t| self.settings.apps.get(&t.app_id))
            .is_some_and(|rule| rule.send);
        StartContext {
            id: UtteranceId::random(),
            started_at: now(),
            engine: self.settings.engine.clone(),
            target,
            mode,
            send,
        }
    }
    pub fn start(&mut self, mode: Mode, scratch: bool) -> Result<()> {
        if let Some(reason) = self.session.unavailable_reason() {
            return Err(reason.into());
        }
        if self.client.is_none() {
            return Err("download and load the model before recording".into());
        }
        if self.capture.is_some() {
            return Err("a recording is already active".into());
        }
        if mode == Mode::Interactive {
            let ids: Vec<_> = self
                .session
                .views()
                .filter(|view| {
                    view.mode == Mode::Interactive
                        && !matches!(
                            view.phase,
                            Phase::Recording | Phase::Finishing | Phase::Saving | Phase::Pasting
                        )
                })
                .map(|view| view.id)
                .collect();
            for id in ids {
                self.session.dismiss(id);
            }
        }
        let context = self.context(mode, scratch);
        if scratch {
            self.state.scratch = Some(context.id);
        }
        self.event(SessionEvent::HotkeyDown(context))
    }
    pub fn stop(&mut self) -> Result<()> {
        if let Some((id, _)) = &self.capture {
            self.event(SessionEvent::HotkeyUp { id: *id })?;
        }
        Ok(())
    }
    pub fn hotkey(&mut self, hotkey: Hotkey, observable: bool) -> Result<()> {
        if self.client.is_none() && matches!(hotkey, Hotkey::Down | Hotkey::Interactive) {
            return Err("model is not ready".into());
        }
        let context = self.context(
            if hotkey == Hotkey::Interactive {
                Mode::Interactive
            } else {
                Mode::Quick
            },
            false,
        );
        let effects = self.session.hotkey(
            hotkey,
            Some(context),
            self.clock.elapsed().as_millis() as u64,
            observable,
        );
        self.execute(effects)
    }
    pub fn ui(&mut self, action: UiAction) -> Result<()> {
        if let UiAction::Edit { id, text } = &action {
            let view = self.session.views().find(|view| view.id == *id);
            match view.map(|view| (view.mode, view.phase)) {
                Some((Mode::Interactive, Phase::Recording | Phase::Finishing | Phase::Saving)) => {
                    return self.event(SessionEvent::Ui(action));
                }
                Some((_, Phase::Recording | Phase::Finishing | Phase::Saving | Phase::Pasting)) => {
                    return Err("finish recognition and delivery before editing this entry".into());
                }
                _ => {
                    self.history.edit(*id, text)?;
                    self.session.history_edited(*id, text.clone());
                    return Ok(());
                }
            }
        }
        let id = match &action {
            UiAction::Insert { id }
            | UiAction::Copy { id }
            | UiAction::Edit { id, .. }
            | UiAction::Discard { id }
            | UiAction::Cancel { id } => Some(*id),
            _ => None,
        };
        if matches!(action, UiAction::Insert { .. })
            && id.is_some_and(|id| {
                !self
                    .session
                    .views()
                    .any(|view| view.id == id && view.mode == Mode::Interactive)
            })
        {
            let row = self
                .history
                .get(id.unwrap())?
                .ok_or("unknown history entry")?;
            if self
                .platform
                .focused()
                .as_ref()
                .is_some_and(|target| self.platform.is_self(target))
            {
                self.platform.focus(
                    self.external
                        .as_ref()
                        .ok_or("focus a target app before opening history")?,
                )?;
            }
            return self.event(SessionEvent::InsertLast {
                utterance: Some(row),
            });
        }
        if id.is_some_and(|id| !self.session.views().any(|view| view.id == id)) {
            let id = id.unwrap();
            let row = self.history.get(id)?.ok_or("unknown history entry")?;
            match action {
                UiAction::Insert { .. } => {
                    return self.event(SessionEvent::InsertLast {
                        utterance: Some(row),
                    });
                }
                UiAction::Copy { .. } => self
                    .platform
                    .copy(row.text.as_deref().ok_or("recognition is unfinished")?)?,
                UiAction::Edit { text, .. } => self.history.edit(id, &text)?,
                UiAction::Discard { .. } => {
                    self.history.set_status(id, UtteranceStatus::Discarded)?
                }
                UiAction::Cancel { .. } => return Err("recording is no longer active".into()),
                UiAction::InsertLast => unreachable!(),
            }
            return Ok(());
        }
        let inserting = matches!(action, UiAction::Insert { .. });
        self.event(SessionEvent::Ui(action))?;
        if inserting
            && let Some(id) = id
            && !self
                .session
                .views()
                .any(|view| view.id == id && view.phase == Phase::Pasted)
        {
            return Err(self
                .state
                .notifications
                .get(&id)
                .cloned()
                .unwrap_or_else(|| {
                    "Text was not inserted. Finish recognition and try again.".into()
                })
                .into());
        }
        Ok(())
    }
    pub fn action(&mut self, action: HostAction) -> Result<()> {
        match action {
            HostAction::SettingsSave { settings } => {
                if self.capture.is_some() || !self.engines.is_empty() || !self.replaying.is_empty()
                {
                    return Err("finish recognition before changing settings".into());
                }
                if !self
                    .state
                    .models
                    .iter()
                    .any(|model| model.id == settings.engine)
                {
                    return Err("unknown engine".into());
                }
                let old = self.settings.clone();
                self.platform.configure(&settings)?;
                if let Err(error) = settings.save(&self.paths.config.join("settings.toml")) {
                    let _ = self.platform.configure(&old);
                    return Err(error);
                }
                let changed = settings.engine != old.engine;
                self.settings = settings;
                if changed {
                    self.client = None;
                    self.state.download.state = "missing".into();
                    self.install()?;
                }
                self.state.instructions = self.platform.instructions();
            }
            HostAction::DownloadModel => self.install()?,
            HostAction::Permission { permission } => {
                crate::desktop::request_permission(&permission)?
            }
            HostAction::HistorySearch { query } => {
                self.history.search(&query)?;
                self.query = query;
            }
            HostAction::HistoryDelete { id } => {
                self.delete(id)?;
            }
            HostAction::HistoryRetention { before } => {
                for row in self
                    .history
                    .search("")?
                    .into_iter()
                    .filter(|row| row.started_at < before)
                {
                    self.delete(row.id)?;
                }
            }
            HostAction::HistoryRetry { id } => {
                let mut row = self.history.get(id)?.ok_or("unknown history entry")?;
                if row.raw_text.is_none()
                    || self.session.views().any(|view| {
                        view.id == id
                            && matches!(
                                view.phase,
                                Phase::Recording
                                    | Phase::Finishing
                                    | Phase::Saving
                                    | Phase::Pasting
                            )
                    })
                {
                    return Err("recognition is still active".into());
                }
                row.id = UtteranceId::random();
                row.started_at = now();
                row.raw_text = None;
                row.text = None;
                row.status = None;
                row.pasted_to = None;
                row.pasted_at = None;
                self.history.insert(&row)?;
                self.event(SessionEvent::Recover { utterance: row })?;
            }
            HostAction::Start { mode, scratch } => self.start(mode, scratch)?,
            HostAction::Stop => self.stop()?,
            HostAction::SetupComplete => {
                if self.client.is_none() {
                    return Err("model is not ready".into());
                }
                if self
                    .platform
                    .permissions()
                    .values()
                    .any(|state| state != "authorized")
                {
                    return Err("grant the required permissions first".into());
                }
                self.settings
                    .save(&self.paths.config.join("settings.toml"))?;
                atomic_write(&self.paths.config.join("setup-complete"), b"complete\n")?;
                self.state.setup_complete = true;
            }
        }
        Ok(())
    }
    fn delete(&mut self, id: UtteranceId) -> Result<()> {
        if self.session.views().any(|view| {
            view.id == id
                && matches!(
                    view.phase,
                    Phase::Recording | Phase::Finishing | Phase::Saving | Phase::Pasting
                )
        }) || self.replaying.contains_key(&id)
        {
            return Err("finish recognition before deleting its audio".into());
        }
        self.history.delete(id)?;
        self.session.dismiss(id);
        self.state.notifications.remove(&id);
        Ok(())
    }
    pub fn recording(&self) -> bool {
        self.capture.is_some()
    }
    pub fn snapshot(&mut self) -> Result<HostSnapshot> {
        self.sequence += 1;
        self.state.unavailable = self.session.unavailable_reason().map(str::to_owned);
        self.state.permissions = self.platform.permissions();
        self.state.instructions = self.platform.instructions();
        match disk_bytes(&self.paths.data) {
            Ok(bytes) => {
                self.state.disk_bytes = bytes;
                self.state.disk_error = None;
            }
            Err(error) => self.state.disk_error = Some(error.to_string()),
        }
        let sessions: Vec<_> = self.session.views().cloned().collect();
        self.state
            .notifications
            .retain(|id, _| sessions.iter().any(|view| view.id == *id));
        Ok(HostSnapshot {
            sequence: self.sequence,
            sessions,
            history: self.history.search(&self.query)?,
            settings: self.settings.clone(),
            devices: self.recorder.list_devices().unwrap_or_default(),
            host: self.state.clone(),
        })
    }
    pub fn poll(&mut self) -> Result<bool> {
        if self.capture.is_none()
            && self.engines.is_empty()
            && self.replaying.is_empty()
            && let Some(client) = &self.client
        {
            client.unload_if_idle(self.model_idle_timeout);
        }
        let refreshed = self.platform.refresh();
        let events = self.platform.poll();
        let mut changed = refreshed || !events.is_empty();
        for event in events {
            let available = match &event {
                SessionEvent::SessionAvailable => Some(true),
                SessionEvent::SessionUnavailable { .. } => Some(false),
                _ => None,
            };
            self.event(event)?;
            if let Some(available) = available {
                self.platform.availability(available);
                self.state.instructions = self.platform.instructions();
            }
        }
        if let Some((id, receiver)) = self
            .capture
            .as_ref()
            .map(|(id, receiver)| (*id, receiver.clone()))
        {
            for _ in 0..128 {
                match receiver.try_recv() {
                    Ok(event) => {
                        changed = true;
                        if let AudioEvent::Error { message, .. } = &event {
                            self.event(SessionEvent::Engine(EngineOutput {
                                id,
                                event: EngineEvent::Error(message.clone()),
                            }))?;
                        }
                        if matches!(event, AudioEvent::Stopped { .. })
                            && self
                                .capture
                                .as_ref()
                                .is_some_and(|(current, _)| *current == id)
                        {
                            let _ = self.recorder.stop();
                            self.capture = None;
                            self.platform.recording(false);
                        }
                        self.event(SessionEvent::Audio(event))?;
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        if self
                            .capture
                            .as_ref()
                            .is_some_and(|(current, _)| *current == id)
                        {
                            changed = true;
                            self.capture = None;
                            let _ = self.recorder.stop();
                            self.platform.recording(false);
                            self.event(SessionEvent::CaptureFailed {id,message:"Audio stopped without a durable flush; kept in history for retry".into()})?;
                        }
                        break;
                    }
                }
            }
        }
        let outputs: Vec<_> = self
            .engines
            .iter()
            .map(|(id, active)| (*id, active.events.clone()))
            .collect();
        for (id, output) in outputs {
            for _ in 0..64 {
                match output.try_recv() {
                    Ok(output) => {
                        changed = true;
                        let terminal =
                            matches!(output.event, EngineEvent::Final | EngineEvent::Error(_));
                        self.event(SessionEvent::Engine(output))?;
                        if terminal {
                            self.engines.remove(&id);
                            break;
                        }
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        if self.engines.contains_key(&id) {
                            changed = true;
                            self.event(SessionEvent::Engine(EngineOutput {
                                id,
                                event: EngineEvent::Error(
                                    "Worker output closed before Final".into(),
                                ),
                            }))?;
                            self.engines.remove(&id);
                        }
                        break;
                    }
                }
            }
        }
        for background in self.background.clone().try_iter() {
            changed = true;
            match background {
                Background::Download(progress) => self.state.download = progress,
                Background::Ready(result) => match result {
                    Ok(client) => {
                        self.set_client(client);
                        self.recover()?;
                    }
                    Err(error) => {
                        self.state.download.state = "failed".into();
                        self.state.download.error = Some(error.to_string());
                    }
                },
                Background::Replay { id, result } => {
                    if self.replaying.remove(&id).is_none() {
                        continue;
                    }
                    match result {
                        Ok((duration_ms, text)) => {
                            self.event(SessionEvent::Audio(AudioEvent::Stopped {
                                id,
                                duration_ms,
                            }))?;
                            self.event(SessionEvent::Engine(EngineOutput {
                                id,
                                event: EngineEvent::Committed(text),
                            }))?;
                            self.event(SessionEvent::Engine(EngineOutput {
                                id,
                                event: EngineEvent::Final,
                            }))?;
                        }
                        Err(error) => self.event(SessionEvent::Engine(EngineOutput {
                            id,
                            event: EngineEvent::Error(error.to_string()),
                        }))?,
                    }
                }
            }
        }
        Ok(changed)
    }
    pub fn event(&mut self, event: SessionEvent) -> Result<()> {
        if let SessionEvent::FocusChanged {
            target: Some(target),
        } = &event
            && !self.platform.is_self(target)
        {
            self.external = Some(target.clone());
        }
        let effects = self.session.reduce(event);
        self.execute(effects)
    }
    fn execute(&mut self, effects: Vec<Effect>) -> Result<()> {
        let mut queue = VecDeque::from(effects);
        while let Some(effect) = queue.pop_front() {
            let mut response = None;
            match effect {
                Effect::Save { utterance } => {
                    let result = if utterance.raw_text.is_none() {
                        self.history.insert(&utterance)
                    } else {
                        self.history.finish(&utterance)
                    };
                    response = Some(match result {
                        Ok(()) => SessionEvent::Saved { id: utterance.id },
                        Err(error) => SessionEvent::SaveFailed {
                            id: utterance.id,
                            message: error.to_string(),
                        },
                    });
                }
                Effect::StartEngine { id } => {
                    let result: Result<ActiveEngine> = (|| {
                        let (mut engine, events) =
                            self.client.as_ref().ok_or("model not ready")?.engine(id)?;
                        engine.start()?;
                        Ok(ActiveEngine { engine, events })
                    })();
                    match result {
                        Ok(active) => {
                            self.engines.insert(id, active);
                        }
                        Err(error) => {
                            response = Some(SessionEvent::Engine(EngineOutput {
                                id,
                                event: EngineEvent::Error(format!("{error}")),
                            }))
                        }
                    }
                }
                Effect::StartCapture { id } => {
                    match self.recorder.start(id, self.settings.microphone.as_deref()) {
                        Ok(events) => {
                            self.capture = Some((id, events));
                            self.platform.recording(true);
                        }
                        Err(error) => {
                            response = Some(SessionEvent::CaptureFailed {
                                id,
                                message: error.to_string(),
                            });
                        }
                    }
                }
                Effect::StopCapture { id } => {
                    if self
                        .capture
                        .as_ref()
                        .is_some_and(|(current, _)| *current == id)
                    {
                        let (_, audio) = self.capture.take().unwrap();
                        let result = self.recorder.stop();
                        self.platform.recording(false);
                        let mut stopped = false;
                        for event in audio.try_iter() {
                            if let AudioEvent::Error { message, .. } = &event {
                                queue.extend(self.session.reduce(SessionEvent::Engine(
                                    EngineOutput {
                                        id,
                                        event: EngineEvent::Error(message.clone()),
                                    },
                                )));
                            }
                            stopped |= matches!(event, AudioEvent::Stopped { .. });
                            queue.extend(self.session.reduce(SessionEvent::Audio(event)));
                        }
                        if !stopped {
                            response = Some(SessionEvent::CaptureFailed {
                                id,
                                message: result
                                    .err()
                                    .map(|error| error.to_string())
                                    .unwrap_or_else(|| {
                                        "Audio stopped without a durable flush".into()
                                    }),
                            });
                        }
                    }
                }
                Effect::FeedEngine { id, pcm } => {
                    if let Some(active) = self.engines.get_mut(&id) {
                        active.engine.feed(&pcm);
                    }
                }
                Effect::FinishEngine { id } => {
                    if let Some(active) = self.engines.get_mut(&id) {
                        active.engine.finish();
                    }
                }
                Effect::CancelEngine { id } => {
                    if let Some(cancellation) = self.replaying.remove(&id) {
                        let _ = cancellation.try_send(());
                    }
                    if let Some(mut active) = self.engines.remove(&id) {
                        active.engine.cancel();
                    }
                }
                Effect::AllocateReplay { id } => {
                    let recognition_id = UtteranceId::random();
                    if self.state.scratch == Some(id) {
                        self.state.scratch = Some(recognition_id);
                    }
                    response = Some(SessionEvent::ReplayReady { id, recognition_id });
                }
                Effect::RecognizeFile {
                    id,
                    audio_id,
                    engine,
                } => {
                    let (cancel, cancellation) = bounded(1);
                    self.replaying.insert(id, cancel);
                    let sender = self.sender.clone();
                    let path = self.history.audio_path(audio_id);
                    let client = (engine == self.settings.engine)
                        .then(|| self.client.clone())
                        .flatten();
                    let config = worker_config(&self.paths.models, &engine)?;
                    std::thread::spawn(move || {
                        let result = (|| {
                            let samples = said_audio::wav::recover_wav(&path)?;
                            let text = match client {
                                Some(client) => client.recognize_file(id, &path, &cancellation)?,
                                None => {
                                    recognize_file_with_cancel(id, &config, &path, &cancellation)?
                                }
                            };
                            Ok((samples * 1000 / 16000, text))
                        })();
                        let _ = sender.send(Background::Replay { id, result });
                    });
                }
                Effect::CheckFocus { id } => {
                    let target = self.platform.focused();
                    for event in self.platform.poll() {
                        self.event(event)?;
                    }
                    response = Some(SessionEvent::FocusChecked { id, target })
                }
                Effect::FocusTarget { id, target } => {
                    let _ = self.platform.focus(&target);
                    response = Some(SessionEvent::FocusChecked {
                        id,
                        target: self.platform.focused(),
                    });
                }
                Effect::Paste {
                    id,
                    target,
                    text,
                    send,
                    automatic,
                } => {
                    response = Some(match self.platform.paste(&target, &text, send, automatic) {
                        Ok(()) => SessionEvent::Pasted { id, at: now() },
                        Err(error) => SessionEvent::PasteFailed {
                            id,
                            message: error.to_string(),
                        },
                    })
                }
                Effect::Copy { id, text } => {
                    if self.state.scratch != Some(id) {
                        self.platform.copy(&text)?;
                    }
                }
                Effect::EditHistory { id, text } => {
                    response = Some(match self.history.edit(id, &text) {
                        Ok(()) => SessionEvent::EditSaved { id },
                        Err(error) => SessionEvent::EditFailed {
                            id,
                            message: error.to_string(),
                        },
                    })
                }
                Effect::SetStatus { id, status } => self.history.set_status(id, status)?,
                Effect::LoadLast => {
                    response = Some(SessionEvent::InsertLast {
                        utterance: self.history.last()?,
                    })
                }
                Effect::RecordDelivery { id, target, at } => {
                    self.history.record_delivery(id, &target, at)?
                }
                Effect::Show { .. } => {}
                Effect::Notify { id, message } => {
                    self.state.notifications.insert(
                        id,
                        if message == "Saved. Use Insert last to insert." {
                            self.settings.saved_hint()
                        } else {
                            message
                        },
                    );
                }
            }
            if let Some(event) = response {
                queue.extend(self.session.reduce(event));
            }
        }
        Ok(())
    }
}
