#!/bin/sh
# scripts/e2e/guard.sh —— e2e X11 安全护栏（阶段 3B 任务 1）
#
# 被 scripts/e2e 下所有脚本 source。目的：让 e2e 永远不会触碰用户桌面会话
# （DISPLAY=:0 及其窗口管理器），所有 X11 操作限定在专用 display :99。
#
# 规则：
#  1) 强制 export DISPLAY=:99；若调用方传入其他 DISPLAY，打印警告并覆盖。
#  2) 所有 X11 客户端（xdotool/xclip/xsel/wmctrl/import/xwd/xrandr/xset）
#     必须经 `x11` 包装调用，包装内再次强制 DISPLAY=:99。
#  3) 禁止按进程名全局杀进程（pkill/killall）；只允许终止自有 PID（kill_own）。
#  4) :99 已被非本脚本进程占用时中止，不抢占；仅清理无对应进程的残留锁。

E2E_DISPLAY=":99"
# guard 自身所在目录（source 时 $0 是调用脚本，故由调用方设置 E2E_GUARD_DIR）。
E2E_GUARD_DIR="${E2E_GUARD_DIR:-}"
E2E_SCREEN_GEOM="${E2E_SCREEN_GEOM:-1920x1440x24}"

# --- 1) 强制 DISPLAY ---
if [ -n "${DISPLAY:-}" ] && [ "$DISPLAY" != "$E2E_DISPLAY" ]; then
    echo "guard: 警告：DISPLAY=${DISPLAY} 不是 e2e 专用 $E2E_DISPLAY，强制覆盖（不使用用户桌面）" >&2
fi
export DISPLAY="$E2E_DISPLAY"

# --- 2) X11 调用包装 ---
# 用法：x11 xdotool key Return / x11 import -window "$WID" out.png
x11() {
    if [ "$DISPLAY" != "$E2E_DISPLAY" ]; then
        echo "guard: FATAL：x11 调用时 DISPLAY=${DISPLAY}，拒绝在非 $E2E_DISPLAY 上操作" >&2
        return 1
    fi
    DISPLAY="$E2E_DISPLAY" "$@"
}

# --- 3) 只终止自有 PID ---
# 用法：kill_own "$PID"（空/0/1 时不动作；只 kill -TERM，不 kill -9 自身以外进程树）
kill_own() {
    _p="${1:-}"
    case "$_p" in
        '' | 0 | 1 | *[!0-9]*) return 0 ;;
    esac
    kill "$_p" 2>/dev/null || true
}

# 按 DISPLAY=:99 特征枚举残留进程（Xvfb 按命令行，openbox 按 environ）。
# 输出 PID 列表；不直接杀，由调用方 kill_own。
e2e_stale_pids() {
    for p in $(pgrep -f "^Xvfb ${E2E_DISPLAY}" 2>/dev/null); do
        echo "$p"
    done
    for p in $(pgrep -x openbox 2>/dev/null); do
        if tr '\0' '\n' < "/proc/$p/environ" 2>/dev/null | grep -qx "DISPLAY=${E2E_DISPLAY}"; then
            echo "$p"
        fi
    done
}

# --- 4) 启动专用 Xvfb（不抢占） ---
# 用法：e2e_start_xvfb <日志路径>；成功时设置 XVFB_PID。
e2e_start_xvfb() {
    _log="${1:-/dev/null}"
    if pgrep -f "^Xvfb ${E2E_DISPLAY}" >/dev/null 2>&1; then
        echo "guard: FATAL：${E2E_DISPLAY} 已被现有 Xvfb 占用（非本次启动），中止而不是抢占" >&2
        return 1
    fi
    if [ -e "/tmp/.X99-lock" ]; then
        # 无对应进程的残留锁（上次异常退出）：清理后启动。
        rm -f "/tmp/.X99-lock" 2>/dev/null || true
    fi
    x11 Xvfb "$E2E_DISPLAY" -screen 0 "$E2E_SCREEN_GEOM" >"$_log" 2>&1 &
    XVFB_PID=$!
    sleep 1
    return 0
}

# 启动专用 openbox（继承 DISPLAY=:99）。
#
# 使用 e2e 自带的最小配置（scripts/e2e/openbox-rc.xml），而不是用户的
# ~/.config/openbox/*.xml：用户配置可能含 <center>yes 等策略，会把窗口强制居中，
# 使 window.position 用例与截图裁剪坐标失效，且让 e2e 行为随用户配置漂移。
e2e_start_wm() {
    _log="${1:-/dev/null}"
    _rc="${E2E_GUARD_DIR:+$E2E_GUARD_DIR/}openbox-rc.xml"
    if [ -f "$_rc" ]; then
        DISPLAY="$E2E_DISPLAY" openbox --config-file "$_rc" >"$_log" 2>&1 &
    else
        echo "guard: 警告：缺少 e2e openbox 配置 $_rc，退回 openbox 默认配置（可能受用户配置影响）" >&2
        DISPLAY="$E2E_DISPLAY" openbox >"$_log" 2>&1 &
    fi
    WM_PID=$!
    sleep 1
    return 0
}
