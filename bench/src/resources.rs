use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct Memory {
    stop: Arc<AtomicBool>,
    sampler: Option<JoinHandle<Option<u64>>>,
}

impl Memory {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let sampler = thread::spawn(move || {
            let mut peak = process_tree_rss();
            while !flag.load(Ordering::Relaxed) {
                if let Some(rss) = process_tree_rss() {
                    peak = Some(peak.unwrap_or(0).max(rss));
                }
                thread::sleep(Duration::from_millis(20));
            }
            peak
        });
        Self {
            stop,
            sampler: Some(sampler),
        }
    }

    pub fn finish(mut self) -> Option<u64> {
        self.stop.store(true, Ordering::Relaxed);
        self.sampler.take()?.join().ok().flatten()
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(sampler) = self.sampler.take() {
            let _ = sampler.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn process_tree_rss() -> Option<u64> {
    let pid = std::process::id();
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut pids = std::collections::BTreeSet::from([pid]);
    for task in tasks.flatten() {
        if let Ok(children) = std::fs::read_to_string(task.path().join("children")) {
            pids.extend(
                children
                    .split_whitespace()
                    .filter_map(|p| p.parse::<u32>().ok()),
            );
        }
    }
    let mut total = 0;
    for pid in pids {
        if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
            let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
            total += line.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024;
        }
    }
    Some(total)
}

#[cfg(target_os = "macos")]
fn process_tree_rss() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss="])
        .output()
        .ok()?;
    let rows = String::from_utf8(output.stdout).ok()?;
    let pid = std::process::id();
    Some(
        rows.lines()
            .filter_map(|row| {
                let values: Vec<u64> = row
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                (values.len() == 3 && (values[0] == u64::from(pid) || values[1] == u64::from(pid)))
                    .then(|| values[2] * 1024)
            })
            .sum(),
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_tree_rss() -> Option<u64> {
    None
}
