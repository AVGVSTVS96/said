# Core

`Session` is a pure reducer: typed hotkey, audio, engine, focus and UI events produce ordered effects. The host executes effects and returns actual success/failure acknowledgments. Committed text appends unchanged; partial text replaces the provisional tail. Durable pending save precedes capture, and durable final save precedes delivery. Explicit edits remain separate from original text.

`SqliteHistory` owns transactional schema migration, immutable-original triggers, FTS5, delivery metadata, shared-audio deletion and recovery. Settings use TOML with validation and atomic writes. `fake` provides finite scripted adapters for headless ordering/failure tests. Shared types are exported to TypeScript by `export-types`.

See [CONTRACTS.md](../../CONTRACTS.md) for units/ordering and [SPEC.md](../../SPEC.md) for behavior and platform limits.

```sh
CARGO_BUILD_JOBS=2 cargo test --locked -p said-core
CARGO_BUILD_JOBS=2 cargo clippy --locked -p said-core --all-targets -- -D warnings
```
