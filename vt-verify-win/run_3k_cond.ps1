# t3 d/e/i for one ConPTY condition. The caller stages the conpty pair next to
# the app before running this script (conditions S/W/N).
param([string]$AppBin = '', [string]$Tag = 'S')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Windows.Forms
$env:LEANTTY_PERF_FRAMES = '1'
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml'
$pwshCfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\pwsh-win.toml'
if (-not (Test-Path $pwshCfg)) { $pwshCfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\ps-win.toml' }

# ---- d) 20 consecutive pwsh launches through the app (liveness only)
$alive = 0
for ($i = 1; $i -le 20; $i++) {
    $a = Start-App2 ("d-$Tag-$i") $pwshCfg
    if ($a.hwnd -eq [IntPtr]::Zero) { continue }
    Start-Sleep -Milliseconds 2500
    if (-not $a.proc.HasExited) { $alive++ }
    Stop-App2 $a.proc
}
Write-Host ("T3D {0} pwsh launches alive={1}/20" -f $Tag, $alive)

# ---- e) paste delivery (real path, raw reader)
function PasteRun([int]$mb, [string]$sub) {
    $pout = Join-Path $ROOT ("vt-verify-win\logs\phase2b-win\paste-{0}-{1}.txt" -f $Tag, $sub)
    Remove-Item $pout -ErrorAction SilentlyContinue
    $env:PROBE_OUT = $pout
    $a = Start-App2 ("e-$Tag-$sub") $cfg
    if ($a.hwnd -eq [IntPtr]::Zero) { return -1 }
    $null = Focus-App2 $a.hwnd
    Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
    Start-Sleep -Seconds 3
    [System.Windows.Forms.Clipboard]::SetText('a' * ($mb * 1024 * 1024))
    Start-Sleep -Milliseconds 400
    $g = Dump-Grid $a.dump
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom 120 30 2 40
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 200
    Click-Right $c.x $c.y
    $want = $mb * 1024 * 1024
    $t0 = Get-Date
    $seen = -1
    while (((Get-Date) - $t0).TotalSeconds -lt 120) {
        Start-Sleep -Milliseconds 500
        if (Test-Path $pout) {
            $t = Get-Content $pout -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -match 'BYTES=(\d+)') { $seen = [int]$Matches[1]; if ($seen -ge $want) { break } }
        }
    }
    Stop-App2 $a.proc
    $seen
}
foreach ($mb in 1, 5) {
    $vals = @()
    for ($k = 1; $k -le 3; $k++) { $vals += (PasteRun $mb "$mb-$k") }
    Write-Host ("T3E {0} paste{1}MB delivered=[{2}] expect={3}" -f $Tag, $mb, ($vals -join ','), ($mb * 1024 * 1024))
}

# ---- e2) 5MB paste interrupted by Ctrl-C: how many bytes reached the child
$pout = Join-Path $ROOT ("vt-verify-win\logs\phase2b-win\paste-{0}-cc.txt" -f $Tag)
Remove-Item $pout -ErrorAction SilentlyContinue
$env:PROBE_OUT = $pout
$a = Start-App2 ("e-$Tag-cc") $cfg
if ($a.hwnd -ne [IntPtr]::Zero) {
    $null = Focus-App2 $a.hwnd
    Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
    Start-Sleep -Seconds 3
    [System.Windows.Forms.Clipboard]::SetText('a' * (5 * 1024 * 1024))
    Start-Sleep -Milliseconds 400
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom 120 30 2 40
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 150
    Click-Right $c.x $c.y
    Set-Mod 0x11 $true
    Send-Vk2 0x43
    Set-Mod 0x11 $false
    Start-Sleep -Seconds 6
    $seen = -1
    if (Test-Path $pout) { $t = Get-Content $pout -Raw -Encoding UTF8; if ($t -match 'BYTES=(\d+)') { $seen = [int]$Matches[1] } }
    Write-Host ("T3E {0} paste5MB-ctrlC delivered={1}" -f $Tag, $seen)
    Stop-App2 $a.proc
}

# ---- i) take_writer path: ping -t foreground + 5MB paste, max loop gap
$a = Start-App2 ("i-$Tag") $cfg
if ($a.hwnd -ne [IntPtr]::Zero) {
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Milliseconds 900
    Send-Text2 'ping -t 127.0.0.1'; Send-Vk2 0x0D
    Start-Sleep -Seconds 3
    [System.Windows.Forms.Clipboard]::SetText('a' * (5 * 1024 * 1024))
    Start-Sleep -Milliseconds 300
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom 120 30 2 40
    $pre = 0
    if (Test-Path $a.err) { $pre = @(Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match 'loop_gap_ms=' }).Count }
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 150
    Click-Right $c.x $c.y
    $keep = Get-Date
    while (((Get-Date) - $keep).TotalMilliseconds -lt 6500) { Move-To ($c.x + 3) ($c.y + 3); Start-Sleep -Milliseconds 40 }
    $all = @()
    if (Test-Path $a.err) { foreach ($l in Get-Content $a.err -Encoding UTF8) { if ($l -match 'loop_gap_ms=(\d+)') { $all += [int]$Matches[1] } } }
    $new = @()
    for ($x = $pre; $x -lt $all.Count; $x++) { $new += $all[$x] }
    $max = if ($new.Count -gt 0) { ($new | Measure-Object -Maximum).Maximum } else { -1 }
    Write-Host ("T3I {0} loopGapMaxMs={1} samples=[{2}]" -f $Tag, $max, ($new -join ','))
    Stop-App2 $a.proc
}
