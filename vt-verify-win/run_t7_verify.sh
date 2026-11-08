#!/bin/sh
# T7 full verification: workspace tests (continue past failures), clippy, release (timed).
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'

echo "=== T7 start $(date '+%F %T') ==="
echo '=== cargo test --workspace ==='
cargo test --workspace
T_RC=$?
echo "test rc=$T_RC"
echo "=== test end $(date '+%F %T') ==="
echo '=== cargo clippy --workspace --all-targets -- -D warnings ==='
cargo clippy --workspace --all-targets -- -D warnings
C_RC=$?
echo "clippy rc=$C_RC"
echo "=== clippy end $(date '+%F %T') ==="
echo '=== cargo build --release ==='
T0=$(date +%s)
cargo build --release
R_RC=$?
T1=$(date +%s)
echo "release rc=$R_RC seconds=$((T1 - T0))"
echo "=== T7 end $(date '+%F %T') ==="
