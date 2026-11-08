#!/bin/sh
# PS-flakiness gauge: run the two powershell tests 3 times in isolation.
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'
for i in 1 2 3; do
  echo "=== PS round $i ==="
  cargo test -p pty --test pty_windows powershell -- --nocapture --test-threads=1 2>&1 | grep -E 'test result|test pty_powershell|panicked|应含'
done
