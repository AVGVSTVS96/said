use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::atomic::Ordering,
    time::Duration,
};

use crossbeam_channel::bounded;
use rtrb::RingBuffer;
use said_core::{AudioEvent, UtteranceId};

use crate::{
    Recorder, ReplayRecorder, Resampler,
    pipeline::{self, EVENT_QUEUE, FRAME_QUEUE, Pipeline},
    transport::{CaptureState, DISCONNECTED, OVERRUN, copy_input},
    wav::{WavReader, WavWriter, recover_wav},
};

thread_local! {
    static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK_ALLOCATIONS.with(|tracking| {
            if tracking.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK_ALLOCATIONS.with(|tracking| {
            if tracking.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/audio-tests")
            .join(UtteranceId::random().0.to_string());
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn callback_copies_interleaved_frames_without_allocating_even_on_overrun() {
    let (mut producer, mut consumer) = RingBuffer::new(12);
    let state = CaptureState::default();
    let samples = [0.25, -0.25, 0.5, -0.5];
    ALLOCATIONS.with(|count| count.set(0));
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(true));
    for _ in 0..1000 {
        copy_input(&samples, 2, &mut producer, &state, |s| s);
        for _ in 0..4 {
            let _ = consumer.pop();
        }
    }
    copy_input(&[0.5; 16], 2, &mut producer, &state, |s| s);
    copy_input(&samples, 2, &mut producer, &state, |s| s);
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(consumer.slots(), 12);
    assert_eq!(state.failure.load(Ordering::Acquire), OVERRUN);
    assert!(state.stop.load(Ordering::Acquire));
}

fn convert(rate: u32, channels: u16, input: &[f32], chunk: usize) -> Vec<f32> {
    let mut resampler = Resampler::new(rate, channels).unwrap();
    let mut result = Vec::new();
    for samples in input.chunks(chunk) {
        resampler
            .push(samples, |out| {
                result.extend_from_slice(out);
                Ok(())
            })
            .unwrap();
    }
    resampler
        .finish(|out| {
            result.extend_from_slice(out);
            Ok(())
        })
        .unwrap();
    result
}

fn sine(rate: u32, frequency: f32, samples: usize) -> Vec<f32> {
    (0..samples)
        .map(|i| 0.5 * (std::f32::consts::TAU * frequency * i as f32 / rate as f32).sin())
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn resampling_preserves_tone_duration_and_final_burst_at_common_rates() {
    for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
        let input = sine(rate, 1000.0, rate as usize);
        let output = convert(rate, 1, &input, 137);
        assert_eq!(output.len(), 16_000, "rate {rate}");
        assert!(
            (rms(&output[1000..15000]) - 0.35355).abs() < 0.01,
            "rate {rate}"
        );
        let mut burst = vec![0.0; 4396];
        burst[4196..].fill(0.5);
        let output = convert(rate, 1, &burst, 311);
        assert_eq!(output.len(), 4396 * 16_000 / rate as usize);
        assert!(
            output.iter().rev().take(100).any(|s| *s > 0.3),
            "lost tail at {rate}"
        );
    }
}

#[test]
fn downsampling_filters_aliases_and_stereo_is_mixed_on_frame_boundaries() {
    let output = convert(48_000, 1, &sine(48_000, 10_000.0, 48_000), 1000);
    assert!(rms(&output[1000..15000]) < 0.005);
    let stereo: Vec<_> = (0..4321).flat_map(|_| [0.5, -0.25]).collect();
    let output = convert(16_000, 2, &stereo, 7);
    assert_eq!(output, vec![0.125; 4321]);
    assert_eq!(
        convert(16_000, 1, &[f32::NAN, f32::INFINITY, 0.5], 1),
        [0.0, 0.0, 0.5]
    );
}

#[test]
fn short_empty_and_new_recordings_do_not_add_padding_or_leak_audio() {
    for length in [0, 1, 99, 1024, 4096, 4396] {
        for rate in [16_000, 44_100, 48_000] {
            let output = convert(rate, 1, &vec![0.5; length], 31);
            assert_eq!(output.len(), length * 16_000 / rate as usize);
            let silence = convert(rate, 1, &vec![0.0; length], 31);
            assert!(silence.iter().all(|sample| *sample == 0.0));
        }
    }
    let mut resampler = Resampler::new(48_000, 2).unwrap();
    resampler.push(&[0.5], |_| Ok(())).unwrap();
    assert!(resampler.finish(|_| Ok(())).is_err());
    assert!(resampler.push(&[], |_| Ok(())).is_err());
}

#[test]
fn stalled_consumer_keeps_full_wav_and_terminal_events_in_a_bounded_queue() {
    let scratch = Scratch::new();
    let path = scratch.0.join("capture.wav");
    let id = UtteranceId::random();
    let (sender, receiver) = bounded(EVENT_QUEUE);
    let mut pipeline = Pipeline::new(id, &path, sender).unwrap();
    for _ in 0..1000 {
        pipeline.push(&[0.25; 320]).unwrap();
    }
    pipeline.finish(0).unwrap();
    assert!(receiver.len() <= EVENT_QUEUE);
    let events: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, AudioEvent::Frame(_)))
            .count(),
        FRAME_QUEUE
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AudioEvent::Error { message, .. } if message.contains("replay")))
    );
    assert!(
        matches!(events.last(), Some(AudioEvent::Stopped { id: stopped_id, duration_ms: 20_000 }) if *stopped_id == id)
    );
    let mut reader = WavReader::open(&path).unwrap();
    let mut count = 0;
    let mut pcm = [0; 4096];
    loop {
        let take = reader.read(&mut pcm).unwrap();
        if take == 0 {
            break;
        }
        assert!(pcm[..take].iter().all(|&s| s == 8192));
        count += take;
    }
    assert_eq!(count, 320_000);
}

#[test]
fn unplug_drains_ring_and_resampler_before_stopped_and_replays_identical_pcm() {
    let scratch = Scratch::new();
    let id = UtteranceId::random();
    let path = scratch.0.join("capture.wav");
    let (sender, receiver) = bounded(EVENT_QUEUE);
    let pipeline = Pipeline::new(id, &path, sender).unwrap();
    let resampler = Resampler::new(48_000, 2).unwrap();
    let (mut producer, consumer) = RingBuffer::new(4096);
    let state = CaptureState::default();
    copy_input(&[0.5; 3000], 2, &mut producer, &state, |s| s);
    state.fail(DISCONNECTED);
    copy_input(&[1.0; 400], 2, &mut producer, &state, |s| s);
    pipeline::finish(consumer, resampler, pipeline, DISCONNECTED).unwrap();
    let events: Vec<_> = receiver.try_iter().collect();
    assert!(
        matches!(events.first(), Some(AudioEvent::Disconnected { id: event_id }) if *event_id == id)
    );
    assert!(matches!(
        events.last(),
        Some(AudioEvent::Stopped {
            duration_ms: 31,
            ..
        })
    ));
    let captured: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AudioEvent::Frame(frame) => Some(frame.pcm.iter().copied()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(captured.len(), 500);
    assert!(
        events.iter().any(
            |e| matches!(e, AudioEvent::Frame(frame) if frame.level > 0.4 && frame.level <= 1.0)
        )
    );
    let mut replay = ReplayRecorder::new(&path, scratch.0.join("replayed"));
    let replay_id = UtteranceId::random();
    let receiver = replay.start(replay_id, None).unwrap();
    let mut replayed = Vec::new();
    loop {
        match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
            AudioEvent::Frame(frame) => {
                assert_eq!(frame.id, replay_id);
                replayed.extend(frame.pcm);
            }
            AudioEvent::Stopped { duration_ms, .. } => {
                assert_eq!(duration_ms, 31);
                break;
            }
            event => panic!("unexpected replay event: {event:?}"),
        }
    }
    replay.stop().unwrap();
    assert_eq!(replayed, captured);
}

#[test]
fn wav_checkpoints_recover_stale_headers_and_partial_samples_without_overwriting() {
    let scratch = Scratch::new();
    let path = scratch.0.join("capture.wav");
    let mut writer = WavWriter::create(&path).unwrap();
    writer.append(&[i16::MIN, -1, 0, 1, i16::MAX]).unwrap();
    writer.checkpoint().unwrap();
    writer.append(&[7, 8, 9]).unwrap();
    drop(writer);
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[0xff])
        .unwrap();
    assert_eq!(recover_wav(&path).unwrap(), 8);
    assert_eq!(recover_wav(&path).unwrap(), 8);
    assert!(WavWriter::create(&path).is_err());
    let mut reader = WavReader::open(&path).unwrap();
    let mut pcm = [0; 10];
    assert_eq!(reader.read(&mut pcm).unwrap(), 8);
    assert_eq!(&pcm[..8], &[i16::MIN, -1, 0, 1, i16::MAX, 7, 8, 9]);
    assert_eq!(reader.read(&mut pcm).unwrap(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn recordings_beyond_riff_limit_use_rf64_and_recovery_rejects_foreign_files() {
    let scratch = Scratch::new();
    let path = scratch.0.join("capture.wav");
    WavWriter::create(&path).unwrap().finish().unwrap();
    let samples = u32::MAX as u64 / 2 + 1000;
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(80 + samples * 2)
        .unwrap();
    assert_eq!(recover_wav(&path).unwrap(), samples);
    let mut bytes = [0; 4];
    use std::io::Read;
    fs::File::open(&path)
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"RF64");
    let mut reader = WavReader::open(&path).unwrap();
    assert_eq!(reader.read(&mut [0; 4]).unwrap(), 4);
    let other = scratch.0.join("foreign.wav");
    fs::write(&other, [0; 100]).unwrap();
    assert!(recover_wav(&other).is_err());
    assert_eq!(fs::read(&other).unwrap(), [0; 100]);
}

#[cfg(unix)]
#[test]
fn kill_nine_leaves_checkpoint_readable_and_recovery_retains_written_tail() {
    let scratch = Scratch::new();
    let path = scratch.0.join("capture.wav");
    let ready = scratch.0.join("ready");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::crash_writer_child",
            "--ignored",
            "--nocapture",
        ])
        .env("SAID_CRASH_DIR", &scratch.0)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(std::time::Instant::now() < deadline);
        assert!(child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        std::process::Command::new("kill")
            .args(["-KILL", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(!child.wait().unwrap().success());
    let mut pcm = [0; 2000];
    assert_eq!(
        WavReader::open(&path).unwrap().read(&mut pcm).unwrap(),
        1600
    );
    assert_eq!(recover_wav(&path).unwrap(), 1700);
    assert_eq!(
        WavReader::open(&path).unwrap().read(&mut pcm).unwrap(),
        1700
    );
    assert!(pcm[..1700].iter().all(|&sample| sample == 123));
}

#[test]
#[ignore = "subprocess fixture for kill_nine test"]
fn crash_writer_child() {
    let Some(path) = std::env::var_os("SAID_CRASH_DIR") else {
        return;
    };
    let path = PathBuf::from(path);
    let mut writer = WavWriter::create(&path.join("capture.wav")).unwrap();
    writer.append(&[123; 1600]).unwrap();
    writer.checkpoint().unwrap();
    writer.append(&[123; 100]).unwrap();
    fs::write(path.join("ready"), []).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "streams one hour of synthetic audio to disk"]
fn one_hour_synthetic_capture_has_flat_ram() {
    let scratch = Scratch::new();
    let path = scratch.0.join("hour.wav");
    let (sender, receiver) = bounded(EVENT_QUEUE);
    let mut pipeline = Pipeline::new(UtteranceId::random(), &path, sender).unwrap();
    let mut resampler = Resampler::new(48_000, 1).unwrap();
    let (mut producer, mut consumer) = RingBuffer::new(96_000);
    let state = CaptureState::default();
    let input = sine(48_000, 1000.0, 4800);
    let mut baseline = 0;
    let mut maximum = 0;
    for tick in 0..36_000 {
        copy_input(&input, 1, &mut producer, &state, |s| s);
        while consumer.slots() > 0 {
            pipeline::drain(&mut consumer, &mut resampler, &mut pipeline).unwrap();
        }
        pipeline.checkpoint().unwrap();
        for event in receiver.try_iter() {
            assert!(matches!(event, AudioEvent::Frame(_)));
        }
        #[cfg(target_os = "linux")]
        if tick % 1000 == 0 && tick >= 1000 {
            let rss = fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<usize>()
                .unwrap();
            if baseline == 0 {
                baseline = rss;
            }
            maximum = maximum.max(rss);
            assert!(
                rss < baseline + 8192,
                "RAM grew from {baseline} to {rss} KiB"
            );
        }
    }
    pipeline::finish(consumer, resampler, pipeline, 0).unwrap();
    assert!(matches!(
        receiver.try_iter().last(),
        Some(AudioEvent::Stopped {
            duration_ms: 3_600_000,
            ..
        })
    ));
    assert_eq!(fs::metadata(&path).unwrap().len(), 80 + 16_000 * 3600 * 2);
    println!("one hour: {baseline} KiB baseline, {maximum} KiB maximum");
}

#[cfg(feature = "native")]
#[test]
#[ignore = "lists real devices and captures for two seconds when available"]
fn native_two_second_smoke() {
    let scratch = Scratch::new();
    let mut recorder = crate::CpalRecorder::new(&scratch.0);
    let devices = recorder.list_devices().unwrap();
    println!("{} input devices", devices.len());
    if devices.is_empty() {
        println!("No input device; capture skipped");
        return;
    }
    let receiver = recorder.start(UtteranceId::random(), None).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    recorder.stop().unwrap();
    let mut count = 0;
    let mut stopped = false;
    for event in receiver.try_iter() {
        match event {
            AudioEvent::Frame(frame) => count += frame.pcm.len(),
            AudioEvent::Stopped { .. } => stopped = true,
            event => panic!("capture failed: {event:?}"),
        }
    }
    assert!(stopped);
    assert!(count > 0);
    println!("{count} normalized samples captured");
}
