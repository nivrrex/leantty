#!/bin/sh
# selftest_tag_checked.sh —— 阶段 4L t2：tag_checked.sh 的自测（在临时仓库里跑）。
#
# 覆盖 5 条路径：
#   1) 脏工作区 → 拒绝
#   2) gates_ok 缺失 → 拒绝
#   3) gates_ok 的 hash 与 HEAD 不匹配 → 拒绝
#   4) 重名 tag → 拒绝
#   5) 成功路径 → 创建 tag
# 另验证：非法命名拒绝、脚本内无 -f/--force。
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SCRIPT="$ROOT/scripts/tag_checked.sh"
[ -x "$SCRIPT" ] || { echo "FATAL: 缺 $SCRIPT"; exit 2; }

TMP=$(mktemp -d /tmp/tagselftest.XXXXXX) || exit 2
trap 'rm -rf "$TMP"' EXIT
cd "$TMP" || exit 2

PASS=0
FAIL=0
ok() { echo "  通过：$1"; PASS=$((PASS + 1)); }
bad() { echo "  失败：$1"; FAIL=$((FAIL + 1)); }

# 准备一个最小仓库（tag_checked.sh 只用 git 与 git status，不需要 Cargo 工程）
git init -q repo && cd repo || exit 2
git config user.email selftest@example.com
git config user.name selftest
: >a.txt && git add a.txt && git commit -qm init
# 把被测脚本复制进临时仓库（脚本以自身位置为仓库根，故必须"安装在"被测仓库里），
# 并提交，保证后续步骤从"干净工作区"起步。
mkdir -p scripts && cp "$SCRIPT" scripts/tag_checked.sh
git add scripts && git commit -qm "add tag_checked.sh"
SCRIPT="$PWD/scripts/tag_checked.sh"
GIT_DIR=$(git rev-parse --git-dir)

# 1) 脏工作区
echo dirty >>a.txt
out=$(sh "$SCRIPT" phase4l-test 2>&1); rc=$?
[ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "工作区不干净" && ok "脏工作区被拒绝" || bad "脏工作区未被拒绝（rc=$rc, $out）"
git checkout -q -- a.txt

# 2) gates_ok 缺失
rm -f "$GIT_DIR/gates_ok"
out=$(sh "$SCRIPT" phase4l-test 2>&1); rc=$?
[ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "缺少" && ok "gates_ok 缺失被拒绝" || bad "gates_ok 缺失未被拒绝（rc=$rc）"

# 3) hash 不匹配
printf 'hash=%s\ntime=x\n' "0000000000000000000000000000000000000000" >"$GIT_DIR/gates_ok"
out=$(sh "$SCRIPT" phase4l-test 2>&1); rc=$?
[ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "不一致" && ok "hash 不匹配被拒绝" || bad "hash 不匹配未被拒绝（rc=$rc）"

# 非法命名
printf 'hash=%s\n' "$(git rev-parse HEAD)" >"$GIT_DIR/gates_ok"
out=$(sh "$SCRIPT" nonsense-tag 2>&1); rc=$?
[ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "不符合" && ok "非法命名被拒绝" || bad "非法命名未被拒绝（rc=$rc）"

# 5) 成功路径
out=$(sh "$SCRIPT" phase4l-test 2>&1); rc=$?
if [ "$rc" -eq 0 ] && git rev-parse -q --verify refs/tags/phase4l-test >/dev/null; then
    ok "成功创建 tag"
else
    bad "成功路径失败（rc=$rc, $out）"
fi

# 4) 重名（同一 tag 再打一次）
out=$(sh "$SCRIPT" phase4l-test 2>&1); rc=$?
[ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "已存在" && ok "重名被拒绝" || bad "重名未被拒绝（rc=$rc）"

# 既有 tag 未被移动
if [ "$(git rev-parse phase4l-test)" = "$(git rev-parse HEAD)" ]; then
    ok "既有 tag 未被移动"
else
    bad "既有 tag 被移动了"
fi

# 脚本内不得出现 -f/--force
if grep -qE '\bgit tag[^|]*-f|\bgit tag[^|]*--force' "$SCRIPT"; then
    bad "脚本内出现 -f/--force"
else
    ok "脚本内无 -f/--force"
fi

echo "===== selftest_tag_checked: 通过=$PASS 失败=$FAIL ====="
[ "$FAIL" -eq 0 ]
