param([string]$AppBin = '')
. 'C:\dev\tty\scripts\windows\e2e\wb2_lib.ps1' -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$a = Start-App2 'c1-sideload' (Join-Path $CFG 'clip-win.toml')
$null = Focus-App2 $a.hwnd
Send-Text2 'powershell -NoProfile -Command "([string][char]0x41)*200"'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'AAAAAAAAAA' 30
Start-Sleep -Milliseconds 1200
$g = Dump-Grid $a.dump
$rA = -1
for ($k = 0; $k -lt 8 -and $rA -lt 0; $k++) { $gg = Dump-Grid $a.dump; $rA = Find-RowContaining $gg.grid ('A' * 40); if ($rA -lt 0) { Start-Sleep -Milliseconds 400 } }
$geom = Get-Geom $a.hwnd
$cp = Cell-XY $geom $g.cols $g.rows 11 5
Click-Left $cp.x $cp.y
Start-Sleep -Milliseconds 350
$c0 = Cell-XY $geom $g.cols $g.rows $rA 0
$c1 = Cell-XY $geom $g.cols $g.rows ($rA + 2) ($g.cols - 3)
[System.Windows.Forms.Clipboard]::Clear()
Drag-Left $c0.x $c0.y $c1.x $c1.y
Start-Sleep -Milliseconds 300
Click-Middle $c1.x $c1.y
Start-Sleep -Milliseconds 600
$clip = [System.Windows.Forms.Clipboard]::GetText()
$segs = $clip -split "`n"
$maxRun = 0; $cur = 0
foreach ($ch in $clip.ToCharArray()) { if ($ch -eq 'A') { $cur++; if ($cur -gt $maxRun) { $maxRun = $cur } } else { $cur = 0 } }
Write-Host ("SIDELOAD segs=" + $segs.Count + " maxARun=" + $maxRun + " clipLen=" + $clip.Length)
Stop-App2 $a.proc
