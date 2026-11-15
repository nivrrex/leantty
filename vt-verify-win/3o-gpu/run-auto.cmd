@echo off
cd /d "C:\dev\tty\vt-verify-win\3o-gpu"
set APP_PERF_FRAMES=1
"C:\dev\tty\target\release\app.exe" --config "C:\dev\tty\vt-verify-win\3o-gpu\gpu-auto.toml" 2>"C:\dev\tty\vt-verify-win\3o-gpu\logs\auto.log"
echo EXITCODE=%ERRORLEVEL%
