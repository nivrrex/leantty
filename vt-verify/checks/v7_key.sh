#!/bin/sh
# V7：KeyEncoder 按键序列断言（普通 + 应用光标键模式）。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export ZIG="$ROOT/toolchain/zig/zig"
export GHOSTTY_SOURCE_DIR="$ROOT/vendor/ghostty-src"
cargo test --manifest-path "$ROOT/checks/v3_project/Cargo.toml" --test v7_key
