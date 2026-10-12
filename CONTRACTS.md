# Contributor contracts

The Rust definitions are authoritative: `crates/core/src/types.rs`, the Engine/Recorder/Platform/History traits and `crates/core/migrations/001.sql`. This document records units and ordering that signatures cannot express. Product behavior and acceptance limits are in [SPEC.md](SPEC.md).

## Identity and text

- `UtteranceId` is a UUID v4. Every capture/recognition event carries its identity; unknown or obsolete events are ignored.
- `audio_id` identifies the original WAV. Retry uses a fresh utterance/recognition ID and shares the old audio ID. Remove the WAV after its last row reference is gone.
- A `Target` is identified by `app_id` plus opaque `window_id`; `name` is display-only. A process or title alone is insufficient window identity.
- `Committed` appends bytes and clears the provisional tail. `Partial` replaces that tail. The displayed raw transcript is committed bytes plus the latest partial; stored raw text comes from committed chunks after `Final`. Core adds no separators or normalization. Adapter spacing/window reconciliation belongs to the finalized-adapter boundary.
- UTC Unix milliseconds are used for persisted timestamps. Durations are milliseconds. Hold/tap, scheduling and inference deadlines use monotonic host time.
- Pending rows have null recognition fields. An empty recognition stores empty strings with `empty` status. Original text and audio identity become immutable; edits occupy the separate `text` field.

## Capture and recognition

Create and acknowledge a durable pending row before starting capture. Start the engine before delivering live frames. Recorder frames are 16 kHz mono signed i16 with a 0..1 level (frame RMS on a -60..0 dBFS scale). A missing device choice resolves the system default when capture starts.

The recorder writes durable audio independently of a slow live receiver. Stop drains the capture ring, resampler and queued frames, repairs/syncs the WAV, then emits `Stopped`. Feed the last frames before asking the engine to finish. A live-frame gap invalidates that recognition; replay the WAV. A capture failure without a durable stop is not a successful recording.

Each Engine instance has one identity and an event receiver. `feed` takes unchanged samples. `finish` eventually emits the remaining committed text followed by one `Final`. Cancel invalidates late text. Overlapping captures have distinct durable WAVs and spools; the worker lease serializes native inference. Recovery allocates a fresh identity, repairs/reads the WAV and replays through the same protocol. Recovered rows never auto-paste.

## Worker stream

The codec is `said_engine::protocol`. stdin carries requests, stdout carries responses, stderr carries diagnostics. Each UTF-8 JSON block has a u32 little-endian byte-length prefix. An Audio request immediately adds a separately length-prefixed little-endian i16 PCM block.

Start precedes Audio; Finish follows the final audio block. Audio sequence numbers start at zero and increment by one. AudioAck follows adapter consumption of that block. It does not prove durable capture or final recognition. Events and acknowledgments retain the utterance ID. Final follows all committed chunks and ends transcript output for that recognition.

JSON and PCM blocks are each bounded to 1 MiB; PCM lengths must be even. Split long input into bounded frames. A clean EOF before a prefix is distinct from a truncated prefix/body. Malformed JSON, oversized frames, wrong identities or sequence errors fail the worker; framing errors cannot be safely resynchronized. Use the shared codec in host, worker and bench.

## Save and delivery

`Session::reduce` returns ordered effects. The host feeds success acknowledgments back after an effect has actually succeeded. A `Save` request alone never authorizes paste. Completed durable Save precedes CheckFocus; FocusChecked can authorize Paste; platform success precedes Pasted and delivery metadata. Failed saves do not paste. Finishing utterances deliver in start order.

Observed focus departure permanently inhibits automatic delivery for that utterance. Explicit interactive insertion can refocus the original window, then verify identity. Insert last uses the current external target. Native focus observation has platform-specific acceptance limits; see [SPEC.md](SPEC.md#verification-and-limits).

`paste` rechecks target identity before injection, writes the exact supplied text to the clipboard and posts the configured chord. Successful return means posted, not accepted. Leave the clipboard changed. Do not retry an uncertain chord automatically. `send` defaults false and applies only to explicitly enabled quick dictation. Interactive/insert-last always pass false. Copy-only mode must not report a successful paste.

Hotkey channels have one consumer; cloned receivers compete. Native Down/Up edges and compositor press-only commands carry different release capability into `Session::hotkey`. Avoid processing one physical press through both routes. Session-unavailable signals stop capture and inhibit delivery before native listeners are disabled.

## Persistence and renderer

Schema version 1 uses a STRICT utterance table, transactional migrations, immutable-original/audio triggers and external-content FTS5. Edit changes edited text, delivery updates delivery metadata, and delete removes the row/search entry transactionally. Shared WAVs outlive individual retry rows. File deletion can fail after the transaction; startup orphan cleanup handles leftovers.

The host sends complete sequenced snapshots, except that `history` is null when it has not changed since the last snapshot; renderers keep their previous rows. A window may skip sequences that only changed levels. Renderers discard duplicates/older sequences. Actions resolve after application/publication and reject on failure. Renderer drafts survive failed edits; explicit insertion waits for the latest edit. `SessionView.edited` owns accepted edits, including the empty string. Streaming updates do not overwrite user-owned regions.

TypeScript types are exported from Rust into `app/src/bindings`; the renderer imports `contracts.ts`. The renderer never reads engine pipes or performs native paste. Playback takes an existing utterance ID and resolves its shared WAV in the host; arbitrary renderer filesystem paths are not accepted.

Fakes are deterministic, finite and headless. They test ordering and literal text preservation without native permissions, clipboard or input. A passing fake trace does not establish acoustic fidelity or destination acceptance.
