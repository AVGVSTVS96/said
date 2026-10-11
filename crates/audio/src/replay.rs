use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, bounded};
use rtrb::RingBuffer;
use said_core::{AudioEvent, InputDevice, Result, UtteranceId};

use crate::{
    Recorder, Resampler, SAMPLE_RATE,
    pipeline::{self, EVENT_QUEUE, Pipeline},
    transport::{CaptureState, copy_input},
    wav::{WavReader, create_audio_dir},
};

pub struct ReplayRecorder {
    input: PathBuf,
    audio_dir: PathBuf,
    active: Option<(Arc<CaptureState>, JoinHandle<Result<()>>)>,
}

impl ReplayRecorder {
    pub fn new(input: impl AsRef<Path>, audio_dir: impl AsRef<Path>) -> Self {
        Self {
            input: input.as_ref().into(),
            audio_dir: audio_dir.as_ref().into(),
            active: None,
        }
    }
}

impl Recorder for ReplayRecorder {
    fn list_devices(&self) -> Result<Vec<InputDevice>> {
        Ok(vec![InputDevice {
            id: "replay".into(),
            name: "Saved WAV replay".into(),
            is_default: true,
        }])
    }

    fn start(&mut self, id: UtteranceId, device: Option<&str>) -> Result<Receiver<AudioEvent>> {
        if self.active.is_some() {
            return Err("capture already active; stop before starting another utterance".into());
        }
        if device.is_some_and(|device| device != "replay") {
            return Err("unknown replay device".into());
        }
        let mut reader = WavReader::open(&self.input)?;
        create_audio_dir(&self.audio_dir)?;
        let (sender, receiver) = bounded(EVENT_QUEUE);
        let mut pipeline = Pipeline::new(
            id,
            &self.audio_dir.join(format!("{}.wav", id.0)),
            sender.clone(),
        )?;
        let mut resampler = Resampler::new(SAMPLE_RATE, 1)?;
        let (mut producer, mut consumer) = RingBuffer::new(4096);
        let state = Arc::new(CaptureState::default());
        let worker_state = state.clone();
        let worker = thread::Builder::new()
            .name("audio-replay".into())
            .spawn(move || {
                let result = (|| {
                    let mut pcm = [0; 4096];
                    while !worker_state.stop.load(Ordering::Acquire) {
                        let take = reader.read(&mut pcm)?;
                        if take == 0 {
                            break;
                        }
                        copy_input(&pcm[..take], 1, &mut producer, &worker_state, |s| {
                            s as f32 / 32768.0
                        });
                        pipeline::drain(&mut consumer, &mut resampler, &mut pipeline)?;
                        pipeline.checkpoint()?;
                        if pipeline.receiver_gone {
                            break;
                        }
                    }
                    pipeline::finish(consumer, resampler, pipeline, 0)
                })();
                if let Err(error) = &result {
                    let _ = sender.try_send(AudioEvent::Error {
                        id,
                        message: error.to_string(),
                    });
                }
                result
            })?;
        self.active = Some((state, worker));
        Ok(receiver)
    }

    fn stop(&mut self) -> Result<()> {
        let (state, worker) = self.active.take().ok_or("capture is not active")?;
        state.stop.store(true, Ordering::Release);
        worker.join().map_err(|_| "audio replay thread panicked")?
    }
}

impl Drop for ReplayRecorder {
    fn drop(&mut self) {
        if self.active.is_some() {
            let _ = self.stop();
        }
    }
}
