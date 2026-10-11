#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
destination="${1:?supply a license staging directory}"
mkdir -p "$destination/engine" "$destination/fonts"
install -m644 "$root/LICENSE" "$root/NOTICE" "$destination/"
install -m644 "$root"/crates/engine/licenses/*.txt "$destination/engine/"
install -m644 "$root"/licenses/fonts/*.txt "$destination/fonts/"
install -m644 "$root"/licenses/CC*.txt "$destination/"
install -m644 "$root/crates/platform/macos/THIRD-PARTY.md" "$destination/macos-adaptations.md"
install -m644 "$root/bench/LICENSES.md" "$destination/corpus.md"
bun "$root/packaging/notices.ts" "$destination/dependencies.txt"
