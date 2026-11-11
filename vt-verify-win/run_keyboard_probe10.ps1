# Probe10: is the event loop alive? dump mtime over time + app CPU time
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

$env:APP_DUMP_SNAPSHOT = "$OUT\probe10.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe10.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"
for ($i = 1; $i -le 5; $i++) {
    Start-Sleep -Seconds 2
    $mt = (Get-Item $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue).LastWriteTime.ToString('HH:mm:ss.fff')
    $proc2 = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    $cpu = if ($proc2) { $proc2.TotalProcessorTime.ToString() } else { 'gone' }
    "t=$($i)x2s dump-mtime=$mt cpu=$cpu"
}
# provoke: write bytes to the pty from OUTSIDE is not possible; instead touch the window
$proc2 = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
"threads=$($proc2.Threads.Count)"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
"--- err ---"
Get-Content "$OUT\probe10.err" -ErrorAction SilentlyContinue | Select-Object -First 4
