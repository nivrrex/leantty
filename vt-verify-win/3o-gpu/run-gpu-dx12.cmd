@echo off
cd /d "C:\dev\tty\vt-verify-win\3o-gpu"
set APP_PERF_FRAMES=1
set WGPU_BACKEND=dx12
"C:\dev\tty\target\release\app.exe" --config "C:\dev\tty\vt-verify-win\3o-gpu\gpu.toml" 2>"C:\dev\tty\vt-verify-win\3o-gpu\logs\gpu-dx12.log"
echo EXITCODE=%ERRORLEVEL%
