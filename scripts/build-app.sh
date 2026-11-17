#!/bin/sh
# 构建 app release（可选直接运行）。全部变量来自环境，未设置必填项报清晰错误；
# 仓库内不出现绝对路径。
#
# 必填（构建契约，见 README）：
#   ZIG                     zig 可执行文件路径
#   GHOSTTY_SOURCE_DIR      ghostty 源码目录
#   GHOSTTY_ZIG_SYSTEM_DIR  zig system 目录
# 可选（默认值）：
#   LEANTTY_COLS=80  LEANTTY_ROWS=24        网格行列（运行期，解析失败回落 80x24）
#   LEANTTY_FONT_FAMILY                字体 family 覆盖（运行期；命令行 --font 优先）
#   LEANTTY_FONT_SIZE                  字号 pt 覆盖（运行期；命令行 --font-size 优先）
#   LEANTTY_SHELL                      运行 shell（未设置回落平台默认：Linux /bin/sh、Windows cmd.exe）
#   RUN=1                          构建后直接运行 target/release/leantty
#
# 示例：
#   ZIG=... GHOSTTY_SOURCE_DIR=... GHOSTTY_ZIG_SYSTEM_DIR=... sh scripts/build-app.sh
#   ZIG=... GHOSTTY_SOURCE_DIR=... GHOSTTY_ZIG_SYSTEM_DIR=... \
#     RUN=1 LEANTTY_COLS=120 LEANTTY_ROWS=40 LEANTTY_SHELL=/bin/bash sh scripts/build-app.sh
set -e
: "${ZIG:?ZIG 环境变量未设置：请指向 zig 可执行文件}"
: "${GHOSTTY_SOURCE_DIR:?GHOSTTY_SOURCE_DIR 环境变量未设置：请指向 ghostty 源码目录}"
: "${GHOSTTY_ZIG_SYSTEM_DIR:?GHOSTTY_ZIG_SYSTEM_DIR 环境变量未设置：请指向 zig system 目录}"
export ZIG GHOSTTY_SOURCE_DIR GHOSTTY_ZIG_SYSTEM_DIR
: "${LEANTTY_COLS:=80}"
: "${LEANTTY_ROWS:=24}"
: "${LEANTTY_SHELL:=}"
: "${RUN:=0}"
cargo build --release -p leantty-app
if [ "$RUN" = "1" ]; then
    # LEANTTY_COLS/LEANTTY_ROWS/LEANTTY_SHELL/LEANTTY_FONT_* 由用户环境透传（未设置时 app 内回落默认）。
    if [ -n "$LEANTTY_SHELL" ]; then
        exec env SHELL="$LEANTTY_SHELL" LEANTTY_COLS="$LEANTTY_COLS" LEANTTY_ROWS="$LEANTTY_ROWS" \
            ./target/release/leantty
    else
        exec env LEANTTY_COLS="$LEANTTY_COLS" LEANTTY_ROWS="$LEANTTY_ROWS" \
            ./target/release/leantty
    fi
fi
echo "构建完成：target/release/leantty"
echo "运行：RUN=1 $0 （或 LEANTTY_COLS=.. LEANTTY_ROWS=.. [SHELL=..] ./target/release/leantty [--font FAMILY] [--font-size PT]）"
