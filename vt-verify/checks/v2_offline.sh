#!/bin/sh
# V2b：离线构建路径。GHOSTTY_SOURCE_DIR + GHOSTTY_ZIG_SYSTEM_DIR + cargo --offline + 网络不可达。
# 前置：zig-system/ 已按 PINS.md 方法构造（p/ 解压 + 目录复制）。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export ZIG="$ROOT/toolchain/zig/zig"
export GHOSTTY_SOURCE_DIR="$ROOT/vendor/ghostty-src"
export GHOSTTY_ZIG_SYSTEM_DIR="$ROOT/zig-system"
cd "$ROOT/vendor/libghostty-rs"
cargo clean
http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 no_proxy= \
    cargo build --workspace --offline
echo "V2b PASS: 离线构建成功"
