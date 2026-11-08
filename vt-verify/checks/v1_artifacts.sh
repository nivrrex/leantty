#!/bin/sh
# V1：构建产物证据——libghostty-vt.a 路径/大小 + 头文件清单。
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(ls -d "$ROOT"/vendor/libghostty-rs/target/debug/build/libghostty-vt-sys-*/out/ghostty-install 2>/dev/null | head -1)
[ -n "$OUT" ] || { echo "V1 FAIL: 未找到 ghostty-install 产物目录（请先运行 v1_build.sh）"; exit 1; }
A="$OUT/lib/libghostty-vt.a"
[ -f "$A" ] || { echo "V1 FAIL: $A 不存在"; exit 1; }
SIZE=$(stat -c%s "$A")
[ "$SIZE" -gt 1000000 ] || { echo "V1 FAIL: libghostty-vt.a 仅 $SIZE 字节，异常偏小"; exit 1; }
echo "libghostty-vt.a: $A"
echo "大小: $SIZE 字节 ($((SIZE/1024/1024)) MiB)"
echo "头文件清单:"
find "$OUT/include" -name '*.h' | sort | tee "$ROOT/checks/v1_headers.txt"
N=$(wc -l < "$ROOT/checks/v1_headers.txt")
echo "头文件数量: $N"
[ "$N" -ge 1 ] || { echo "V1 FAIL: 无头文件"; exit 1; }
echo "V1 PASS: 静态库与头文件证据已落盘 checks/v1_headers.txt"
