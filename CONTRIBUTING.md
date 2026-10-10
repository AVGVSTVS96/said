# Module guide

Read SPEC.md and CONTRACTS.md first. The integrator owns shared contracts and updates every consumer when they change. Module agents implement behind those contracts in separate worktrees; do not edit another module's files. Commit explicit paths, with SSH signing and conventional messages. Fixtures are synthetic or freely licensed; recordings, databases, private prompts and research stay outside this public repository.

Use idiomatic Rust 2024 and React/TypeScript, small modules and direct calls. Comments explain non-obvious platform facts; avoid narrating the code. Add tests for meaningful behavior rather than implementation details.

| Module | Paths and ownership | Local check |
| --- | --- | --- |
| core | `crates/core/src/session.rs`, `history.rs`, `fake.rs`; add persistence, settings, recovery and remaining state transitions | `cargo test -p voice-core` |
| audio | `crates/audio`; capture, bounded buffers, resampling, durable WAV, devices and disconnect handling | `cargo test -p voice-audio` |
| engine | `crates/engine`; adapters, host client, worker dispatch; reuse `protocol` unchanged in app and bench | `cargo test -p voice-engine` |
| platform-macos | `crates/platform/macos`; CGEventTap, focus, clipboard and NSPanel; dependency changes go through integrator | `cargo check -p voice-platform --features native` on macOS |
| platform-linux | `crates/platform/linux`; Niri IPC, control socket, evdev, clipboard, virtual keyboard, layer-shell | `cargo check -p voice-platform --features native` on Linux |
| ui | `app/src` excluding generated `bindings/` and `product.ts`; five screens, core state subscription and actions | `cd app && bun run typecheck && bun run build` |
| bench | `bench`; public corpus, runner, scorer and results; same engine crate as the app | add a pure scorer check before corpus integration |
| app/packaging | `app/src-tauri`, `packaging`; effect executor, worker CLI, IPC, permissions and install artifacts | `cargo check -p voice-app` |
| integrator | `crates/core/src/types.rs`, `bindings.rs`, core build/export scripts, schema, crate manifests, `crates/platform/src`, `product.toml`, app build configuration, CI | checks below |

```sh
git worktree add ../voice-wt/core -b m/core master
```

Use your module name in place of `core`. Both platform agents share one crate manifest: request target-specific dependencies from the integrator rather than editing it concurrently. Common fakes stay available on every OS; native modules are gated by OS and the `native` feature, enabled by the host. Future desktop dependencies must be optional and activated by that feature so pure tests stay headless.

Set `CARGO_BUILD_JOBS=2`. Default workspace members are the four pure crates; this intentionally avoids building Tauri during an adapter test. Scratch data belongs on disk outside the repo; models live in the shared cache, not git. No tests may inject into the live desktop, acquire real hotkeys or open native windows. Linux GUI checks use a nested/headless compositor. macOS permission and input checks belong in a separate test account.

```sh
cargo fmt --all --check
CARGO_BUILD_JOBS=2 cargo clippy -p voice-core -p voice-audio -p voice-engine -p voice-platform --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test -p voice-core -p voice-audio -p voice-engine -p voice-platform
cd app
bun install --frozen-lockfile
CARGO_BUILD_JOBS=2 bun run typecheck
CARGO_BUILD_JOBS=2 bun run build
```

`bun run generate` regenerates product metadata and TypeScript, including after deleting generated files. Rust builds also export the shared types. UI code imports types from `src/contracts.ts`; do not duplicate Rust definitions. The UI sends `UiAction` to core through host IPC and never invokes clipboard or input APIs itself. The IPC executor and subscriptions are the next integration step, not implemented in the empty shell.

CI runs format, clippy and pure tests on Linux and macOS, UI typecheck/build on Linux, and a Linux Tauri host check. Golden audio, nested Niri, recovery and signed macOS evidence are future milestones, not current coverage.
