# engine

The app calls `manifest::worker_main` before desktop initialization for its
`--worker` mode. The host warms a `WorkerClient` during setup, then calls
`client.engine(id)`
and consumes its receiver while invoking the existing `Engine` trait. The
`voice-engine` probe demonstrates both sides in the same executable; it is not
an additional process shipped alongside the app. App and bench use the same
`protocol` codec and `recognize_file` path.

`models.json` selects the adapter and per-platform default. Its model entries
pin the architecture, files, SHA-256 hashes, source and license. Change one
platform's default line after the full bake-off; no adapter code changes.
`Manifest::read` accepts an installed manifest, and `Manifest::bundled` embeds
the checked-in one. Small on Linux and Medium on macOS remain provisional.

## Native runtime

Moonshine Voice 0.1.5+voice.2's C ABI uses native ONNX Runtime on both platforms.
CPU is upstream's recommended default, including Apple Silicon; CoreML is not
required. `return_audio_data=false` releases completed segment audio; the
recorder already owns the durable WAV. The official archive/wheel supplies the
pinned ONNX Runtime library. Provisioning builds Moonshine from a SHA-256-pinned
source archive plus `moonshine-0.1.5.patch`. No Python interpreter or build process
runs in the app. The loader requires the patch's native revision marker, so an
unpatched library fails explicitly.

The patch gives each stream independent Silero recurrent state: upstream's
global detector carried one recording's state into the next, suppressing the
quiet AMI clip after the CI prefix. It replaces the incorrect duration fade with
a hard 15-second segment bound, removes speech look-behind at forced boundaries,
clears reused look-behind buffers, and pads VAD/frontend remainders at finish.
Stop forces final VAD/decoder updates even with no new audio, and encoder
lookahead is flushed even when all frontend samples were already consumed.
Forced segments own contiguous, disjoint sample ranges. Padding adds silence
after the final original sample; it never replaces captured samples. CPU sessions
use two intra-op threads, one inter-op thread and no busy spinning, avoiding
upstream's per-session default thread-pool contention.

A hard boundary can still change recognition of a word spanning it. Sample
conservation is proven; zero recognition errors are not. The public long test
checks repeated passage anchors and the golden pause test checks all six spoken
occurrences of "go". Neither certifies arbitrary boundary word accuracy.

Sources: [C ABI](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-c-api.h),
[execution providers](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/execution-providers.md),
[native releases](https://github.com/moonshine-ai/moonshine/releases/tag/v0.1.5).
The Rust binding checks the transcript ABI before loading a model. Unsafe code
is confined to the native loader and ABI calls, inside the isolated worker.

```sh
crates/engine/scripts/fetch-model.sh
CARGO_BUILD_JOBS=2 cargo build -p voice-engine --bin voice-engine --examples
cargo run -p voice-engine --bin voice-engine -- /path/to/16k-mono-i16.wav
```

The provisioning script requires curl, jq, OpenSSL, tar, unzip, patch, CMake 3.22+
and a C++20 compiler. It builds with two jobs and disk-backed scratch. It verifies
all downloaded model files and runtime archives before installing them under
`~/.cache/voice-models/`. macOS arm64 requires macOS 15 or newer for this build.
Linux x86_64 and aarch64 runtime artifacts are pinned; aarch64 is not yet tested.
Setup can use the same manifest hashes in its own downloader.

## Text and recovery

Moonshine's stable line indexes identify audio segments. Completed lines append
once; the incomplete tail replaces its previous value. No word overlap test,
trimming, case conversion, punctuation changes or spelling fusion is used.
Incoming samples are buffered into 8,000-sample blocks (500 ms at 16 kHz),
independent of feed fragmentation. Quiet blocks are amplified toward a peak of
0.25, capped at 32×; louder blocks and exact silence are unchanged. There is no
clipping, sample removal or text normalization. This addresses empty decodes on
low-level speech even after VAD isolation. The durable WAV remains the recorder's
original audio. Each block triggers one decoding update. Finish drains immediately,
including any short tail.

A detected speech line that decodes to empty/whitespace emits the existing
`EngineEvent::Error` with its time range and no Final. This also catches an empty
speech segment inside an otherwise nonempty utterance. Silence with no speech
lines still finishes empty normally. No shared event or protocol change is needed.
The adapter supplies one space between nonempty model lines, following the
upstream transcript composition convention. All text inside each line remains
unchanged. Vocabulary explicitly enables native keyterm biasing; commas/NUL in
individual terms are rejected because upstream's keyterm interface uses commas.

Live audio is spooled in order to a 0600 temporary PCM file created inside a 0700
`~/.cache/voice-models/audio-spool` directory. Only published complete writes are
read, in at most 1,600-sample frames. Bounded queues hold notifications and frames,
so a slow recognizer cannot overflow capture's backlog into lost words. Finish is
sent after the spool is consumed and all audio is acknowledged. The file is
unlinked immediately after opening: the engine can still read/write
its handle, and the OS removes its audio when the handles close, including after
a process crash. The recorder's durable WAV remains authoritative. Spool I/O
failure is explicit.

Host request and transport queues are bounded; response/event queues hold at
most 64 frames. A receiver closing before Final is an engine failure, including
an event consumer that has stopped draining its queue. PCM is sent in blocks of
at most 1,600 samples. Audio acknowledgments must match the utterance and next
sequence. Malformed output, native exit, queue overflow or a missed deadline
kills the worker and emits Error without Final for that identity. Capture
continues separately on disk. Core allocates a fresh UUID and a new pending row
with the original `audio_id`, then requests file recognition after capture stops.
`recognize_file(new_id, config, wav_path, vocabulary)` starts a fresh worker,
replays the same repaired, closed 16 kHz mono i16 WAV in bounded chunks, and
returns its full raw text. It never appends replay text to the interrupted ID.
The executor sends file duration as AudioEvent::Stopped before the replay's
Committed/Final events. Cancellation kills the live child and emits no usable
late transcript. A failed replay returns an error; it never loops.

`WorkerConfig` retains its existing fields. Startup has a separate cold-load
budget, retained until the first nonempty transcript proves the decoder has run:
a warm empty-audio acknowledgment proves model loading, not inference warmup.
Audio/finish values then seed the no-progress floor. Acknowledgments measure
busy wall time per half-second of consumed audio, excluding time with no pending
work. The worker retains 32 half-second measurements (16 seconds of consumed
audio), including between utterances. Its next-progress budget is the larger of
four times the rolling median RTF for a bounded 15-second speech segment and
four times the recent peak RTF for one half-second update, with the caller's
seed floor. Neither term grows with the entire queue. The median handles normal
work; the peak allows recently observed stalls without multiplying them by every
queued second. Old load spikes age out as more audio is processed. Acknowledgments
rearm the progress clock; queue age never consumes a later frame's allowance.
Finish timing begins after audio has been acknowledged. Partial output cannot
keep a hung worker alive. The same policy applies to file replay; benchmark does
not add a second fixed finish timer. Errors include measured RTF and audio count.

The probe's 60/10/30-second floors remain diagnostic bootstrap limits, not latency
targets. Queues remain bounded; audio backlog grows on disk until processed or canceled. Probe queues hold 256 100-ms blocks (less than 1 MiB
of PCM per queue). `WorkerClient` retains its model, child and learned speed between
utterances, with warming before capture. `WorkerEngine::new` supports an independent
cold worker. A concurrent utterance can use another isolated worker.

The executor forwards worker Error to core. Core's AllocateReplay/ReplayReady
and RecognizeFile effects own retry identity and durable-row ordering. No new
shared engine event or wire message is needed.

## Verification

```sh
CARGO_BUILD_JOBS=2 cargo test -p voice-engine
CARGO_BUILD_JOBS=2 cargo clippy -p voice-engine --all-targets -- -D warnings
cargo fmt -p voice-engine --check
crates/engine/scripts/test-native.sh
CARGO_BUILD_JOBS=2 cargo test -p voice-engine --test public_regressions -- --ignored --nocapture
```

The suite includes 50,000 deterministic mutated protocol inputs with fragmented
reads; framing limits/truncation; lifecycle/identity/sequence validation; literal
fake traces; crashes, hangs and malformed stdout; cancellation before start; accepted finish surviving producer drop; and recovery
fresh replay identity and isolation from failed live text. It adds no runtime dependency or test-only dependency.

Generate the public synthetic clips on macOS, then copy only these WAVs to the
same cache location on Linux. No microphone recording or private audio is used.
The scripts and literal synthetic sentences are public; generated clips remain
outside git.

```sh
crates/engine/scripts/generate-clips.sh
CARGO_BUILD_JOBS=2 cargo test -p voice-engine --test golden -- --ignored --nocapture
cargo run -p voice-engine --example stream -- target/debug/voice-engine \
  "$HOME/.cache/voice-models/engine-proof/repeats.wav" moonshine-small-en
```

`VOICE_PROOF_TIMEOUT_SECS` can increase only the golden test's diagnostic
deadlines on a busy host. It changes no production watchdog setting and does
not establish latency compliance.

Golden verification compares direct adapter and same-binary worker results byte
for byte on three synthetic speech clips, silence, and two repeat clips separated by a
pause. The last clip must produce multiple committed chunks and retain all six
spoken occurrences of "go". It checks one terminal
Final and no post-Final events, and silence must produce no text. These are
adapter-integrity checks, not a fidelity ranking or the full public CI corpus.

The public regression primes both models with the benchmark's CC0 CI clips,
recognizes the same CC-BY AMI crop twice and requires identical nonempty output,
checks the last spoken word of an exact eight-second recording, then completes the 98-second monologue and checks four occurrences of each passage
anchor. `test-native.sh` checks three minutes of synthetic PCM with five feed sizes
(including one sample), every original sample exactly once, contiguous times,
bounded segments and zero-only padding. Supervisor tests verify queued slow audio,
a slow finish, cancellation and a hung finish. Native checks require provisioning.
