#!/bin/sh
# soak.sh —— 稳定性长跑（阶段 4F t3）。**默认不进 run_all**。
#
# 用法：sh scripts/e2e/soak.sh [每后端分钟数，默认 15]
#
# 每个后端（cpu、gpu）各跑一轮：
#   负载循环 = 大量滚动输出 + 随机转义序列（光标/颜色/擦除/备用屏/OSC 标题与改色）
#            + 对**自身窗口**的 windowsize 缩放 + 1MB 粘贴 + 选区拖动 + 滚动条拖动
#   采集（每 30s）= RSS、fd 数、线程数，并在结束时汇总 [perf] 帧日志
#
# 安全：所有 X11 操作经 guard.sh 的 x11()（强制 DISPLAY=:99）；只操作自己启动的
# app 与其窗口 id；结束时只对自己的 PID 发 SIGTERM 并 wait（不用 pkill/killall）。
#
# 判据（工程判断，见 DECISIONS）：预热 2 分钟后 RSS 增长 ≤10%、fd 与线程数不单调
# 增长、stderr 无 panic/backtrace。

set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$ROOT/scripts/e2e/out/soak"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"

DUR_MIN="${1:-15}"
DUR_S=$((DUR_MIN * 60))
APP_BIN="$ROOT/target/release/app"
[ -x "$APP_BIN" ] || APP_BIN="$ROOT/target/debug/app"
FAILED=0

for p in $(e2e_stale_pids); do kill_own "$p"; done
sleep 0.5
e2e_start_xvfb "$OUT/xvfb.log" || exit 1
trap 'kill_own "${APP_PID:-}"; kill_own "${CLIP_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"' EXIT
trap 'exit 1' INT TERM
e2e_start_wm "$OUT/wm.log"

# 1MB 粘贴素材（固定内容，避免每次随机化影响可复现性）
CLIP_FILE="$OUT/clip1mb.txt"
if [ ! -s "$CLIP_FILE" ]; then
    awk 'BEGIN { for (i = 0; i < 20000; i++) printf "soak_paste_line_%06d_xxxxxxxxxxxxxxxxxx\n", i }' >"$CLIP_FILE"
fi

run_backend() { # $1 = cpu|gpu
    BACKEND="$1"
    CFG="$OUT/soak-$BACKEND.toml"
    CSV="$OUT/soak-$BACKEND.csv"
    LOG="$OUT/soak-$BACKEND.log"
    printf '[renderer]\nbackend = "%s"\n[window]\ndimensions = { columns = 120, lines = 30 }\n' "$BACKEND" >"$CFG"
    : >"$CSV"
    echo "t_s,rss_kb,fd,threads" >>"$CSV"
    APP_RANDOM_SEED=20260201 APP_PERF_FRAMES=1 APP_CONFIG="$CFG" SHELL=/bin/sh \
        "$APP_BIN" >"$LOG" 2>&1 &
    APP_PID=$!
    i=0
    WID=""
    while [ $i -lt 80 ]; do
        WID=$(x11 xdotool search --pid "$APP_PID" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.2
        i=$((i+1))
    done
    if [ -z "$WID" ]; then
        echo "FAIL soak-$BACKEND (窗口未出现)"
        FAILED=$((FAILED+1))
        return
    fi
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    WX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
    WY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)

    echo "=== soak $BACKEND：${DUR_MIN} 分钟（pid=$APP_PID wid=$WID）"

    ELAPSED=0
    NEXT_SAMPLE=0
    STEP=0
    while [ "$ELAPSED" -lt "$DUR_S" ]; do
        # —— 负载：按 STEP 轮换，避免单一负载掩盖问题 ——
        # 每轮都持续滚动输出（保证渲染压力连续，避免只有突发负载）
        x11 xdotool type --window "$WID" "seq 1 800"
        x11 xdotool key --window "$WID" Return
        case $((STEP % 8)) in
            0)
                # 大量滚动输出（固定种子由 APP_RANDOM_SEED 控制渲染随机性）
                x11 xdotool type --window "$WID" "seq 1 6000 | tail -n +1"
                x11 xdotool key --window "$WID" Return
                ;;
            1)
                # 随机转义序列：光标移动/颜色/擦除/备用屏/OSC
                x11 xdotool type --window "$WID" \
                    "printf '\\033[2J\\033[H\\033[31mR\\033[32mG\\033[33mY\\033[0m\\033[10;20H@\\033[?1049h\\033[7mALT\\033[0m\\033[?1049l\\033]0;soak-title\\007\\033]10;#112233\\007\\033]11;#445566\\007\\033[110\\007\\033[111\\007\\033[K'"
                x11 xdotool key --window "$WID" Return
                ;;
            2)
                # 自身窗口缩放（±40 像素，往返避免单调变小）
                CUR=$(x11 xdotool getwindowgeometry --shell "$WID" | grep '^WIDTH=' | cut -d= -f2)
                if [ "${CUR:-0}" -gt 900 ]; then
                    x11 xdotool windowsize "$WID" 900 500
                else
                    x11 xdotool windowsize "$WID" 1000 560
                fi
                ;;
            3)
                # 1MB 粘贴（右键走生产粘贴路径）
                x11 xclip -selection clipboard -i "$CLIP_FILE" >/dev/null 2>&1 &
                CLIP_PID=$!
                sleep 0.4
                x11 xdotool mousemove $(( ${WX:-0} + 100 )) $(( ${WY:-0} + 100 ))
                x11 xdotool click 3
                sleep 0.6
                kill -TERM "$CLIP_PID" 2>/dev/null
                CLIP_PID=""
                ;;
            4)
                # 选区拖动（两次，覆盖拖选与自动滚动分支）
                x11 xdotool mousemove $(( ${WX:-0} + 20 )) $(( ${WY:-0} + 40 ))
                x11 xdotool mousedown 1
                x11 xdotool mousemove $(( ${WX:-0} + 300 )) $(( ${WY:-0} + 200 ))
                sleep 0.2
                x11 xdotool mouseup 1
                ;;
            6)
                # 阶段 4J t6：滚轮上滚进 scrollback 后再滚回底部（覆盖视口光标隐藏
                # 与 auto-follow 恢复路径）
                GC=$(x11 xdotool getwindowgeometry --shell "$WID")
                CCX=$(echo "$GC" | grep '^X=' | cut -d= -f2)
                CCY=$(echo "$GC" | grep '^Y=' | cut -d= -f2)
                CCW=$(echo "$GC" | grep '^WIDTH=' | cut -d= -f2)
                CCH=$(echo "$GC" | grep '^HEIGHT=' | cut -d= -f2)
                x11 xdotool mousemove $(( ${CCX:-0} + ${CCW:-800} / 2 )) $(( ${CCY:-0} + ${CCH:-500} / 2 ))
                n=0; while [ $n -lt 8 ]; do x11 xdotool click 4; n=$((n+1)); done
                sleep 0.3
                n=0; while [ $n -lt 14 ]; do x11 xdotool click 5; n=$((n+1)); done
                ;;
            7)
                # 阶段 4J t6：粘贴进行中发 Ctrl-C 取消（写线程丢弃未写数据 + 补发
                # ESC[201~ 的路径），每 10 轮至少一次（本分支每 8 轮一次）
                x11 xdotool type --window "$WID" "sleep 20"
                x11 xdotool key --window "$WID" Return
                sleep 0.8
                x11 xclip -selection clipboard -i "$CLIP_FILE" >/dev/null 2>&1 &
                CLIP_PID=$!
                sleep 0.4
                GC=$(x11 xdotool getwindowgeometry --shell "$WID")
                CCX=$(echo "$GC" | grep '^X=' | cut -d= -f2)
                CCY=$(echo "$GC" | grep '^Y=' | cut -d= -f2)
                x11 xdotool mousemove $(( ${CCX:-0} + 100 )) $(( ${CCY:-0} + 100 ))
                x11 xdotool click --delay 0 3
                sleep 2
                x11 xdotool key --window "$WID" ctrl+c
                sleep 0.5
                kill -TERM "$CLIP_PID" 2>/dev/null
                CLIP_PID=""
                ;;
            5)
                # 滚动条拖动（右缘 12px 条带内）
                CW=$(x11 xdotool getwindowgeometry --shell "$WID" | grep '^WIDTH=' | cut -d= -f2)
                CH=$(x11 xdotool getwindowgeometry --shell "$WID" | grep '^HEIGHT=' | cut -d= -f2)
                x11 xdotool mousemove $(( ${WX:-0} + ${CW:-800} - 6 )) $(( ${WY:-0} + 30 ))
                x11 xdotool mousedown 1
                x11 xdotool mousemove $(( ${WX:-0} + ${CW:-800} - 6 )) $(( ${WY:-0} + ${CH:-500} - 60 ))
                sleep 0.2
                x11 xdotool mouseup 1
                ;;
        esac
        STEP=$((STEP+1))

        # 每轮约 1s，每 30 秒采样一次
        sleep 1
        ELAPSED=$((ELAPSED+1))
        if [ "$ELAPSED" -ge "$NEXT_SAMPLE" ]; then
            RSS=$(ps -o rss= -p "$APP_PID" 2>/dev/null | tr -d ' ')
            FD=$(ls /proc/"$APP_PID"/fd 2>/dev/null | wc -l)
            TH=$(ls /proc/"$APP_PID"/task 2>/dev/null | wc -l)
            echo "$ELAPSED,${RSS:-0},${FD:-0},${TH:-0}" >>"$CSV"
            echo "  [${BACKEND} t=${ELAPSED}s] rss=${RSS:-0}KB fd=${FD:-0} threads=${TH:-0}"
            NEXT_SAMPLE=$((NEXT_SAMPLE+30))
        fi
        # app 意外退出则提前结束
        if ! kill -0 "$APP_PID" 2>/dev/null; then
            echo "FAIL soak-$BACKEND (app 在 t=${ELAPSED}s 提前退出)"
            FAILED=$((FAILED+1))
            APP_PID=""
            return
        fi
    done

    # —— 收尾：SIGTERM 并在限定时间退出 ——
    kill -TERM "$APP_PID" 2>/dev/null
    WAITED=0
    while kill -0 "$APP_PID" 2>/dev/null && [ "$WAITED" -lt 20 ]; do
        sleep 0.5
        WAITED=$((WAITED+1))
    done
    if kill -0 "$APP_PID" 2>/dev/null; then
        echo "FAIL soak-$BACKEND (SIGTERM 后 10s 未退出)"
        FAILED=$((FAILED+1))
        kill -KILL "$APP_PID" 2>/dev/null
    fi
    wait "$APP_PID" 2>/dev/null
    APP_PID=""

    # —— 判据 ——
    if grep -qE "panicked at|RUST_BACKTRACE|thread '.*' panicked" "$LOG"; then
        echo "FAIL soak-$BACKEND (stderr 有 panic/backtrace)"
        FAILED=$((FAILED+1))
    fi
    # 预热 2 分钟后（第 4 个样本起）的 RSS 增长 ≤10%
    awk -F, -v b="$BACKEND" '
        NR > 1 && $1 >= 120 {
            if (base == 0) { base = $2; next }
            if ($2 > max) max = $2
        }
        NR > 1 && base != 0 { last = $2 }
        END {
            if (base == 0) { printf "  [%s] 样本不足（无 >=120s 的数据），RSS 判据跳过\n", b; exit 0 }
            g = (max - base) / base * 100
            printf "  [%s] RSS 预热末=%dKB 峰值=%dKB 增长=%.1f%%（判据 ≤10%%）\n", b, base, max, g
            if (g > 10) { printf "FAIL soak-%s (RSS 增长 %.1f%% > 10%%)\n", b, g; exit 1 }
        }
    ' "$CSV" || FAILED=$((FAILED+1))

    # fd / 线程：不得单调增长（把样本序列的严格单调递增视为失败）
    for col in 3 4; do
        awk -F, -v c="$col" -v b="$BACKEND" '
            BEGIN { inc = 1; prev = -1 }
            NR == 2 { prev = $c + 0; next }
            NR > 2 {
                v = $c + 0
                if (v <= prev) inc = 0
                prev = v
            }
            END {
                if (NR > 5 && inc) { printf "FAIL soak-%s (列 %d 单调增长)\n", b, c; exit 1 }
            }
        ' "$CSV" || FAILED=$((FAILED+1))
    done

    echo "  [perf] 帧日志条数=$(grep -c '\[perf\] frames n=' "$LOG" 2>/dev/null || echo 0)"
    LG=$(grep -oE 'loop_gap_ms=[0-9]+' "$LOG" | cut -d= -f2 | sort -n | tail -1)
    echo "  [perf] loop_gap 最大值=${LG:-NA}ms（长跑负载下仅作观测，不作为门限）"
    echo "  采集表：$CSV"
}

run_backend cpu
run_backend gpu

echo "===== soak 汇总: backend=2 failed=$FAILED 时长=${DUR_MIN}min/后端 ====="
[ "$FAILED" -eq 0 ]
