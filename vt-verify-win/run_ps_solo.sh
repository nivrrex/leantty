#!/bin/sh
# PS isolation retest: echo_cn alone (libtest args after --).
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'
cargo test -p pty --test pty_windows -- pty_powershell_echo_cn --exact --nocapture --test-threads=1
