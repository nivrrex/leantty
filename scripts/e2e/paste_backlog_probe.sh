#!/bin/sh
# paste_backlog_probe.sh —— 阶段 4L t1a：粘贴积压超限时是否卡死 readline（默认不进 run_all）。
#
# 关键点（bash 语义）：括号粘贴模式下 readline 把粘贴文本**插入行缓冲**、按 Return 才执行；
# 若结束标记 ESC[201~ 丢失，readline 会一直处于"粘贴中"，之后的 Return 与命令都被当成
# 字面内容插入 → 不会执行。因此判据用"命令是否真的执行"（标记文件），而不是屏幕文字。
#
# 用法：sh scripts/e2e/paste_backlog_probe.sh [轮数，默认 5] [后端，默认 cpu]
#
# 复现思路（真实路径，release）：
#   1) bash --norc 交互（readline 打开括号粘贴，会发 ESC[200~ … ESC[201~）
#   2) 前台 `sleep 25` 不读 stdin → 写线程阻塞 → pending_paste 积压
#   3) 连续 13 次 1MB 真实粘贴（xclip + 右键），第 13 次会撞上 12MiB 积压上限
#   4) sleep 结束后，键入 `echo WEDGE_OK_$((1+1)) > 标记文件` 并回车
#   5) 标记文件出现且内容为 WEDGE_OK_2 ⇒ 未卡死；否则说明 readline 仍处在
#      "粘贴内容"状态（结束标记 ESC[201~ 被截断掉了）
#
# 安全：X11 操作经 guard.sh 的 x11()（强制 :99）；只操作自己启动的进程与窗口。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
RUNS="${1:-5}"
BACKEND="${2:-cpu}"
OUT="$ROOT/scripts/e2e/out/pb"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
LEANTTY_BIN="$ROOT/target/release/leantty"
[ -x "$LEANTTY_BIN" ] || { echo "FATAL: 需要 release 二进制"; exit 1; }

CLIP="$OUT/clip1mb.txt"
# 13MiB 注释行（bash 执行 `#` 注释是纯解析、不 fork，消费很快；用注释避免"逐行 not found"拖慢）
[ -s "$CLIP" ] && [ "$(wc -c <"$CLIP")" -gt 13000000 ] || \
    awk 'BEGIN { for (i = 0; i < 400000; i++) printf "#pb_line_%06d_xxxxxxxxxxxxxxxxxxxx\n", i }' >"$CLIP"
MARK="/tmp/pb_wedge_$$.txt"

PASS=0
FAIL=0
i=0
while [ "$i" -lt "$RUNS" ]; do
    i=$((i + 1))
    for p in $(e2e_stale_pids); do kill_own "$p"; done
    sleep 0.3
    e2e_start_xvfb "$OUT/xvfb.log" || exit 1
    e2e_start_wm "$OUT/wm.log"
    CFG="$OUT/cfg-$BACKEND.toml"
    printf '[renderer]\nbackend = "%s"\n[window]\ndimensions = { columns = 100, lines = 30 }\n[terminal.shell]\nprogram = "/bin/bash"\nargs = ["--norc", "--noprofile"]\n' "$BACKEND" >"$CFG"
    LOG="$OUT/run$i-$BACKEND.log"
    rm -f "$MARK"
    LEANTTY_CONFIG="$CFG" LEANTTY_PERF_FRAMES=1 SHELL=/bin/bash "$LEANTTY_BIN" >"$LOG" 2>&1 &
    LEANTTY_PID=$!
    k=0
    WID=""
    while [ $k -lt 100 ]; do
        WID=$(x11 xdotool search --pid "$LEANTTY_PID" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.2
        k=$((k + 1))
    done
    if [ -z "$WID" ]; then
        echo "run$i: 窗口未出现"
        FAIL=$((FAIL + 1))
        kill_own "$LEANTTY_PID"; kill_own "$XVFB_PID"; kill_own "$WM_PID"
        continue
    fi
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 1.2   # 等 bash 就绪并发出 ?2004h
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
    GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    x11 xdotool type --window "$WID" "sleep 25"
    x11 xdotool key --window "$WID" Return
    sleep 0.6
    # 单次 13MiB 粘贴（> 12MiB 上限）：旧实现会截断成 12MiB 并丢掉尾部 ESC[201~
    x11 xclip -selection clipboard -i "$CLIP" >/dev/null 2>&1 &
    CLIP_PID=$!
    sleep 0.5
    x11 xdotool mousemove $(( ${GX:-0} + 120 )) $(( ${GY:-0} + 120 ))
    x11 xdotool click --delay 0 3
    sleep 0.4
    kill -TERM "$CLIP_PID" 2>/dev/null
    CLIP_PID=""
    REJ=$(grep -c "paste_dropped\|paste_rejected" "$LOG" 2>/dev/null || true)
    # 等 sleep 结束（25s）让 shell 恢复读取，并给写线程足够时间把 12MiB 灌进 tty
    sleep 26
    # 先按 Return 执行被粘贴进行缓冲的内容；若 readline 仍处于"粘贴中"，这个 Return
    # 只会被当作字面内容插入，不会执行。
    x11 xdotool key --window "$WID" Return
    sleep 6
    # 再给上行缓冲一个回车（正常路径下此时已在提示符），然后跑标记命令
    x11 xdotool key --window "$WID" Return
    sleep 1
    x11 xdotool type --window "$WID" "echo WEDGE_OK_\$((1+1)) > $MARK"
    x11 xdotool key --window "$WID" Return
    sleep 5
    GOT=""
    [ -s "$MARK" ] && GOT=$(cat "$MARK")
    STAGE=first-attempt
    if [ "$GOT" != "WEDGE_OK_2" ]; then
        # 区分"还在消化 12MiB"与"真卡在粘贴状态"：再等 20s 重试
        rm -f "$MARK"
        sleep 20
        x11 xdotool key --window "$WID" Return
        sleep 1
        x11 xdotool type --window "$WID" "echo WEDGE_OK_\$((1+1)) > $MARK"
        x11 xdotool key --window "$WID" Return
        sleep 5
        [ -s "$MARK" ] && GOT=$(cat "$MARK")
        [ "$GOT" = "WEDGE_OK_2" ] && STAGE="second-attempt(+20s)"
    fi
    if [ "$GOT" != "WEDGE_OK_2" ]; then
        # 仍不执行：发 Ctrl-C（readline 会退出粘贴状态）再试一次，用于区分真卡死
        rm -f "$MARK"
        x11 xdotool key ctrl+c
        sleep 1
        x11 xdotool type --window "$WID" "echo WEDGE_OK_\$((1+1)) > $MARK"
        x11 xdotool key --window "$WID" Return
        sleep 4
        [ -s "$MARK" ] && GOT=$(cat "$MARK")
        [ "$GOT" = "WEDGE_OK_2" ] && STAGE="after-ctrl-c"
    fi
    if [ "$GOT" = "WEDGE_OK_2" ]; then
        echo "run$i: PASS（WEDGE_OK_2 已执行，阶段=$STAGE，截断/拒绝日志数=${REJ:-0}）"
        PASS=$((PASS + 1))
    else
        echo "run$i: FAIL（三次尝试后仍未执行，截断/拒绝日志数=${REJ:-0}）"
        FAIL=$((FAIL + 1))
    fi
    kill -TERM "$LEANTTY_PID" 2>/dev/null
    w=0
    while kill -0 "$LEANTTY_PID" 2>/dev/null && [ "$w" -lt 20 ]; do sleep 0.5; w=$((w + 1)); done
    kill -KILL "$LEANTTY_PID" 2>/dev/null
    wait "$LEANTTY_PID" 2>/dev/null
    LEANTTY_PID=""
    kill_own "${XVFB_PID:-}"
    kill_own "${WM_PID:-}"
    sleep 0.5
done

echo "===== paste_backlog_probe: runs=$RUNS 通过=$PASS 失败=$FAIL ====="
[ "$FAIL" -eq 0 ]
