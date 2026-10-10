#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
export CARGO_BUILD_JOBS=2
cargo build --locked --manifest-path "$root/Cargo.toml"
run="$HOME/.cache/voice-models/bench-runs/ci-$(date +%s)-$$"
"$root/../target/debug/voice-bench" bench "$root/clips.jsonl" --ci --output "$run"
python3 "$root/scripts/check-ci.py" "$run/summary.json"
