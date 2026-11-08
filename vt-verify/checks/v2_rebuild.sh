#!/bin/sh
# V2a：清理构建缓存后重建，确认可重复。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO_ROOT=$(cd "$ROOT/.." && pwd)
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
export ZIG
cd "$REPO_ROOT/vendor/libghostty-rs"
cargo clean
cargo build --workspace
echo "V2a PASS: 清理后重建成功"
