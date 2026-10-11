# Builds

Use Rust 1.90+, Bun 1.4+, curl, jq, OpenSSL and bzip2. The build verifies the native
archive pinned in `crates/engine/models.json` and rejects build-machine library
paths. Linux libraries use `$ORIGIN`; macOS uses `@rpath`. All sherpa/ONNX libraries
ship compressed in the archive. A native launcher stages it in private app data;
the host checks its SHA-256 again before extraction and loads the C ABI from there.
No runtime library relies on a build directory or Python installation.

Models remain outside every artifact. First launch downloads the FP32 model into
`~/.local/share/said/models` on Linux or `~/Library/Application Support/said/models`
on macOS. Each file must match the manifest SHA-256. Settings/history survive
upgrades. Runtime/model licenses ship with the artifacts.

## Arch and CachyOS

Install `base-devel rust bun webkit2gtk-4.1 libayatana-appindicator gtk-layer-shell
alsa-lib wl-clipboard libxkbcommon curl jq openssl bzip2`, then:

```sh
cd packaging/linux
CARGO_BUILD_JOBS=2 makepkg
sudo pacman -U said-*.pkg.tar.zst
systemctl --user enable --now said.service
```

The PKGBUILD builds the committed checkout with two Cargo jobs and locked Rust/Bun
dependencies. `SAID_REPO` and `SAID_REVISION` select another local clone/commit.
Models are not packaged. `said` is on PATH. The user service starts/stops with
`graphical-session.target`; Niri's systemd session imports Wayland environment
before activating that target. Enable it in a systemd graphical session.

Show `packaging/linux/niri.kdl` before adding this include to Niri configuration:

```kdl
include "/usr/share/said/niri.kdl"
```

The example binds Super+Ctrl+Alt+Space to `said hotkey`. Match that chord in said's
Settings for physical hold/release; the default Ctrl+Alt+Space remains independent
until changed. `sudo usermod -aG input "$USER"` requires logout/login. Without
readable evdev or the shortcuts portal, the Niri bind toggles recording.

For a portable directory, after a release build run
`packaging/linux/install.sh target/packaging/said-0.0.0-linux-x86_64`, then tar that
directory. Run its `bin/said`; moving the directory preserves relative paths.
It needs the listed desktop libraries and compatible glibc. This is not a
clean-distro-tested AppImage; cross-distro compatibility remains unverified.

## Apple Silicon

Install Xcode command-line tools, Rust, Bun, jq and OpenSSL. On an idle Mac:

```sh
nice -n 10 packaging/macos.sh
```

The arm64 `.app` is under `target/aarch64-apple-darwin/release/bundle/macos/`;
the compressed `.dmg` and SHA256SUMS are under `target/packaging/`. Deployment
targets macOS 14. The complete bundle is ad-hoc signed and verified. Developer ID
signing/notarization in SPEC.md is deferred because no Developer ID exists.
See [INSTALL](INSTALL.md). Builds do not pass the separate-account manual checklist.

The app starts hidden. Open Setup from the tray menu. Brand tray artwork is in
`app/src-tauri/icons/tray`; selecting idle/recording artwork remains app-module
integration work. Ship versioned artifacts with SHA256SUMS; verify Linux `ldd`,
isolated-cache native loading, macOS `otool -L`, and complete bundle signatures.

The PKGBUILD consumes a local clone; a public upstream URL has not been configured.
