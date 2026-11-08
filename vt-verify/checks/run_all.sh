#!/bin/sh
# 一条命令跑全部验证项（当前已实现项）。新增 V 项在此注册。
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
PASS=0; FAIL=0
run() {
    name="$1"; shift
    echo "===== $name ====="
    if sh "$ROOT/checks/$1" >"$ROOT/checks/$name.log" 2>&1; then
        echo "$name: PASS"
        PASS=$((PASS+1))
    else
        echo "$name: FAIL (见 checks/$name.log)"
        tail -20 "$ROOT/checks/$name.log"
        FAIL=$((FAIL+1))
    fi
}
run v1 v1_build.sh
run v1a v1_artifacts.sh
run v2a v2_rebuild.sh
run v2b v2_offline.sh
run v3 v3_check.sh
run v4 v4_grid.sh
run v5 v5_api.sh
run v7 v7_key.sh
run v8 v8_snapshot.sh
run v6 v6_pty.sh
echo "===== 汇总: PASS=$PASS FAIL=$FAIL ====="
[ "$FAIL" -eq 0 ]
