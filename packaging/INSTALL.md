# Install said

Build the artifacts with [packaging instructions](README.md). Recognition runs locally; the model downloads on first launch and each file is SHA-256 checked. Setup shows the model, permissions and active input mode.

## macOS

1. Copy `said.app` from the DMG to `/Applications` and open that app in Finder. This build is ad-hoc signed and not notarized. For a blocked first open, use **System Settings → Privacy & Security → Open Anyway**, authenticate and choose **Open**.
2. Use Setup's microphone permission action and allow access. In **Privacy & Security → Accessibility**, add `/Applications/said.app` with **+** and enable it. Do the same under **Input Monitoring**, then quit and reopen said.
3. Let the model finish downloading/verifying. Try a scratch dictation in Setup before inserting into another app.

| Action | Default shortcut |
| --- | --- |
| Quick dictation | Option+Space |
| Interactive review | Option+X |
| Insert last | Option+A |

Hold the dictation or interactive shortcut to talk and release to finish. A quick tap starts toggle recording; the next press stops it. Escape cancels during recording. Insert last never presses Enter.

All three shortcuts are editable in Settings: click one and press the new keys. Interactive review has its own shortcut; Shift is not reserved. Rebuilding an ad-hoc bundle can invalidate permission grants, so re-enable the exact installed app after an upgrade. For autostart, add said to **General → Login Items → Open at Login**.

Live microphone, permissions, focus and paste acceptance remains pending in a separate test account. See the [manual checklist](../crates/platform/macos/MANUAL-CHECKLIST.md).

## Linux

Install the Arch package:

```sh
sudo pacman -U said-*.pkg.tar.zst
```

On Niri, review `/usr/share/said/niri.kdl` and `/usr/share/said/autostart.kdl`, then add their includes to the compositor configuration:

```kdl
include "/usr/share/said/niri.kdl"
include "/usr/share/said/autostart.kdl"
```

The startup include runs `/usr/bin/said` directly in a 4 GiB/no-swap scope tied to the graphical session. Use one startup mechanism; the [user service](README.md#arch-and-cachyos) is an alternative.

For a one-time installed-app launch on Niri:

```sh
niri msg action spawn -- said
```

Or start it under the user service:

```sh
systemctl --user start said.service
```

Let the compositor or user service own the app's lifetime. Running `said` as a child of a build terminal can stop it when that terminal exits.

| Action | Default shortcut |
| --- | --- |
| Quick dictation | Super+Z |
| Interactive review | Super+X |
| Insert last | Super+A |

All three settings match the supplied binds. Readable physical keyboards or a functioning GlobalShortcuts portal supply release events for hold-to-talk. The compositor bind alone toggles on each press. `said status` and Setup report the active mode and unavailable reason.

To enable physical keyboard access:

```sh
sudo usermod -aG input "$USER"
```

Log out and back in before starting said. Membership grants access to keyboard-device events. Input still requires an active unlocked logind session. The Niri binds suppress duplicate handling when native edges are available.

All three shortcuts are editable in Settings. After changing them, replace the corresponding Niri binds with the instructions shown in Setup. The saved-transcript hint uses your configured insert-last shortcut.

The portable Linux directory uses `bin/said` and still requires the [system libraries](README.md#arch-and-cachyos). Other Wayland desktops provide clipboard-only delivery; pure X11 is unsupported. Cross-distribution acceptance remains unverified.
