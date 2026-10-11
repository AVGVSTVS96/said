#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
output="${1:-$root/target/packaging}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
scratch="$(mktemp -d /var/tmp/said-build.XXXXXX)"
trap 'rm -rf "$scratch"' EXIT
cp "$root/packaging/linux/PKGBUILD" "$scratch/PKGBUILD"
cat > "$scratch/makepkg.conf" <<CONFIG
source /etc/makepkg.conf
BUILDDIR='$scratch/build'
PKGDEST='$output'
PACKAGER='said contributors'
CONFIG
cd "$scratch"
SAID_REPO="$root" SAID_REVISION="$(git -C "$root" rev-parse HEAD)" \
    makepkg --config "$scratch/makepkg.conf" --noconfirm
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n1)"
package="$output/said-$version-1-x86_64.pkg.tar.zst"
python3 "$root/packaging/check-private-paths.py" "$package"
portable="$scratch/said-$version-linux-x86_64"
mkdir -p "$scratch/extracted"
tar -xf "$package" -C "$scratch/extracted"
mv "$scratch/extracted/usr" "$portable"
tar --owner=0 --group=0 --numeric-owner -czf "$output/said-$version-linux-x86_64.tar.gz" -C "$scratch" "${portable##*/}"
python3 "$root/packaging/check-private-paths.py" "$output/said-$version-linux-x86_64.tar.gz"
