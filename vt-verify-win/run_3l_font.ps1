param([string]$AppBin = '', [string]$Cfg = '', [string]$Tag = 'A', [int]$Runs = 5, [string]$ErrTarget = 'file')
$ROOT = 'C:\dev\tty'
$env:APP_PERF_FRAMES = '1'
$dir = Join-Path $ROOT 'vt-verify-win\logs\3l-font'
for ($i = 1; $i -le $Runs; $i++) {
    $out = Join-Path $dir ("{0}-{1}.err" -f $Tag, $i)
    $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $out
    # 等首帧后再多留 2 秒，然后只结束本进程句柄
    Start-Sleep -Seconds 9
    $ff = -1
    foreach ($l in Get-Content $out -Encoding UTF8 -ErrorAction SilentlyContinue) { if ($l -match 'first_frame_ms=([0-9.]+)') { $ff = [double]$Matches[1] } }
    Write-Host ("FONT {0} run{1} first_frame_ms={2} bytes={3}" -f $Tag, $i, $ff, (Get-Item $out).Length)
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 800
}
