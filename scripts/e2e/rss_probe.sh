#!/bin/sh
# rss_probe.sh —— 阶段 4K t2：RSS 增长的最小复现与映射分类取证（默认不进 run_all）。
#
# 用法：sh scripts/e2e/rss_probe.sh [分钟数，默认 6] [后端，默认 cpu]
#
# 负载（最小组合，来自 4J t6 的隔离实验结论）：每轮 `seq 1 800` + 每轮一次 1MB 真实
# 路径粘贴（xclip + 右键 → do_paste → paste_from_text）。release 二进制。
#
# 每次采样（每 30s）记录：
#   - /proc/<pid>/status 的 VmRSS / RssAnon / RssFile / VmData
#   - /proc/<pid>/smaps_rollup
#   - /proc/<pid>/smaps 按映射名归类汇总 Rss（[heap] / 匿名 mmap / 文件映射 / [stack]）
#   - Xvfb 的 RSS（判断增长是否其实在 X 服务器）
#   - app 自己的 5 秒 [perf] 帧日志（含 sb_rows/glyphs/fb_bytes/pending/wq_*）
#
# 安全：X11 操作经 guard.sh 的 x11()（强制 :99）；只操作自己启动的进程与窗口。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
MIN="${1:-6}"
BACKEND="${2:-cpu}"
OUT="$ROOT/scripts/e2e/out/rss"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
APP_BIN="$ROOT/target/release/app"
[ -x "$APP_BIN" ] || { echo "FATAL: 需要 release 二进制：cargo build --release"; exit 1; }

CSV="$OUT/rss-$BACKEND.csv"
LOG="$OUT/app-$BACKEND.log"
CLIP="$OUT/clip1mb.txt"
[ -s "$CLIP" ] || awk 'BEGIN { for (i = 0; i < 20000; i++) printf "rss_probe_line_%06d_xxxxxxxxxxxx\n", i }' >"$CLIP"

for p in $(e2e_stale_pids); do kill_own "$p"; done
sleep 0.5
e2e_start_xvfb "$OUT/xvfb.log" || exit 1
trap 'kill_own "${APP_PID:-}"; kill_own "${CLIP_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"' EXIT
e2e_start_wm "$OUT/wm.log"

CFG="$OUT/cfg-$BACKEND.toml"
printf '[renderer]\nbackend = "%s"\n[window]\ndimensions = { columns = 120, lines = 30 }\n' "$BACKEND" >"$CFG"

# classify：把 smaps 按映射类型汇总 Rss（kB）
classify() { # $1=pid → "heap=X anon=Y file=Z stack=W"
    awk '
        /^[0-9a-f]+-[0-9a-f]+ / { name=$0; sub(/^[^ ]+ [^ ]+ [^ ]+ [^ ]+ +/, "", name); if (name == "") name = "[anon]"; cur = name; next }
        /^Rss:/ { rss[cur] += $2 }
        END {
            for (n in rss) {
                if (n == "[heap]") h += rss[n]
                else if (n == "[stack]") s += rss[n]
                else if (n ~ /^\//) f += rss[n]
                else a += rss[n]
            }
            printf "heap=%d anon=%d file=%d stack=%d", h, a, f, s
        }
    ' "/proc/$1/smaps" 2>/dev/null
}

echo "t_s,vmsize_kb,vmrss_kb,rssanon_kb,rssfile_kb,vmdata_kb,allocator,xvfb_rss_kb" >"$CSV"
: >"$LOG"
APP_CONFIG="$CFG" APP_PERF_FRAMES=1 SHELL=/bin/sh "$APP_BIN" >"$LOG" 2>&1 &
APP_PID=$!

i=0
WID=""
while [ $i -lt 100 ]; do
    WID=$(x11 xdotool search --pid "$APP_PID" 2>/dev/null | head -1)
    [ -n "$WID" ] && break
    sleep 0.2
    i=$((i+1))
done
[ -z "$WID" ] && { echo "FAIL: 窗口未出现"; exit 1; }
x11 xdotool windowactivate "$WID" >/dev/null 2>&1
x11 xdotool windowfocus "$WID" >/dev/null 2>&1
GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
WX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
WY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)

XVFB_PID_LOCAL=$(pgrep -f "^Xvfb ${E2E_DISPLAY}" | head -1)

ELAPSED=0
NEXT=0
while [ "$ELAPSED" -lt $((MIN * 60)) ]; do
    x11 xdotool type --window "$WID" "seq 1 800"
    x11 xdotool key --window "$WID" Return
    x11 xclip -selection clipboard -i "$CLIP" >/dev/null 2>&1 &
    CLIP_PID=$!
    sleep 0.3
    x11 xdotool mousemove $(( ${WX:-0} + 120 )) $(( ${WY:-0} + 120 ))
    x11 xdotool click --delay 0 3
    sleep 0.7
    kill -TERM "$CLIP_PID" 2>/dev/null
    CLIP_PID=""
    ELAPSED=$((ELAPSED + 3))
    sleep 3
    if [ "$ELAPSED" -ge "$NEXT" ]; then
        VM=$(awk '/^VmSize:/ {print $2}' "/proc/$APP_PID/status" 2>/dev/null)
        RSS=$(awk '/^VmRSS:/ {print $2}' "/proc/$APP_PID/status" 2>/dev/null)
        ANON=$(awk '/^RssAnon:/ {print $2}' "/proc/$APP_PID/status" 2>/dev/null)
        FILE=$(awk '/^RssFile:/ {print $2}' "/proc/$APP_PID/status" 2>/dev/null)
        DATA=$(awk '/^VmData:/ {print $2}' "/proc/$APP_PID/status" 2>/dev/null)
        XR=$(ps -o rss= -p "${XVFB_PID_LOCAL:-0}" 2>/dev/null | tr -d ' ')
        CL=$(classify "$APP_PID")
        echo "$ELAPSED,${VM:-0},${RSS:-0},${ANON:-0},${FILE:-0},${DATA:-0},\"$CL\",${XR:-0}" >>"$CSV"
        echo "  [t=${ELAPSED}s] RSS=${RSS}KB anon=${ANON} file=${FILE} data=${DATA} | $CL | xvfb=${XR}KB"
        NEXT=$((NEXT + 30))
    fi
    if ! kill -0 "$APP_PID" 2>/dev/null; then
        echo "FAIL: app 提前退出（t=${ELAPSED}s）"
        break
    fi
done

# app 侧结构观测（5 秒帧日志里的最后一条）
echo "--- 最后一次 [perf] 帧日志（含 sb_rows/glyphs/fb_bytes/pending/wq_*）："
grep "frames n=" "$LOG" | tail -1
echo "--- 采集表：$CSV"

kill -TERM "$APP_PID" 2>/dev/null
W=0
while kill -0 "$APP_PID" 2>/dev/null && [ "$W" -lt 20 ]; do sleep 0.5; W=$((W + 1)); done
kill -KILL "$APP_PID" 2>/dev/null
wait "$APP_PID" 2>/dev/null
APP_PID=""
