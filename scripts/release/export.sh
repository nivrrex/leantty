#!/bin/sh
# 导出"干净的发布源码目录"（阶段 5C t6，白名单方式）。
#
# 用法：sh scripts/release/export.sh [目标目录]
#   默认目标：仓库根的上一级下的 leantty-release；目标已存在且非空则停止（不覆盖）。
#
# 白名单之外的一切都不导出（尤其是 AGENTS.md、过程文档（PHASE*.md、DECISIONS.md、
# WIN-NOTES.md、RELEASE-*.md、HANDOFF-*）、scripts/e2e、scripts/windows/{tonight,e2e}、
# vt-verify-win、.git）。导出后对全目录做泄漏扫描（scripts/release/scan.sh）。
#
# 面向用户的文档在 DOCS 里单列（BUILDING / 性能对照 / 排障）：它们要出现在公开仓库的
# docs/ 下，而发布流程用 `rsync --delete` 把本目录整体覆盖到公开仓库——不在导出里的文档
# 会在下次发布时被删掉（0.2.12 与 0.2.13 各踩过一次）。新增用户文档时**加到这里**。
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)
DEST=${1:-"$(dirname -- "$ROOT")/leantty-release"}

if [ -e "$DEST" ] && [ -n "$(ls -A "$DEST" 2>/dev/null)" ]; then
    echo "目标目录已存在且非空：$DEST（不覆盖）" >&2
    exit 1
fi
mkdir -p "$DEST"

FILES="Cargo.toml Cargo.lock .gitignore LICENSE THIRD-PARTY-NOTICES.md README.md README.zh-CN.md CHANGELOG.md"
DIRS="crates vendor config assets packaging"
# 注意：export.sh 与 scan.sh 属开发侧工具，不导出（s5）。
EXTRA=".cargo/config.toml .config/nextest.toml scripts/gen-icons.sh scripts/release/build-deb.sh scripts/release/gen-notices.sh"
# 面向用户的文档（缺失则跳过：老分支上可能没有这些文件，不影响导出其它内容）。
DOCS="docs/BUILDING.md docs/BENCH-0.2.10.md docs/WIN-PASTE-BRACKETED.md"

for f in $FILES; do
    [ -e "$ROOT/$f" ] || { echo "缺少 $f" >&2; exit 1; }
    cp -a "$ROOT/$f" "$DEST/"
done
for d in $DIRS; do
    [ -d "$ROOT/$d" ] || { echo "缺少 $d/" >&2; exit 1; }
    cp -a "$ROOT/$d" "$DEST/"
done
for f in $EXTRA; do
    [ -e "$ROOT/$f" ] || { echo "缺少 $f" >&2; exit 1; }
    mkdir -p "$DEST/$(dirname -- "$f")"
    cp -a "$ROOT/$f" "$DEST/$f"
done
for f in $DOCS; do
    [ -e "$ROOT/$f" ] || continue
    mkdir -p "$DEST/$(dirname -- "$f")"
    cp -a "$ROOT/$f" "$DEST/$f"
done

# Windows 侧构建脚本（不含 tonight/ 与 e2e/）
mkdir -p "$DEST/scripts/windows"
for f in build.ps1 package.ps1 package-release.ps1; do
    [ -e "$ROOT/scripts/windows/$f" ] && cp -a "$ROOT/scripts/windows/$f" "$DEST/scripts/windows/"
done
[ -d "$ROOT/scripts/windows/conpty" ] && cp -a "$ROOT/scripts/windows/conpty" "$DEST/scripts/windows/"

# 清掉不该出现的构建产物（crates 下若有 target 目录）
find "$DEST" -name target -type d -prune -exec rm -rf {} + 2>/dev/null || true
find "$DEST" -name '.git' -type d -prune -exec rm -rf {} + 2>/dev/null || true

echo "已导出到：$DEST"
echo "文件数=$(find "$DEST" -type f | wc -l) 大小=$(du -sh "$DEST" | cut -f1)"
echo "顶层内容："; ls "$DEST" | sed 's/^/  /'
echo
echo "== 泄漏扫描 =="
sh "$SCRIPT_DIR/scan.sh" "$DEST"
