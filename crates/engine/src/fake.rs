use std::collections::VecDeque;

use crossbeam_channel::{Receiver, Sender, unbounded};
use voice_core::{EngineEvent, EngineOutput, Result, UtteranceId};

use crate::Engine;

#[derive(Debug, Default)]
pub struct Trace {
    pub start: Vec<EngineEvent>,
    pub feeds: VecDeque<Vec<EngineEvent>>,
    pub finish: Vec<EngineEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineCall {
    Start(Vec<String>),
    Feed(Vec<i16>),
    Finish,
    Cancel,
}

pub struct FakeEngine {
    pub calls: Vec<EngineCall>,
    id: UtteranceId,
    trace: Trace,
    sender: Sender<EngineOutput>,
    active: bool,
}

impl FakeEngine {
    pub fn new(id: UtteranceId, trace: Trace) -> (Self, Receiver<EngineOutput>) {
        let (sender, receiver) = unbounded();
        (
            Self {
                calls: vec![],
                id,
                trace,
                sender,
                active: false,
            },
            receiver,
        )
    }

    fn emit(&self, events: Vec<EngineEvent>) {
        for event in events {
            let _ = self.sender.send(EngineOutput { id: self.id, event });
        }
    }
}

impl Engine for FakeEngine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()> {
        if self.active {
            return Err("engine already active".into());
        }
        self.active = true;
        self.calls.push(EngineCall::Start(vocabulary.to_vec()));
        let events = std::mem::take(&mut self.trace.start);
        self.emit(events);
        Ok(())
    }

    fn feed(&mut self, pcm16k: &[i16]) {
        if self.active {
            self.calls.push(EngineCall::Feed(pcm16k.to_vec()));
            if let Some(events) = self.trace.feeds.pop_front() {
                self.emit(events);
            }
        }
    }

    fn finish(&mut self) {
        if self.active {
            self.active = false;
            self.calls.push(EngineCall::Finish);
            let events = std::mem::take(&mut self.trace.finish);
            self.emit(events);
        }
    }

    fn cancel(&mut self) {
        self.active = false;
        self.calls.push(EngineCall::Cancel);
    }
}
