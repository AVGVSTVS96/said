# said: behavior and architecture

said is a local dictation app for Linux (Niri) and Apple Silicon macOS. This document describes the implementation, with acceptance limits stated separately. Product identity comes from `product.toml`. Build and install instructions are in [packaging](packaging/README.md).

## Text and delivery

The saved original is the Parakeet adapter's finalized text. Core concatenates committed chunks without adding a separator. No language model, trimming, deduplication or punctuation pass changes that text afterward. The adapter itself decodes and reconciles overlapping acoustic windows; this boundary is not the native decoder's raw bytes and is not a guarantee that speech was heard correctly.

Provisional text can change. Committed chunks append and are not revised within one recognition. Your edits are stored separately from the immutable original. Retry creates a new recognition sharing the same recording.

Pending history is saved before capture starts. Audio streams to a durable WAV while recording. Completed history is saved before automatic paste. A failed save inhibits delivery. Clipboard text remains after insertion. Paste success means the adapter posted a chord; it cannot prove that a destination accepted the text. Automatic Enter is off by default and requires an explicit per-app setting in quick mode. Interactive and insert-last never submit.

## Interaction

Quick dictation and interactive review have separate configurable shortcuts; insert-last has a third. Hold the selected recording shortcut to record and release to finish. A tap shorter than 300 ms starts toggle recording; the next press stops it. Linux compositor-only input has no release edge, so every press toggles. Setup/status describe the selected input mode; the current shortcut and Niri binds are documented in [installation](packaging/INSTALL.md).

Quick dictation remembers the external target, shows live text in a nonfocusing overlay, finishes recognition, saves and checks focus before delivery. Observed departures inhibit automatic paste even after returning. Linux consumes Niri focus events. macOS focus observation and live delivery acceptance are subject to the limits below.

Interactive recording opens a normal editor. Edited regions remain user-owned while untouched text streams. After recording, play the whole clip, inspect Original, edit, copy or insert. Insert refocuses the remembered external window and checks it before pasting. Enter adds a newline; Ctrl+Enter or Cmd+Enter requests insertion after recognition finishes. Discard keeps a discarded history entry.

History provides full-text search, playback, editing, copy, insertion, recognition retry and deletion. Insert last uses the latest stored transcript, including explicit edits. Deletion removes the row and search entry; the WAV is removed after its last referencing row is gone. File-deletion failure can leave an orphan for startup cleanup.

Setup selects a microphone, verifies/downloads the model, configures the shortcut, reports input mode and permissions, and records into a local scratch field without pasting into another application. macOS grants are requested explicitly. Linux hold needs readable physical keyboards or a working GlobalShortcuts portal. Joining `input` requires a fresh login and grants keyboard-device access.

## Failure behavior

| Event | Implementation behavior |
| --- | --- |
| Silence | Capture continues until stop; there is no silence gate or timeout. |
| Empty recognition | Save the recording and empty result; paste nothing. |
| Another utterance is finishing | Capture the next one into its own WAV/spool; one worker lease serializes recognition. |
| Observed focus departure or closed target | Retain text for explicit insertion and inhibit automatic delivery. |
| Explicit microphone disconnect | Drain captured samples and finish; do not select a replacement device. PipeWire's default input may follow a replacement source. |
| Live audio receiver gap | Invalidate live recognition and replay the complete durable WAV. |
| Capture/storage failure without durable stop | Save failed status rather than treating partial capture as a successful result. |
| Worker failure | Preserve capture; a fresh recognition replays the recording. |
| App interrupted | Startup repairs pending WAVs and recognizes them into recovered history without automatic paste. A write or hardware failure can still lose unflushed data. |
| Cancel | Stop capture, retain canceled history and inhibit delivery. Escape depends on native input availability. |
| Lock, revoked grant or Secure Input | Native availability signals stop capture and inhibit delivery; show the reason. |
| Paste rejected or unsupported target | Retain the stored transcript for explicit copy/insertion. Do not automatically retry an uncertain chord. |

Bounded rings and disk spools avoid retaining the whole recording in memory. The audio crate has a one-hour synthetic capture test; it does not certify full-app memory use with a large history or long native recognition. Multi-line text is preserved, but destination handling of newlines and bracketed paste needs per-target testing.

## Architecture

```text
hotkeys / focus -> host -> pure session reducer -> ordered effects
                    |          |
                    |          +-> React snapshots and actions
                    +-> recorder -> durable WAV
                    +-> engine worker -> finalized chunks
                    +-> SQLite save -> focus check -> platform paste
```

Tauri 2 hosts five React windows: overlay, interactive, history, settings and setup. The Rust host owns capture, persistence and platform input. The renderer sends typed actions and receives sequenced snapshots through Tauri commands/channels. Shared Rust types are exported to TypeScript.

Recognition runs in a child process of the same host binary (`--worker`). Native runtime failure is isolated from capture. The host stages the packaged compressed runtime archive before provisioning its model. The recognition path needs no Python and runs locally.

[core](crates/core/README.md) owns state, settings and history. [audio](crates/audio/README.md) owns bounded capture and durable WAVs. [engine](crates/engine/README.md) owns model/protocol/supervision. [platform](crates/platform/README.md) owns native focus/input/clipboard/overlay hooks. [host](app/src-tauri/README.md) executes effects; [renderer](app/src/README.md) displays state. [CONTRACTS.md](CONTRACTS.md) specifies ordering and units.

Both platforms select `parakeet-v3-fp32` in `crates/engine/models.json`. It pins the NVIDIA model's ONNX export, SHA-256 hashes, CPU provider, eight inference threads and sherpa-onnx 1.13.8. Recognition uses 30-second windows with five seconds of overlap and timestamp-based joins. It can make recognition and boundary errors. Finalized adapter text is preserved afterward.

One native worker stays warm between utterances and unloads after 300 idle seconds by default (`SAID_MODEL_IDLE_SECONDS`). Active leases prevent unloading. Concurrent recordings spool separately while waiting for it. Worker framing, identities, acknowledgments and progress deadlines are checked; failures invalidate that recognition rather than silently dropping input.

Linux requires a Wayland session. Niri supplies window identities and focus events; `wl-copy` writes the clipboard and a Wayland virtual keyboard posts the configured chord. GTK layer-shell keeps the overlay unfocused. Other Wayland desktops use clipboard-only delivery and have no automatic input. Pure X11 is unsupported.

macOS uses a filtering Quartz session event tap for shortcuts, Accessibility window identities and focus notifications for targeting, NSPasteboard for text, and Quartz events posted to the HID event stream for the paste chord. A nonactivating NSPanel hosts the overlay. Microphone and Accessibility are required. Input Monitoring is requested only if macOS refuses the shortcut tap with Accessibility alone. Clipboard, Copy and the overlay work before any grant. The host rechecks grants every second, publishes changes to Setup, and starts focus observation and shortcuts as soon as they are allowed, without a relaunch. A focus departure is recorded only when a notification resolves to a different window or app. Secure Input/session availability is polled because the OS supplies no public Secure Input change notification.

## Local data

| Data | Linux | macOS |
| --- | --- | --- |
| Settings | `$XDG_CONFIG_HOME/said/settings.toml` (default `~/.config/said`) | `~/Library/Application Support/said/settings.toml` |
| History and WAVs | `$XDG_DATA_HOME/said/` (default `~/.local/share/said`) | `~/Library/Application Support/said/` |
| App model/runtime cache | `<data>/models/` | `<data>/models/` |
| Worker spool / development cache | `~/.cache/said-models/` | `~/.cache/said-models/` |
| Linux control socket | `$XDG_RUNTIME_DIR/said.sock`, 0600 | Not used |

Personal directories/files are owner-only. The app holds a data-directory lock; Linux CLI peers must have the same UID. History uses SQLite STRICT tables, immutable-original triggers and FTS5. Settings writes use a temporary file, sync and rename. Retention is explicit; there is no automatic age limit. Nothing syncs across machines. Model provisioning uses network downloads; audio is not uploaded.

## Verification and limits

[Benchmark results](bench/RESULTS.md) retain dated receipts, including historical engines and failures. Negative-token, filler/repetition, identifier and WER metrics are lexical counts, not a measured rate of meaning changes. Generated public clips are labelled `authored_unreviewed`; recognizer agreement is not human listening gold. The provisional default has not met every performance/fidelity gate.

Controlled Linux checks include nested-Niri scratch delivery of Parakeet text, save-before-paste and no Enter. They do not certify real evdev access, every desktop, microphone or target. macOS native checks/builds have passed; live microphone, permissions, focus and delivery acceptance remains pending. In particular, successful compilation does not establish that every away-and-back focus transition is observed.

Packaging builds an Arch package and portable Linux directory/tar, plus an arm64 macOS 14+ app/DMG. The Mac app is signed with `SAID_SIGN_IDENTITY`, a Developer ID, or the self-signed identity from `packaging/macos-identity.sh`, falling back to ad-hoc, and is not notarized. Only a stable identity keeps macOS grants across rebuilds; the Linux tar is not a clean-distro-tested AppImage. There is no auto-updater. App, model, font, adapted-code, runtime and dependency terms travel with the artifacts; see [NOTICE](NOTICE).
