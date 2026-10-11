#!/usr/bin/env bash
set -euo pipefail
said_build_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_BUILD_JOBS=2
said_prefixes=(
    "$HOME=/build/home"
    "${CARGO_HOME:-$HOME/.cargo}/registry=/build/cargo/registry"
    "${CARGO_HOME:-$HOME/.cargo}/git=/build/cargo/git"
    "${RUSTUP_HOME:-$HOME/.rustup}=/build/rustup"
    "$said_build_root=/build/said"
    "${CARGO_TARGET_DIR:-$said_build_root/target}=/build/target"
    "$(rustc --print sysroot)=/build/toolchain"
)
said_rust_flags=()
for said_prefix in "${said_prefixes[@]}"; do
    said_rust_flags+=("--remap-path-prefix=$said_prefix")
    export CFLAGS="${CFLAGS:-} -ffile-prefix-map=$said_prefix"
    export CXXFLAGS="${CXXFLAGS:-} -ffile-prefix-map=$said_prefix"
done
export CARGO_ENCODED_RUSTFLAGS
CARGO_ENCODED_RUSTFLAGS="$(printf '%s\037' "${said_rust_flags[@]}")"
CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS%$'\037'}"
unset RUSTFLAGS
