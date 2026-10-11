#!/usr/bin/env bash
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project=$(cd -- "$script_dir/../../../.." && pwd)
scratch=${SAID_LINUX_TEST_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/said-linux-tests}
mkdir -p "$scratch/harness"
cat > "$scratch/harness/Cargo.toml" <<MANIFEST
[package]
name = "said-linux-check"
version = "0.0.0"
edition = "2024"
[lib]
name = "said_platform"
path = "$project/crates/platform/src/lib.rs"
[features]
default = ["native"]
native = []
[dependencies]
said-core = { path = "$project/crates/core" }
crossbeam-channel = "0.5"
serde_json = "1"
libc = "0.2"
evdev = "0.13"
gtk = "0.18"
gtk-layer-shell = { version = "0.8", features = ["v0_6"] }
tauri = { version = "2", default-features = false, features = ["wry"] }
ashpd = "0.12.3"
futures-util = "0.3"
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }
wayland-client = "0.31"
wayland-protocols-misc = { version = "0.3", features = ["client"] }
xkbcommon = { version = "0.9", default-features = false }
[[bin]]
name = "said"
path = "$script_dir/said.rs"
[[bin]]
name = "e2e"
path = "$script_dir/e2e.rs"
[profile.dev]
debug = 0
MANIFEST
mkdir -p "$scratch/tmp"
export TMPDIR="$scratch/tmp"
export CARGO_BUILD_JOBS=2
rustfmt --edition 2024 --check "$script_dir/../"*.rs "$script_dir/"*.rs
cargo check --manifest-path "$scratch/harness/Cargo.toml"
cargo test --manifest-path "$scratch/harness/Cargo.toml" --lib
cargo clippy --manifest-path "$scratch/harness/Cargo.toml" --all-targets -- -D warnings
cargo build --manifest-path "$scratch/harness/Cargo.toml" --bins
