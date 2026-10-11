# Install said

## macOS

1. Copy `said.app` from the DMG to `/Applications`. Open that exact app in Finder.
   This build is ad-hoc signed, not notarized. After macOS blocks the first open,
   go to **System Settings → Privacy & Security → Open Anyway**, authenticate and
   choose **Open**. Normally this approval is needed once per downloaded build.
2. Open **Setup** from said's tray menu. Choose **Grant Microphone**, then **Allow**.
   In **Privacy & Security → Accessibility**, add `/Applications/said.app` with
   **+** and enable it. Repeat under **Input Monitoring**, then quit/reopen said.
3. Let Setup download and verify the model. Default hotkey: **Control+Option+Space**.
   Hold to dictate until release; tap to start, tap again to stop. Add Shift for
   interactive review. **Control+Option+V** inserts the last transcript.

Rebuilding an ad-hoc app can invalidate grants; re-enable the exact installed app
after upgrades. Installation does not launch it or trigger permission prompts.
For autostart, add said to **General → Login Items → Open at Login**.

## Linux

Install the Arch package and enable the graphical-session user service described
in [builds](README.md). Back up Niri's bind file before adding the provided include.
Super+Ctrl+Alt+Space toggles recording; configure that chord in said Settings for
genuine hold after joining `input` and logging out/in. Model download is automatic
and hash-checked. `said status` passively inspects the running host.
