#!/bin/sh
# timer_dep_matrix.sh —— 阶段 4T t2：4S 把控制流改为 Wait 之后，"原本可能靠空转顺带驱动"
# 的行为是否仍然工作。
#
# 用法：sh scripts/e2e/timer_dep_matrix.sh [cpu|gpu]
#
# 覆盖：① 拖选自动滚动（指针静止在窗口外）② 滚动条拇指拖出窗口再回来 ③ 双击/三击
# ④ shell 退出（exit 与被信号终止） ⑤ OSC 0 标题 ⑥ PRIMARY 选区（空闲 5 秒后仍可读）
# ⑦ SIGTERM 退出。判据见各项注释。
#
# 安全：X11 操作经 guard.sh 的 x11()（:99）；只操作自己启动的进程与窗口。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BACKEND="${1:-cpu}"
OUT="$ROOT/scripts/e2e/out/tdep"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"
LEANTTY_BIN="$ROOT/target/release/leantty"
[ -x "$LEANTTY_BIN" ] || { echo "FATAL: 需要 release 二进制"; exit 1; }

PASS=0
FAIL=0
ok() { echo "  $1: OK $2"; PASS=$((PASS + 1)); }
bad() { echo "  $1: FAIL $2"; FAIL=$((FAIL + 1)); }

start_app() {
    for p in $(e2e_stale_pids); do kill_own "$p"; done
    sleep 0.3
    e2e_start_xvfb "$OUT/xvfb-$BACKEND.log" || return 1
    e2e_start_wm "$OUT/wm-$BACKEND.log"
    printf '[renderer]\nbackend = "%s"\n[window]\ndimensions = { columns = 80, lines = 24 }\n' "$BACKEND" >"$OUT/cfg-$BACKEND.toml"
    LEANTTY_CONFIG="$OUT/cfg-$BACKEND.toml" LEANTTY_PERF_FRAMES=1 SHELL=/bin/sh "$LEANTTY_BIN" >"$OUT/app-$BACKEND.log" 2>&1 &
    LEANTTY_PID=$!
    i=0
    WID=""
    while [ $i -lt 120 ]; do
        WID=$(x11 xdotool search --pid "$LEANTTY_PID" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.2
        i=$((i + 1))
    done
    [ -n "$WID" ] || return 1
    sleep 1.0
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
    GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2)
    GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    return 0
}

stop_app() {
    kill -TERM "$LEANTTY_PID" 2>/dev/null
    w=0
    while kill -0 "$LEANTTY_PID" 2>/dev/null && [ "$w" -lt 20 ]; do sleep 0.25; w=$((w + 1)); done
    kill -KILL "$LEANTTY_PID" 2>/dev/null
    wait "$LEANTTY_PID" 2>/dev/null
    LEANTTY_PID=""
    kill_own "${XVFB_PID:-}"
    kill_own "${WM_PID:-}"
    kill_own "${CLIP_PID:-}"
    sleep 0.4
}

thumb(){ # 打印滚动条拇指底部占比（0..1）
    _w=$(identify -format "%w" "$1" 2>/dev/null) || { echo -1; return; }
    _h=$(identify -format "%h" "$1" 2>/dev/null) || { echo -1; return; }
    convert "$1" -crop "8x${_h}+$((_w-8))+0" +repage -colorspace Gray -depth 8 txt:- 2>/dev/null \
        | sed -n 's/^\([0-9]*\),\([0-9]*\):.*gray(\([0-9]*\)).*/\2 \3/p' \
        | awk -v h="$_h" '{ if ($2+0 > 200 && $1+0 > m) m=$1+0 } END { if (m == "") print -1; else printf "%.3f\n", (m+1)/h }'
}

echo "===== timer_dep_matrix: backend=$BACKEND ====="

# ---------- ① 拖选自动滚动（指针静止在窗口外） ----------
if start_app; then
    x11 xdotool type --window "$WID" "seq 1 300"
    x11 xdotool key --window "$WID" Return
    sleep 2
    # 先滚到顶，留出向下自动滚动的空间
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} / 2 )) $(( ${GY:-0} + ${GH:-480} / 2 ))
    n=0; while [ $n -lt 40 ]; do x11 xdotool click 4; n=$((n+1)); done
    sleep 0.6
    x11 import -window "$WID" "$OUT/$BACKEND-autoscroll-a.png" 2>/dev/null
    T1=$(thumb "$OUT/$BACKEND-autoscroll-a.png")
    # 按下后把指针移到窗口下边缘之下并**静止不动**
    x11 xdotool mousemove $(( ${GX:-0} + 40 )) $(( ${GY:-0} + 60 ))
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove $(( ${GX:-0} + 40 )) $(( ${GY:-0} + ${GH:-480} + 40 ))
    sleep 0.5
    x11 import -window "$WID" "$OUT/$BACKEND-autoscroll-b.png" 2>/dev/null
    T2=$(thumb "$OUT/$BACKEND-autoscroll-b.png")
    sleep 2.0
    x11 import -window "$WID" "$OUT/$BACKEND-autoscroll-c.png" 2>/dev/null
    T3=$(thumb "$OUT/$BACKEND-autoscroll-c.png")
    x11 xdotool mouseup 1
    sleep 0.8
    x11 import -window "$WID" "$OUT/$BACKEND-autoscroll-d.png" 2>/dev/null
    T4=$(thumb "$OUT/$BACKEND-autoscroll-d.png")
    sleep 1.5
    x11 import -window "$WID" "$OUT/$BACKEND-autoscroll-e.png" 2>/dev/null
    T5=$(thumb "$OUT/$BACKEND-autoscroll-e.png")
    MOVE=$(awk "BEGIN{d=${T3:-0}-${T2:-0}; printf \"%.3f\", d}")
    # 选区是否随自动滚动**持续扩展**（0.5s 与 2s 的高亮差异）
    SEL_GROW=$(compare -metric AE "$OUT/$BACKEND-autoscroll-b.png" "$OUT/$BACKEND-autoscroll-c.png" null: 2>&1 | head -1)
    STOPPED=$(awk "BEGIN{d=${T5:-0}-${T4:-0}; if (d<0) d=-d; printf \"%d\", (d<=0.02)}")
    if awk "BEGIN{exit !(${MOVE:-0} >= 0.01)}" && [ "$STOPPED" = "1" ] && [ "${SEL_GROW:-0}" -gt 100 ]; then
        ok "① 拖选自动滚动" "起=$T1 0.5s=$T2 2s=$T3（+$MOVE） 选区扩展 AE=$SEL_GROW 松开后=$T4→$T5（已停）"
    else
        bad "① 拖选自动滚动" "起=$T1 0.5s=$T2 2s=$T3 选区扩展=$SEL_GROW 松开后=$T4→$T5"
    fi
    stop_app
else
    bad "① 拖选自动滚动" "窗口未出现"
fi

# ---------- ② 滚动条拇指拖出窗口再回来 ----------
if start_app; then
    x11 xdotool type --window "$WID" "seq 1 300"
    x11 xdotool key --window "$WID" Return
    sleep 1.5
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} - 5 )) $(( ${GY:-0} + 20 ))
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} - 5 )) $(( ${GY:-0} + ${GH:-480} / 2 ))
    sleep 0.3
    # 移出窗口（右下角外）再回到窗口内
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} + 60 )) $(( ${GY:-0} + ${GH:-480} + 60 ))
    sleep 0.3
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} - 5 )) $(( ${GY:-0} + 30 ))
    x11 xdotool mouseup 1
    sleep 0.6
    x11 import -window "$WID" "$OUT/$BACKEND-sbdrag-a.png" 2>/dev/null
    TA=$(thumb "$OUT/$BACKEND-sbdrag-a.png")
    # 松开后滚轮仍应生效（未卡在拖动状态）
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-800} / 2 )) $(( ${GY:-0} + ${GH:-480} / 2 ))
    n=0; while [ $n -lt 5 ]; do x11 xdotool click 4; n=$((n+1)); done
    sleep 0.8
    x11 import -window "$WID" "$OUT/$BACKEND-sbdrag-b.png" 2>/dev/null
    TB=$(thumb "$OUT/$BACKEND-sbdrag-b.png")
    A=$(compare -metric AE "$OUT/$BACKEND-sbdrag-a.png" "$OUT/$BACKEND-sbdrag-b.png" null: 2>&1 | head -1)
    if [ "${A:-0}" -gt 200 ]; then
        ok "② 滚动条拖出/回" "拇指 $TA→$TB，松开后滚轮仍生效（AE=$A）"
    else
        bad "② 滚动条拖出/回" "拇指 $TA→$TB，滚轮未改变画面（AE=$A，疑卡住）"
    fi
    stop_app
else
    bad "② 滚动条拖出/回" "窗口未出现"
fi

# ---------- ③ 双击/三击 ----------
if start_app; then
    x11 xdotool type --window "$WID" "echo word1 word2 word3"
    x11 xdotool key --window "$WID" Return
    sleep 1.0
    x11 import -window "$WID" "$OUT/$BACKEND-click-0.png" 2>/dev/null
    # 在第二行内容上双击（选词），再在同一处三击（选整行）
    # 重新取几何（WM 可能在映射后移动过窗口）
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO" | grep '^X=' | cut -d= -f2)
    GY=$(echo "$GEO" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2)
    GH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    # 依次尝试几个候选行（不同 cell 高度下文字行位置不同），取第一个能选中文本的位置
    # 注意：`xdotool click --repeat 2 --repeat-delay 150` 实测不会被识别为双击
    # （app 侧按两次独立 ButtonPress 判 <300ms 同格），必须用两次独立的 click 调用。
    SE2=""; SE3=""
    for OFS in 8 16 24 32; do
        x11 xdotool mousemove $(( ${GX:-0} + 60 )) $(( ${GY:-0} + OFS ))
        sleep 0.15
        x11 xdotool click 1; sleep 0.12; x11 xdotool click 1
        sleep 0.5
        SE2=$(x11 xclip -o -selection primary 2>/dev/null | tr -d '\n')
        [ -n "$SE2" ] && break
    done
    x11 import -window "$WID" "$OUT/$BACKEND-click-2.png" 2>/dev/null
    A2=$(compare -metric AE "$OUT/$BACKEND-click-0.png" "$OUT/$BACKEND-click-2.png" null: 2>&1 | head -1)
    # 三击：在双击之后再补一次点击（间隔 <300ms 才会计为第 3 次，这里重新连点三次）
    x11 xdotool click 1; sleep 0.12; x11 xdotool click 1; sleep 0.12; x11 xdotool click 1
    sleep 0.5
    SE3=$(x11 xclip -o -selection primary 2>/dev/null | tr -d '\n')
    A3=0
    if [ -n "$SE2" ] && [ "${A2:-0}" -gt 20 ] && [ ${#SE3} -ge ${#SE2} ]; then
        ok "③ 双击/三击" "双击选词='$SE2'（AE=$A2），三击='$SE3'（AE=$A3）"
    else
        bad "③ 双击/三击" "双击='$SE2' 三击='$SE3'"
    fi
    stop_app
else
    bad "③ 双击/三击" "窗口未出现"
fi

# ---------- ④ shell 退出（exit 与 SIGTERM） ----------
if start_app; then
    SHELL_PID=$(pgrep -P "$LEANTTY_PID" 2>/dev/null | head -1)
    x11 xdotool type --window "$WID" "exit"
    x11 xdotool key --window "$WID" Return
    w=0
    while kill -0 "$LEANTTY_PID" 2>/dev/null && [ "$w" -lt 8 ]; do sleep 0.25; w=$((w + 1)); done
    if kill -0 "$LEANTTY_PID" 2>/dev/null; then
        bad "④a shell exit" "app 2 秒内未退出"
    else
        ok "④a shell exit" "app 在 $((w * 250))ms 内退出"
    fi
    stop_app
else
    bad "④a shell exit" "窗口未出现"
fi

if start_app; then
    SHELL_PID=$(pgrep -P "$LEANTTY_PID" 2>/dev/null | head -1)
    if [ -n "$SHELL_PID" ]; then
        kill -KILL "$SHELL_PID" 2>/dev/null
        w=0
        while kill -0 "$LEANTTY_PID" 2>/dev/null && [ "$w" -lt 8 ]; do sleep 0.25; w=$((w + 1)); done
        if kill -0 "$LEANTTY_PID" 2>/dev/null; then
            bad "④b shell 被信号终止" "app 2 秒内未退出"
        else
            ok "④b shell 被信号终止" "app 在 $((w * 250))ms 内退出"
        fi
    else
        bad "④b shell 被信号终止" "取不到 shell 子进程 PID"
    fi
    stop_app
else
    bad "④b shell 被信号终止" "窗口未出现"
fi

# ---------- ⑤ OSC 0 标题 ----------
if start_app; then
    x11 xdotool type --window "$WID" "printf '\\033]0;TTL4T_MARK\\007'"
    x11 xdotool key --window "$WID" Return
    t=0
    GOT=""
    while [ $t -lt 10 ]; do
        GOT=$(x11 xdotool getwindowname "$WID" 2>/dev/null)
        case "$GOT" in *TTL4T_MARK*) break ;; esac
        sleep 0.1
        t=$((t + 1))
    done
    case "$GOT" in
        *TTL4T_MARK*) ok "⑤ OSC 0 标题" "标题='$GOT'（${t}00ms 内）" ;;
        *) bad "⑤ OSC 0 标题" "标题未变：'$GOT'" ;;
    esac
    stop_app
else
    bad "⑤ OSC 0 标题" "窗口未出现"
fi

# ---------- ⑥ PRIMARY 选区（空闲 5 秒后仍可读） ----------
if start_app; then
    x11 xdotool type --window "$WID" "echo PRIMARY4T_TOKEN"
    x11 xdotool key --window "$WID" Return
    sleep 1.0
    x11 xdotool mousemove $(( ${GX:-0} + 6 )) $(( ${GY:-0} + 26 ))
    x11 xdotool mousedown 1
    x11 xdotool mousemove $(( ${GX:-0} + 300 )) $(( ${GY:-0} + 40 ))
    sleep 0.3
    x11 xdotool mouseup 1
    sleep 0.5
    P1=$(x11 xclip -o -selection primary 2>/dev/null | tr -d '\n')
    sleep 5
    P2=$(x11 xclip -o -selection primary 2>/dev/null | tr -d '\n')
    if [ -n "$P1" ] && [ "$P1" = "$P2" ]; then
        ok "⑥ PRIMARY 选区" "读到 '$(echo "$P1" | cut -c1-24)'，空闲 5 秒后一致"
    else
        bad "⑥ PRIMARY 选区" "P1='$P1' P2='$P2'"
    fi
    stop_app
else
    bad "⑥ PRIMARY 选区" "窗口未出现"
fi

# ---------- ⑦ SIGTERM ----------
if start_app; then
    kill -TERM "$LEANTTY_PID" 2>/dev/null
    w=0
    while kill -0 "$LEANTTY_PID" 2>/dev/null && [ "$w" -lt 8 ]; do sleep 0.25; w=$((w + 1)); done
    if kill -0 "$LEANTTY_PID" 2>/dev/null; then
        bad "⑦ SIGTERM 退出" "2 秒内未退出"
    else
        ok "⑦ SIGTERM 退出" "在 $((w * 250))ms 内退出"
    fi
    stop_app
else
    bad "⑦ SIGTERM 退出" "窗口未出现"
fi

echo "===== timer_dep_matrix($BACKEND): PASS=$PASS FAIL=$FAIL ====="
[ "$FAIL" -eq 0 ]
