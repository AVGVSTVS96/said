# Linux adapter

`LinuxPlatform` implements `Platform` behind the `native` feature and requires a Wayland session. Niri focus/window identity comes from its IPC socket. Clipboard writes use `wl-copy` with the supplied UTF-8 bytes; a Wayland virtual keyboard posts the configured chord. The text is not typed as key presses. GTK layer-shell keeps the overlay unfocused with exclusive zone zero.

Other Wayland sessions use `Mode::Clipboard`: no focused target or automatic input. `copy` writes the clipboard; `paste` rejects so the host cannot mistake a copy for delivery. Pure X11 is unsupported.

## Input and focus

The host builds `Config::from_environment(storage_key)`, parses settings chords and merges app paste overrides. GlobalShortcuts Activated/Deactivated signals or readable evdev keyboards supply real key edges. Niri compositor commands supply press-only toggles when those edges are unavailable. Setup/status expose the input mode and unavailable reason. Joining the input group requires a new login and grants physical keyboard-device access.

Native events flow through one hotkey receiver. Control requests use `control_hotkey` separately, including their release capability; bind commands are suppressed when native edges already handled the press. Explicit commands still work. The three configurable defaults are Super+Z for dictation, Super+X for interactive
review and Super+A for insert-last. Packaged shortcuts are in [installation](../../../packaging/INSTALL.md) and [niri.kdl](../../../packaging/linux/niri.kdl).

Niri's focus-change stream includes target close/disconnect. The host forwards it to core, which inhibits automatic delivery after an observed departure. Explicit focus waits for actual app/window identity. Paste checks identity before copying and again before posting; explicit Enter adds another check. Success means chord posted, not field acceptance. Clipboard text remains available after a failed post check.

logind lock/deactivation signals make the session unavailable before disabling physical input. Missing logind prevents unsafe physical-input activation. Native overlay creation/configuration happens on the GTK main thread before realization; the interactive editor is a separate normal window.

## Control socket

The socket is `$XDG_RUNTIME_DIR/said.sock`, mode 0600 in an owner-only directory. A lock prevents concurrent hosts. Stale owned sockets are replaced while holding the lock; symlinks/non-sockets are refused. Requests/replies are bounded, peers must have the same UID, and the socket is removed when the adapter drops.

System dependencies include GTK 3, gtk-layer-shell, libxkbcommon and wl-clipboard; the host also needs Tauri's WebKit/tray libraries. The crate manifest keeps native dependencies optional so pure tests remain headless.

## Acceptance checks

```sh
CARGO_BUILD_JOBS=2 crates/platform/linux/tests/build.sh
python3 crates/platform/linux/tests/run.py
```

`SAID_LINUX_TEST_DIR` selects a scratch build directory. The runner creates a private runtime and D-Bus, headless Cage and nested Niri. GTK, Ghostty's bracketed-paste scratch editor and an Electron textarea run inside it. Parent-compositor events exercise the actual Niri bind. No real kernel input device is opened or created; unit tests feed finite fake evdev events. Clipboard fallback uses Cage's separate Wayland socket.

Checks cover literal whitespace/Unicode/newlines, no implicit Enter, explicit send, focus mismatch, Niri focus events, a nonfocusing overlay, socket routing and clipboard-only delivery. Synthetic receipts/logs stay in the scratch directory. The runner stops its own processes. It requires `niri`, `cage`, `electron`, `ghostty`, `python-gobject`, `wl-clipboard` and native dependencies.

These checks do not establish a functioning portal backend or physical keyboard permissions. See [recorded results](tests/RESULTS.md) and [product limits](../../../SPEC.md#verification-and-limits). App/model checks additionally need the shared model lock and memory scope in [CONTRIBUTING.md](../../../CONTRIBUTING.md#native-verification).
