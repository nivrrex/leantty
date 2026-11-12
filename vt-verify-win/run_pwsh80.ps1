$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = "$ROOT\vt-verify-win\logs\phase2a-win"
$env:APP_DUMP_SNAPSHOT = "$OUT\pwsh80.dump"
$p = Start-Process -FilePath "$ROOT\target\debug\app.exe" -ArgumentList @('--config', "$OUT\pwsh80.toml") -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\pwsh80.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $pr = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($pr -and $pr.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $pr.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"
Start-Sleep -Seconds 8
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -First 4
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
