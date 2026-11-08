#!/bin/sh
# V8：owned 快照拷贝不变性 + 快照耗时中位数。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export ZIG="$ROOT/toolchain/zig/zig"
export GHOSTTY_SOURCE_DIR="$ROOT/vendor/ghostty-src"
cargo test --manifest-path "$ROOT/checks/v3_project/Cargo.toml" --test v8_snapshot -- --nocapture
