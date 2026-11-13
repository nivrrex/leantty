# t4: inverse-video verification (Linux case34 口径: pure-white pixel count >= 768
# for a 10-cell inverse block on a dark theme).
param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Drawing

function Grab([IntPtr]$h, [string]$p) {
  $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($h, [ref]$cr)
  $pt = New-Object WB2+POINT; $pt.x=0; $pt.y=0; [void][WB2]::ClientToScreen($h, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
  $gp = [System.Drawing.Graphics]::FromImage($bmp)
  $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
  $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
}
function CountColor([string]$path, [string]$kind) {
  $b = [System.Drawing.Bitmap]::FromFile($path)
  $white = 0; $red = 0; $nonBlack = 0
  for ($y = 0; $y -lt $b.Height; $y++) {
    for ($x = 0; $x -lt $b.Width; $x++) {
      $c = $b.GetPixel($x, $y)
      if ($c.R -eq 255 -and $c.G -eq 255 -and $c.B -eq 255) { $white++ }
      if ($c.R -gt 150 -and $c.G -lt 100 -and $c.B -lt 100) { $red++ }
      if (($c.R + $c.G + $c.B) -gt 60) { $nonBlack++ }
    }
  }
  $b.Dispose()
  @{ white = $white; red = $red; nonBlack = $nonBlack }
}

$a = Start-App2 't4-inverse' (Join-Path $CFG 'inverse-dark.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T4 no-window"; exit }
$null = Focus-App2 $a.hwnd
$esc = "[char]27"
# (A) default colours: ESC[7m + 10 spaces -> block of foreground colour (white)
Send-Text2 ('powershell -NoProfile -Command "' + $esc + '+''[7m'' + ''          '' + ' + $esc + '+''[0m''"')
Send-Vk2 0x0D
Start-Sleep -Seconds 4
$pA = Join-Path $OUT 't4-a-default.png'; Grab $a.hwnd $pA
$ca = CountColor $pA
Write-Host ("T4 A-default white={0} (threshold 768) red={1}" -f $ca.white, $ca.red)
# (B) coloured: ESC[7m ESC[31m + 10 spaces -> block of red
Send-Text2 ('powershell -NoProfile -Command "' + $esc + '+''[7m'' + ' + $esc + '+''[31m'' + ''          '' + ' + $esc + '+''[0m''"')
Send-Vk2 0x0D
Start-Sleep -Seconds 4
$pB = Join-Path $OUT 't4-b-colored.png'; Grab $a.hwnd $pB
$cb = CountColor $pB
Write-Host ("T4 B-colored red={0} (threshold 768) white={1}" -f $cb.red, $cb.white)
# (C) selection inverse: select the printed normal text line, compare region ink
Send-Text2 'echo SELECTIONTEST'; Send-Vk2 0x0D
Start-Sleep -Seconds 2
$pC0 = Join-Path $OUT 't4-c-before.png'; Grab $a.hwnd $pC0
$g = Dump-Grid $a.dump
$row = (Find-RowRetry $a.dump 'SELECTIONTEST' 6).row
$geom = Get-Geom $a.hwnd
if ($row -ge 0) {
  $c0 = Cell-XY $geom $g.cols $g.rows $row 0
  $c1 = Cell-XY $geom $g.cols $g.rows $row 16
  Drag-Left $c0.x $c0.y $c1.x $c1.y
  Start-Sleep -Milliseconds 600
}
$pC1 = Join-Path $OUT 't4-c-selected.png'; Grab $a.hwnd $pC1
$b1 = [System.Drawing.Bitmap]::FromFile($pC0); $b2 = [System.Drawing.Bitmap]::FromFile($pC1)
$diff = 0
for ($y = 0; $y -lt [math]::Min($b1.Height,$b2.Height); $y++) {
  for ($x = 0; $x -lt [math]::Min($b1.Width,$b2.Width); $x++) {
    if ($b1.GetPixel($x,$y).ToArgb() -ne $b2.GetPixel($x,$y).ToArgb()) { $diff++ }
  }
}
$b1.Dispose(); $b2.Dispose()
Write-Host ("T4 C-selection diffPixels={0} row={1}" -f $diff, $row)
Stop-App2 $a.proc
