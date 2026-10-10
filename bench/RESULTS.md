# First public results

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
target/debug/voice-bench bench bench/clips.jsonl --all \
  --audio-root "$HOME/.cache/voice-models/bench-public"
```

A failed clip produces a nonzero exit and an incomplete coverage count. Do not
choose a default from WER on the successful subset alone. The checked-in records
are real measurements, not promised exact transcripts on every host.
