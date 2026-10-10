use std::{
    path::Path,
    time::{Duration, Instant},
};

use crossbeam_channel::{Sender, TrySendError};
use rtrb::Consumer;
use voice_core::{AudioEvent, AudioFrame, Result, UtteranceId};

use crate::{
    FLUSH_INTERVAL_MS, Resampler, SAMPLE_RATE,
    transport::{BACKEND_ERROR, DISCONNECTED, OVERRUN},
    wav::WavWriter,
};

pub(crate) const FRAME_SAMPLES: usize = 320;
pub(crate) const FRAME_QUEUE: usize = 128;
pub(crate) const EVENT_QUEUE: usize = FRAME_QUEUE + 4;

pub(crate) struct Pipeline {
    id: UtteranceId,
    sender: Sender<AudioEvent>,
    writer: WavWriter,
    pcm: Vec<i16>,
    checkpoint: Instant,
    skipped_frames: u64,
    pub receiver_gone: bool,
}

impl Pipeline {
    pub fn new(id: UtteranceId, path: &Path, sender: Sender<AudioEvent>) -> Result<Self> {
        Ok(Self {
            id,
            sender,
            writer: WavWriter::create(path)?,
            pcm: Vec::with_capacity(FRAME_SAMPLES),
            checkpoint: Instant::now(),
            skipped_frames: 0,
            receiver_gone: false,
        })
    }

    pub fn push(&mut self, samples: &[f32]) -> Result<()> {
        for &sample in samples {
            self.pcm.push(
                (sample.clamp(-1.0, 1.0) * 32768.0)
                    .round()
                    .clamp(-32768.0, 32767.0) as i16,
            );
            if self.pcm.len() == FRAME_SAMPLES {
                self.emit()?;
            }
        }
        Ok(())
    }

    fn emit(&mut self) -> Result<()> {
        self.writer.append(&self.pcm)?;
        if !self.receiver_gone && self.sender.len() < FRAME_QUEUE {
            let level = (self
                .pcm
                .iter()
                .map(|&s| (s as f64 / 32768.0).powi(2))
                .sum::<f64>()
                / self.pcm.len() as f64)
                .sqrt() as f32;
            let event = AudioEvent::Frame(AudioFrame {
                id: self.id,
                pcm: self.pcm.clone(),
                level,
            });
            match self.sender.try_send(event) {
                Ok(()) => (),
                Err(TrySendError::Disconnected(_)) => self.receiver_gone = true,
                Err(TrySendError::Full(_)) => self.skipped_frames += 1,
            }
        } else {
            self.skipped_frames += 1;
        }
        self.pcm.clear();
        Ok(())
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        if self.checkpoint.elapsed() >= Duration::from_millis(FLUSH_INTERVAL_MS) {
            if !self.pcm.is_empty() {
                self.emit()?;
            }
            self.writer.checkpoint()?;
            self.checkpoint = Instant::now();
        }
        Ok(())
    }

    pub fn disconnected(&self) {
        let _ = self
            .sender
            .try_send(AudioEvent::Disconnected { id: self.id });
    }

    pub fn finish(mut self, failure: u8) -> Result<()> {
        if !self.pcm.is_empty() {
            self.emit()?;
        }
        let samples = self.writer.finish()?;
        if failure == BACKEND_ERROR || failure == OVERRUN {
            let message = if failure == OVERRUN {
                "Capture ring overflowed; recording stopped at the last retained frame"
            } else {
                "Audio input stream failed; recording stopped"
            };
            let _ = self.sender.try_send(AudioEvent::Error {
                id: self.id,
                message: message.into(),
            });
        }
        if self.skipped_frames != 0 {
            let _ = self.sender.try_send(AudioEvent::Error {
                id: self.id,
                message:
                    "Live audio consumer fell behind; replay the complete saved WAV for recognition"
                        .into(),
            });
        }
        let _ = self.sender.try_send(AudioEvent::Stopped {
            id: self.id,
            duration_ms: samples * 1000 / SAMPLE_RATE as u64,
        });
        Ok(())
    }
}

pub(crate) fn drain(
    consumer: &mut Consumer<f32>,
    resampler: &mut Resampler,
    pipeline: &mut Pipeline,
) -> Result<()> {
    let mut input = [0.0; 4096];
    let take = consumer.slots().min(input.len());
    for sample in &mut input[..take] {
        *sample = consumer.pop().expect("only this consumer drains the ring");
    }
    resampler.push(&input[..take], |samples| pipeline.push(samples))
}

pub(crate) fn finish(
    mut consumer: Consumer<f32>,
    mut resampler: Resampler,
    mut pipeline: Pipeline,
    failure: u8,
) -> Result<()> {
    if failure == DISCONNECTED {
        pipeline.disconnected();
    }
    while consumer.slots() > 0 {
        drain(&mut consumer, &mut resampler, &mut pipeline)?;
    }
    resampler.finish(|samples| pipeline.push(samples))?;
    pipeline.finish(failure)
}
