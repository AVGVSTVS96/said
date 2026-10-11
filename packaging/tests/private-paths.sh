#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/var/tmp}/said-private-check.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
check=(python3 "$root/packaging/check-private-paths.py" --private-name synthetic-builder --private-home /home/synthetic-builder)
mkdir -p "$scratch/tree"
printf 'neutral build data\n' > "$scratch/tree/host"
"${check[@]}" "$scratch/tree"
printf '/home/synthetic-builder/source.rs\n' > "$scratch/tree/host"
tar --owner=0 --group=0 --numeric-owner -cjf "$scratch/runtime.archive" -C "$scratch/tree" .
if "${check[@]}" "$scratch/runtime.archive"; then
    echo 'Nested private path was accepted' >&2
    exit 1
fi
printf 'packager = synthetic-builder\n' > "$scratch/tree/.BUILDINFO"
printf 'neutral\n' > "$scratch/tree/host"
tar --owner=0 --group=0 --numeric-owner -czf "$scratch/package.tar.gz" -C "$scratch/tree" .
if "${check[@]}" "$scratch/package.tar.gz"; then
    echo 'Private package metadata was accepted' >&2
    exit 1
fi

printf 'neutral\n' > "$scratch/tree/.BUILDINFO"
tar --owner=synthetic-builder --group=0 -czf "$scratch/owner.tar.gz" -C "$scratch/tree" .
if "${check[@]}" "$scratch/owner.tar.gz"; then
    echo 'Private archive owner was accepted' >&2
    exit 1
fi
