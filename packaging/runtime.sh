#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
destination="${1:?supply a runtime output directory}"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) key=linux-x86_64 ;;
    Darwin-arm64) key=macos-aarch64 ;;
    *) echo 'Unsupported architecture' >&2; exit 1 ;;
esac
manifest="$root/crates/engine/models.json"
url="$(jq -er --arg key "$key" '.runtime.artifacts[$key].url' "$manifest")"
hash="$(jq -er --arg key "$key" '.runtime.artifacts[$key].sha256' "$manifest")"
mkdir -p "$destination"
archive="$destination/runtime.archive"
verify() {
    local actual
    actual="$(openssl dgst -sha256 "$1")"
    test "${actual##* }" = "$hash"
}
if ! test -f "$archive" || ! verify "$archive"; then
    curl --fail --location --retry 3 "$url" -o "$archive.part"
    verify "$archive.part"
    mv "$archive.part" "$archive"
fi
mkdir -p "$destination/unpacked"
tar xjf "$archive" -C "$destination/unpacked" --strip-components=1
case "$key" in
    linux-*)
        for library in "$destination"/unpacked/lib/*.so; do
            readelf -d "$library" | awk '/RPATH|RUNPATH/ { if ($0 !~ /\[\$ORIGIN\]/) exit 1 }'
        done ;;
    macos-*)
        for library in "$destination"/unpacked/lib/*.dylib; do
            if otool -L "$library" | tail -n +2 | awk '{print $1}' | grep -E '^/(Users|private|opt|home)/'; then
                echo 'Native runtime has a build-machine dependency' >&2; exit 1
            fi
        done ;;
esac
