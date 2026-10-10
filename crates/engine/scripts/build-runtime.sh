#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
manifest="$root/models.json"
cache="$HOME/.cache/voice-models"
runtime="$cache/runtime/moonshine-$(jq -r '.runtime.version' "$manifest")"
patch_hash=$(openssl dgst -sha256 "$root/moonshine-0.1.5.patch")
patch_hash=${patch_hash##* }
source="$cache/runtime/source-$(jq -r '.runtime.source.revision' "$manifest")-$patch_hash"
build="$cache/runtime/build-$(jq -r '.runtime.version' "$manifest")-$patch_hash"
case "$(uname -s)" in
    Linux) library=libmoonshine.so; ort=libonnxruntime.so.1; platform=linux ;;
    Darwin) library=libmoonshine.dylib; ort=libonnxruntime.1.23.2.dylib; platform=macos ;;
    *) exit 1 ;;
esac
case "$(uname -m)" in x86_64) arch=x86_64 ;; arm64|aarch64) arch=arm64 ;; *) exit 1 ;; esac
if test "$platform" = linux && test "$arch" = arm64; then arch=aarch64; fi
mkdir -p "$source" "$build"
if ! test -f "$source/.voice-ready"; then
    archive="$cache/runtime/moonshine-source.tar.gz"
    expected=$(jq -r '.runtime.source.sha256' "$manifest")
    actual=$(openssl dgst -sha256 "$archive" 2>/dev/null || true)
    if test "${actual##* }" != "$expected"; then
        curl -fsSL "$(jq -r '.runtime.source.url' "$manifest")" -o "$archive.part"
        actual=$(openssl dgst -sha256 "$archive.part")
        test "${actual##* }" = "$expected"
        mv "$archive.part" "$archive"
    fi
    tar xzf "$archive" -C "$source" --strip-components=1
    patch -d "$source" -p1 < "$root/moonshine-0.1.5.patch"
    touch "$source/.voice-ready"
fi
mkdir -p "$source/core/third-party/onnxruntime/lib/$platform/$arch"
cp "$runtime/lib/$ort" "$source/core/third-party/onnxruntime/lib/$platform/$arch/$ort"
export TMPDIR="$build"
cmake -S "$source/core" -B "$build" -DCMAKE_BUILD_TYPE=Release -DMOONSHINE_TTS_BUILD_ONNX=ON
cmake --build "$build" --target moonshine -j 2
cp "$build/$library" "$runtime/lib/$library.part"
mv "$runtime/lib/$library.part" "$runtime/lib/$library"
if test "$platform" = macos; then codesign --force --sign - "$runtime/lib/$library"; fi
