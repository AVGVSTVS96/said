# Verification

## 2026-10-12: grants, focus notifications and HID paste

macOS 26.6.1, aarch64-apple-darwin, Rust 1.96.1, scratch copy of the working
tree, `CARGO_BUILD_JOBS=2`, `nice -n 10`.

```sh
cargo fmt --all --check
cargo test --locked -p said-platform --features native
cargo clippy --locked -p said-platform --features native --all-targets -- -D warnings
cargo clippy --locked -p said-app --features custom-protocol --all-targets -- -D warnings
cargo test --locked -p said-app --features custom-protocol
cargo test --locked -p said-platform --features native permission_report -- --ignored --nocapture
```

- Format, both clippy runs: pass with warnings denied.
- Platform tests: 16 passed, 1 ignored (the diagnostic). New: focus changes
  queue only when the resolved window differs, and departure then return both
  stay queued; recovery text never asks for a relaunch or Input Monitoring.
- Host tests on macOS: unit 2 passed; end-to-end 1 passed, 1 ignored (model);
  host regressions 9 passed.
- Passive diagnostic ran in an SSH session, whose grants differ from the app's.

Not established: that a filtering session tap starts with Accessibility alone on
this macOS version, live focus notifications, HID paste delivery, the
Accessibility/Input Monitoring prompts, or `packaging/macos-identity.sh` trust
setup. Those need the [manual checklist](MANUAL-CHECKLIST.md) on a built bundle.

## 2026-10-10

2026-10-10, macOS 26.6.1, aarch64-apple-darwin, Rust 1.96.1.
Implementation revision: `0678bb4`. Includes core/audio from master `ab57382`.

```sh
cargo check -p said-platform --features native --target aarch64-apple-darwin
cargo test -p said-platform --features native --target aarch64-apple-darwin
cargo clippy -p said-platform --features native --target aarch64-apple-darwin --all-targets -- -D warnings
cargo test -p said-platform --features native --target aarch64-apple-darwin permission_report -- --ignored --nocapture
cargo fmt --all --check
```

- Native check: pass.
- Native unit tests: 12 passed; the permission diagnostic is ignored by default.
- Clippy: pass, including tests, with warnings denied.
- Passive permission diagnostic: pass. No permission was requested. Microphone
  authorization was undetermined for the test process, so live audio smoke was
  skipped to avoid a TCC prompt.
- Format and whitespace checks: pass.
- SSH signature on implementation commit: verified.

The headless callback test constructs unposted Quartz events and invokes the
production tap callback directly, proving native-field decoding, suppression,
channel delivery, repeat rejection, real release pairing and conditional Escape.
Other tests cover interactive/insert-last edges, interruption reset, custom
shortcuts, layout failure, explicit event flags, lossless CFString conversion,
permission recovery and session-lock decoding.

No automated check installed a global hotkey tap, opened a panel, changed the
clipboard, activated another app, posted synthetic input, captured microphone
audio, or changed TCC grants. Tests do not demonstrate universal paste delivery.
[Live desktop acceptance](MANUAL-CHECKLIST.md) is pending execution in a separate test account. The current adapter is described in [README.md](README.md).
