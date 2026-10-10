use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use rtrb::Producer;

pub(crate) const DISCONNECTED: u8 = 1;
pub(crate) const BACKEND_ERROR: u8 = 2;
pub(crate) const OVERRUN: u8 = 3;

#[derive(Default)]
pub(crate) struct CaptureState {
    pub stop: AtomicBool,
    pub failure: AtomicU8,
}

impl CaptureState {
    pub fn fail(&self, failure: u8) {
        let _ = self
            .failure
            .compare_exchange(0, failure, Ordering::AcqRel, Ordering::Acquire);
        self.stop.store(true, Ordering::Release);
    }
}

pub(crate) fn copy_input<T: Copy>(
    data: &[T],
    channels: usize,
    producer: &mut Producer<f32>,
    state: &CaptureState,
    convert: impl Fn(T) -> f32,
) {
    if state.stop.load(Ordering::Acquire) {
        return;
    }
    let take = data.len().min(producer.slots()) / channels * channels;
    for &sample in &data[..take] {
        let _ = producer.push(convert(sample));
    }
    if take != data.len() {
        state.fail(OVERRUN);
    }
}
