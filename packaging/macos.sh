#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
test "$(uname -m)" = arm64
idle="$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ && !seen {print int($NF/1e9); seen=1}')"
test "$idle" -ge 300 || { echo "Mac active ($idle idle seconds); build postponed" >&2; exit 1; }
export CARGO_BUILD_JOBS=2 MACOSX_DEPLOYMENT_TARGET=14.0
runtime="$root/target/packaging/runtime"
"$root/packaging/runtime.sh" "$runtime"
cd app
bun install --frozen-lockfile
bun run tauri build --target aarch64-apple-darwin --bundles app
cd "$root"
bundle="$root/target/aarch64-apple-darwin/release/bundle/macos/said.app"
mv "$bundle/Contents/MacOS/said" "$bundle/Contents/MacOS/said-host"
SAID_RUNTIME_VERSION="$(jq -r '.runtime.version' crates/engine/models.json)" \
    rustc --edition=2024 -O packaging/launcher.rs -o "$bundle/Contents/MacOS/said"
cp "$runtime/runtime.archive" "$bundle/Contents/Resources/runtime.archive"
mkdir -p "$bundle/Contents/Resources/tray"
cp app/src-tauri/icons/tray/*.png "$bundle/Contents/Resources/tray/"
mkdir -p "$bundle/Contents/Resources/licenses"
cp -R crates/engine/licenses/. "$bundle/Contents/Resources/licenses/"
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
output="$root/target/packaging"
stage="$output/dmg-stage"
mkdir -p "$stage"
ditto "$bundle" "$stage/said.app"
ln -sfn /Applications "$stage/Applications"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
hdiutil create -ov -volname said -srcfolder "$stage" -format UDZO "$output/said-$version-macos-arm64.dmg"
rm -rf "$stage"
shasum -a 256 "$output/said-$version-macos-arm64.dmg" > "$output/SHA256SUMS"
