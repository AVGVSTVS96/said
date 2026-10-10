use std::time::Duration;

#[derive(Default)]
pub(crate) struct Watchdog {
    samples: u64,
    busy: Duration,
    rtf: f64,
}

impl Watchdog {
    pub fn observe(&mut self, samples: u64, busy: Duration) {
        if samples == 0 {
            return;
        }
        self.samples += samples;
        self.busy += busy;
        if self.samples >= 8000 {
            self.rtf = self
                .rtf
                .max(self.busy.as_secs_f64() * 16000.0 / self.samples as f64);
            self.samples = 0;
            self.busy = Duration::ZERO;
        }
    }

    pub fn real_time_factor(&self) -> f64 {
        self.rtf
    }

    pub fn budget(&self, floor: Duration, samples: u64) -> Duration {
        floor.max(Duration::from_secs_f64(
            self.rtf * samples as f64 / 16000.0 * 4.0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_progress_scales_with_audio_work_and_a_hang_still_has_a_deadline() {
        let mut watchdog = Watchdog::default();
        let floor = Duration::from_millis(100);
        assert_eq!(watchdog.budget(floor, 16000), floor);
        for _ in 0..5 {
            watchdog.observe(1600, Duration::from_millis(300));
        }
        assert_eq!(watchdog.real_time_factor(), 3.0);
        assert_eq!(watchdog.budget(floor, 16000), Duration::from_secs(12));
        assert_eq!(watchdog.budget(floor, 15 * 16000), Duration::from_secs(180));
        watchdog.observe(8000, Duration::from_secs(3));
        assert_eq!(watchdog.real_time_factor(), 6.0);
        assert_eq!(watchdog.budget(floor, 16000), Duration::from_secs(24));
        assert_eq!(watchdog.budget(floor, 0), floor);
    }
}
