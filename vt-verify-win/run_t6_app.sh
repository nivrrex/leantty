#!/bin/sh
# T6: rebuild app, launch with relative SHELL=cmd.exe, verify window+proc, repeat for powershell.
cd /c/dev/tty || exit 9
export ZIG='C:\dev\tty\scripts\windows\zig-inject-target.cmd'
export ZIG_REAL='C:\vt\toolchain\zig\zig.exe'
export GHOSTTY_SOURCE_DIR='C:\vt\vendor\ghostty-src'
export GHOSTTY_ZIG_SYSTEM_DIR='C:\vt\zig-system'
export CARGO_TARGET_DIR='C:\dev\tty\target'
cargo build -p app || exit 1
SHELL=cmd.exe nohup /e/dev/build/billy-target/debug/leantty.exe > vt-verify-win/logs/t6-app-cmd4.log 2>&1 &
sleep 10
powershell.exe -NoProfile -Command "Get-Process app -ErrorAction SilentlyContinue | Select-Object Id,MainWindowTitle,Responding | Format-Table -AutoSize" > vt-verify-win/logs/t6-app-cmd4-proc.txt 2>&1
powershell.exe -NoProfile -Command "Get-Process app -ErrorAction SilentlyContinue | Stop-Process -Force"
cat vt-verify-win/logs/t6-app-cmd4-proc.txt
echo '--- powershell variant ---'
SHELL='C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' nohup /e/dev/build/billy-target/debug/leantty.exe > vt-verify-win/logs/t6-app-ps2.log 2>&1 &
sleep 10
powershell.exe -NoProfile -Command "Get-Process app -ErrorAction SilentlyContinue | Select-Object Id,MainWindowTitle,Responding | Format-Table -AutoSize" > vt-verify-win/logs/t6-app-ps2-proc.txt 2>&1
powershell.exe -NoProfile -Command "Get-Process app -ErrorAction SilentlyContinue | Stop-Process -Force"
cat vt-verify-win/logs/t6-app-ps2-proc.txt
echo '--- logs ---'
head -3 vt-verify-win/logs/t6-app-cmd4.log vt-verify-win/logs/t6-app-ps2.log
