#!/bin/sh
cd /c/dev/tty/vt-verify-win/probe_render || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='E:\dev\build\probe-t'
cargo run 2>&1 | grep -vE '^\s+Compiling|^\s+Running|^\s+Fresh' | tail -30
