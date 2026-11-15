param([string]$AppBin = '', [string]$Cfg = '', [string]$Tag = 'NUL', [int]$Runs = 5)
$ROOT = 'C:\dev\tty'
$env:APP_PERF_FRAMES = '1'
for ($i = 1; $i -le $Runs; $i++) {
    # NUL 对照：stdout/stderr 都丢给 NUL，无法事后解析，只测首帧时间
    $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError 'NUL' -RedirectStandardOutput 'NUL'
    Start-Sleep -Seconds 9
    $cpu = 0
    try { $cpu = [int](Get-Process -Id $p.Id).TotalProcessorTime.TotalMilliseconds } catch {}
    Write-Host ("NULRUN {0} run{1} alive={2} cpu_ms={3}" -f $Tag, $i, (-not $p.HasExited), $cpu)
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 800
}
