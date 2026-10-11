#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
destination="${1:?supply staging prefix}"
target="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p "$destination/bin" "$destination/lib/said"
install -m755 "$target/release/said" "$destination/bin/said"
runtime="${SAID_RUNTIME_DIR:-$root/target/packaging/runtime}"
"$root/packaging/runtime.sh" "$runtime"
install -m644 "$runtime/runtime.archive" "$destination/lib/said/runtime.archive"
install -Dm644 "$root/packaging/linux/said.desktop" "$destination/share/applications/said.desktop"
install -Dm644 "$root/packaging/linux/said.service" "$destination/lib/systemd/user/said.service"
install -Dm644 "$root/packaging/linux/niri.kdl" "$destination/share/said/niri.kdl"
install -Dm644 "$root/packaging/linux/autostart.kdl" "$destination/share/said/autostart.kdl"
mkdir -p "$destination/share/said/tray"
install -m644 "$root"/app/src-tauri/icons/tray/*.png "$destination/share/said/tray/"
install -Dm644 "$root/LICENSE" "$destination/share/licenses/said/LICENSE"
cp -r "$root/crates/engine/licenses" "$destination/share/licenses/said/engine"
for icon in "$root"/app/src-tauri/icons/linux/*.png; do
    size="${icon##*-}"; size="${size%.png}"
    install -Dm644 "$icon" "$destination/share/icons/hicolor/${size}x${size}/apps/said.png"
done
