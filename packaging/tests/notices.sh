#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d "${XDG_CACHE_HOME:-$HOME/.cache}/said-notices-test.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
linux="$scratch/linux/usr/share/licenses/said"
macos="$scratch/macos/said.app/Contents/Resources/licenses"
for destination in "$linux" "$macos"; do
    "$root/packaging/install-notices.sh" "$destination"
    cmp "$root/LICENSE" "$destination/LICENSE"
    cmp "$root/NOTICE" "$destination/NOTICE"
    cmp "$root/app/src/fonts/OFL.txt" "$destination/fonts/courier-prime.txt"
    cmp "$root/licenses/fonts/instrument-sans.txt" "$destination/fonts/instrument-sans.txt"
    cmp "$root/crates/platform/macos/THIRD-PARTY.md" "$destination/macos-adaptations.md"
    cmp "$root/bench/LICENSES.md" "$destination/corpus.md"
    for source in "$root"/crates/engine/licenses/*.txt; do
        cmp "$source" "$destination/engine/${source##*/}"
    done
    rg -q 'Copyright 2008 Google Inc' "$destination/engine/onnxruntime-third-party.txt"
    rg -q 'CC-BY-4.0' "$destination/engine/parakeet.txt"
    rg -q 'react@' "$destination/dependencies.txt"
    rg -q 'tauri@' "$destination/dependencies.txt"
    rg -q 'objc2@' "$destination/dependencies.txt"
    rg -q 'MPL-covered source' "$destination/dependencies.txt"
done
cmp "$linux/dependencies.txt" "$macos/dependencies.txt"
for format in arch portable; do
    tar -czf "$scratch/$format.tar.gz" -C "$scratch/linux" usr
    mkdir "$scratch/$format"
    tar -xzf "$scratch/$format.tar.gz" -C "$scratch/$format"
    diff -r "$scratch/linux" "$scratch/$format"
done
diff -r "$linux" "$macos"
rg -Fq 'packaging/install-notices.sh' "$root/packaging/linux/install.sh"
rg -Fq 'packaging/install-notices.sh' "$root/packaging/macos.sh"
rg -Fq 'packaging/linux/install.sh' "$root/packaging/linux/PKGBUILD"
mkdir -p "$scratch/missing" "$scratch/bin"
cat > "$scratch/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
cat <<JSON
{"packages":[{"id":"said-app","name":"said-app","version":"0","source":null},{"id":"fixture","name":"unlicensed-fixture","version":"1.0.0","source":"registry","manifest_path":"$SAID_NOTICE_FIXTURE/Cargo.toml","license":"MIT"}],"resolve":{"nodes":[{"id":"said-app","dependencies":["fixture"]},{"id":"fixture","dependencies":[]}]}}
JSON
CARGO
chmod +x "$scratch/bin/cargo"
if SAID_NOTICE_FIXTURE="$scratch/missing" PATH="$scratch/bin:$PATH" \
    bun "$root/packaging/notices.ts" "$scratch/rejected.txt" > "$scratch/error.log" 2>&1; then
    echo 'Missing license terms were accepted' >&2
    exit 1
fi
rg -q 'Missing license texts: unlicensed-fixture@1.0.0' "$scratch/error.log"
printf 'PASS: notice layouts, archive round trips, package hooks, missing-term rejection\n'
