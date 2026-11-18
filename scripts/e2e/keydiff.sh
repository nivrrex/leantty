#!/bin/sh
# 阶段 6B t1：逐键对拍驱动。对每个按键起一次干净会话，注入 a / 目标键 / a，
# 取中间时间窗内的字节作为该键的实收（前后两个 a 作为"注入确实生效"的证据）。
#
# 用法：sh scripts/e2e/keydiff.sh <leantty|alacritty> <kitty7|legacy|mok2> <out.tsv> [keysfile]
set -eu
ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
TERM_KIND=${1:?}; MODE=${2:?}; OUT_TSV=${3:?}; KEYS=${4:-"$ROOT/scripts/e2e/keylist.txt"}
export DISPLAY=:99
. "$ROOT/scripts/e2e/guard.sh"
DELTA=0.35
rm -f "$OUT_TSV"
for p in $(e2e_stale_pids); do kill_own "$p"; done; sleep 0.3
e2e_start_xvfb /tmp/kd-xvfb.log || exit 1
trap 'kill_own "${P:-}"; kill_own "$XVFB_PID"; kill_own "$WM_PID"' EXIT
e2e_start_wm /tmp/kd-wm.log

run_one() {
    KEY=$1
    LOG=$(mktemp /tmp/kd-XXXX.log)
    if [ "$TERM_KIND" = leantty ]; then
        CFG=$(mktemp /tmp/kd-XXXX.toml)
        printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 100, lines = 24 }\n[terminal.shell]\nprogram = "/usr/bin/python3"\nargs = ["%s/scripts/e2e/keyprobe.py", "--mode", "%s", "--log", "%s", "--settle", "0.4"]\n' \
            "$ROOT" "$MODE" "$LOG" > "$CFG"
        LEANTTY_CONFIG=$CFG SHELL=/bin/sh "$ROOT/target/release/leantty" >/dev/null 2>/dev/null &
        P=$!
    else
        alacritty -e /usr/bin/python3 "$ROOT/scripts/e2e/keyprobe.py" --mode "$MODE" --log "$LOG" --settle 0.4 >/dev/null 2>&1 &
        P=$!
    fi
    i=0
    while [ $i -lt 100 ]; do WID=$(x11 xdotool search --pid "$P" 2>/dev/null | head -1); [ -n "$WID" ] && break; sleep 0.2; i=$((i+1)); done
    x11 xdotool windowactivate "$WID" >/dev/null 2>&1
    x11 xdotool windowfocus "$WID" >/dev/null 2>&1
    sleep 1.2
    x11 xdotool key a; sleep "$DELTA"
    T_KEY_START=$(date +%s%N)
    x11 xdotool key "$KEY"; sleep "$DELTA"
    T_KEY_END=$(date +%s%N)
    x11 xdotool key a; sleep "$DELTA"
    kill -TERM "$P" 2>/dev/null || true
    wait "$P" 2>/dev/null || true
    [ -n "${CFG:-}" ] && rm -f "$CFG"
    # 探针日志的时间戳是相对探针启动的毫秒；换算成本次会话的绝对时间不可行，
    # 因此改为按"IN 记录序号"切分：第 1 条 = 第一个 a，最后一条 = 最后一个 a，
    # 中间的全部属于目标键。
    # 以普通字母 a 的字节（61）作为哨兵：第一个 61 之前的记录是终端对查询的应答，
    # 第一个 61 是注入前置证据、最后一个 61 是注入后置证据，二者之间即目标键的字节。
    HEX=$(awk '
        /^[0-9]+ IN /{ rec[++n]=$3 }
        END{
            first=0; last=0
            for (i=1; i<=n; i++) if (rec[i]=="61") { first=i; break }
            for (i=n; i>=1; i--) if (rec[i]=="61") { last=i; break }
            out=""
            if (first>0 && last>first) for (i=first+1; i<last; i++) out = out rec[i] " "
            sub(/ $/, "", out)
            print out
        }' "$LOG")
    printf '%s\t%s\t%s\n' "$KEY" "$MODE" "$HEX" >> "$OUT_TSV"
    rm -f "$LOG"
}

while IFS= read -r k; do
    case "$k" in ''|'#'*) continue ;; esac
    run_one "$k"
done < "$KEYS"
echo "完成：$OUT_TSV（$(wc -l < "$OUT_TSV") 行）"
