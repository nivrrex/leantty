#!/bin/sh
# T4 收尾 + T1 结论：全量 pty 测试 + 整工作区构建（仅 rustflags，无链接器包装器）。
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'

echo '=== pty tests (all) ==='
cargo test -p pty -- --nocapture --test-threads=1 || exit 1

echo '=== workspace build (rustflags only, no linker wrapper) ==='
cargo build --workspace || exit 1
echo 'ALL OK'
