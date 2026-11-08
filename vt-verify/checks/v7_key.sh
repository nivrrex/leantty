#!/bin/sh
# V7：KeyEncoder 按键序列断言（普通 + 应用光标键模式）。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
: "${GHOSTTY_SOURCE_DIR:?GHOSTTY_SOURCE_DIR 环境变量未设置：请指向 ghostty 源码目录}"
export ZIG
export GHOSTTY_SOURCE_DIR
cargo test --manifest-path "$ROOT/checks/v3_project/Cargo.toml" --test v7_key
