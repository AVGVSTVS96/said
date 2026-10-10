# Linux acceptance receipt

Date: 2026-10-10. Niri 26.04 (8ed0da4), Cage 0.3.1, GTK3, gtk-layer-shell 0.10.1,
Ghostty 1.3.1 and Electron 44.6.0. All text is synthetic.

`tests/build.sh` passed formatting, native source check, five unit tests, clippy with `-D warnings`,
and the CLI/e2e binary build. The temporary manifest compiled the real platform crate entry point
and current core contracts; no copied Platform trait or edited production manifests were used.

`python3 tests/run.py` passed inside Niri nested under headless Cage, with a fresh private runtime
directory, separate Wayland sockets and private D-Bus:

| Check | Result |
| --- | --- |
| GTK3 TextView exact UTF-8 paste | Pass, all 94 bytes |
| Ghostty raw bracketed-paste editor | Pass, all 94 bytes inside paste delimiters |
| Electron textarea exact UTF-8 paste | Pass, all 94 bytes |
| Leading/trailing whitespace, apostrophes, code punctuation, Unicode and newlines | Preserved |
| Enter with `send = false` in all three targets | None |
| Explicit `send = true` in GTK scratch field | Enter observed |
| GTK layer-shell overlay | Target remains focused; keyboard mode none; exclusive zone zero |
| Focus mismatch | Injection rejected |
| Niri focus event stream | Target change observed |
| Actual compositor bind → `voice hotkey` → owner-only socket | Pass |
| Clipboard-only mode on headless Cage, without Niri IPC | Exact bytes; input refused |
| Fake evdev stream | Down/Up, repeat rejection, unrelated-key rejection and disabled dispatch pass |
| Fake keyboard removal/modifier release | Up emitted |
| Fake portal edges | Activated/Deactivated, repeat rejection and reset pass |
| Socket lifecycle | Mode 0600, singleton exclusion, command/reply and removal pass |

The fixed full virtual-keyboard keymap passed where the reference tool's changing minimal map did
not reliably activate GTK paste shortcuts in this test environment. Production insertion therefore
uses the protocol directly. Text still travels only through the clipboard.

The runner retains receipt bytes, key logs and compositor logs in its private session directory and
terminates its own process groups. No uinput device was created, no real evdev device was opened by
these tests, and the live session's clipboard and input were untouched.

Scope: native module acceptance, not the composed Tauri app. The integrator still adds the optional
native dependencies and wires CLI/control replies, focus notifications, settings, Copy and session
lock/deactivation. Actual portal backend operation and physical-device permission/hotplug behavior
are not proven by fake streams. Production paste success means the chord was posted; these scratch
fixtures alone supply readback evidence of exact insertion.
