# Platform

The `Platform` trait separates hotkey/focus/paste/overlay operations from the pure reducer. `Target` identifies an application and a window. Paste success means the chord was posted, not that a destination accepted it. `FakePlatform` records calls and rejects mismatched focus without touching native input.

The optional `native` feature enables the OS adapter: [Linux/Niri](linux/README.md) uses Wayland, Niri IPC and GTK layer-shell; [macOS](macos/README.md) uses Quartz, Accessibility and a nonactivating NSPanel. Their acceptance procedures and limitations are documented separately. Default pure tests do not acquire desktop hotkeys.

```sh
CARGO_BUILD_JOBS=2 cargo test --locked -p said-platform
CARGO_BUILD_JOBS=2 cargo check --locked -p said-platform --features native
```
