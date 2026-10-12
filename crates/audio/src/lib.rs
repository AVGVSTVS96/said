pub mod fake;
mod pipeline;
mod replay;
mod resample;
mod transport;
pub mod wav;

#[cfg(feature = "native")]
mod capture;
#[cfg(feature = "native")]
pub use capture::{CpalRecorder, list_devices};
pub use replay::ReplayRecorder;
pub use resample::Resampler;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

use crossbeam_channel::Receiver;
use said_core::{AudioEvent, InputDevice, Result, UtteranceId};

pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;
pub const FLUSH_INTERVAL_MS: u64 = 250;

#[cfg(test)]
mod tests;

pub trait Recorder {
    fn list_devices(&self) -> Result<Vec<InputDevice>>;
    fn start(&mut self, id: UtteranceId, device: Option<&str>) -> Result<Receiver<AudioEvent>>;
    fn stop(&mut self) -> Result<()>;
    fn preview(&self, _device: Option<&str>) -> Result<Preview> {
        Err("microphone preview is unavailable".into())
    }
}

/// Maps RMS amplitude to 0..1 across -60..0 dBFS, so speech lands mid-scale instead of near zero.
pub fn level(rms: f64) -> f32 {
    ((20.0 * rms.max(1e-6).log10() + 60.0) / 60.0).clamp(0.0, 1.0) as f32
}

/// A level-only microphone stream that writes nothing to disk; dropping it closes the device.
pub struct Preview {
    levels: Receiver<std::result::Result<f32, String>>,
    worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

impl Preview {
    pub fn new(
        levels: Receiver<std::result::Result<f32, String>>,
        worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
    ) -> Self {
        Self { levels, worker }
    }

    pub fn levels(&self) -> &Receiver<std::result::Result<f32, String>> {
        &self.levels
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if let Some((stop, worker)) = self.worker.take() {
            stop.store(true, Ordering::Release);
            let _ = worker.join();
        }
    }
}
