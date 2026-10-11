# Builds

Use Rust 1.90+, Bun 1.4+, curl, jq, OpenSSL and bzip2. The build verifies the native
archive pinned in `crates/engine/models.json` and rejects build-machine library
paths. Linux libraries use `$ORIGIN`; macOS uses `@rpath`. All sherpa/ONNX libraries
ship compressed in the archive. The host finds the archive in the installed
layout, stages it in private app data, rechecks its SHA-256 before extraction and
loads the C ABI from there.
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
systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0 -- env CARGO_BUILD_JOBS=2 makepkg
sudo pacman -U said-*.pkg.tar.zst
systemctl --user enable --now said.service
```

The PKGBUILD builds the committed checkout with two Cargo jobs and locked Rust/Bun
dependencies. `SAID_REPO` and `SAID_REVISION` select another local clone/commit.
Models are not packaged. `said` is on PATH. The service has a 4G memory cap and no swap. The user service starts/stops with
`graphical-session.target`; Niri's systemd session imports Wayland environment
before activating that target. Enable it in a systemd graphical session.

On Niri, prefer its native startup hook: include `/usr/share/said/autostart.kdl`
from the compositor configuration instead of enabling the user service. It starts
a 4G/no-swap scope tied to `graphical-session.target`, inheriting the Wayland
environment. For physical hold/release, join `input` and start a fresh login before running
said. The startup snippet invokes the capped host directly. Choose one startup
mechanism, so only one app runs.

Show `packaging/linux/niri.kdl` before adding this include to Niri configuration:

```kdl
include "/usr/share/said/niri.kdl"
```

The binds and settings use the same shortcut defaults, listed in [INSTALL](INSTALL.md).
Readable keyboards or a working portal provide hold/release; the Niri bind alone
toggles. `sudo usermod -aG input "$USER"` grants physical keyboard-device access
and requires logout/login. `said status` reports the active mode.

For a portable directory, after a release build read the workspace version from
`Cargo.toml`, run `packaging/linux/install.sh target/packaging/said-VERSION-linux-x86_64`
with that version, then tar that directory. Run its `bin/said`; the runtime archive stays at `lib/said/runtime.archive`, so
moving the directory preserves relative paths.
It needs the listed desktop libraries and compatible glibc. This is not a
clean-distro-tested AppImage; cross-distro compatibility remains unverified.

## Apple Silicon

Install Xcode command-line tools, Rust, Bun, jq and OpenSSL. On an idle Mac:

```sh
nice -n 10 packaging/macos.sh
```

The arm64 `.app` is under `target/aarch64-apple-darwin/release/bundle/macos/`;
the compressed `.dmg` and SHA256SUMS are under `target/packaging/`. Deployment
targets macOS 14. The host stays at `Contents/MacOS/said`, with `runtime.archive` in Resources.
The complete bundle is ad-hoc signed and verified. This script does not perform Developer ID signing or notarization.
See [INSTALL](INSTALL.md). Builds do not pass the separate-account manual checklist.

First-launch behavior and permission steps are described in [INSTALL](INSTALL.md).
Versioned artifacts should include SHA256SUMS. Verify Linux `ldd`, isolated-cache
native loading, macOS `otool -L`, and complete bundle signatures. A successful
build does not certify the separate-account macOS acceptance checklist.

## Release verification

Build and install one final artifact from the intended committed revision, using
the platform recipes above. Linux release builds use the 4 GiB/no-swap scope;
model-loading acceptance checks also use the shared lock from
[CONTRIBUTING.md](../CONTRIBUTING.md#native-verification). macOS builds run at low
priority on an idle desktop. Do not substitute an old locally built host for the
new artifact during release acceptance.

On Linux, launch the installed host through the compositor or the user service:

```sh
niri msg action spawn -- said
# Alternatively:
systemctl --user start said.service
said status
```

Choose one launch path. The Niri startup include and user service retain their
memory caps and start the host directly. Compositor/service ownership keeps the
process alive independently of the build terminal. Inspect Setup, input mode,
model readiness and the configured bind instructions before scratch delivery.

On macOS, copy the final app from its DMG to `/Applications` and open that bundle
in Finder. Verify its complete signature and [permission/input/delivery checklist](../crates/platform/macos/MANUAL-CHECKLIST.md)
against that same bundle. A rebuilt ad-hoc signature can invalidate previous
grants. Record the artifact hash, source revision, platform and observed results;
compilation and model benchmark receipts do not certify installed desktop behavior.

## Licenses

Both build paths call `packaging/install-notices.sh`. The Arch package and portable
directory contain `share/licenses/said/` (under `/usr` in the Arch package); the Mac
app contains `Contents/Resources/licenses/`. Each has the app LICENSE, [NOTICE](../NOTICE),
model/runtime terms, the matching ONNX Runtime third-party notices, native dependency
terms, Handy/tauri-nspanel credits, font OFLs and corpus provenance.

`dependencies.txt` is generated from locked Rust/JavaScript package sources using
Bun and Cargo metadata. It inventories both host targets, including build/test
packages, plus production frontend dependencies, and retains complete supplied
terms. Missing terms or a hash mismatch abort packaging. [License sources](../licenses/README.md)
describes the verified upstream fallbacks and source links.

```sh
bash packaging/tests/notices.sh
```

The check stages both layouts, compares complete notice contents, round-trips the
Linux tree through archives and verifies rejection of a dependency missing terms.
It does not rebuild/sign the app or establish runtime acceptance.

The PKGBUILD consumes a local clone; a public upstream URL has not been configured.
