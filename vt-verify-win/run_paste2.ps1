# t5 paste deadlock check: paste N MB into a raw-mode reader and wait for the
# full byte count. Release build, so the reader report comes from PROBE_OUT.
param(
    [string]$AppBin = '',
    [int]$Mb = 1,
    [string]$Tag = 'system',
    [int]$TimeoutSec = 180
)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$pout = Join-Path $ROOT "vt-verify-win\logs\phase2b-win\paste-$Tag-$Mb.txt"
Remove-Item $pout -ErrorAction SilentlyContinue
$env:PROBE_OUT = $pout
$env:LEANTTY_PERF_FRAMES = '1'
$a = Start-App2 ("paste-$Tag-$Mb") (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "PASTE no-window"; exit }
$null = Focus-App2 $a.hwnd
Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
$ready = Wait-Dump2 $a.dump 'READY' 25
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
$seen = -1
while (((Get-Date) - $t0).TotalSeconds -lt $TimeoutSec) {
    Start-Sleep -Milliseconds 500
    if (Test-Path $pout) {
        $t = Get-Content $pout -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($t -match 'BYTES=(\d+)') { $seen = [int]$Matches[1]; if ($seen -ge $bytes) { break } }
    }
}
$ms = [int]((Get-Date) - $t0).TotalMilliseconds
$alive = -not $a.proc.HasExited
Write-Host ("PASTE {0} {1}MB ready={2} seen={3} expect={4} ms={5} alive={6}" -f $Tag, $Mb, $ready, $seen, $bytes, $ms, $alive)
Stop-App2 $a.proc
