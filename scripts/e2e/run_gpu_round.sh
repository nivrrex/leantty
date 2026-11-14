#!/bin/sh
# GPU 后端 e2e 轮（阶段 4B t5）：backend=gpu / auto 的窗口冒烟、截图阈值对拍、
# 无 ICD 回落与强制失败两条边界用例。
#
# 为什么不是"把 run_all 的全部用例跑第二遍"：绝大多数用例的断言依赖
# `APP_DUMP_SNAPSHOT`（仅 CPU 后端可写）与脏行增量语义（GPU 每帧整帧重绘），
# 在 GPU 后端下不具备等价物。GPU 与 CPU 的逐像素等价性由
# `cargo test -p render --test gpu_parity`（8 个用例，含 1 LSB 门禁）覆盖。
#
# 安全护栏同 run_all：所有 X11 操作经 guard.sh 的 x11()，强制 DISPLAY=:99。

set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$ROOT/scripts/e2e/out"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
APP_BIN="$ROOT/target/debug/app"
PASS=0
FAIL=0
SKIP=0

for p in $(e2e_stale_pids); do kill_own "$p"; done
sleep 0.3
e2e_start_xvfb "$OUT/gpu-round-xvfb.log" || exit 1
trap 'kill_own "${APP_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"; exit 0' EXIT INT TERM
e2e_start_wm "$OUT/gpu-round-wm.log"

CFG_GPU="$OUT/gpu-round-gpu.toml"
CFG_AUTO="$OUT/gpu-round-auto.toml"
CFG_CPU="$OUT/gpu-round-cpu.toml"
cat >"$CFG_GPU" <<'TOML'
[window]
dimensions = { columns = 80, lines = 24 }
[renderer]
backend = "gpu"
TOML
sed 's/^backend = "gpu"/backend = "auto"/' "$CFG_GPU" >"$CFG_AUTO"
sed 's/^backend = "gpu"/backend = "cpu"/' "$CFG_GPU" >"$CFG_CPU"

# 起 app 并等待窗口；输出 APP_PID/WID
start_gpu_app() { # $1=config $2=log $3=extra-env
    env $3 APP_CONFIG="$1" SHELL=/bin/sh APP_PERF_FRAMES=1 "$APP_BIN" >"$2" 2>&1 &
    APP_PID=$!
    i=0
    WID=""
    while [ $i -lt 80 ]; do
        WID=$(x11 xdotool search --pid "$APP_PID" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.2
        i=$((i+1))
    done
    [ -n "$WID" ]
}

stop_app() {
    [ -n "${APP_PID:-}" ] || return 0
    kill -TERM "$APP_PID" 2>/dev/null
    wait "$APP_PID" 2>/dev/null
    APP_PID=""
}

echo "--- GPU 轮：backend=gpu 冒烟（窗口 + 首帧 + present）"
L="$OUT/gpu-round-smoke.log"
if start_gpu_app "$CFG_GPU" "$L" ""; then
    # debug 构建下 GPU 每帧渲染很慢，2s 内可能凑不满 50 帧的 perf 汇总行，
    # 因此断言只要求"真的用了 GPU + 出了首帧"；frames 行出现则额外打印。
    sleep 3
    if grep -q "gpu_init_ms" "$L" && grep -q "backend=gpu" "$L" && grep -q "first_frame_ms" "$L"; then
        grep -q "\[perf\] frames n=" "$L" && echo "  （perf frames 行已出现）"
        echo "PASS gpu-round-smoke"; PASS=$((PASS+1))
    else
        echo "FAIL gpu-round-smoke (缺少 gpu_init_ms/backend=gpu/first_frame_ms)"; FAIL=$((FAIL+1))
    fi
    x11 xdotool type --window "$WID" "echo GPU_ROUND_OK"
    x11 xdotool key --window "$WID" Return
    sleep 1
    x11 import -window "$WID" "$OUT/gpu-round-gpu.png" 2>/dev/null
    stop_app
else
    echo "FAIL gpu-round-smoke (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：backend=cpu 同场景截图（阈值对拍参照）"
L="$OUT/gpu-round-cpu.log"
if start_gpu_app "$CFG_CPU" "$L" ""; then
    sleep 2
    x11 xdotool type --window "$WID" "echo GPU_ROUND_OK"
    x11 xdotool key --window "$WID" Return
    sleep 1
    x11 import -window "$WID" "$OUT/gpu-round-cpu.png" 2>/dev/null
    stop_app
else
    echo "FAIL gpu-round-cpu-ref (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：GPU 与 CPU 截图差异（≤1 LSB 允许，结构性差异判失败）"
if [ -s "$OUT/gpu-round-gpu.png" ] && [ -s "$OUT/gpu-round-cpu.png" ]; then
    # 用 ImageMagick 统计"通道差 > 8"的像素数（阈值远大于 1 LSB 舍入，但远小于
    # 整行缺失这类结构性差异），见 DECISIONS 登记的理由。
    BIG=$(compare -metric AE -fuzz 3% "$OUT/gpu-round-gpu.png" "$OUT/gpu-round-cpu.png" null: 2>&1 | head -1)
    TOTAL=$(identify -format "%[fx:w*h]" "$OUT/gpu-round-gpu.png" 2>/dev/null)
    echo "  gpu vs cpu 截图：超过 3% fuzz 的像素 $BIG / 总像素 $TOTAL"
    if [ -n "$BIG" ] && [ "$BIG" -lt 2000 ]; then
        echo "PASS gpu-round-screenshot-threshold ($BIG px)"; PASS=$((PASS+1))
    else
        echo "FAIL gpu-round-screenshot-threshold ($BIG px，疑似结构性差异)"; FAIL=$((FAIL+1))
    fi
else
    echo "FAIL gpu-round-screenshot-threshold (截图缺失)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：inverse（ESC[7m）反色可见性（绝对像素，非 CPU 对比）"
L="$OUT/gpu-round-inverse.log"
S="$OUT/gpu-round-inverse.png"
if start_gpu_app "$CFG_GPU" "$L" ""; then
    sleep 1.5
    x11 xdotool type --window "$WID" "printf '\033[7m          \033[0m'"
    x11 xdotool key --window "$WID" Return
    sleep 1.2
    x11 import -window "$WID" "$S" 2>/dev/null
    if [ -s "$S" ]; then
        WHITE=$(convert "$S" -fuzz 2% -fill black +opaque "#ffffff" -fill white -opaque "#ffffff" \
            -format "%[fx:int(mean*w*h)]" info: 2>/dev/null)
        echo "  反色块白色像素=$WHITE（阈值 ≥768）"
        if [ "$WHITE" -ge 768 ]; then
            echo "PASS gpu-round-inverse-visible ($WHITE 白像素)"; PASS=$((PASS+1))
        else
            echo "FAIL gpu-round-inverse-visible (白像素 $WHITE < 768)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL gpu-round-inverse-visible (截图缺失)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL gpu-round-inverse-visible (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：交互 bash 括号粘贴反色可见性（像素门禁）"
CFG35="$OUT/gpu-round-bash.toml"
printf '[renderer]\nbackend = "gpu"\n[window]\ndimensions = { columns = 80, lines = 24 }\n[terminal.shell]\nprogram = "/bin/bash"\nargs = ["--norc", "--noprofile"]\n' > "$CFG35"
printf 'PASTED35XX' > "$OUT/gpu-round-clip.txt"
L="$OUT/gpu-round-paste-inverse.log"
S="$OUT/gpu-round-paste-inverse.png"
if start_gpu_app "$CFG35" "$L" ""; then
    sleep 2.5
    x11 xclip -selection clipboard -i "$OUT/gpu-round-clip.txt" >/dev/null 2>&1 &
    CLIP_PID=$!
    sleep 0.5
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2); GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2); GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-200} / 2 )) $(( ${GY:-0} + ${GH:-200} / 2 ))
    sleep 0.2
    x11 xdotool click 3
    sleep 1.2
    x11 import -window "$WID" "$S" 2>/dev/null
    kill -TERM "${CLIP_PID:-}" 2>/dev/null
    CLIP_PID=""
    if [ -s "$S" ]; then
        W=$(convert "$S" -fuzz 2% -fill black +opaque "#ffffff" -fill white -opaque "#ffffff" \
            -format "%[fx:int(mean*w*h)]" info: 2>/dev/null)
        echo "  括号粘贴反色块白像素=$W（阈值 ≥768）"
        if [ "$W" -ge 768 ]; then
            echo "PASS gpu-round-paste-inverse-visible ($W 白像素)"; PASS=$((PASS+1))
        else
            echo "FAIL gpu-round-paste-inverse-visible (白像素 $W < 768)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL gpu-round-paste-inverse-visible (截图缺失)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL gpu-round-paste-inverse-visible (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：配置调色板到达 VT 层（ESC[31m / ESC[7m）"
CFG36="$OUT/gpu-round-palette.toml"
cat >"$CFG36" <<'TOML'
[renderer]
backend = "gpu"
[window]
dimensions = { columns = 80, lines = 24 }
[colors.primary]
background = "#101010"
foreground = "#e0e0e0"
[colors.normal]
red = "#d32f2f"
TOML
L="$OUT/gpu-round-palette.log"
S="$OUT/gpu-round-palette.png"
if start_gpu_app "$CFG36" "$L" ""; then
    sleep 1.5
    x11 xdotool type --window "$WID" "printf '\\033[31mREDTEXT\\033[0m\\n\\033[7m          \\033[0m\\n\\033[7m\\033[31m          \\033[0m\\n'"
    x11 xdotool key --window "$WID" Return
    sleep 1.2
    x11 import -window "$WID" "$S" 2>/dev/null
    if [ -s "$S" ]; then
        R=$(convert "$S" -fuzz 2% -fill black +opaque "#d32f2f" -fill white -opaque "#d32f2f" \
            -format "%[fx:int(mean*w*h)]" info: 2>/dev/null)
        echo "  配置红(#d32f2f) 像素=$R（阈值 ≥500）"
        if [ "${R:-0}" -ge 500 ]; then
            echo "PASS gpu-round-palette-reaches-vt ($R)"; PASS=$((PASS+1))
        else
            echo "FAIL gpu-round-palette-reaches-vt (配置红像素 $R < 500)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL gpu-round-palette-reaches-vt (截图缺失)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL gpu-round-palette-reaches-vt (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：视口上滚后光标隐藏"
CFG37="$OUT/gpu-round-cursor.toml"
cat >"$CFG37" <<'TOML'
[renderer]
backend = "gpu"
[window]
dimensions = { columns = 80, lines = 24 }
[colors.cursor]
cursor = "#ff00ff"
[colors.primary]
background = "#000000"
foreground = "#ffffff"
TOML
L="$OUT/gpu-round-cursor.log"
if start_gpu_app "$CFG37" "$L" ""; then
    sleep 1.5
    x11 xdotool type --window "$WID" "seq 1 200"
    x11 xdotool key --window "$WID" Return
    sleep 1.5
    x11 import -window "$WID" "$OUT/gpu-round-cursor-bottom.png" 2>/dev/null
    CB=$(convert "$OUT/gpu-round-cursor-bottom.png" -fuzz 2% -fill black +opaque "#ff00ff" \
        -fill white -opaque "#ff00ff" -format "%[fx:int(mean*w*h)]" info: 2>/dev/null)
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2); GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2); GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-200} / 2 )) $(( ${GY:-0} + ${GH:-200} / 2 ))
    sleep 0.2
    i=0; while [ $i -lt 8 ]; do x11 xdotool click 4; i=$((i+1)); done
    sleep 1.5
    Z=0; T=0
    for k in 1 2 3; do
        x11 import -window "$WID" "$OUT/gpu-round-cursor-s$k.png" 2>/dev/null
        N=$(convert "$OUT/gpu-round-cursor-s$k.png" -fuzz 2% -fill black +opaque "#ff00ff" \
            -fill white -opaque "#ff00ff" -format "%[fx:int(mean*w*h)]" info: 2>/dev/null)
        [ "${N:-0}" -eq 0 ] && Z=$((Z+1))
        T=$((T+1))
        sleep 0.5
    done
    echo "  回底=$CB 上滚后全零=$Z/$T"
    if [ "${CB:-0}" -gt 0 ] && [ "$Z" -eq "$T" ]; then
        echo "PASS gpu-round-cursor-hidden-on-scroll"; PASS=$((PASS+1))
    else
        echo "FAIL gpu-round-cursor-hidden-on-scroll (bottom=$CB zero=$Z/$T)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL gpu-round-cursor-hidden-on-scroll (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：Ctrl-C 截断括号粘贴后 bash 不得卡住"
CFG41G="$OUT/gpu-round-bracketed-cancel.toml"
W41G="$OUT/gpu-round-wedge.txt"
CLIP41G="$OUT/gpu-round-clip.txt"
rm -f "$W41G"
printf '[renderer]\nbackend = "gpu"\n[window]\ndimensions = { columns = 100, lines = 30 }\n[terminal.shell]\nprogram = "/bin/bash"\nargs = ["--norc", "--noprofile"]\n' > "$CFG41G"
awk 'BEGIN { for (i = 0; i < 130000; i++) printf "paste5m_line_%06d_yyyyyyyyyyyyyy\n", i }' > "$CLIP41G"
L="$OUT/gpu-round-bracketed-cancel.log"
if start_gpu_app "$CFG41G" "$L" ""; then
    sleep 1.5
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    x11 xclip -selection clipboard -i "$CLIP41G" >/dev/null 2>&1 &
    CLIP_PID=$!
    sleep 0.5
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2); GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2); GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-400} / 2 )) $(( ${GY:-0} + ${GH:-300} / 2 ))
    sleep 0.2
    x11 xdotool click 3
    sleep 0.6
    x11 xdotool key --window "$WID" ctrl+c
    kill -TERM "${CLIP_PID:-}" 2>/dev/null
    CLIP_PID=""
    sleep 1.2
    x11 xdotool type --window "$WID" "echo WEDGE_OK_\$((1+1)) > $W41G"
    x11 xdotool key --window "$WID" Return
    i=0
    while [ $i -lt 30 ]; do
        [ -s "$W41G" ] && break
        sleep 0.5
        i=$((i+1))
    done
    DROP=$(grep -oE "ctrl_c_drop_paste bytes=[0-9]+" "$L" | tail -1 | cut -d= -f2)
    WOK=$(grep -c "WEDGE_OK_2" "$W41G" 2>/dev/null || true)
    echo "  丢弃=${DROP:-0}B 探针=$WOK"
    if [ "${DROP:-0}" -gt 0 ] && [ "${WOK:-0}" -ge 1 ]; then
        echo "PASS gpu-round-bracketed-paste-ctrl-c"; PASS=$((PASS+1))
    else
        echo "FAIL gpu-round-bracketed-paste-ctrl-c (dropped=${DROP:-0} ok=${WOK:-0})"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL gpu-round-bracketed-paste-ctrl-c (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：auto + 无 ICD → 回落 CPU 且窗口正常"
L="$OUT/gpu-round-auto-noicd.log"
if start_gpu_app "$CFG_AUTO" "$L" "VK_ICD_FILENAMES=/nonexistent/icd.json"; then
    sleep 1.5
    if grep -qE "gpu 不可用，回落 cpu" "$L" && grep -q "first_frame_ms" "$L"; then
        echo "PASS gpu-round-auto-fallback-cpu"; PASS=$((PASS+1))
    else
        echo "FAIL gpu-round-auto-fallback-cpu (未出现回落日志或未出首帧)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL gpu-round-auto-fallback-cpu (窗口未出现)"; FAIL=$((FAIL+1))
fi

echo "--- GPU 轮：gpu + 无 ICD → 非零退出"
APP_CONFIG="$CFG_GPU" VK_ICD_FILENAMES=/nonexistent/icd.json timeout 20 "$APP_BIN" >"$OUT/gpu-round-gpu-noicd.log" 2>&1
RC=$?
if [ "$RC" -ne 0 ] && [ "$RC" -ne 124 ]; then
    echo "PASS gpu-round-gpu-noicd-exit ($RC)"; PASS=$((PASS+1))
else
    echo "FAIL gpu-round-gpu-noicd-exit (exit=$RC，期望非零且非超时)"; FAIL=$((FAIL+1))
fi

echo "===== GPU 轮汇总: PASS=$PASS FAIL=$FAIL SKIP=$SKIP (dump 依赖用例在 GPU 轮不适用) ====="
[ "$FAIL" -eq 0 ]
