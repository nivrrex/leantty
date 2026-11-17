@echo off
cd /d "C:\dev\tty\vt-verify-win\3p-env"
set LEANTTY_PERF_FRAMES=1
set WGPU_BACKEND=dx12&& echo set-ok
"C:\dev\tty\target\release\leantty.exe" --config "C:\dev\tty\vt-verify-win\3p-env\gpu.toml" 2>"C:\dev\tty\vt-verify-win\3p-env\logs\nospace-dx12.log"
echo EXITCODE=%ERRORLEVEL%
