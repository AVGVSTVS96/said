use std::collections::VecDeque;

use crate::*;

#[derive(Default)]
pub struct Session {
    entries: VecDeque<Entry>,
}

struct Entry {
    view: SessionView,
    utterance: Utterance,
    send: bool,
    canceled: bool,
    delivery_started: bool,
    delivery_done: bool,
}

impl Session {
    pub fn views(&self) -> impl Iterator<Item = &SessionView> {
        self.entries.iter().map(|entry| &entry.view)
    }

    pub fn reduce(&mut self, event: SessionEvent) -> Vec<Effect> {
        if let SessionEvent::HotkeyDown(context) = event {
            if self
                .entries
                .iter()
                .any(|entry| entry.view.id == context.id || entry.view.phase == Phase::Recording)
            {
                return vec![];
            }
            let view = SessionView {
                id: context.id,
                target: context.target,
                mode: context.mode,
                phase: Phase::Recording,
                committed: String::new(),
                partial: String::new(),
                level: None,
            };
            let effects = vec![
                Effect::StartEngine {
                    id: context.id,
                    vocabulary: context.vocabulary,
                },
                Effect::StartCapture { id: context.id },
                Effect::Show { view: view.clone() },
            ];
            self.entries.push_back(Entry {
                view,
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
                send: context.send,
                canceled: false,
                delivery_started: false,
                delivery_done: false,
            });
            return effects;
        }

        let id = match &event {
            SessionEvent::HotkeyDown(_) => unreachable!(),
            SessionEvent::HotkeyUp { id }
            | SessionEvent::Saved { id }
            | SessionEvent::SaveFailed { id, .. }
            | SessionEvent::FocusChecked { id, .. }
            | SessionEvent::Pasted { id, .. }
            | SessionEvent::PasteFailed { id, .. }
            | SessionEvent::Cancel { id } => *id,
            SessionEvent::Engine(output) => output.id,
            SessionEvent::Audio(AudioEvent::Frame(frame)) => frame.id,
            SessionEvent::Audio(
                AudioEvent::Stopped { id, .. }
                | AudioEvent::Disconnected { id }
                | AudioEvent::Error { id, .. },
            ) => *id,
        };
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.view.id == id) else {
            return vec![];
        };
        let mut effects = vec![];
        match event {
            SessionEvent::HotkeyUp { .. } if entry.view.phase == Phase::Recording => {
                entry.view.phase = Phase::Finishing;
                effects.push(Effect::StopCapture { id });
            }
            SessionEvent::Audio(AudioEvent::Frame(frame))
                if matches!(entry.view.phase, Phase::Recording | Phase::Finishing)
                    && !entry.canceled =>
            {
                entry.view.level = Some(frame.level);
                effects.push(Effect::FeedEngine { id, pcm: frame.pcm });
            }
            SessionEvent::Audio(AudioEvent::Stopped { duration_ms, .. })
                if entry.view.phase == Phase::Finishing =>
            {
                entry.utterance.duration_ms = duration_ms;
                if entry.canceled {
                    effects.push(entry.save(UtteranceStatus::Canceled));
                } else {
                    effects.push(Effect::FinishEngine { id });
                }
            }
            SessionEvent::Engine(output)
                if matches!(entry.view.phase, Phase::Recording | Phase::Finishing)
                    && !entry.canceled =>
            {
                match output.event {
                    EngineEvent::Partial(text) => entry.view.partial = text,
                    EngineEvent::Committed(text) => {
                        entry.view.committed.push_str(&text);
                        entry.view.partial.clear();
                    }
                    EngineEvent::Final if entry.view.phase == Phase::Finishing => {
                        let status = if entry.view.committed.is_empty() {
                            UtteranceStatus::Empty
                        } else {
                            UtteranceStatus::Done
                        };
                        effects.push(entry.save(status));
                    }
                    EngineEvent::Error(message) => effects.push(Effect::Notify { id, message }),
                    EngineEvent::Final => return vec![],
                }
            }
            SessionEvent::Cancel { .. } if entry.view.phase == Phase::Recording => {
                entry.canceled = true;
                entry.view.phase = Phase::Finishing;
                effects.extend([Effect::StopCapture { id }, Effect::CancelEngine { id }]);
            }
            SessionEvent::Saved { .. } if entry.view.phase == Phase::Saving => {
                entry.view.phase = if entry.view.mode == Mode::Interactive {
                    Phase::Reviewing
                } else {
                    Phase::Saved
                };
                entry.delivery_done = entry.view.mode == Mode::Interactive
                    || entry.canceled
                    || entry.view.committed.is_empty();
            }
            SessionEvent::SaveFailed { message, .. } if entry.view.phase == Phase::Saving => {
                entry.view.phase = Phase::Failed;
                entry.delivery_done = true;
                effects.push(Effect::Notify { id, message });
            }
            SessionEvent::FocusChecked { target, .. }
                if entry.view.phase == Phase::Saved
                    && entry.delivery_started
                    && !entry.delivery_done =>
            {
                if let Some(original) = &entry.view.target
                    && target
                        .as_ref()
                        .is_some_and(|current| original.same_window(current))
                {
                    entry.view.phase = Phase::Pasting;
                    effects.push(Effect::Paste {
                        id,
                        target: original.clone(),
                        text: entry.view.committed.clone(),
                        send: entry.send,
                    });
                } else {
                    entry.delivery_done = true;
                    effects.push(Effect::Notify {
                        id,
                        message: "Saved. Press Ctrl+Alt+V to insert".into(),
                    });
                }
            }
            SessionEvent::Pasted { at, .. } if entry.view.phase == Phase::Pasting => {
                entry.view.phase = Phase::Pasted;
                entry.delivery_done = true;
                effects.push(Effect::RecordDelivery {
                    id,
                    target: entry.view.target.clone().unwrap(),
                    at,
                });
            }
            SessionEvent::PasteFailed { message, .. } if entry.view.phase == Phase::Pasting => {
                entry.view.phase = Phase::Saved;
                entry.delivery_done = true;
                effects.push(Effect::Notify { id, message });
            }
            _ => return vec![],
        }
        effects.push(Effect::Show {
            view: entry.view.clone(),
        });
        if let Some(next) = self.entries.iter_mut().find(|entry| !entry.delivery_done)
            && next.view.phase == Phase::Saved
            && !next.delivery_started
        {
            next.delivery_started = true;
            effects.push(Effect::CheckFocus { id: next.view.id });
        }
        effects
    }
}

impl Entry {
    fn save(&mut self, status: UtteranceStatus) -> Effect {
        self.view.phase = Phase::Saving;
        self.view.partial.clear();
        self.utterance.raw_text = Some(self.view.committed.clone());
        self.utterance.text = self.utterance.raw_text.clone();
        self.utterance.status = Some(status);
        Effect::Save {
            utterance: self.utterance.clone(),
        }
    }
}
