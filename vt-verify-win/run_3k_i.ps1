param([string]$AppBin = '', [string]$Tag = 'S')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Windows.Forms
$env:APP_PERF_FRAMES = '1'
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml'
for ($k = 1; $k -le 2; $k++) {
    $a = Start-App2 ("i-$Tag-$k") $cfg
    if ($a.hwnd -eq [IntPtr]::Zero) { continue }
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
    Click-Left $c.x $c.y; Start-Sleep -Milliseconds 150
    Click-Right $c.x $c.y
    $keep = Get-Date
    while (((Get-Date) - $keep).TotalMilliseconds -lt 6500) { Move-To ($c.x + 3) ($c.y + 3); Start-Sleep -Milliseconds 40 }
    $all = @()
    if (Test-Path $a.err) { foreach ($l in Get-Content $a.err -Encoding UTF8) { if ($l -match 'loop_gap_ms=(\d+)') { $all += [int]$Matches[1] } } }
    $new = @(); for ($x = $pre; $x -lt $all.Count; $x++) { $new += $all[$x] }
    $max = if ($new.Count -gt 0) { ($new | Measure-Object -Maximum).Maximum } else { -1 }
    Write-Host ("T3I2 {0} run{1} max={2} samples=[{3}]" -f $Tag, $k, $max, ($new -join ','))
    Stop-App2 $a.proc
    Start-Sleep -Seconds 1
}
