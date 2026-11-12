#!/bin/sh
# T1: post-merge full build/test/clippy with timings.
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'
echo "=== T1 start $(date '+%F %T') ==="
T0=$(date +%s)
cargo build --workspace || { echo 'T1 DEBUG BUILD FAIL'; exit 1; }
echo "debug build seconds: $(( $(date +%s) - T0 ))"
T0=$(date +%s)
cargo build --release || { echo 'T1 RELEASE BUILD FAIL'; exit 1; }
echo "release build seconds: $(( $(date +%s) - T0 ))"
T0=$(date +%s)
cargo test --workspace; T_RC=$?
echo "test rc=$T_RC seconds: $(( $(date +%s) - T0 ))"
T0=$(date +%s)
cargo clippy --workspace --all-targets -- -D warnings; C_RC=$?
echo "clippy rc=$C_RC seconds: $(( $(date +%s) - T0 ))"
echo "=== T1 end $(date '+%F %T') ==="
