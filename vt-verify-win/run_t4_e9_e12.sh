#!/bin/sh
# T4 probe runner: E9/E11/E12 (+ rerun E12 sort data).
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'
cargo test -p pty --test pty_windows t4_e9 -- --nocapture --test-threads=1
cargo test -p pty --test pty_windows t4_e11 -- --nocapture --test-threads=1
cargo test -p pty --test pty_windows t4_e12 -- --nocapture --test-threads=1
