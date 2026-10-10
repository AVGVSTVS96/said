# Host

`custom-protocol` embeds the built frontend for a standalone app; build `app/dist` first.
The `voice` binary dispatches `--worker` before initializing Tauri. The host opens owner-only
storage and a single-process data lock, verifies/downloads the selected model and pinned native
runtime into `data/models`, warms the worker, and recovers pending recordings without pasting.
The model manifest remains the source of platform defaults, hashes, source and license.

`runtime::Host` executes the core's effects. Pending/history saves are durable before capture or
paste is acknowledged. Audio and worker receivers are bounded and continuously drained; a live
audio gap invalidates recognition, while capture failure without a durable stop is saved as failed.
Worker recovery allocates a fresh identity and replays the repaired WAV. Core owns ordering, edits,
hold/tap, inhibition after focus changes and delivery. Platform success means the chord was posted.

Linux subscribes to Niri focus changes and logind lock/activation signals. A lock stops and saves
before physical input is disabled; unlock restores it. Missing logind disables physical input.
Niri binds and native key edges are routed separately so one press cannot toggle twice. Unsupported
Wayland desktops use clipboard delivery. macOS initialization passively checks grants, keeps native
calls on AppKit's main thread, configures the hidden nonactivating panel before showing it, and
positions it at the bottom of the screen with keyboard focus. Permission requests are explicit UI
actions. Microphone usage text is in `Info.plist`.

## IPC

- `connect({ updates: Channel<HostSnapshot> })` installs the subscription and returns its initial
  full snapshot. Every update has a monotonically increasing `sequence`; renderers discard older
  snapshots and duplicates. Full snapshots make a newer state sufficient even when numbers skip.
- `ui_action({ action: UiAction })` and `host_action({ action: HostAction })` run on one host actor.
  Their promises resolve with the applied snapshot after publication; failures reject. Stored
  history IDs route through SQLite when they are no longer active core sessions.
- `voice-audio://localhost/<utterance-id>` resolves only an existing history row's audio reference.
  Bounded range responses support native audio controls without loading an entire recording.
  Filesystem paths are not accepted from the renderer. Moonshine supplies no word timestamps;
  playback is the whole clip.

Settings, microphone, vocabulary and app-rule saves persist to the existing plain files. History
search uses SQLite FTS, retry creates a fresh row sharing the WAV, and delete/retention use core's
reference-aware deletion. Setup scratch recordings have no target and suppress clipboard effects,
including worker replay. History Insert returns to the last external target when the app owns focus.
Interactive windows activate once per recording; closing them releases completed core views.
Notifications and local/vocabulary/auto-send indicators come from host state.

## CLI

The running Linux app owns `$XDG_RUNTIME_DIR/<storage_key>.sock` (0600, same-UID peers).

```sh
voice hotkey
voice toggle
voice start
voice stop
voice interactive
voice insert-last
voice cancel
voice status
voice history search 'no AND file'
voice bench bench/clips.jsonl --model-cache "$HOME/.cache/voice-models"
voice import-vocabulary codex [dictionary.json]
voice import-vocabulary wispr /path/to/flow.sqlite
```

macOS Wispr import defaults to its Application Support database. It reads active `Dictionary.phrase`
rows, excludes deleted entries/snippets, and never imports replacements or history. Codex import
accepts `dictationDictionary` at the JSON root or inside `electron-saved-atom-state`, as strings or
arrays of terms. Missing/unknown dictionaries fail explicitly. Import adds terms to vocabulary.txt;
it does not enable recognition bias. The Codex default path is `~/.codex/.codex-global-state.json`.

## Proof

Build with `CARGO_BUILD_JOBS=2`. No headless checks acquire a microphone or a physical keyboard.
Synthetic clips and large model/runtime files stay outside git.

```sh
CARGO_BUILD_JOBS=2 cargo build -p voice-app --features custom-protocol --bin voice
CARGO_BUILD_JOBS=2 cargo clippy -p voice-app --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test -p voice-app --test end_to_end
CARGO_BUILD_JOBS=2 cargo test -p voice-app --test end_to_end -- --ignored --nocapture
```

The default proof uses a scripted same-binary worker and a disk-backed synthetic recorder. It
queues final frames during stop, immediately starts another utterance, verifies both complete WAVs
and ordered exact-byte delivery. Stop drains the old receiver before a new capture replaces it.

The ignored proof needs the engine's hash-verified cache and public synthetic `engine-proof` WAVs.
It compares a direct Moonshine adapter at the recorder's 20 ms cadence with the same-binary worker,
durable reopened history, bounded playback ranges and the exact bytes received by fake paste. Fake paste independently
opens SQLite read-only at the instant of delivery and rejects text without an exact durable save.
Negation, self-repair, repeated words across pauses and silence are exercised. This proves byte
preservation, not acoustic correctness or an engine ranking.

Use the platform's headless Cage/nested Niri runner for the native terminal flow:

```sh
VOICE_APP_BINARY="$PWD/target/debug/voice" \
VOICE_APP_E2E="[\"$PWD/target/debug/voice\",\"--nested-proof\",\"$HOME/.cache/voice-models/engine-proof/negation.wav\",\"--models\",\"$HOME/.cache/voice-models\"]" \
VOICE_LINUX_TEST_DIR="$HOME/.cache/voice-niri-app" \
python3 crates/platform/linux/tests/run.py
```

That entry point refuses a runtime/socket outside the fresh nested harness. It disables physical
input and portals, exercises the actual Niri bind → app CLI → host → Moonshine → durable history →
Ghostty scratch editor, checks exact terminal bytes and no stray Enter, and searches persistent
history over the control socket. Keep the test runtime path short enough for Unix sockets.

The worker's 60/10/30-second startup/audio/final deadlines are diagnostic limits inherited from the
engine probe. `bench/RESULTS.md` reports failed full-suite/quality gates and supplies no release watchdog recommendation.
Release defaults and budgets remain uncertified. macOS
verification is check/build only; the platform's signed test-account manual checklist remains
separate and is not marked passed by a successful compilation.
