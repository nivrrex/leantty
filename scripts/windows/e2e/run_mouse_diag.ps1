# Mouse/selection diagnostic: control (drag) then double/triple/shift gestures.
param([string]$AppBin = '')
. (Join-Path $PSScriptRoot 'wb2_lib.ps1') -AppBin $AppBin

$a = Start-App2 'diag-mouse' (Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml')
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "NOHWND"; exit 1 }
Focus-App2 $a.hwnd
Send-Text2 'echo alpha beta gamma'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'alpha beta gamma' 25
Start-Sleep -Milliseconds 800

$g = Dump-Grid $a.dump
$row = Find-Row $g.grid 'alpha beta gamma'
$geom = Get-Geom $a.hwnd
Write-Host "GEOM cw=$($geom.cw) ch=$($geom.ch) ox=$($geom.ox) oy=$($geom.oy) cols=$($g.cols) rows=$($g.rows) row=$row"
Write-Host "FG=$( [WB2]::GetForegroundWindow() -eq $a.hwnd )"

# control: drag selection col0 -> col17, then middle click
[System.Windows.Forms.Clipboard]::Clear()
$c0 = Cell-XY $geom $g.cols $g.rows $row 0
$c17 = Cell-XY $geom $g.cols $g.rows $row 17
Write-Host "CELL0 x=$($c0.x) y=$($c0.y) cellW=$($c0.cellW) cellH=$($c0.cellH)"
Drag-Left $c0.x $c0.y $c17.x $c17.y
Click-Middle $c17.x $c17.y
Start-Sleep -Milliseconds 600
Write-Host "DRAG_CLIP=[$([System.Windows.Forms.Clipboard]::GetText())]"

# double-click on col 7 ('beta' = cols 6..9)
[System.Windows.Forms.Clipboard]::Clear()
$cb = Cell-XY $geom $g.cols $g.rows $row 7
Click-Left $cb.x $cb.y
Start-Sleep -Milliseconds 30
Click-Left $cb.x $cb.y
Start-Sleep -Milliseconds 300
Click-Middle $cb.x $cb.y
Start-Sleep -Milliseconds 600
Write-Host "DBL_CLIP=[$([System.Windows.Forms.Clipboard]::GetText())]"

# triple-click on col 2
[System.Windows.Forms.Clipboard]::Clear()
$c2 = Cell-XY $geom $g.cols $g.rows $row 2
Click-Left $c2.x $c2.y; Start-Sleep -Milliseconds 30
Click-Left $c2.x $c2.y; Start-Sleep -Milliseconds 30
Click-Left $c2.x $c2.y
Start-Sleep -Milliseconds 300
Click-Middle $c2.x $c2.y
Start-Sleep -Milliseconds 600
Write-Host "TRIPLE_CLIP=[$([System.Windows.Forms.Clipboard]::GetText())]"

# shift-click extend col0 -> col17
[System.Windows.Forms.Clipboard]::Clear()
Click-Left $c0.x $c0.y
Start-Sleep -Milliseconds 250
[WB2]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 80
Click-Left $c17.x $c17.y
Start-Sleep -Milliseconds 80
[WB2]::keybd_event(0x10, 0, 0x0002, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 300
Click-Middle $c17.x $c17.y
Start-Sleep -Milliseconds 600
Write-Host "SHIFT_CLIP=[$([System.Windows.Forms.Clipboard]::GetText())]"

Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap $SW, $SH
$gp = [System.Drawing.Graphics]::FromImage($bmp)
$gp.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
$bmp.Save((Join-Path $OUT 'diag-mouse.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$gp.Dispose(); $bmp.Dispose()

Stop-App2 $a.proc
Write-Host "DIAG_DONE"
