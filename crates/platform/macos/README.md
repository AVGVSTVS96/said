# macOS adapter

Enable `voice-platform/native`. No additional crate dependencies are needed.
`MacosPlatform::<tauri::Window>::start(Shortcut::default(), configure_overlay)`
implements the shared `Platform` trait. Start only after the user grants
Accessibility and Input Monitoring: every permission check is passive, and
failure provides the precise Settings path. Microphone checks belong to setup;
the adapter does not request any permission or capture audio.

The host supplies `configure_overlay: fn(&tauri::Window) -> voice_core::Result<()>`.
The adapter calls it on Tauri's main thread. Obtain `window.ns_window()`, convert it to `NonNull<c_void>`,
and call `unsafe { macos::overlay::configure(pointer) }` **before the first show**.
Create the window hidden, undecorated, transparent and unfocusable. Keep Tauri's
native window alive. Show via `overlay::show(pointer)` on the main thread; do not
activate the app or call `set_focus`. The panel remains registered with Tauri and
keeps the webview. Conversion checks the native allocation size, installs
`canBecomeKeyWindow = false` and `canBecomeMainWindow = false`, and applies the
WindowServer activation prevention tag used by tauri-nspanel. An unsupported
conversion returns an error: leave the overlay hidden. Consume `overlay_errors()`
because the shared `make_overlay` signature cannot return errors. Panel creation,
positioning, content, fade and reduced-motion handling remain host/UI effects.

`set_shortcut` replaces the input listener while retaining desktop window identities. The host
uses it for settings changes so an open interactive review can still refocus its original window.
Consume one `hotkeys()` receiver. Feed each event to
`Session::hotkey(event, context, monotonic_ms, true)`: the adapter supplies real
Down/Up, ignores repeats and pairs swallowed key releases, even when modifiers
are released first. Core decides the 300 ms hold/tap boundary. Shift plus the
shortcut emits Interactive on down and Up on release. Default shortcuts are
Control+Option+Space and Control+Option+V; Fn/Globe is not used. Configuration
uses a physical macOS virtual keycode and CONTROL/OPTION/COMMAND flags, with
Shift reserved for interactive mode. Reject collisions with Insert last in setup.

Call `set_recording(true)` while any capture is active, and false when none is.
Escape is intercepted only while recording; it then emits Cancel. Consume
`input_statuses()`: Available maps to `SessionAvailable`; SecureInput,
SessionLocked, PermissionRequired and TapInterrupted map to
`SessionUnavailable { reason }`. Core then stops and saves, including toggle
recordings whose release cannot be seen. Polling reads OS state every 100 ms;
it is required because Secure Input has no public change notification. Revoked
grants never trigger a request, and disabled taps are reenabled after checking
grants. Drop removes the tap and joins its worker.

`focused_target()` returns a detailed error instead of the trait's None. Focus
identity is bundle ID plus an opaque process/window token backed by retained
AX window references and CFEqual, not a window title or just a PID. Tokens are
valid for this adapter's lifetime; recovered recordings never auto-paste. The
host must feed `FocusChanged` for desktop focus changes, including switching
windows in one app, to preserve core's inhibition after an away-and-back change.
`focus()` raises the remembered window, activates its app and verifies focus;
the main loop is serviced until actual focus arrives, with a 500 ms failure
bound and no paste on failure. The host AppKit main loop must be running when
calling desktop methods or configuring the panel from a worker thread.

Paste rechecks window identity before posting, resolves Command+V against the
current command-modified Unicode keyboard layout, and writes an NSString from
the original UTF-8 bytes, including whitespace and embedded NUL. Missing layout
or AX identity fails safely. It posts only the V down/up with explicit Command
flags directly to the remembered PID. No clipboard restoration, text rewrite,
AppleScript, automatic retry or delivery receipt is involved. Success means
command posted. `send = true` explicitly adds Return after 50 ms and another
focus/security check; all other calls emit no Return. Physical modifiers held
by the user are never released by this adapter. Copy-only delivery uses `copy`.

## Automated checks

Run on an idle Apple Silicon Mac, with `CARGO_BUILD_JOBS=2` and `nice -n 10`:

```sh
cargo check -p voice-platform --features native --target aarch64-apple-darwin
cargo test -p voice-platform --features native
cargo clippy -p voice-platform --features native --all-targets -- -D warnings
cargo test -p voice-platform --features native permission_report -- --ignored --nocapture
```

Tests create unposted Quartz events and run the exact production decoder. They
never install a tap, show a window, modify the clipboard, activate another app,
request a grant, or post input. The ignored permission diagnostic reads current
process grants; these may differ from the signed app's grants. Live desktop
acceptance is separate: [manual checklist](MANUAL-CHECKLIST.md).
