# t4: default Ctrl+Shift+C on a config without custom keyboard bindings.
# Assertions: clipboard equals the dragged text, and the following typed command
# runs normally (marker file) with no stray "C" inserted into it.
param([string]$AppBin = '', [int]$Runs = 3)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Windows.Forms
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\ctrlshift-default.toml'

for ($run = 1; $run -le $Runs; $run++) {
    $marker = Join-Path $ROOT ("vt-verify-win\osc\t4-marker-$run.txt")
    Remove-Item $marker -ErrorAction SilentlyContinue
    $a = Start-App2 ("t4-$run") $cfg
    if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T4 run$run no-window"; continue }
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Milliseconds 1200
    Send-Text2 'echo CSELECTME'; Send-Vk2 0x0D
    $ok = Wait-Dump2 $a.dump 'CSELECTME' 25
    Start-Sleep -Milliseconds 800
    $g = Dump-Grid $a.dump
    $row = -1
    for ($k = 0; $k -lt 8 -and $row -lt 0; $k++) {
        $gg = Dump-Grid $a.dump
        for ($i = 0; $i -lt $gg.grid.Count; $i++) { if ($gg.grid[$i].Trim() -eq 'CSELECTME') { $row = $i; break } }
        if ($row -lt 0) { Start-Sleep -Milliseconds 400 }
    }
    $geom = Get-Geom $a.hwnd
    [System.Windows.Forms.Clipboard]::Clear()
    if ($row -ge 0) {
        $c0 = Cell-XY $geom $g.cols $g.rows $row 0
        $c1 = Cell-XY $geom $g.cols $g.rows $row 8
        Drag-Left $c0.x $c0.y $c1.x $c1.y
        Start-Sleep -Milliseconds 500
        Set-Mod 0x11 $true
        Set-Mod 0x10 $true
        Send-Vk2 0x43
        Set-Mod 0x10 $false
        Set-Mod 0x11 $false
        Start-Sleep -Milliseconds 700
    }
    $clip = [System.Windows.Forms.Clipboard]::GetText()
    # now a normal command must run; if a stray "C" had been typed it would
    # appear in the command line and the marker would not be created
    Send-Text2 ('echo MARK > ' + $marker)
    Send-Vk2 0x0D
    $seen = $false; $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 15) { if (Test-Path $marker) { $seen = $true; break }; Start-Sleep -Milliseconds 250 }
    $content = if (Test-Path $marker) { (Get-Content $marker -Raw).Trim() } else { '' }
    Write-Host ("T4 run{0} echoSeen={1} row={2} clip='{3}' marker={4} content='{5}'" -f $run, $ok, $row, $clip, $seen, $content)
    Stop-App2 $a.proc
    Start-Sleep -Seconds 1
}
