#!/usr/bin/env bash
set -euo pipefail
manifest="$(cd "$(dirname "$0")/.." && pwd)/models.json"
cache="$HOME/.cache/voice-models"
case "$(uname -s)" in Linux) platform=linux ;; Darwin) platform=macos ;; *) exit 1 ;; esac
case "$(uname -m)" in x86_64) arch=x86_64 ;; arm64|aarch64) arch=aarch64 ;; *) exit 1 ;; esac
model="${1:-$(jq -r --arg platform "$platform" '.defaults[$platform]' "$manifest")}"
directory="$(jq -er --arg model "$model" '.models[$model].directory' "$manifest")"
base="$(jq -er --arg model "$model" '.models[$model].source' "$manifest")"
mkdir -p "$cache/$directory"
verify() {
    actual="$(openssl dgst -sha256 "$1")"
    test "${actual##* }" = "$2"
}
while IFS=$'\t' read -r filename hash; do
    file="$cache/$directory/$filename"
    if test -f "$file" && verify "$file" "$hash"; then continue; fi
    curl -fsSL "$base$filename" -o "$file.part"
    verify "$file.part" "$hash"
    mv "$file.part" "$file"
done < <(jq -r --arg model "$model" '.models[$model].files | to_entries[] | [.key,.value] | @tsv' "$manifest")
artifact="$(jq -er --arg key "$platform-$arch" '.runtime.artifacts[$key]' "$manifest")"
library="$(jq -er --arg platform "$platform" '.runtime.libraries[$platform]' "$manifest")"
runtime="$cache/$(dirname "$(dirname "$library")")"
mkdir -p "$cache/runtime"
scratch="$(mktemp -d "$cache/runtime/download.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
curl -fsSL "$(jq -r '.url' <<<"$artifact")" -o "$scratch/archive"
verify "$scratch/archive" "$(jq -r '.sha256' <<<"$artifact")"
mkdir "$scratch/unpacked"
test "$(jq -r '.format' <<<"$artifact")" = tar.bz2
tar xjf "$scratch/archive" -C "$scratch/unpacked" --strip-components=1
mkdir -p "$runtime"
while IFS= read -r -d '' source; do
    destination="$runtime/${source#"$scratch/unpacked/"}"
    mkdir -p "$(dirname "$destination")"
    cp "$source" "$destination.part"
    mv "$destination.part" "$destination"
done < <(find "$scratch/unpacked" -type f -print0)
printf 'Ready: %s (%s), %s\n' "$model" "$directory" "$runtime"
