#!/bin/sh
# :0 真机 GPU 冒烟（阶段 4B t7）——**默认不运行**，仅在显式授权时执行：
#   ALLOW_DISPLAY0=yes sh scripts/e2e/gpu_smoke_display0.sh
#
# 安全约束（与本仓库 e2e 规则一致的部分在此放宽，理由见下）：
#   - 只启动 app 自身**一个**窗口；不向任何其他窗口发送事件（不使用 xdotool/wmctrl）；
#   - 不触碰窗口管理器，不按名杀进程（只对自己启动的 PID 发 SIGTERM 并 wait）；
#   - 仅用只读的 `xwininfo` 定位自身窗口、只用 `xwd` 抓自身窗口一张截图；
#   - DISPLAY 从环境继承（用户当前会话，通常是 :0），脚本内不修改 DISPLAY。
#
# 与 scripts/e2e/lint_scripts.sh 的关系：本脚本是任务书明确授权的"一次性 :0 冒烟"，
# 因此 lint 对它放行"裸 X11 调用"与"非 :99 DISPLAY"两条规则（仍禁止 pkill/killall）。
# 其余 e2e 脚本（run_all/run_gpu_round 等）继续强制 DISPLAY=:99。

set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
LEANTTY_BIN="${LEANTTY_BIN:-$ROOT/target/release/leantty}"
OUT="$ROOT/scripts/e2e/out"
mkdir -p "$OUT"

if [ "${ALLOW_DISPLAY0:-no}" != "yes" ]; then
    echo "SKIP gpu_smoke_display0（需 ALLOW_DISPLAY0=yes 才在 :0 上运行）"
    exit 0
fi

printf '[gpu_smoke] 开始，当前显示：%s\n' "${DISPLAY:-<未设置>}"
if [ -z "${DISPLAY:-}" ]; then
    echo "FAIL gpu_smoke_display0（DISPLAY 未设置）"
    exit 1
fi
case "${DISPLAY}" in
    :99)
        echo "注意：当前 DISPLAY 是 :99（e2e 虚拟显示），本次冒烟本应在 :0 真机进行"
        ;;
esac

CFG="$OUT/gpu-smoke-display0.toml"
CAT_LOG="$OUT/gpu-smoke-display0.log"
SHOT="$OUT/gpu-smoke-display0.png"
cat >"$CFG" <<'TOML'
[window]
dimensions = { columns = 100, lines = 30 }
[renderer]
backend = "gpu"
TOML

: >"$CAT_LOG"
LEANTTY_CONFIG="$CFG" LEANTTY_PERF_FRAMES=1 SHELL=/bin/sh "$LEANTTY_BIN" >>"$CAT_LOG" 2>&1 &
LEANTTY_PID=$!
# 只清理自己启动的进程：等待其退出，必要时按 PID 发送信号。
cleanup() {
    if [ -n "${LEANTTY_PID:-}" ] && kill -0 "$LEANTTY_PID" 2>/dev/null; then
        kill -TERM "$LEANTTY_PID" 2>/dev/null
        wait "$LEANTTY_PID" 2>/dev/null
    fi
}
trap 'cleanup; exit 1' INT TERM

# 等窗口出现：只读查询自身 PID 对应的窗口（xwininfo 不修改任何状态）。
WID=""
i=0
while [ $i -lt 40 ]; do
    # 用 xwininfo 列出窗口树，按标题（app 的窗口标题为二进制名）找自身窗口。
    WID=$(xwininfo -root -tree 2>/dev/null | grep -m1 -F '"app"' | sed -n 's/^ *\(0x[0-9a-f]*\).*/\1/p')
    [ -n "$WID" ] && break
    sleep 0.25
    i=$((i+1))
done
if [ -z "$WID" ]; then
    echo "FAIL gpu_smoke_display0（未找到自身窗口）"
    cleanup
    exit 1
fi
echo "[gpu_smoke] 自身窗口 id=$WID"

# 让 GPU 后端多跑几帧，再抓自身窗口截图（xwd 只读）。
sleep 3
XWD_BIN=$(command -v xwd 2>/dev/null || true)
if [ -n "$XWD_BIN" ]; then
    "$XWD_BIN" -id "$WID" -out "$SHOT" >/dev/null 2>&1 && echo "[gpu_smoke] 截图已保存：$SHOT"
else
    echo "[gpu_smoke] 未安装 xwd，跳过截图"
fi

cleanup
LEANTTY_PID=""

FRAMES_LINE=$(grep -E "\[perf\] frames n=" "$CAT_LOG" | tail -1)
GPU_LINE=$(grep -E "gpu_init_ms|backend=gpu" "$CAT_LOG" | head -2 | tr '\n' ' ')
FIRST=$(grep -oE "first_frame_ms=[0-9]+" "$CAT_LOG" | head -1)

printf '[gpu_smoke] 结束，当前显示：%s\n' "${DISPLAY:-<未设置>}"
echo "[gpu_smoke] $GPU_LINE $FIRST"
if [ -n "$FRAMES_LINE" ]; then
    echo "[gpu_smoke] present 帧统计：$FRAMES_LINE"
else
    echo "[gpu_smoke] 未采到 frames 行（运行时间不足 50 帧或渲染失败）"
fi

if grep -q "backend=gpu" "$CAT_LOG" && [ -n "$FIRST" ]; then
    echo "PASS gpu_smoke_display0"
    exit 0
fi
echo "FAIL gpu_smoke_display0（未确认 GPU 后端与首帧）"
exit 1
