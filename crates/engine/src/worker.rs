use std::io::{Read, Write};

use crossbeam_channel::Receiver;
use said_core::{EngineEvent, EngineOutput, Result, UtteranceId, WorkerRequest, WorkerResponse};

use crate::{Engine, protocol};

type Active = (UtteranceId, Box<dyn Engine>, Receiver<EngineOutput>, u64);

pub type Adapter = (Box<dyn Engine>, Receiver<EngineOutput>);

pub fn run(
    mut input: impl Read,
    mut output: impl Write,
    mut create: impl FnMut(UtteranceId) -> Result<Adapter>,
) -> Result<()> {
    let mut active: Option<Active> = None;
    while let Some(frame) = protocol::read_request(&mut input)? {
        match frame.message {
            WorkerRequest::Start { id } => {
                if active.is_some() {
                    return Err("Start while an utterance is active".into());
                }
                let (mut engine, events) = create(id)?;
                engine.start()?;
                active = Some((id, engine, events, 0));
            }
            WorkerRequest::Audio { id, sequence } => {
                let (current, engine, _, expected) = active.as_mut().ok_or("Audio before Start")?;
                if id != *current || sequence != *expected {
                    return Err("Audio identity or sequence mismatch".into());
                }
                engine.feed(frame.pcm.as_deref().ok_or("missing PCM")?);
                *expected = expected.checked_add(1).ok_or("sequence overflow")?;
                drain(&mut active, &mut output)?;
                if active.is_none() {
                    return Err("adapter ended before Finish".into());
                }
                protocol::write_response(&mut output, &WorkerResponse::AudioAck { id, sequence })?;
            }
            WorkerRequest::Finish { id } => {
                let (current, engine, events, _) = active.as_mut().ok_or("Finish before Start")?;
                if id != *current {
                    return Err("Finish identity mismatch".into());
                }
                engine.finish();
                loop {
                    let event = events.recv().map_err(|_| "adapter closed without Final")?;
                    if event.id != id {
                        return Err("adapter identity mismatch".into());
                    }
                    let terminal =
                        matches!(event.event, EngineEvent::Final | EngineEvent::Error(_));
                    protocol::write_response(
                        &mut output,
                        &WorkerResponse::Event {
                            id,
                            event: event.event,
                        },
                    )?;
                    if terminal {
                        if !events.is_empty() {
                            return Err("adapter emitted events after its terminal event".into());
                        }
                        active = None;
                        break;
                    }
                }
            }
            WorkerRequest::Cancel { id } => {
                let (current, engine, _, _) = active.as_mut().ok_or("Cancel before Start")?;
                if id != *current {
                    return Err("Cancel identity mismatch".into());
                }
                engine.cancel();
                active = None;
            }
        }
        drain(&mut active, &mut output)?;
    }
    if let Some((_, mut engine, _, _)) = active {
        engine.cancel();
    }
    Ok(())
}

fn drain(active: &mut Option<Active>, output: &mut impl Write) -> Result<()> {
    if let Some((id, _, events, _)) = active {
        for event in events.try_iter() {
            if event.id != *id {
                return Err("adapter identity mismatch".into());
            }
            if matches!(event.event, EngineEvent::Final) {
                return Err("adapter emitted Final before Finish".into());
            }
            let failed = matches!(event.event, EngineEvent::Error(_));
            protocol::write_response(
                output,
                &WorkerResponse::Event {
                    id: *id,
                    event: event.event,
                },
            )?;
            if failed {
                return Err("adapter failed".into());
            }
        }
    }
    Ok(())
}
