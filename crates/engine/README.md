# engine

The default on Linux CPU and Apple Silicon is **Parakeet TDT v3, FP32,
sherpa-onnx 1.13.8**, with 30-second windows and 5-second overlap. The app and
`said bench` run this adapter through the same worker protocol. No Python,
server daemon, VAD gate, text cleanup or automatic fallback
runs in the recognition path.

`models.json` pins the model export revision, each file's SHA-256, runtime
archives, license, CPU provider, eight inference threads and 32,000 samples
between steady partial updates. Both platform default lines select the same
model. The unquantized ONNX export includes 2.435 GB of external encoder
weights. Model weights are CC-BY-4.0; sherpa-onnx is Apache-2.0 and ONNX Runtime
is MIT. Attribution is retained in `licenses/`.

```sh
crates/engine/scripts/fetch-model.sh
CARGO_BUILD_JOBS=2 cargo build -p said-engine --bin said-engine --examples
cargo run -p said-engine --bin said-engine -- /path/to/16k-mono-i16.wav
```

Provisioning needs curl, jq, OpenSSL, tar and bzip2 support. It downloads and
hash-checks published native libraries; no runtime source compilation is needed.
The app downloader installs the same pinned libraries. Linux x86_64 and macOS
arm64 are measured; the manifest supplies runtime archives only for those targets.
The loader checks the exact C ABI version before constructing a recognizer.
Unsafe calls stay in the isolated worker. CPU arenas, memory patterns and busy
spinning are disabled through the native session configuration.

## Windows, timestamps and finality

Input is unchanged 16 kHz mono i16, converted to f32 by division by 32,768.
The adapter retains at most 30 seconds of PCM. After decoding a full window,
it advances 25 seconds and retains five seconds of overlap. A window is closed
only when more audio arrives, so stopping at exactly 30 seconds does not force
an extra tail decode. Every original sample enters at least one window; overlap
samples deliberately enter two.

Native tokens carry start times and TDT durations. UTF-8 byte-fallback tokens
are reconstructed before word boundaries are used; reconstructed text must
match the native raw transcript or recognition fails explicitly. Words ending
before the next window's start can be committed. The remaining words stay
provisional until the next window resolves the overlap.

The merge aligns overlap words one-to-one using timestamp intervals at the
encoder's 80 ms resolution. It maximizes timed matches, then minimizes timing
difference, and joins at the matched pair nearest the overlap's middle. The
earlier prefix includes that word once; the later suffix starts after its
matched counterpart. Continuous speech needs no artificial silence gap.
Words with no acoustic overlap are retained in time order. There is no string
comparison, deduplication or repetition filter. Each chunk supplies its own
edge space so committed strings concatenate directly.

The first partial is eligible after 500 ms of captured audio and wall time.
Empty hypotheses retry at that cadence; nonempty hypotheses schedule the next
update after the configured interval, measured from completion of decoding.
Each update decodes the open window from scratch and replaces `Partial`.
Partial decoding changes no final decoder state. Only closed prefixes emit
`Committed`; finish commits the remaining tail before the sole `Final`.
Cancel emits no usable late transcript. Committed bytes are never revised.

Timestamp alignment joins complete words and preserves repeated words in the
chosen hypotheses. It cannot prove that imperfect model timestamps or hypotheses match
every spoken word. The private comparison is against machine silver, not a
listening gold reference. See [results](../../bench/RESULTS.md) for measured
errors and the performance targets that remain unmet.

## One loaded worker and recovery

`WorkerClient` keeps at most one warm native process. The app unloads it after
five minutes without dictation. `SAID_MODEL_IDLE_SECONDS` is the single setting
(seconds; default 300, zero unloads immediately once idle). It is read at app
startup. Capture starts independently of model loading; the next hotkey starts
a fresh worker while every captured sample continues into the private disk spool.
Active and queued utterances retain the worker lease; unload never interrupts
recognition. Concurrent utterances wait for its
lease while their input continues into separate private disk spools. Successful
Final returns the worker; cancellation, a crash or malformed output destroys
it. The next lease starts a fresh process. A queued utterance's inference
watchdog starts after it acquires the worker, not while capture waits.

`WorkerClient::recognize_file` uses that same lease with a fresh utterance ID.
The existing standalone `recognize_file` helpers create independent workers.
The host retry executor calls the shared-client method for its current model,
avoiding a second loaded model beside live dictation.
PCM spools are 0600 files in a 0700 cache directory, immediately unlinked after
opening. Bounded queues signal available audio; original samples remain in
order on disk while recognition catches up. The recorder's durable WAV remains
authoritative. I/O failures, acknowledgment/identity mismatches, missing Final,
malformed framing, process exit and missed progress deadlines are explicit
errors. File retry never appends to the failed utterance's text.

The watchdog retains 32 busy half-second observations. Its budget is the larger
of the caller's floor and four times the larger of recent median work for a
bounded 30-second window and recent peak work for one half-second update.
Loading and the first nonempty decode retain the cold budget. Partial output
cannot keep a hung worker alive.

## Precision evaluation

FP32 remains the only application model. [The benchmark-only manifest](../../bench/precision-models.json)
pins the official INT8 export and the available community mixed FP16-encoder /
INT8-decoder-and-joiner export. Its filenames are selected from the hashed
manifest entries; there are no renamed-weight aliases or precision fallbacks
in the adapter. These exports are experimental benchmark options, not weaker
application choices. See [precision results](../../bench/RESULTS.md#precision-and-idle-memory-polish)
for both platforms, memory, the acceptance gate and reference limitations.

The fetch helper accepts a model ID and an optional manifest path:

```sh
crates/engine/scripts/fetch-model.sh parakeet-v3-int8 bench/precision-models.json
```

## Verification

```sh
CARGO_BUILD_JOBS=2 cargo test -p said-engine
CARGO_BUILD_JOBS=2 cargo clippy -p said-engine --all-targets -- -D warnings
cargo fmt -p said-engine --check
bench/scripts/ci.sh
CARGO_BUILD_JOBS=2 cargo test -p said-app --test end_to_end
```

The public synthetic fixtures can be regenerated on macOS without a microphone
or GUI. The boundary fixture includes a 25-second pause, exercising Parakeet's
window advancement while preserving all six spoken occurrences of “go”. Copy
only these synthetic WAVs to Linux before running the ignored native checks.

```sh
crates/engine/scripts/generate-clips.sh
CARGO_BUILD_JOBS=2 cargo test -p said-engine --test golden -- --ignored --nocapture
CARGO_BUILD_JOBS=2 cargo test -p said-engine --test public_regressions -- --ignored --nocapture
```

On Linux, run each native check separately under the [shared model lock and memory
scope](../../CONTRIBUTING.md#native-verification).
Direct/worker proof tests reload native sessions and can also retain allocator
pages after a direct model is dropped. Their cgroup budget includes reclaimable
file cache as well as resident allocations; keep these checks outside the editor's
own memory scope. The warm production worker reuses its loaded model.

The app's ignored worker/host comparison starts the expected recognizer in a
separate worker and waits for its exit before running the host. The test parent
never loads ONNX, so allocator tuning is unnecessary for that proof.

```sh
CARGO_BUILD_JOBS=2 flock "$HOME/.cache/said-model.lock" systemd-run --user --scope -q \
  -p MemoryMax=4G -p MemorySwapMax=0 -- \
  nice -n 10 cargo test -p said-app --test end_to_end -- --include-ignored
```

Protocol fuzzing, truncated/oversized frames, lifecycle identity, literal fake
traces, crashes, hangs, cancellation, producer drop after Finish and disk backlog
remain covered. The warm-worker regression also queues overlapping utterances
and file replay, verifying that only one process is started. Idle polling is
checked against its real deadline; an active lease resists unload. A delayed
cold-start worker receives all 12,800 samples fed before it loads, in order.
The native public regression preserves the initial “don't delete” phrase and
requires byte-identical warm/cold transcripts after unloading.
