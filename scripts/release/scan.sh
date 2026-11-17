#!/bin/sh
# 发布资产扫描（阶段 5C t4b/t5d/t6d）。
#
# 用法：sh scripts/release/scan.sh <目录或文件…>
#
# 分类（命中行截断到 120 字符，避免长串刷屏）：
#   A 类（身份/环境泄漏）：仓库根/cargo/rustup 路径片段、用户主目录、主机名、内网与回环 IP、
#       凭据特征、git remote 地址 —— 必须处理到零。
#   B 类（依赖内置的通用系统常量，如 C:\Windows\Fonts、/usr/share/fonts）—— 允许，逐条列出。
#   C 类（无法归类，如盘符数据碎片）—— 原样列出，由使用者裁定。
#
# 用户名/主机名在运行时读取，只用于匹配，不写入任何文件。
set -u

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)
USER_NAME=$(whoami 2>/dev/null || echo "")
HOST_SHORT=$(hostname 2>/dev/null | cut -d. -f1)

# A 类（ERE）：
# - 路径形态的身份信息（不匹配裸词，避免二进制里 root/agent 之类的噪声）
# - 主机名与用户名只在"非通用值"时参与匹配
A_PATTERNS="/root/|/home/[A-Za-z0-9._-]+/|/mnt/|/Users/[A-Za-z0-9._-]+/"
[ "$USER_NAME" != "root" ] && A_PATTERNS="$A_PATTERNS|$USER_NAME"
case "$HOST_SHORT" in ""|localhost|build|dev) : ;; *) A_PATTERNS="$A_PATTERNS|$HOST_SHORT" ;; esac
A_PATTERNS="$A_PATTERNS|127\\.0\\.0\\.1|192\\.168\\.[0-9]+\\.[0-9]+|ssh://[a-z]|git@[a-z]|BEGIN [A-Z ]*PRIVATE KEY|ghp_[A-Za-z0-9]{10,}|AKIA[0-9A-Z]{12,}|password=|passwd=|secret=|token=|api[_-]key"

B_PATTERNS='C:\\Windows|AppData\\Local\\Microsoft\\Windows\\Fonts|/usr/share/fonts|/usr/local/share/fonts|\.local/share/fonts'

# C 类：盘符路径（形如 C:\dir 或 C:/dir）
C_PATTERNS='(^|[^A-Za-z0-9])[A-Za-z]:[\\\\/][A-Za-z0-9_.]'

# 仅报告模式（阶段 5C s8）：主机名变体与开发机硬件型号字样。命中只列出，不计入 A/C，不阻塞。
R_PATTERNS='home[-_ ]?pc|office[-_ ]?pc|GT ?710|Tesla P4|RX ?[0-9]{3}|5600U|LTSC'

ALLOW_FILE="$SCRIPT_DIR/scan-allow.txt"
A_LIST=/tmp/scan_a.$$; B_LIST=/tmp/scan_b.$$; C_LIST=/tmp/scan_c.$$; EXEMPT_LIST=/tmp/scan_x.$$; R_LIST=/tmp/scan_r.$$
: > "$A_LIST"; : > "$B_LIST"; : > "$C_LIST"; : > "$EXEMPT_LIST"; : > "$R_LIST"

# 命中是否落在豁免清单里（清单每行： <路径 glob>|<命中正则>|<原因>）
is_allowlisted() {
    f=$1; hit=$2
    [ -f "$ALLOW_FILE" ] || return 1
    grep -vE '^[[:space:]]*(#|$)' "$ALLOW_FILE" | while IFS='|' read -r glob pat reason; do
        case "$f" in $glob) ;; *) continue ;; esac
        printf '%s\n' "$hit" | grep -aqE "$pat" || continue
        printf '%s|%s\n' "$f" "$reason"
        return 0
    done | head -1 | grep -q .
}

scan_one() {
    f=$1
    [ -f "$f" ] || return 0
    case "$f" in */scripts/release/scan.sh) return 0 ;; esac   # 扫描器自身的模式定义不算泄漏
    kind=$(file -b "$f" 2>/dev/null || echo "")
    case "$kind" in
        *ELF*|*PE32*|*"executable"*)
            strings -a -n 6 "$f" 2>/dev/null > /tmp/scan_str.$$; src=/tmp/scan_str.$$ ;;
        *)
            src=$f ;;
    esac
    grep -anE "$A_PATTERNS" "$src" 2>/dev/null | cut -c1-140 > /tmp/scan_a_hits.$$
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        if is_allowlisted "$f" "$line"; then
            printf '%s: %s\n' "$f" "$line" >> "$EXEMPT_LIST"
        else
            printf '%s: %s\n' "$f" "$line" >> "$A_LIST"
        fi
    done < /tmp/scan_a_hits.$$
    grep -anE "$B_PATTERNS" "$src" 2>/dev/null | cut -c1-140 | sed "s|^|$f: |" >> "$B_LIST"
    # C 类是兜底桶：已经归入 B 类（系统常量）的命中不再重复计入 C。
    grep -anE "$C_PATTERNS" "$src" 2>/dev/null | grep -vE 'C:\\path\\to' \
        | grep -avE "$B_PATTERNS" \
        | cut -c1-140 | sed "s|^|$f: |" >> "$C_LIST"
    grep -ainE "$R_PATTERNS" "$src" 2>/dev/null | cut -c1-140 | sed "s|^|$f: |" >> "$R_LIST"
    rm -f /tmp/scan_a_hits.$$
    [ "$src" = "$f" ] || rm -f "$src"
}

for target in "$@"; do
    if [ -d "$target" ]; then
        find "$target" -type f -print0 | while IFS= read -r -d '' f; do scan_one "$f"; done
    else
        scan_one "$target"
    fi
done

X_HITS=$(wc -l < "$EXEMPT_LIST" | tr -d ' ')
R_HITS=$(wc -l < "$R_LIST" | tr -d ' ')
A_HITS=$(wc -l < "$A_LIST" | tr -d ' ')
B_HITS=$(wc -l < "$B_LIST" | tr -d ' ')
C_HITS=$(wc -l < "$C_LIST" | tr -d ' ')
echo "扫描目标：$*"
echo "  A 类命中=$A_HITS  B 类命中=$B_HITS  C 类命中=$C_HITS  豁免=$X_HITS  仅报告=$R_HITS"
[ "$A_HITS" -gt 0 ] && { echo "--- A 类（必须清零）---"; sort -u "$A_LIST" | head -20; }
[ "$B_HITS" -gt 0 ] && { echo "--- B 类（允许）---"; sort -u "$B_LIST" | head -20; }
[ "$C_HITS" -gt 0 ] && { echo "--- C 类（需裁定）---"; sort -u "$C_LIST" | head -20; }
[ "$X_HITS" -gt 0 ] && { echo "--- 豁免（scan-allow.txt）---"; sort -u "$EXEMPT_LIST" | head -20; }
if [ "$R_HITS" -gt 0 ]; then
    echo "--- 仅报告（主机名变体 / 开发机硬件型号，不阻塞）---"
    sort -u "$R_LIST" | head -40
fi
rm -f "$A_LIST" "$B_LIST" "$C_LIST" "$EXEMPT_LIST" "$R_LIST"
[ "$A_HITS" -eq 0 ] && [ "$C_HITS" -eq 0 ]
