# said

Local dictation for macOS and Linux that never edits what you said, ums and all: hold a hotkey, speak, and the text lands in the focused app. A Rust workspace (session reducer, audio, the Parakeet engine, desktop adapters) behind a Tauri app with a React renderer. `SPEC.md` is current behavior, `CONTRACTS.md` the text, identity and delivery ordering.

## Commands

- `bun run check`: builds the renderer (with generated types), oxlint, oxfmt, renderer tests, then cargo fmt, clippy `-D warnings` and tests over the whole workspace. CI on Linux and macOS and the pre-commit hook run it.
- `bun run fix`: oxlint's fixes, oxfmt, cargo fmt.
- `bun --filter said-ui dev`, then `/?demo=1&screen=interactive`: the renderer with a synthetic stream.
- `bun --filter said-ui tauri dev`: the native app (registers hotkeys, asks for permissions).
- Model tests and the benchmark load a real model: run them under the lock and memory cap in `CONTRIBUTING.md#native-verification`.

## Map

| Path              | Job                                                                | May use                                             |
| ----------------- | ------------------------------------------------------------------ | --------------------------------------------------- |
| `crates/core`     | session reducer, shared types, SQLite history, settings, fakes     | nothing in the workspace                            |
| `crates/audio`    | capture (cpal), ring buffer, resampling, durable WAV               | core                                                |
| `crates/engine`   | sherpa-onnx Parakeet worker, protocol, supervision, model manifest | core                                                |
| `crates/platform` | hotkeys, focus, clipboard, paste, overlay; `macos/` and `linux/`   | core                                                |
| `app/src-tauri`   | host actor, effects, IPC, downloads, CLI                           | every crate                                         |
| `app/src`         | React overlay, editor, history, settings, setup                    | the host over Tauri IPC, types from `src/bindings/` |
| `bench`           | public corpus, replay runner, scoring, receipts                    | core, engine                                        |
| `packaging`       | Arch package, macOS app and DMG, notices                           |                                                     |

## Rules and what enforces them

| Rule                                                                                               | Enforced by                                                                                 |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `unsafe` only in FFI modules (`platform/macos`, `platform/linux`, the engine's sherpa-onnx loader) | `unsafe_code = "deny"` in `[workspace.lints]`; each exception is an `allow` with its reason |
| A crate uses only what its `Cargo.toml` lists                                                      | Cargo                                                                                       |
| Renderer types come from Rust                                                                      | `ts-rs` generates `app/src/bindings/`; `contracts.ts` re-exports them                       |
| No `useEffect` outside `app/src/outside.ts`, no comments, `as` or `any` in TypeScript              | `oxlint.config.ts`                                                                          |
| Clippy pedantic, no warnings                                                                       | `[workspace.lints]` and `-D warnings` in `check`                                            |

## Judgment

- The repo is public: fixtures are synthetic or freely licensed, and personal recordings, transcripts and databases stay out. New data carries its provenance; reused code keeps its credit in `NOTICE`.
- Desktop checks run in scratch sessions (`crates/platform/linux/README.md`, `crates/platform/macos/MANUAL-CHECKLIST.md`), never against live hotkeys or personal apps.
- Keep result claims tied to their command, commit and scope (`bench/RESULTS.md`).
