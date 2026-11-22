#!/bin/sh
# 括号粘贴端到端探针（手动运行，不属于 run_all 的 131 个用例）：
# 让子进程开 ?2004 并 dump 收到的原始字节，用 debug 钩子注入两段载荷，断言
#   1) 整段只有一个 ESC[200~ … ESC[201~（分块后不得重复标记）；
#   2) 每个换行只有一个 CR（不得出现 CRCR 双回车）；
#   3) 正文字节数与源一致（不截断）。
# 用法：sh scripts/e2e/paste_bracket_probe.sh
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd); OUT=$ROOT/scripts/e2e/out/paste-bracket; rm -rf $OUT; mkdir -p $OUT
E2E_GUARD_DIR="$ROOT/scripts/e2e"; . "$ROOT/scripts/e2e/guard.sh"
printf '[renderer]\nbackend = "cpu"\n[window]\ndimensions = { columns = 100, lines = 30 }\n' > $OUT/cfg.toml
run() { # $1=名称 $2=载荷文件 $3=秒
  for p in $(e2e_stale_pids); do kill_own "$p"; done; sleep 0.4
  e2e_start_xvfb $OUT/x.log >/dev/null 2>&1; XV=$XVFB_PID
  e2e_start_wm $OUT/w.log >/dev/null 2>&1; WM=$WM_PID
  sleep 1
  LEANTTY_CONFIG=$OUT/cfg.toml LEANTTY_TEST_PASTE="@$2" LEANTTY_TEST_PASTE_AT_MS=1200 SHELL=/bin/sh \
    setsid "$ROOT/target/debug/leantty" -e python3 "$ROOT/scripts/e2e/paste_bracket_child.py" $OUT/$1.bin 4 >/dev/null 2>$OUT/$1.log &
  P=$!; W=""
  for k in $(seq 1 150); do W=$(x11 xdotool search --pid $P 2>/dev/null | head -1); [ -n "$W" ] && break; sleep 0.05; done
  sleep 5; kill $P 2>/dev/null; sleep 0.3; kill_own $WM; kill_own $XV; sleep 0.3
  python3 "$ROOT/scripts/e2e/paste_bracket_report.py" "$OUT/$1.bin" "$2" "$1"
}
printf 'line 01 test\nline 02 test\nline 03 test\nline 04 test\nline 05 test\nline 06 test\nline 07 test\n' > $OUT/p7.txt
run small $OUT/p7.txt 4
python3 -c "open('$OUT/p300k.txt','w').write(('\n'.join('line %04d payload'%i for i in range(20000)))+'\n')"
run big $OUT/p300k.txt 4
