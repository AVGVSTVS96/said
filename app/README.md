# Desktop app

The [native host](src-tauri/README.md) executes core effects and owns capture, persistence, downloads and platform operations. The [React renderer](src/README.md) receives sequenced snapshots and sends typed actions for the five windows. Shared contracts are exported from Rust by `bun run generate`.

From this directory:

```sh
bun install --frozen-lockfile
bun run typecheck
bun run build
```

`bun run dev` previews the renderer. Add `?demo=1&screen=interactive` for the finite synthetic demo; ordinary browser previews have no native host. `bun run tauri dev` launches the desktop app and can register input listeners or request permissions. Use the [isolated verification workflow](../CONTRIBUTING.md#native-verification) for native checks and [packaging](../packaging/README.md) for installed artifacts.
