#!/bin/sh
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'
echo '=== vt tests ==='
cargo test -p vt 2>&1 | grep -E 'test result|panicked'
echo '=== render tests ==='
cargo test -p render 2>&1 | grep -E 'test result|panicked|应全为|发现差异'
