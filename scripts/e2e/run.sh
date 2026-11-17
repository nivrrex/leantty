#!/bin/sh
# e2e 复现脚本（阶段 A）：Xvfb(:99) + app + xdotool 输入 + 截图 + 日志。
# 安全护栏（阶段 3B 任务 1）：经 guard.sh 强制 DISPLAY=:99，X11 调用统一走 x11 包装。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$ROOT/scripts/e2e/out"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
LEANTTY_BIN="$ROOT/target/release/leantty"

[ -x "$LEANTTY_BIN" ] || { echo "FATAL: $LEANTTY_BIN 不存在，请先 cargo build --release -p app"; exit 1; }
command -v Xvfb >/dev/null 2>&1 || { echo "FATAL: Xvfb 未安装"; exit 1; }
command -v xdotool >/dev/null 2>&1 || { echo "FATAL: xdotool 未安装"; exit 1; }

# 起 Xvfb（固定 :99，guard 保证不抢占）与窗口管理器
e2e_start_xvfb "$OUT/xvfb.log" || exit 1
trap 'kill_own "${LEANTTY_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"; exit 0' EXIT INT TERM
e2e_start_wm "$OUT/openbox.log"

# 起 app（SHELL=/bin/sh）
SHELL=/bin/sh "$LEANTTY_BIN" >"$OUT/app.log" 2>&1 &
LEANTTY_PID=$!

# 等窗口出现（最多 10s）
WID=""
i=0
while [ $i -lt 20 ]; do
    WID=$(x11 xdotool search --pid "$LEANTTY_PID" 2>/dev/null | head -1)
    [ -n "$WID" ] && break
    sleep 0.5
    i=$((i+1))
done
[ -n "$WID" ] || { echo "FATAL: 未找到 tty 窗口"; x11 xdotool search --name "" 2>/dev/null | head -3; exit 1; }
echo "窗口 WID=$WID"
x11 xdotool windowactivate "$WID"
x11 xdotool windowfocus "$WID"
sleep 0.5

# 输入 echo hello 回车
x11 xdotool type --window "$WID" "echo hello_e2e"
x11 xdotool key --window "$WID" Return
sleep 1

# 截图（辅助证据）
x11 import -window root "$OUT/screenshot-repro.png" 2>/dev/null || x11 xwd -root >"$OUT/screenshot-repro.xwd"
echo "截图已保存"

# 关 app
x11 xdotool windowclose "$WID" 2>/dev/null
sleep 1
kill $LEANTTY_PID 2>/dev/null
wait $LEANTTY_PID 2>/dev/null
echo "复现完成：$OUT（app.log / screenshot-repro.png）"
