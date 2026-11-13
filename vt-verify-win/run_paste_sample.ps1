# t7: paste stall sampling under the sideloaded ConPTY.
# Pastes 1 MB into the raw reader and samples how many bytes it has received.
param([string]$AppBin = '', [int]$Mb = 1, [int]$CapSec = 100)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$pout = Join-Path $ROOT 'vt-verify-win\logs\phase2b-win\paste-sample.txt'
Remove-Item $pout -ErrorAction SilentlyContinue
$env:PROBE_OUT = $pout
$a = Start-App2 'paste-sample' (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "PSAMPLE no-window"; exit }
$null = Focus-App2 $a.hwnd
Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
Start-Sleep -Seconds 2
$bytes = $Mb * 1024 * 1024
[System.Windows.Forms.Clipboard]::SetText('a' * $bytes)
Start-Sleep -Milliseconds 400
$g0 = Dump-Grid $a.dump
$geom = Get-Geom $a.hwnd
$c = Cell-XY $geom $g0.cols $g0.rows 1 40
Click-Left $c.x $c.y
Start-Sleep -Milliseconds 400
$t0 = Get-Date
Click-Right $c.x $c.y
$marks = @(5, 15, 30, 60, 90)
$last = -1
foreach ($m in $marks) {
    if ($m -gt $CapSec) { break }
    while (((Get-Date) - $t0).TotalSeconds -lt $m) { Start-Sleep -Milliseconds 300 }
    $cur = -1
    if (Test-Path $pout) {
        $t = Get-Content $pout -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($t -match 'BYTES=(\d+)') { $cur = [int]$Matches[1] }
    }
    $delta = if ($last -ge 0 -and $cur -ge 0) { $cur - $last } else { -1 }
    Write-Host ("PSAMPLE t={0}s bytes={1} delta={2}" -f $m, $cur, $delta)
    $last = $cur
}
Write-Host ("PSAMPLE alive=" + (-not $a.proc.HasExited))
Stop-App2 $a.proc
