#!/bin/sh
# idle_repaint_matrix.sh —— 阶段 4R t2：GPU 空闲跳过（4Q t3）的回归矩阵。
#
# 用法：sh scripts/e2e/idle_repaint_matrix.sh [nvidia|lavapipe]
#
# 目的：4Q t3 让 GPU 后端在"快照干净 + 无 urgent + 滚动条几何未变"时跳过整帧渲染，
# 空闲帧数从 205/5s 降到 0。风险是**不改变 VT 快照却可见的变化**被漏画。本脚本对每一项
# 触发后 1 秒内截图与触发前比对（AE 像素差），非 0 即说明确实发生了重绘。
#
# 口径：截图对比用 ImageMagick 的 AE（绝对误差像素数），阈值见每项；"非目标色变黑再数白"
# 用于需要精确统计某色像素的项目（这里未用到，仅 AE 足够判定"是否重绘"）。
#
# 安全：X11 操作经 guard.sh 的 x11()（:99）；只操作自己启动的进程与窗口。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
MODE="${1:-nvidia}"
OUT="$ROOT/scripts/e2e/out/matrix"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
APP_BIN="$ROOT/target/release/app"
[ -x "$APP_BIN" ] || { echo "FATAL: 需要 release 二进制"; exit 1; }

# 适配器选择
if [ "$MODE" = "lavapipe" ]; then
    export APP_GPU_ALLOW_SOFTWARE=1
    export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
fi

for p in $(e2e_stale_pids); do kill_own "$p"; done
sleep 0.4
e2e_start_xvfb "$OUT/xvfb.log" || exit 1
trap 'kill_own "${APP_PID:-}"; kill_own "${CLIP_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"' EXIT
e2e_start_wm "$OUT/wm.log"

CFG="$OUT/cfg-$MODE.toml"
printf '[renderer]\nbackend = "gpu"\n[window]\ndimensions = { columns = 80, lines = 24 }\n' >"$CFG"
LOG="$OUT/app-$MODE.log"
APP_CONFIG="$CFG" APP_PERF_FRAMES=1 SHELL=/bin/sh "$APP_BIN" >"$LOG" 2>&1 &
APP_PID=$!
i=0
WID=""
while [ $i -lt 120 ]; do
    WID=$(x11 xdotool search --pid "$APP_PID" 2>/dev/null | head -1)
    [ -n "$WID" ] && break
    sleep 0.2
    i=$((i + 1))
done
[ -z "$WID" ] && { echo "FAIL: 窗口未出现（$MODE）"; exit 1; }
sleep 1.2
x11 xdotool windowactivate "$WID" >/dev/null 2>&1
x11 xdotool windowfocus "$WID" >/dev/null 2>&1
sleep 0.5
GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2)
GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
CX=$(( ${GX:-0} + ${GW:-640} / 2 ))
CY=$(( ${GY:-0} + ${GH:-384} / 2 ))

# 先灌一些内容（含 CJK/颜色/多行）
x11 xdotool type --window "$WID" "printf 'alpha beta gamma\\n第二行内容\\n\\033[31mRED\\033[0m \\033[1mBOLD\\033[0m\\n'"
x11 xdotool key --window "$WID" Return
sleep 1.2
x11 xdotool type --window "$WID" "seq 1 80"
x11 xdotool key --window "$WID" Return
sleep 2.0

snap() { x11 import -window "$WID" "$OUT/$MODE-$1.png" 2>/dev/null; }
ae() { compare -metric AE "$OUT/$MODE-$1.png" "$OUT/$MODE-$2.png" null: 2>&1 | head -1; }
row() { # $1=项 $2=AE $3=判定
    printf '  %-22s AE=%-8s %s\n' "$1" "${2:-NA}" "${3:-}"
}

echo "=== 回归矩阵（$MODE）==="
snap base

# 1) 选区：拖选 → 高亮；再点空白清除
x11 xdotool mousemove $(( ${GX:-0} + 20 )) $(( ${GY:-0} + 30 ))
x11 xdotool mousedown 1
x11 xdotool mousemove $(( ${GX:-0} + 300 )) $(( ${GY:-0} + 80 ))
sleep 0.3
x11 xdotool mouseup 1
sleep 1.0
snap sel
A=$(ae base sel)
row "1a 选区高亮" "$A" "$([ "${A:-0}" -gt 50 ] && echo OK || echo FAIL)"
x11 xdotool mousemove $CX $CY
x11 xdotool click --delay 0 1
sleep 1.0
snap selcleared
A=$(ae sel selcleared)
row "1b 清除选区" "$A" "$([ "${A:-0}" -gt 50 ] && echo OK || echo FAIL)"

# 3) 滚动条：滚轮上滚 → 回底（拇指位置变化）
x11 xdotool mousemove $CX $CY
n=0; while [ $n -lt 6 ]; do x11 xdotool click 4; n=$((n+1)); done
sleep 1.0
snap scrollup
A=$(ae base scrollup)
row "3a 滚轮上滚" "$A" "$([ "${A:-0}" -gt 50 ] && echo OK || echo FAIL)"
n=0; while [ $n -lt 10 ]; do x11 xdotool click 5; n=$((n+1)); done
sleep 1.0
snap scrolldown
A=$(ae scrollup scrolldown)
row "3b 滚回底部" "$A" "$([ "${A:-0}" -gt 50 ] && echo OK || echo FAIL)"

# 6) 动态颜色：OSC 11 改背景 → OSC 111 复位
x11 xdotool type --window "$WID" "printf '\\033]11;#203040\\007'"
x11 xdotool key --window "$WID" Return
sleep 1.0
snap osc11
A=$(ae scrolldown osc11)
row "6a OSC11 改背景" "$A" "$([ "${A:-0}" -gt 50 ] && echo OK || echo FAIL)"
x11 xdotool type --window "$WID" "printf '\\033]111\\007'"
x11 xdotool key --window "$WID" Return
sleep 1.0
snap osc111
A=$(ae osc11 osc111)
row "6b OSC111 复位" "$A" "$([ "${A:-0}" -gt 50 ] && echo OK || echo FAIL)"

# 7) 光标：隐藏/显示 + 形状（DECSCUSR）
x11 xdotool type --window "$WID" "printf '\\033[?25l'"
x11 xdotool key --window "$WID" Return
sleep 1.0
snap cursorhidden
A=$(ae osc111 cursorhidden)
row "7a 光标隐藏" "$A" "$([ "${A:-0}" -gt 20 ] && echo OK || echo FAIL)"
x11 xdotool type --window "$WID" "printf '\\033[?25h\\033[4 q'"
x11 xdotool key --window "$WID" Return
sleep 1.0
snap cursorbar
A=$(ae cursorhidden cursorbar)
row "7b 光标显示+下划线" "$A" "$([ "${A:-0}" -gt 20 ] && echo OK || echo FAIL)"

# 8) 窗口缩放
x11 xdotool windowsize "$WID" 700 420
sleep 1.0
snap resized
A=$(ae cursorbar resized)
row "8 窗口缩放" "$A" "$([ "${A:-0}" -gt 1000 ] && echo OK || echo FAIL)"

# 空闲检查：最后两个 5 秒窗口应 ≤3 帧
sleep 11
# 只取最后一个窗口（缩放收尾会占用前一个窗口的前几十帧）
IDLE=$(grep -oE "frames_window=[0-9]+" "$LOG" | tail -1 | cut -d= -f2)
row "空闲窗口帧数" "${IDLE:-NA}" "$([ "${IDLE:-999}" -le 3 ] && echo OK || echo FAIL)"

echo "--- 说明：项 2（预编辑）需 debug 二进制（APP_TEST_PREEDIT 为 debug 专有），由 case30 覆盖；"
echo "    项 4（焦点光标样式）本实现无此行为；项 5（Expose）由 case31 覆盖；项 9（反色/粗体等纯快照变化）"
echo "    由既有 case26/27/34/35/36 与 gpu_parity 覆盖。"
kill -TERM "$APP_PID" 2>/dev/null
wait "$APP_PID" 2>/dev/null
APP_PID=""
