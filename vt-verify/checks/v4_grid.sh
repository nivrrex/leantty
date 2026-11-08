#!/bin/sh
# V4：网格断言测试（a-f 六项输入）。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO_ROOT=$(cd "$ROOT/.." && pwd)
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
: "${GHOSTTY_SOURCE_DIR:?GHOSTTY_SOURCE_DIR 环境变量未设置：请指向 ghostty 源码目录}"
export ZIG
export GHOSTTY_SOURCE_DIR
cargo test --manifest-path "$ROOT/checks/v3_project/Cargo.toml" --test v4_grid
