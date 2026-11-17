#!/bin/sh
# 阶段 6A t5c：在 leantty 里跑 keyprobe，注入 a/Enter/Esc/Tab/Up/ctrl+a/ctrl+c/ctrl+d/ctrl+p，
# 打印探针收到的字节（hex）。每个模式一次调用：sh scripts/e2e/keymatrix.sh <mode> <log>
#   模式：legacy / kitty7 / mok2（探针预设见 scripts/e2e/keyprobe.py）
set -eu
ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
MODE=${1:-kitty7}; LOG=${2:-/tmp/keymatrix.log}
export DISPLAY=:99
. "$ROOT/scripts/e2e/guard.sh"
rm -f "$LOG"
for p in $(e2e_stale_pids); do kill_own "$p"; done; sleep 0.3
e2e_start_xvfb /tmp/keymatrix-xvfb.log || exit 1
trap 'kill_own "${P:-}"; kill_own "$XVFB_PID"; kill_own "$WM_PID"' EXIT
e2e_start_wm /tmp/keymatrix-wm.log
CFG=$(mktemp /tmp/keymatrix-XXXX.toml)
printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 100, lines = 24 }\n[terminal.shell]\nprogram = "/usr/bin/python3"\nargs = ["%s/scripts/e2e/keyprobe.py", "--mode", "%s", "--log", "%s"]\n' \
    "$ROOT" "$MODE" "$LOG" > "$CFG"
LEANTTY_CONFIG=$CFG SHELL=/bin/sh "$ROOT/target/release/leantty" >/tmp/keymatrix-app.out 2>/tmp/keymatrix-app.err &
P=$!
i=0
while [ $i -lt 100 ]; do WID=$(x11 xdotool search --pid "$P" 2>/dev/null | head -1); [ -n "$WID" ] && break; sleep 0.2; i=$((i+1)); done
x11 xdotool windowactivate "$WID" >/dev/null 2>&1
x11 xdotool windowfocus "$WID" >/dev/null 2>&1
sleep 1.5
x11 xdotool key a; sleep 0.3            # 注入生效证据（普通字母）
x11 xdotool key Return; sleep 0.2
x11 xdotool key Escape; sleep 0.2
x11 xdotool key Tab; sleep 0.2
x11 xdotool key Up; sleep 0.2
x11 xdotool key ctrl+a; sleep 0.2
x11 xdotool key ctrl+c; sleep 0.2
x11 xdotool key ctrl+d; sleep 0.3
x11 xdotool key ctrl+p; sleep 0.3
x11 xdotool key a; sleep 0.3            # 收尾注入证据
sleep 0.4
kill -TERM "$P" 2>/dev/null; wait "$P" 2>/dev/null || true
rm -f "$CFG"
grep " IN " "$LOG" | awk '{print $3}' | tr '\n' ' '; echo
