#!/bin/sh
# V1：绑定库 vendored 构建（首次/联网路径）。可重复运行。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO_ROOT=$(cd "$ROOT/.." && pwd)
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
export ZIG
cd "$REPO_ROOT/vendor/libghostty-rs"
cargo build --workspace
echo "V1 PASS: cargo build --workspace OK"
