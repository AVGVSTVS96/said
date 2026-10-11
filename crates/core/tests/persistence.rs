use std::{fs, path::PathBuf};

use said_core::{history::SqliteHistory, *};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/core-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn pending(at: i64) -> Utterance {
    let id = UtteranceId::random();
    Utterance {
        id,
        audio_id: id,
        started_at: at,
        duration_ms: 0,
        engine: "fake".into(),
        raw_text: None,
        text: None,
        status: None,
        pasted_to: None,
        pasted_at: None,
    }
}
fn finished(mut row: Utterance, text: &str, status: UtteranceStatus) -> Utterance {
    row.raw_text = Some(text.into());
    row.text = Some(text.into());
    row.status = Some(status);
    row.duration_ms = 500;
    row
}

#[test]
fn sqlite_reopen_recovers_pending_rows_once_and_keeps_literal_bytes_and_fts_edits() {
    let directory = Directory::new();
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    let row = pending(1);
    history.insert(&row).unwrap();
    fs::write(history.audio_path(row.audio_id), b"synthetic wav fixture").unwrap();
    drop(history);
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    assert_eq!(
        history.recover().unwrap().as_slice(),
        std::slice::from_ref(&row)
    );
    let raw = "  no no um café\n e\u{301}\0 end  ";
    let recovered = finished(row.clone(), raw, UtteranceStatus::Recovered);
    history.finish(&recovered).unwrap();
    assert!(history.finish(&recovered).is_err());
    history.edit(row.id, "edited identifier --flag\n").unwrap();
    assert_eq!(history.search("\"no no\"").unwrap().len(), 1);
    assert_eq!(history.search("identifier").unwrap().len(), 1);
    assert!(history.search("identifier AND missing").unwrap().is_empty());
    assert!(history.search("\"").is_err());
    drop(history);
    let history = SqliteHistory::open(&directory.0).unwrap();
    let saved = history.last().unwrap().unwrap();
    assert_eq!(saved.raw_text.unwrap().as_bytes(), raw.as_bytes());
    assert_eq!(saved.text.as_deref(), Some("edited identifier --flag\n"));
    assert!(history.recover().unwrap().is_empty());
    assert!(history.audio_path(row.audio_id).exists());
}

#[test]
fn deletion_removes_search_and_only_last_audio_reference_and_restart_finishes_interrupted_unlink() {
    let directory = Directory::new();
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    let original = finished(pending(1), "original synthetic", UtteranceStatus::Done);
    let mut retry = finished(pending(2), "retry synthetic", UtteranceStatus::Done);
    retry.audio_id = original.audio_id;
    history.insert(&original).unwrap();
    history.insert(&retry).unwrap();
    let audio = history.audio_path(original.audio_id);
    fs::write(&audio, b"synthetic wav fixture").unwrap();
    assert_eq!(history.delete(original.id).unwrap(), None);
    assert!(audio.exists());
    assert!(history.search("original").unwrap().is_empty());
    assert_eq!(history.search("retry").unwrap().len(), 1);
    assert_eq!(history.delete(retry.id).unwrap(), Some(original.audio_id));
    assert!(!audio.exists());
    assert!(history.search("synthetic").unwrap().is_empty());
    fs::write(&audio, b"left after database commit before unlink").unwrap();
    drop(history);
    let history = SqliteHistory::open(&directory.0).unwrap();
    assert!(!audio.exists());
    assert!(history.last().unwrap().is_none());
}

#[test]
fn history_order_delivery_discard_and_pending_guards_survive_reopen() {
    let directory = Directory::new();
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    let first = finished(pending(1), "first", UtteranceStatus::Done);
    let second = finished(pending(2), "second", UtteranceStatus::Done);
    let third = pending(3);
    for row in [&first, &second, &third] {
        history.insert(row).unwrap();
    }
    let target = Target {
        app_id: "synthetic.editor".into(),
        window_id: "1".into(),
        name: "Scratch".into(),
    };
    history.record_delivery(second.id, &target, 1000).unwrap();
    history
        .set_status(first.id, UtteranceStatus::Discarded)
        .unwrap();
    assert!(history.edit(third.id, "edit").is_err());
    assert!(history.record_delivery(third.id, &target, 1000).is_err());
    assert!(
        history
            .set_status(third.id, UtteranceStatus::Discarded)
            .is_err()
    );
    let mut changed = finished(third.clone(), "text", UtteranceStatus::Done);
    changed.audio_id = UtteranceId::random();
    assert!(history.finish(&changed).is_err());
    let mut changed = finished(third.clone(), "text", UtteranceStatus::Done);
    changed.engine = "other".into();
    assert!(history.finish(&changed).is_err());
    drop(history);
    let history = SqliteHistory::open(&directory.0).unwrap();
    assert_eq!(
        history
            .search("")
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        [third.id, second.id, first.id]
    );
    assert_eq!(history.last().unwrap().unwrap().id, second.id);
    let saved = history.get(second.id).unwrap().unwrap();
    assert_eq!(saved.pasted_to, Some(target));
    assert_eq!(saved.pasted_at, Some(1000));
    assert_eq!(
        history.get(first.id).unwrap().unwrap().status,
        Some(UtteranceStatus::Discarded)
    );
}

#[test]
fn invalid_rows_and_future_schema_are_rejected() {
    let directory = Directory::new();
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    let mut row = pending(0);
    row.status = Some(UtteranceStatus::Done);
    assert!(history.insert(&row).is_err());
    let mut row = pending(0);
    row.duration_ms = u64::MAX;
    assert!(history.insert(&row).is_err());
    let row = pending(i64::MAX);
    assert!(history.insert(&row).is_err());
    drop(history);
    let db = rusqlite::Connection::open(directory.0.join("history.sqlite")).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    assert!(SqliteHistory::open(&directory.0).is_err());
}

#[test]
fn settings_round_trip_atomic_save_defaults_and_invalid_file() {
    let directory = Directory::new();
    let path = directory.0.join("config/settings.toml");
    let defaults = Settings::load(&path, "manifest-engine").unwrap();
    assert_eq!(defaults.engine, "manifest-engine");
    assert!(!defaults.vocabulary);
    assert!(defaults.apps.values().all(|app| !app.send));
    let mut settings = defaults;
    settings.microphone = Some("synthetic-mic".into());
    settings.vocabulary = true;
    settings.apps.insert(
        "synthetic.editor".into(),
        AppSettings {
            paste: "ctrl+v".into(),
            send: true,
        },
    );
    settings.save(&path).unwrap();
    assert_eq!(Settings::load(&path, "other-default").unwrap(), settings);
    settings.engine = "new-engine".into();
    settings.save(&path).unwrap();
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    assert_eq!(Settings::load(&path, "unused").unwrap(), settings);
    fs::write(&path, "unknown_setting = true").unwrap();
    assert!(Settings::load(&path, "manifest-engine").is_err());
    fs::write(&path, "hotkey = \"\"").unwrap();
    assert!(Settings::load(&path, "manifest-engine").is_err());
    fs::write(&path, "vocabulary = true").unwrap();
    assert_eq!(
        Settings::load(&path, "manifest-engine").unwrap().engine,
        "manifest-engine"
    );
}

#[cfg(unix)]
#[test]
fn personal_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let directory = Directory::new();
    let _history = SqliteHistory::open(&directory.0).unwrap();
    let settings = directory.0.join("settings.toml");
    Settings::defaults("fake").save(&settings).unwrap();
    assert_eq!(
        fs::metadata(&directory.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(directory.0.join("audio"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for path in [settings, directory.0.join("history.sqlite")] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn restart_recovery_and_insert_last_run_against_sqlite_and_fake_adapters() {
    use said_engine::{
        Engine,
        fake::{FakeEngine, Trace},
    };
    use said_platform::{
        Platform,
        fake::{FakePlatform, PlatformCall},
    };
    use std::collections::VecDeque;
    fn run(
        session: &mut Session,
        event: SessionEvent,
        history: &mut SqliteHistory,
        platform: &FakePlatform,
    ) {
        let mut events = VecDeque::from([event]);
        while let Some(event) = events.pop_front() {
            for effect in session.reduce(event) {
                match effect {
                    Effect::AllocateReplay { id } => events.push_back(SessionEvent::ReplayReady {
                        id,
                        recognition_id: UtteranceId::random(),
                    }),
                    Effect::Save { utterance } => {
                        if history.get(utterance.id).unwrap().is_some() {
                            history.finish(&utterance).unwrap();
                        } else {
                            history.insert(&utterance).unwrap();
                        }
                        events.push_back(SessionEvent::Saved { id: utterance.id });
                    }
                    Effect::RecognizeFile { id, audio_id, .. } => {
                        assert!(history.audio_path(audio_id).exists());
                        let (mut engine, output) = FakeEngine::new(
                            id,
                            Trace {
                                finish: vec![
                                    EngineEvent::Committed("  no no café\n".into()),
                                    EngineEvent::Final,
                                ],
                                ..Default::default()
                            },
                        );
                        engine.start(&[]).unwrap();
                        engine.feed(&[1, -1]);
                        engine.finish();
                        events.push_back(SessionEvent::Audio(AudioEvent::Stopped {
                            id,
                            duration_ms: 500,
                        }));
                        events.extend(output.try_iter().map(SessionEvent::Engine));
                    }
                    Effect::LoadLast => events.push_back(SessionEvent::InsertLast {
                        utterance: history.last().unwrap(),
                    }),
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
                        assert_eq!(
                            history.get(id).unwrap().unwrap().raw_text.as_deref(),
                            Some(text.as_str())
                        );
                        platform.paste(&target, &text, send).unwrap();
                        events.push_back(SessionEvent::Pasted { id, at: 1000 });
                    }
                    Effect::RecordDelivery { id, target, at } => {
                        history.record_delivery(id, &target, at).unwrap()
                    }
                    Effect::Show { .. } | Effect::Notify { .. } => {}
                    other => panic!("unexpected recovery effect {other:?}"),
                }
            }
        }
    }
    let directory = Directory::new();
    let row = pending(1);
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    history.insert(&row).unwrap();
    fs::write(history.audio_path(row.audio_id), b"synthetic wav fixture").unwrap();
    drop(history);
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    let mut session = Session::default();
    let platform = FakePlatform::new(Some(Target {
        app_id: "synthetic.editor".into(),
        window_id: "1".into(),
        name: "Scratch".into(),
    }));
    let pending = history.recover().unwrap().pop().unwrap();
    run(
        &mut session,
        SessionEvent::Recover { utterance: pending },
        &mut history,
        &platform,
    );
    assert!(platform.calls.borrow().is_empty());
    let recovered = history.last().unwrap().unwrap();
    assert_eq!(recovered.status, Some(UtteranceStatus::Recovered));
    assert_eq!(recovered.duration_ms, 500);
    assert_eq!(recovered.audio_id, row.audio_id);
    drop(history);
    let mut history = SqliteHistory::open(&directory.0).unwrap();
    let mut session = Session::default();
    run(
        &mut session,
        SessionEvent::Ui(UiAction::InsertLast),
        &mut history,
        &platform,
    );
    assert!(
        matches!(&platform.calls.borrow()[0],PlatformCall::Paste { text,send:false,.. } if text.as_bytes()==b"  no no caf\xc3\xa9\n")
    );
    assert!(
        history
            .get(recovered.id)
            .unwrap()
            .unwrap()
            .pasted_at
            .is_some()
    );
}
