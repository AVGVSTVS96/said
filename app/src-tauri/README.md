# Native host

Setup opens automatically until setup is complete. On macOS, a Dock click with no visible window reopens Setup or, once complete, History. Shortcut and tray actions that fail open Setup and show the reason. Setup and Settings show the
active input capability and all three configurable shortcuts. Portal registration
runs in the background; settings saves retain the control socket and focus adapter.

The `said` host embeds the built renderer with `custom-protocol`. Build `app/dist` before a standalone host build. `--worker` dispatch happens before Tauri initialization and runs the same Parakeet code used by the benchmark.

`runtime::Host` executes core effects on one actor. It opens owner-only storage and a data lock, verifies/downloads the model and native runtime, warms the worker, and recovers pending recordings without automatic paste. Each file's SHA-256 is checked once after download; later launches trust a stamp of its size and modification time, and Verify in Setup rehashes everything. Pending saves precede capture; completed durable saves precede focus checks and delivery. Recorder stop drains final frames before engine finish. Live gaps replay the durable WAV with a new recognition identity.

Linux consumes Niri focus events and logind session signals. Native key edges and compositor commands are routed separately to avoid double toggles. Other Wayland desktops provide clipboard-only delivery. macOS queues application and window focus notifications, checks grants passively, runs AppKit operations on the main thread and configures the nonactivating panel before showing it. Permission requests are explicit actions; microphone usage text lives in `Info.plist`.

## IPC

- `connect({ updates: Channel<HostSnapshot> })` returns the bootstrap snapshot and publishes sequenced complete snapshots. Renderers reject older/duplicate sequences.
- `ui_action({ action: UiAction })` and `host_action({ action: HostAction })` resolve after applying the action and publishing its snapshot. Failed operations reject. Durable history actions must use the stored edited text rather than stale raw session state.
- `said-audio://localhost/<utterance-id>` resolves the WAV only through an existing history row. Range responses support whole-clip playback with bounded reads. The renderer cannot supply an arbitrary file path.

Settings and app rules persist to plain files. Default app rules are per platform; loading drops untouched rules that only the other platform defines. History uses SQLite FTS; retry creates a new row sharing the WAV. Reference-aware deletion keeps recordings used by other rows. Setup scratch recording suppresses delivery, including during replay. Interactive insertion refocuses the remembered external target; History insertion uses the last external target when the app owns focus.

## CLI

Linux control uses the owner-only `$XDG_RUNTIME_DIR/said.sock` with same-UID peers. These commands address the running host:

```sh
said hotkey
said toggle
said start
said stop
said interactive
said insert-last
said cancel
said status
said history search "don't rename"
```

`said hotkey` is the compositor-bind path. Explicit `toggle`, `interactive` and `insert-last` commands remain available. The three editable settings are `hotkey`, `interactive_hotkey` and
`insert_last_hotkey`. Linux defaults are Super+Z, Super+X and Super+A;
macOS defaults are Option+Space, Option+X and Option+A, respectively.
The saved-transcript hint uses the configured insert-last chord. See
[installation](../../packaging/INSTALL.md) for the matching Niri bind file.

The benchmark runs without desktop input:

```sh
said bench bench/clips.jsonl --model-cache "$HOME/.cache/said-models"
```

Use the [shared model lock and memory scope](../../CONTRIBUTING.md#native-verification) for every model-loading run.

## Checks and acceptance

```sh
CARGO_BUILD_JOBS=2 cargo check --locked -p said-app --features custom-protocol
CARGO_BUILD_JOBS=2 cargo clippy --locked -p said-app --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked -p said-app --test end_to_end
```

The default end-to-end proof uses a scripted same-binary worker and disk-backed synthetic recorder. It verifies drained final frames, overlapping capture, reopened history, whole-clip range playback and exact fake delivery. Fake paste independently reads SQLite at delivery time to verify save-before-paste.

The ignored Parakeet comparison runs expected recognition in a separate child, waits for its exit, then runs the host. Its parent never loads ONNX. It compares adapter output, saved text and fake paste for public synthetic clips, including negation, self-repair, repetition and silence. It proves preservation of that adapter output, not acoustic correctness or universal destination acceptance.

```sh
flock "$HOME/.cache/said-model.lock" systemd-run --user --scope -q \
  -p MemoryMax=4G -p MemorySwapMax=0 -- \
  env CARGO_BUILD_JOBS=2 cargo test -p said-app --test end_to_end -- --ignored --nocapture
```

The [Linux harness](../../crates/platform/linux/README.md#acceptance-checks) isolates Niri, runtime, clipboard and scratch targets. `--nested-proof` refuses a runtime/socket outside that harness. It exercises bind, CLI, host, Parakeet, durable history and exact terminal input with no Enter. Physical input/portal availability needs separate verification.

Worker startup/audio/final deadlines are diagnostic limits; benchmark results do not certify them as release budgets. [macOS acceptance](../../crates/platform/macos/MANUAL-CHECKLIST.md) remains pending; passing a compile or native unit test does not establish permission, capture, focus or delivery behavior.

## First-launch regression

The native check requires an existing verified model cache, `cage`, `niri` and
`grim`. It launches in a separate headless compositor and does not send input or
start microphone capture:

```sh
SAID_APP_BINARY=/path/to/built/said \
SAID_MODEL_CACHE=/path/to/verified/models \
SAID_FIRST_RUN_STATE="$HOME/.cache/said-first-run-$RANDOM" \
flock "$HOME/.cache/said-model.lock" \
systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0 -- \
python3 app/src-tauri/tests/first_run.py
```

It checks that a fresh configuration shows Setup without a hotkey and that
completed setup stays hidden. The scratch directory contains snapshots and a screenshot.
