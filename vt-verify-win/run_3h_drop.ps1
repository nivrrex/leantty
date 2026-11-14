param([string]$AppBin = '', [int]$Mb = 5)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$pout = Join-Path $ROOT 'vt-verify-win\logs\phase2b-win\drop-probe.txt'
Remove-Item $pout -ErrorAction SilentlyContinue
$env:PROBE_OUT = $pout
$a = Start-App2 'gate-drop' (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'DROP no-window'; exit }
$null = Focus-App2 $a.hwnd
Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
Start-Sleep -Seconds 3
$bytes = $Mb * 1024 * 1024
[System.Windows.Forms.Clipboard]::SetText('a' * $bytes)
Start-Sleep -Milliseconds 300
$geom = Get-Geom $a.hwnd
$c = Cell-XY $geom 120 30 2 40
Click-Left $c.x $c.y
Start-Sleep -Milliseconds 200
Click-Right $c.x $c.y
# Ctrl-C immediately: pending paste must be dropped by the writer queue
Set-Mod 0x11 $true
Send-Vk2 0x43
Set-Mod 0x11 $false
Start-Sleep -Seconds 6
$seen = -1
if (Test-Path $pout) {
    $t = Get-Content $pout -Raw -Encoding UTF8
    if ($t -match 'BYTES=(\d+)') { $seen = [int]$Matches[1] }
}
$alive = -not $a.proc.HasExited
Write-Host ("DROP paste={0}MB delivered={1} expect<{2} alive={3}" -f $Mb, $seen, $bytes, $alive)
Stop-App2 $a.proc
