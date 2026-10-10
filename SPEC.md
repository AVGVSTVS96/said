# Spec

Status: design to build, 2026-10-10. The name "voice" is a placeholder and lives in one file, `product.toml`. Branding comes after the product works.

## What this is

A dictation app for macOS and Linux that types exactly what you said. Hold a key, talk, let go, and your words land in the focused app. Fillers, repeats, false starts and "actually no, the other one" all stay. Nothing is rewritten, ever. Recognition runs locally. Every recording and its text are saved before anything is pasted, so an utterance is never lost. It is built for talking to coding agents: Claude Code, Codex, T3 Code, terminals, Electron apps and browsers.

## The promise

1. **Verbatim.** The text we paste is the recognizer's output, byte for byte. We never trim, dedupe, punctuate, capitalize, reformat or "fix" it. No LLM touches it.
2. **Final words never change.** Words shown as provisional may change while you talk. Words shown as final never do.
3. **Nothing is lost.** Audio streams to disk while you speak. Text is saved before it is pasted. History keeps everything until you delete it.
4. **No stray Enter.** We paste; we never submit. The only exception is a per-app setting you turn on yourself.
5. **Honest about the recognizer.** Any engine can mishear. We pick engines with a benchmark that scores negations, fillers and identifiers, and we publish the numbers. We do not claim zero errors.
6. **Extras are opt-in and visible.** Vocabulary bias and per-app auto-send are off by default and shown when on.

## How it works

### Quick dictation, the default

```
hotkey down        hotkey up
    |                  |
    v                  v
  record  --live words in overlay-->  finish  -->  save  -->  paste into the app
                                                    |              that had focus
                                              history.sqlite
                                              audio/<id>.wav
```

1. **Hotkey down.** Remember which app has focus. Start recording. The overlay appears with a level meter and shows "Listening" as soon as the mic delivers its first frame.
2. **While you talk.** Audio streams to the engine and to disk. The overlay shows the last few lines of text. Final words are normal; provisional words are dimmed and underlined. Pauses do nothing; there is no silence timeout. Saying "stop" or "send" just adds those words.
3. **Hotkey up.** Stop recording, flush the engine, wait for its final text. The overlay shows "Finishing". No extra wait is added after release.
4. **Save.** Write the final text to history. This happens before any paste.
5. **Paste.** Check that the same app still has focus. Put the text on the clipboard and send that app's paste chord. The text stays on the clipboard. The overlay shows "Pasted" for a second and fades. If focus moved, the overlay shows "Saved. Press Ctrl+Alt+V to insert" and nothing is pasted.

Hold and tap work on the same key with no mode setting. Hold the key to talk and let go to finish. A tap shorter than 300 ms starts a toggle recording; the next tap stops it. Where key release cannot be observed (see Linux below), every press is a toggle.

The overlay never takes keyboard focus. You can keep typing in the target app while recognition finishes.

### Interactive mode

Same hotkey plus Shift, or the "Interactive" item in the tray menu.

```
hotkey down            hotkey up                    Insert
    |                      |                           |
    v                      v                           v
  record --text streams into a window-->  edit, listen  -->  refocus app, paste
```

- The window is a normal editor. Text streams in while you talk. Provisional words are dimmed; final words are normal. You can edit at any time. Once you edit a region, late recognizer updates never touch it.
- After release you can fix words, play the audio (click a word to play from there when the engine gives word times, otherwise play the whole clip) and see the raw recognizer text under "Original".
- **Insert** refocuses the app that had focus when you started, verifies it got focus, and pastes. **Copy** copies. **Discard** keeps the utterance in history marked discarded. Enter is a newline; Ctrl+Enter (Cmd+Enter) is Insert.
- Both the raw text and your edited text are saved. The raw text is never overwritten.

### Insert last, history

- **Ctrl+Alt+V** pastes the most recent utterance into the current app. It never submits. Use it after a focus slip or to paste the same words somewhere else.
- **History** (tray menu) lists utterances newest first with full-text search. Each has play, copy, insert, edit, retry recognition, and delete. Delete removes the audio, the text and the search entry together.

### Setup, first run

One screen: pick a microphone (with a live meter), download the default model (progress, size, license), set the hotkey, grant permissions (macOS: Microphone and Accessibility), and try it in a scratch text field on the same screen. On Niri, setup shows the one key bind to add and, for hold-to-talk, the one command that grants input access. Nothing is typed into any other app during setup.

### What the overlay looks like

A small panel, about 480 px wide, bottom center of the monitor with the focused window. One status row (state, target app name) above up to three lines of 16 px text in the system font. A level meter, not a decorative waveform. Theme-aware. Provisional text is dimmed and underlined, never color alone. State changes get a short fade; reduced-motion turns it off.

### Edge cases

| Event | Behavior |
|---|---|
| Silence while recording | Keep recording. No timeout. |
| Empty result | Save the audio. Paste nothing. Overlay: "No speech recognized". |
| Very long dictation | No cap. Audio goes to disk as it arrives; RAM stays flat. Tested with a one-hour recording. |
| Hotkey pressed while the previous utterance is still finishing | Start the new recording at once. Utterances finish and paste in order, each checked against the app that had focus when it started. |
| Focus moved during recording or finishing | Do not paste. Save, show the "Insert last" hint. |
| Target app closed | Same as focus moved. |
| Mic unplugged | An explicitly selected non-default mic stops at the last frame, finishes normally with what was captured, and shows "Mic disconnected". On Linux, the default input follows PipeWire and may continue from a new source after unplug. |
| Engine crash or hang | Keep recording to disk. On release, restart the worker and recognize from the file. Overlay: "Recognizer restarted". |
| App crash | Audio on disk is complete up to the last flush (every 250 ms). On next start, recordings without text are recognized and shown in history as "Recovered". They are never auto-pasted. |
| Cancel | Click the ✕ on the overlay, or press Escape where the hotkey adapter can see it (macOS, Linux with evdev). The utterance stays in history marked canceled. |
| Clipboard | Dictated text stays on the clipboard. We do not restore the previous clipboard; a timed restore races the paste. |
| Multi-line text | Pasted as is. Agent TUIs use bracketed paste, so newlines do not submit. The test matrix verifies this per target. Engines in v1 do not emit newlines; the app never adds one. |
| Secure input (macOS) or locked session | Hotkey unavailable; the tray icon says why. Recording in progress stops and saves. |

## Architecture

### Processes

```
          +---------------------- app (one binary) -----------------------+
          |                                                               |
hotkey -->|  platform adapter     core session      audio recorder        |
focus  <--|  (mac / linux)   <--> state machine <-- cpal, 16 kHz mono     |
paste  <--|                           |      \-- audio/<id>.wav           |
          |                           |                                   |
          |                     Tauri windows: overlay, interactive,      |
          |                     history, settings, setup (React)          |
          +---------------------------|-----------------------------------+
                                      | pipes: length-prefixed JSON + PCM
                              +-------v--------+
                              | engine worker  |  same binary, --worker
                              | one model      |
                              +----------------+
```

Tauri 2, Rust, React/TypeScript. One binary. The engine runs in a child process of the same binary so a native crash in a model runtime cannot take down the recorder, and so the benchmark can run the exact same code. The host restarts it on crash. No daemon, no Python, no HTTP server.

Repo:

```
product.toml        name and bundle id; everything derives from it
crates/core         session state machine, history (SQLite), settings, shared types
crates/audio        capture, resampling, wav writer
crates/engine       Engine trait, worker protocol, adapters (moonshine, scribe, fake)
crates/platform     hotkey, focus, paste, overlay hooks: macos/, linux/
app/src-tauri       Tauri host, CLI, composition
app/src             React UI
bench/              corpus manifest, clips, runner, scorer
packaging/          dmg, AppImage, PKGBUILD, autostart, niri snippet
```

Shared types live in `crates/core` and are exported to TypeScript at build time. The UI never pastes; it sends actions to core.

### Session state

```
idle --hotkey down--> recording --hotkey up--> finishing --final--> saved --focus ok--> pasted
                          |                                           |
                          +--interactive--> reviewing --Insert--------+--focus moved--> saved (not pasted)
```

Core is a pure state machine: it takes events (hotkey, audio frame, engine event, focus change, UI action) and returns effects (start capture, feed engine, write file, paste, show overlay state). Adapters run the effects. Every test of the edge-case table is a trace through this machine with fake adapters.

Utterance IDs are random. A late event from a canceled or previous utterance is dropped by ID.

### Audio

`cpal` capture on both platforms. The callback copies frames into a bounded ring and does nothing else: no allocation, no I/O. A worker thread drains the ring, resamples to 16 kHz mono i16, appends to `audio/<id>.wav`, flushes every 250 ms, and feeds the engine. The level meter comes from the same thread. On stop, drain the ring, flush, then tell the engine to finish. The wav header is repaired on next start if the app died mid-write.

### Engine interface

```rust
trait Engine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()>;
    fn feed(&mut self, pcm16k: &[i16]);
    fn finish(&mut self);          // flush; a Final event follows
    fn cancel(&mut self);
}

enum EngineEvent {
    Partial(String),               // the provisional tail, replaces the previous Partial
    Committed(String),             // a final chunk, appended; never changes
    Final,                         // all audio consumed, no more events
    Error(String),
}
```

The transcript is the committed chunks joined with nothing, plus the current partial. Adapters include their own spaces at chunk edges. The host adds no characters. Repeated words are real speech; adapters must not dedupe across chunks (use sample times, not string overlap). An engine without partials just emits Committed chunks at finish; the overlay shows the meter until then.

Vocabulary is passed to engines that support biasing (hotwords, prompt). Engines that do not support it ignore it, and settings say so.

The worker speaks this over stdin/stdout: a 4-byte length, a JSON message, and for audio a second length-prefixed raw PCM block. Messages are `Start`, `Audio`, `Finish`, `Cancel` in, and the four events out, each tagged with the utterance ID. The host kills and restarts a worker that stops acknowledging audio for a benchmark-derived interval.

Adapters in v1:

- **Moonshine v2 streaming** (Small on Linux CPU, Medium on Apple Silicon). Provisional default; the bake-off (`bench/RESULTS.md`) sets the real default per platform in the model manifest, one line.
- **Fake**: replays a scripted event trace for tests.
- **ElevenLabs Scribe v2 Realtime**, verbatim mode, BYOK. Same interface. Added after the local path is solid. The key goes in the OS keychain.

Changing the default engine means editing the manifest, not the app.

### Platform adapters

```rust
trait Platform {
    fn hotkeys(&self) -> Receiver<Hotkey>;           // Down, Up, Interactive, InsertLast, Cancel
    fn focused(&self) -> Option<Target>;              // app id + window id + name
    fn focus(&self, t: &Target) -> Result<()>;        // bring it back (interactive mode)
    fn paste(&self, t: &Target, text: &str, send: bool) -> Result<()>;
    fn make_overlay(&self, w: &tauri::Window);        // non-focusing, always on top
}
```

`paste` sets the clipboard, sends the target's paste chord, and if `send` sends Enter after a short gap. The chord comes from the app table in settings: terminals get Ctrl+Shift+V on Linux and Cmd+V on macOS, everything else Ctrl+V or Cmd+V. The table ships with the common terminals and agents filled in and is editable.

We do not verify that the app accepted the paste. There is no universal way to. The text is saved and on the clipboard, so the worst case is pasting it again by hand.

**macOS.** CGEventTap for key down and up (gives real hold, Escape to cancel, and detects Secure Input). `NSWorkspace` frontmost app for focus; `NSRunningApplication.activate` to refocus. `NSPasteboard` plus a Cmd+V `CGEvent`. Overlay is a non-activating `NSPanel` (tauri-nspanel). Permissions: Microphone and Accessibility; Input Monitoring if the OS asks. Default hotkey Ctrl+Option+Space. Fn/Globe is not used; many external keyboards handle it in firmware.

**Linux, Niri first.**

| Need | How |
|---|---|
| Hotkey | One Niri bind runs `voice hotkey` over the control socket. Without input access that is a toggle. With the user in the `input` group the app reads key down and up for the chord from evdev and real hold works; the bind then only keeps the chord from reaching the focused app. |
| Focus | Niri IPC: `focused-window` and the event stream. Gives window id and app_id. `focus-window --id` to refocus. |
| Paste | Wayland data-control for the clipboard, `zwp_virtual_keyboard_v1` for the chord (wtype is the reference). No uinput, no root. |
| Overlay | GTK layer-shell, exclusive zone 0, keyboard interactivity none. |
| Control | Unix socket in `$XDG_RUNTIME_DIR`, mode 0600. The `voice` CLI talks to it: `hotkey`, `toggle`, `interactive`, `insert-last`, `cancel`, `status`. |

The evdev listener reads only keyboard devices, tracks only the chord keys, and ignores its own virtual keyboard. It never logs or injects. Niri's config parser has no key-release action, so there is no way to get hold without this. Setup explains the trade-off in one sentence and shows the command. The GlobalShortcuts portal is checked at startup; if it ever delivers Activated and Deactivated on Niri, the app uses it instead and needs nothing else.

GNOME, KDE and X11 are not v1 targets. On an unsupported desktop the app runs in clipboard mode: the hotkey is whatever the user binds to `voice toggle`, the overlay is a normal window, and the text is copied for the user to paste. The settings screen says so.

### Data on disk

macOS: `~/Library/Application Support/<name>/`. Linux: XDG data, config and cache dirs. Owner-only.

```
settings.toml
vocabulary.txt           one term per line
history.sqlite           utterances, FTS index
audio/<id>.wav           16 kHz mono
models/<name>/           downloaded, hash-checked
```

One table, `utterances`: id, started_at, duration_ms, engine, raw_text, text (equals raw_text unless edited), status (done, empty, canceled, discarded, recovered, failed), pasted_to, pasted_at. Retry recognition adds a new row pointing at the same audio file. Raw text is never updated in place.

Nothing is ever uploaded. The optional cloud engine sends audio only while it is the selected engine, and the overlay shows "cloud" the whole time.

### Settings

```toml
hotkey = "ctrl+alt+space"          # +shift = interactive; ctrl+alt+v = insert last
engine = "moonshine-small-en"      # per-platform default from the manifest
vocabulary = false                 # bias recognition with vocabulary.txt

[apps."com.mitchellh.ghostty"]
paste = "ctrl+shift+v"
send = false
```

That is the whole surface. Microphone is the system default unless chosen in setup. History is kept forever until deleted; the settings screen shows disk use and a "delete older than" button.

`voice import-vocabulary wispr|codex` reads the terms from a Wispr Flow database or Codex `dictationDictionary` into `vocabulary.txt` for the user to look over. It imports terms only, never history or settings.

## Benchmark

A CLI, `voice bench <manifest>`, runs clips through any engine adapter and scores the raw output against a literal reference. It is the gate for choosing and changing the default engine, and a small subset runs in CI.

Corpus: `bench/clips.jsonl` with audio hash, exact reference, tags, and license. Public clips are synthetic (TTS scripts, listened to before acceptance) or consented recordings. Private clips live outside the repo and run with the same command. Scripts cover: negatives and their affirmative twins, negatives at the start and end of a clip, "no" as a backchannel, self-repairs, repeats, fillers, identifiers, paths, flags, versions and numbers, profanity, one-word clips, silence and noise, long monologues.

Scores, per engine and per platform:

| Metric | What it reports |
|---|---|
| Negation | Dropped, added or flipped negations over tagged opportunities. The most important number. |
| Fillers and repeats | Recall and precision by class. Repeats are never deduped by the scorer. |
| Identifiers | Exact match on tagged identifiers, paths and flags. |
| Literal WER | Over the literal reference, fillers kept. Normalized WER as a second column for comparison with the outside world. |
| Hallucination | Words produced on silence or noise. |
| Latency | First partial, release to final, p50 and p95, replayed in real time. |
| Resources | Peak RSS, cold load. Linux target under 1 GB total. |

Default engine per platform is the best candidate in this order: negation, then fillers and identifiers, then WER, then latency and memory. Dropped negatives are never offset by better punctuation. A manifest records model hash, runtime version, settings and host so a run is reproducible. Any change to engine or model reruns the full suite before it becomes the default.

Target budgets, to measure not market: hotkey to "Listening" under 50 ms; first live words under 800 ms p50; release to final under 1 s p95 for 5 to 20 s clips; final to paste under 200 ms.

## Tests

- **Core**: the state machine against every row of the edge-case table, with fake audio, engine and platform. Raw text byte-identical through the whole path. Recovery on restart. Late events dropped by ID.
- **Audio**: callback never allocates; one-hour run bounded; kill -9 mid-write leaves a readable wav.
- **Engine**: the Fake adapter trace tests; the worker protocol with truncated and oversized frames; worker crash and restart; repeated words preserved across chunks.
- **Golden audio**: a dozen public clips through the real engine in CI, checking the benchmark counts, not exact strings.
- **Linux end to end**: a nested Niri with the headless backend, its own Wayland socket, runtime dir and D-Bus, so nothing touches the live desktop. Scratch targets: a GTK text view, a terminal running `cat`, an Electron text area. Hotkey via the bind and via a fake evdev device. Assert exact pasted bytes, no Enter, overlay never focused, focus-moved blocks the paste, cancel pastes nothing, restart recovers.
- **macOS**: a checklist run on a signed build in a separate user account: permission grant, deny and revoke; hold and tap; Secure Input; paste into TextEdit, Terminal, Ghostty, a browser textarea and an Electron composer; sleep and wake; mic unplug; worker crash.
- **Public CI**: format, lint, build, core and engine tests, golden subset, on Linux and macOS. The nested Niri job runs on Linux. Signed Mac checks and the full bake-off are release evidence, run by hand.

## Packaging and install

- **macOS**: signed and notarized arm64 `.app` in a `.dmg`. Stable bundle id from `product.toml`. Models download on first run, hash-checked, outside the bundle. Minimum macOS 14.
- **Linux**: x86_64 AppImage and a PKGBUILD for Arch. The AppImage is tested on a clean distro. Models download on first run.
- A per-user autostart entry, a `voice` CLI on PATH, and on Niri a one-line include snippet for the bind. The install never edits the user's compositor config without showing the line.
- No auto-updater in v1. Versioned, checksummed releases; settings and history survive upgrades; schema migrations are transactional.
- MIT for our code. Each model's license is in its manifest and shown in setup.

The app runs on the machine with the microphone and the focused app. The user installs it on both the Mac and the Linux desktop; each has its own history and models. No sync.

## Build plan

### Modules

Freeze the shared types first: `Utterance`, `Target`, `EngineEvent`, `Hotkey`, the `Engine` and `Platform` traits, the worker wire format, and the SQLite schema. That is a half-day. After that every module builds against those types with fakes, in parallel. Only the integrator changes a shared type, and updates all users.

| Module | Owns | Interface | Done when |
|---|---|---|---|
| core | `crates/core` | events in, effects out; `History` (insert, search, recover, delete); `Settings` load/save | Every edge-case row has a passing trace test. Raw text is byte-identical end to end. Recovery and insert-last work after a restart. FTS search works. |
| audio | `crates/audio` | `list_devices()`, `Recorder::start(device) -> frames + level`, `stop()` | Callback allocation-free. One-hour run flat RAM. Unplug yields an end event. Killed mid-write leaves a readable wav. |
| engine | `crates/engine` | `Engine` trait; worker binary mode and host client; `moonshine`, `fake` adapters; `scribe` after | Fake trace tests pass. Worker protocol fuzzed. Crash restarts. Repeated words kept. Moonshine runs the golden clips. The bench uses this crate unchanged. |
| platform-macos | `crates/platform/macos` | `Platform` trait | Signed test-account checklist passes: hold vs tap, Escape cancel, Secure Input detected, paste into the five scratch targets, no Enter unless `send`, NSPanel never focused. |
| platform-linux | `crates/platform/linux` | `Platform` trait; control socket; evdev listener | Nested Niri e2e passes: bind toggle, evdev hold with a fake device, exact paste into GTK, terminal and Electron scratch targets, overlay never focused, clipboard mode on a plain Wayland session. |
| ui | `app/src` | reads core state, sends core actions | Overlay, interactive, history, settings, setup screens. Provisional vs final styling. Edits are never overwritten by late recognition. Enter is a newline. Keyboard-navigable. The UI never pastes directly. |
| bench | `bench/` | `voice bench <manifest>`; scorer; corpus manifest | Reproducible scores on the public corpus. Same command runs a private corpus. CI subset under five minutes. `RESULTS.md` names the default engine per platform with numbers. |
| app and packaging | `app/src-tauri`, `packaging/` | composition, CLI, dmg, AppImage, PKGBUILD, autostart, Niri snippet | Fresh install on a clean Mac account and a clean Arch VM: setup, dictate into a terminal and an Electron app, interactive mode, insert-last, restart recovery. |

### Order

1. Shared types and fakes (half a day).
2. core, audio, engine, both platforms, ui, bench in parallel.
3. Integrate quick dictation on Mac and Niri with the fake engine, then the real one.
4. Interactive mode, history, vocabulary import.
5. Bake-off result into the manifest. Packaging. Install on both machines.
6. Fresh-eyes review against this spec on the real binary. Then branding.

## Not in v1

- A PTY wrapper for terminals. Bracketed paste in the agent TUIs covers multi-line; the e2e matrix proves it per target.
- Paste receipts or field readback. We cannot verify a paste universally, so we save first and keep the clipboard instead.
- LocalAgreement preview for batch engines. The default engines stream. Add it only if the bake-off picks a batch engine.
- A separate root helper for hold on Niri. The `input` group is one command and the listener is in the app.
- Spell and symbol conversion in review. The editor is enough.
- Per-entry enable flags on vocabulary. One file, one switch.
- GNOME, KDE, X11 adapters. Clipboard mode only.
- LLM cleanup in any form, voice commands, auto-learning corrections, screen or repo context, cloud accounts, sync, Windows, mobile, plugins, auto-update, clipboard restore.

## Where this differs from the first draft

- **Delivery evidence.** The first draft had a prepared, posted, confirmed state machine with receipts and readback. No adapter could produce a real receipt, so the states were theater. Save first, paste once, leave the clipboard. Same safety, a tenth of the code.
- **Auto-send.** The draft required an exact receipt before Enter, which made the feature unreachable. It is now an honest opt-in per app: paste, short gap, Enter, only in quick mode and only when focus is unchanged.
- **Hold on Niri.** The draft wanted a privileged helper service with its own socket and security review. Joining the `input` group is one command on a single-user desktop and the evdev read lives in the app. The portal path is checked at startup and wins if it ever works.
- **Engine events.** Segment maps with byte ranges, revisions and conflicting finals became `Partial` and `Committed`. Every streaming engine maps onto these. The "final words never change" rule is enforced by the type, not by a conflict resolver.
- **Hold and toggle.** One key: hold to talk, tap to toggle. No activation setting.
- **Audio.** Store 16 kHz mono wav, not source-rate checked PCM records. Every engine takes 16 kHz, playback is fine, and a wav is one third the size.
- **Data model.** One table instead of sessions, takes, versions and attempts. Raw text stays immutable; retry makes a new row.
- **Terminal PTY bridge.** Cut. It solved a problem the agent TUIs already solve with bracketed paste, and it was the largest single piece of machinery in the draft.
