#!/bin/sh
# gates.sh —— 阶段 4L t2：打 tag 前的机械门禁（唯一入口，替代"手工逐条跑"）。
#
# 用法：sh scripts/gates.sh          （需要在仓库根目录；构建所需环境变量由调用方提供）
#
# 依次执行并逐项报告：
#   1) cargo test --workspace
#   2) cargo clippy --workspace --all-targets -- -D warnings
#   3) cargo clippy --release --workspace --all-targets -- -D warnings
#   4) cargo build --release（统计 warning 必须为 0）
#   5) scripts/e2e/run_all.sh
#   6) Windows 目标：依赖树中 wgpu/pollster 命中为 0
#   7) Windows 目标：cargo check --target x86_64-pc-windows-gnu --workspace --tests
#
# 全部通过**且工作区干净**时，把当前 HEAD 的完整 hash 与时间写入 <git-dir>/gates_ok
# （不入库）；任一失败则删除该文件并以非零退出，防止"tag 打在未过门禁的提交上"。
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2

GIT_DIR=$(git rev-parse --git-dir 2>/dev/null) || { echo "FATAL: 不在 git 仓库内"; exit 2; }
OK_FILE="$GIT_DIR/gates_ok"
rm -f "$OK_FILE"

fail() {
    echo "GATES: FAIL — $1"
    rm -f "$OK_FILE"
    exit 1
}

step() { printf '\n=== gates: %s\n' "$1"; }

# 工作区必须干净（避免把未提交改动算进门禁结果）
if [ -n "$(git status --porcelain)" ]; then
    git status --porcelain | head -5
    fail "工作区不干净（先提交或 stash）"
fi

step "1/7 cargo test --workspace"
cargo test --workspace >/tmp/gates_test.log 2>&1 || { tail -30 /tmp/gates_test.log; fail "cargo test"; }
grep -E "^test result:" /tmp/gates_test.log | tail -3

step "2/7 clippy (debug, -D warnings)"
cargo clippy --workspace --all-targets -- -D warnings >/tmp/gates_clippy_dbg.log 2>&1 \
    || { tail -30 /tmp/gates_clippy_dbg.log; fail "clippy debug"; }

step "3/7 clippy (release, -D warnings)"
cargo clippy --release --workspace --all-targets -- -D warnings >/tmp/gates_clippy_rel.log 2>&1 \
    || { tail -30 /tmp/gates_clippy_rel.log; fail "clippy release"; }

step "4/7 cargo build --release（告警必须为 0）"
cargo build --release >/tmp/gates_build.log 2>&1 || { tail -30 /tmp/gates_build.log; fail "release build"; }
WARN=$(grep -cE "^warning" /tmp/gates_build.log || true)
echo "release 告警数=$WARN"
[ "${WARN:-1}" -eq 0 ] || fail "release 构建有 $WARN 条告警"

step "5/7 scripts/e2e/run_all.sh"
sh scripts/e2e/run_all.sh >/tmp/gates_e2e.log 2>&1 || { tail -40 /tmp/gates_e2e.log; fail "e2e"; }
grep -E "e2e 汇总" /tmp/gates_e2e.log | tail -1
grep -E "^FAIL" /tmp/gates_e2e.log && fail "e2e 有 FAIL 项"

step "6/7 Windows 依赖树（wgpu/pollster 必须为 0）"
HITS=$(cargo tree --target x86_64-pc-windows-gnu -e normal 2>/dev/null | grep -cE "wgpu|pollster" || true)
echo "命中数=$HITS"
[ "${HITS:-1}" -eq 0 ] || fail "Windows 目标依赖树出现 wgpu/pollster（$HITS 条）"

step "7/7 Windows 目标检查"
cargo check --target x86_64-pc-windows-gnu --workspace --tests >/tmp/gates_win.log 2>&1 \
    || { tail -30 /tmp/gates_win.log; fail "windows-gnu 目标检查"; }

# 再次确认工作区干净（run_all 不应修改受版本控制的文件）
[ -z "$(git status --porcelain)" ] || { git status --porcelain | head -5; fail "门禁过程中工作区变脏"; }

HEAD_HASH=$(git rev-parse HEAD)
{
    echo "hash=$HEAD_HASH"
    echo "time=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} >"$OK_FILE"
echo
echo "GATES: PASS — HEAD=$HEAD_HASH（已写入 $OK_FILE）"
