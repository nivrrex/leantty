#!/bin/sh
# V2b：离线构建路径。GHOSTTY_SOURCE_DIR + GHOSTTY_ZIG_SYSTEM_DIR + cargo --offline + 网络不可达。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO_ROOT=$(cd "$ROOT/.." && pwd)
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
: "${GHOSTTY_SOURCE_DIR:?GHOSTTY_SOURCE_DIR 环境变量未设置：请指向 ghostty 源码目录}"
: "${GHOSTTY_ZIG_SYSTEM_DIR:?GHOSTTY_ZIG_SYSTEM_DIR 环境变量未设置：请指向 zig system 目录}"
export ZIG
export GHOSTTY_SOURCE_DIR
export GHOSTTY_ZIG_SYSTEM_DIR
cd "$REPO_ROOT/vendor/libghostty-rs"
cargo clean
http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 no_proxy= \
    cargo build --workspace --offline
echo "V2b PASS: 离线构建成功"
