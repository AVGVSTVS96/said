use std::io::{self, Cursor};

use said_core::{EngineEvent, UtteranceId, WorkerRequest, WorkerResponse};
use said_engine::protocol::*;

#[test]
fn wire_matches_an_independent_little_endian_fixture() {
    let json = br#"{"type":"audio","id":"00000000-0000-4000-8000-000000000001","sequence":0}"#;
    let mut fixture = vec![json.len() as u8, 0, 0, 0];
    fixture.extend(json);
    fixture.extend([10, 0, 0, 0, 0, 128, 255, 255, 0, 0, 1, 0, 255, 127]);
    let frame = read_request(&mut &fixture[..]).unwrap().unwrap();
    assert_eq!(frame.pcm, Some(vec![i16::MIN, -1, 0, 1, i16::MAX]));
    let mut encoded = vec![];
    write_request(&mut encoded, &frame.message, frame.pcm.as_deref()).unwrap();
    assert_eq!(encoded, fixture);
}

#[test]
fn requests_round_trip_on_one_stream_without_changing_pcm() {
    let id = UtteranceId::random();
    let samples = vec![i16::MIN, -1, 0, 1, i16::MAX];
    let frames = vec![
        RequestFrame {
            message: WorkerRequest::Start {
                id,
                vocabulary: vec!["synthetic_term".into()],
            },
            pcm: None,
        },
        RequestFrame {
            message: WorkerRequest::Audio { id, sequence: 0 },
            pcm: Some(samples),
        },
        RequestFrame {
            message: WorkerRequest::Audio { id, sequence: 1 },
            pcm: Some(vec![]),
        },
        RequestFrame {
            message: WorkerRequest::Finish { id },
            pcm: None,
        },
        RequestFrame {
            message: WorkerRequest::Cancel { id },
            pcm: None,
        },
    ];
    let mut stream = Cursor::new(vec![]);
    for frame in &frames {
        write_request(&mut stream, &frame.message, frame.pcm.as_deref()).unwrap();
    }
    stream.set_position(0);
    for frame in frames {
        assert_eq!(read_request(&mut stream).unwrap(), Some(frame));
    }
    assert_eq!(read_request(&mut stream).unwrap(), None);
}

#[test]
fn responses_round_trip_with_literal_text_and_acknowledgments() {
    let id = UtteranceId::random();
    let mut messages: Vec<_> = [
        EngineEvent::Partial("  um, um actually—\n".into()),
        EngineEvent::Committed("no no  café\t".into()),
        EngineEvent::Final,
        EngineEvent::Error("synthetic failure".into()),
    ]
    .into_iter()
    .map(|event| WorkerResponse::Event { id, event })
    .collect();
    messages.push(WorkerResponse::AudioAck { id, sequence: 41 });
    let mut stream = Cursor::new(vec![]);
    for message in &messages {
        write_response(&mut stream, message).unwrap();
    }
    stream.set_position(0);
    for message in messages {
        assert_eq!(read_response(&mut stream).unwrap(), Some(message));
    }
    assert_eq!(read_response(&mut stream).unwrap(), None);
}

#[test]
fn every_truncated_audio_frame_is_an_error_except_clean_stream_eof() {
    let mut bytes = vec![];
    write_request(
        &mut bytes,
        &WorkerRequest::Audio {
            id: UtteranceId::random(),
            sequence: 0,
        },
        Some(&[1, -2]),
    )
    .unwrap();
    for length in 1..bytes.len() {
        assert!(
            matches!(read_request(&mut &bytes[..length]), Err(ProtocolError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof),
            "length {length}"
        );
    }
    assert_eq!(read_request(&mut &[][..]).unwrap(), None);
}

#[test]
fn oversized_frames_are_rejected_from_the_prefix() {
    let prefix = ((MAX_JSON_BYTES + 1) as u32).to_le_bytes();
    assert!(matches!(
        read_request(&mut &prefix[..]),
        Err(ProtocolError::Oversized { .. })
    ));
    let message = WorkerRequest::Audio {
        id: UtteranceId::random(),
        sequence: 0,
    };
    let json = serde_json::to_vec(&message).unwrap();
    let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
    bytes.extend(json);
    bytes.extend(((MAX_PCM_BYTES + 1) as u32).to_le_bytes());
    assert!(matches!(
        read_request(&mut &bytes[..]),
        Err(ProtocolError::Oversized { .. })
    ));
}

#[test]
fn malformed_payloads_fail_without_writing_partial_frames() {
    let id = UtteranceId::random();
    let mut bytes = vec![];
    assert!(matches!(
        write_request(&mut bytes, &WorkerRequest::Audio { id, sequence: 0 }, None),
        Err(ProtocolError::AudioPayload)
    ));
    assert!(matches!(
        write_request(&mut bytes, &WorkerRequest::Finish { id }, Some(&[1])),
        Err(ProtocolError::AudioPayload)
    ));
    assert!(bytes.is_empty());
    let json = serde_json::to_vec(&WorkerRequest::Audio { id, sequence: 0 }).unwrap();
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(json);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0);
    assert!(matches!(
        read_request(&mut &bytes[..]),
        Err(ProtocolError::OddPcm)
    ));
    assert!(matches!(
        read_request(&mut &[1, 0, 0, 0, b'{'][..]),
        Err(ProtocolError::Json(_))
    ));
}
