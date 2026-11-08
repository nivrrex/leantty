#!/bin/sh
# V1：绑定库 vendored 构建（首次/联网路径）。可重复运行。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export ZIG="$ROOT/toolchain/zig/zig"
cd "$ROOT/vendor/libghostty-rs"
cargo build --workspace
echo "V1 PASS: cargo build --workspace OK"
