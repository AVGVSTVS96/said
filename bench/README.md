# Benchmark

`said bench <manifest>` replays 16 kHz mono PCM i16 WAVs through the unchanged
`said-engine` worker and scores its committed text. It concatenates committed
chunks with no separator. Partial text is timed and recorded, never promoted to
final text. Recognition has no text postprocessing.

Run every model-loading command below with the [shared lock and memory scope](../CONTRIBUTING.md#native-verification).

The app provides `said bench` through this library and uses its own `--worker` dispatch.
The `said-bench` development probe is a workspace target for isolated tests; it is not shipped.
The application defaults to its installed data/models directory; use `--model-cache` explicitly
for the shared development cache. The probe defaults to `~/.cache/said-models`.

```sh
CARGO_BUILD_JOBS=2 cargo build --locked --manifest-path bench/Cargo.toml
target/debug/said-bench bench bench/clips.jsonl --ci
```

That command selects the bundled platform default. The native Parakeet default runs with no engine flag, or select
`--engine parakeet-v3-fp32`. `--all` runs the installed manifest catalog. The fake worker is for protocol checks:
`--engine fake --trace /path/to/scripted-events.json`.

The 12 CI clips are checked into `bench/clips/`. The full corpus adds a long
synthetic monologue kept outside git. Provision it, then run all engines:

```sh
crates/engine/scripts/fetch-model.sh
bench/scripts/fetch.sh
target/debug/said-bench bench bench/clips.jsonl --all \
  --audio-root "$HOME/.cache/said-models/bench-public"
```

Model provisioning needs curl, jq, OpenSSL, tar and bzip2 support; the long clip also
needs `uv`. Public source URLs and SHA-256 hashes live in `sources.json`.
`recipes.jsonl` pins AMI crop times, verbatim references, TTS input and control
seeds. The fetch script verifies source and derived WAV hashes, uses one Piper
model at a time, and generates one clip at a time. Zero generator/width noise
reduces randomness; a different toolchain may still produce different PCM.
Such a run fails the hash check rather than silently changing the corpus.
Short clips are bundled, so CI needs neither TTS nor AMI downloads.

## Public references

There are five AMI excerpts, twenty-one Piper clips and two controls. The
synthetic text and control audio are dedicated to CC0. AMI excerpts and
transcripts retain CC BY 4.0; see [LICENSES.md](LICENSES.md) for attribution,
provenance and upstream sources. No private recordings, private transcripts,
silver labels, private outputs or personal research are included.

AMI references come from manual word XML, retaining repetitions, fragments
such as `charac`, hesitations and contractions. Punctuation attaches to the
preceding word; a previous utterance's boundary punctuation is excluded.
The speaker-to-headset mapping is recorded in upstream `meetings.xml`.
Excerpts preserve the original headset channel, including background leakage.
Upstream annotation is a reference, not a guarantee of perfect acoustic truth.

Piper scripts cover negative/affirmative twins, start/end negatives,
backchannel `no`, fillers, repeats, false starts, self-corrections, identifiers,
paths, CLI flags, versions, numbers, profanity, one word and a long monologue.
They carry `reference_status: authored_unreviewed`: **these generated clips
have not received human listening acceptance**. Orthographic identifier/path
references are especially provisional: TTS can pronounce spelling differently.
The separate `spoken-path` clip tests literal spoken path/flag wording.
The first results are engineering evidence, not a certified engine selection.
Listen, adjudicate references, update their status and rerun before choosing a
release default. Never use recognizer agreement as acoustic adjudication.

## Manifest and private runs

`--unpaced` feeds audio as quickly as the disk spool accepts it, through the
same adapter and protocol. It is for file fidelity/throughput runs. The summary
labels that mode explicitly; its first-partial/release times are not live-input
latency measurements. Real-time pacing remains the default. Failed clips score
as empty hypotheses and count as deletions; successful latency samples retain
separate denominators.

One JSON object per line, UTF-8. Blank lines are allowed; unknown fields,
duplicate IDs, invalid terms, missing hashes and incomplete polarity twins fail.
`audio` resolves relative to the manifest, or `--audio-root` overrides the root.
Absolute paths are used unchanged. Every selected WAV is hash-checked before
any engine starts; audio is streamed in 1,600-sample blocks, not loaded whole.

```json
{"id":"example","audio":"clips/example.wav","sha256":"<64 hex digits>","reference":"Um, don't use --force","tags":["filler","negation","flag"],"license":"CC0-1.0","source":"synthetic example","reference_status":"listened","terms":[{"text":"--force","kind":"flag"}],"ci":true}
```

`terms` are optional and use `technical`, `identifier`, `path` or `flag`.
Each term must occur in the reference. Use one entry per distinct term; every
occurrence is scored. Optional `pair` + `polarity` identify a negative and its
`affirmative` twin; every pair must have exactly one of each. `ci` selects the
small subset. Tag silence and noise explicitly with an empty reference.

The identical command runs private audio outside the repository:

```sh
target/debug/said-bench bench /private/corpus/clips.jsonl \
  --engine parakeet-v3-fp32 --unpaced --output /private/results/new-run
```

Private references use `license: private` and an honest `reference_status`
(such as `human_verbatim`, `silver_unreviewed`); the benchmark makes no claim
that an arbitrary reference is ground truth. Existing private bake-off manifests
need their fields mapped to this documented schema outside the repository.
The benchmark never changes an input manifest or WAV.

Results default to a new directory under `~/.cache/said-models/bench-runs/`,
not the repo. Each directory is 0700 and each output file 0600. Explicit output
paths must be new. `clips.jsonl` records the reference, raw text, every received
event/timestamp, scores and failures. `summary.json` records aggregates, strata,
model hashes, native library hashes, runtime version, provider/thread/chunk settings, corpus/binary/dependency
hashes, deadlines and host CPU/OS/architecture; it omits hostname and input paths.
Treat private results as private: they contain transcripts and reference text.
An incomplete run exits nonzero; failed clips never masquerade as empty success.

## Scoring

- **Literal WER:** whitespace-delimited tokens, preserving case, punctuation,
  contractions, fillers and repeats. Edit distance uses deterministic ties:
  diagonal, then deletion, then insertion. Numerator is S + D + I; denominator
  is reference tokens. Corpus WER pools counts, never averages clip percentages.
- **Normalized WER:** Unicode NFC, lowercase, curly-to-straight apostrophes,
  alphanumeric/contraction tokens. Punctuation and symbol differences disappear
  only in this comparison metric. It is never applied to the saved output.
- **Negation:** aligned tokens `not`, `never`, `no`, `without`, `neither`, `nor`,
  `cannot`, and contractions ending in `n't`. Count lost, added and changed
  negative tokens separately. A negative changed to a different negative is
  `changed`; this is a lexical measure, not a semantic polarity classifier.
  Affirmative twins expose added negatives. `no` backchannels count explicitly.
- **Fillers:** `um`, `uh`, `uhm`, `erm`, `er`, `hmm`. Ambiguous lexical `like`
  is not automatically a hesitation. Recall and precision use aligned tokens.
- **Repetition:** each adjacent equal word after the first is an opportunity;
  extra repeated words lower precision. Nonadjacent rephrasings remain in WER.
  No reference or hypothesis is deduplicated.
- **Technical terms:** normalized, contiguous, exactly aligned tagged phrases.
- **Identifiers, paths, flags:** contiguous, exactly aligned literal tokens,
  retaining case and symbols. Include adjacent punctuation in the tagged term
  when present in its reference token. Normalized WER cannot hide these losses.
- **Hallucination:** normalized words produced on tagged silence/noise. Empty
  reference WER and zero-opportunity retention are null, not artificial zero.
- **Latency:** real-time replay by default, pacing each block to its sample end.
  First nonempty Partial and first text (Partial or Committed) are separate.
  Release is immediately before Finish; final timing includes pending inference.
  p50/p95 use nearest rank; missing partials and sample counts are explicit.
  Release latency is also broken out for the spec's 5–20-second budget.
  Max replay lag shows host scheduling/backpressure. Unpaced runs report their separate timing scope.
- **RAM/load:** sampled total RSS of host plus direct worker every 20 ms; shared
  mappings can be counted twice and short peaks can be missed. On macOS this
  uses `ps`; it does not measure unified-memory physical footprint. Cold load
  times process launch through the worker's first empty-audio acknowledgment.
  Model hashing and warm inference are separate from cold load.

A worker stays loaded between clips and is released before the next model.
Startup and audio/final no-progress floors default to 60/10/30 seconds; override
with `--startup-seconds`, `--audio-seconds`, `--finish-seconds`. They are not
production latency recommendations. The engine learns busy real-time factor
from acknowledgments. It budgets fourfold measured work: the larger of rolling
median RTF for a bounded 30-second speech segment and recent peak RTF for one
half-second update, plus the caller's floor. Measurements cover the last
16 seconds of consumed audio; the entire queued recording never multiplies a
stall allowance. Pending queue age does not shorten later
frames' allowances. The benchmark records learned busy RTF and runtime source/patch
hashes, and delegates finish supervision entirely to the engine. Engine/model/runtime manifests can be
selected with `--model-manifest` and `--model-cache`; model hashes are verified.
Alignment uses two cost rows and a byte per traceback cell. A score exceeding
64 million cells fails explicitly; split extremely long reference material into
clips rather than exhausting the server.

## Checks

```sh
cargo fmt --manifest-path bench/Cargo.toml --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --manifest-path bench/Cargo.toml --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path bench/Cargo.toml
bench/scripts/ci.sh
```

The scorer checks run without corpus/audio integration; the external-manifest
check proves worker use, raw byte preservation and hash rejection. `ci.sh` builds the runner, executes the public CI subset and validates the summary
with `scripts/check-ci.py`. Its thresholds detect lexical regressions; they are
not a fidelity or release certificate. Run it under the shared model lock and
memory scope. Real-model provisioning is separate from the pure CI job. See
[RESULTS.md](RESULTS.md) for dated failures, reference limits and acoustic metrics.

The host owns the production `said` CLI and routes benchmark worker flags through this same
entrypoint before Tauri initialization. The root workspace/lockfile owns dependencies; pure scorer
and fake-worker checks run in CI. The real quality gate remains separate and currently fails as
reported in RESULTS.md. Model defaults remain provisional.

## Precision probes

`precision-models.json` is the benchmark-only catalog for the measured FP32,
INT8 and mixed FP16-encoder / INT8-decoder-and-joiner exports. All sources and
weight hashes are pinned; application selection still uses `crates/engine/models.json`.
The pinned upstream FP16 repository contains no model files, so the available
community mixed export is labeled explicitly. No all-FP16 result is claimed.

```sh
crates/engine/scripts/fetch-model.sh parakeet-v3-fp16-int8 bench/precision-models.json
flock "$HOME/.cache/said-model.lock" systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0 -- \
  nice -n 10 target/debug/said-bench bench /private/clips.jsonl --unpaced \
  --engine parakeet-v3-fp16-int8 --model-manifest bench/precision-models.json \
  --output /private/new-result
```

That resource scope is Linux-specific. Run one model-loading probe at a time;
on macOS use `nice -n 10`. The private headline comparison uses the original
external silver scorer, not this public scorer's narrower negation/repetition
classes. Only aggregate precision receipts are published in
[RESULTS.md](RESULTS.md#precision-and-idle-memory-polish); audio, manifests with
private references, clip identifiers and transcripts stay outside the repository.
