#!/bin/sh
# tag_checked.sh —— 阶段 4L t2：只在门禁通过后打 tag（防"tag 打在未过门禁的提交上"）。
#
# 用法：sh scripts/tag_checked.sh <tag 名>
#
# 仅当下列条件**全部**成立时才执行 `git tag <名>`：
#   1) 工作区干净；
#   2) <git-dir>/gates_ok 存在，且其中的 hash 等于当前 HEAD（由 scripts/gates.sh 写入）；
#   3) 该 tag 名尚不存在；
#   4) 名字符合 ^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$。
# 否则打印原因并以非零退出。脚本内不使用 -f/--force，绝不移动既有 tag。
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2

NAME="${1:-}"
[ -n "$NAME" ] || { echo "用法：sh scripts/tag_checked.sh <tag 名>"; exit 2; }
GIT_DIR=$(git rev-parse --git-dir 2>/dev/null) || { echo "拒绝：不在 git 仓库内"; exit 2; }
OK_FILE="$GIT_DIR/gates_ok"

# 4) 命名规则（先查，避免对非法名做其它操作）
case "$NAME" in
    phase* ) : ;;
    * ) echo "拒绝：tag 名 '$NAME' 不符合 ^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$"; exit 1 ;;
esac
printf '%s' "$NAME" | grep -Eq '^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$' \
    || { echo "拒绝：tag 名 '$NAME' 不符合 ^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$"; exit 1; }

# 1) 工作区干净
if [ -n "$(git status --porcelain)" ]; then
    echo "拒绝：工作区不干净（先把改动提交或 stash）"
    git status --porcelain | head -5
    exit 1
fi

# 2) gates_ok 存在且 hash 匹配
if [ ! -f "$OK_FILE" ]; then
    echo "拒绝：缺少 $OK_FILE（先运行 sh scripts/gates.sh）"
    exit 1
fi
GATED=$(sed -n 's/^hash=//p' "$OK_FILE" | head -1)
HEAD_HASH=$(git rev-parse HEAD)
if [ -z "$GATED" ] || [ "$GATED" != "$HEAD_HASH" ]; then
    echo "拒绝：gates_ok 记录的 hash（${GATED:-空}）与当前 HEAD（$HEAD_HASH）不一致"
    echo "      说明门禁是在别的提交上跑的；请先对当前 HEAD 重新运行 sh scripts/gates.sh"
    exit 1
fi

# 3) 重名
if git rev-parse -q --verify "refs/tags/$NAME" >/dev/null 2>&1; then
    echo "拒绝：tag '$NAME' 已存在（本脚本不移动既有 tag；如需更正请新建带后缀的 tag）"
    exit 1
fi

git tag "$NAME" || { echo "失败：git tag '$NAME'"; exit 1; }
echo "已创建 tag '$NAME' -> $(git rev-parse "$NAME")（门禁 hash 匹配，工作区干净）"
