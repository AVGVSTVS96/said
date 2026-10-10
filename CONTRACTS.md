# Milestone 0 contracts

The code in `crates/core/src/types.rs`, the Engine/Recorder/History/Platform traits and `crates/core/migrations/001.sql` is the shared contract. The integrator reconciles shared changes; the core agent owns additive type changes in the core implementation wave. This document supplies units, ordering and ownership that the type signatures cannot express.

## Identity and text

- `UtteranceId` is a random UUID v4, serialized as a lowercase hyphenated string. IDs are created by the host before capture; events always carry that ID. The reducer ignores unknown or completed IDs.
- `audio_id` is the original capture's ID. A retry creates a new utterance ID and keeps the original audio ID, resolving to `audio/<audio_id>.wav`. Do not copy the WAV. Deleting a row removes the WAV only after the last reference is gone.
- `Target` uses an opaque string window ID so Niri numeric IDs and native macOS IDs share one format. Identity is `app_id + window_id`; `name` is display-only. macOS adapters must provide window identity, not merely process identity.
- Transcript = committed chunks concatenated with **no separator**, plus the current partial for display. Partial replaces the previous tail. Committed appends bytes and clears the partial. The engine must commit the last tail before Final; core never promotes provisional text itself. No normalization at any boundary.
- Times ending in `_at` are UTC Unix milliseconds; durations are milliseconds; UI/wire numbers must fit JavaScript's safe integer range. Timing measurements use a monotonic clock in the host. No wall-clock subtraction for hold/tap or latency.
- Pending rows have `raw_text = text = status = NULL`. Empty recognition becomes two empty strings and status `empty`; it is different from unfinished recognition. Completed raw text is immutable. Edited text is separate. `pasted_to` is Target JSON in SQLite, nullable with `pasted_at`.

## Engine and audio

An Engine instance is scoped to one utterance ID. Its constructor receives the ID and supplies an event receiver to the host. The trait follows the spec's four methods exactly. `start` loads/starts with explicit vocabulary; `feed` accepts 16 kHz mono signed i16 samples; `finish` flushes asynchronously and eventually emits Final; `cancel` produces no usable late transcript. Overlapping recordings and finishing utterances are scheduled by the host; do not reuse one instance's identity.

`FakeEngine::new(id, Trace)` returns the engine and an `EngineOutput` receiver. `Trace.start` is emitted by start, each feed takes the next event group, and finish emits `Trace.finish`. Calls record exact vocabulary and samples. Fixtures decide the event timing; there are no sleeps or recognizer dependencies.

Recorder owns capture and durable audio, not transcription. `start(id, device)` returns one receiver of AudioEvent; device None means the system default at start. Frames contain normalized samples and a 0..1 meter value. Stop drains queued samples and flushes/repairs the WAV before emitting Stopped with duration. Only then may the executor call Engine::finish. An unplug emits Disconnected, stops at the last captured frame, then follows the same drain/Stopped path. The callback's production ring and host queues must be bounded; fake receivers are unbounded only for finite scripted tests.

FakeRecorder emits its synthetic frames on start and Stopped on stop. It never touches a microphone or disk. The native recorder owns WAV recovery; core creates the durable pending row before requesting capture.

## Platform

Platform retains the spec's methods, with one structural adaptation: `type Window` replaces the concrete Tauri window in `make_overlay`. Production adapters set it to `tauri::Window`; FakePlatform uses `()`. This keeps desktop dependencies out of the pure crates and tests. Native window creation/compositor plumbing belongs to app/platform, not core.

Hotkey is Down, Up, Interactive, InsertLast or Cancel. Use one consumer of `hotkeys()`; channel clones compete, they are not broadcast. The host supplies capture identity, focus snapshot and settings in StartContext. `Session::hotkey` receives the host monotonic clock and release capability to normalize hold/tap and toggle-only inputs. Direct HotkeyDown/HotkeyUp events are already-normalized starts/stops.

`focused()` captures the target at start and checks it again at delivery. `focus()` is only for explicit interactive insertion. `paste()` must recheck the target immediately before injection, set the clipboard to the exact bytes, emit the configured chord and send Enter only for explicit `send = true`. Clipboard stays changed. A successful return means the paste chord was sent, not that the target accepted it. Default send is false; interactive and insert-last always use false.

FakePlatform allows explicit target replacement, records focus/paste/overlay calls and rejects a paste to an unfocused window. It never uses the system clipboard, desktop or keyboard.

## Worker stream, version 1

stdin is requests, stdout is responses. stderr is diagnostics; no logs on stdout. Each JSON block is UTF-8 preceded by **u32 little-endian byte length**. An Audio request is followed immediately by another u32 little-endian length and raw little-endian i16 PCM. No other message has a PCM block. Request and response codecs live in `voice_engine::protocol`; use them unchanged in the worker, host and bench.

```json
{"type":"start","id":"00000000-0000-4000-8000-000000000001","vocabulary":[]}
{"type":"audio","id":"00000000-0000-4000-8000-000000000001","sequence":0}
{"type":"finish","id":"00000000-0000-4000-8000-000000000001"}
{"type":"cancel","id":"00000000-0000-4000-8000-000000000001"}
{"type":"event","id":"00000000-0000-4000-8000-000000000001","event":{"type":"committed","text":"no no "}}
{"type":"event","id":"00000000-0000-4000-8000-000000000001","event":{"type":"final"}}
{"type":"audio_ack","id":"00000000-0000-4000-8000-000000000001","sequence":0}
```

Sequence starts at zero per utterance and increases by one per Audio block. AudioAck echoes the sequence after the adapter has consumed that block. This is an explicit addition to the four recognizer events: the spec requires audio acknowledgments for its hang detector but did not name their wire message. Ack does not prove recognition completed or the WAV was persisted. Start must precede Audio; Finish follows the last Audio; Cancel aborts that ID. Final is emitted once after every final chunk; no transcript events follow it. The worker dispatcher, not the stateless codec, enforces this lifecycle.

JSON and PCM blocks each have a 1 MiB maximum. This bounds an individual allocation, not recording duration. Larger audio is split into frames. PCM byte lengths must be even; empty blocks are allowed and acknowledged. Clean EOF before a JSON prefix returns None. A partial prefix/body or a missing PCM block is an error. Unknown message types, malformed JSON and oversized prefixes are errors. Writers validate both payload lengths and Audio/PCM pairing before writing, then flush a complete frame. The host discards/kills the worker on a framing error; it cannot safely resynchronize. The watchdog interval comes from the benchmark, not a constant in this codec.

## History and storage

Schema migration 001 creates a STRICT utterances table, two ordering/reference indexes and external-content FTS5 over original and edited text. Triggers track insert/update/delete and reject changing an original or audio identity. Run migrations transactionally, checking `PRAGMA user_version` (currently 1). Do not rerun migration 001 over an existing database.

History::insert creates a pending or completed row. History::finish fills a pending row once. edit changes only edited text; record_delivery changes delivery metadata. search is an FTS query returning newest first; recover returns rows without recognition. delete removes the row/search entry atomically and returns the audio ID only when no remaining row references it. The history implementation owns crash-safe file deletion/recovery sequencing. No recovered row is auto-pasted.

FakeHistory is in memory. Its search is a literal substring convenience, not an FTS emulator. `SqliteHistory` implements the production connection, transactional migrations, FTS and audio deletion lifecycle; persistence tests exercise reopen and crash cleanup.

`product.toml` is the source of display name, stable bundle ID and stable storage key. Rename **name** only; preserve the other two. This deliberately uses the storage key instead of the spec's display-name directory so a later rename cannot strand data:

- macOS: `~/Library/Application Support/<storage_key>/` for settings, vocabulary, history, audio and models.
- Linux: `$XDG_CONFIG_HOME/<storage_key>/` for settings/vocabulary, `$XDG_DATA_HOME/<storage_key>/` for history/audio, `$XDG_DATA_HOME/<storage_key>/models/` for downloaded models. Standard XDG fallback directories apply.
- Linux control socket: `$XDG_RUNTIME_DIR/<storage_key>.sock`, mode 0600. Data directories 0700, personal files 0600. Stable storage key also prefixes any UI preference keys.

## Reducer and UI boundary

Session::reduce accepts typed events and returns ordered effects. The executor starts the engine before capture, drains/stops audio before finishing recognition, runs Save durably and feeds Saved back only after success. A Save effect alone cannot authorize paste. Saved enables CheckFocus; FocusChecked may authorize Paste; only an adapter success feeds Pasted back and records delivery. Failed saves never paste. Multiple finishing entries are delivered in start order. Unknown/late engine events do nothing.

The reducer covers hold/tap, interactive editing/insertion, insert-last, recovery, errors/restart, disconnect handling, overlay first-frame timing and completed-entry eviction. Every edge-case row has a trace against fakes. Native audio durability and actual desktop input remain adapter acceptance tests; the host must execute the effects and return the acknowledgments described below.

Shared types are exported by ts-rs at Rust build time into `app/src/bindings`. The export binary makes regeneration explicit even with a cached build. SessionEvent uses adjacent `type`/`data` tagging so nested AudioEvent tags cannot collide. UI imports `app/src/contracts.ts`, displays SessionView and sends UiAction through host IPC. SessionView.level is None until the first microphone frame and then carries the latest meter value, so the UI can distinguish waiting for audio from Listening. The host's action dispatcher and event subscription are not wired yet. UI never reads engine pipes or performs paste.

## Core implementation wave

These additions preserve the Engine, Recorder and Platform traits, engine events, worker wire format,
History trait and schema version 1.

- `SessionView.edited: Option<String>` is the interactive editor value. `committed` and `partial`
  remain the raw engine display. Before the first edit, render their concatenation. Editing final
  text leaves the provisional tail live. Editing inside the provisional tail protects that tail
  through its next commit; subsequent chunks keep streaming. On worker replay an existing editor
  value stays user-owned. No editor operation changes raw history.
- New `SessionEvent` variants: `Ui(UiAction)`, `FocusChanged { target }`, `Recover { utterance }`,
  `InsertLast { utterance }`, `SessionUnavailable { reason }`, `SessionAvailable`, `EditSaved { id }`
  and `EditFailed { id, message }`, plus `ReplayReady { id, recognition_id }`. Feed every focus change, including a closed target, into core;
  returning to the original window does not restore automatic delivery. Session unavailability
  stops capture, inhibits automatic delivery for those recordings and exposes the tray reason
  through `Session::unavailable_reason()`.
- New `Effect` variants: `RecognizeFile { id, audio_id, engine, vocabulary }`,
  `FocusTarget { id, target }`, `Copy { id, text }`, `EditHistory { id, text }`,
  `SetStatus { id, status }`, `LoadLast` and `AllocateReplay { id }`. Pending creation reuses `Save`; there is no separate
  pending-save effect. Discard and cancellation share `SetStatus`.
- `Save` with null recognition fields means insert a pending row durably. Only its `Saved` response
  starts engine/capture. `Save` with completed recognition means finish that row exactly once;
  `Saved` then authorizes automatic delivery. Save failures do not paste. The executor must reply
  once per operation, in operation order for each ID, and must never acknowledge a failed save.
- `AllocateReplay` asks the host for a fresh UUID v4, returned as `ReplayReady` for the interrupted
  ID. The reducer itself never draws randomness or reads a clock.
- `RecognizeFile` repairs/reads the durable WAV, starts a fresh worker instance using the supplied
  new utterance ID, streams bounded frames, and finishes. Emit `AudioEvent::Stopped` with its
  measured file duration before final engine output. It does not start a microphone. Core first
  inserts a new pending row referring to the same `audio_id`, then finishes the interrupted row
  as failed so a crash between writes always leaves a recoverable row; old worker events cannot affect the new row. A failed replay is saved as failed and
  does not loop. Startup feeds `History::recover()` rows to `Recover`; resulting rows are recovered,
  have no target, and never auto-paste. The recorder owns the 250 ms WAV flush guarantee.
- `FocusTarget` refocuses only for explicit interactive Insert and returns `FocusChecked` after
  checking actual focus, including after failure. `Copy` changes the clipboard only. A quick start
  with no target is clipboard mode and copies only after a successful final save.
- Execute `EditHistory` durably and respond `EditSaved`/`EditFailed`. Core serializes edit saves,
  resaves newer edits after an older acknowledgment, and inhibits Insert until the latest edit is
  saved. `SetStatus` calls `SqliteHistory::set_status` (canceled/discarded only).
- `LoadLast` calls `SqliteHistory::last` and feeds `InsertLast` with its optional row. This works
  with a new `Session` after restart; it checks current focus and never sends Enter. For equal start
  timestamps, SQLite insertion order breaks ties. Empty and pending rows cannot cause a paste.
- `Session::hotkey(hotkey, context, monotonic_ms, release_observable)` normalizes observable
  Down/Up into hold or tap, rejects key repeat, and makes press-only inputs toggle. Supply a fresh
  `StartContext` when a press may start recording. Core adds no silence timer or post-release wait.
  Adapters must distinguish repeats from individual presses for press-only inputs.
- `Session::views()` retains active/reviewing entries and one latest terminal snapshot; older quick
  completions are evicted. Open interactive editors remain until `dismiss(id)`; the host may dismiss
  a terminal overlay after its display fade. Effects contain snapshots that remain valid after eviction.
- `SqliteHistory::open(data_directory)` migrates transactionally, uses FULL synchronization and
  owner-only files, supports FTS queries, and preserves raw bytes. Delete commits row/FTS removal
  before unlinking the last audio reference. Startup removes leftover UUID WAVs without references,
  completing an interrupted deletion; pending rows protect active/crashed captures. Open storage
  before starting capture; one application process owns the data directory.
- `Settings::defaults(engine)` takes the engine from the model manifest; `load(path, engine)` fills
  missing fields from those defaults and rejects malformed/unknown top-level settings. `save(path)`
  writes an owner-only temporary file, fsyncs, renames, then fsyncs its directory. Vocabulary and
  per-app send default off. Added modules: `settings`, `storage`. Dependencies: workspace serde_json,
  rusqlite 0.40 bundled, toml 0.9; crossbeam-channel is a test dependency.

### Edge-case evidence

Every SPEC edge-case row is exercised in `crates/core/tests/session_trace.rs` using synthetic recorder,
engine, platform and history fakes. The one-hour trace supplies 14,400 quarter-second frames without
retaining PCM in core; it models one hour rather than waiting an hour. Native recorder durability and
real paste behavior remain audio/platform acceptance tests. `persistence.rs` verifies SQLite reopen,
FTS edits/deletion, audio references, interrupted unlink cleanup, immutable raw bytes and settings.

## Host integration wave

The host adds `HostAction`, `HostState`, `DownloadState`, `ModelInfo` and `HostSnapshot`, exported
through the existing ts-rs pipeline. They carry setup/settings/download/history operations and the
numbered full UI subscription; they do not change worker messages, Engine/Recorder/Platform traits,
UiAction or schema version 1. Tauri command and subscription details are in app/src-tauri/README.md.

`SessionEvent::CaptureFailed { id, message }` distinguishes an audio receiver closing without a
successful durable Stopped from an ordinary end. Core saves the row as failed, cancels recognition,
inhibits delivery and permits later recordings. The host must not fabricate a successful Stopped
for a capture that failed to start or flush. Live audio delivery gaps with a subsequent Stopped
still use worker invalidation and fresh-identity file replay.

Model installation lives in the app's data/models directory on both platforms, as required by the
host build brief. Hash-verified archives are retained there; runtime libraries install by fsync and
rename so validation never truncates a mapped library. RF64 is accepted by the engine's file reader.
Linux control requests add start/stop/history FTS search without changing ownership or peer checks.
