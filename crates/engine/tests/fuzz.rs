use std::io::{self, Read};

use said_core::{EngineEvent, UtteranceId, WorkerRequest, WorkerResponse};
use said_engine::protocol::{read_request, read_response, write_request, write_response};

struct Fragments<'a>(&'a [u8]);
impl Read for Fragments<'_> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        let length = destination.len().min(self.0.len()).min(3);
        destination[..length].copy_from_slice(&self.0[..length]);
        self.0 = &self.0[length..];
        Ok(length)
    }
}

#[test]
fn mutation_fuzzing_request_and_response_decoders() {
    let mut state = 0x8123_6745_09ab_cdef_u64;
    let mut random = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let id: UtteranceId = serde_json::from_str("\"00000000-0000-4000-8000-000000000001\"").unwrap();
    let mut corpus = vec![vec![], vec![255; 4], br#"\x00{"type":"bogus"}"#.to_vec()];
    for message in [
        WorkerRequest::Start {
            id,
            vocabulary: vec!["no no café\n".into()],
        },
        WorkerRequest::Audio { id, sequence: 0 },
        WorkerRequest::Finish { id },
        WorkerRequest::Cancel { id },
    ] {
        let mut bytes = vec![];
        write_request(
            &mut bytes,
            &message,
            matches!(message, WorkerRequest::Audio { .. }).then_some(&[i16::MIN, 0, i16::MAX][..]),
        )
        .unwrap();
        corpus.push(bytes);
    }
    for response in [
        WorkerResponse::Event {
            id,
            event: EngineEvent::Partial("  café\tno no".into()),
        },
        WorkerResponse::Event {
            id,
            event: EngineEvent::Committed("no no\n".into()),
        },
        WorkerResponse::Event {
            id,
            event: EngineEvent::Final,
        },
        WorkerResponse::Event {
            id,
            event: EngineEvent::Error("synthetic".into()),
        },
        WorkerResponse::AudioAck { id, sequence: 0 },
    ] {
        let mut bytes = vec![];
        write_response(&mut bytes, &response).unwrap();
        corpus.push(bytes);
    }
    for _ in 0..50_000 {
        let mut bytes = corpus[random() as usize % corpus.len()].clone();
        for _ in 0..(random() % 8 + 1) {
            match random() % 4 {
                0 if !bytes.is_empty() => {
                    let index = random() as usize % bytes.len();
                    bytes[index] ^= random() as u8;
                }
                1 => bytes.truncate(random() as usize % (bytes.len() + 1)),
                2 => bytes.push(random() as u8),
                _ if !bytes.is_empty() => {
                    let index = random() as usize % bytes.len();
                    bytes.remove(index);
                }
                _ => {}
            }
        }
        let _ = read_request(&mut Fragments(&bytes));
        let _ = read_response(&mut Fragments(&bytes));
    }
}
