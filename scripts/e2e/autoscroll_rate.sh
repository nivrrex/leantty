#!/bin/sh
# autoscroll_rate.sh —— 阶段 4U t2：拖选自动滚动的推进速率是否与帧耗时相关。
#
# 用法：sh scripts/e2e/autoscroll_rate.sh <cpu|gpu|lavapipe> [次数，默认 3]
#
# 方法：输出 2000 行 → 滚到顶部 → 在窗口内按下左键并把指针移到窗口下缘之外并**静止** →
# 持续 3 秒 → 松开。用滚动条拇指位置换算视口偏移（公式见下），对比起止偏移得到
# "3 秒推进的行数"。同时统计该轮的帧间隔（[perf] frames 行的 interval_ms）作为对照。
#
# 拇指→偏移换算：offset ≈ round(thumb_y / (ph - thumb_h) * sb_rows)（与 app 的
# scrollbar_geometry 互逆；本脚本单独用一段 dump 校验过，见 PHASE4U-LINUX.md §2）。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
MODE="${1:-cpu}"
RUNS="${2:-3}"
OUT="$ROOT/scripts/e2e/out/asrate"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
LEANTTY_BIN="$ROOT/target/release/leantty"
[ -x "$LEANTTY_BIN" ] || { echo "FATAL: 需要 release 二进制"; exit 1; }

BACKEND=gpu
case "$MODE" in
    cpu) BACKEND=cpu ;;
    gpu) BACKEND=gpu ;;
    lavapipe)
        BACKEND=gpu
        export LEANTTY_GPU_ALLOW_SOFTWARE=1
        export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
        ;;
    *) echo "用法: $0 <cpu|gpu|lavapipe> [次数]"; exit 2 ;;
esac

thumb_y(){ # 打印拇指顶部的 y 像素
    _w=$(identify -format "%w" "$1" 2>/dev/null) || { echo -1; return; }
    _h=$(identify -format "%h" "$1" 2>/dev/null) || { echo -1; return; }
    convert "$1" -crop "8x${_h}+$((_w-8))+0" +repage -colorspace Gray -depth 8 txt:- 2>/dev/null \
        | sed -n 's/^\([0-9]*\),\([0-9]*\):.*gray(\([0-9]*\)).*/\2 \3/p' \
        | awk -v h="$_h" '{ if ($2+0 > 200 && (m == "" || $1+0 < m)) m=$1+0 } END { if (m == "") print -1; else print m }'
}
thumb_h_px(){
    _w=$(identify -format "%w" "$1" 2>/dev/null)
    _h=$(identify -format "%h" "$1" 2>/dev/null)
    convert "$1" -crop "8x${_h}+$((_w-8))+0" +repage -colorspace Gray -depth 8 txt:- 2>/dev/null \
        | sed -n 's/^\([0-9]*\),\([0-9]*\):.*gray(\([0-9]*\)).*/\2 \3/p' \
        | awk '{ if ($2+0 > 200) n++ } END { print n+0 }'
}

echo "=== 自动滚动速率：mode=$MODE（backend=$BACKEND），每轮 3 秒 ==="
for k in $(seq 1 "$RUNS"); do
    for p in $(e2e_stale_pids); do kill_own "$p"; done
    sleep 0.3
    e2e_start_xvfb "$OUT/xvfb-$MODE.log" || exit 1
    e2e_start_wm "$OUT/wm-$MODE.log"
    printf '[renderer]\nbackend = "%s"\n[window]\ndimensions = { columns = 80, lines = 24 }\n' "$BACKEND" >"$OUT/cfg-$MODE.toml"
    LOG="$OUT/app-$MODE-$k.log"
    LEANTTY_CONFIG="$OUT/cfg-$MODE.toml" LEANTTY_PERF_FRAMES=1 SHELL=/bin/sh "$LEANTTY_BIN" >"$LOG" 2>&1 &
    LEANTTY_PID=$!
    i=0
    WID=""
    while [ $i -lt 120 ]; do
        WID=$(x11 xdotool search --pid "$LEANTTY_PID" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.2
        i=$((i + 1))
    done
    [ -z "$WID" ] && { echo "run$k: 窗口未出现"; kill_own "$LEANTTY_PID"; kill_own "$XVFB_PID"; kill_own "$WM_PID"; continue; }
    sleep 1.0
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
    GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2)
    GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    x11 xdotool type --window "$WID" "seq 1 2000"
    x11 xdotool key --window "$WID" Return
    sleep 3
    # 滚到顶部（多按几次，clamp 在顶）
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} / 2 )) $(( ${GY:-0} + ${GH:-480} / 2 ))
    n=0; while [ $n -lt 300 ]; do x11 xdotool click 4; n=$((n+1)); done
    sleep 1.0
    x11 import -window "$WID" "$OUT/$MODE-$k-before.png" 2>/dev/null
    TY1=$(thumb_y "$OUT/$MODE-$k-before.png")
    TH=$(thumb_h_px "$OUT/$MODE-$k-before.png")
    PH=$(identify -format "%h" "$OUT/$MODE-$k-before.png" 2>/dev/null)
    # 按下并把指针移到窗口下缘之外，静止 3 秒
    x11 xdotool mousemove $(( ${GX:-0} + 60 )) $(( ${GY:-0} + 60 ))
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove $(( ${GX:-0} + 60 )) $(( ${GY:-0} + ${GH:-480} + 60 ))
    sleep 3.0
    x11 import -window "$WID" "$OUT/$MODE-$k-after.png" 2>/dev/null
    TY2=$(thumb_y "$OUT/$MODE-$k-after.png")
    x11 xdotool mouseup 1
    # 帧间隔（本轮的 50 帧统计行中位）
    IV=$(grep -oE "interval_ms=[0-9]+" "$LOG" | cut -d= -f2 | sort -n | tail -1)
    # 偏移换算：offset = ty/(ph-th)*sb_rows，sb_rows ≈ 2000-24
    SB=1976
    O1=$(awk -v ty="$TY1" -v th="$TH" -v ph="$PH" -v sb="$SB" 'BEGIN{ d=ph-th; if (d<=0) {print -1; exit} printf "%.0f", ty/d*sb }')
    O2=$(awk -v ty="$TY2" -v th="$TH" -v ph="$PH" -v sb="$SB" 'BEGIN{ d=ph-th; if (d<=0) {print -1; exit} printf "%.0f", ty/d*sb }')
    ADV=$(( ${O2:-0} - ${O1:-0} ))
    echo "run$k: offset ${O1:-NA} → ${O2:-NA}，3 秒推进 ${ADV} 行（≈$(( ADV / 3 )) 行/秒），最大帧间隔=${IV:-NA}ms"
    kill -TERM "$LEANTTY_PID" 2>/dev/null
    wait "$LEANTTY_PID" 2>/dev/null
    LEANTTY_PID=""
    kill_own "${XVFB_PID:-}"
    kill_own "${WM_PID:-}"
    sleep 0.4
done
