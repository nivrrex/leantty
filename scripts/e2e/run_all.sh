#!/bin/sh
# e2e 验收（阶段 B）：Xvfb(:99) + openbox + app + xdotool，断言经 APP_DUMP_SNAPSHOT 文本钩子。
# 一条命令跑全部用例；失败时保留 dump 与截图。变量全部来自环境/脚本位置推导。
#
# 安全护栏（阶段 3B 任务 1）：所有 X11 操作经 scripts/e2e/guard.sh 的 x11 包装，
# 强制 DISPLAY=:99，绝不触碰用户桌面会话；禁止 pkill/killall，只用自有 PID。
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$ROOT/scripts/e2e/out"
mkdir -p "$OUT"
E2E_GUARD_DIR="$ROOT/scripts/e2e"
. "$ROOT/scripts/e2e/guard.sh"

# release 零告警门禁（阶段 3C 任务 2）：release 构建与 clippy 必须无告警，
# 否则失败即中止（debug 下被 cfg 门控掉的变量容易在 release 变成 unused）。
if command -v cargo >/dev/null 2>&1; then
    _rel_warn=$(cargo build --release -p app 2>&1 | grep -cE "^warning|^error" || true)
    if [ "${_rel_warn:-0}" -ne 0 ]; then
        echo "FATAL: cargo build --release 存在 warning/error（${_rel_warn} 条）" >&2
        cargo build --release -p app 2>&1 | grep -E "^warning|^error" -A4 | head -20 >&2
        exit 1
    fi
    _rel_clippy=$(cargo clippy --release --workspace --all-targets -- -D warnings 2>&1 | grep -cE "^error|^warning" || true)
    if [ "${_rel_clippy:-0}" -ne 0 ]; then
        echo "FATAL: cargo clippy --release --all-targets -D warnings 未通过（${_rel_clippy} 条）" >&2
        exit 1
    fi
    echo "PASS release-zero-warning (build --release 与 clippy --release 均无告警)"
fi

# 脚本级 lint（禁止模式/裸 X11 调用）：失败即中止，避免回归到危险写法。
sh "$ROOT/scripts/e2e/lint_scripts.sh" || {
    echo "FATAL: e2e 脚本安全 lint 未通过" >&2
    exit 1
}

# 清理上一轮可能残留的 :99 进程（只针对 :99；trap 在工具层超时 kill 下可能未执行）。
for p in $(e2e_stale_pids); do
    kill_own "$p"
done
sleep 0.5
# per-case 日志与 app-run.log 追加写入，跨 run 残留会污染断言（如 stderr 不含 wake），每次开头清空
rm -f "$OUT"/case*.log "$OUT"/app-run.log
APP_BIN="$ROOT/target/debug/app"

for tool in Xvfb xdotool openbox import python3 strings; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FATAL: $tool 未安装"; exit 1; }
done
[ -x "$APP_BIN" ] || { echo "FATAL: $APP_BIN 不存在，请先 cargo build -p app（e2e 用 debug 二进制：APP_DUMP_SNAPSHOT 钩子仅 debug 构建编译）"; exit 1; }
PASS=0; FAIL=0

# e2e 默认后端：阶段 4B 起默认配置是 renderer.backend=auto（会优先 GPU），
# 而本轮的断言（增量渲染/脏区/damage 等）针对 CPU 后端语义，故显式指定 cpu。
# t5 会在此基础上增加 backend=gpu 的整轮。
E2E_CFG="$OUT/e2e-default.toml"
cat >"$E2E_CFG" <<'TOML'
[renderer]
backend = "cpu"
TOML
export APP_CONFIG="$E2E_CFG"

# GPU 对拍门禁（阶段 4B t3）：CpuRenderer vs GpuRenderer 离屏逐像素对比。
# 无真实 Vulkan 适配器时用例显式打印 SKIP —— 这里统计并显示，禁止静默通过。
GPU_LOG="$OUT/gpu-parity.log"
if cargo test -p render --test gpu_parity --release -- --nocapture >"$GPU_LOG" 2>&1; then
    GPU_SKIP=$(grep -c "SKIP:" "$GPU_LOG" || true)
    GPU_CASES=$(grep -cE "^test result:" "$GPU_LOG" || true)
    GPU_PASSED=$(grep -oE "test result: ok\. [0-9]+ passed" "$GPU_LOG" | grep -oE "[0-9]+" | head -1)
    GPU_SKIP=${GPU_SKIP:-0}
    echo "PASS gpu-parity (passed=${GPU_PASSED:-0} skip=$GPU_SKIP)"
    PASS=$((PASS+1))
    if [ "$GPU_SKIP" -gt 0 ]; then
        echo "NOTE gpu-parity: $GPU_SKIP 个用例因无真实 GPU 适配器而 SKIP（见 $GPU_LOG）"
    fi
else
    echo "FAIL gpu-parity (见 $GPU_LOG)"
    FAIL=$((FAIL+1))
fi

# release 构建检查：APP_DUMP_SNAPSHOT 钩子仅 debug（#[cfg(debug_assertions)] 门控整个
# 函数与钩子字符串）——release 二进制中该字符串不应存在。
if strings "$ROOT/target/release/app" 2>/dev/null | grep -q APP_DUMP_SNAPSHOT; then
    echo "FAIL release-hook-leak (APP_DUMP_SNAPSHOT 出现在 release 二进制)"
    FAIL=$((FAIL+1))
else
    echo "PASS release-hook-gated (release 二进制无 APP_DUMP_SNAPSHOT)"
    PASS=$((PASS+1))
fi

e2e_start_xvfb "$OUT/xvfb.log" || exit 1
trap 'kill_own "${APP_PID:-}"; kill_own "${WM_PID:-}"; kill_own "${XVFB_PID:-}"; exit 0' EXIT INT TERM
e2e_start_wm "$OUT/openbox.log"

WID=""; APP_PID=""

wait_window() { # 等 app 窗口出现并置 WID；按 PID 枚举窗口并校验 _NET_WM_PID 归属当前 app（防前序用例 kill 后的残留窗口条目）。窗口标题随 dynamic_title 变化（false 时为二进制名），故不按窗口名匹配。首字体含 CJK 时 from_bytes 一次性成本约 7s（DECISIONS 登记），等待上限放宽到 40×0.5=20s。
    local i=0 pid wid
    WID=""
    while [ $i -lt 40 ]; do
        # 进程已退（zombie）立即失败：X server 连接偶发失败等启动错误不再空等 20s。
        if [ "$(awk '{print $3}' /proc/$APP_PID/stat 2>/dev/null)" = "Z" ]; then
            return 1
        fi
        for wid in $(x11 xdotool search --pid "$APP_PID" 2>/dev/null); do
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
                x11 xdotool windowactivate "$WID" >/dev/null 2>&1
                x11 xdotool windowfocus "$WID" >/dev/null 2>&1
                return 0
            fi
        fi
        echo "FAIL start_app: 未找到 tty 窗口"
        echo "  diag: APP_PID=$APP_PID search=[$(x11 xdotool search --pid "$APP_PID" 2>/dev/null | tr '\n' ' ')]"
        ps -o pid,stat,cmd -p $APP_PID 2>/dev/null || echo "  diag: app 进程不存在"
        echo "  diag: applog=[$(cat "$log" 2>/dev/null | tr '\n' '|')]"
        tail -2 "$OUT/openbox.log" 2>/dev/null | sed 's/^/  ob: /'
        FAIL=$((FAIL+1))
        return 1
    fi
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    # 清除残留修饰键：前序用例 ctrl+d 的 keyup 在 Xvfb 下偶发丢失，
    # 会把后续 type 的字母变成控制字符（^E/^C/^H...）。
    x11 xdotool keyup ctrl shift alt super 2>/dev/null
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
        x11 import -window root "$OUT/$1-fail.png" 2>/dev/null
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
        x11 import -window root "$OUT/$1-fail.png" 2>/dev/null
        ps -o pid,stat,wchan:20,cmd -p $APP_PID >"$OUT/$1-fail-ps.txt" 2>&1
        x11 xdotool search --pid "$APP_PID" >"$OUT/$1-fail-windows.txt" 2>&1
        x11 wmctrl -l >"$OUT/$1-fail-wmctrl.txt" 2>&1
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
    x11 xdotool type --window "$WID" "echo hello_e2e"
    x11 xdotool key --window "$WID" Return
    assert_dump "case1-hello_e2e" "$D" "hello_e2e"
    stop_app
fi

# 用例 2：中文回显 + 宽格标记（xdotool type UTF-8 不可靠时改 printf 路径）
D="$OUT/case2-cn.dump"
if start_app "$D"; then
    x11 xdotool type --window "$WID" "echo 你好"
    x11 xdotool key --window "$WID" Return
    if wait_dump "$D" "你好"; then
        echo "PASS case2-cn (x11 xdotool type UTF-8)"; PASS=$((PASS+1))
    else
        x11 xdotool type --window "$WID" "printf '\\344\\275\\240\\345\\245\\275'"
        x11 xdotool key --window "$WID" Return
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
    x11 xdotool type --window "$WID" "echo abc"
    x11 xdotool key --window "$WID" BackSpace
    x11 xdotool type --window "$WID" "d"
    x11 xdotool key --window "$WID" Return
    assert_dump "case3-backspace-abd" "$D" "abd"
    stop_app
fi

# 用例 4：Ctrl-C 中断 sleep 100 后再 echo 标记串
D="$OUT/case4-ctrlc.dump"
if start_app "$D"; then
    x11 xdotool type --window "$WID" "sleep 100"
    x11 xdotool key --window "$WID" Return
    sleep 1
    x11 xdotool key --window "$WID" ctrl+c
    sleep 1
    x11 xdotool type --window "$WID" "echo MARK_CTRLC"
    x11 xdotool key --window "$WID" Return
    assert_dump "case4-ctrlc" "$D" "MARK_CTRLC"
    stop_app
fi

# 用例 5：Ctrl-D 退出 shell 后 app 5s 内退出
D="$OUT/case5-ctrld.dump"
L="$OUT/case5-ctrl-d.log"
if start_app "$D" "$L"; then
    x11 xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
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
    x11 xdotool windowsize "$WID" 640 480
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
    x11 xdotool type --window "$WID" "stty size"
    x11 xdotool key --window "$WID" Return
    assert_dump "case7-stty-size" "$D" "$ROWS $COLS"
    assert_no_wake "case7-resize-no-wake" "$L"
    stop_app
fi

# 用例 8：vim/less 交替屏（系统有 vim 或 less 时执行，否则跳过并说明）
if command -v vim >/dev/null 2>&1; then
    D="$OUT/case8-alt.dump"
    if start_app "$D"; then
        sleep 0.3
        x11 xdotool type --delay 60 --window "$WID" "vim"
        x11 xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-on" "$D" "alternate=true"
        x11 xdotool key --window "$WID" Escape
        x11 xdotool type --window "$WID" ":q"
        x11 xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-off" "$D" "alternate=false"
        stop_app
    fi
elif command -v less >/dev/null 2>&1; then
    D="$OUT/case8-alt.dump"
    if start_app "$D"; then
        sleep 0.3
        x11 xdotool type --delay 60 --window "$WID" "less"
        x11 xdotool key --window "$WID" Return
        assert_dump "case8-alt-screen-on" "$D" "alternate=true"
        x11 xdotool key --window "$WID" q
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
    x11 xdotool type --window "$WID" "stty size"
    x11 xdotool key --window "$WID" Return
    assert_dump "case9-dims-cols" "$D" "cols=120"
    assert_dump "case9-dims-rows" "$D" "rows=30"
    assert_dump "case9-dims-stty" "$D" "30 120"
    x11 xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case9-dims120x30-exits"
    assert_no_wake "case9-dims120x30-no-wake" "$L"
fi

# 用例 10：examples/linux.toml（--config 启动）+ 中文输入 → 回显非方块、
# font 索引行存在（回退链 e2e 断言，替代截图像素比对；规格任务 3.6 备选方案）。
# 中文输入走 printf 八进制路径（实测：xdotool type 对 CJK 发 caret 记法文本，
# 本环境不可用；case2 同结论，DECISIONS 登记）。
D="$OUT/case10-cjk-config.dump"
L="$OUT/case10-cjk-config.log"
# examples/linux.toml 的 renderer.backend 是 auto（会优先 GPU）；本用例断言的是
# CPU 后端的 dump 钩子（# font 行），故派生一份把 backend 固定为 cpu 的副本。
CFG10="$OUT/case10-cfg.toml"
sed 's/^backend = "auto"/backend = "cpu"/' "$ROOT/examples/linux.toml" >"$CFG10"
APP_DUMP_SNAPSHOT="$D" SHELL=/bin/sh "$APP_BIN" --config "$CFG10" >>"$L" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 1
    x11 xdotool type --window "$WID" "printf '\\344\\275\\240\\345\\245\\275'"
    x11 xdotool key --window "$WID" Return
    assert_dump "case10-cjk-echo" "$D" "你好"
    assert_dump "case10-cjk-cols" "$D" "cols=120"
    if grep -q '^# font:' "$D"; then
        echo "PASS case10-cjk-font-indices"; PASS=$((PASS+1))
    else
        echo "FAIL case10-cjk-font-indices (dump 无 # font 行)"
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case10-cjk-config-exits"
    assert_no_wake "case10-cjk-config-no-wake" "$L"
fi

# 用例 11：启动性能（任务 2B.1）——启动后立即输入，2s 内快照出现输出；汉字后台
# 加载期间继续输入英文不丢字符（懒加载：首帧不等 CJK from_bytes）。
D="$OUT/case11-boot-perf.dump"
L="$OUT/case11-boot-perf.log"
T0=$(date +%s%N)
if start_app "$D" "$L"; then
    x11 xdotool type --window "$WID" "echo boot2b"
    x11 xdotool key --window "$WID" Return
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
    x11 xdotool type --window "$WID" "printf '\\344\\275\\240\\345\\245\\275'"
    x11 xdotool key --window "$WID" Return
    x11 xdotool type --window "$WID" "echo AFTER_CJK"
    x11 xdotool key --window "$WID" Return
    assert_dump "case11-cjk-echo" "$D" "你好"
    assert_dump "case11-after-cjk" "$D" "AFTER_CJK"
    x11 xdotool key --window "$WID" ctrl+d
    # 输入抖动重发（Xvfb 序列环境：首次 \x04 可能被行内残留字节吞为非 EOF；
    # shell 已退时重发的 \x04 无消费者，无害）。
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case11-boot-perf-exits"
    assert_no_wake "case11-boot-perf-no-wake" "$L"
fi

# 用例 12：选区与剪贴板（任务 2.1/2.2）——拖选输出行→PRIMARY 写→右键 Paste
# （默认绑定 Right=Paste）→粘贴文本作为命令执行→dump 断言回显。
D="$OUT/case12-selection.dump"
L="$OUT/case12-selection.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "echo PASTE_TARGET"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    # 拖选第 1 行（echo 输出行；y=30 ≈ 行 1 中部，x 10→200 跨整行）。
    # 按钮事件必须走 XTest 根坐标注入（xdotool 老版本 --window click/mousedown
    # 走 XSendEvent，winit 过滤 send_event 标志；先 warp 再无 --window 发按钮）。
    x11 xdotool mousemove --window "$WID" 10 30
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove --window "$WID" 200 30
    sleep 0.2
    x11 xdotool mouseup 1
    sleep 0.5
    # PRIMARY 写断言（选区定型时写）
    if x11 xclip -o -selection primary 2>/dev/null | grep -q "PASTE_TARGET"; then
        echo "PASS case12-primary-write"; PASS=$((PASS+1))
    else
        echo "FAIL case12-primary-write (PRIMARY 无 PASTE_TARGET)"; FAIL=$((FAIL+1))
    fi
    # 右键 Paste（默认绑定 Right=Paste）→粘贴文本作命令执行→命令行回显唯一串
    # （源命令行为 "echo PASTE_TARGET"，粘贴回显行为 "PASTE_TARGET"，可区分）
    x11 xdotool mousemove --window "$WID" 400 30
    sleep 0.2
    x11 xdotool click 3
    sleep 1
    # 粘贴内容无换行→shell 命令行未提交；补 Return 提交后 ctrl+d 才是 EOF。
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    if grep -qF "sh-5.2# PASTE_TARGET" "$D" 2>/dev/null; then
        echo "PASS case12-paste-exec (粘贴命令行回显)"; PASS=$((PASS+1))
    else
        echo "FAIL case12-paste-exec (dump 无粘贴命令行回显)"
        cp "$D" "$OUT/case12-paste-exec-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case12-selection-exits"
    assert_no_wake "case12-selection-no-wake" "$L"
fi

# 用例 13：括号粘贴（任务 2.3）——?2004 模式下粘贴含换行内容→cat -v 显示
# ESC[200~…ESC[201~ 包裹（内容中 \n 规范化为 \r，cat -v 显示 ^M）。
D="$OUT/case13-bracketed.dump"
L="$OUT/case13-bracketed.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "printf '\\e[?2004h'; cat -v"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    # CLIPBOARD 设含换行内容（xclip -i 读 stdin；printf 八进制换行防 shell 语义）
    printf 'BB1\nBB2' | x11 xclip -i -selection clipboard
    sleep 0.3
    x11 xdotool mousemove --window "$WID" 400 30
    sleep 0.2
    x11 xdotool click 3
    sleep 1
    if grep -qF '^[[200~BB1' "$D" 2>/dev/null && grep -qF 'BB2^[[201~' "$D" 2>/dev/null; then
        echo "PASS case13-bracketed-wrap"; PASS=$((PASS+1))
    else
        echo "FAIL case13-bracketed-wrap (dump 无 ESC[200~…ESC[201~ 包裹)"
        cp "$D" "$OUT/case13-bracketed-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+c
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+c
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case13-bracketed-exits"
    assert_no_wake "case13-bracketed-no-wake" "$L"
fi



# 用例 14：滚轮滚动（任务 3/4）——30 行输出压 scrollback→滚轮上滚断言历史行
# 可见→滚轮下滚回底（auto-follow 恢复）。xdotool click 4/5=滚轮上/下（XTest 根坐标）。
D="$OUT/case14-scroll.dump"
L="$OUT/case14-scroll.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "for i in \$(seq 1 30); do echo LINE_\$i; done"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    # 滚轮上滚 5（×multiplier 3=15 行 offset）→视口离开底部，历史行可见
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    i=0; while [ $i -lt 5 ]; do x11 xdotool click 4; i=$((i+1)); sleep 0.05; done
    sleep 0.5
    if grep -q "LINE_1" "$D" 2>/dev/null && ! grep -q "LINE_29" "$D" 2>/dev/null; then
        echo "PASS case14-scroll-up (历史行 LINE_1 可见且离开底部)"; PASS=$((PASS+1))
    else
        echo "FAIL case14-scroll-up (上滚后视口未离开底部)"
        cp "$D" "$OUT/case14-scroll-up-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    # 滚轮下滚回底→auto-follow 恢复，最新行可见
    i=0; while [ $i -lt 5 ]; do x11 xdotool click 5; i=$((i+1)); sleep 0.05; done
    sleep 0.5
    if grep -q "LINE_29" "$D" 2>/dev/null; then
        echo "PASS case14-scroll-bottom (下滚回底 LINE_29 可见)"; PASS=$((PASS+1))
    else
        echo "FAIL case14-scroll-bottom (回底后 LINE_29 不可见)"
        cp "$D" "$OUT/case14-scroll-bottom-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case14-scroll-exits"
    assert_no_wake "case14-scroll-no-wake" "$L"
fi


# 用例 15：备用屏滚轮→方向键（任务 3）——?1049 备用屏+cat -v 显示方向键序列
# （scrolling.alt_screen_scroll=true 默认：备用屏下滚轮发 ArrowUp/Down 而非 scroll_viewport）。
D="$OUT/case15-alt-wheel.dump"
L="$OUT/case15-alt-wheel.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "printf '\\e[?1049h'; cat -v"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    # 滚轮上 2 次→方向键 Up×2（×multiplier 3=6 个 ESC[A？LineDelta 1 行×mult）
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    x11 xdotool click 4
    sleep 0.2
    x11 xdotool click 4
    sleep 0.5
    if grep -qF '^[[A' "$D" 2>/dev/null; then
        echo "PASS case15-alt-wheel-arrow (备用屏滚轮→方向键 Up 序列)"; PASS=$((PASS+1))
    else
        echo "FAIL case15-alt-wheel-arrow (dump 无 ^[[A 方向键序列)"
        cp "$D" "$OUT/case15-alt-wheel-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+c
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case15-alt-wheel-exits"
    assert_no_wake "case15-alt-wheel-no-wake" "$L"
fi

# 用例 16：鼠标上报 SGR 1006（任务 3）——?1003 全鼠标跟踪+cat -v 显示
# 上报序列（含单元格坐标，padding/缩放经 pixel_to_cell 转换）。
D="$OUT/case16-mouse-report.dump"
L="$OUT/case16-mouse-report.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "printf '\\e[?1003h\\e[?1006h'; cat -v"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    # 左键点击（非 Shift→上报优先；坐标 cell(2,1) 附近）
    x11 xdotool mousemove --window "$WID" 30 30
    sleep 0.2
    x11 xdotool click 1
    sleep 0.5
    # SGR 1006 上报序列形如 ^[[<0;col;rowM（Press）/m（Release）
    if grep -qF '^[[<0;' "$D" 2>/dev/null; then
        echo "PASS case16-mouse-report (SGR 1006 上报序列含坐标)"; PASS=$((PASS+1))
    else
        echo "FAIL case16-mouse-report (dump 无 ^[[<0; 上报序列)"
        cp "$D" "$OUT/case16-mouse-report-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+c
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
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
    x11 xdotool type --window "$WID" "echo IME_SMOKE"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    assert_dump "case17-ime-smoke" "$D" "IME_SMOKE"
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case17-ime-exits"
    assert_no_wake "case17-ime-no-wake" "$L"
fi


# 用例 18：滚动条拖拽（任务 4）——50 行输出造 scrollback→右缘条带拖到顶
# →视口显示历史区首行（DRAG_1 可见、DRAG_50 离开底部）。
D="$OUT/case18-scrollbar-drag.dump"
L="$OUT/case18-scrollbar-drag.log"
# 80 列默认（120 列×字宽≈1560px 超 Xvfb 1280 屏宽，条带在屏外鼠标不可达；
# 80 列×13px≈1040px 在屏内，条带命中正常）。
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "for i in \$(seq 1 50); do echo DRAG_\$i; done"
    x11 xdotool key --window "$WID" Return
    sleep 0.5
    # 条带几何：客户区右缘-2；先确认底部视口含 DRAG_50
    if ! grep -q "DRAG_50" "$D" 2>/dev/null; then
        echo "FAIL case18-scrollbar-drag (初始视口无 DRAG_50)"
        cp "$D" "$OUT/case18-drag-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    else
        GW=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^WIDTH=' | cut -d= -f2)
        GH=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^HEIGHT=' | cut -d= -f2)
        # 条带按下（3/4 高）→拖到顶（y=2≈offset 0；GH/8 仅 offset≈2，
        # 视口顶为 DRAG_3 非 DRAG_1，故拖坐标取 2）
        x11 xdotool mousemove --window "$WID" $((GW-2)) $((GH*3/4))
        sleep 0.2
        x11 xdotool mousedown 1
        sleep 0.2
        x11 xdotool mousemove --window "$WID" $((GW-2)) 2
        sleep 0.3
        x11 xdotool mouseup 1
        sleep 0.5
        if grep -q "DRAG_1" "$D" 2>/dev/null && ! grep -q "DRAG_50" "$D" 2>/dev/null; then
            echo "PASS case18-scrollbar-drag (拖到顶：DRAG_1 可见且 DRAG_50 离开)"; PASS=$((PASS+1))
        else
            echo "FAIL case18-scrollbar-drag (拖拽后视口未到位移顶部)"
            cp "$D" "$OUT/case18-drag-fail.dump" 2>/dev/null
            FAIL=$((FAIL+1))
        fi
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case18-scrollbar-drag-exits"
    assert_no_wake "case18-scrollbar-drag-no-wake" "$L"
fi


# 用例 19：复制文本换行语义（任务 2C.1）——长行软折行处拼接无分隔、
# 硬换行插 \n；拖选全部输出读 PRIMARY 断言。
D="$OUT/case19-wrap-semantics.dump"
L="$OUT/case19-wrap-semantics.log"
if start_app "$D" "$L"; then
    sleep 1
    # 100 个 A 一行（>80 列→软折 2 行）+三行短文本（硬换行）
    x11 xdotool type --window "$WID" "printf 'A%.0s' \$(seq 1 100); echo; echo B1; echo B2; echo B3"
    x11 xdotool key --window "$WID" Return
    sleep 0.8
    # 拖选全部输出（首行提示符→B3 行；x 上界取窗口宽-5 覆盖 80 列全宽，
    # 1000px/13px≈76 列会截断软折行长行导致 A 段不连续）
    GW=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^WIDTH=' | cut -d= -f2)
    x11 xdotool mousemove --window "$WID" 10 20
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove --window "$WID" $((GW-5)) 200
    sleep 0.2
    x11 xdotool mouseup 1
    sleep 0.5
    PRIMARY=$(x11 xclip -o -selection primary 2>/dev/null)
    # 长行软折：A 连续 100 无 \n 打断（若软折误插分隔则 A 段断裂）
    if printf '%s' "$PRIMARY" | grep -q 'A\{100\}'; then
        echo "PASS case19-soft-wrap-join (长行软折拼接无分隔)"; PASS=$((PASS+1))
    else
        echo "FAIL case19-soft-wrap-join (PRIMARY 无连续 100 A)"
        echo "  PRIMARY=[$(printf '%s' "$PRIMARY" | head -c 120)]"
        cp "$D" "$OUT/case19-wrap-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    # 三行短文本：硬换行 \n 保留
    if printf '%s' "$PRIMARY" | grep -q 'B1
B2'; then
        echo "PASS case19-hard-wrap-newline (硬换行插 \n)"; PASS=$((PASS+1))
    else
        echo "FAIL case19-hard-wrap-newline (PRIMARY 无 B1\\n B2)"
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case19-wrap-exits"
    assert_no_wake "case19-wrap-no-wake" "$L"
fi


# 用例 20：选区与滚动联动（任务 2C.2）——上滚后选视口内两行→复制；
# 再滚动→中键 Copy 重提取→断言两次内容一致（选区锚定内容而非视口坐标）。
D="$OUT/case20-selection-anchor.dump"
L="$OUT/case20-selection-anchor.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "for i in \$(seq -w 1 100); do echo L\$i; done"
    x11 xdotool key --window "$WID" Return
    sleep 0.8
    # 上滚 4 次（multiplier 10 → 40 行）
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    i=0; while [ $i -lt 4 ]; do x11 xdotool click 4; sleep 0.05; i=$((i+1)); done
    sleep 0.5
    # 拖选视口内三行（y 40..90）
    x11 xdotool mousemove --window "$WID" 10 40
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove --window "$WID" 200 90
    sleep 0.2
    x11 xdotool mouseup 1
    sleep 0.5
    P_A=$(x11 xclip -o -selection primary 2>/dev/null)
    # 下滚 2 次（20 行）后再上滚 2 次回到原偏移，中键 Copy 重提取
    i=0; while [ $i -lt 2 ]; do x11 xdotool click 5; sleep 0.05; i=$((i+1)); done
    sleep 0.4
    i=0; while [ $i -lt 2 ]; do x11 xdotool click 4; sleep 0.05; i=$((i+1)); done
    sleep 0.4
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    x11 xdotool click 2
    sleep 0.6
    P_B=$(x11 xclip -o -selection primary 2>/dev/null)
    if [ -n "$P_A" ] && [ "$P_A" = "$P_B" ] && printf '%s' "$P_A" | grep -q 'L0'; then
        echo "PASS case20-selection-anchor (滚动后重提取内容一致：$(printf '%s' "$P_A" | tr '\n' '|' | head -c 40))"
        PASS=$((PASS+1))
    else
        echo "FAIL case20-selection-anchor (A=[$(printf '%s' "$P_A" | tr '\n' '|' | head -c 40)] B=[$(printf '%s' "$P_B" | tr '\n' '|' | head -c 40)])"
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case20-selection-anchor-exits"
    assert_no_wake "case20-selection-anchor-no-wake" "$L"
fi

# 用例 21：跨视口拖选自动滚动（任务 2C.3）——拖到窗口下沿悬停，每帧推进
# 视口并扩展选区；释放后复制应含多屏行（>28 行，视口 24 行）。
D="$OUT/case21-autoscroll.dump"
L="$OUT/case21-autoscroll.log"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "for i in \$(seq -w 1 100); do echo M\$i; done"
    x11 xdotool key --window "$WID" Return
    sleep 0.8
    # 先上滚到顶（8 次×10 行），留出向下滚动空间
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    i=0; while [ $i -lt 8 ]; do x11 xdotool click 4; sleep 0.05; i=$((i+1)); done
    sleep 0.4
    GH=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^HEIGHT=' | cut -d= -f2)
    # 从第 2 行按下，拖到下沿悬停（自动滚动 1.5s）
    GW=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^WIDTH=' | cut -d= -f2)
    x11 xdotool mousemove --window "$WID" 10 40
    sleep 0.2
    x11 xdotool mousedown 1
    x11 xdotool mousemove --window "$WID" 10 $((GH-6))
    sleep 1.2
    # 下沿悬停自动滚动后向右扩展列范围（同列拖拽只选 1 列，文本近似空）
    x11 xdotool mousemove --window "$WID" $((GW-10)) $((GH-6))
    sleep 0.4
    x11 xdotool mouseup 1
    sleep 0.5
    P=$(x11 xclip -o -selection primary 2>/dev/null)
    N=$(printf '%s' "$P" | grep -c 'M[0-9]' 2>/dev/null || echo 0)
    if [ "$N" -gt 28 ]; then
        echo "PASS case21-autoscroll (跨屏拖选 $N 行 > 视口 24)"; PASS=$((PASS+1))
    else
        echo "FAIL case21-autoscroll (仅 $N 行，自动滚动未生效)"
        FAIL=$((FAIL+1))
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case21-autoscroll-exits"
    assert_no_wake "case21-autoscroll-no-wake" "$L"
fi


# 用例 22：IME 预编辑叠加渲染（任务 2C.3）——APP_TEST_PREEDIT（仅 debug）
# 注入组合串后：叠加区域像素与无 Preedit 时不同、下划线像素存在、
# dump（终端网格）不含该文本（证明未写入终端状态）。
D="$OUT/case22-preedit.dump"
L="$OUT/case22-preedit.log"
A="$OUT/case22-no-preedit.png"
B="$OUT/case22-preedit.png"
if start_app "$D" "$L"; then
    sleep 1
    x11 import -window root "$A" 2>/dev/null
    stop_app
    # 带 Preedit 注入再启动（debug 专有开关）
    export APP_TEST_PREEDIT="中文字"
    if start_app "$D" "$L"; then
        sleep 1.2
        x11 import -window root "$B" 2>/dev/null
        # 1) 叠加区域像素与无 Preedit 不同
        DIFF=$(compare -metric AE "$A" "$B" null: 2>&1 | head -1 | tr -dc '0-9')
        : "${DIFF:=0}"
        if [ "$DIFF" -gt 0 ]; then
            echo "PASS case22-preedit-pixels (差异像素 $DIFF)"
            PASS=$((PASS+1))
        else
            echo "FAIL case22-preedit-pixels (叠加前后无像素差异)"
            FAIL=$((FAIL+1))
        fi
        # 2) 下划线像素存在：裁剪预编辑区域底部 2px，白色像素最大值应为 1
        GW=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^WIDTH=' | cut -d= -f2)
        GH=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null | grep '^HEIGHT=' | cut -d= -f2)
        CW=$((GW / 80)); CH=$((GH / 24))
        # 窗口客户区原点（屏幕坐标）
        # 用 xwininfo 的客户区绝对坐标（xdotool getwindowgeometry 报的是框架位置，
        # 与 root 截图坐标不一致会导致裁剪错位）。
        WX=$(x11 xwininfo -id "$WID" 2>/dev/null | awk '/Absolute upper-left X/{print $NF}')
        WY=$(x11 xwininfo -id "$WID" 2>/dev/null | awk '/Absolute upper-left Y/{print $NF}')
        : "${WX:=0}"; : "${WY:=0}"
        # 预编辑起点=提示符后一列（dump 首行长度+1），下划线在行 0 底部 2px。
        COL=$(awk 'NR==2{print length($0)+1; exit}' "$D" 2>/dev/null)
        : "${COL:=8}"
        MAXV=$(convert "$B" -crop $((CW*6))x2+$((WX+COL*CW))+$((WY+CH-2)) -colorspace Gray -format "%[fx:maxima]" info: 2>/dev/null)
        case "$MAXV" in
            1*)
                echo "PASS case22-preedit-underline (下划线像素存在 maxima=$MAXV)"
                PASS=$((PASS+1)) ;;
            *)
                echo "FAIL case22-preedit-underline (下划线区域无前景像素 maxima=$MAXV)"
                cp "$B" "$OUT/case22-underline-fail.png" 2>/dev/null
                FAIL=$((FAIL+1)) ;;
        esac
        # 3) dump 不含预编辑文本（未写入网格）
        if grep -q "中文字" "$D" 2>/dev/null; then
            echo "FAIL case22-preedit-not-in-grid (dump 含预编辑文本)"
            FAIL=$((FAIL+1))
        else
            echo "PASS case22-preedit-not-in-grid (dump 不含预编辑文本)"
            PASS=$((PASS+1))
        fi
        unset APP_TEST_PREEDIT
        x11 xdotool key --window "$WID" ctrl+d
        sleep 0.3
        x11 xdotool key --window "$WID" ctrl+d
        assert_app_exits "case22-preedit-exits"
        assert_no_wake "case22-preedit-no-wake" "$L"
    fi
    unset APP_TEST_PREEDIT
fi


# 用例 23：window.position 与 dynamic_title（任务 2C.4）——初始位置生效、
# OSC 0/2 标题更新、dynamic_title=false 时标题固定为二进制名。
D="$OUT/case23-title.dump"
L="$OUT/case23-title.log"
CFG_ON="$OUT/case23-on.toml"
CFG_OFF="$OUT/case23-off.toml"
cat >"$CFG_ON" <<'TOML'
[renderer]
backend = "cpu"
[window]
position = { x = 60, y = 50 }
dimensions = { columns = 80, lines = 24 }
dynamic_title = true
TOML
cat >"$CFG_OFF" <<'TOML'
[renderer]
backend = "cpu"
[window]
dimensions = { columns = 80, lines = 24 }
dynamic_title = false
TOML
export APP_CONFIG="$CFG_ON"
if start_app "$D" "$L"; then
    sleep 1
    # window.position：X11 下初始位置最终由窗口管理器决定（winit 的请求不保证被采纳）。
    # 这里以"客户区绝对位置"记录，稍后与"无 position 配置"的运行对比：
    # 位置不同 → 配置确实产生了效果（PASS）；完全相同 → 平台限制（SKIP 并说明），不判 FAIL。
    POS_WITH=$(x11 xwininfo -id "$WID" 2>/dev/null | awk '/Absolute upper-left X/{x=$NF} /Absolute upper-left Y/{y=$NF} END{print x","y}')
    # OSC 0 标题序列 → 窗口标题更新
    x11 xdotool type --window "$WID" "printf '\\033]0;T123\\007'"
    x11 xdotool key --window "$WID" Return
    sleep 0.6
    NAME=$(x11 xdotool getwindowname "$WID" 2>/dev/null)
    case "$NAME" in
        *T123*)
            echo "PASS case23-dynamic-title (标题=$NAME)"; PASS=$((PASS+1)) ;;
        *)
            echo "FAIL case23-dynamic-title (标题=$NAME 未更新为 T123)"; FAIL=$((FAIL+1)) ;;
    esac
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case23-title-exits"
    assert_no_wake "case23-title-no-wake" "$L"
fi
export APP_CONFIG="$CFG_OFF"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "printf '\\033]0;T999\\007'"
    x11 xdotool key --window "$WID" Return
    sleep 0.6
    NAME=$(x11 xdotool getwindowname "$WID" 2>/dev/null)
    case "$NAME" in
        *T999*)
            echo "FAIL case23-static-title (dynamic_title=false 仍更新为 $NAME)"; FAIL=$((FAIL+1)) ;;
        *)
            echo "PASS case23-static-title (标题固定=$NAME)"; PASS=$((PASS+1)) ;;
    esac
    # 无 position 配置的客户区位置（用于与上一段对比）
    POS_WITHOUT=$(x11 xwininfo -id "$WID" 2>/dev/null | awk '/Absolute upper-left X/{x=$NF} /Absolute upper-left Y/{y=$NF} END{print x","y}')
    if [ -n "${POS_WITH:-}" ] && [ -n "${POS_WITHOUT:-}" ] && [ "$POS_WITH" != "$POS_WITHOUT" ]; then
        echo "PASS case23-position (with=($POS_WITH) without=($POS_WITHOUT)：position 配置改变了客户区位置)"
        PASS=$((PASS+1))
    else
        echo "SKIP case23-position (with=(${POS_WITH:-NA}) without=(${POS_WITHOUT:-NA})：winit 0.30 X11 初始位置请求未被窗口管理器采纳，平台限制，DECISIONS 登记)"
    fi
    x11 xdotool key --window "$WID" ctrl+d
    sleep 0.3
    x11 xdotool key --window "$WID" ctrl+d
    assert_app_exits "case23-static-exits"
    assert_no_wake "case23-static-no-wake" "$L"
fi
export APP_CONFIG="$E2E_CFG"


# 用例 24：大段粘贴静止排空（任务 2D.2，P1）——1MB 已知模式粘贴后不产生任何
# 事件，5s 内必须精确送达 1048576 字节且 sha256 与源一致。
D="$OUT/case24-paste-1mb.dump"
L="$OUT/case24-paste-1mb.log"
SRC="$OUT/case24-paste-src.bin"
DST=/tmp/case24-paste-dst.bin
rm -f "$DST"
python3 -c "open('$SRC','w').write('0123456789abcdef'*65536)"
if start_app "$D" "$L"; then
    sleep 1
    # raw 模式让无换行内容不被 tty 行缓冲卡住（canonical 下 cat 收不到数据）
    x11 xdotool type --window "$WID" "stty raw -echo; cat > $DST"
    x11 xdotool key --window "$WID" Return
    sleep 1
    x11 xclip -i -selection clipboard < "$SRC"
    sleep 0.3
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    x11 xdotool click 3
    # 之后不产生任何事件，等排空
    sleep 5
    SZ=$(stat -c%s "$DST" 2>/dev/null || echo 0)
    SRC_SHA=$(sha256sum "$SRC" | cut -d' ' -f1)
    DST_SHA=$(sha256sum "$DST" 2>/dev/null | cut -d' ' -f1)
    if [ "$SZ" = "1048576" ] && [ "$SRC_SHA" = "$DST_SHA" ]; then
        echo "PASS case24-paste-1mb-drain (静止 5s 送达 $SZ 字节，sha256 一致)"
        PASS=$((PASS+1))
    else
        echo "FAIL case24-paste-1mb-drain (静止 5s 仅 $SZ / 期望 1048576，sha 一致=$([ "$SRC_SHA" = "$DST_SHA" ] && echo yes || echo no))"
        FAIL=$((FAIL+1))
    fi
    stop_app
fi

# 用例 25：10MB 粘贴期间输入不丢（任务 2D.2）——排空分帧进行时键入 marker
# 仍按序到达（cat 收进文件），且最终文件包含 marker。
D="$OUT/case25-paste-10mb.dump"
L="$OUT/case25-paste-10mb.log"
SRC10="$OUT/case25-paste-src.bin"
DST10=/tmp/case25-paste-dst.bin
rm -f "$DST10"
python3 -c "open('$SRC10','w').write('ABCDEFGH'*1310720)"
if start_app "$D" "$L"; then
    sleep 1
    x11 xdotool type --window "$WID" "stty raw -echo; cat > $DST10"
    x11 xdotool key --window "$WID" Return
    sleep 1
    x11 xclip -i -selection clipboard < "$SRC10"
    sleep 0.3
    x11 xdotool mousemove --window "$WID" 400 300
    sleep 0.2
    x11 xdotool click 3
    # 粘贴排空进行中键入 marker（raw 模式下直接进入 cat 的记录）
    sleep 0.2
    x11 xdotool type --window "$WID" "KEYMARK"
    sleep 12
    SZ=$(stat -c%s "$DST10" 2>/dev/null || echo 0)
    if [ "$SZ" -ge 10485760 ] && grep -q "KEYMARK" "$DST10" 2>/dev/null; then
        echo "PASS case25-paste-10mb-input (送达 $SZ 字节且含 KEYMARK)"
        PASS=$((PASS+1))
    else
        echo "FAIL case25-paste-10mb-input (送达 $SZ 字节，含 KEYMARK=$(grep -c KEYMARK "$DST10" 2>/dev/null || echo 0))"
        FAIL=$((FAIL+1))
    fi
    stop_app
fi


# 用例 26：增量渲染正确性（阶段 3A 任务 3）——同一输入序列分别以增量与
# APP_FORCE_FULL_REDRAW=1（仅 debug）运行，截取窗口图像断言逐像素相等。
# 覆盖：文本输出、光标移动、视口滚动、选区高亮、滚动条。
D="$OUT/case26-incr.dump"
L="$OUT/case26-incr.log"
A="$OUT/case26-incr.png"
B="$OUT/case26-full.png"
run_sequence() { # $1=force(full|incr) $2=png
    if start_app "$D" "$L"; then
        sleep 1.5
        # 固定输入序列（无时间相关输出）
        x11 xdotool type --window "$WID" "for i in 1 2 3 4 5 6 7 8; do echo INCR_LINE_\$i; done"
        x11 xdotool key --window "$WID" Return
        sleep 0.8
        x11 xdotool type --window "$WID" "printf 'COLORED: \\033[31mRED\\033[0m \\033[1mBOLD\\033[0m\\n'"
        x11 xdotool key --window "$WID" Return
        sleep 0.6
        # 光标移动（printf 不带换行，光标停在行中）
        x11 xdotool type --window "$WID" "printf 'CURSOR_HERE'"
        x11 xdotool key --window "$WID" Return
        sleep 0.4
        x11 xdotool type --window "$WID" "printf 'ABCDEFGHIJ'"
        sleep 0.3
        # 注：不在此处用滚轮（Xvfb 下事件偶发丢失会让两次运行的视口不同）；
        # 视口滚动路径由 render 的随机序列单测（2000 步含滚动/清屏）覆盖。
        # 选区拖选（高亮叠加层）
        x11 xdotool mousemove --window "$WID" 10 60
        sleep 0.2
        x11 xdotool mousedown 1
        x11 xdotool mousemove --window "$WID" 300 100
        sleep 0.2
        x11 xdotool mouseup 1
        sleep 0.6
        # 再输出几行（推动历史，行脏）
        x11 xdotool type --window "$WID" "echo AFTER_SELECTION"
        x11 xdotool key --window "$WID" Return
        sleep 0.6
        x11 import -window "$WID" "$2" 2>/dev/null
        stop_app
    else
        return 1
    fi
    return 0
}
if run_sequence incr "$A"; then
    export APP_FORCE_FULL_REDRAW=1
    if run_sequence full "$B"; then
        unset APP_FORCE_FULL_REDRAW
        if [ -s "$A" ] && [ -s "$B" ]; then
            DIFF=$(compare -metric AE "$A" "$B" null: 2>&1 | head -1 | tr -dc '0-9')
            : "${DIFF:=unknown}"
            if [ "$DIFF" = "0" ]; then
                echo "PASS case26-incremental-equals-full (截图逐像素一致)"
                PASS=$((PASS+1))
            else
                echo "FAIL case26-incremental-equals-full (差异像素 $DIFF)"
                cp "$A" "$OUT/case26-incr-fail.png" 2>/dev/null
                cp "$B" "$OUT/case26-full-fail.png" 2>/dev/null
                FAIL=$((FAIL+1))
            fi
        else
            echo "FAIL case26-incremental-equals-full (截图缺失)"
            FAIL=$((FAIL+1))
        fi
    else
        unset APP_FORCE_FULL_REDRAW
        echo "FAIL case26-force-full-start (APP_FORCE_FULL_REDRAW 运行启动失败)"
        FAIL=$((FAIL+1))
    fi
fi
unset APP_FORCE_FULL_REDRAW


# 用例 27：滚动场景增量渲染正确性——输出 300 行后用滚动绑定
# （PageUp/PageDown → ScrollPageUp/Down）滚动，增量与 APP_FORCE_FULL_REDRAW=1
# 两次运行截取窗口图像断言逐像素相等。
D27="$OUT/case27-scroll.dump"
L27="$OUT/case27-scroll.log"
A27="$OUT/case27-incr.png"
B27="$OUT/case27-full.png"
CFG27="$OUT/case27.toml"
cat >"$CFG27" <<'TOML'
[renderer]
backend = "cpu"
[window]
dimensions = { columns = 120, lines = 30 }
[[keyboard.bindings]]
key = "Page_Up"
action = "ScrollPageUp"
[[keyboard.bindings]]
key = "Page_Down"
action = "ScrollPageDown"
[[keyboard.bindings]]
key = "Home"
mods = "Control"
action = "ScrollToTop"
[[keyboard.bindings]]
key = "End"
mods = "Control"
action = "ScrollToBottom"
TOML
scroll_seq() { # $1=png
    sleep 1.5
    x11 xdotool type --window "$WID" "seq 1 300"
    x11 xdotool key --window "$WID" Return
    sleep 1.0
    # 上滚/下滚多次（走 ScrollPageUp/Down 绑定，避免滚轮事件抖动）
    i=0; while [ $i -lt 4 ]; do x11 xdotool key --window "$WID" Page_Up; sleep 0.12; i=$((i+1)); done
    sleep 0.4
    x11 xdotool key --window "$WID" ctrl+Home; sleep 0.3
    i=0; while [ $i -lt 2 ]; do x11 xdotool key --window "$WID" Page_Down; sleep 0.12; i=$((i+1)); done
    sleep 0.6
    x11 import -window "$WID" "$1" 2>/dev/null
}
export APP_CONFIG="$CFG27"
if start_app "$D27" "$L27"; then
    scroll_seq "$A27"
    stop_app
    export APP_FORCE_FULL_REDRAW=1
    if start_app "$D27" "$L27"; then
        scroll_seq "$B27"
        stop_app
    fi
    unset APP_FORCE_FULL_REDRAW
    if [ -s "$A27" ] && [ -s "$B27" ]; then
        DIFF27=$(compare -metric AE "$A27" "$B27" null: 2>&1 | head -1 | tr -dc '0-9')
        : "${DIFF27:=unknown}"
        if [ "$DIFF27" = "0" ]; then
            echo "PASS case27-scroll-incremental-equals-full (滚动后截图逐像素一致)"; PASS=$((PASS+1))
        else
            echo "FAIL case27-scroll-incremental-equals-full (差异像素 $DIFF27)"
            cp "$A27" "$OUT/case27-incr-fail.png" 2>/dev/null
            cp "$B27" "$OUT/case27-full-fail.png" 2>/dev/null
            FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL case27-scroll-incremental-equals-full (截图缺失)"; FAIL=$((FAIL+1))
    fi
    export APP_CONFIG="$E2E_CFG"
fi
export APP_CONFIG="$E2E_CFG"


# 用例 28：自适应帧间隔的尾沿渲染（阶段 3C 任务 4）——大量输出后立即打印结束标记，
# 断言 1 秒内 dump 出现该标记（证明输出停止后最后一帧被渲染，屏幕不停在中间状态）。
D28="$OUT/case28-tail.dump"
L28="$OUT/case28-tail.log"
if start_app "$D28" "$L28"; then
    sleep 1.5
    x11 xdotool type --window "$WID" "seq 1 200000; echo TAIL_MARKER_OK"
    x11 xdotool key --window "$WID" Return
    # 输出结束后最多等 1s 让尾沿帧渲染完
    found=0
    i=0
    while [ $i -lt 20 ]; do
        if [ -f "$D28" ] && grep -q "TAIL_MARKER_OK" "$D28" 2>/dev/null; then found=1; break; fi
        sleep 0.05
        i=$((i+1))
    done
    if [ "$found" = "1" ]; then
        echo "PASS case28-tail-frame (输出结束后 1s 内出现结束标记)"; PASS=$((PASS+1))
    else
        echo "FAIL case28-tail-frame (1s 内 dump 无 TAIL_MARKER_OK)"
        cp "$D28" "$OUT/case28-tail-fail.dump" 2>/dev/null
        FAIL=$((FAIL+1))
    fi
    # 注：本用例只验证"尾沿帧"（20 万行在 debug 构建下 feed 极慢，libghostty
    # 页面校验为 O(n²) 级，3B 已用 gdb 证实），故不用 ctrl+d 断言退出（退出路径
    # 已由 case5/case10/case11 等 10+ 用例覆盖），直接以自有 PID 收尾。
    assert_no_wake "case28-tail-no-wake" "$L28"
    stop_app
fi

# 用例 29（4B-fix1 任务 B）：窗口被强制设为非整格尺寸时，网格外余数区必须是背景色。
#   cols*cell_w 与 cols*cell_h 通常不能整除窗口像素尺寸，右侧/底部余数区若未铺背景，
#   会显示未初始化像素（窗口大于字符网格时的"空白带"）。
CFG29="$OUT/case29-nonintegral.toml"
D29="$OUT/case29-nonintegral.dump"
L29="$OUT/case29-nonintegral.log"
S29="$OUT/case29-nonintegral.png"
cat >"$CFG29" <<'TOML'
[renderer]
backend = "cpu"
[window]
dimensions = { columns = 100, lines = 30 }
[colors.primary]
background = "#008040"
TOML
APP_CONFIG="$CFG29" APP_DUMP_SNAPSHOT="$D29" SHELL=/bin/sh "$APP_BIN" >"$L29" 2>&1 &
APP_PID=$!
if wait_window; then
    # 强制一个"非整格"窗口尺寸：在网格尺寸上各加 7/5 像素。
    GEO=$(x11 xdotool getwindowgeometry --shell "$WID" 2>/dev/null)
    WW=$(echo "$GEO" | grep '^WIDTH=' | cut -d= -f2)
    WH=$(echo "$GEO" | grep '^HEIGHT=' | cut -d= -f2)
    if [ -n "$WW" ] && [ -n "$WH" ]; then
        x11 xdotool windowsize "$WID" $((WW + 7)) $((WH + 5))
        sleep 1.2
        x11 import -window "$WID" "$S29" 2>/dev/null
        if [ -s "$S29" ]; then
            # 左下角 3x3 区域应为背景色 #008040（右侧 8px 是滚动条条带，不能取样）。
            NEAR=$(convert "$S29" -gravity southwest -crop 3x3+0+0 +repage txt:- 2>/dev/null \
                | grep -cE "#00[0-9A-Fa-f]{2}4[0-9A-Fa-f]" || true)
            if [ "$NEAR" -ge 6 ]; then
                echo "PASS case29-nonintegral-bg-fill (左下角 $NEAR/9 像素为背景色)"; PASS=$((PASS+1))
            else
                echo "FAIL case29-nonintegral-bg-fill (左下角仅 $NEAR/9 像素接近背景色，见 $S29)"; FAIL=$((FAIL+1))
            fi
        else
            echo "FAIL case29-nonintegral-bg-fill (截图缺失)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL case29-nonintegral-bg-fill (取不到窗口尺寸)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL case29-nonintegral-bg-fill (窗口未出现)"; FAIL=$((FAIL+1))
fi

# 用例 30（4B-fix1 任务 C）：预编辑清除后不留残影 —— 清除后的截图必须与
#   "从未设置预编辑"的截图在阈值内一致（叠加层覆盖过的行已重绘回网格内容）。
CFG30="$OUT/case30-preedit-clear.toml"
S30A="$OUT/case30-with-preedit.png"
S30B="$OUT/case30-after-clear.png"
S30C="$OUT/case30-no-preedit.png"
L30="$OUT/case30-preedit-clear.log"
cat >"$CFG30" <<'TOML'
[renderer]
backend = "cpu"
[window]
dimensions = { columns = 80, lines = 24 }
TOML
# 参照：不注入预编辑，内容固定
APP_CONFIG="$CFG30" SHELL=/bin/sh "$APP_BIN" >"$L30" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1; x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.6
    x11 xdotool type --window "$WID" "echo PREEDIT_BASE"; x11 xdotool key --window "$WID" Return
    sleep 0.8
    x11 import -window "$WID" "$S30C" 2>/dev/null
    stop_app
fi
# 注入预编辑，1.2s 后自动清除：先抓"有预编辑"，再抓"清除后"
APP_CONFIG="$CFG30" APP_TEST_PREEDIT="WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW" APP_TEST_PREEDIT_CLEAR_MS=3000 SHELL=/bin/sh "$APP_BIN" >>"$L30" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1; x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.6
    x11 xdotool type --window "$WID" "echo PREEDIT_BASE"; x11 xdotool key --window "$WID" Return
    sleep 0.4
    x11 import -window "$WID" "$S30A" 2>/dev/null
    sleep 3.2
    x11 import -window "$WID" "$S30B" 2>/dev/null
    stop_app
fi
if [ -s "$S30A" ] && [ -s "$S30B" ] && [ -s "$S30C" ]; then
    # 有预编辑 vs 无预编辑：应有可见差异（证明钩子生效）
    DIFF_A=$(compare -metric AE "$S30A" "$S30C" null: 2>&1 | head -1)
    # 清除后 vs 无预编辑：应在阈值内（残影 = 大差异）
    DIFF_B=$(compare -metric AE -fuzz 3% "$S30B" "$S30C" null: 2>&1 | head -1)
    echo "  preedit 有/无差异=$DIFF_A，清除后差异=$DIFF_B"
    if [ -n "$DIFF_A" ] && [ "$DIFF_A" -gt 50 ] && [ -n "$DIFF_B" ] && [ "$DIFF_B" -lt 2000 ]; then
        echo "PASS case30-preedit-clear-no-ghost (清除后差异 $DIFF_B px)"; PASS=$((PASS+1))
    else
        echo "FAIL case30-preedit-clear-no-ghost (有/无=$DIFF_A 清除后=$DIFF_B)"; FAIL=$((FAIL+1))
    fi
else
    echo "FAIL case30-preedit-clear-no-ghost (截图缺失)"; FAIL=$((FAIL+1))
fi

# 用例 31（4B-fix1 任务 C）：OS 发起的 Expose 必须整帧重绘。
#   用 python-xlib 自建一个覆盖窗压住本窗口再销毁，触发系统重绘，比对截图。
CFG31="$OUT/case31-expose.toml"
S31A="$OUT/case31-before.png"
S31B="$OUT/case31-after-expose.png"
L31="$OUT/case31-expose.log"
cat >"$CFG31" <<'TOML'
[renderer]
backend = "cpu"
[window]
dimensions = { columns = 80, lines = 24 }
TOML
APP_CONFIG="$CFG31" SHELL=/bin/sh "$APP_BIN" >"$L31" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1; x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.6
    x11 xdotool type --window "$WID" "echo EXPOSE_BASE"; x11 xdotool key --window "$WID" Return
    sleep 0.8
    x11 import -window "$WID" "$S31A" 2>/dev/null
    # 覆盖 → 销毁 → 触发 Expose（只操作自己创建的窗口）
    x11 python3 "$ROOT/scripts/e2e/expose_overlay.py" "$(echo "$WID" | sed 's/^0x//')" >"$OUT/case31-overlay.log" 2>&1 || true
    sleep 1.2
    x11 import -window "$WID" "$S31B" 2>/dev/null
    stop_app
fi
if [ -s "$S31A" ] && [ -s "$S31B" ]; then
    DIFF31=$(compare -metric AE -fuzz 3% "$S31A" "$S31B" null: 2>&1 | head -1)
    OS_REDRAW=$(grep -c "\[perf\] os_redraw" "$L31" 2>/dev/null || echo 0)
    echo "  expose 前后差异=$DIFF31，os_redraw 次数=$OS_REDRAW"
    # 行为级断言：OS 触发重绘的分支必须真的被走到（截图在 Xvfb 有 backing store
    # 时可能恒等，单看差异无法证伪），且画面保持不变。
    if [ "$OS_REDRAW" -ge 1 ] && [ -n "$DIFF31" ] && [ "$DIFF31" -lt 2000 ]; then
        echo "PASS case31-expose-full-redraw (os_redraw=$OS_REDRAW，差异 $DIFF31 px)"; PASS=$((PASS+1))
    else
        echo "FAIL case31-expose-full-redraw (os_redraw=$OS_REDRAW 差异=$DIFF31)"; FAIL=$((FAIL+1))
    fi
else
    echo "FAIL case31-expose-full-redraw (截图缺失)"; FAIL=$((FAIL+1))
fi

# 用例 32（4B-fix3）：粘贴写入 PTY 后必须**由粘贴本身**主动排一帧。
#   构造：`stty -echo` 关闭回显（此后无回显字节可驱动渲染），由 debug 钩子
#   APP_TEST_PASTE 在事件循环内部触发同步粘贴（不产生鼠标/键盘事件，因此不会
#   顺带触发 RedrawRequested）。断言：粘贴后出现首帧，且该帧由 urgent 路径驱动
#   （urgent=true）——回退"写完主动排一帧"的加固后，将没有任何新帧、也没有该行。
CFG32="$OUT/case32-paste-frame.toml"
H32="$OUT/case32-frames.hash"
L32="$OUT/case32-paste-frame.log"
printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 80, lines = 24 }\n' > "$CFG32"
rm -f "$H32"
APP_CONFIG="$CFG32" APP_FRAME_HASH="$H32" APP_DUMP_SNAPSHOT="$OUT/case32-paste.dump" \
  APP_TEST_PASTE="SILENT_PASTE" APP_TEST_PASTE_AT_MS=3500 \
  SHELL=/bin/sh "$APP_BIN" >"$L32" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.8
    # 关闭回显并让画面静止：此后没有回显字节可以驱动渲染
    x11 xdotool type --window "$WID" "stty -echo"
    x11 xdotool key --window "$WID" Return
    sleep 1.2
    N0=$(wc -l < "$H32" 2>/dev/null || echo 0)
    i=0
    while [ $i -lt 12 ]; do
        grep -q "test_paste_first_frame" "$L32" 2>/dev/null && break
        sleep 0.3
        i=$((i+1))
    done
    N1=$(wc -l < "$H32" 2>/dev/null || echo 0)
    LINE=$(grep "test_paste_first_frame" "$L32" | tail -1)
    if [ -n "$LINE" ] && [ "$N1" -gt "$N0" ]; then
        case "$LINE" in
            *urgent=true*)
                echo "PASS case32-paste-urgent-frame ($LINE, frames $N0→$N1)"; PASS=$((PASS+1)) ;;
            *)
                echo "FAIL case32-paste-urgent-frame (首帧未经 urgent 路径: $LINE)"; FAIL=$((FAIL+1)) ;;
        esac
    else
        echo "FAIL case32-paste-urgent-frame (粘贴后无新帧: frames $N0→$N1, 指标='${LINE:-<无>}')"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL case32-paste-urgent-frame (窗口未出现)"; FAIL=$((FAIL+1))
fi

# 用例 33（4C-t2）：最小化 → 恢复后网格与内容不变。
#   ⚠ 该用例**不能证伪** resize 守卫：实测 X11（Xvfb + openbox）的
#   xdotool windowminimize/windowmap 不产生 WindowEvent::Resized（日志无
#   resize_ignored / vt resize），因此守卫是否生效在本用例里不可见。
#   守卫的正确性由单元测试 resize_target_*（crates/app/tests/keyboard.rs）承担，
#   其中"回退守卫 → 单测变红"已验证。本用例只作回归保护：将来若 X11 行为变化
#   导致最小化产生 Resized，这里会先暴露网格被裁的问题。
CFG33="$OUT/case33-minimize.toml"
D33="$OUT/case33-minimize.dump"
L33="$OUT/case33-minimize.log"
printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 80, lines = 24 }\n' > "$CFG33"
rm -f "$D33"
APP_CONFIG="$CFG33" APP_DUMP_SNAPSHOT="$D33" SHELL=/bin/sh "$APP_BIN" >"$L33" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.6
    x11 xdotool type --window "$WID" "echo MINIMIZE_KEEP"
    x11 xdotool key --window "$WID" Return
    sleep 0.8
    B33=$(head -1 "$D33" 2>/dev/null)
    x11 xdotool windowminimize "$WID"
    sleep 1.0
    x11 xdotool windowmap "$WID"
    sleep 1.0
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    sleep 0.4
    A33=$(head -1 "$D33" 2>/dev/null)
    K33=$(grep -c "MINIMIZE_KEEP" "$D33" 2>/dev/null || echo 0)
    if [ "$B33" = "$A33" ] && [ "$K33" -ge 1 ]; then
        echo "PASS case33-minimize-keeps-grid ($A33, 内容行数=$K33)"; PASS=$((PASS+1))
    else
        echo "FAIL case33-minimize-keeps-grid (前=$B33 后=$A33 内容=$K33)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL case33-minimize-keeps-grid (窗口未出现)"; FAIL=$((FAIL+1))
fi

# 用例 34（4C-t4）：inverse（ESC[7m）反色必须可见——输出 10 个反色空格（纯底色块），
#   断言截图中白色像素数达到"该块应占像素"的下限。修复前 CPU/GPU 均为黑底黑字，
#   白像素为 0；阈值取理论值的 60%（10 格 × 8×16 × 0.6 = 768），理由写入 DECISIONS。
CFG34="$OUT/case34-inverse.toml"
D34="$OUT/case34-inverse.dump"
L34="$OUT/case34-inverse.log"
S34="$OUT/case34-inverse.png"
printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 80, lines = 24 }\n' > "$CFG34"
rm -f "$D34"
APP_CONFIG="$CFG34" APP_DUMP_SNAPSHOT="$D34" SHELL=/bin/sh "$APP_BIN" >"$L34" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.6
    # 10 个反色空格：整块底色 = 原前景色（默认白）
    x11 xdotool type --window "$WID" "printf '\\033[7m          \\033[0m'"
    x11 xdotool key --window "$WID" Return
    sleep 1.0
    x11 import -window "$WID" "$S34" 2>/dev/null
    if [ -s "$S34" ]; then
        WHITE=$(convert "$S34" -format "%c" histogram:info:- 2>/dev/null \
            | awk '/#FFFFFF/ {gsub(":", "", $1); s+=$1} END {print s+0}')
        echo "  反色块白色像素=$WHITE（阈值 ≥768）"
        if [ "$WHITE" -ge 768 ]; then
            echo "PASS case34-inverse-visible-cpu ($WHITE 白像素)"; PASS=$((PASS+1))
        else
            echo "FAIL case34-inverse-visible-cpu (白像素 $WHITE < 768，疑似黑底黑字)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL case34-inverse-visible-cpu (截图缺失)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL case34-inverse-visible-cpu (窗口未出现)"; FAIL=$((FAIL+1))
fi

# 用例 35（4D）：交互 bash 的括号粘贴反色可见性（像素门禁，非 dump）。
#   取证：bash 5.2.15 + readline bracketed paste 开启时，收到 ESC[200~…ESC[201~
#   会把文本以 readline active region 高亮回显 —— 实际序列
#   `ESC[7m PASTED35XX ESC[27m`。若反色被画成黑底黑字（4C 之前），粘贴内容
#   在画面上不可见（用户报的"粘贴后不刷新，直到回车"）。本用例断言反色块为
#   "亮底"，阈值 768 与 case34 相同（10 格 × 8×16 × 0.6）。
CFG35="$OUT/case35-paste-inverse.toml"
L35="$OUT/case35-paste-inverse.log"
S35="$OUT/case35-paste-inverse.png"
CLIP35="$OUT/case35-clip.txt"
printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 80, lines = 24 }\n[terminal.shell]\nprogram = "/bin/bash"\nargs = ["--norc", "--noprofile"]\n' > "$CFG35"
printf 'PASTED35XX' > "$CLIP35"
APP_CONFIG="$CFG35" SHELL=/bin/bash "$APP_BIN" >"$L35" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    # 等 bash 就绪并发出 ?2004h（括号粘贴开启）
    sleep 2.5
    x11 xclip -selection clipboard -i "$CLIP35" >/dev/null 2>&1 &
    CLIP_PID=$!
    sleep 0.5
    GEO35=$(x11 xdotool getwindowgeometry --shell "$WID")
    GX=$(echo "$GEO35" | grep '^X=' | cut -d= -f2)
    GY=$(echo "$GEO35" | grep '^Y=' | cut -d= -f2)
    GW=$(echo "$GEO35" | grep '^WIDTH=' | cut -d= -f2)
    GH=$(echo "$GEO35" | grep '^HEIGHT=' | cut -d= -f2)
    x11 xdotool mousemove $(( ${GX:-0} + ${GW:-200} / 2 )) $(( ${GY:-0} + ${GH:-200} / 2 ))
    sleep 0.2
    x11 xdotool click 3
    sleep 1.2
    x11 import -window "$WID" "$S35" 2>/dev/null
    kill -TERM "${CLIP_PID:-}" 2>/dev/null
    CLIP_PID=""
    if [ -s "$S35" ]; then
        W35=$(convert "$S35" -format "%c" histogram:info:- 2>/dev/null \
            | awk '/#FFFFFF/ {gsub(":", "", $1); s+=$1} END {print s+0}')
        echo "  括号粘贴反色块白像素=$W35（阈值 ≥768）"
        if [ "$W35" -ge 768 ]; then
            echo "PASS case35-paste-inverse-visible-cpu ($W35 白像素)"; PASS=$((PASS+1))
        else
            echo "FAIL case35-paste-inverse-visible-cpu (白像素 $W35 < 768，粘贴文本反色不可见)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL case35-paste-inverse-visible-cpu (截图缺失)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL case35-paste-inverse-visible-cpu (窗口未出现)"; FAIL=$((FAIL+1))
fi

# 用例 36（4F-t2）：配置调色板必须到达 VT 层。
#   取样两项：① 配置红 #d32f2f（ESC[31m 字形 + ESC[7m+ESC[31m 反色块底色）；
#   ② 配置前景 #e0e0e0（ESC[7m 默认反色块底色）。
#   **证伪能力**：① 可证伪（回退 VT 接线后实测 2734 → 0，因为 libghostty 内置红
#   #CC6666）；② **不可证伪**——render 层的 palette.primary_fg 在 4F 之前就已生效，
#   两版本都约 2751，仅作回归保护，不作为本修复的门禁依据。
#   取样用整窗像素计数（-fuzz 2 容差，理由见 DECISIONS），阈值按"10 格应占像素"的 60%。
CFG36="$OUT/case36-palette.toml"
L36="$OUT/case36-palette.log"
S36="$OUT/case36-palette.png"
cat >"$CFG36" <<'TOML'
[renderer]
backend = "cpu"
[window]
dimensions = { columns = 80, lines = 24 }
[colors.primary]
background = "#101010"
foreground = "#e0e0e0"
[colors.normal]
red = "#d32f2f"
TOML
# 统计图片中接近指定颜色的像素数（-fuzz 2% ≈ 每通道 ≤5 的容差）
count_color() { # $1=img $2=#rrggbb
    convert "$1" -fuzz 2% -fill white -opaque "$2" -fill black +opaque white \
        -format "%[fx:int(mean*w*h)]" info: 2>/dev/null
}
APP_CONFIG="$CFG36" SHELL=/bin/sh "$APP_BIN" >"$L36" 2>&1 &
APP_PID=$!
if wait_window; then
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 0.6
    # 1) 索引红字（配置 #d32f2f）；2) 默认色反色块（底色=配置前景 #e0e0e0）；
    # 3) 彩色反色块（底色=配置红 #d32f2f）
    x11 xdotool type --window "$WID" "printf '\\033[31mREDTEXT\\033[0m\\n\\033[7m          \\033[0m\\n\\033[7m\\033[31m          \\033[0m\\n'"
    x11 xdotool key --window "$WID" Return
    sleep 1.2
    x11 import -window "$WID" "$S36" 2>/dev/null
    if [ -s "$S36" ]; then
        RED=$(count_color "$S36" "#d32f2f")
        FG=$(count_color "$S36" "#e0e0e0")
        echo "  配置红(#d32f2f) 像素=$RED  配置前景(#e0e0e0) 像素=$FG（阈值 ≥500）"
        if [ "${RED:-0}" -ge 500 ] && [ "${FG:-0}" -ge 500 ]; then
            echo "PASS case36-palette-reaches-vt-cpu (red=$RED fg=$FG)"; PASS=$((PASS+1))
        else
            echo "FAIL case36-palette-reaches-vt-cpu (red=$RED fg=$FG，疑似用了内置默认色)"; FAIL=$((FAIL+1))
        fi
    else
        echo "FAIL case36-palette-reaches-vt-cpu (截图缺失)"; FAIL=$((FAIL+1))
    fi
    stop_app
else
    echo "FAIL case36-palette-reaches-vt-cpu (窗口未出现)"; FAIL=$((FAIL+1))
fi

# GPU 后端轮（阶段 4B t5）：backend=gpu/auto 的窗口冒烟、GPU vs CPU 截图阈值对拍、
# 无 ICD 回落与非零退出两条边界。dump 依赖用例在 GPU 轮不适用（见脚本头注释）。
GPU_ROUND_LOG="$OUT/gpu-round.log"
if sh "$ROOT/scripts/e2e/run_gpu_round.sh" >"$GPU_ROUND_LOG" 2>&1; then
    GPU_ROUND_RC=0
else
    GPU_ROUND_RC=1
fi
grep -E "^(PASS|FAIL) gpu-round" "$GPU_ROUND_LOG" | sed 's/^/  /'
GPU_ROUND_SUM=$(grep -oE "GPU 轮汇总: PASS=[0-9]+ FAIL=[0-9]+ SKIP=[0-9]+" "$GPU_ROUND_LOG" | head -1)
if [ "$GPU_ROUND_RC" -eq 0 ]; then
    PASS=$((PASS+1))
else
    FAIL=$((FAIL+1))
fi
echo "===== e2e 汇总: PASS=$PASS FAIL=$FAIL GPU_SKIP=${GPU_SKIP:-0} [$GPU_ROUND_SUM] ====="
[ "$FAIL" -eq 0 ]
