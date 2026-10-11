use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler as _};
use said_core::Result;

use crate::SAMPLE_RATE;

pub struct Resampler {
    rate: u32,
    channels: usize,
    channel_sum: f64,
    channel_count: usize,
    fft: Option<Fft<f32>>,
    chunk_size: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    skip: usize,
    input_count: u64,
    output_count: u64,
    finished: bool,
}

impl Resampler {
    pub fn new(rate: u32, channels: u16) -> Result<Self> {
        if !(8_000..=384_000).contains(&rate) || !(1..=32).contains(&channels) {
            return Err("unsupported input sample rate or channel count".into());
        }
        let fft = (rate != SAMPLE_RATE)
            .then(|| {
                Fft::new(
                    rate as usize,
                    SAMPLE_RATE as usize,
                    512,
                    2,
                    1,
                    FixedSync::Input,
                )
            })
            .transpose()?;
        let input_size = fft.as_ref().map_or(320, |r| r.input_frames_max());
        let output_size = fft.as_ref().map_or(320, |r| r.output_frames_max());
        let skip = fft.as_ref().map_or(0, |r| r.output_delay());
        Ok(Self {
            rate,
            channels: channels as usize,
            channel_sum: 0.0,
            channel_count: 0,
            fft,
            chunk_size: input_size,
            input: Vec::with_capacity(input_size),
            output: vec![0.0; output_size],
            skip,
            input_count: 0,
            output_count: 0,
            finished: false,
        })
    }

    pub fn push(
        &mut self,
        interleaved: &[f32],
        mut emit: impl FnMut(&[f32]) -> Result<()>,
    ) -> Result<()> {
        if self.finished {
            return Err("resampler already finished".into());
        }
        for &sample in interleaved {
            self.channel_sum += if sample.is_finite() {
                sample as f64
            } else {
                0.0
            };
            self.channel_count += 1;
            if self.channel_count != self.channels {
                continue;
            }
            self.input
                .push((self.channel_sum / self.channels as f64) as f32);
            self.channel_sum = 0.0;
            self.channel_count = 0;
            self.input_count += 1;
            if self.input.len() == self.chunk_size {
                self.process(&mut emit)?;
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, mut emit: impl FnMut(&[f32]) -> Result<()>) -> Result<()> {
        if self.finished {
            return Err("resampler already finished".into());
        }
        self.finished = true;
        if self.channel_count != 0 {
            return Err("incomplete input channel frame".into());
        }
        if !self.input.is_empty() {
            self.process(&mut emit)?;
        }
        let expected = self.input_count * SAMPLE_RATE as u64 / self.rate as u64;
        while self.output_count < expected {
            self.process(&mut emit)?;
        }
        Ok(())
    }

    fn process(&mut self, emit: &mut impl FnMut(&[f32]) -> Result<()>) -> Result<()> {
        if let Some(fft) = &mut self.fft {
            self.input.resize(fft.input_frames_next(), 0.0);
            let input = InterleavedSlice::new(&self.input, 1, self.input.len())?;
            let output_len = self.output.len();
            let mut output = InterleavedSlice::new_mut(&mut self.output, 1, output_len)?;
            let (_, produced) = fft.process_into_buffer(&input, &mut output, None)?;
            let skip = self.skip.min(produced);
            self.skip -= skip;
            let expected = self.input_count * SAMPLE_RATE as u64 / self.rate as u64;
            let take = (produced - skip).min((expected - self.output_count) as usize);
            if take > 0 {
                emit(&self.output[skip..skip + take])?;
                self.output_count += take as u64;
            }
        } else if !self.input.is_empty() {
            emit(&self.input)?;
            self.output_count += self.input.len() as u64;
        }
        self.input.clear();
        Ok(())
    }
}
