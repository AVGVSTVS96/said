use std::collections::{BTreeMap, VecDeque};

use crossbeam_channel::Receiver;
use said_audio::{
    Recorder,
    fake::{FakeRecorder, RecorderCall},
};
use said_core::{fake::FakeHistory, *};
use said_engine::{
    Engine,
    fake::{FakeEngine, Trace},
};
use said_platform::{
    Platform,
    fake::{FakePlatform, PlatformCall},
};

fn target(window: &str) -> Target {
    Target {
        app_id: "synthetic.editor".into(),
        window_id: window.into(),
        name: "Scratch".into(),
    }
}

struct Harness {
    session: Session,
    history: FakeHistory,
    recorder: FakeRecorder,
    platform: FakePlatform,
    engines: BTreeMap<UtteranceId, (FakeEngine, Receiver<EngineOutput>)>,
    traces: BTreeMap<UtteranceId, Trace>,
    events: VecDeque<SessionEvent>,
    capture: Option<Receiver<AudioEvent>>,
    effects: Vec<Effect>,
    saves: Vec<Utterance>,
    notices: Vec<String>,
    held_saves: Vec<UtteranceId>,
    held_outputs: Vec<EngineOutput>,
    delay_saves: bool,
    delay_finish: bool,
    fail_save: bool,
    fail_focus: bool,
    fail_edit: bool,
    delay_edit: bool,
    delay_focus: bool,
    delayed_focus: Vec<SessionEvent>,
    clipboard: String,
}

impl Harness {
    fn new() -> Self {
        Self {
            session: Session::default(),
            history: FakeHistory::default(),
            recorder: FakeRecorder::new(vec![(vec![1, -1], 0.5)], 500),
            platform: FakePlatform::new(Some(target("1"))),
            engines: BTreeMap::new(),
            traces: BTreeMap::new(),
            events: VecDeque::new(),
            capture: None,
            effects: vec![],
            saves: vec![],
            notices: vec![],
            held_saves: vec![],
            held_outputs: vec![],
            delay_saves: false,
            delay_finish: false,
            fail_save: false,
            fail_focus: false,
            fail_edit: false,
            delay_edit: false,
            delay_focus: false,
            delayed_focus: vec![],
            clipboard: "old clipboard".into(),
        }
    }

    fn context(&self, id: UtteranceId, mode: Mode) -> StartContext {
        StartContext {
            id,
            started_at: self.history.rows.len() as i64,
            engine: "fake".into(),
            target: self.platform.focused(),
            mode,
            send: false,
        }
    }

    fn start(&mut self, mode: Mode, text: &str) -> UtteranceId {
        let id = UtteranceId::random();
        self.traces.insert(
            id,
            Trace {
                finish: vec![EngineEvent::Committed(text.into()), EngineEvent::Final],
                ..Default::default()
            },
        );
        self.event(SessionEvent::HotkeyDown(self.context(id, mode)));
        id
    }

    fn event(&mut self, event: SessionEvent) {
        self.events.push_back(event);
        self.drain();
    }

    fn drain(&mut self) {
        let mut count = 0;
        while let Some(event) = self.events.pop_front() {
            count += 1;
            assert!(count < 1000, "unbounded reducer trace");
            let effects = self.session.reduce(event);
            self.execute(effects);
        }
    }

    fn execute(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            self.effects.push(effect.clone());
            match effect {
                Effect::AllocateReplay { id } => self.events.push_back(SessionEvent::ReplayReady {
                    id,
                    recognition_id: UtteranceId::random(),
                }),
                Effect::Save { utterance } => {
                    let id = utterance.id;
                    if self.fail_save {
                        self.events.push_back(SessionEvent::SaveFailed {
                            id,
                            message: "synthetic disk failure".into(),
                        });
                    } else {
                        if self.history.get(id).unwrap().is_some() {
                            self.history.finish(&utterance).unwrap();
                        } else {
                            self.history.insert(&utterance).unwrap();
                        }
                        self.saves.push(utterance);
                        if self.delay_saves {
                            self.held_saves.push(id);
                        } else {
                            self.events.push_back(SessionEvent::Saved { id });
                        }
                    }
                }
                Effect::StartEngine { id } => {
                    let (mut engine, output) =
                        FakeEngine::new(id, self.traces.remove(&id).unwrap_or_default());
                    engine.start().unwrap();
                    self.events
                        .extend(output.try_iter().map(SessionEvent::Engine));
                    self.engines.insert(id, (engine, output));
                }
                Effect::StartCapture { id } => {
                    assert!(self.history.get(id).unwrap().unwrap().raw_text.is_none());
                    self.capture = Some(self.recorder.start(id, None).unwrap());
                    self.events.extend(
                        self.capture
                            .as_ref()
                            .unwrap()
                            .try_iter()
                            .map(SessionEvent::Audio),
                    );
                }
                Effect::StopCapture { .. } => {
                    self.recorder.stop().unwrap();
                    self.events.extend(
                        self.capture
                            .as_ref()
                            .unwrap()
                            .try_iter()
                            .map(SessionEvent::Audio),
                    );
                }
                Effect::FeedEngine { id, pcm } => {
                    let (engine, output) = self.engines.get_mut(&id).unwrap();
                    engine.feed(&pcm);
                    self.events
                        .extend(output.try_iter().map(SessionEvent::Engine));
                }
                Effect::FinishEngine { id } => {
                    let (engine, output) = self.engines.get_mut(&id).unwrap();
                    engine.finish();
                    if self.delay_finish {
                        self.held_outputs.extend(output.try_iter());
                    } else {
                        self.events
                            .extend(output.try_iter().map(SessionEvent::Engine));
                    }
                }
                Effect::CancelEngine { id } => {
                    if let Some((engine, _)) = self.engines.get_mut(&id) {
                        engine.cancel();
                    }
                }
                Effect::RecognizeFile { id, .. } => {
                    let (mut engine, output) = FakeEngine::new(
                        id,
                        self.traces.remove(&id).unwrap_or(Trace {
                            finish: vec![
                                EngineEvent::Committed("replayed no no".into()),
                                EngineEvent::Final,
                            ],
                            ..Default::default()
                        }),
                    );
                    engine.start().unwrap();
                    engine.feed(&[1, -1]);
                    engine.finish();
                    self.events
                        .extend(output.try_iter().map(SessionEvent::Engine));
                    self.engines.insert(id, (engine, output));
                }
                Effect::CheckFocus { id } => {
                    let event = SessionEvent::FocusChecked {
                        id,
                        target: self.platform.focused(),
                    };
                    if self.delay_focus {
                        self.delayed_focus.push(event);
                    } else {
                        self.events.push_back(event);
                    }
                }
                Effect::FocusTarget { id, target } => {
                    if !self.fail_focus {
                        self.platform.focus(&target).unwrap();
                    }
                    self.events.push_back(SessionEvent::FocusChecked {
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
                    let row = self.history.get(id).unwrap().unwrap();
                    assert_eq!(row.text.as_deref(), Some(text.as_str()));
                    assert!(row.raw_text.is_some());
                    match self.platform.paste(&target, &text, send, automatic) {
                        Ok(()) => {
                            self.clipboard = text;
                            self.events.push_back(SessionEvent::Pasted { id, at: 1000 });
                        }
                        Err(error) => self.events.push_back(SessionEvent::PasteFailed {
                            id,
                            message: error.to_string(),
                        }),
                    }
                }
                Effect::RecordDelivery { id, target, at } => {
                    self.history.record_delivery(id, &target, at).unwrap()
                }
                Effect::Notify { message, .. } => self.notices.push(message),
                Effect::Copy { text, .. } => self.clipboard = text,
                Effect::EditHistory { id, text } => {
                    if self.fail_edit {
                        self.events.push_back(SessionEvent::EditFailed {
                            id,
                            message: "synthetic edit failure".into(),
                        });
                    } else {
                        self.history.edit(id, &text).unwrap();
                        if !self.delay_edit {
                            self.events.push_back(SessionEvent::EditSaved { id });
                        }
                    }
                }
                Effect::SetStatus { id, status } => self.history.set_status(id, status).unwrap(),
                Effect::LoadLast => self.events.push_back(SessionEvent::InsertLast {
                    utterance: self.history.last(),
                }),
                Effect::Show { .. } => {}
            }
        }
    }

    fn release(&mut self, id: UtteranceId) {
        self.event(SessionEvent::HotkeyUp { id });
    }
    fn engine(&mut self, id: UtteranceId, event: EngineEvent) {
        self.event(SessionEvent::Engine(EngineOutput { id, event }));
    }
    fn pastes(&self) -> Vec<(String, bool)> {
        self.platform
            .calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                PlatformCall::Paste { text, send, .. } => Some((text.clone(), *send)),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn raw_bytes_survive_partials_commits_save_and_multiline_paste() {
    let mut h = Harness::new();
    let id = h.start(Mode::Quick, "actually no, no café\n\n");
    h.engine(id, EngineEvent::Partial("wrong".into()));
    h.engine(id, EngineEvent::Committed("  um um ".into()));
    h.engine(id, EngineEvent::Partial("changed tail".into()));
    h.release(id);
    let expected = "  um um actually no, no café\n\n";
    assert_eq!(
        h.history
            .get(id)
            .unwrap()
            .unwrap()
            .raw_text
            .unwrap()
            .as_bytes(),
        expected.as_bytes()
    );
    assert_eq!(h.clipboard.as_bytes(), expected.as_bytes());
    assert_eq!(h.pastes(), [(expected.into(), false)]);
    let final_save = h
        .effects
        .iter()
        .position(|e| matches!(e,Effect::Save { utterance } if utterance.raw_text.is_some()))
        .unwrap();
    let paste = h
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Paste { .. }))
        .unwrap();
    assert!(final_save < paste);
    assert!(
        h.session
            .reduce(SessionEvent::Engine(EngineOutput {
                id,
                event: EngineEvent::Committed("late".into())
            }))
            .is_empty()
    );
    assert!(
        h.session
            .reduce(SessionEvent::Engine(EngineOutput {
                id: UtteranceId::random(),
                event: EngineEvent::Final
            }))
            .is_empty()
    );
}

#[test]
fn silence_keeps_recording_and_empty_result_saves_audio_without_paste() {
    let mut h = Harness::new();
    h.recorder.frames = vec![(vec![0; 4000], 0.0)];
    let id = h.start(Mode::Quick, "");
    assert_eq!(h.session.views().next().unwrap().phase, Phase::Recording);
    assert_eq!(h.session.views().next().unwrap().level, Some(0.0));
    assert!(h.pastes().is_empty());
    h.release(id);
    let row = h.history.get(id).unwrap().unwrap();
    assert_eq!(row.status, Some(UtteranceStatus::Empty));
    assert_eq!(row.raw_text.as_deref(), Some(""));
    assert_eq!(row.audio_id, id);
    assert!(h.pastes().is_empty());
    assert!(h.notices.contains(&"No speech recognized".into()));
}

#[test]
fn one_hour_frames_have_no_duration_cap_or_retained_pcm_in_core() {
    let mut h = Harness::new();
    h.recorder.duration_ms = 3_600_000;
    let id = h.start(Mode::Quick, "one hour no cap");
    for _ in 0..14_400 {
        let effects = h
            .session
            .reduce(SessionEvent::Audio(AudioEvent::Frame(AudioFrame {
                id,
                pcm: vec![0; 4000],
                level: 0.0,
            })));
        for effect in effects {
            if let Effect::FeedEngine { pcm, .. } = effect {
                h.engines.get_mut(&id).unwrap().0.feed(&pcm);
                h.engines.get_mut(&id).unwrap().0.calls.clear();
            }
        }
        assert_eq!(h.session.views().count(), 1);
        assert_eq!(h.session.views().next().unwrap().committed, "");
    }
    h.release(id);
    assert_eq!(h.history.get(id).unwrap().unwrap().duration_ms, 3_600_000);
    assert_eq!(h.pastes(), [("one hour no cap".into(), false)]);
}

#[test]
fn overlapping_finishes_paste_in_start_order() {
    let mut h = Harness::new();
    h.delay_finish = true;
    let first = h.start(Mode::Quick, "first");
    h.release(first);
    let second = h.start(Mode::Quick, "second");
    assert_eq!(
        h.recorder
            .calls
            .iter()
            .filter(|c| matches!(c, RecorderCall::Start { .. }))
            .count(),
        2
    );
    h.release(second);
    let mut outputs = std::mem::take(&mut h.held_outputs);
    let later = outputs.split_off(2);
    for output in later {
        h.event(SessionEvent::Engine(output));
    }
    assert!(h.pastes().is_empty());
    for output in outputs {
        h.event(SessionEvent::Engine(output));
    }
    assert_eq!(
        h.pastes(),
        [("first".into(), false), ("second".into(), false)]
    );
}

#[test]
fn focus_move_target_close_and_move_back_all_save_without_paste() {
    for destination in [Some(target("2")), None] {
        for finishing in [false, true] {
            let mut h = Harness::new();
            h.delay_finish = finishing;
            let id = h.start(Mode::Quick, "saved words");
            if finishing {
                h.release(id);
            }
            *h.platform.target.borrow_mut() = destination.clone();
            h.event(SessionEvent::FocusChanged {
                target: destination.clone(),
            });
            *h.platform.target.borrow_mut() = Some(target("1"));
            h.event(SessionEvent::FocusChanged {
                target: Some(target("1")),
            });
            if finishing {
                for output in std::mem::take(&mut h.held_outputs) {
                    h.event(SessionEvent::Engine(output));
                }
            } else {
                h.release(id);
            }
            assert!(h.pastes().is_empty());
            assert_eq!(
                h.history.get(id).unwrap().unwrap().text.as_deref(),
                Some("saved words")
            );
            assert!(
                h.notices
                    .contains(&"Saved. Use Insert last to insert.".into())
            );
        }
    }
}

#[test]
fn disconnected_mic_drains_and_finishes_without_switching_devices() {
    let mut h = Harness::new();
    let id = h.start(Mode::Quick, "last frame");
    h.event(SessionEvent::Audio(AudioEvent::Disconnected { id }));
    assert!(h.notices.contains(&"Mic disconnected".into()));
    assert_eq!(h.history.get(id).unwrap().unwrap().duration_ms, 500);
    assert_eq!(
        h.recorder
            .calls
            .iter()
            .filter(|c| matches!(c, RecorderCall::Start { .. }))
            .count(),
        1
    );
    assert_eq!(h.pastes(), [("last frame".into(), false)]);
}

#[test]
fn crashed_or_hung_worker_records_until_release_then_replays_same_audio_with_fresh_id() {
    for message in ["synthetic crash", "synthetic audio acknowledgment timeout"] {
        let mut h = Harness::new();
        let id = h.start(Mode::Quick, "old worker");
        h.engine(id, EngineEvent::Committed("final before crash".into()));
        h.engine(id, EngineEvent::Error(message.into()));
        assert_eq!(h.session.views().next().unwrap().phase, Phase::Recording);
        h.engine(id, EngineEvent::Committed("late".into()));
        h.event(SessionEvent::Audio(AudioEvent::Frame(AudioFrame {
            id,
            pcm: vec![3],
            level: 0.2,
        })));
        h.release(id);
        let rows = h.history.search("").unwrap();
        assert_eq!(rows.len(), 2);
        let replay = rows.iter().find(|row| row.id != id).unwrap();
        assert_eq!(replay.audio_id, id);
        let pending_replay=h.effects.iter().position(|effect| matches!(effect,Effect::Save { utterance } if utterance.id==replay.id && utterance.raw_text.is_none())).unwrap();
        let interrupted=h.effects.iter().position(|effect| matches!(effect,Effect::Save { utterance } if utterance.id==id && utterance.raw_text.is_some())).unwrap();
        assert!(pending_replay < interrupted);
        assert_eq!(replay.raw_text.as_deref(), Some("replayed no no"));
        assert_eq!(
            h.history.get(id).unwrap().unwrap().raw_text.as_deref(),
            Some("final before crash")
        );
        assert_eq!(h.pastes(), [("replayed no no".into(), false)]);
        assert!(h.notices.contains(&"Recognizer restarted".into()));
    }
}

#[test]
fn app_restart_recovers_pending_audio_and_never_auto_pastes() {
    let mut h = Harness::new();
    let id = h.start(Mode::Quick, "never finished");
    h.engine(id, EngineEvent::Committed("in memory only".into()));
    let pending = h.history.recover().unwrap().pop().unwrap();
    h.session = Session::default();
    h.event(SessionEvent::Recover { utterance: pending });
    assert!(h.pastes().is_empty());
    assert_eq!(h.history.recover().unwrap().len(), 0);
    let recovered = h
        .history
        .search("")
        .unwrap()
        .into_iter()
        .find(|row| row.status == Some(UtteranceStatus::Recovered))
        .unwrap();
    assert_ne!(recovered.id, id);
    assert_eq!(recovered.audio_id, id);
    assert_eq!(recovered.text.as_deref(), Some("replayed no no"));
    h.event(SessionEvent::Ui(UiAction::InsertLast));
    assert_eq!(h.pastes(), [("replayed no no".into(), false)]);
}

#[test]
fn cancel_during_capture_or_finishing_keeps_history_and_drops_late_events() {
    for finishing in [false, true] {
        let mut h = Harness::new();
        h.delay_finish = finishing;
        let id = h.start(Mode::Quick, "late result");
        h.engine(id, EngineEvent::Committed("captured text".into()));
        if finishing {
            h.release(id);
        }
        h.event(SessionEvent::Cancel { id });
        for output in std::mem::take(&mut h.held_outputs) {
            h.event(SessionEvent::Engine(output));
        }
        h.engine(id, EngineEvent::Committed("late".into()));
        assert_eq!(
            h.history.get(id).unwrap().unwrap().status,
            Some(UtteranceStatus::Canceled)
        );
        assert_eq!(
            h.history.get(id).unwrap().unwrap().text.as_deref(),
            Some("captured text")
        );
        assert!(h.pastes().is_empty());
        assert_eq!(
            h.recorder
                .calls
                .iter()
                .filter(|c| **c == RecorderCall::Stop)
                .count(),
            1
        );
    }
}

#[test]
fn secure_input_or_lock_stops_saves_and_disables_hotkey_even_after_unlock() {
    for reason in ["Secure input enabled", "Session locked"] {
        let mut h = Harness::new();
        let id = h.start(Mode::Quick, "saved while unavailable");
        h.event(SessionEvent::SessionUnavailable {
            reason: reason.into(),
        });
        assert_eq!(h.session.unavailable_reason(), Some(reason));
        assert_eq!(
            h.history.get(id).unwrap().unwrap().text.as_deref(),
            Some("saved while unavailable")
        );
        assert!(h.pastes().is_empty());
        let blocked = h.session.reduce(SessionEvent::HotkeyDown(
            h.context(UtteranceId::random(), Mode::Quick),
        ));
        assert!(blocked.is_empty());
        h.event(SessionEvent::SessionAvailable);
        assert!(h.session.unavailable_reason().is_none());
        assert!(h.pastes().is_empty());
    }
}

#[test]
fn pending_save_and_final_save_acknowledgments_gate_capture_and_paste() {
    let mut h = Harness::new();
    h.delay_saves = true;
    let id = h.start(Mode::Quick, "gated");
    assert!(h.recorder.calls.is_empty());
    h.event(SessionEvent::Saved { id });
    h.release(id);
    assert!(h.pastes().is_empty());
    assert!(
        h.session
            .reduce(SessionEvent::FocusChecked {
                id,
                target: Some(target("1"))
            })
            .is_empty()
    );
    h.event(SessionEvent::Saved { id });
    assert_eq!(h.pastes(), [("gated".into(), false)]);
}

#[test]
fn failed_pending_or_final_save_never_pastes_and_does_not_block_later_utterances() {
    for pending in [false, true] {
        let mut h = Harness::new();
        h.fail_save = pending;
        let id = h.start(Mode::Quick, "not delivered");
        if !pending {
            h.fail_save = true;
            h.release(id);
        }
        assert!(h.pastes().is_empty());
        h.fail_save = false;
        let next = h.start(Mode::Quick, "next");
        h.release(next);
        assert_eq!(h.pastes(), [("next".into(), false)]);
    }
}

#[test]
fn hold_tap_toggle_only_and_key_repeat_follow_monotonic_timing() {
    for (duration, release_observable) in [(299, true), (300, true), (0, false)] {
        let mut h = Harness::new();
        let id = UtteranceId::random();
        h.traces.insert(
            id,
            Trace {
                finish: vec![
                    EngineEvent::Committed("held or toggled".into()),
                    EngineEvent::Final,
                ],
                ..Default::default()
            },
        );
        let context = h.context(id, Mode::Quick);
        let effects = h.session.hotkey(
            Hotkey::Down,
            Some(context.clone()),
            10_000,
            release_observable,
        );
        h.execute(effects);
        h.drain();
        if release_observable {
            assert!(
                h.session
                    .hotkey(Hotkey::Down, Some(context), 10_001, true)
                    .is_empty()
            );
            let effects = h.session.hotkey(Hotkey::Up, None, 10_000 + duration, true);
            h.execute(effects);
            h.drain();
        }
        if duration < 300 {
            assert_eq!(h.session.views().next().unwrap().phase, Phase::Recording);
            let effects = h
                .session
                .hotkey(Hotkey::Down, None, 20_000, release_observable);
            h.execute(effects);
            h.drain();
            let effects = h
                .session
                .hotkey(Hotkey::Up, None, 20_010, release_observable);
            h.execute(effects);
            h.drain();
        }
        assert_eq!(h.pastes(), [("held or toggled".into(), false)]);
    }
}

#[test]
fn interactive_edits_survive_late_recognition_and_save_before_refocus_verified_insert() {
    let mut h = Harness::new();
    let id = h.start(Mode::Interactive, "recognizer ending");
    h.engine(id, EngineEvent::Committed("original beginning ".into()));
    h.event(SessionEvent::Ui(UiAction::Edit {
        id,
        text: "my own\nedit".into(),
    }));
    h.engine(id, EngineEvent::Partial("late provisional".into()));
    assert_eq!(
        h.session.views().next().unwrap().edited.as_deref(),
        Some("my own\neditlate provisional")
    );
    h.release(id);
    assert!(h.pastes().is_empty());
    let row = h.history.get(id).unwrap().unwrap();
    assert_eq!(
        row.raw_text.as_deref(),
        Some("original beginning recognizer ending")
    );
    assert_eq!(row.text.as_deref(), Some("my own\neditrecognizer ending"));
    *h.platform.target.borrow_mut() = Some(target("other"));
    h.event(SessionEvent::Ui(UiAction::Copy { id }));
    assert_eq!(h.clipboard, "my own\neditrecognizer ending");
    h.event(SessionEvent::Ui(UiAction::Insert { id }));
    assert!(matches!(
        &h.platform.calls.borrow()[0],
        PlatformCall::Focus(_)
    ));
    assert_eq!(
        h.pastes(),
        [("my own\neditrecognizer ending".into(), false)]
    );
    assert_eq!(h.history.get(id).unwrap().unwrap().raw_text, row.raw_text);
}

#[test]
fn failed_interactive_refocus_and_discard_keep_text_without_paste() {
    let mut h = Harness::new();
    let id = h.start(Mode::Interactive, "original");
    h.release(id);
    *h.platform.target.borrow_mut() = None;
    h.fail_focus = true;
    h.event(SessionEvent::Ui(UiAction::Insert { id }));
    assert!(h.pastes().is_empty());
    h.event(SessionEvent::Ui(UiAction::Discard { id }));
    assert_eq!(
        h.history.get(id).unwrap().unwrap().status,
        Some(UtteranceStatus::Discarded)
    );
}

#[test]
fn interactive_and_insert_last_never_send_enter_even_when_quick_send_is_enabled() {
    let mut h = Harness::new();
    let id = UtteranceId::random();
    let mut context = h.context(id, Mode::Quick);
    context.send = true;
    h.traces.insert(
        id,
        Trace {
            finish: vec![EngineEvent::Committed("opt in".into()), EngineEvent::Final],
            ..Default::default()
        },
    );
    h.event(SessionEvent::HotkeyDown(context));
    h.release(id);
    assert_eq!(h.pastes(), [("opt in".into(), true)]);
    h.session = Session::default();
    h.event(SessionEvent::Ui(UiAction::InsertLast));
    assert_eq!(h.pastes().last(), Some(&("opt in".into(), false)));
    let id = UtteranceId::random();
    let mut context = h.context(id, Mode::Interactive);
    context.send = true;
    h.traces.insert(
        id,
        Trace {
            finish: vec![
                EngineEvent::Committed("interactive".into()),
                EngineEvent::Final,
            ],
            ..Default::default()
        },
    );
    h.event(SessionEvent::HotkeyDown(context));
    h.release(id);
    h.event(SessionEvent::Ui(UiAction::Insert { id }));
    assert_eq!(h.pastes().last(), Some(&("interactive".into(), false)));
}

#[test]
fn finished_entries_are_evicted_and_first_frame_distinguishes_waiting_from_listening() {
    let mut h = Harness::new();
    h.recorder.frames.clear();
    for _ in 0..100 {
        let id = h.start(Mode::Quick, "words");
        assert_eq!(h.session.views().find(|v| v.id == id).unwrap().level, None);
        h.event(SessionEvent::Audio(AudioEvent::Frame(AudioFrame {
            id,
            pcm: vec![0],
            level: 0.1,
        })));
        assert_eq!(
            h.session.views().find(|v| v.id == id).unwrap().level,
            Some(0.1)
        );
        h.release(id);
        assert_eq!(h.session.views().count(), 1);
    }
}

#[test]
fn cancel_while_save_or_focus_acknowledgment_is_delayed_prevents_paste() {
    for focus in [false, true] {
        let mut h = Harness::new();
        let id = h.start(Mode::Quick, "cancel at boundary");
        h.delay_saves = !focus;
        h.delay_focus = focus;
        h.release(id);
        h.event(SessionEvent::Cancel { id });
        if focus {
            for event in std::mem::take(&mut h.delayed_focus) {
                h.event(event);
            }
        } else {
            h.event(SessionEvent::Saved { id });
        }
        assert!(h.pastes().is_empty());
        assert_eq!(
            h.history.get(id).unwrap().unwrap().status,
            Some(UtteranceStatus::Canceled)
        );
    }
}

#[test]
fn release_or_cancel_before_pending_save_ack_never_leaves_capture_running() {
    for cancel in [false, true] {
        let mut h = Harness::new();
        h.delay_saves = true;
        let id = h.start(Mode::Quick, "tap before disk ack");
        if cancel {
            h.event(SessionEvent::Cancel { id });
        } else {
            h.release(id);
        }
        assert!(h.recorder.calls.is_empty());
        h.event(SessionEvent::Saved { id });
        h.event(SessionEvent::Saved { id });
        assert_eq!(
            h.history.get(id).unwrap().unwrap().status,
            Some(if cancel {
                UtteranceStatus::Canceled
            } else {
                UtteranceStatus::Done
            })
        );
        if cancel {
            assert!(h.recorder.calls.is_empty());
            assert!(h.pastes().is_empty());
        } else {
            assert_eq!(h.recorder.calls.len(), 2);
            assert_eq!(h.pastes(), [("tap before disk ack".into(), false)]);
        }
    }
}

#[test]
fn failed_or_inflight_edits_cannot_insert_unsaved_text() {
    let mut h = Harness::new();
    let id = h.start(Mode::Interactive, "original");
    h.release(id);
    h.fail_edit = true;
    h.event(SessionEvent::Ui(UiAction::Edit {
        id,
        text: "first edit".into(),
    }));
    h.event(SessionEvent::Ui(UiAction::Insert { id }));
    assert!(h.pastes().is_empty());
    assert_eq!(
        h.history.get(id).unwrap().unwrap().text.as_deref(),
        Some("original")
    );
    h.fail_edit = false;
    h.delay_edit = true;
    h.event(SessionEvent::Ui(UiAction::Insert { id }));
    h.event(SessionEvent::Ui(UiAction::Edit {
        id,
        text: "newest edit".into(),
    }));
    h.event(SessionEvent::EditSaved { id });
    assert!(h.pastes().is_empty());
    assert_eq!(
        h.history.get(id).unwrap().unwrap().text.as_deref(),
        Some("newest edit")
    );
    h.event(SessionEvent::EditSaved { id });
    assert_eq!(h.pastes(), [("newest edit".into(), false)]);
    assert_eq!(
        h.history.get(id).unwrap().unwrap().raw_text.as_deref(),
        Some("original")
    );
}

#[test]
fn worker_failure_while_finishing_and_replay_failure_are_bounded() {
    let mut h = Harness::new();
    h.delay_finish = true;
    let id = h.start(Mode::Quick, "old final");
    h.release(id);
    h.delay_saves = true;
    h.engine(id, EngineEvent::Error("crashed while flushing".into()));
    let replay = *h
        .held_saves
        .iter()
        .find(|candidate| **candidate != id)
        .unwrap();
    assert_ne!(replay, id);
    h.traces.insert(
        replay,
        Trace {
            finish: vec![EngineEvent::Error("restart also failed".into())],
            ..Default::default()
        },
    );
    h.delay_saves = false;
    h.event(SessionEvent::Saved { id });
    h.event(SessionEvent::Saved { id: replay });
    assert_eq!(
        h.history.get(replay).unwrap().unwrap().status,
        Some(UtteranceStatus::Failed)
    );
    assert!(h.pastes().is_empty());
    assert_eq!(
        h.effects
            .iter()
            .filter(|e| matches!(e, Effect::RecognizeFile { .. }))
            .count(),
        1
    );
    for output in std::mem::take(&mut h.held_outputs) {
        h.event(SessionEvent::Engine(output));
    }
    assert!(h.pastes().is_empty());
}

#[test]
fn premature_final_and_late_audio_are_ignored_until_capture_has_drained() {
    let mut h = Harness::new();
    let id = h.start(Mode::Quick, "actual final");
    h.engine(id, EngineEvent::Final);
    assert!(h.history.get(id).unwrap().unwrap().raw_text.is_none());
    h.release(id);
    let prior = h.effects.len();
    h.event(SessionEvent::Audio(AudioEvent::Frame(AudioFrame {
        id,
        pcm: vec![4],
        level: 1.0,
    })));
    assert_eq!(h.effects.len(), prior);
}

#[test]
fn paste_adapter_rechecks_focus_after_core_authorizes_it() {
    let mut h = Harness::new();
    h.delay_focus = true;
    let id = h.start(Mode::Quick, "saved regardless");
    h.release(id);
    let event = h.delayed_focus.pop().unwrap();
    *h.platform.target.borrow_mut() = Some(target("different"));
    h.event(event);
    assert!(h.pastes().is_empty());
    assert_eq!(
        h.history.get(id).unwrap().unwrap().text.as_deref(),
        Some("saved regardless")
    );
    assert_eq!(h.clipboard, "old clipboard");
}

#[test]
fn edited_provisional_tail_is_protected_while_future_chunks_keep_streaming() {
    let mut h = Harness::new();
    let id = h.start(Mode::Interactive, " ending");
    h.engine(id, EngineEvent::Committed("café ".into()));
    h.engine(id, EngineEvent::Partial("wrong".into()));
    h.event(SessionEvent::Ui(UiAction::Edit {
        id,
        text: "café corrected".into(),
    }));
    h.engine(id, EngineEvent::Partial("rewritten provisional".into()));
    assert_eq!(
        h.session.views().next().unwrap().edited.as_deref(),
        Some("café corrected")
    );
    h.engine(id, EngineEvent::Committed("recognized instead".into()));
    h.engine(id, EngineEvent::Partial(" more".into()));
    assert_eq!(
        h.session.views().next().unwrap().edited.as_deref(),
        Some("café corrected more")
    );
    h.release(id);
    let row = h.history.get(id).unwrap().unwrap();
    assert_eq!(
        row.raw_text.as_deref(),
        Some("café recognized instead ending")
    );
    assert_eq!(row.text.as_deref(), Some("café corrected ending"));
}

#[test]
fn committed_edits_leave_provisional_updates_live_and_multibyte_deletions_safe() {
    let mut h = Harness::new();
    let id = h.start(Mode::Interactive, " tail");
    h.engine(id, EngineEvent::Committed("café café ".into()));
    h.engine(id, EngineEvent::Partial("p".into()));
    h.event(SessionEvent::Ui(UiAction::Edit {
        id,
        text: "café p".into(),
    }));
    h.engine(id, EngineEvent::Partial("provisional".into()));
    assert_eq!(
        h.session.views().next().unwrap().edited.as_deref(),
        Some("café provisional")
    );
    h.release(id);
    assert_eq!(
        h.history.get(id).unwrap().unwrap().text.as_deref(),
        Some("café  tail")
    );
}

#[test]
fn clipboard_mode_copies_saved_bytes_without_target_or_enter() {
    let mut h = Harness::new();
    *h.platform.target.borrow_mut() = None;
    let id = h.start(Mode::Quick, "  fallback\nno no");
    h.release(id);
    assert_eq!(h.clipboard, "  fallback\nno no");
    assert!(h.pastes().is_empty());
    assert!(h.notices.contains(&"Copied".into()));
}

#[test]
fn replay_reducer_is_deterministic_with_host_supplied_identity() {
    let id = UtteranceId::random();
    let recognition_id = UtteranceId::random();
    let context = StartContext {
        id,
        started_at: 0,
        engine: "fake".into(),
        target: Some(target("1")),
        mode: Mode::Quick,
        send: false,
    };
    let mut first = Session::default();
    let mut second = Session::default();
    for event in [
        SessionEvent::HotkeyDown(context),
        SessionEvent::Saved { id },
        SessionEvent::Engine(EngineOutput {
            id,
            event: EngineEvent::Error("synthetic crash".into()),
        }),
        SessionEvent::HotkeyUp { id },
        SessionEvent::Audio(AudioEvent::Stopped {
            id,
            duration_ms: 500,
        }),
        SessionEvent::ReplayReady { id, recognition_id },
    ] {
        assert_eq!(first.reduce(event.clone()), second.reduce(event));
    }
}
