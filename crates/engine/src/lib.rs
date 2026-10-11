pub mod fake;
pub mod host;
pub mod manifest;
mod native;
pub mod parakeet;
pub mod protocol;
mod sherpa_abi;
mod spool;
mod watchdog;
pub mod wav;
pub mod worker;

use said_core::Result;

pub trait Engine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()>;
    fn feed(&mut self, pcm16k: &[i16]);
    fn finish(&mut self);
    fn cancel(&mut self);
}
