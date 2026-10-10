pub mod fake;

use crossbeam_channel::Receiver;
use voice_core::{AudioEvent, InputDevice, Result, UtteranceId};

pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;
pub const FLUSH_INTERVAL_MS: u64 = 250;

pub trait Recorder {
    fn list_devices(&self) -> Result<Vec<InputDevice>>;
    fn start(&mut self, id: UtteranceId, device: Option<&str>) -> Result<Receiver<AudioEvent>>;
    fn stop(&mut self) -> Result<()>;
}
