#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
cache="$HOME/.cache/voice-models/runtime"
patch_hash=$(openssl dgst -sha256 "$root/moonshine-0.1.5.patch")
patch_hash=${patch_hash##* }
source="$cache/source-$(jq -r '.runtimes.moonshine.source.revision' "$root/models.json")-$patch_hash"
runtime="$cache/moonshine-$(jq -r '.runtimes.moonshine.version' "$root/models.json")"
mkdir -p "$cache/native-tests"
export TMPDIR="$cache/native-tests"
c++ -std=c++20 -O2 -I "$source/core" -I "$source/core/third-party/onnxruntime/include" "$root/tests/native-segments.cpp" \
    -L "$runtime/lib" -Wl,-rpath,"$runtime/lib" -lmoonshine -o "$cache/native-tests/segments"
"$cache/native-tests/segments"
