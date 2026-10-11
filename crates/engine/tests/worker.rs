use std::collections::VecDeque;

use said_core::{EngineEvent, UtteranceId, WorkerRequest, WorkerResponse};
use said_engine::{
    fake::{FakeEngine, Trace},
    protocol, worker,
};

#[test]
fn commits_are_literal_and_every_consumed_frame_is_acknowledged() {
    let id = UtteranceId::random();
    let trace = Trace {
        start: vec![EngineEvent::Partial("wrong".into())],
        feeds: VecDeque::from([
            vec![EngineEvent::Partial("no".into())],
            vec![EngineEvent::Committed("no no ".into())],
        ]),
        finish: vec![
            EngineEvent::Committed("no  café\t\n".into()),
            EngineEvent::Final,
        ],
    };
    let mut input = vec![];
    for (message, pcm) in [
        (
            WorkerRequest::Start {
                id,
                vocabulary: vec!["literal".into()],
            },
            None,
        ),
        (WorkerRequest::Audio { id, sequence: 0 }, Some(vec![1, -2])),
        (WorkerRequest::Audio { id, sequence: 1 }, Some(vec![])),
        (WorkerRequest::Finish { id }, None),
    ] {
        protocol::write_request(&mut input, &message, pcm.as_deref()).unwrap();
    }
    let mut output = vec![];
    worker::run(&input[..], &mut output, |id| {
        let (engine, events) = FakeEngine::new(id, trace.clone());
        Ok((Box::new(engine), events))
    })
    .unwrap();
    let mut reader = &output[..];
    let mut text = String::new();
    let mut acks = vec![];
    let mut final_count = 0;
    while let Some(response) = protocol::read_response(&mut reader).unwrap() {
        match response {
            WorkerResponse::Event { id: current, event } => {
                assert_eq!(id, current);
                match event {
                    EngineEvent::Committed(chunk) => text.push_str(&chunk),
                    EngineEvent::Final => final_count += 1,
                    _ => {}
                }
            }
            WorkerResponse::AudioAck { sequence, .. } => acks.push(sequence),
        }
    }
    assert_eq!(text, "no no no  café\t\n");
    assert_eq!(acks, [0, 1]);
    assert_eq!(final_count, 1);
}

#[test]
fn worker_rejects_wrong_identity_order_and_events_after_final() {
    let id = UtteranceId::random();
    for message in [
        WorkerRequest::Audio { id, sequence: 0 },
        WorkerRequest::Finish { id },
        WorkerRequest::Cancel { id },
    ] {
        let mut input = vec![];
        let pcm = matches!(message, WorkerRequest::Audio { .. }).then_some(&[][..]);
        protocol::write_request(&mut input, &message, pcm).unwrap();
        assert!(worker::run(&input[..], vec![], |_| panic!("factory must not run")).is_err());
    }
    for message in [
        WorkerRequest::Audio { id, sequence: 1 },
        WorkerRequest::Finish {
            id: UtteranceId::random(),
        },
        WorkerRequest::Start {
            id,
            vocabulary: vec![],
        },
    ] {
        let mut input = vec![];
        protocol::write_request(
            &mut input,
            &WorkerRequest::Start {
                id,
                vocabulary: vec![],
            },
            None,
        )
        .unwrap();
        let pcm = matches!(message, WorkerRequest::Audio { .. }).then_some(&[][..]);
        protocol::write_request(&mut input, &message, pcm).unwrap();
        assert!(
            worker::run(&input[..], vec![], |id| {
                let (engine, events) = FakeEngine::new(id, Trace::default());
                Ok((Box::new(engine), events))
            })
            .is_err()
        );
    }
    let mut input = vec![];
    protocol::write_request(
        &mut input,
        &WorkerRequest::Start {
            id,
            vocabulary: vec![],
        },
        None,
    )
    .unwrap();
    protocol::write_request(&mut input, &WorkerRequest::Finish { id }, None).unwrap();
    assert!(
        worker::run(&input[..], vec![], |id| {
            let (engine, events) = FakeEngine::new(
                id,
                Trace {
                    finish: vec![EngineEvent::Final, EngineEvent::Committed("late".into())],
                    ..Trace::default()
                },
            );
            Ok((Box::new(engine), events))
        })
        .is_err()
    );
}

#[test]
fn fake_records_exact_inputs_and_cancel_suppresses_late_text() {
    use said_engine::{Engine, fake::EngineCall};
    let (mut fake, events) = FakeEngine::new(
        UtteranceId::random(),
        Trace {
            finish: vec![EngineEvent::Committed("late".into()), EngineEvent::Final],
            ..Trace::default()
        },
    );
    fake.start(&["  spaces\t".into()]).unwrap();
    fake.feed(&[i16::MIN, i16::MAX]);
    fake.cancel();
    fake.finish();
    fake.feed(&[0]);
    assert!(events.is_empty());
    assert_eq!(
        fake.calls,
        [
            EngineCall::Start(vec!["  spaces\t".into()]),
            EngineCall::Feed(vec![i16::MIN, i16::MAX]),
            EngineCall::Cancel
        ]
    );
}
