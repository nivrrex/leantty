# t4a: main-thread stall while pasting on the REAL path (release build).
# Clipboard is set via .NET, paste is triggered by a right-click on our own window
# (the configured binding). The foreground shell runs `ping -n 20 127.0.0.1`
# (a program that does not read stdin), and mouse moves are injected every 40 ms
# during the paste window so the event loop keeps running.
param(
    [string]$AppBin = '',
    [int]$Mb = 1,
    [string]$Tag = 'paste',
    [int]$Runs = 1
)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$env:APP_PERF_FRAMES = '1'

for ($run = 1; $run -le $Runs; $run++) {
    $a = Start-App2 ("t4-$Tag-$Mb-$run") (Join-Path $CFG 'clip-win.toml')
    if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T4 no-window"; continue }
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Milliseconds 900
    # foreground: a program that does not consume stdin
    Send-Text2 'ping -n 20 127.0.0.1'; Send-Vk2 0x0D
    Start-Sleep -Seconds 3
    $bytes = $Mb * 1024 * 1024
    [System.Windows.Forms.Clipboard]::SetText('a' * $bytes)
    Start-Sleep -Milliseconds 400
    $g0 = Dump-Grid $a.dump
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom $g0.cols $g0.rows 2 40
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 300
    # count loop_gap lines emitted so far: only later ones belong to the paste
    $preGap = 0
    if (Test-Path $a.err) {
        $preGap = @(Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match '^\[perf\] loop_gap_ms=' }).Count
    }
    $t0 = Get-Date
    # paste via the configured Right-click binding, then keep the loop busy
    Click-Right $c.x $c.y
    $elapsed = 0
    while ($elapsed -lt 60) {
        Move-To ($c.x + ($elapsed % 5)) ($c.y + ($elapsed % 3))
        Start-Sleep -Milliseconds 40
        $elapsed += 40
        if ($elapsed -ge 8000) { break }
    }
    # keep the loop busy for another 6 s so the 5 s loop_gap report covers the
    # window that contains the paste
    $keep = (Get-Date)
    while (((Get-Date) - $keep).TotalMilliseconds -lt 6000) {
        Move-To ($c.x + 3) ($c.y + 3)
        Start-Sleep -Milliseconds 40
    }
    $ms = [int]((Get-Date) - $t0).TotalMilliseconds
    # collect loop gaps from stderr (only lines printed after the paste started)
    $all = @()
    if (Test-Path $a.err) {
        foreach ($line in Get-Content $a.err -Encoding UTF8) {
            if ($line -match '^\[perf\] loop_gap_ms=(\d+)') { $all += [int]$Matches[1] }
        }
    }
    $gaps = @()
    for ($i = $preGap; $i -lt $all.Count; $i++) { $gaps += $all[$i] }
    $maxGap = if ($gaps.Count -gt 0) { ($gaps | Measure-Object -Maximum).Maximum } else { -1 }
    $frames = ''
    if (Test-Path $a.err) {
        $l = Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match '\[perf\] frames' } | Select-Object -Last 1
        if ($l) { $frames = $l }
    }
    Write-Host ("T4 {0} {1}MB run{2} pasteWindowMs={3} preGap={4} gapSamples={5} maxLoopGapMs={6} gaps=[{7}]" -f $Tag, $Mb, $run, $ms, $preGap, $gaps.Count, $maxGap, ($gaps -join ','))
    if ($frames) { Write-Host ("T4 frames: " + $frames) }
    Stop-App2 $a.proc
    Start-Sleep -Seconds 1
}
