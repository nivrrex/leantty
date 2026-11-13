# t5: release throughput via a batch file (marker file signals completion).
# usage: run_throughput2.ps1 -AppBin <release app> -Kind 20k|2m -Cols 120 -Rows 30 -Tag cond
param(
    [string]$AppBin = '',
    [string]$Kind = '20k',
    [int]$Cols = 120,
    [int]$Rows = 30,
    [string]$Tag = 'system',
    [int]$TimeoutSec = 600
)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
$done = Join-Path $ROOT "vt-verify-win\perfdata\done$Kind.txt"
$batch = Join-Path $ROOT "vt-verify-win\perfdata\tp$Kind.cmd"
$env:APP_PERF_FRAMES = '1'

# a config with the requested geometry
$cfgPath = Join-Path $CFG ("tp-$Cols" + "x$Rows.toml")
$src = Get-Content (Join-Path $CFG 'clip-win.toml') -Raw
$src = $src -replace 'dimensions = \{[^}]*\}', ("dimensions = { columns = $Cols, lines = $Rows }")
Set-Content -Path $cfgPath -Value $src -Encoding UTF8

Remove-Item $done -ErrorAction SilentlyContinue
$a = Start-App2 ("tp2-$Tag-$Kind-$Cols" + "x$Rows") $cfgPath
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "TP2 no-window"; exit }
$null = Focus-App2 $a.hwnd
Start-Sleep -Milliseconds 900
# warm the shell so shell startup is not part of the measurement
Send-Text2 'echo warm'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'warm' 20
Start-Sleep -Milliseconds 400
$t0 = Get-Date
Send-Text2 $batch
Send-Vk2 0x0D
$ok = $false
while (((Get-Date) - $t0).TotalSeconds -lt $TimeoutSec) {
    if (Test-Path $done) { $ok = $true; break }
    Start-Sleep -Milliseconds 200
}
$ms = [int]((Get-Date) - $t0).TotalMilliseconds
$frames = ''
if (Test-Path $a.err) {
    $l = Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match '\[perf\] frames' }
    if ($l) { $frames = ($l | Select-Object -Last 1) }
}
Write-Host ("TP2 {0} {1} {2}x{3} done={4} ms={5} | {6}" -f $Tag, $Kind, $Cols, $Rows, $ok, $ms, $frames)
Stop-App2 $a.proc
