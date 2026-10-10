pub mod fake;
pub mod host;
pub mod manifest;
pub mod moonshine;
mod native;
pub mod protocol;
mod spool;
mod watchdog;
pub mod wav;
pub mod worker;

use voice_core::Result;

pub trait Engine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()>;
    fn feed(&mut self, pcm16k: &[i16]);
    fn finish(&mut self);
    fn cancel(&mut self);
}
