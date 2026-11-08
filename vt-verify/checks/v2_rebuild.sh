#!/bin/sh
# V2a：清理构建缓存后重建，确认可重复。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export ZIG="$ROOT/toolchain/zig/zig"
cd "$ROOT/vendor/libghostty-rs"
cargo clean
cargo build --workspace
echo "V2a PASS: 清理后重建成功"
