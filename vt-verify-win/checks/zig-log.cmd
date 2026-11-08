@echo off
setlocal
rem Log every invocation, then forward to the real zig.
rem This host lacks the Windows SDK / UCRT headers that zig's native msvc ABI
rem needs (LibCStdLibHeaderNotFound). build.rs's own zig_target() table maps
rem x86_64-pc-windows-gnu to x86_64-windows-gnu for cross builds; for a native
rem cargo-gnu build build.rs omits -Dtarget, so zig defaults to native-msvc.
rem Via the sanctioned ZIG env override we insert the same -Dtarget value
rem build.rs itself would pass for that triple. No upstream code changed.
echo %DATE% %TIME% zig %* >> "%ZIG_LOG_FILE%"
if not "%1"=="build" goto forward
echo %* | findstr /C:"-Dtarget=" >nul
if not errorlevel 1 goto forward
set "args=%*"
if "%args:~0,6%"=="build " set "args=%args:~6%"
echo %DATE% %TIME% INJECT -Dtarget=x86_64-windows-gnu >> "%ZIG_LOG_FILE%"
"C:\vt\toolchain\zig\zig.exe" build -Dtarget=x86_64-windows-gnu %args%
exit /b %errorlevel%
:forward
"C:\vt\toolchain\zig\zig.exe" %*
exit /b %errorlevel%
