#!/bin/sh
# V6：portable-pty 起 /bin/sh，后台线程读 PTY 经 channel 送入 Terminal，
# 写入 echo 你好 断言网格；再 resize 断言行列变化。Terminal 仅主线程访问。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
: "${GHOSTTY_SOURCE_DIR:?GHOSTTY_SOURCE_DIR 环境变量未设置：请指向 ghostty 源码目录}"
cd "$ROOT/checks/v3_project"
GHOSTTY_SOURCE_DIR="$GHOSTTY_SOURCE_DIR" ZIG="$ZIG" \
    cargo test --test v6_pty -- --nocapture
echo "V6 PASS: v6_pty_echo_cn_and_resize"
