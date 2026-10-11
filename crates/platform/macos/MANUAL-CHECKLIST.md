# One-time macOS acceptance

Pending: run after the host wires the adapter and produces the app bundle.
Use a separate macOS test account and scratch documents, never live conversations.
The automatic checks require no TCC grants. Keep the bundle ID from product.toml
and the same signing identity/build location throughout these checks. Re-signing
an ad-hoc build can invalidate grants; test one unchanged bundle.

1. Build the host's app bundle. Ad-hoc sign that bundle once with
   `codesign --force --deep --sign - target/release/bundle/macos/said.app`, then
   verify `codesign --verify --deep --strict target/release/bundle/macos/said.app`.
   Launch the bundle from Finder. Its Info.plist must contain
   `NSMicrophoneUsageDescription` before using Grant Microphone.
2. In System Settings > Privacy & Security > Accessibility, add that exact app
   using **+** and enable it. Do the same in **Input Monitoring**. Quit and reopen.
   Use the setup screen's **Grant Microphone** once, choose **Allow**, and confirm
   the app appears enabled under **Microphone**. A denial must show its exact
   Settings recovery path; a policy restriction must say to contact an admin.
3. Focus a scratch field. Hold Control+Option+Space for over 300 ms and release:
   exactly one capture starts and ends. Repeat releasing Option before Space.
   Tap for under 300 ms: capture stays on; the next tap stops. Hold the chord
   through key repeat: no duplicate start. Shift+the chord starts interactive mode.
   Escape cancels hold and toggle recordings, leaves a canceled history entry,
   and inserts nothing. Escape passes through normally when idle.
4. While recording, type in the scratch field and click the overlay close button.
   The panel never becomes the key/main window and the app never activates.
   Test light/dark themes, fullscreen, a second screen and reduced motion.
   Switch to another window of the same app while finishing: nothing is pasted.
   Switch away and back: automatic paste stays inhibited. Close the target:
   saved text stays available. Interactive Insert refocuses its original window
   and pastes only after the host observes that focus.
5. Enable Terminal > Secure Keyboard Entry, including during a toggle recording.
   Within one polling interval the tray shows Secure Input, capture stops and
   saves, and no paste happens. Disable it: hotkeys recover without relaunch.
   Repeat with screen lock, then unlock. Revoke Input Monitoring: recovery names
   that permission and no new prompt appears. Regrant and relaunch.
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
