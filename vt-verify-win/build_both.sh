#!/usr/bin/env bash
set -u
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='E:\dev\build\billy-target'
echo "=== new build $(date)"
cd /c/dev/tty && cargo build --release -p leantty-app 2>&1 | tail -3
echo "new rc=$?"
export CARGO_TARGET_DIR='E:\dev\build\tty-base-target'
echo "=== base build $(date)"
cd /c/dev/tty-base && cargo build --release -p leantty-app 2>&1 | tail -3
echo "base rc=$?"
echo "=== done $(date)"
