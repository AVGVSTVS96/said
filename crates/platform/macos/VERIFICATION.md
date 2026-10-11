# Verification

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
