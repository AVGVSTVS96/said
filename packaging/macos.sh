#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
test "$(uname -m)" = arm64
idle="$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ && !seen {print int($NF/1e9); seen=1}')"
test "$idle" -ge 300 || { echo "Mac active ($idle idle seconds); build postponed" >&2; exit 1; }
source "$root/packaging/build-env.sh"
export MACOSX_DEPLOYMENT_TARGET=14.0
identities="$(security find-identity -v -p codesigning)"
identity="${SAID_SIGN_IDENTITY:-}"
if [ -z "$identity" ]; then
    identity="$(sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' <<<"$identities" | head -n1)"
fi
if [ -z "$identity" ] && grep -qF '"said local signing"' <<<"$identities"; then
    identity="said local signing"
fi
if [ -z "$identity" ]; then
    identity="-"
    echo "Ad-hoc signing: macOS forgets permissions after every rebuild. Run packaging/macos-identity.sh once to keep them." >&2
fi
runtime="$root/target/packaging/runtime"
"$root/packaging/runtime.sh" "$runtime"
cd app
bun install --frozen-lockfile
bun run tauri build --target aarch64-apple-darwin --bundles app
cd "$root"
bundle="$root/target/aarch64-apple-darwin/release/bundle/macos/said.app"
cp "$runtime/runtime.archive" "$bundle/Contents/Resources/runtime.archive"
mkdir -p "$bundle/Contents/Resources/tray"
cp app/src-tauri/icons/tray/*.png "$bundle/Contents/Resources/tray/"
"$root/packaging/install-notices.sh" "$bundle/Contents/Resources/licenses"
codesign --force --deep --sign "$identity" "$bundle"
codesign --verify --deep --strict "$bundle"
python3 "$root/packaging/check-private-paths.py" "$bundle"
output="$root/target/packaging"
stage="$output/dmg-stage"
mkdir -p "$stage"
ditto "$bundle" "$stage/said.app"
ln -sfn /Applications "$stage/Applications"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
hdiutil create -ov -volname said -srcfolder "$stage" -format UDZO "$output/said-$version-macos-arm64.dmg"
codesign --force --sign "$identity" "$output/said-$version-macos-arm64.dmg"
python3 "$root/packaging/check-private-paths.py" "$output/said-$version-macos-arm64.dmg"
rm -rf "$stage"
cd "$output"
shasum -a 256 "said-$version-macos-arm64.dmg" > SHA256SUMS
python3 "$root/packaging/check-private-paths.py" SHA256SUMS
