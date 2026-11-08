# Contributing to said

Read [SPEC.md](SPEC.md) for current behavior and limits, and [CONTRACTS.md](CONTRACTS.md) for text, identity and delivery ordering. Keep changes small and direct. Update the affected consumers when a shared Rust type changes; TypeScript types are generated rather than copied.

## Source map

| Path              | Responsibility                                                                  |
| ----------------- | ------------------------------------------------------------------------------- |
| `crates/core`     | Session reducer, shared types, SQLite history, settings and deterministic fakes |
| `crates/audio`    | Capture, bounded buffers, resampling, durable WAV and recovery                  |
| `crates/engine`   | Parakeet adapter, worker protocol, process supervision and model manifest       |
| `crates/platform` | Hotkeys, focus, clipboard, paste and native overlay hooks                       |
| `app/src-tauri`   | Host actor, effect execution, IPC, downloads, permissions and CLI               |
| `app/src`         | React overlay, interactive editor, history, settings and setup                  |
| `bench`           | Public corpus, replay runner, scoring and receipts                              |
| `packaging`       | Arch package, Linux staging, macOS app/DMG and distribution notices             |

Use Rust 2024 and React/TypeScript, with small modules and short call paths. In Rust, comments explain non-obvious platform facts; TypeScript carries none (oxlint enforces it). Tests should prove a meaningful behavior or failure path. Fixtures must be synthetic or freely licensed; keep personal recordings, transcripts, databases and research outside this public repository. Include provenance for new data and preserve third-party credits for reused code.

## Local checks

Rust 1.90+ and Bun 1.4+ are required, plus the [native build dependencies](packaging/README.md) on Linux. `.cargo/config.toml` bounds compilation to two jobs. Checks need no display or microphone.

```sh
bun install --frozen-lockfile
bun run check
```

`check` builds the renderer, runs oxlint, oxfmt and the renderer tests, then cargo fmt, clippy with `-D warnings` and tests over the whole workspace. `bun run fix` applies the formatters and lint fixes. CI runs `check` on Linux and macOS, and lefthook runs it before each commit.

`bun --filter said-ui generate` exports product metadata and shared types. Rust builds also export types. The renderer imports `src/contracts.ts` and sends actions through Tauri IPC; native adapters own clipboard and input operations. `bun --filter said-ui dev` previews the renderer; `/?demo=1&screen=interactive` selects a synthetic development demo. `bun --filter said-ui tauri dev` launches the native app and can request desktop permissions or register input listeners.

Packaging checks its notices with `bash packaging/tests/notices.sh`.

## Native verification

Run each model test, benchmark or release build in its own bounded scope. On Linux, serialize model-loading runs with a shared lock:

```sh
flock "$HOME/.cache/said-model.lock" systemd-run --user --scope -q \
  -p MemoryMax=4G -p MemorySwapMax=0 -- \
  env CARGO_BUILD_JOBS=2 cargo test -p said-engine --test golden -- --ignored --nocapture
```

The command is a verification procedure, not a claim that the golden suite currently passes. See [benchmark results](bench/RESULTS.md) for recorded failures and reference limits. Use the same cap for full/release builds. Keep scratch data on disk outside git and load one model at a time.

Desktop checks belong in isolated scratch sessions. Linux uses [nested/headless compositors](crates/platform/linux/README.md#acceptance-checks). macOS uses the [separate-account checklist](crates/platform/macos/MANUAL-CHECKLIST.md). Pure and decoder tests do not establish native acceptance. Avoid acquiring live desktop hotkeys, injecting into personal applications or changing permission grants during automated checks.

CI does not run the model suite, nested Niri or live macOS acceptance. Keep result claims tied to their command, commit and scope.

## Distribution credits

Update [NOTICE](NOTICE) and the applicable license files when adding a model, font or adapted source. Dependency notice generation and package checks are described in [packaging](packaging/README.md#licenses). Commit explicit paths with conventional messages and retain the repository's configured signing policy.
