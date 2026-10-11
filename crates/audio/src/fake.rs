use crossbeam_channel::{Receiver, Sender, unbounded};
use said_core::{AudioEvent, AudioFrame, InputDevice, Result, UtteranceId};

use crate::Recorder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecorderCall {
    Start {
        id: UtteranceId,
        device: Option<String>,
    },
    Stop,
}

#[derive(Default)]
pub struct FakeRecorder {
    pub calls: Vec<RecorderCall>,
    pub frames: Vec<(Vec<i16>, f32)>,
    pub duration_ms: u64,
    active: Option<(UtteranceId, Sender<AudioEvent>)>,
}

impl FakeRecorder {
    pub fn new(frames: Vec<(Vec<i16>, f32)>, duration_ms: u64) -> Self {
        Self {
            frames,
            duration_ms,
            ..Self::default()
        }
    }
}

impl Recorder for FakeRecorder {
    fn list_devices(&self) -> Result<Vec<InputDevice>> {
        Ok(vec![InputDevice {
            id: "synthetic".into(),
            name: "Synthetic microphone".into(),
            is_default: true,
        }])
    }

    fn start(&mut self, id: UtteranceId, device: Option<&str>) -> Result<Receiver<AudioEvent>> {
        if self.active.is_some() {
            return Err("capture already active".into());
        }
        let (sender, receiver) = unbounded();
        self.calls.push(RecorderCall::Start {
            id,
            device: device.map(str::to_owned),
        });
        for (pcm, level) in &self.frames {
            sender.send(AudioEvent::Frame(AudioFrame {
                id,
                pcm: pcm.clone(),
                level: *level,
            }))?;
        }
        self.active = Some((id, sender));
        Ok(receiver)
    }

    fn stop(&mut self) -> Result<()> {
        let (id, sender) = self.active.take().ok_or("capture is not active")?;
        self.calls.push(RecorderCall::Stop);
        sender.send(AudioEvent::Stopped {
            id,
            duration_ms: self.duration_ms,
        })?;
        Ok(())
    }
}
