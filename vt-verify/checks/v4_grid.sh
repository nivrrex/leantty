#!/bin/sh
# V4：网格断言测试（a-f 六项输入）。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export ZIG="$ROOT/toolchain/zig/zig"
export GHOSTTY_SOURCE_DIR="$ROOT/vendor/ghostty-src"
cargo test --manifest-path "$ROOT/checks/v3_project/Cargo.toml" --test v4_grid
