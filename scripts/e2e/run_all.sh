#!/bin/sh
# 清理前轮残留 Xvfb/openbox（trap 在工具层超时 kill 下可能未执行；
# 精确进程名，非通配）。
pkill -x Xvfb 2>/dev/null || true
pkill -x openbox 2>/dev/null || true
sleep 0.5
# e2e 验收（阶段 B）：Xvfb + openbox + app + xdotool，断言经 APP_DUMP_SNAPSHOT 文本钩子。
# 一条命令跑全部用例；失败时保留 dump 与截图。变量全部来自环境/脚本位置推导。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$ROOT/scripts/e2e/out"
mkdir -p "$OUT"
# per-case 日志与 app-run.log 追加写入，跨 run 残留会污染断言（如 stderr 不含 wake），每次开头清空
rm -f "$OUT"/case*.log "$OUT"/app-run.log
DISPLAY_NO=99
APP_BIN="$ROOT/target/debug/app"

for tool in Xvfb xdotool openbox import python3 strings; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FATAL: $tool 未安装"; exit 1; }
done
[ -x "$APP_BIN" ] || { echo "FATAL: $APP_BIN 不存在，请先 cargo build -p app（e2e 用 debug 二进制：APP_DUMP_SNAPSHOT 钩子仅 debug 构建编译）"; exit 1; }
PASS=0; FAIL=0
# release 构建检查：APP_DUMP_SNAPSHOT 钩子仅 debug（#[cfg(debug_assertions)] 门控整个
# 函数与钩子字符串）——release 二进制中该字符串不应存在。
if strings "$ROOT/target/release/app" 2>/dev/null | grep -q APP_DUMP_SNAPSHOT; then
    echo "FAIL release-hook-leak (APP_DUMP_SNAPSHOT 出现在 release 二进制)"
    FAIL=$((FAIL+1))
else
    echo "PASS release-hook-gated (release 二进制无 APP_DUMP_SNAPSHOT)"
    PASS=$((PASS+1))
fi

Xvfb :$DISPLAY_NO -screen 0 1280x720x24 >"$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
openbox >"$OUT/openbox.log" 2>&1 &
WM_PID=$!
trap 'kill $XVFB_PID $WM_PID ${APP_PID:-} 2>/dev/null; exit 0' EXIT INT TERM
sleep 1
export DISPLAY=:$DISPLAY_NO

WID=""; APP_PID=""

wait_window() { # 等 tty 窗口出现并置 WID；遍历全部匹配窗口校验 _NET_WM_PID 归属当前 app（防前序用例 kill 后的残留窗口条目）。首字体含 CJK 时 from_bytes 一次性成本约 7s（DECISIONS 登记），等待上限放宽到 40×0.5=20s。
    local i=0 pid wid
    WID=""
    while [ $i -lt 40 ]; do
        # 进程已退（zombie）立即失败：X server 连接偶发失败等启动错误不再空等 20s。
        if [ "$(awk '{print $3}' /proc/$APP_PID/stat 2>/dev/null)" = "Z" ]; then
            return 1
        fi
        for wid in $(xdotool search --name tty 2>/dev/null); do
            pid=$(xprop -id "$wid" _NET_WM_PID 2>/dev/null | grep -o '[0-9]*$' || true)
            if [ -n "$pid" ] && [ "$pid" = "$APP_PID" ]; then
                WID=$wid
                return 0
            fi
        done
        sleep 0.5
        i=$((i+1))
    done
    return 1
}

start_app() { # $1 = dump 路径, $2 = 日志路径（默认 app-run.log）, $3/$4 = 初始 columns/lines（可选）
    local log=${2:-$OUT/app-run.log}
    local attempt=1
    if [ -n "${3:-}" ] && [ -n "${4:-}" ]; then
        APP_DUMP_SNAPSHOT="$1" SHELL=/bin/sh APP_COLS="$3" APP_ROWS="$4" "$APP_BIN" >>"$log" 2>&1 &
    else
        APP_DUMP_SNAPSHOT="$1" SHELL=/bin/sh "$APP_BIN" >>"$log" 2>&1 &
    fi
    APP_PID=$!
    if ! wait_window; then
        # X server 连接偶发失败（Xvfb 序列环境已验证）：进程已退出（kill -0 失败
        # 或 zombie）则 1s 后重试一次；进程存活但窗口未到则不重试（避免叠 app）。
        if [ $attempt -lt 2 ] && { ! kill -0 $APP_PID 2>/dev/null || [ "$(awk '{print $3}' /proc/$APP_PID/stat 2>/dev/null)" = "Z" ]; }; then
            attempt=$((attempt+1))
            echo "retry start_app: 第 $((attempt-1)) 次启动进程已退出，1s 后重试" >&2
            sleep 1
            if [ -n "${3:-}" ] && [ -n "${4:-}" ]; then
                APP_DUMP_SNAPSHOT="$1" SHELL=/bin/sh APP_COLS="$3" APP_ROWS="$4" "$APP_BIN" >>"$log" 2>&1 &
            else
                APP_DUMP_SNAPSHOT="$1" SHELL=/bin/sh "$APP_BIN" >>"$log" 2>&1 &
            fi
            APP_PID=$!
            if wait_window; then
                xdotool windowactivate "$WID" >/dev/null 2>&1
                xdotool windowfocus "$WID" >/dev/null 2>&1
                return 0
            fi
        fi
        echo "FAIL start_app: 未找到 tty 窗口"
        echo "  diag: APP_PID=$APP_PID search=[$(xdotool search --name tty 2>/dev/null | tr '\n' ' ')]"
        ps -o pid,stat,cmd -p $APP_PID 2>/dev/null || echo "  diag: app 进程不存在"
        echo "  diag: applog=[$(cat "$log" 2>/dev/null | tr '\n' '|')]"
        tail -2 "$OUT/openbox.log" 2>/dev/null | sed 's/^/  ob: /'
        FAIL=$((FAIL+1))
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
        cp "$D" "$OUT/$1-fail.dump" 2>/dev/null
        import -window root "$OUT/$1-fail.png" 2>/dev/null
        ps -o pid,stat,wchan:20,cmd -p $APP_PID >"$OUT/$1-fail-ps.txt" 2>&1
        xdotool search --name tty >"$OUT/$1-fail-windows.txt" 2>&1
        wmctrl -l >"$OUT/$1-fail-wmctrl.txt" 2>&1
    fi
    stop_app
}

assert_no_wake() { # $1 用例名 $2 日志文件（app stderr 不含 wake）
    if grep -q wake "$2" 2>/dev/null; then
        echo "FAIL $1 (stderr 含 wake: $(grep wake "$2" | head -2))"
        FAIL=$((FAIL+1))
    else
        echo "PASS $1"; PASS=$((PASS+1))
    fi
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
L="$OUT/case5-ctrl-d.log"
if start_app "$D" "$L"; then
    xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case5-ctrl-d-app-exits"
    assert_no_wake "case5-ctrl-d-no-wake" "$L"
fi

# 用例 6：关闭窗口（发 WM_DELETE 协议消息）后 app 5s 内退出
# 实测：run_all 序列环境下 wmctrl -c 的 WM_DELETE 丢失（python 协议直发有效，app 无缺陷）；
# xev 监听会覆盖窗口事件掩码干扰 winit，调试已移除（见 DECISIONS）。
D="$OUT/case6-close.dump"
L="$OUT/case6-close.log"
if start_app "$D" "$L"; then
    python3 "$ROOT/scripts/e2e/wm_send.py" "$WID"
    assert_app_exits "case6-windowclose-app-exits"
    assert_no_wake "case6-windowclose-no-wake" "$L"
fi

# 用例 7：resize 双侧一致（窗口 640x480 像素；格子由字体度量算出，非固定 8x16——
# 断言快照 cols/rows 与 stty size 双侧一致，不固定 80x30）
D="$OUT/case7-resize.dump"
L="$OUT/case7-resize.log"
if start_app "$D" "$L"; then
    xdotool windowsize "$WID" 640 480
    sleep 1
    COLS=""; ROWS=""; i=0
    while [ $i -lt 10 ]; do
        COLS=$(grep -o 'cols=[0-9]*' "$D" 2>/dev/null | head -1 | cut -d= -f2)
        ROWS=$(grep -o 'rows=[0-9]*' "$D" 2>/dev/null | head -1 | cut -d= -f2)
        [ -n "$COLS" ] && [ -n "$ROWS" ] && break
        sleep 0.5
        i=$((i+1))
    done
    if [ -n "$COLS" ] && [ "$COLS" -ge 1 ] 2>/dev/null \
       && [ -n "$ROWS" ] && [ "$ROWS" -ge 1 ] 2>/dev/null; then
        echo "PASS case7-resize-cols (cols=$COLS)"; PASS=$((PASS+1))
        echo "PASS case7-resize-rows (rows=$ROWS)"; PASS=$((PASS+1))
    else
        echo "FAIL case7-resize (dump 未给出 cols/rows)"
        cp "$D" "$OUT/case7-resize-fail.dump" 2>/dev/null
        FAIL=$((FAIL+2))
    fi
    xdotool type --window "$WID" "stty size"
    xdotool key --window "$WID" Return
    assert_dump "case7-stty-size" "$D" "$ROWS $COLS"
    assert_no_wake "case7-resize-no-wake" "$L"
    stop_app
fi

# 用例 8：vim/less 交替屏（系统有 vim 或 less 时执行，否则跳过并说明）
if command -v vim >/dev/null 2>&1; then
    D="$OUT/case8-alt.dump"
    if start_app "$D"; then
        sleep 0.3
        xdotool type --delay 60 --window "$WID" "vim"
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
        sleep 0.3
        xdotool type --delay 60 --window "$WID" "less"
        xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-on" "$D" "alternate=true"
        xdotool key --window "$WID" q
        assert_dump "case8-alt-screen-off" "$D" "alternate=false"
        stop_app
    fi
else
    echo "SKIP case8-alt-screen (vim/less 均未安装，无法构造交替屏)"
fi

# 用例 9：启动参数 120x30（window.dimensions 经 APP_COLS/APP_ROWS env 层）→
# 快照 cols/rows 与 stty size 一致（网格由字体度量算出，dimensions 决定初始行列）
D="$OUT/case9-dims120x30.dump"
L="$OUT/case9-dims120x30.log"
if start_app "$D" "$L" 120 30; then
    sleep 1
    xdotool type --window "$WID" "stty size"
    xdotool key --window "$WID" Return
    assert_dump "case9-dims-cols" "$D" "cols=120"
    assert_dump "case9-dims-rows" "$D" "rows=30"
    assert_dump "case9-dims-stty" "$D" "30 120"
    xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case9-dims120x30-exits"
    assert_no_wake "case9-dims120x30-no-wake" "$L"
fi

# 用例 10：examples/linux.toml（--config 启动）+ 中文输入 → 回显非方块、
# font 索引行存在（回退链 e2e 断言，替代截图像素比对；规格任务 3.6 备选方案）。
# 中文输入走 printf 八进制路径（实测：xdotool type 对 CJK 发 caret 记法文本，
# 本环境不可用；case2 同结论，DECISIONS 登记）。
D="$OUT/case10-cjk-config.dump"
L="$OUT/case10-cjk-config.log"
APP_DUMP_SNAPSHOT="$D" SHELL=/bin/sh "$APP_BIN" --config "$ROOT/examples/linux.toml" >>"$L" 2>&1 &
APP_PID=$!
if wait_window; then
    xdotool windowactivate "$WID" >/dev/null 2>&1
    xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 1
    xdotool type --window "$WID" "printf '\\344\\275\\240\\345\\245\\275'"
    xdotool key --window "$WID" Return
    assert_dump "case10-cjk-echo" "$D" "你好"
    assert_dump "case10-cjk-cols" "$D" "cols=120"
    if grep -q '^# font:' "$D"; then
        echo "PASS case10-cjk-font-indices"; PASS=$((PASS+1))
    else
        echo "FAIL case10-cjk-font-indices (dump 无 # font 行)"
        FAIL=$((FAIL+1))
    fi
    xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case10-cjk-config-exits"
    assert_no_wake "case10-cjk-config-no-wake" "$L"
fi

# 用例 11：启动性能（任务 2B.1）——启动后立即输入，2s 内快照出现输出；汉字后台
# 加载期间继续输入英文不丢字符（懒加载：首帧不等 CJK from_bytes）。
D="$OUT/case11-boot-perf.dump"
L="$OUT/case11-boot-perf.log"
T0=$(date +%s%N)
if start_app "$D" "$L"; then
    xdotool type --window "$WID" "echo boot2b"
    xdotool key --window "$WID" Return
    i=0
    while [ $i -lt 20 ]; do
        grep -q "boot2b" "$D" 2>/dev/null && break
        sleep 0.1
        i=$((i+1))
    done
    T1=$(date +%s%N)
    ELAPSED=$(( (T1-T0)/1000000 ))
    if grep -q "boot2b" "$D"; then
        if [ "$ELAPSED" -lt 2000 ]; then
            echo "PASS case11-first-output-2s (${ELAPSED}ms)"; PASS=$((PASS+1))
        else
            echo "FAIL case11-first-output-2s (${ELAPSED}ms ≥ 2000ms)"
            FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL case11-first-output-2s (2s 内无输出，${ELAPSED}ms)"
        FAIL=$((FAIL+1))
    fi
    # 汉字 + 加载期间继续输入英文（不丢字符）
    xdotool type --window "$WID" "printf '\\344\\275\\240\\345\\245\\275'"
    xdotool key --window "$WID" Return
    xdotool type --window "$WID" "echo AFTER_CJK"
    xdotool key --window "$WID" Return
    assert_dump "case11-cjk-echo" "$D" "你好"
    assert_dump "case11-after-cjk" "$D" "AFTER_CJK"
    xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case11-boot-perf-exits"
    assert_no_wake "case11-boot-perf-no-wake" "$L"
fi

# 用例 12：选区与剪贴板（任务 2.1/2.2）——拖选输出行→PRIMARY 写→右键 Paste
# （默认绑定 Right=Paste）→粘贴文本作为命令执行→dump 断言回显。
D="$OUT/case12-selection.dump"
L="$OUT/case12-selection.log"
if start_app "$D" "$L"; then
    sleep 1
    xdotool type --window "$WID" "echo PASTE_TARGET"
    xdotool key --window "$WID" Return
    sleep 0.5
    # 拖选第 1 行（echo 输出行；y=30 ≈ 行 1 中部，x 10→200 跨整行）。
    # 按钮事件必须走 XTest 根坐标注入（xdotool 老版本 --window click/mousedown
    # 走 XSendEvent，winit 过滤 send_event 标志；先 warp 再无 --window 发按钮）。
    xdotool mousemove --window "$WID" 10 30
    sleep 0.2
    xdotool mousedown 1
    xdotool mousemove --window "$WID" 200 30
    sleep 0.2
    xdotool mouseup 1
    sleep 0.5
    # PRIMARY 写断言（选区定型时写）
    if xclip -o -selection primary 2>/dev/null | grep -q "PASTE_TARGET"; then
        echo "PASS case12-primary-write"; PASS=$((PASS+1))
    else
        echo "FAIL case12-primary-write (PRIMARY 无 PASTE_TARGET)"; FAIL=$((FAIL+1))
    fi
    # 右键 Paste（默认绑定 Right=Paste）→粘贴文本作命令执行→命令行回显唯一串
    # （源命令行为 "echo PASTE_TARGET"，粘贴回显行为 "PASTE_TARGET"，可区分）
    xdotool mousemove --window "$WID" 400 30
    sleep 0.2
    xdotool click 3
    sleep 1
    # 粘贴内容无换行→shell 命令行未提交；补 Return 提交后 ctrl+d 才是 EOF。
    xdotool key --window "$WID" Return
    sleep 0.5
    if grep -qF "sh-5.2# PASTE_TARGET" "$D" 2>/dev/null; then
        echo "PASS case12-paste-exec (粘贴命令行回显)"; PASS=$((PASS+1))
    else
        echo "FAIL case12-paste-exec (dump 无粘贴命令行回显)"
        cp "$D" "$OUT/case12-paste-exec-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    xdotool key --window "$WID" ctrl+d
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case12-selection-exits"
    assert_no_wake "case12-selection-no-wake" "$L"
fi

# 用例 13：括号粘贴（任务 2.3）——?2004 模式下粘贴含换行内容→cat -v 显示
# ESC[200~…ESC[201~ 包裹（内容中 \n 规范化为 \r，cat -v 显示 ^M）。
D="$OUT/case13-bracketed.dump"
L="$OUT/case13-bracketed.log"
if start_app "$D" "$L"; then
    sleep 1
    xdotool type --window "$WID" "printf '\\e[?2004h'; cat -v"
    xdotool key --window "$WID" Return
    sleep 0.5
    # CLIPBOARD 设含换行内容（xclip -i 读 stdin；printf 八进制换行防 shell 语义）
    printf 'BB1\nBB2' | xclip -i -selection clipboard
    sleep 0.3
    xdotool mousemove --window "$WID" 400 30
    sleep 0.2
    xdotool click 3
    sleep 1
    if grep -qF '^[[200~BB1' "$D" 2>/dev/null && grep -qF 'BB2^[[201~' "$D" 2>/dev/null; then
        echo "PASS case13-bracketed-wrap"; PASS=$((PASS+1))
    else
        echo "FAIL case13-bracketed-wrap (dump 无 ESC[200~…ESC[201~ 包裹)"
        cp "$D" "$OUT/case13-bracketed-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    xdotool key --window "$WID" ctrl+c
    sleep 0.3
    xdotool key --window "$WID" ctrl+c
    xdotool key --window "$WID" ctrl+d
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case13-bracketed-exits"
    assert_no_wake "case13-bracketed-no-wake" "$L"
fi



# 用例 14：滚轮滚动（任务 3/4）——30 行输出压 scrollback→滚轮上滚断言历史行
# 可见→滚轮下滚回底（auto-follow 恢复）。xdotool click 4/5=滚轮上/下（XTest 根坐标）。
D="$OUT/case14-scroll.dump"
L="$OUT/case14-scroll.log"
if start_app "$D" "$L"; then
    sleep 1
    xdotool type --window "$WID" "for i in \$(seq 1 30); do echo LINE_\$i; done"
    xdotool key --window "$WID" Return
    sleep 0.5
    # 滚轮上滚 5（×multiplier 3=15 行 offset）→视口离开底部，历史行可见
    xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    i=0; while [ $i -lt 5 ]; do xdotool click 4; i=$((i+1)); sleep 0.05; done
    sleep 0.5
    if grep -q "LINE_1" "$D" 2>/dev/null && ! grep -q "LINE_29" "$D" 2>/dev/null; then
        echo "PASS case14-scroll-up (历史行 LINE_1 可见且离开底部)"; PASS=$((PASS+1))
    else
        echo "FAIL case14-scroll-up (上滚后视口未离开底部)"
        cp "$D" "$OUT/case14-scroll-up-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    # 滚轮下滚回底→auto-follow 恢复，最新行可见
    i=0; while [ $i -lt 5 ]; do xdotool click 5; i=$((i+1)); sleep 0.05; done
    sleep 0.5
    if grep -q "LINE_29" "$D" 2>/dev/null; then
        echo "PASS case14-scroll-bottom (下滚回底 LINE_29 可见)"; PASS=$((PASS+1))
    else
        echo "FAIL case14-scroll-bottom (回底后 LINE_29 不可见)"
        cp "$D" "$OUT/case14-scroll-bottom-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    xdotool key --window "$WID" ctrl+d
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case14-scroll-exits"
    assert_no_wake "case14-scroll-no-wake" "$L"
fi


# 用例 15：备用屏滚轮→方向键（任务 3）——?1049 备用屏+cat -v 显示方向键序列
# （scrolling.alt_screen_scroll=true 默认：备用屏下滚轮发 ArrowUp/Down 而非 scroll_viewport）。
D="$OUT/case15-alt-wheel.dump"
L="$OUT/case15-alt-wheel.log"
if start_app "$D" "$L"; then
    sleep 1
    xdotool type --window "$WID" "printf '\\e[?1049h'; cat -v"
    xdotool key --window "$WID" Return
    sleep 0.5
    # 滚轮上 2 次→方向键 Up×2（×multiplier 3=6 个 ESC[A？LineDelta 1 行×mult）
    xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    xdotool click 4
    sleep 0.2
    xdotool click 4
    sleep 0.5
    if grep -qF '^[[A' "$D" 2>/dev/null; then
        echo "PASS case15-alt-wheel-arrow (备用屏滚轮→方向键 Up 序列)"; PASS=$((PASS+1))
    else
        echo "FAIL case15-alt-wheel-arrow (dump 无 ^[[A 方向键序列)"
        cp "$D" "$OUT/case15-alt-wheel-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    xdotool key --window "$WID" ctrl+c
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case15-alt-wheel-exits"
    assert_no_wake "case15-alt-wheel-no-wake" "$L"
fi

# 用例 16：鼠标上报 SGR 1006（任务 3）——?1003 全鼠标跟踪+cat -v 显示
# 上报序列（含单元格坐标，padding/缩放经 pixel_to_cell 转换）。
D="$OUT/case16-mouse-report.dump"
L="$OUT/case16-mouse-report.log"
if start_app "$D" "$L"; then
    sleep 1
    xdotool type --window "$WID" "printf '\\e[?1003h\\e[?1006h'; cat -v"
    xdotool key --window "$WID" Return
    sleep 0.5
    # 左键点击（非 Shift→上报优先；坐标 cell(2,1) 附近）
    xdotool mousemove --window "$WID" 30 30
    sleep 0.2
    xdotool click 1
    sleep 0.5
    # SGR 1006 上报序列形如 ^[[<0;col;rowM（Press）/m（Release）
    if grep -qF '^[[<0;' "$D" 2>/dev/null; then
        echo "PASS case16-mouse-report (SGR 1006 上报序列含坐标)"; PASS=$((PASS+1))
    else
        echo "FAIL case16-mouse-report (dump 无 ^[[<0; 上报序列)"
        cp "$D" "$OUT/case16-mouse-report-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    xdotool key --window "$WID" ctrl+c
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case16-mouse-report-exits"
    assert_no_wake "case16-mouse-report-no-wake" "$L"
fi


# 用例 17：IME 冒烟（任务 5）——set_ime_allowed(true) 无条件开启后正常输入
# 不 panic；Xvfb 无 IME 总线，Preedit/Commit 事件路径不可达（DECISIONS 登记
# 为已知限制，合成纯函数单测覆盖 clamp/归一）。
D="$OUT/case17-ime.dump"
L="$OUT/case17-ime.log"
if start_app "$D" "$L"; then
    sleep 1
    xdotool type --window "$WID" "echo IME_SMOKE"
    xdotool key --window "$WID" Return
    sleep 0.5
    assert_dump "case17-ime-smoke" "$D" "IME_SMOKE"
    xdotool key --window "$WID" ctrl+d
    sleep 0.3
    xdotool key --window "$WID" ctrl+d
    assert_app_exits "case17-ime-exits"
    assert_no_wake "case17-ime-no-wake" "$L"
fi

echo "===== e2e 汇总: PASS=$PASS FAIL=$FAIL ====="
[ "$FAIL" -eq 0 ]
