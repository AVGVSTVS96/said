use std::collections::VecDeque;
use std::time::Duration;

#[derive(Default)]
pub(crate) struct Watchdog {
    samples: u64,
    busy: Duration,
    recent: VecDeque<f64>,
}

impl Watchdog {
    pub fn observe(&mut self, samples: u64, busy: Duration) {
        if samples == 0 {
            return;
        }
        self.samples += samples;
        self.busy += busy;
        if self.samples >= 8000 {
            self.recent
                .push_back(self.busy.as_secs_f64() * 16000.0 / self.samples as f64);
            if self.recent.len() > 32 {
                self.recent.pop_front();
            }
            self.samples = 0;
            self.busy = Duration::ZERO;
        }
    }

    pub fn real_time_factor(&self) -> f64 {
        let mut rates: Vec<_> = self.recent.iter().copied().collect();
        if rates.is_empty() {
            return 0.0;
        }
        let middle = rates.len() / 2;
        *rates.select_nth_unstable_by(middle, f64::total_cmp).1
    }

    pub fn budget(&self, floor: Duration, samples: u64) -> Duration {
        let peak = self.recent.iter().copied().fold(0.0, f64::max);
        let segment_work = self.real_time_factor() * samples.min(480000) as f64 / 16000.0;
        let update_work = peak * samples.min(8000) as f64 / 16000.0;
        floor.max(Duration::from_secs_f64(segment_work.max(update_work) * 4.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_progress_uses_measured_update_work_without_multiplying_queue_depth() {
        let mut watchdog = Watchdog::default();
        let floor = Duration::from_millis(100);
        assert_eq!(watchdog.budget(floor, 16000), floor);
        for _ in 0..5 {
            watchdog.observe(1600, Duration::from_millis(300));
        }
        assert_eq!(watchdog.real_time_factor(), 3.0);
        assert_eq!(watchdog.budget(floor, 16000), Duration::from_secs(12));
        assert_eq!(watchdog.budget(floor, 15 * 16000), Duration::from_secs(180));
        assert_eq!(
            watchdog.budget(floor, 600 * 16000),
            Duration::from_secs(360)
        );
        watchdog.observe(8000, Duration::from_secs(3));
        assert_eq!(watchdog.real_time_factor(), 6.0);
        assert_eq!(watchdog.budget(floor, 16000), Duration::from_secs(24));
        assert_eq!(watchdog.budget(floor, 0), floor);
        for _ in 0..32 {
            watchdog.observe(8000, Duration::from_millis(10));
        }
        assert_eq!(watchdog.real_time_factor(), 0.02);
        assert_eq!(
            watchdog.budget(floor, 15 * 16000),
            Duration::from_millis(1200)
        );
    }
}
