@echo off
rem Zig target injection wrapper (log-free):
rem only for `zig build ...` without any -Dtarget arg, inject
rem -Dtarget=x86_64-windows-gnu right after `build`.
rem Real zig exe path comes from env ZIG_REAL (no absolute path in repo).
if not defined ZIG_REAL (
    echo zig-inject-target: ZIG_REAL env var not set >&2
    exit /b 1
)
setlocal EnableDelayedExpansion
set "inject=0"
if /I "%~1"=="build" (
    set "args=%*"
    echo !args! | findstr /C:"-Dtarget" >nul
    if errorlevel 1 set inject=1
)
if !inject! equ 1 (
    rem %* does not change after shift; strip the leading "build " by length.
    set "args=%*"
    set "args=!args:~6!"
    "%ZIG_REAL%" build -Dtarget=x86_64-windows-gnu !args!
) else (
    "%ZIG_REAL%" %*
)
