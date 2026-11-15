#!/bin/sh
# font_mem_probe.sh —— 阶段 4N t2a：字体内存的 RSS 时间线对照（默认不进 run_all）。
#
# 用法：sh scripts/e2e/font_mem_probe.sh [秒数，默认 60]
#
# 三种配置（其余相同：release、cpu 后端、空闲）：
#   1) nofb    ：font.fallback = []（无 CJK 回退）
#   2) cjk     ：font.fallback = 黑体（系统 fontconfig 解析到 Noto Sans CJK）
#   3) cjkjp   ：font.fallback = Noto Sans CJK JP（同一个 .ttc 的另一面）
#
# 采样：启动后每 50ms 采一次直到 3s（抓首帧与后台字体到位），之后每 5s 到总时长；
# 每次记录 VmRSS / RssAnon / smaps_rollup 的 Rss，并把 app 的 [perf] 时间点
# （first_frame_ms、bg_fonts_ms）与采样时刻对齐。
#
# 安全：X11 操作经 guard.sh 的 x11()；只操作自己启动的进程与窗口。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DUR="${1:-60}"
OUT="$ROOT/scripts/e2e/out/fontmem"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
APP_BIN="$ROOT/target/release/app"
[ -x "$APP_BIN" ] || { echo "FATAL: 需要 release 二进制"; exit 1; }

for p in $(e2e_stale_pids); do kill_own "$p"; done
sleep 0.4
e2e_start_xvfb "$OUT/xvfb.log" || exit 1
trap 'kill_own "${APP_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"' EXIT
e2e_start_wm "$OUT/wm.log"

sample() { # $1=pid $2=标签 $3=已过毫秒
    _rss=$(awk '/^VmRSS:/ {print $2}' "/proc/$1/status" 2>/dev/null)
    _anon=$(awk '/^RssAnon:/ {print $2}' "/proc/$1/status" 2>/dev/null)
    _roll=$(awk '/^Rss:/ {print $2}' "/proc/$1/smaps_rollup" 2>/dev/null)
    echo "$3,${_rss:-0},${_anon:-0},${_roll:-0}"
}

run_case() { # $1=名字 $2=font 段内容
    NAME="$1"
    CFG="$OUT/$NAME.toml"
    CSV="$OUT/$NAME.csv"
    LOG="$OUT/$NAME.log"
    {
        echo '[renderer]'
        echo "backend = \"${BACKEND4N:-cpu}\"" 
        echo '[window]'
        echo 'dimensions = { columns = 80, lines = 24 }'
        echo '[font]'
        printf '%s\n' "$2"
    } >"$CFG"
    echo "t_ms,vmrss_kb,rssanon_kb,smaps_rss_kb" >"$CSV"
    APP_CONFIG="$CFG" APP_PERF_FRAMES=1 SHELL=/bin/sh "$APP_BIN" >"$LOG" 2>&1 &
    APP_PID=$!
    i=0
    WID=""
    while [ $i -lt 100 ]; do
        WID=$(x11 xdotool search --pid "$APP_PID" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.05
        i=$((i + 1))
    done
    # 窗口出现后开始密集采样（50ms × 60 = 3s）
    n=0
    while [ "$n" -lt 60 ]; do
        sample "$APP_PID" "$NAME" $((n * 50)) >>"$CSV"
        n=$((n + 1))
        sleep 0.05
    done
    # 之后每 5s 一次
    t=3000
    while [ "$t" -le $((DUR * 1000)) ]; do
        sleep 5
        t=$((t + 5000))
        sample "$APP_PID" "$NAME" "$t" >>"$CSV"
    done
    FINAL=$(awk -F, 'END {print $2}' "$CSV")
    BG=$(grep -oE "bg_fonts_ms=[0-9]+" "$LOG" | head -1 | cut -d= -f2)
    FF=$(grep -oE "first_frame_ms=[0-9]+" "$LOG" | head -1 | cut -d= -f2)
    echo "  [$NAME] 结束 RSS=${FINAL}KB  first_frame=${FF:-NA}ms  bg_fonts=${BG:-NA}ms"
    kill -TERM "$APP_PID" 2>/dev/null
    w=0
    while kill -0 "$APP_PID" 2>/dev/null && [ "$w" -lt 20 ]; do sleep 0.5; w=$((w + 1)); done
    kill -KILL "$APP_PID" 2>/dev/null
    wait "$APP_PID" 2>/dev/null
    APP_PID=""
    sleep 1
}

run_case nofb 'fallback = []'
run_case cjk 'fallback = [ { family = "黑体", style = "Regular" } ]'
run_case cjkjp 'fallback = [ { family = "Noto Sans CJK JP", style = "Regular" } ]'

echo "--- 汇总（结束时刻 RSS；差值相对 nofb）"
BASE=$(awk -F, 'END {print $2}' "$OUT/nofb.csv")
for n in nofb cjk cjkjp; do
    V=$(awk -F, 'END {print $2}' "$OUT/$n.csv")
    echo "  $n: ${V}KB（差 $((V - BASE))KB ≈ $(( (V - BASE) / 1024 ))MB）"
done
echo "--- 逐点表在各 $OUT/*.csv"
