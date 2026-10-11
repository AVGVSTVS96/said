# Linux adapter

`LinuxPlatform` implements the shared `Platform` trait. Niri focus and window identity come from
its Unix IPC socket. Clipboard writes use `wl-copy` with the original UTF-8 bytes; insertion uses
`zwp_virtual_keyboard_v1` directly, with a complete fixed XKB map. The text is never typed as key
presses. There is no clipboard restoration, uinput device, Enter by default, or uncertain-paste retry.
GTK layer-shell configures a hidden overlay with exclusive zone zero and no keyboard interactivity.

Other Wayland sessions have `Mode::Clipboard`, no focused target, and no automatic input.
Run core's `Copy` effect through `LinuxPlatform::copy`. `paste` returns an error in this mode so a
caller cannot mistake copying for a successfully posted chord.

## Integrator wiring

The shared platform manifest and app binary are integrator-owned. This branch changes neither.
The executable CLI fixture in `tests/said.rs` demonstrates routing `said hotkey`, `toggle`,
`interactive`, `insert-last`, `cancel` and `status` through `control::run_cli`. Use the stable storage
key from `product.toml`; the host and CLI must agree on it.

1. Construct `Config::from_environment(storage_key)`. Set `chord` with `Chord::parse(settings.hotkey)`,
   set `read_input` from the initial unlocked/active session state, set `portal_trigger` to the
   corresponding XDG accelerator, and merge app overrides into
   `paste_chords`. A terminal pane and composer sharing an app ID require a user override.
2. Construct `LinuxPlatform`. The GlobalShortcuts portal is attempted first on Niri. Successful
   registration supplies Activated/Deactivated events; otherwise readable physical evdev keyboards
   provide real Down/Up. Without either, the Niri bind supplies press-only toggles. Expose
   `input_unavailable_reason`, `portal_unavailable_reason` and `mode` in setup. No permission is granted
   by the adapter. Input access requires joining the input group and a fresh login.
3. Feed `hotkeys()` to `Session::hotkey` with `release_observable = true`. These are physical or portal
   edges. Consume `control_requests()` separately: `control_hotkey(command)` returns an optional
   `(Hotkey, release_observable)` for core. The bind commands are suppressed when real edges are
   available; explicit `toggle`/`interactive` remain press-only. Answer every request's `reply` with
   the actual host result. For `status`, return host state and the adapter mode.
4. Keep `niri().focus_changes()` alive and feed every event into `SessionEvent::FocusChanged`,
   including `None` on target close or IPC disconnect. `focus()` waits for actual focus and verifies
   app ID and window ID. `paste()` checks identity before copying, again before posting the chord,
   and again before an explicitly requested Enter after 50 ms. A return of success means posted,
   not accepted by the field. Clipboard text remains available when the second focus check fails.
5. On logind session lock/deactivation, feed `SessionUnavailable` before `set_input_enabled(false)`;
   on unlock, re-enable input and feed `SessionAvailable`. The host owns this lifecycle signal.
   Niri focused-window IPC alone is not a lock detector. Do not enable evdev without this wiring.
6. Create the overlay hidden and without focus. Configure it before GTK realization, then show it
   on the main thread. `make_overlay` dispatches GTK work to that thread and selects the focused
   output's logical monitor. `overlay::configure` also provides a fallible GTK entry point for
   native creation. Interactive editing uses a separate normal window.

The same niri binds work in toggle, evdev and portal modes:

```kdl
binds {
    Ctrl+Alt+Space repeat=false allow-inhibiting=false {
        spawn "/absolute/path/to/said" "hotkey";
    }
    Ctrl+Alt+Shift+Space repeat=false allow-inhibiting=false {
        spawn "/absolute/path/to/said" "hotkey" "interactive";
    }
    Ctrl+Alt+V repeat=false allow-inhibiting=false {
        spawn "/absolute/path/to/said" "hotkey" "insert-last";
    }
}
```

The socket is `$XDG_RUNTIME_DIR/<storage_key>.sock`, mode 0600. The directory must be owner-only.
An advisory lock prevents two hosts; stale owned sockets are replaced only while holding that lock.
Symlinks and non-socket paths are refused. Requests and replies have bounds and timeouts; peer UID
must match. The socket is removed when the host drops the adapter.

## Native manifest additions requested

Make these dependencies optional, under the Linux target, and activate them from `native`:

```toml
serde_json = { workspace = true, optional = true }
libc = { version = "0.2", optional = true }
evdev = { version = "0.13", optional = true }
gtk = { version = "0.18", optional = true }
gtk-layer-shell = { version = "0.8", features = ["v0_6"], optional = true }
tauri = { version = "2", default-features = false, features = ["wry"], optional = true }
ashpd = { version = "0.12.3", optional = true }
futures-util = { version = "0.3", optional = true }
tokio = { version = "1", features = ["rt", "sync", "time", "macros"], optional = true }
wayland-client = { version = "0.31", optional = true }
wayland-protocols-misc = { version = "0.3", features = ["client"], optional = true }
xkbcommon = { version = "0.9", default-features = false, optional = true }
```

`ashpd` 0.12 meets the workspace's Rust 1.90 minimum; 0.13 requires Rust 1.92. System dependencies
include GTK3, gtk-layer-shell, libxkbcommon and wl-clipboard. The isolated harness builds the actual
`crates/platform/src/lib.rs` and Linux modules against the current core, without editing the manifest.
It is a temporary integration check, not an alternative production package.

## Acceptance checks

```sh
CARGO_BUILD_JOBS=2 crates/platform/linux/tests/build.sh
python3 crates/platform/linux/tests/run.py
```

Set `SAID_LINUX_TEST_DIR` to reuse a build directory on disk. The runner creates a fresh 0700
runtime directory and private D-Bus, headless Cage, and Niri nested inside Cage. GTK3, Ghostty with a
raw bracketed-paste scratch editor, and Electron's textarea run exclusively inside that Niri.
The parent compositor receives physical-key-shaped Wayland events to exercise the real Niri bind;
Niri's own virtual keyboard path bypasses compositor bindings. No kernel input device is created.
The production evdev dispatcher receives a finite fake event stream in unit tests; no real device
is opened by tests. Clipboard fallback runs on Cage's separate Wayland socket.

The runner verifies exact leading/trailing spaces, Unicode, punctuation and newlines; no Enter in
any target; explicit send; focus mismatch; the Niri event stream; nonfocusing overlay; socket bind
routing; and exact clipboard mode. It retains synthetic receipt files and logs in its scratch
session directory and stops only its own compositor and target process groups. Tests need `niri`,
`cage`, `electron`, `ghostty`, `python-gobject`, `wl-clipboard` and the native system dependencies.
Portal signal mapping is tested with fake edges; a functioning portal backend is not established
by these headless tests. A real physical keyboard and input-group membership are deliberately not
part of automated verification.
