use std::collections::VecDeque;

use voice_audio::{Recorder, fake::FakeRecorder};
use voice_core::{fake::FakeHistory, *};
use voice_engine::{
    Engine,
    fake::{FakeEngine, Trace},
};
use voice_platform::{
    Platform,
    fake::{FakePlatform, PlatformCall},
};

#[test]
fn hotkey_trace_saves_literal_commits_before_pasting_against_fakes() {
    let id = UtteranceId::random();
    let target = Target {
        app_id: "synthetic.editor".into(),
        window_id: "1".into(),
        name: "Scratch editor".into(),
    };
    let platform = FakePlatform::new(Some(target));
    let mut recorder = FakeRecorder::new(vec![(vec![1, -1], 0.5), (vec![2, -2], 0.5)], 500);
    let trace = Trace {
        feeds: VecDeque::from([
            vec![
                EngineEvent::Partial("provisional".into()),
                EngineEvent::Committed("  um um ".into()),
            ],
            vec![EngineEvent::Partial("actually yes".into())],
        ]),
        finish: vec![
            EngineEvent::Committed("actually no, no café\n".into()),
            EngineEvent::Final,
        ],
        ..Default::default()
    };
    let (mut engine, output) = FakeEngine::new(id, trace);
    let mut session = Session::default();
    let mut history = FakeHistory::default();
    let mut actions = vec![];
    let mut events = VecDeque::new();
    let mut capture = None;
    let hotkeys = platform.hotkeys();
    platform.emit(Hotkey::Down);
    assert_eq!(hotkeys.recv().unwrap(), Hotkey::Down);
    events.push_back(SessionEvent::HotkeyDown(StartContext {
        id,
        started_at: 0,
        engine: "fake".into(),
        target: platform.focused(),
        mode: Mode::Quick,
        vocabulary: vec![],
        send: false,
    }));
    let mut released = false;
    loop {
        while let Some(event) = events.pop_front() {
            for effect in session.reduce(event) {
                match effect {
                    Effect::StartEngine { vocabulary, .. } => engine.start(&vocabulary).unwrap(),
                    Effect::StartCapture { id } => {
                        capture = Some(recorder.start(id, None).unwrap())
                    }
                    Effect::FeedEngine { pcm, .. } => engine.feed(&pcm),
                    Effect::StopCapture { .. } => recorder.stop().unwrap(),
                    Effect::FinishEngine { .. } => engine.finish(),
                    Effect::Save { utterance } => {
                        assert!(platform.calls.borrow().is_empty());
                        history.insert(&utterance).unwrap();
                        actions.push("save");
                        events.push_back(SessionEvent::Saved { id: utterance.id });
                    }
                    Effect::CheckFocus { id } => events.push_back(SessionEvent::FocusChecked {
                        id,
                        target: platform.focused(),
                    }),
                    Effect::Paste {
                        id,
                        target,
                        text,
                        send,
                    } => {
                        let saved = history.get(id).unwrap().unwrap();
                        assert_eq!(saved.raw_text.as_deref(), Some(text.as_str()));
                        platform.paste(&target, &text, send).unwrap();
                        actions.push("paste");
                        events.push_back(SessionEvent::Pasted { id, at: 500 });
                    }
                    Effect::RecordDelivery { id, target, at } => {
                        history.record_delivery(id, &target, at).unwrap()
                    }
                    Effect::Show { .. } => {}
                    effect => panic!("unexpected effect: {effect:?}"),
                }
                if let Some(receiver) = &capture {
                    events.extend(receiver.try_iter().map(SessionEvent::Audio));
                }
                events.extend(output.try_iter().map(SessionEvent::Engine));
            }
        }
        if released {
            break;
        }
        assert_eq!(session.views().next().unwrap().committed, "  um um ");
        assert_eq!(session.views().next().unwrap().partial, "actually yes");
        assert_eq!(session.views().next().unwrap().level, Some(0.5));
        assert!(history.rows.is_empty());
        platform.emit(Hotkey::Up);
        assert_eq!(hotkeys.recv().unwrap(), Hotkey::Up);
        events.push_back(SessionEvent::HotkeyUp { id });
        released = true;
    }
    let expected = "  um um actually no, no café\n";
    assert_eq!(actions, ["save", "paste"]);
    let row = history.get(id).unwrap().unwrap();
    assert_eq!(row.raw_text.as_deref(), Some(expected));
    assert_eq!(row.text.as_deref(), Some(expected));
    assert_eq!(row.duration_ms, 500);
    assert_eq!(row.status, Some(UtteranceStatus::Done));
    assert!(
        matches!(&platform.calls.borrow()[0], PlatformCall::Paste { text, send: false, .. } if text.as_bytes() == expected.as_bytes())
    );
    assert!(
        session
            .reduce(SessionEvent::Engine(EngineOutput {
                id,
                event: EngineEvent::Committed("late".into())
            }))
            .is_empty()
    );
    assert!(
        session
            .reduce(SessionEvent::Engine(EngineOutput {
                id: UtteranceId::random(),
                event: EngineEvent::Final
            }))
            .is_empty()
    );
}

#[test]
fn save_acknowledgment_and_original_focus_are_required_for_paste() {
    let id = UtteranceId::random();
    let original = Target {
        app_id: "synthetic.editor".into(),
        window_id: "1".into(),
        name: "Scratch".into(),
    };
    let mut session = Session::default();
    session.reduce(SessionEvent::HotkeyDown(StartContext {
        id,
        started_at: 0,
        engine: "fake".into(),
        target: Some(original.clone()),
        mode: Mode::Quick,
        vocabulary: vec![],
        send: false,
    }));
    session.reduce(SessionEvent::Engine(EngineOutput {
        id,
        event: EngineEvent::Committed("no no".into()),
    }));
    session.reduce(SessionEvent::HotkeyUp { id });
    let effects = session.reduce(SessionEvent::Engine(EngineOutput {
        id,
        event: EngineEvent::Final,
    }));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Save { .. }))
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Paste { .. } | Effect::CheckFocus { .. }))
    );
    assert!(
        session
            .reduce(SessionEvent::FocusChecked {
                id,
                target: Some(original.clone())
            })
            .is_empty()
    );
    let effects = session.reduce(SessionEvent::Saved { id });
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CheckFocus { .. }))
    );
    let moved = Target {
        window_id: "2".into(),
        ..original
    };
    let effects = session.reduce(SessionEvent::FocusChecked {
        id,
        target: Some(moved),
    });
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Paste { .. }))
    );
    assert_eq!(session.views().next().unwrap().phase, Phase::Saved);
}
