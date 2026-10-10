pub mod fake;
pub mod protocol;

use voice_core::Result;

pub trait Engine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()>;
    fn feed(&mut self, pcm16k: &[i16]);
    fn finish(&mut self);
    fn cancel(&mut self);
}
