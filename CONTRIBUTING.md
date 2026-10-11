# Contributing to said

Read [SPEC.md](SPEC.md) for current behavior and limits, and [CONTRACTS.md](CONTRACTS.md) for text, identity and delivery ordering. Keep changes small and direct. Update the affected consumers when a shared Rust type changes; TypeScript types are generated rather than copied.

## Source map

| Path | Responsibility |
| --- | --- |
| `crates/core` | Session reducer, shared types, SQLite history, settings and deterministic fakes |
| `crates/audio` | Capture, bounded buffers, resampling, durable WAV and recovery |
| `crates/engine` | Parakeet adapter, worker protocol, process supervision and model manifest |
| `crates/platform` | Hotkeys, focus, clipboard, paste and native overlay hooks |
| `app/src-tauri` | Host actor, effect execution, IPC, downloads, permissions and CLI |
| `app/src` | React overlay, interactive editor, history, settings and setup |
| `bench` | Public corpus, replay runner, scoring and receipts |
| `packaging` | Arch package, Linux staging, macOS app/DMG and distribution notices |

Use Rust 2024 and React/TypeScript, with small modules and short call paths. Comments explain non-obvious platform facts. Tests should prove a meaningful behavior or failure path. Fixtures must be synthetic or freely licensed; keep personal recordings, transcripts, databases and research outside this public repository. Include provenance for new data and preserve third-party credits for reused code.

## Local checks

Rust 1.90+ and Bun 1.4+ are required. Set `CARGO_BUILD_JOBS=2` to bound parallel compilation. Default workspace members exclude Tauri; pure tests need no display or microphone.

```sh
cargo fmt --all --check
CARGO_BUILD_JOBS=2 cargo clippy --locked -p said-core -p said-audio -p said-engine -p said-platform -p said-bench --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked -p said-core -p said-audio -p said-engine -p said-platform -p said-bench
cd app
bun install --frozen-lockfile
bun run typecheck
bun run build
```

`bun run generate` exports product metadata and shared types. Rust builds also export types. The renderer imports `src/contracts.ts` and sends actions through Tauri IPC; native adapters own clipboard and input operations. `bun run dev` previews the renderer; `/?demo=1&screen=interactive` selects a synthetic development demo. `bun run tauri dev` launches the native app and can request desktop permissions or register input listeners.

A host check needs the [native build dependencies](packaging/README.md):

```sh
CARGO_BUILD_JOBS=2 cargo check --locked -p said-app --features custom-protocol
CARGO_BUILD_JOBS=2 cargo test --locked -p said-app --test end_to_end
bash packaging/tests/notices.sh
```

## Native verification

Run each model test, benchmark or release build in its own bounded scope. On Linux, serialize model-loading runs with a shared lock:

```sh
flock "$HOME/.cache/said-model.lock" systemd-run --user --scope -q \
  -p MemoryMax=4G -p MemorySwapMax=0 -- \
  env CARGO_BUILD_JOBS=2 cargo test -p said-engine --test golden -- --ignored --nocapture
```

The command is a verification procedure, not a claim that the golden suite currently passes. See [benchmark results](bench/RESULTS.md) for recorded failures and reference limits. Use the same cap for full/release builds. Keep scratch data on disk outside git and load one model at a time.

Desktop checks belong in isolated scratch sessions. Linux uses [nested/headless compositors](crates/platform/linux/README.md#acceptance-checks). macOS uses the [separate-account checklist](crates/platform/macos/MANUAL-CHECKLIST.md). Pure and decoder tests do not establish native acceptance. Avoid acquiring live desktop hotkeys, injecting into personal applications or changing permission grants during automated checks.

CI runs format, lint and pure tests on Linux/macOS, plus renderer checks and Linux host check/end-to-end fakes. It does not run the model suite, nested Niri or live macOS acceptance. Keep result claims tied to their command, commit and scope.

## Distribution credits

Update [NOTICE](NOTICE) and the applicable license files when adding a model, font or adapted source. Dependency notice generation and package checks are described in [packaging](packaging/README.md#licenses). Commit explicit paths with conventional messages and retain the repository's configured signing policy.
