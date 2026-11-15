@echo off
cd /d "C:\dev\tty\vt-verify-win\3p-env"
set APP_PERF_FRAMES=1
set WGPU_BACKEND=dx12 && echo set-ok
"C:\dev\tty\target\release\app.exe" --config "C:\dev\tty\vt-verify-win\3p-env\gpu.toml" 2>"C:\dev\tty\vt-verify-win\3p-env\logs\plain-dx12.log"
echo EXITCODE=%ERRORLEVEL%
