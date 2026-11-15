@echo off
cd /d "C:\dev\tty\vt-verify-win\3p-env"
set APP_PERF_FRAMES=1
set "WGPU_BACKEND=vulkan"
"C:\dev\tty\target\release\app.exe" --config "C:\dev\tty\vt-verify-win\3p-env\gpu.toml" 2>"C:\dev\tty\vt-verify-win\3p-env\logs\quoted-vulkan.log"
echo EXITCODE=%ERRORLEVEL%
