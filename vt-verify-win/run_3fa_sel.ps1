param([string]$AppBin = '', [string]$Cfg = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
$dump = Join-Path $OUT '3fa-sel.dump'
Remove-Item $dump -ErrorAction SilentlyContinue
$env:LEANTTY_DUMP_SNAPSHOT = $dump
$err = Join-Path $OUT '3fa-sel.err'
$p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
$Script:FoundPid = [uint64]$p.Id; $Script:FoundClass = 'Window Class'; $Script:FoundHwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40 -and $Script:FoundHwnd -eq [IntPtr]::Zero) {
  [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero); if ($Script:FoundHwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 300 } }
$hwnd = $Script:FoundHwnd
if ($hwnd -eq [IntPtr]::Zero) { Write-Host 'SEL no-window'; Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; exit }
$null = Focus-App2 $hwnd
Start-Sleep -Milliseconds 1500
# no typing needed: the shell banner is already on screen
$ok = Wait-Dump2 $dump 'Active code page' 20
Write-Host ("SEL echoSeen=" + $ok)
Start-Sleep -Milliseconds 800
function Grab([string]$path) {
  $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($hwnd, [ref]$cr)
  $pt = New-Object WB2+POINT; $pt.x=0; $pt.y=0; [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
  $gp = [System.Drawing.Graphics]::FromImage($bmp); $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
}
$p1 = Join-Path $OUT '3fa-sel-before.png'; Grab $p1
# release builds have no dump hook: pick the banner row by pixels instead
$geom = Get-Geom $hwnd
$cellH = [math]::Max(8, [int]($geom.ch / 24))
$y = $geom.oy + $cellH + [int]($cellH / 2)
$x0 = $geom.ox + 4
$x1 = $geom.ox + 240
Drag-Left $x0 $y $x1 $y
Start-Sleep -Milliseconds 700
$row = -2
$p2 = Join-Path $OUT '3fa-sel-after.png'; Grab $p2
$b1 = [System.Drawing.Bitmap]::FromFile($p1); $b2 = [System.Drawing.Bitmap]::FromFile($p2)
$diff = 0
for ($y=0; $y -lt [math]::Min($b1.Height,$b2.Height); $y++) { for ($x=0; $x -lt [math]::Min($b1.Width,$b2.Width); $x++) { if ($b1.GetPixel($x,$y).ToArgb() -ne $b2.GetPixel($x,$y).ToArgb()) { $diff++ } } }
$b1.Dispose(); $b2.Dispose()
Write-Host ("SEL row={0} diffPixels={1}" -f $row, $diff)
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Write-Host ("SEL stoppedPid=" + $p.Id)
