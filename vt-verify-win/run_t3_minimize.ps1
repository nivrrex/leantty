# t3: minimise/restore guard re-verification (4C P1).
# Uses the debug build because LEANTTY_DUMP_SNAPSHOT is debug-only.
param([string]$AppBin = '', [int]$Runs = 3)
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
function Header([string]$f) {
  if (-not (Test-Path $f)) { return 'no-dump' }
  return (Get-Content $f -Encoding UTF8 -TotalCount 1)
}
function PixelDiff([string]$p, [string]$q) {
  $b1 = [System.Drawing.Bitmap]::FromFile($p); $b2 = [System.Drawing.Bitmap]::FromFile($q)
  $w = [math]::Min($b1.Width, $b2.Width); $h = [math]::Min($b1.Height, $b2.Height)
  $diff = 0; $dark1 = 0; $dark2 = 0
  for ($y = 0; $y -lt $h; $y++) {
    for ($x = 0; $x -lt $w; $x++) {
      $c1 = $b1.GetPixel($x, $y); $c2 = $b2.GetPixel($x, $y)
      if ($c1.ToArgb() -ne $c2.ToArgb()) { $diff++ }
      if (($c1.R + $c1.G + $c1.B) -lt 300) { $dark1++ }
      if (($c2.R + $c2.G + $c2.B) -lt 300) { $dark2++ }
    }
  }
  $b1.Dispose(); $b2.Dispose()
  @{ diff = $diff; dark1 = $dark1; dark2 = $dark2 }
}

for ($i = 1; $i -le $Runs; $i++) {
  $a = Start-App2 "t3-min-$i" (Join-Path $CFG 'clip-win.toml')
  if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T3 run$i no-window"; continue }
  $null = Focus-App2 $a.hwnd
  Send-Text2 'powershell -NoProfile -Command "Write-Host ''汉字ABC mixed text''; Write-Host ([char]27+''[7m''+''INVERSE LINE''+[char]27+''[0m'')"'
  Send-Vk2 0x0D
  Start-Sleep -Seconds 4
  Write-Host ("T3 run{0} headerNormal: {1}" -f $i, (Header $a.dump))
  $pngBefore = Join-Path $OUT "t3-run$i-before.png"
  Grab $a.hwnd $pngBefore
  Start-Sleep -Milliseconds 800
  $pngNoise = Join-Path $OUT "t3-run$i-noise.png"
  Grab $a.hwnd $pngNoise
  [void][WB2]::ShowWindow($a.hwnd, 6)   # SW_MINIMIZE
  Start-Sleep -Seconds 2
  Write-Host ("T3 run{0} headerMinimised: {1}" -f $i, (Header $a.dump))
  [void][WB2]::ShowWindow($a.hwnd, 9)   # SW_RESTORE
  Start-Sleep -Milliseconds 600
  $null = Focus-App2 $a.hwnd
  Start-Sleep -Milliseconds 1500
  Write-Host ("T3 run{0} headerRestored: {1}" -f $i, (Header $a.dump))
  $pngAfter = Join-Path $OUT "t3-run$i-after.png"
  Grab $a.hwnd $pngAfter
  $noise = PixelDiff $pngBefore $pngNoise
  $res = PixelDiff $pngBefore $pngAfter
  Write-Host ("T3 run{0} noiseDiff={1} restoreDiff={2} darkBefore={3} darkAfter={4}" -f $i, $noise.diff, $res.diff, $res.dark1, $res.dark2)
  Stop-App2 $a.proc
  Start-Sleep -Seconds 1
}

# maximise then restore once
$a = Start-App2 't3-max' (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -ne [IntPtr]::Zero) {
  $null = Focus-App2 $a.hwnd
  Send-Text2 'echo MAXTEST'; Send-Vk2 0x0D
  $null = Wait-Dump2 $a.dump 'MAXTEST' 20
  Start-Sleep -Milliseconds 900
  $p1 = Join-Path $OUT 't3-max-before.png'; Grab $a.hwnd $p1
  [void][WB2]::ShowWindow($a.hwnd, 3)   # SW_MAXIMIZE
  Start-Sleep -Seconds 2
  Write-Host ("T3 max headerMaximised: " + (Header $a.dump))
  [void][WB2]::ShowWindow($a.hwnd, 9)   # SW_RESTORE
  Start-Sleep -Milliseconds 800
  $null = Focus-App2 $a.hwnd
  Start-Sleep -Milliseconds 1500
  $p2 = Join-Path $OUT 't3-max-after.png'; Grab $a.hwnd $p2
  $r = PixelDiff $p1 $p2
  Write-Host ("T3 max diff={0} darkBefore={1} darkAfter={2}" -f $r.diff, $r.dark1, $r.dark2)
  Stop-App2 $a.proc
}
