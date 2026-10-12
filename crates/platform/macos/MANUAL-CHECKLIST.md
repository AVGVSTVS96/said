# One-time macOS acceptance

Pending live acceptance of the packaged app.
Use a separate macOS test account and scratch documents, never live conversations.
The automatic checks require no TCC grants. Keep the bundle ID from product.toml
and the same signing identity/build location throughout these checks. Re-signing
an ad-hoc build invalidates grants; sign with a stable identity
([packaging](../../../packaging/README.md#apple-silicon)) and test one unchanged bundle.

1. Build the bundle with `packaging/macos.sh`, then verify
   `codesign --verify --deep --strict target/aarch64-apple-darwin/release/bundle/macos/said.app`
   and that `codesign -dv` names the stable identity, not `adhoc`.
   Launch the bundle from Finder. Its Info.plist must contain
   `NSMicrophoneUsageDescription` before pressing Allow for Microphone.
2. Before any grant: the overlay panel appears without activating said and Copy
   works. Press Setup's **Allow** for Accessibility: the system prompt appears;
   enable said. Within a second Setup shows Accessibility authorized and
   Hold-to-talk available, without relaunching; Input Monitoring is not listed.
   Press **Allow** for Microphone, choose **Allow**, and confirm Setup updates.
   Deny Microphone on a fresh account: **Allow** then opens the Microphone
   pane. A policy restriction must say to contact an admin. Setup can finish.
3. Focus a scratch field. Hold Option+Space for over 300 ms and release:
   exactly one capture starts and ends. Repeat releasing Option before Space.
   Tap for under 300 ms: capture stays on; the next tap stops. Hold the chord
   through key repeat: no duplicate start. Option+X starts interactive mode; Option+A inserts the latest stored transcript.
   Change all three shortcuts in Settings and verify the new bindings and hint.
   Escape cancels hold and toggle recordings, leaves a canceled history entry,
   and inserts nothing. Escape passes through normally when idle.
4. While recording, type in the scratch field and click the overlay close button.
   The panel never becomes the key/main window and the app never activates.
   Test light/dark themes, fullscreen, a second screen and reduced motion.
   Dictate into a scratch field without switching away: the text is pasted.
   Switch to another window of the same app while finishing: nothing is pasted.
   Switch away and back: automatic paste stays inhibited. Close the target:
   saved text stays available. Interactive Insert refocuses its original window
   and pastes only after the host observes that focus.
5. Enable Terminal > Secure Keyboard Entry, including during a toggle recording.
   Within one polling interval the tray shows Secure Input, capture stops and
   saves, and no paste happens. Disable it: hotkeys recover without relaunch.
   Repeat with screen lock, then unlock. Revoke Accessibility: recovery names
   that permission, capture stops and no prompt appears. Regrant: shortcuts
   recover without relaunch.
6. Paste the synthetic text `no no, um actually no --no-verify  a/b` plus leading
   and trailing spaces, a tab and two newlines into scratch targets: TextEdit,
   Terminal, Ghostty, a browser textarea and an Electron agent composer. Compare
   clipboard/inserted text exactly. Confirm no submission with send off and with
   Insert last/interactive Insert. Enable send only for one scratch app: exactly
   one Return follows paste; switching focus before Return withholds it. Repeat
   using US, Dvorak and a non-Latin keyboard layout, with Command/Option already
   held. Text stays on the clipboard. Unsupported window/layout reports Copy
   recovery and injects nothing. Never retry an uncertain paste automatically.

Record macOS version, app commit/signature and pass/fail for these six steps.
A passing compile and headless decoder test do not mark this checklist passed.
