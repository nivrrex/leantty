@echo off
cd /d "C:\dev\tty\vt-verify-win\3o-gpu"
set LEANTTY_PERF_FRAMES=1
"C:\dev\tty\target\release\leantty.exe" --config "C:\dev\tty\vt-verify-win\3o-gpu\gpu.toml" 2>"C:\dev\tty\vt-verify-win\3o-gpu\logs\gpu-plain.log"
echo EXITCODE=%ERRORLEVEL%
