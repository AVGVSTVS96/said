# Public results

The first runs below are retained as baseline evidence. The engine-fix update
at the end records the corrected pipeline and full completion; model defaults
remain provisional.

## First public results

Measured 2026-10-10 on Linux x86_64, AMD Ryzen 7 5700G (16 logical CPUs),
Moonshine Voice 0.1.5, CPU inference, no vocabulary bias or postprocessing.
The corpus has 28 clips and 208.34 seconds of audio: five AMI meeting excerpts,
twenty-one local Piper clips and two deterministic controls.

**No release default is certified.** Both candidates failed the long clip in
the first real-time run. Synthetic references still need human listening
acceptance; identifiers and paths can have TTS pronunciation mismatches.
Scores below describe successful clips only. Failed coverage is explicit,
and the table uses matching successful subsets from the identified runs.

## Comparable completed clips

Small uses the first attempt; Medium uses a full repeat after TTS generation
finished. Each completed the **same 27 clips**, excluding the long monologue.
Both full runs returned nonzero. These are comparable short-corpus scores,
not full-suite pass results.

| Engine | Completed | Literal WER | Normalized WER | Negative tokens kept | Lost / added / changed | Filler recall | Repeat recall | Identifiers exact | Technical terms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| moonshine-medium-en | 27/28 | 31.7% | 18.8% | 13/15 | 1 / 0 / 1 | 7/10 | 3/6 | 2/8 | 9/10 |
| moonshine-small-en | 27/28 | 34.5% | 24.7% | 12/15 | 3 / 0 / 0 | 7/10 | 3/6 | 2/8 | 9/10 |

| Engine | First partial p50 / p95 | Release→final p50 / p95, 5–20 s clips | Release samples | Cold load | Sampled total RSS | Silence/noise words |
|---|---:|---:|---:|---:|---:|---:|
| moonshine-medium-en | 815 / 1078 ms | 2089 / 8798 ms | 8 | 1604 ms | 782 MiB | 0 |
| moonshine-small-en | 874 / 1262 ms | 1128 / 7023 ms | 8 | 1197 ms | 445 MiB | 0 |

Both miss the measured first-words target of 800 ms p50 and release-to-final
target of 1 s p95. Both stay under the sampled 1 GB Linux RAM target in these
runs. RSS covers parent plus worker, sampled every 20 ms; it can miss short peaks
and count shared pages twice. First-partial statistics exclude two missing
partials per engine (silence/noise); their sample counts are in the JSON.
These measurements do not cover microphone-to-overlay or final-to-paste.

In the first attempt, Medium completed 26/28 (31.1% literal / 19.0% normalized
WER on that smaller subset). `ami-controls` completed in the idle-TTS repeat;
the long clip still failed with the explicit worker acknowledgment/final
deadline. The repeat does not establish that TTS caused the initial extra
failure; this is a shared host, not an isolated performance laboratory.
No other shared services were stopped.

## Failures and concrete retention losses

- Both first-attempt long-monologue runs failed before Final. The initial
  runner reported `replay stopped after engine failure`; that generic secondary
  error obscured the worker cause. The runner now preserves the original worker
  error, and the repeat uses that corrected reporting.
- Medium also failed `ami-controls` with the worker acknowledgment/final deadline.
  First-attempt startup/audio/final diagnostic limits were 60/10/30 seconds.
- Both reduced the authored `I I I don't don't` repetition. Repeats are counted,
  never silently deduplicated by scoring.
- Small emitted `neveruseforce` for the scripted `never use --force` and omitted
  the negative clause in the AMI project excerpt. This is literal lexical loss;
  the scorer does not infer whether a reader could recover the intended meaning.
- The clear `don't`/`do` and `never`/`always` twin pairs remained polarity-clean.
- Both missed most literal identifier spellings. Spelled-out path/flag wording
  is a separate clip; orthographic TTS references need listening adjudication.

Inspect [full raw outputs and events](results/linux-first.clips.jsonl) and
[first-run manifest/metrics](results/linux-first.summary.json), plus the
[idle-TTS repeat raw outputs](results/linux-medium-idle.clips.jsonl) and
[repeat metrics](results/linux-medium-idle.summary.json). References,
PCM hashes and counts are public. Corpus SHA-256:
`650c19b6436e8840c0f01d02fb2d8546e364c275d4761847268b017f132cbb6c`.

## Timed CI subset

`bench/scripts/ci.sh` completed its 12 real-engine recognitions in **63.2 seconds**
including the cached build, below the five-minute requirement. The command
returned **failure at the quality gate**: Small's normalized WER was **43.4%**
against the 35% ceiling. It returned an empty final transcript for `ami-project`,
although that same PCM produced a nonempty transcript in the full run. The
worker finished normally, so WER counts the lost words rather than pretending
this is a process crash. The discrepancy is observed; its acoustic/runtime
cause has not been isolated. The benchmark runs the engine crate unchanged.

The smoke gate also checks clean clear polarity twins, filler/repetition counts,
control hallucinations, missing clips, resource measurement and replay pacing.
Its tolerances are regression checks for the initial recognizers, not permission
to claim their text is verbatim. The separate pure scorer and external-manifest
checks pass (13 tests), as do formatting, clippy and source/clip hash verification.

Inspect [CI raw results](results/linux-ci.clips.jsonl),
[CI metrics](results/linux-ci.summary.json) and
[timing/exit receipt](results/linux-ci.timing.json).

## Platform defaults and scope

| Platform | Existing provisional manifest default | Public selection status |
|---|---|---|
| Linux | moonshine-small-en | Not certified: full-suite failure and negative-word losses |
| macOS | moonshine-medium-en | Not measured on macOS; Linux results cannot set a Mac default |

The model manifest is unchanged. A release decision requires listened synthetic
references, full completion, platform runs and the priority order in SPEC:
negation, then fillers/identifiers, then WER, then latency and RAM.
Fake is a transport/scorer fixture with no recognition ranking. Scribe has no
implemented crate adapter and no fabricated score. The private bake-off and its
provisional silver references were not modified or published.

## Reproduce

```sh
CARGO_BUILD_JOBS=2 cargo build --locked --manifest-path bench/Cargo.toml
crates/engine/scripts/fetch-model.sh moonshine-small-en
crates/engine/scripts/fetch-model.sh moonshine-medium-en
bench/scripts/fetch.sh
bench/target/debug/voice bench bench/clips.jsonl --all \
  --audio-root "$HOME/.cache/voice-models/bench-public"
```

A failed clip produces a nonzero exit and an incomplete coverage count. Do not
choose a default from WER on the successful subset alone. The checked-in records
are real measurements, not promised exact transcripts on every host.

## Engine fix: causes and verification

The corrected engine uses the same pinned Small/Medium weights and a patched
native runtime, `0.1.5+voice.2`. Defaults are unchanged. The source archive,
patch, executable and native-library hashes are recorded in each new summary.

The empty `ami-project` result was reproducible without decoding: native VAD
found no speech after the eight CI-prefix clips, but found four spans when the
same PCM ran alone. The crop's peak was 0.0135 and RMS about 0.0011. Silero's
recurrent state was process-global and was never reset between streams. Each
stream now owns independent VAD state. VAD isolation exposed an additional
empty decode on a quiet speech span; bounded input gain (target peak 0.25,
maximum 32×, fixed 500-ms blocks) addressed that level sensitivity. The WAV
and saved text are not normalized or edited.

A detected speech line with empty/whitespace text now emits the existing Error
with its time range, without Final. This catches an empty segment inside a
nonempty recording too. Silence with no detected speech still finishes empty.
There is no new core event or worker message.

The long-run failure combined queued-frame deadlines with inference slower
than replay. Queue age counted against future work, then bounded PCM queues
could overflow. The host now spools audio to an immediately unlinked private
disk file and sends bounded frames in order. Accepted Finish drains the spool
and all acknowledgments, including after the producer handle is dropped.
Cancellation before Start closes the output stream. No PCM is deduplicated.

The watchdog measures busy RTF over half-second windows, retains the most
recent 16 seconds of consumed audio, and forecasts bounded segment/update work
from its median and peak with fourfold headroom. It does not multiply a stall
by the entire queued recording. Model loading and first decoder warmup are
separate: an empty-audio acknowledgment does not prove the decoder has run.

The native duration fade was not a hard bound and could reuse speech look-behind
at forced splits. Continuous segments now have a 15-second budget and disjoint
sample ownership. VAD/frontend remainders are padded rather than dropped.
Stop forces final decoding even with no new audio; encoder lookahead is flushed
even when all frontend samples were consumed. CPU inference uses two intra-op
threads, one inter-op thread and no spinning. The worker remains CPU-only.

The three-minute synthetic native regression verifies every original sample
exactly once, contiguous segment times, bounded continuous segments and only
zero padding, across five input fragment sizes. Real regressions run both models:
the CI prefix followed by the quiet AMI crop twice, an exact eight-second
recording ending in the spoken word "Stop", and the 98.13-second monologue with
four occurrences of each passage anchor. The golden pause fixture retains all
six spoken occurrences of "go" and preserves direct/worker transcript bytes.
The slow-worker test passes with queues of one and two frames and compares all
160,000 consumed synthetic samples byte for byte; hangs still terminate.

These checks prove transport/sample ownership and the specified regressions.
They do not prove zero recognizer errors at an arbitrary word crossing a forced
boundary. Hard splits can change recognition of a spanning word. Listening
acceptance for the authored synthetic references remains outstanding.

### CI gate calibration

The baseline CI failure reproduced at 43.4% normalized WER, including the empty
AMI result. The successful baseline short-run subsets were 16/64 errors (25%)
on eight synthetic speech clips and 12/42 (28.6%) on two AMI crops. The new
ceilings, chosen before the patched run, are 30% synthetic and 40% AMI, plus an
independent nonempty-text requirement for every speech clip. Their maximum
combined allowance is 35/106 errors, compared with 37/106 under pooled 35%.
An empty AMI transcript cannot pass either the completeness or subset gate.

These AMI crops are **headset-channel meeting audio**, not far-field microphones.
Spontaneous disfluencies and background leakage justify a separate stress
subset. Negation, clear polarity twins, filler opportunities, repetitions,
control hallucinations, replay pacing and resource measurements remain gated.

### Build and handoff

Only `crates/engine` and `bench` changed. No shared type change is needed.
Provisioning now requires CMake 3.22+ and a C++20 compiler in addition to the
existing download tools; CMake 4.2.3 was installed through uv for the Linux
build. Packaging/downloading must supply the patched native library, using
`fetch-model.sh`/`build-runtime.sh` or an equivalently built artifact. The loader
rejects an unpatched library instead of silently restoring these defects.

Linux used two Cargo/native-build jobs on the shared CPU host. macOS used the
installed Rust 1.96.1 toolchain, an isolated worktree, idle checks and `nice -n 10`.
No GUI app, microphone recording, permission prompt or desktop input was used.

### Before and after: identical 27-clip Linux subset

Both completed the same 27 short clips before and after. The long clip is excluded
from this comparison because neither baseline completed it.

| Engine | Literal before → after | Normalized before → after | Negatives kept before → after | Filler kept | Repeats kept |
|---|---:|---:|---:|---:|---:|
| moonshine-small-en | 34.5% → 33.5% | 24.7% → 23.3% | 12/15 → 12/15 | 7/10 → 7/10 | 3/6 → 3/6 |
| moonshine-medium-en | 31.7% → 31.7% | 18.8% → 17.4% | 13/15 → 13/15 | 7/10 → 7/10 | 3/6 → 3/6 |

Small still drops three negatives. Medium keeps the same 13/15, but its error
mix changes from one dropped plus one changed to two dropped. Identifier exactness
remains 2/8; technical terms remain 9/10. This is modest WER improvement, not a
verbatim certificate or improved negation retention.

### Full corrected corpus: 28 clips per engine and platform

| Platform / engine | Completed | Literal WER | Normalized WER | First partial p50 / p95 | Release p95, 5–20 s clips | Sampled RSS |
|---|---:|---:|---:|---:|---:|---:|
| linux / moonshine-medium-en | 28/28 | 22.3% | 12.3% | 854 / 3092 ms | 20770 ms | 778 MiB |
| linux / moonshine-small-en | 28/28 | 23.1% | 14.9% | 784 / 2130 ms | 3565 ms | 442 MiB |
| macos / moonshine-medium-en | 28/28 | 22.3% | 12.5% | 596 / 2188 ms | 961 ms | 2171 MiB |
| macos / moonshine-small-en | 28/28 | 23.8% | 15.4% | 578 / 2083 ms | 733 ms | 2050 MiB |

The lower full-suite WER includes the easy, repeated long script and must not be
compared directly with baseline 27-clip WER. Linux is the shared Ryzen 7 5700G
CPU host; macOS is Apple M1 Max, CPU inference at `nice -n 10`. Other tasks and
validation ran concurrently. Timing differences are not an isolated speed
experiment. RSS is sampled parent + direct worker, can double-count mappings,
and is not macOS physical/unified-memory footprint. The measured macOS RSS is
over the spec's 1.5 GB target. Linux release p95 remains over its 1-second target.

| Platform / engine | Long clip normalized WER | Release → final | Long negative tokens kept |
|---|---:|---:|---:|
| linux / moonshine-medium-en | 7.6% | 231.332 s | 16/16 |
| linux / moonshine-small-en | 6.9% | 62.466 s | 16/16 |
| macos / moonshine-medium-en | 7.6% | 0.412 s | 16/16 |
| macos / moonshine-small-en | 6.9% | 0.197 s | 16/16 |

The 98.13-second clip completed everywhere. All 16 long-script negative tokens
were retained. Fillers remain weak: Small keeps 1/8, Medium 3/8; both keep 4/8
repetition opportunities. Pipeline sample conservation does not fix model omissions.

### Final checks and receipts

- `cargo test -p voice-engine`: pass, including protocol mutation fuzzing,
  cold/slow/hung workers, producer drop after Finish, cancel before Start and
  private unlinked spooling. Native golden and public regressions were also
  explicitly run with `--ignored --nocapture`, not counted as skipped evidence.
- `cargo test --locked --manifest-path bench/Cargo.toml`: 13 checks pass.
  Engine/bench clippy with `-D warnings`, formatting and shell syntax checks pass.
- `crates/engine/scripts/test-native.sh`: pass on Linux and macOS.
- `bench/scripts/ci.sh`: Linux pass, 12/12 clips; normalized WER **24.5%**
  versus reproduced baseline **43.4%**. Subsets are synthetic 16/64 (25%)
  and AMI 10/42 (23.8%). Empty speech is forbidden.
- macOS CI subset + the same checker: pass; macOS real golden: pass.
- Full public `--all --audio-root`: both engines 28/28 on both platforms,
  zero failed clips, no control hallucinations. Defaults remain uncertified.

Receipts: [Linux full metrics](results/linux-engine-fix.summary.json),
[Linux raw events](results/linux-engine-fix.clips.jsonl),
[macOS full metrics](results/macos-engine-fix.summary.json),
[macOS raw events](results/macos-engine-fix.clips.jsonl),
[Linux CI metrics](results/linux-engine-fix-ci.summary.json),
[Linux CI raw events](results/linux-engine-fix-ci.clips.jsonl),
[macOS CI metrics](results/macos-engine-fix-ci.summary.json), and
[macOS CI raw events](results/macos-engine-fix-ci.clips.jsonl).

The corpus/reference statuses and all WAV hashes are unchanged. Inspect the
receipts' actual hashes rather than expecting byte-identical timings on a busy host.
