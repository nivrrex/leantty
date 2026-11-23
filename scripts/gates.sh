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

step "1/10 cargo nextest run --all（+ doctest）"
# 项目硬性规定：Rust 测试用 cargo nextest run（约为 cargo test 的 3 倍速度）。
# nextest 不跑 doctest，故单独补 `cargo test --doc`（规则里的唯一例外场景）。
if command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run --all >/tmp/gates_test.log 2>&1 \
        || { tail -40 /tmp/gates_test.log; fail "cargo nextest"; }
    cargo test --workspace --doc >/tmp/gates_doctest.log 2>&1 \
        || { tail -30 /tmp/gates_doctest.log; fail "cargo test --doc"; }
    grep -E "Summary|test result:" /tmp/gates_test.log | tail -2
    grep -E "^test result:" /tmp/gates_doctest.log | tail -2
else
    # 兜底：nextest 不可用时回退 cargo test（需在交付说明里写明已回退）
    echo "WARN: cargo-nextest 不可用，回退 cargo test --workspace" >&2
    cargo test --workspace >/tmp/gates_test.log 2>&1 || { tail -30 /tmp/gates_test.log; fail "cargo test"; }
    grep -E "^test result:" /tmp/gates_test.log | tail -3
fi

step "2/10 clippy (debug, -D warnings)"
cargo clippy --workspace --all-targets -- -D warnings >/tmp/gates_clippy_dbg.log 2>&1 \
    || { tail -30 /tmp/gates_clippy_dbg.log; fail "clippy debug"; }

step "3/10 clippy (release, -D warnings)"
cargo clippy --release --workspace --all-targets -- -D warnings >/tmp/gates_clippy_rel.log 2>&1 \
    || { tail -30 /tmp/gates_clippy_rel.log; fail "clippy release"; }

step "4/10 cargo build --release（告警必须为 0）"
cargo build --release >/tmp/gates_build.log 2>&1 || { tail -30 /tmp/gates_build.log; fail "release build"; }
WARN=$(grep -cE "^warning" /tmp/gates_build.log || true)
echo "release 告警数=$WARN"
[ "${WARN:-1}" -eq 0 ] || fail "release 构建有 $WARN 条告警"

step "5/10 scripts/e2e/run_all.sh"
sh scripts/e2e/run_all.sh >/tmp/gates_e2e.log 2>&1 || { tail -40 /tmp/gates_e2e.log; fail "e2e"; }
# 汇总行必须存在：run_all 中途因 set -u/异常退出时不会打印汇总，此前只检查退出码与
# ^FAIL 行，会让"跑到一半就中断"的 e2e 蒙过去（阶段 5A t3 实测踩到，用未定义变量名导致）。
grep -q "e2e 汇总" /tmp/gates_e2e.log || {
    tail -20 /tmp/gates_e2e.log
    fail "e2e 未跑到汇总（疑似中途中断）"
}
grep -E "e2e 汇总" /tmp/gates_e2e.log | tail -1
grep -E "^FAIL" /tmp/gates_e2e.log && fail "e2e 有 FAIL 项"

step "6/10 Windows 依赖树（wgpu 必须为 30.0.2）"
# 阶段 4P：Windows 目标已启用 wgpu（Vulkan+DX12），不再要求 wgpu 命中为 0；
# 改为断言版本精确等于 30.0.2（本项目硬性锁定，不升级）。
WV=$(cargo tree --target x86_64-pc-windows-gnu -e normal 2>/dev/null | grep -oE "wgpu v[0-9.]+" | sort -u | tr '\n' ' ')
echo "Windows 目标 wgpu: ${WV:-<missing>}"
[ "$(echo "$WV" | tr -d ' ')" = "wgpuv30.0.2" ] || fail "Windows 目标 wgpu 版本不是 30.0.2（实测：${WV:-无}）"

step "7/10 Windows 目标检查"
cargo check --target x86_64-pc-windows-gnu --workspace --tests >/tmp/gates_win.log 2>&1 \
    || { tail -30 /tmp/gates_win.log; fail "windows-gnu 目标检查"; }

step "8/10 wgpu 环境变量与特性防护"
# 阶段 4R t3：wgpu-types 的 env::var 在未启用 std 特性时恒返回 None，因此 wgpu 自己的
# `*::from_env()` 在本构建里全部失效。仓库内不得调用它们（否则是"看起来支持、实际不生效"
# 的陷阱）；且 wgpu-types 不得被启用 std 特性——若将来有人启用，必须同时重新审查
# 环境变量行为（见 PHASE4R-LINUX.md §3）。
BAD_ENV=$(grep -rnE "\b(Backends|InstanceFlags|Dx12Compiler|Gles3MinorVersion|PowerPreference|BackendOptions)::from_env|wgpu::[A-Za-z_]+::from_env" crates/ --include=*.rs 2>/dev/null || true)
if [ -n "$BAD_ENV" ]; then
    echo "$BAD_ENV"
    fail "crates/ 出现对 wgpu 的 from_env 调用（本构建下恒返回 None；请改用自有解析器）"
fi
STDFEAT=$(cargo tree -e features 2>/dev/null | grep -E 'wgpu-types feature "std"' || true)
if [ -n "$STDFEAT" ]; then
    echo "$STDFEAT"
    fail "wgpu-types 启用了 std 特性——需要同时审查环境变量行为（WGPU_* 会突然生效）"
fi
echo "crates/ 无 wgpu from_env 调用；wgpu-types 未启用 std"

step "9/10 Windows 目标 clippy（cfg(windows) 代码的 Linux 侧覆盖）"
# 阶段 5C t2a：Linux 上 `cargo clippy --workspace --all-targets` 编译不了 cfg(windows)
# 分支，Windows 专属代码因此没有 clippy 覆盖。这里用 windows-gnu 目标跑同一套 lint。
if cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings \
    >/tmp/gates_clippy_win.log 2>&1; then
    echo "Windows 目标 clippy 0 告警"
else
    tail -40 /tmp/gates_clippy_win.log
    fail "Windows 目标 clippy 有告警/错误"
fi

step "10/10 rustfmt（全仓，含 Windows 专属文件）"
# 阶段 4U t3：格式化统一后加入门禁。阶段 4W t3：Windows 侧 3T 已把
# crates/pty/src/windows.rs 与 crates/pty/tests/pty_windows.rs 过 rustfmt，
# 两平台 rustfmt 对该两文件输出一致（实测 0 偏差），排除名单已去掉。
if ! FMT_OUT=$(cargo fmt --all --check 2>&1); then
    printf '%s\n' "$FMT_OUT" | head -40
    fail "rustfmt 偏差（需要 cargo fmt --all）"
fi
echo "rustfmt 通过（全仓）"

# 再次确认工作区干净（run_all 不应修改受版本控制的文件）
[ -z "$(git status --porcelain)" ] || { git status --porcelain | head -5; fail "门禁过程中工作区变脏"; }

HEAD_HASH=$(git rev-parse HEAD)
{
    echo "hash=$HEAD_HASH"
    echo "time=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} >"$OK_FILE"
echo
echo "GATES: PASS — HEAD=$HEAD_HASH（已写入 $OK_FILE）"
