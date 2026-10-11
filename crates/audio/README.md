# Audio

`CpalRecorder::new(audio_dir)` implements `Recorder` behind the `native` feature.
`list_devices()` returns CPAL device IDs; pass an ID unchanged to `start`. `None`
resolves the system default when capture starts. Start creates `<id>.wav` without
overwriting an existing recording. Consume the returned receiver while recording,
then call `stop` to join the capture thread. After a spontaneous disconnect, call
`stop` to reap that thread before starting another recording.

The callback copies interleaved samples into a two-second SPSC ring, with no
allocation, locks, logging, or I/O. The worker mixes channels, resamples with an
antialiasing filter, removes the filter delay, and emits 20 ms PCM frames and RMS
levels. The final frame is short; it is never padded. Capture has no silence gate.

The worker writes audio before delivering frames. WAV headers are checkpointed
and synced every 250 ms; `Stopped` follows the final sync. Files and directories
are owner-only on Unix. Recordings exceeding RIFF's size limit become RF64.
Call `wav::recover_wav` for pending recordings on startup, before replaying them.
Recovery repairs the header from the file length and truncates an incomplete
sample. It only accepts this module's WAV layout. `WavReader` streams that layout
with bounded memory, and `ReplayRecorder` sends it through the same pipeline at
file speed for verification and recovery.

The event channel is bounded to 128 live frames plus four terminal events. A
stalled receiver never blocks disk writing. A gap in live delivery emits `Error`
before `Stopped`: the host must recognize from the complete WAV instead of
accepting the live transcript. Capture-ring overflow stops recording explicitly
and emits `Error` and `Stopped`; it cannot silently discard input and continue.
Storage failures emit `Error` without a successful `Stopped`.

Reported device removal emits `Disconnected`, drains the ring and filter, then
emits `Stopped`. The recorder never reopens or chooses a replacement device.
On Linux, the default input follows PipeWire and may continue from a new source after unplug.

Explicit non-default devices retain the disconnect/end-event behavior. Reported
ALSA backend failures are checked for device removal on the worker thread. Real
hot unplug and macOS permission/device tests remain platform integration checks.

```sh
CARGO_BUILD_JOBS=2 cargo test -p said-audio
CARGO_BUILD_JOBS=2 cargo test -p said-audio --features native
CARGO_BUILD_JOBS=2 cargo clippy -p said-audio --all-targets --features native -- -D warnings
CARGO_BUILD_JOBS=2 cargo test -p said-audio one_hour_synthetic_capture_has_flat_ram -- --ignored --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 cargo test -p said-audio --features native native_two_second_smoke -- --ignored --nocapture --test-threads=1
```

Default tests use synthetic signals, replay, a killed child writer, and a sparse
RF64 file. The one-hour test streams 48 kHz synthetic input, checks the resulting
duration and file length, and samples Linux RSS after warmup. The explicit native
smoke test captures for two seconds and deletes its recording.
