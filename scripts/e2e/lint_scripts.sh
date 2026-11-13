#!/bin/sh
# scripts/e2e/lint_scripts.sh —— e2e 脚本安全 lint（阶段 3B 任务 1）
#
# 检查 scripts/e2e 下所有 shell 脚本：
#   1) 禁止 pkill / killall（按进程名全局杀进程，可能误杀用户桌面会话）；
#   2) 禁止裸调用 X11 客户端（xdotool/xclip/xsel/wmctrl/import/xwd/xrandr/xset），
#      必须经 guard.sh 的 `x11` 包装；
#   3) 禁止在脚本内 export/赋值非 :99 的 DISPLAY（guard.sh 除外）。
#
# 退出码：0 通过；1 有违规（逐条打印 文件:行:内容）。
# 用法：sh scripts/e2e/lint_scripts.sh [目录]，默认 lint 自身所在目录。

set -u
DIR="${1:-$(cd "$(dirname "$0")" && pwd)}"
GUARD="$DIR/guard.sh"
rc=0

fail() {
    echo "lint: $*" >&2
    rc=1
}

for f in "$DIR"/*.sh; do
    [ -f "$f" ] || continue
    base=$(basename "$f")
    # guard.sh 是包装/清理的唯一实现处；lint 自身包含禁止模式字符串（用于检测）。
    [ "$base" = "guard.sh" ] && continue
    [ "$base" = "lint_scripts.sh" ] && continue
    # gpu_smoke_display0.sh 是任务书显式授权的"一次性 :0 真机冒烟"：允许继承用户
    # DISPLAY（非 :99）与只读的 xwininfo/xwd 调用；仍受下面的 pkill/killall 检查约束。
    DISPLAY0_SMOKE=no
    [ "$base" = "gpu_smoke_display0.sh" ] && DISPLAY0_SMOKE=yes

    # 1) pkill / killall
    if grep -nE '(^|[^[:alnum:]_])(pkill|killall)([^[:alnum:]_]|$)' "$f" >/tmp/lint_hits 2>/dev/null; then
        while IFS= read -r hit; do
            case "$hit" in
                *'#'*) ;;             # 注释行允许提及（如说明"禁止 pkill"）
                *) fail "$base:$hit 使用了 pkill/killall（禁止按名杀进程，改用 kill_own 自有 PID）" ;;
            esac
        done </tmp/lint_hits
    fi

    # 2) 裸 X11 客户端调用（排除 command -v 探测、注释、x11 包装）
    if [ "$DISPLAY0_SMOKE" = "no" ] && grep -nE '(^|[;&|(`]|[[:space:]])[[:space:]]*(xdotool|xclip|xsel|wmctrl|import|xwd|xrandr|xset)[[:space:]]' "$f" >/tmp/lint_hits2 2>/dev/null; then
        while IFS= read -r hit; do
            case "$hit" in
                *'#'*) continue ;;
                *'command -v'*) continue ;;
                *'for tool in'*) continue ;;
                *'x11 '*) continue ;;
            esac
            fail "$base:$hit 裸调用 X11 客户端（须经 x11 包装，以固定 DISPLAY=:99）"
        done </tmp/lint_hits2
    fi

    # 3) 非 :99 的 DISPLAY 赋值
    if [ "$DISPLAY0_SMOKE" = "no" ] && grep -nE 'export[[:space:]]+DISPLAY=|DISPLAY=' "$f" >/tmp/lint_hits3 2>/dev/null; then
        while IFS= read -r hit; do
            case "$hit" in
                *'#'*) continue ;;
                *':99'*) continue ;;
            esac
            fail "$base:$hit 直接设置非 :99 的 DISPLAY（e2e 只允许 :99）"
        done </tmp/lint_hits3
    fi
done

rm -f /tmp/lint_hits /tmp/lint_hits2 /tmp/lint_hits3
if [ "$rc" -eq 0 ]; then
    echo "lint: scripts/e2e 安全检查通过（无 pkill/killall、无裸 X11 调用、DISPLAY 仅 :99）"
fi
exit "$rc"
