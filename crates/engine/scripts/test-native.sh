#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
cache="$HOME/.cache/voice-models/runtime"
source="$cache/source-$(jq -r '.runtime.source.revision' "$root/models.json")"
runtime="$cache/moonshine-$(jq -r '.runtime.version' "$root/models.json")"
mkdir -p "$cache/native-tests"
export TMPDIR="$cache/native-tests"
c++ -std=c++20 -O2 -I "$source/core" -I "$source/core/third-party/onnxruntime/include" "$root/tests/native-segments.cpp" \
    -L "$runtime/lib" -Wl,-rpath,"$runtime/lib" -lmoonshine -o "$cache/native-tests/segments"
"$cache/native-tests/segments"
