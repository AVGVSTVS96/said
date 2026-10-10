use std::collections::VecDeque;

use crate::*;

#[derive(Default)]
pub struct Session {
    entries: VecDeque<Entry>,
    unavailable_reason: Option<String>,
    key: Option<KeyPress>,
    toggle: bool,
}

struct KeyPress {
    id: UtteranceId,
    at: u64,
    stopping: bool,
}

struct Entry {
    view: SessionView,
    utterance: Utterance,
    vocabulary: Vec<String>,
    send: bool,
    pending: bool,
    capture_started: bool,
    stopped: bool,
    broken_engine: bool,
    canceled: bool,
    recovered: bool,
    replay: bool,
    editor_tail: usize,
    owned_tail: bool,
    edited_version: u64,
    saving_edit: Option<u64>,
    saved_edit_version: u64,
    insert_after_edit: bool,
    manual_target: Option<Target>,
    manual: bool,
    focus_lost: bool,
    delivery_started: bool,
    delivery_done: bool,
}

impl Session {
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason.as_deref()
    }

    pub fn views(&self) -> impl Iterator<Item = &SessionView> {
        self.entries.iter().map(|entry| &entry.view)
    }

    pub fn dismiss(&mut self, id: UtteranceId) {
        self.entries.retain(|entry| {
            entry.view.id != id
                || entry.saving_edit.is_some()
                || !entry.delivery_done
                || !matches!(
                    entry.view.phase,
                    Phase::Saved | Phase::Pasted | Phase::Failed | Phase::Reviewing
                )
        });
    }

    pub fn hotkey(
        &mut self,
        hotkey: Hotkey,
        context: Option<StartContext>,
        monotonic_ms: u64,
        release_observable: bool,
    ) -> Vec<Effect> {
        match hotkey {
            Hotkey::InsertLast => self.reduce(SessionEvent::Ui(UiAction::InsertLast)),
            Hotkey::Cancel => {
                self.toggle = false;
                self.key = None;
                let id = self
                    .entries
                    .iter()
                    .rev()
                    .find(|entry| {
                        !entry.delivery_done
                            && matches!(
                                entry.view.phase,
                                Phase::Recording | Phase::Finishing | Phase::Saving | Phase::Saved
                            )
                    })
                    .map(|entry| entry.view.id);
                id.map(|id| self.reduce(SessionEvent::Cancel { id }))
                    .unwrap_or_default()
            }
            Hotkey::Up => {
                let Some(key) = self.key.take() else {
                    return vec![];
                };
                if key.stopping {
                    return vec![];
                }
                if monotonic_ms.saturating_sub(key.at) < 300 {
                    self.toggle = true;
                    vec![]
                } else {
                    self.reduce(SessionEvent::HotkeyUp { id: key.id })
                }
            }
            Hotkey::Down | Hotkey::Interactive => {
                if self.key.is_some() && release_observable {
                    return vec![];
                }
                let active = self
                    .entries
                    .iter()
                    .find(|entry| entry.view.phase == Phase::Recording)
                    .map(|entry| entry.view.id);
                if let Some(id) = active {
                    if self.toggle || !release_observable {
                        self.toggle = false;
                        self.key = release_observable.then_some(KeyPress {
                            id,
                            at: monotonic_ms,
                            stopping: true,
                        });
                        return self.reduce(SessionEvent::HotkeyUp { id });
                    }
                    return vec![];
                }
                let Some(mut context) = context else {
                    return vec![];
                };
                if hotkey == Hotkey::Interactive {
                    context.mode = Mode::Interactive;
                }
                let id = context.id;
                let effects = self.reduce(SessionEvent::HotkeyDown(context));
                if !effects.is_empty() {
                    self.toggle = !release_observable;
                    self.key = release_observable.then_some(KeyPress {
                        id,
                        at: monotonic_ms,
                        stopping: false,
                    });
                }
                effects
            }
        }
    }

    pub fn reduce(&mut self, event: SessionEvent) -> Vec<Effect> {
        match event {
            SessionEvent::FocusChanged { target } => {
                for entry in &mut self.entries {
                    if !entry.delivery_done
                        && !entry.manual
                        && !target.as_ref().is_some_and(|current| {
                            entry
                                .view
                                .target
                                .as_ref()
                                .is_some_and(|original| original.same_window(current))
                        })
                    {
                        entry.focus_lost = true;
                    }
                }
                return vec![];
            }
            SessionEvent::ReplayReady { id, recognition_id } => {
                if self
                    .entries
                    .iter()
                    .any(|entry| entry.view.id == recognition_id)
                {
                    return vec![];
                }
                let Some(index) = self.entries.iter().position(|entry| {
                    entry.view.id == id
                        && entry.broken_engine
                        && entry.stopped
                        && !entry.canceled
                        && !entry.replay
                        && entry.view.phase == Phase::Finishing
                }) else {
                    return vec![];
                };
                return self.restart(index, recognition_id);
            }
            SessionEvent::HotkeyDown(context) => return self.start(context),
            SessionEvent::Recover { utterance } => return self.recover(utterance),
            SessionEvent::InsertLast { utterance } => return self.insert_last(utterance),
            SessionEvent::Ui(action) => return self.action(action),
            SessionEvent::SessionUnavailable { reason } => {
                self.unavailable_reason = Some(reason.clone());
                self.key = None;
                self.toggle = false;
                let ids: Vec<_> = self
                    .entries
                    .iter()
                    .filter(|entry| entry.view.phase == Phase::Recording)
                    .map(|entry| entry.view.id)
                    .collect();
                let mut effects = vec![];
                for entry in &mut self.entries {
                    entry.send = false;
                    entry.focus_lost = true;
                    entry.delivery_done = true;
                    entry.insert_after_edit = false;
                    if entry.view.phase != Phase::Pasting {
                        entry.manual = false;
                        entry.delivery_started = false;
                    }
                }
                for id in ids {
                    effects.extend(self.reduce(SessionEvent::HotkeyUp { id }));
                    effects.push(Effect::Notify {
                        id,
                        message: reason.clone(),
                    });
                }
                return effects;
            }
            SessionEvent::SessionAvailable => {
                self.unavailable_reason = None;
                return vec![];
            }
            _ => {}
        }
        let id = event_id(&event);
        let Some(index) = self.entries.iter().position(|entry| entry.view.id == id) else {
            return vec![];
        };
        let entry = &mut self.entries[index];
        let mut effects = vec![];
        let mut restart = false;
        match event {
            SessionEvent::HotkeyUp { .. } if entry.view.phase == Phase::Recording => {
                entry.view.phase = Phase::Finishing;
                if entry.capture_started {
                    effects.push(Effect::StopCapture { id });
                }
            }
            SessionEvent::Audio(AudioEvent::Frame(frame))
                if entry.capture_started
                    && !entry.stopped
                    && matches!(entry.view.phase, Phase::Recording | Phase::Finishing) =>
            {
                entry.view.level = Some(frame.level);
                if !entry.canceled && !entry.broken_engine {
                    effects.push(Effect::FeedEngine { id, pcm: frame.pcm });
                }
            }
            SessionEvent::Audio(AudioEvent::Disconnected { .. })
                if entry.capture_started && !entry.stopped =>
            {
                if entry.view.phase == Phase::Recording {
                    effects.push(Effect::StopCapture { id });
                }
                entry.view.phase = Phase::Finishing;
                effects.push(Effect::Notify {
                    id,
                    message: "Mic disconnected".into(),
                });
                self.toggle = false;
                self.key = None;
            }
            SessionEvent::Audio(AudioEvent::Error { message, .. })
                if entry.capture_started && !entry.stopped =>
            {
                if entry.view.phase == Phase::Recording {
                    effects.push(Effect::StopCapture { id });
                }
                entry.view.phase = Phase::Finishing;
                effects.push(Effect::Notify { id, message });
            }
            SessionEvent::Audio(AudioEvent::Stopped { duration_ms, .. })
                if entry.replay && !entry.pending && entry.view.phase == Phase::Finishing =>
            {
                entry.utterance.duration_ms = duration_ms;
            }
            SessionEvent::Audio(AudioEvent::Stopped { duration_ms, .. })
                if entry.capture_started && !entry.stopped =>
            {
                entry.stopped = true;
                entry.capture_started = false;
                entry.view.phase = Phase::Finishing;
                entry.utterance.duration_ms = duration_ms;
                if entry.canceled {
                    effects.push(entry.save(UtteranceStatus::Canceled));
                } else if entry.broken_engine {
                    restart = true;
                } else {
                    effects.push(Effect::FinishEngine { id });
                }
            }
            SessionEvent::Engine(output)
                if !entry.pending
                    && !entry.canceled
                    && !entry.broken_engine
                    && matches!(entry.view.phase, Phase::Recording | Phase::Finishing) =>
            {
                match output.event {
                    EngineEvent::Partial(text) => {
                        entry.update_editor(&text, false);
                        entry.view.partial = text;
                    }
                    EngineEvent::Committed(text) => {
                        entry.update_editor(&text, true);
                        entry.view.committed.push_str(&text);
                        entry.view.partial.clear();
                    }
                    EngineEvent::Final if entry.stopped => {
                        let status = if entry.recovered {
                            UtteranceStatus::Recovered
                        } else if entry.view.committed.is_empty() {
                            UtteranceStatus::Empty
                        } else {
                            UtteranceStatus::Done
                        };
                        effects.push(entry.save(status));
                    }
                    EngineEvent::Error(message) => {
                        entry.broken_engine = true;
                        effects
                            .extend([Effect::CancelEngine { id }, Effect::Notify { id, message }]);
                        restart = entry.stopped && !entry.replay;
                        if entry.replay {
                            entry.delivery_done = true;
                            effects.push(entry.save(UtteranceStatus::Failed));
                        }
                    }
                    EngineEvent::Final => return vec![],
                }
            }
            SessionEvent::Cancel { .. }
                if matches!(
                    entry.view.phase,
                    Phase::Saving | Phase::Saved | Phase::Reviewing
                ) && !entry.canceled =>
            {
                entry.canceled = true;
                entry.delivery_done = true;
                entry.delivery_started = false;
                entry.manual = false;
                entry.insert_after_edit = false;
                if entry.view.phase != Phase::Saving {
                    entry.view.phase = Phase::Saved;
                    effects.push(Effect::SetStatus {
                        id,
                        status: UtteranceStatus::Canceled,
                    });
                }
            }
            SessionEvent::Cancel { .. }
                if matches!(entry.view.phase, Phase::Recording | Phase::Finishing)
                    && !entry.canceled =>
            {
                let was_recording = entry.view.phase == Phase::Recording;
                entry.canceled = true;
                entry.delivery_done = true;
                entry.view.phase = Phase::Finishing;
                if !entry.pending {
                    effects.push(Effect::CancelEngine { id });
                    if entry.capture_started {
                        if was_recording {
                            effects.push(Effect::StopCapture { id });
                        }
                    } else {
                        effects.push(entry.save(UtteranceStatus::Canceled));
                    }
                }
            }
            SessionEvent::Saved { .. } if entry.pending => {
                entry.pending = false;
                if entry.canceled {
                    effects.push(entry.save(UtteranceStatus::Canceled));
                } else if entry.replay {
                    entry.view.phase = Phase::Finishing;
                    effects.push(Effect::RecognizeFile {
                        id,
                        audio_id: entry.utterance.audio_id,
                        engine: entry.utterance.engine.clone(),
                        vocabulary: entry.vocabulary.clone(),
                    });
                } else {
                    entry.capture_started = true;
                    effects.extend([
                        Effect::StartEngine {
                            id,
                            vocabulary: entry.vocabulary.clone(),
                        },
                        Effect::StartCapture { id },
                    ]);
                    if entry.view.phase == Phase::Finishing {
                        effects.push(Effect::StopCapture { id });
                    }
                }
            }
            SessionEvent::Saved { .. } if entry.view.phase == Phase::Saving => {
                entry.view.phase = if entry.view.mode == Mode::Interactive
                    && !entry.canceled
                    && entry.utterance.status != Some(UtteranceStatus::Failed)
                {
                    Phase::Reviewing
                } else {
                    Phase::Saved
                };
                entry.delivery_done |= entry.view.mode == Mode::Interactive
                    || entry.canceled
                    || entry.recovered
                    || entry.utterance.status == Some(UtteranceStatus::Failed)
                    || entry.view.committed.is_empty();
                if entry.view.mode == Mode::Quick
                    && entry.view.target.is_none()
                    && !entry.delivery_done
                {
                    entry.delivery_done = true;
                    effects.push(Effect::Copy {
                        id,
                        text: entry.text().to_owned(),
                    });
                    effects.push(Effect::Notify {
                        id,
                        message: "Copied".into(),
                    });
                }
                if entry.view.committed.is_empty() && !entry.canceled && !entry.recovered {
                    effects.push(Effect::Notify {
                        id,
                        message: "No speech recognized".into(),
                    });
                }
                if entry.canceled && entry.utterance.status != Some(UtteranceStatus::Canceled) {
                    effects.push(Effect::SetStatus {
                        id,
                        status: UtteranceStatus::Canceled,
                    });
                }
                if entry.view.edited.is_some() {
                    effects.push(entry.persist_edit());
                }
            }
            SessionEvent::SaveFailed { message, .. }
                if entry.pending || entry.view.phase == Phase::Saving =>
            {
                entry.pending = false;
                entry.view.phase = Phase::Failed;
                entry.delivery_done = true;
                effects.push(Effect::Notify { id, message });
            }
            SessionEvent::FocusChecked { target, .. }
                if entry.delivery_started
                    && (!entry.delivery_done || entry.manual)
                    && matches!(entry.view.phase, Phase::Saved | Phase::Reviewing) =>
            {
                let destination = if entry.manual && entry.manual_target.is_none() {
                    target.clone()
                } else {
                    entry
                        .manual_target
                        .clone()
                        .or_else(|| entry.view.target.clone())
                };
                if let Some(destination) = destination
                    && target
                        .as_ref()
                        .is_some_and(|current| destination.same_window(current))
                    && self.unavailable_reason.is_none()
                    && (entry.manual || !entry.focus_lost)
                {
                    entry.manual_target = Some(destination.clone());
                    entry.view.phase = Phase::Pasting;
                    effects.push(Effect::Paste {
                        id,
                        target: destination,
                        text: entry.text().to_owned(),
                        send: entry.send && !entry.manual && entry.view.mode == Mode::Quick,
                    });
                } else {
                    entry.delivery_done = true;
                    entry.delivery_started = false;
                    entry.manual = false;
                    effects.push(Effect::Notify {
                        id,
                        message: "Saved. Press Ctrl+Alt+V to insert".into(),
                    });
                }
            }
            SessionEvent::Pasted { at, .. } if entry.view.phase == Phase::Pasting => {
                entry.view.phase = Phase::Pasted;
                entry.delivery_done = true;
                entry.delivery_started = false;
                entry.manual = false;
                effects.push(Effect::RecordDelivery {
                    id,
                    target: entry.manual_target.take().unwrap(),
                    at,
                });
                effects.push(Effect::Notify {
                    id,
                    message: "Pasted".into(),
                });
            }
            SessionEvent::PasteFailed { message, .. } if entry.view.phase == Phase::Pasting => {
                entry.view.phase = if entry.view.mode == Mode::Interactive {
                    Phase::Reviewing
                } else {
                    Phase::Saved
                };
                entry.delivery_done = true;
                entry.delivery_started = false;
                entry.manual = false;
                effects.push(Effect::Notify { id, message });
            }
            SessionEvent::EditSaved { .. } if entry.saving_edit.is_some() => {
                let version = entry.saving_edit.take().unwrap();
                entry.saved_edit_version = version;
                if version != entry.edited_version {
                    effects.push(entry.persist_edit());
                } else if entry.insert_after_edit {
                    entry.insert_after_edit = false;
                    if self.unavailable_reason.is_none() {
                        effects.extend(entry.insert());
                    }
                }
            }
            SessionEvent::EditFailed { message, .. } if entry.saving_edit.is_some() => {
                entry.saving_edit = None;
                entry.insert_after_edit = false;
                effects.push(Effect::Notify { id, message });
            }
            _ => return vec![],
        }
        effects.push(Effect::Show {
            view: entry.view.clone(),
        });
        if restart {
            effects.push(Effect::AllocateReplay { id });
        }
        self.deliver_next(&mut effects);
        self.evict_completed();
        effects
    }

    fn start(&mut self, context: StartContext) -> Vec<Effect> {
        if self.unavailable_reason.is_some()
            || self
                .entries
                .iter()
                .any(|entry| entry.view.id == context.id || entry.view.phase == Phase::Recording)
        {
            return vec![];
        }
        let entry = Entry::new(context);
        let effects = vec![
            Effect::Save {
                utterance: entry.utterance.clone(),
            },
            Effect::Show {
                view: entry.view.clone(),
            },
        ];
        self.entries.push_back(entry);
        effects
    }

    fn recover(&mut self, utterance: Utterance) -> Vec<Effect> {
        if utterance.raw_text.is_some()
            || self
                .entries
                .iter()
                .any(|entry| entry.view.id == utterance.id)
        {
            return vec![];
        }
        let mut entry = Entry::new(StartContext {
            id: utterance.id,
            started_at: utterance.started_at,
            engine: utterance.engine.clone(),
            target: None,
            mode: Mode::Quick,
            vocabulary: vec![],
            send: false,
        });
        entry.utterance = utterance;
        entry.pending = false;
        entry.stopped = true;
        entry.broken_engine = true;
        entry.recovered = true;
        entry.delivery_done = true;
        entry.view.phase = Phase::Finishing;
        let effects = vec![
            Effect::AllocateReplay { id: entry.view.id },
            Effect::Show {
                view: entry.view.clone(),
            },
        ];
        self.entries.push_back(entry);
        effects
    }

    fn restart(&mut self, index: usize, recognition_id: UtteranceId) -> Vec<Effect> {
        let entry = &mut self.entries[index];
        entry.delivery_done = true;
        let mut next = Entry::new(StartContext {
            id: recognition_id,
            started_at: entry.utterance.started_at,
            engine: entry.utterance.engine.clone(),
            target: entry.view.target.clone(),
            mode: entry.view.mode,
            vocabulary: entry.vocabulary.clone(),
            send: entry.send,
        });
        next.utterance.audio_id = entry.utterance.audio_id;
        next.utterance.duration_ms = entry.utterance.duration_ms;
        next.recovered = entry.recovered;
        next.focus_lost = entry.focus_lost;
        next.delivery_done = entry.recovered || self.unavailable_reason.is_some();
        next.replay = true;
        next.stopped = true;
        next.view.phase = Phase::Finishing;
        next.view.edited = entry.view.edited.clone();
        next.editor_tail = next.view.edited.as_ref().map_or(0, String::len);
        next.owned_tail = next.view.edited.is_some();
        let interrupted = entry.save(UtteranceStatus::Failed);
        let effects = vec![
            Effect::Save {
                utterance: next.utterance.clone(),
            },
            interrupted,
            Effect::Show {
                view: entry.view.clone(),
            },
            Effect::Show {
                view: next.view.clone(),
            },
            Effect::Notify {
                id: next.view.id,
                message: if next.recovered {
                    "Recovered"
                } else {
                    "Recognizer restarted"
                }
                .into(),
            },
        ];
        self.entries.insert(index + 1, next);
        effects
    }

    fn insert_last(&mut self, utterance: Option<Utterance>) -> Vec<Effect> {
        let Some(utterance) = utterance else {
            return vec![];
        };
        if utterance.text.as_ref().is_none_or(String::is_empty) || self.unavailable_reason.is_some()
        {
            return vec![];
        }
        let id = utterance.id;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.view.id == id) {
            if !entry.delivery_done || entry.view.phase == Phase::Pasting {
                return vec![];
            }
            entry.utterance = utterance;
            entry.view.edited = entry.utterance.text.clone();
            entry.view.phase = Phase::Saved;
            entry.manual_target = None;
            entry.manual = true;
            entry.delivery_started = true;
        } else {
            let mut entry = Entry::new(StartContext {
                id,
                started_at: utterance.started_at,
                engine: utterance.engine.clone(),
                target: None,
                mode: Mode::Quick,
                vocabulary: vec![],
                send: false,
            });
            entry.view.committed = utterance.raw_text.clone().unwrap();
            entry.view.edited = utterance.text.clone();
            entry.utterance = utterance;
            entry.pending = false;
            entry.stopped = true;
            entry.delivery_done = true;
            entry.manual = true;
            entry.delivery_started = true;
            entry.view.phase = Phase::Saved;
            self.entries.push_back(entry);
        }
        vec![Effect::CheckFocus { id }]
    }

    fn action(&mut self, action: UiAction) -> Vec<Effect> {
        let id = match action {
            UiAction::InsertLast => return vec![Effect::LoadLast],
            UiAction::Cancel { id } => return self.reduce(SessionEvent::Cancel { id }),
            UiAction::Insert { id }
            | UiAction::Copy { id }
            | UiAction::Edit { id, .. }
            | UiAction::Discard { id } => id,
        };
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.view.id == id) else {
            return vec![];
        };
        match action {
            UiAction::Edit { text, .. }
                if entry.view.mode == Mode::Interactive
                    && matches!(
                        entry.view.phase,
                        Phase::Recording
                            | Phase::Finishing
                            | Phase::Saving
                            | Phase::Reviewing
                            | Phase::Pasted
                    ) =>
            {
                entry.edit(text);
                entry.edited_version += 1;
                let mut effects = vec![Effect::Show {
                    view: entry.view.clone(),
                }];
                if matches!(entry.view.phase, Phase::Reviewing | Phase::Pasted)
                    && entry.saving_edit.is_none()
                {
                    effects.push(entry.persist_edit());
                }
                effects
            }
            UiAction::Copy { .. } => vec![Effect::Copy {
                id,
                text: entry.text().to_owned(),
            }],
            UiAction::Insert { .. }
                if matches!(entry.view.phase, Phase::Reviewing | Phase::Pasted)
                    && self.unavailable_reason.is_none() =>
            {
                if entry.saving_edit.is_some() || entry.saved_edit_version != entry.edited_version {
                    entry.insert_after_edit = true;
                    if entry.saving_edit.is_none() {
                        vec![entry.persist_edit()]
                    } else {
                        vec![]
                    }
                } else {
                    entry.insert()
                }
            }
            UiAction::Discard { .. } if entry.view.phase == Phase::Reviewing => {
                entry.delivery_done = true;
                entry.view.phase = Phase::Saved;
                vec![
                    Effect::SetStatus {
                        id,
                        status: UtteranceStatus::Discarded,
                    },
                    Effect::Show {
                        view: entry.view.clone(),
                    },
                ]
            }
            _ => vec![],
        }
    }

    fn evict_completed(&mut self) {
        let latest = self
            .entries
            .iter()
            .rev()
            .find(|entry| {
                entry.delivery_done
                    && matches!(
                        entry.view.phase,
                        Phase::Saved | Phase::Pasted | Phase::Failed
                    )
            })
            .map(|entry| entry.view.id);
        self.entries.retain(|entry| {
            !entry.delivery_done
                || entry.saving_edit.is_some()
                || (entry.view.mode == Mode::Interactive && entry.view.phase == Phase::Pasted)
                || !matches!(
                    entry.view.phase,
                    Phase::Saved | Phase::Pasted | Phase::Failed
                )
                || Some(entry.view.id) == latest
        });
    }

    fn deliver_next(&mut self, effects: &mut Vec<Effect>) {
        if self
            .entries
            .iter()
            .any(|entry| entry.view.phase == Phase::Pasting)
        {
            return;
        }
        if let Some(next) = self.entries.iter_mut().find(|entry| !entry.delivery_done)
            && next.view.phase == Phase::Saved
            && !next.delivery_started
        {
            next.delivery_started = true;
            effects.push(Effect::CheckFocus { id: next.view.id });
        }
    }
}

impl Entry {
    fn new(context: StartContext) -> Self {
        Self {
            view: SessionView {
                id: context.id,
                target: context.target,
                mode: context.mode,
                phase: Phase::Recording,
                committed: String::new(),
                partial: String::new(),
                level: None,
                edited: None,
            },
            utterance: Utterance {
                id: context.id,
                audio_id: context.id,
                started_at: context.started_at,
                duration_ms: 0,
                engine: context.engine,
                raw_text: None,
                text: None,
                status: None,
                pasted_to: None,
                pasted_at: None,
            },
            vocabulary: context.vocabulary,
            send: context.send,
            pending: true,
            capture_started: false,
            stopped: false,
            broken_engine: false,
            canceled: false,
            recovered: false,
            replay: false,
            editor_tail: 0,
            owned_tail: false,
            edited_version: 0,
            saving_edit: None,
            saved_edit_version: 0,
            insert_after_edit: false,
            manual_target: None,
            manual: false,
            focus_lost: false,
            delivery_started: false,
            delivery_done: false,
        }
    }

    fn edit(&mut self, text: String) {
        let previous = self
            .view
            .edited
            .clone()
            .unwrap_or_else(|| format!("{}{}", self.view.committed, self.view.partial));
        if self.view.edited.is_none() {
            self.editor_tail = self.view.committed.len();
        }
        let prefix = previous
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf8())
            .sum::<usize>();
        let suffix = previous[prefix..]
            .chars()
            .rev()
            .zip(text[prefix..].chars().rev())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf8())
            .sum::<usize>();
        let end = previous.len() - suffix;
        if end > self.editor_tail || (prefix >= self.editor_tail && !self.view.partial.is_empty()) {
            self.owned_tail = true;
        } else {
            self.editor_tail = self.editor_tail + text.len() - previous.len();
        }
        self.view.edited = Some(text);
    }

    fn update_editor(&mut self, text: &str, committed: bool) {
        if self.replay && self.view.edited.is_some() {
            return;
        }
        if let Some(edited) = &mut self.view.edited {
            if !self.owned_tail {
                edited.replace_range(self.editor_tail.., text);
            }
            if committed {
                self.editor_tail = edited.len();
                self.owned_tail = false;
            }
        }
    }

    fn text(&self) -> &str {
        self.view.edited.as_deref().unwrap_or(&self.view.committed)
    }

    fn save(&mut self, status: UtteranceStatus) -> Effect {
        self.view.phase = Phase::Saving;
        self.update_editor("", false);
        self.view.partial.clear();
        self.utterance.raw_text = Some(self.view.committed.clone());
        self.utterance.text = self.utterance.raw_text.clone();
        self.utterance.status = Some(status);
        Effect::Save {
            utterance: self.utterance.clone(),
        }
    }

    fn persist_edit(&mut self) -> Effect {
        self.saving_edit = Some(self.edited_version);
        Effect::EditHistory {
            id: self.view.id,
            text: self.text().to_owned(),
        }
    }

    fn insert(&mut self) -> Vec<Effect> {
        let Some(target) = self.view.target.clone() else {
            return vec![Effect::Notify {
                id: self.view.id,
                message: "Original app is unavailable".into(),
            }];
        };
        if self.text().is_empty() {
            return vec![];
        }
        self.view.phase = Phase::Reviewing;
        self.manual = true;
        self.delivery_started = true;
        self.manual_target = Some(target.clone());
        vec![Effect::FocusTarget {
            id: self.view.id,
            target,
        }]
    }
}

fn event_id(event: &SessionEvent) -> UtteranceId {
    match event {
        SessionEvent::HotkeyUp { id }
        | SessionEvent::Saved { id }
        | SessionEvent::SaveFailed { id, .. }
        | SessionEvent::FocusChecked { id, .. }
        | SessionEvent::Pasted { id, .. }
        | SessionEvent::PasteFailed { id, .. }
        | SessionEvent::Cancel { id }
        | SessionEvent::EditSaved { id }
        | SessionEvent::EditFailed { id, .. } => *id,
        SessionEvent::Engine(output) => output.id,
        SessionEvent::Audio(AudioEvent::Frame(frame)) => frame.id,
        SessionEvent::Audio(
            AudioEvent::Stopped { id, .. }
            | AudioEvent::Disconnected { id }
            | AudioEvent::Error { id, .. },
        ) => *id,
        _ => unreachable!(),
    }
}
