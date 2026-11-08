#!/bin/sh
# V3：下游 cargo 工程（path 依赖）编译链接 + ldd 检查无 Zig 运行时依赖。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT/checks/v3_project"
GHOSTTY_SOURCE_DIR="$ROOT/vendor/ghostty-src" ZIG="$ROOT/toolchain/zig/zig" \
    cargo build
BIN=target/debug/vt-downstream
"$BIN" | grep -q "downstream link ok"
echo "=== ldd 动态依赖清单 ==="
ldd "$BIN" | tee "$ROOT/v3_ldd.txt"
if ldd "$BIN" | grep -i zig; then
    echo "V3 FAIL: 发现 Zig 相关运行时依赖" >&2
    exit 1
fi
echo "V3 PASS: 链接成功，运行 OK，无 Zig 运行时依赖"
