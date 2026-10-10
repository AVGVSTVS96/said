# voice

A local dictation app being built for macOS and Linux, with Niri as the first Linux target. The name is a placeholder. The intended behavior is to preserve the recognizer's words byte for byte, including fillers, repeats and false starts. No LLM cleanup.

**Milestone 0 is a scaffold, not a working dictation app.** Shared Rust/TypeScript contracts, test adapters, a worker codec, a session reducer seed and five Tauri window declarations are present. Microphone capture, recognition, desktop integration, persistence and the real screens are not connected. No accuracy or delivery claims have been measured.

Read [SPEC.md](SPEC.md) for product behavior, [CONTRIBUTING.md](CONTRIBUTING.md) for module ownership and checks, and [CONTRACTS.md](CONTRACTS.md) before implementing an adapter.

## Development

Rust 1.90+ and Bun 1.4+. Linux host builds also need GTK 3, WebKitGTK 4.1 and the Tauri tray dependencies; see [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/). Pure crate tests need no display, microphone or desktop libraries.

```sh
CARGO_BUILD_JOBS=2 cargo test
cd app
bun install --frozen-lockfile
CARGO_BUILD_JOBS=2 bun run typecheck
CARGO_BUILD_JOBS=2 bun run build
```

`bun run dev` previews the UI shell. `bun run tauri dev` launches the native shell on your desktop; the hidden overlay is not yet a native panel or layer-shell surface. From the repository root, `CARGO_BUILD_JOBS=2 cargo check -p voice-app` checks the host without launching it.

`product.toml` owns the display name, bundle identifier and storage key. Bundle identifier and storage key remain stable across renames. Cargo package names are internal module identifiers. Generated TypeScript and Tauri configuration are rebuilt locally and ignored by git.

MIT for this code. Models and third-party runtimes will have their own licenses, recorded in their manifests before integration.
