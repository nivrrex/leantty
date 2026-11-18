#!/bin/sh
# 生成 leantty 的图标资源（阶段 5A t3）。
#
# 输入：assets/icon/leantty.svg（仓库内；由项目维护者提供，原样使用）
# 输出：
#   assets/icon/png/leantty-{16,24,32,48,64,128,256,512}.png
#   assets/icon/leantty.ico                          （16/24/32/48/64/128/256 多尺寸）
#   crates/app/assets/window-icon-128.rgba           （128x128 原始 RGBA，供 Icon::from_rgba）
#
# 优先用 rsvg-convert（渲染质量稳定，无 ImageMagick 的 Delegate 依赖），
# 缺失时退回 ImageMagick 的 convert（两者都按相对路径调用，不写死任何本机路径）。
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd)
SVG="$ROOT/assets/icon/leantty.svg"
PNG_DIR="$ROOT/assets/icon/png"
ICO="$ROOT/assets/icon/leantty.ico"
RGBA="$ROOT/crates/app/assets/window-icon-128.rgba"
SIZES="16 24 32 48 64 128 256 512"
ICO_SIZES="16 24 32 48 64 128 256"

[ -f "$SVG" ] || { echo "缺少源文件：assets/icon/leantty.svg" >&2; exit 1; }

if command -v rsvg-convert >/dev/null 2>&1; then
    RENDER=rsvg
elif command -v convert >/dev/null 2>&1; then
    RENDER=magick
else
    echo "缺少光栅化工具：需要 rsvg-convert 或 ImageMagick 的 convert" >&2
    exit 1
fi

mkdir -p "$PNG_DIR" "$(dirname -- "$RGBA")"

for s in $SIZES; do
    out="$PNG_DIR/leantty-$s.png"
    if [ "$RENDER" = rsvg ]; then
        rsvg-convert -w "$s" -h "$s" -o "$out" "$SVG"
    else
        convert -background none -density 384 "$SVG" -resize "${s}x${s}" "$out"
    fi
    [ -s "$out" ] || { echo "生成失败：$out" >&2; exit 1; }
done

# 多尺寸 ICO：按尺寸从大到小喂给同一个 convert 调用，保证各尺寸都进同一个 ICO 目录。
ico_args=""
for s in $ICO_SIZES; do
    ico_args="$ico_args $PNG_DIR/leantty-$s.png"
done
# shellcheck disable=SC2086
convert $ico_args "$ICO"
[ -s "$ICO" ] || { echo "生成失败：$ICO" >&2; exit 1; }

# 原始 RGBA：winit 的 Icon::from_rgba 需要未压缩的 RGBA8 字节。
convert "$PNG_DIR/leantty-128.png" -depth 8 "rgba:$RGBA"
[ -s "$RGBA" ] || { echo "生成失败：$RGBA" >&2; exit 1; }
size=$(wc -c < "$RGBA")
[ "$size" -eq $((128 * 128 * 4)) ] || { echo "RGBA 大小异常：$size（应为 65536）" >&2; exit 1; }

# 断言：每个尺寸都非空、含背景色 #14181f，且存在明显区别于底色的前景像素。
# - ≥32px：必须出现主色 #5eead4（青绿）本身；
# - 16/24px：缩放到这么小时青绿会与底色混合（实测 16px 最接近的是 #326c68），
#   因此改为断言"存在与底色通道差 >30 的前景像素"，避免用经验色值硬凑。
for s in $SIZES; do
    f="$PNG_DIR/leantty-$s.png"
    [ -s "$f" ] || { echo "断言失败：$f 为空" >&2; exit 1; }
    hist=$(convert "$f" -format %c histogram:info:- 2>/dev/null)
    bg=$(printf '%s\n' "$hist" | grep -icE '#14181F' || true)
    [ "${bg:-0}" -ge 1 ] || { echo "断言失败：$f 不含背景色 #14181f" >&2; exit 1; }
    if [ "$s" -ge 32 ]; then
        cyan=$(printf '%s\n' "$hist" | grep -icE '#5EEAD4' || true)
        [ "${cyan:-0}" -ge 1 ] || { echo "断言失败：$f 不含主色 #5eead4" >&2; exit 1; }
    else
        fg=$(printf '%s\n' "$hist" | awk -F'[(),]' '{ r=$2; g=$3; b=$4;
            dr=r-20; if (dr<0) dr=-dr; dg=g-24; if (dg<0) dg=-dg; db=b-31; if (db<0) db=-db;
            if (dr>30 || dg>30 || db>30) n+=1 } END { print n+0 }')
        [ "${fg:-0}" -ge 1 ] || { echo "断言失败：$f 无前景像素（与底色差 >30）" >&2; exit 1; }
    fi
done
echo "断言通过：所有尺寸 PNG 非空、含背景色 #14181f，且具备前景图形（≥32px 含主色 #5eead4）"

echo "已生成："
for s in $SIZES; do echo "  assets/icon/png/leantty-$s.png ($(wc -c < "$PNG_DIR/leantty-$s.png") B)"; done
echo "  assets/icon/leantty.ico ($(wc -c < "$ICO") B)"
echo "  crates/app/assets/window-icon-128.rgba ($size B)"
