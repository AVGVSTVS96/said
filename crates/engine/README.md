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

Moonshine Voice 0.1.5's C ABI uses native ONNX Runtime on both platforms.
CPU is upstream's recommended default, including Apple Silicon; CoreML is not
required. `return_audio_data=false` releases completed segment audio; the
recorder already owns the durable WAV. The Linux release contains self-contained shared libraries. On macOS
we extract only the two native dylibs from the official arm64 wheel: no Python
interpreter, package imports or Python subprocess runs in the app.

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

The provisioning script requires curl, jq, OpenSSL, tar and unzip. It verifies
all downloaded model files and runtime archives before installing them under
`~/.cache/voice-models/`. macOS arm64 requires macOS 15 or newer for this build.
Linux x86_64 and aarch64 runtime artifacts are pinned; aarch64 is not yet tested.
Setup can use the same manifest hashes in its own downloader.

## Text and recovery

Moonshine's stable line indexes identify audio segments. Completed lines append
once; the incomplete tail replaces its previous value. No word overlap test,
trimming, case conversion, punctuation changes or spelling fusion is used.
Decoding updates use 8,000 incoming samples (500 ms at 16 kHz), matching the
bake-off harness's streaming cadence. Finish drains immediately, including any
short tail. The adapter supplies one space between nonempty model lines, following the
upstream transcript composition convention. All text inside each line remains
unchanged. Vocabulary explicitly enables native keyterm biasing; commas/NUL in
individual terms are rejected because upstream's keyterm interface uses commas.

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

`WorkerConfig` takes startup, acknowledgment and finish deadlines from the
benchmark caller. Startup is distinct from warm inference. The probe's generous
60/10/30-second deadlines are diagnostic limits, not release budgets. The
initial queue must accommodate cold loading; probe queues hold 256 100-ms
blocks (less than 1 MiB of PCM per queue). `WorkerClient` retains its model and child between utterances; warming happens
before capture. A concurrent utterance can use a second isolated worker.
`WorkerEngine::new` is also available for an independent cold worker.

The executor forwards worker Error to core. Core's AllocateReplay/ReplayReady
and RecognizeFile effects own retry identity and durable-row ordering. No new
shared engine event or wire message is needed.

## Verification

```sh
CARGO_BUILD_JOBS=2 cargo test -p voice-engine
CARGO_BUILD_JOBS=2 cargo clippy -p voice-engine --all-targets -- -D warnings
cargo fmt -p voice-engine --check
```

The suite includes 50,000 deterministic mutated protocol inputs with fragmented
reads; framing limits/truncation; lifecycle/identity/sequence validation; literal
fake traces; crashes, hangs and malformed stdout; cancellation; and recovery
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
