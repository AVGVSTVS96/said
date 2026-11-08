pub mod fake;
pub mod host;
pub mod manifest;
#[allow(unsafe_code, reason = "dlopen of the sherpa-onnx runtime")]
mod native;
#[allow(unsafe_code, reason = "sherpa-onnx C API")]
pub mod parakeet;
pub mod protocol;
mod sherpa_abi;
mod spool;
mod watchdog;
pub mod wav;
pub mod worker;

use said_core::Result;

pub trait Engine {
    fn start(&mut self) -> Result<()>;
    fn feed(&mut self, pcm16k: &[i16]);
    fn finish(&mut self);
    fn cancel(&mut self);
}
