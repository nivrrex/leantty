#!/bin/sh
# 构建 leantty 的 .deb（阶段 5C t4）。
#
# 用法：sh scripts/release/build-deb.sh [输出目录]
#   输出目录默认在仓库根的上一级下（leantty-release-assets），可用参数覆盖。
#
# 要点：
# - `cargo build --release --locked`，RUSTFLAGS 用 --remap-path-prefix 把仓库根、
#   CARGO_HOME、RUSTUP_HOME 映射到中性前缀（避免把构建机路径写进二进制）。
# - strip 状态沿用 profile 的 strip = "symbols"，脚本不再额外 strip。
# - 打包前对二进制做 A 类扫描（scripts/release/scan.sh），A 类命中即失败。
# - 不改动仓库内任何文件；所有中间产物都在临时目录。
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)
OUT_DIR=${1:-"$(dirname -- "$ROOT")/leantty-release-assets"}
PKG=leantty
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
ARCH=amd64

mkdir -p "$OUT_DIR"
OUT_DEB="$OUT_DIR/${PKG}_${VER}_${ARCH}.deb"
[ ! -e "$OUT_DEB" ] || { echo "输出目录已有 $OUT_DEB，不覆盖" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "== 1/6 构建（--locked，路径重映射）=="
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME_DIR=${RUSTUP_HOME:-$HOME/.rustup}
export RUSTFLAGS="--remap-path-prefix=$ROOT=/build/leantty --remap-path-prefix=$CARGO_HOME_DIR=/build/cargo --remap-path-prefix=$RUSTUP_HOME_DIR=/build/rustup"
(cd "$ROOT" && cargo build --release --locked)
BIN="$ROOT/target/release/leantty"
[ -x "$BIN" ] || { echo "缺少 $BIN" >&2; exit 1; }
strip_state=$(file -b "$BIN" | tr ',' '\n' | grep -ci "stripped" || true)
echo "  二进制 $(wc -c < "$BIN") B，stripped 标记=$strip_state"

echo "== 2/6 二进制 A 类扫描 =="
sh "$SCRIPT_DIR/scan.sh" "$BIN"

echo "== 3/6 组装包内容 =="
PKGROOT="$WORK/root"
install -d "$PKGROOT/usr/bin" "$PKGROOT/usr/share/applications" \
    "$PKGROOT/usr/share/doc/$PKG/examples"
install -m 0755 "$BIN" "$PKGROOT/usr/bin/$PKG"

install -m 0644 "$ROOT/packaging/linux/$PKG.desktop" "$PKGROOT/usr/share/applications/$PKG.desktop"

for s in 16 24 32 48 64 128 256 512; do
    d="$PKGROOT/usr/share/icons/hicolor/${s}x${s}/apps"
    install -d "$d"
    install -m 0644 "$ROOT/assets/icon/png/$PKG-$s.png" "$d/$PKG.png"
done
install -d "$PKGROOT/usr/share/icons/hicolor/scalable/apps"
install -m 0644 "$ROOT/assets/icon/$PKG.svg" "$PKGROOT/usr/share/icons/hicolor/scalable/apps/$PKG.svg"

DOC="$PKGROOT/usr/share/doc/$PKG"
install -m 0644 "$ROOT/README.md" "$DOC/README.md"
install -m 0644 "$ROOT/README.zh-CN.md" "$DOC/README.zh-CN.md"
install -m 0644 "$ROOT/THIRD-PARTY-NOTICES.md" "$DOC/THIRD-PARTY-NOTICES.md"
install -m 0644 "$ROOT/CHANGELOG.md" "$DOC/CHANGELOG.md"
install -m 0644 "$ROOT/config/leantty.linux.toml" "$DOC/examples/leantty.toml"

cat > "$DOC/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: $PKG

Files: *
Copyright: leantty contributors
License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in all
 copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 SOFTWARE.
 .
 Third-party components and their licenses are listed in
 /usr/share/doc/$PKG/THIRD-PARTY-NOTICES.md.
EOF

echo "== 4/6 依赖推导（ldd 解析出的真实路径 + dlopen soname → dpkg -S 精确路径）=="
# 只按"真实文件路径"查包，避免 dpkg -S 通配把多架构/wps 之类的无关包匹配进来。
pkg_of_path() {
    dpkg -S "$1" 2>/dev/null | head -1 | cut -d: -f1
}
DEPS=""
add_dep() {
    pkg=$1
    [ -n "$pkg" ] || return 0
    case "$pkg" in libc6|libgcc-s1|libc6-dev) pkg=libc6 ;; esac
    # GPU 后端是可选的：Vulkan 加载器只作为 Recommends（见 control 文件）。
    [ "$pkg" = "libvulkan1" ] && return 0
    case " $DEPS " in *" $pkg "*) return 0 ;; esac
    DEPS="$DEPS $pkg"
    echo "  + $pkg"
}
for so in $(readelf -d "$BIN" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p'); do
    path=$(ldd "$BIN" | awk -v n="$so" '$1==n {print $3}' | head -1)
    pkg=$(pkg_of_path "$path")
    echo "  NEEDED $so → ${path:-?} → ${pkg:-?}"
    add_dep "$pkg"
done
# dlopen 的库不在 NEEDED 里（winit 对 X11/xkbcommon/wayland 用 dlopen），
# 按 soname 在标准库目录里找到真实文件再查包。
# 注意：字符类里不能含 `.`，否则会把 strings 里粘连的 "libX11.so.6libX11.so" 整段当成一个名字。
for so in $(strings -a "$BIN" | grep -oE 'lib[A-Za-z0-9_+-]+\.so(\.[0-9]+)?' | sort -u); do
    # 优先用 SONAME 的真实文件（ldconfig 缓存的带版本号路径），避免匹配到 -dev 包的
    # 无版本号符号链接（如 libXi.so → libxi-dev，运行时实际是 libXi.so.6 → libxi6）。
    found=$(ldconfig -p 2>/dev/null | awk -v n="$so" '$1==n {print $NF; exit}')
    [ -n "$found" ] || continue
    [ -e "$found" ] || continue
    real=$(readlink -f "$found" 2>/dev/null || echo "$found")
    pkg=$(pkg_of_path "$real")
    [ -n "$pkg" ] || pkg=$(pkg_of_path "$found")
    [ -n "$pkg" ] || continue
    echo "  dlopen $so → $real → $pkg"
    add_dep "$pkg"
done
DEPS=$(printf '%s' "$DEPS" | sed 's/^ *//; s/ /, /g')
[ -n "$DEPS" ] || DEPS="libc6"
echo "  Depends: $DEPS"

echo "== 5/6 control / changelog / 打包 =="
mkdir -p "$WORK/debian-tmp/DEBIAN"
install -m 0755 -d "$WORK/debian-tmp/DEBIAN"
{
    echo "Package: $PKG"
    echo "Version: $VER"
    echo "Architecture: $ARCH"
    echo "Section: x11"
    echo "Priority: optional"
    echo "Maintainer: leantty <noreply@example.invalid>"
    echo "Depends: $DEPS"
    echo "Recommends: libvulkan1"
    echo "Description: lightweight terminal emulator with CPU and GPU rendering"
    echo " A terminal emulator built on libghostty for VT parsing, with a software"
    echo " rasterizer and an optional wgpu (Vulkan) backend."
} > "$WORK/debian-tmp/DEBIAN/control"
chmod 0644 "$WORK/debian-tmp/DEBIAN/control"
cp -a "$PKGROOT/." "$WORK/debian-tmp/"

# 注意：$PKGROOT 在上一行已经 cp 到 $WORK/debian-tmp，此处必须直接写进 debian-tmp，
# 否则 changelog.Debian.gz 不会被打进包（0.1.1 与 0.2.0 首版实测缺失）。
DOCPKG="$WORK/debian-tmp/usr/share/doc/$PKG"
{
    echo "$PKG ($VER) unstable; urgency=medium"
    echo
    echo "  * Kitty keyboard protocol input (Ctrl+C/D/P) fixed; function keys,
     Insert/Delete/PageUp/PageDown and Ctrl+[ now produce input."
    echo
    echo " -- leantty <noreply@example.invalid>  $(date -R)"
} > "$DOCPKG/changelog.Debian"
gzip -9 -n "$DOCPKG/changelog.Debian"
chmod 0644 "$DOCPKG/changelog.Debian.gz"

# 规范化权限：目录 0755、文件 0644、二进制 0755
find "$WORK/debian-tmp" -type d -exec chmod 0755 {} +
find "$WORK/debian-tmp" -type f ! -path "*/usr/bin/*" -exec chmod 0644 {} +
chmod 0755 "$WORK/debian-tmp/usr/bin/$PKG"

dpkg-deb --build --root-owner-group "$WORK/debian-tmp" "$OUT_DEB" >/dev/null
echo "  已生成 $OUT_DEB"

echo "== 6/6 静态验证 =="
dpkg-deb --info "$OUT_DEB" | sed 's/^/  /'
dpkg-deb --contents "$OUT_DEB" | sed 's/^/  /'
if command -v lintian >/dev/null 2>&1; then
    lintian "$OUT_DEB" || true
else
    echo "  lintian 未安装 → 未测"
fi
echo "大小=$(wc -c < "$OUT_DEB") B"
echo "sha256=$(sha256sum "$OUT_DEB" | cut -d' ' -f1)"
