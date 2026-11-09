#!/bin/sh
# e2e 验收（阶段 B）：Xvfb + openbox + app + xdotool，断言经 APP_DUMP_SNAPSHOT 文本钩子。
# 一条命令跑全部用例；失败时保留 dump 与截图。变量全部来自环境/脚本位置推导。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$ROOT/scripts/e2e/out"
mkdir -p "$OUT"
DISPLAY_NO=99
APP_BIN="$ROOT/target/release/app"

for tool in Xvfb xdotool openbox import wmctrl; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FATAL: $tool 未安装"; exit 1; }
done
[ -x "$APP_BIN" ] || { echo "FATAL: $APP_BIN 不存在，请先 sh scripts/build-app.sh"; exit 1; }

Xvfb :$DISPLAY_NO -screen 0 1280x720x24 >"$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
openbox >"$OUT/openbox.log" 2>&1 &
WM_PID=$!
trap 'kill $XVFB_PID $WM_PID ${APP_PID:-} 2>/dev/null; exit 0' EXIT INT TERM
sleep 1
export DISPLAY=:$DISPLAY_NO

PASS=0; FAIL=0
WID=""; APP_PID=""

start_app() { # $1 = dump 路径
    APP_DUMP_SNAPSHOT="$1" SHELL=/bin/sh "$APP_BIN" >>"$OUT/app-run.log" 2>&1 &
    APP_PID=$!
    local i=0
    WID=""
    while [ $i -lt 20 ]; do
        WID=$(xdotool search --name "tty" 2>/dev/null | head -1)
        [ -n "$WID" ] && break
        sleep 0.5
        i=$((i+1))
    done
    if [ -z "$WID" ]; then
        echo "FAIL start_app: 未找到 tty 窗口"; FAIL=$((FAIL+1))
        return 1
    fi
    xdotool windowactivate "$WID" >/dev/null 2>&1
    xdotool windowfocus "$WID" >/dev/null 2>&1
    return 0
}

stop_app() {
    [ -n "$APP_PID" ] && kill $APP_PID 2>/dev/null
    [ -n "$APP_PID" ] && wait $APP_PID 2>/dev/null
    APP_PID=""
}

# 等 dump 文件出现 pattern（最多 5s）
wait_dump() { # $1 文件 $2 pattern
    local i=0
    while [ $i -lt 10 ]; do
        grep -q "$2" "$1" 2>/dev/null && return 0
        sleep 0.5
        i=$((i+1))
    done
    return 1
}

assert_dump() { # $1 用例名 $2 文件 $3 pattern
    if wait_dump "$2" "$3"; then
        echo "PASS $1"; PASS=$((PASS+1))
    else
        echo "FAIL $1 (pattern: $3)"
        FAIL=$((FAIL+1))
        cp "$2" "$OUT/$1-fail.dump" 2>/dev/null
        import -window root "$OUT/$1-fail.png" 2>/dev/null
    fi
}

assert_app_exits() { # $1 用例名（APP_PID 须全局）
    local i=0 alive=1
    while [ $i -lt 10 ]; do
        kill -0 $APP_PID 2>/dev/null || { alive=0; break; }
        sleep 0.5
        i=$((i+1))
    done
    if [ $alive -eq 0 ]; then
        echo "PASS $1"; PASS=$((PASS+1))
    else
        echo "FAIL $1 (app 5s 内未退出)"
        FAIL=$((FAIL+1))
    fi
    stop_app
}

# 用例 1：echo hello_e2e 回显
D="$OUT/case1-hello.dump"
if start_app "$D"; then
    xdotool type --window "$WID" "echo hello_e2e"
    xdotool key --window "$WID" Return
    assert_dump "case1-hello_e2e" "$D" "hello_e2e"
    stop_app
fi

# 用例 2：中文回显 + 宽格标记（xdotool type UTF-8 不可靠时改 printf 路径）
D="$OUT/case2-cn.dump"
if start_app "$D"; then
    xdotool type --window "$WID" "echo 你好"
    xdotool key --window "$WID" Return
    if wait_dump "$D" "你好"; then
        echo "PASS case2-cn (xdotool type UTF-8)"; PASS=$((PASS+1))
    else
        xdotool type --window "$WID" "printf '\\344\\275\\240\\345\\245\\275'"
        xdotool key --window "$WID" Return
        if wait_dump "$D" "你好"; then
            echo "PASS case2-cn (printf 八进制回退)"; PASS=$((PASS+1))
        else
            echo "FAIL case2-cn (无 你好 输出)"; FAIL=$((FAIL+1))
            cp "$D" "$OUT/case2-cn-fail.dump" 2>/dev/null
        fi
    fi
    # 宽格标记：summary 行 wide>=2（你/好 各占 Wide）
    W=$(grep -o 'wide=[0-9]*' "$D" 2>/dev/null | head -1 | cut -d= -f2)
    if [ -n "$W" ] && [ "$W" -ge 2 ] 2>/dev/null; then
        echo "PASS case2-wide-marker (wide=$W)"; PASS=$((PASS+1))
    else
        echo "FAIL case2-wide-marker (wide=$W)"; FAIL=$((FAIL+1))
    fi
    stop_app
fi

# 用例 3：abc + Backspace + d 回车 → abd
D="$OUT/case3-backspace.dump"
if start_app "$D"; then
    xdotool type --window "$WID" "echo abc"
    xdotool key --window "$WID" BackSpace
    xdotool type --window "$WID" "d"
    xdotool key --window "$WID" Return
    assert_dump "case3-backspace-abd" "$D" "abd"
    stop_app
fi

# 用例 4：Ctrl-C 中断 sleep 100 后再 echo 标记串
D="$OUT/case4-ctrlc.dump"
if start_app "$D"; then
    xdotool type --window "$WID" "sleep 100"
    xdotool key --window "$WID" Return
    sleep 1
    xdotool key --window "$WID" ctrl+c
    sleep 1
    xdotool type --window "$WID" "echo MARK_CTRLC"
    xdotool key --window "$WID" Return
    assert_dump "case4-ctrlc" "$D" "MARK_CTRLC"
    stop_app
fi

# 用例 5：Ctrl-D 退出 shell 后 app 5s 内退出
D="$OUT/case5-ctrld.dump"
if start_app "$D"; then
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case5-ctrl-d-app-exits"
fi

# 用例 6：关闭窗口（发 WM_DELETE 协议消息）后 app 5s 内退出
# xdotool windowclose 实测直接 XDestroyWindow 不发协议消息（xev 无 ClientMessage），
# 故用 wmctrl -c 发 WM_DELETE_WINDOW（winit 正常派发 CloseRequested）。
D="$OUT/case6-close.dump"
if start_app "$D"; then
    wmctrl -i -c "$WID"
    assert_app_exits "case6-windowclose-app-exits"
fi

# 用例 7：resize 双侧一致（窗口 640x480 像素 → 80x30 格；stty size 一致）
D="$OUT/case7-resize.dump"
if start_app "$D"; then
    xdotool windowsize "$WID" 640 480
    assert_dump "case7-resize-cols" "$D" "cols=80"
    assert_dump "case7-resize-rows" "$D" "rows=30"
    xdotool type --window "$WID" "stty size"
    xdotool key --window "$WID" Return
    assert_dump "case7-stty-size" "$D" "30 80"
    stop_app
fi

# 用例 8：vim/less 交替屏（系统有 vim 或 less 时执行，否则跳过并说明）
if command -v vim >/dev/null 2>&1; then
    D="$OUT/case8-alt.dump"
    if start_app "$D"; then
        xdotool type --window "$WID" "vim"
        xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-on" "$D" "alternate=true"
        xdotool key --window "$WID" Escape
        xdotool type --window "$WID" ":q"
        xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-off" "$D" "alternate=false"
        stop_app
    fi
elif command -v less >/dev/null 2>&1; then
    D="$OUT/case8-alt.dump"
    if start_app "$D"; then
        xdotool type --window "$WID" "less"
        xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-on" "$D" "alternate=true"
        xdotool key --window "$WID" q
        assert_dump "case8-alt-screen-off" "$D" "alternate=false"
        stop_app
    fi
else
    echo "SKIP case8-alt-screen (vim/less 均未安装，无法构造交替屏)"
fi

echo "===== e2e 汇总: PASS=$PASS FAIL=$FAIL ====="
[ "$FAIL" -eq 0 ]
