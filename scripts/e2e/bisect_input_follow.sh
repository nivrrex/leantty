#!/bin/sh
# bisect_input_follow.sh —— "滚到历史后键入是否回到底部"的判别用例（可跨历史 tag 运行）。
#
# 用法：sh scripts/e2e/bisect_input_follow.sh <app 二进制> [标签名]
#
# 判据（两种观测，优先文本、回退像素）：
#   1) 输出 300 行 → 截图 A（视口在底部）
#   2) 对自身窗口注入滚轮上滚 10 格 → 截图 B（应明显不同于 A，证明已离开底部）
#   3) 键入 `echo MARK_$((1+1))` 并回车 → 截图 C
#   4) 判定：若该版本支持 LEANTTY_DUMP_SNAPSHOT（debug 钩子），则以"视口网格里出现
#      MARK_2 行"为准（MARK_2 只会在回底后进入可视区）；否则回退到像素判据
#      "C 相对 B 的变化远大于 A 相对 B 的差异 ⇒ 视口移动过"。
#   两种观测都不依赖实现细节，可在历史 tag 上运行。
#
# 安全：所有 X11 操作经 guard.sh 的 x11()（强制 :99）；只操作自己启动的 app；
# 结束时只对自己记录的 PID 发 SIGTERM。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
LEANTTY_BIN="${1:?需要 app 二进制路径}"
LABEL="${2:-$(basename "$LEANTTY_BIN")}"
OUT="${BISECT_OUT:-$ROOT/scripts/e2e/out/bisect}"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
DISPLAY=:99
. "$ROOT/scripts/e2e/guard.sh"

for p in $(e2e_stale_pids); do kill_own "$p"; done
sleep 0.3
e2e_start_xvfb "$OUT/xvfb.log" || exit 2
trap 'kill_own "${LEANTTY_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"; exit 0' EXIT INT TERM
e2e_start_wm "$OUT/wm.log" || exit 2

CFG="$OUT/cfg-$LABEL.toml"
# 显式写 backend：历史 tag 里 backend 字段名可能不存在（4B 起才有），故只在配置里
# 写窗口尺寸，渲染后端交给默认值（历史 tag 上默认即 cpu）。
printf '[window]\ndimensions = { columns = 80, lines = 24 }\n' >"$CFG"
LOG="$OUT/app-$LABEL.log"
DUMP="$OUT/dump-$LABEL.txt"
rm -f "$DUMP"
LEANTTY_CONFIG="$CFG" LEANTTY_DUMP_SNAPSHOT="$DUMP" SHELL=/bin/sh "$LEANTTY_BIN" >"$LOG" 2>&1 &
LEANTTY_PID=$!

i=0
WID=""
while [ $i -lt 100 ]; do
    WID=$(x11 xdotool search --pid "$LEANTTY_PID" 2>/dev/null | head -1)
    [ -n "$WID" ] && break
    sleep 0.2
    i=$((i+1))
done
if [ -z "$WID" ]; then
    echo "VERDICT=$LABEL UNKNOWN (窗口未出现)"
    exit 0
fi
sleep 1.0
x11 xdotool windowactivate "$WID" >/dev/null 2>&1
x11 xdotool windowfocus "$WID" >/dev/null 2>&1
sleep 0.3

# 1) 大量输出
x11 xdotool type --window "$WID" "seq 1 300"
x11 xdotool key --window "$WID" Return
sleep 2.0
x11 import -window "$WID" "$OUT/$LABEL-A-bottom.png" 2>/dev/null

# 2) 滚轮上滚 10 格（真实指针 + 自身窗口中央）
GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2)
GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
x11 xdotool mousemove $(( ${GX:-0} + ${GW:-640} / 2 )) $(( ${GY:-0} + ${GH:-384} / 2 ))
sleep 0.2
n=0
while [ $n -lt 10 ]; do
    x11 xdotool click 4
    n=$((n+1))
done
sleep 1.0
x11 import -window "$WID" "$OUT/$LABEL-B-scrolled.png" 2>/dev/null

# 3) 键入并回车
x11 xdotool type --window "$WID" 'echo MARK_$((1+1))'
x11 xdotool key --window "$WID" Return
sleep 2.0
x11 import -window "$WID" "$OUT/$LABEL-C-after-input.png" 2>/dev/null

kill -TERM "$LEANTTY_PID" 2>/dev/null
wait "$LEANTTY_PID" 2>/dev/null
LEANTTY_PID=""

HAS_DUMP=0
[ -s "$DUMP" ] && HAS_DUMP=1
MARK_VISIBLE=0
if [ "$HAS_DUMP" = "1" ] && grep -q "MARK_2" "$DUMP" 2>/dev/null; then MARK_VISIBLE=1; fi
AE_AB=$(compare -metric AE "$OUT/$LABEL-A-bottom.png" "$OUT/$LABEL-B-scrolled.png" null: 2>&1 | head -1)
AE_AC=$(compare -metric AE "$OUT/$LABEL-A-bottom.png" "$OUT/$LABEL-C-after-input.png" null: 2>&1 | head -1)
AE_BC=$(compare -metric AE "$OUT/$LABEL-B-scrolled.png" "$OUT/$LABEL-C-after-input.png" null: 2>&1 | head -1)

VERDICT=UNKNOWN
if [ "${AE_AB:-0}" -le 0 ]; then
    VERDICT="UNKNOWN(滚轮未生效)"
elif [ "$HAS_DUMP" = "1" ]; then
    # 文本判据：MARK_2 出现在视口网格 = 回底
    if [ "$MARK_VISIBLE" = "1" ]; then
        VERDICT="PASS(回底,dump)"
    else
        VERDICT="FAIL(停在历史,dump)"
    fi
elif [ "${AE_BC:-0}" -gt $(( ${AE_AB:-0} * 2 )) ]; then
    # 像素回退判据：键入后画面相对"上滚视图"变化很大 ⇒ 视口移动过
    VERDICT="PASS(回底,pixel)"
else
    VERDICT="FAIL(停在历史,pixel)"
fi
echo "VERDICT=$LABEL $VERDICT dump=$HAS_DUMP mark=$MARK_VISIBLE AE(A,B)=$AE_AB AE(A,C)=$AE_AC AE(B,C)=$AE_BC"
