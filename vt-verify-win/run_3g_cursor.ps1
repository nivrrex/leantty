# t3: cursor must disappear when the viewport is scrolled above it.
# Counting rule: mask everything except the target colour to black, mask the
# target colour to white, then count white pixels.
param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg\cursor-magenta.toml'

function Grab([IntPtr]$h, [string]$p) {
    $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($h, [ref]$cr)
    $pt = New-Object WB2+POINT; $pt.x = 0; $pt.y = 0; [void][WB2]::ClientToScreen($h, [ref]$pt)
    $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
    $gp = [System.Drawing.Graphics]::FromImage($bmp)
    $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
    $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
}
# count pixels within tolerance of the target colour
function CountColor([string]$path, [int]$r, [int]$g, [int]$b, [int]$tol) {
    $bmp = [System.Drawing.Bitmap]::FromFile($path)
    $n = 0
    for ($y = 0; $y -lt $bmp.Height; $y++) {
        for ($x = 0; $x -lt $bmp.Width; $x++) {
            $c = $bmp.GetPixel($x, $y)
            if ([math]::Abs($c.R - $r) -le $tol -and [math]::Abs($c.G - $g) -le $tol -and [math]::Abs($c.B - $b) -le $tol) { $n++ }
        }
    }
    $bmp.Dispose()
    $n
}

$env:LEANTTY_DUMP_SNAPSHOT = Join-Path $OUT 't3-cursor.dump'
$p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $CFG) -WorkingDirectory $ROOT -PassThru -RedirectStandardError (Join-Path $OUT 't3-cursor.err')
$Script:FoundPid = [uint64]$p.Id
$Script:FoundClass = 'Window Class'
$Script:FoundHwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40 -and $Script:FoundHwnd -eq [IntPtr]::Zero) {
    [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
    if ($Script:FoundHwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 300 }
}
$hwnd = $Script:FoundHwnd
if ($hwnd -eq [IntPtr]::Zero) { Write-Host "T3 no-window"; Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; exit }
Write-Host ("T3 pid={0} hwnd={1}" -f $p.Id, $hwnd)
$null = Focus-App2 $hwnd
Start-Sleep -Milliseconds 1200
Send-Text2 'for /l %i in (1,1,200) do @echo C%i'; Send-Vk2 0x0D
Start-Sleep -Seconds 6
Start-Sleep -Milliseconds 500
# a) cursor visible at the bottom
$pa = Join-Path $OUT 't3-a-bottom.png'; Grab $hwnd $pa
$ca = CountColor $pa 255 0 255 40
Write-Host ("T3 a-bottom cursorPixels={0}" -f $ca)
# b) scroll up with the wheel, then three grabs
$geom = Get-Geom $hwnd
Move-To ([int]($geom.ox + $geom.cw / 2)) ([int]($geom.oy + $geom.ch / 2))
for ($i = 0; $i -lt 5; $i++) {
    [WB2]::mouse_event(0x0800, 0, 0, 120, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 120
}
Start-Sleep -Milliseconds 1500
for ($k = 1; $k -le 3; $k++) {
    $pb = Join-Path $OUT ("t3-b-scrolled-$k.png"); Grab $hwnd $pb
    $cb = CountColor $pb 255 0 255 40
    Write-Host ("T3 b-scrolled$k cursorPixels={0}" -f $cb)
    Start-Sleep -Milliseconds 600
}
# c) back to the bottom with the End-equivalent: wheel down a lot
for ($i = 0; $i -lt 12; $i++) {
    [WB2]::mouse_event(0x0800, 0, 0, [uint32]4294967176, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 120
}
Start-Sleep -Milliseconds 1200
# three grabs: the cursor blinks, so the max is the meaningful number
$maxC = 0
for ($k = 1; $k -le 3; $k++) {
    $pc = Join-Path $OUT ("t3-c-back-$k.png"); Grab $hwnd $pc
    $cc = CountColor $pc 255 0 255 40
    if ($cc -gt $maxC) { $maxC = $cc }
    Write-Host ("T3 c-back$k cursorPixels={0}" -f $cc)
    Start-Sleep -Milliseconds 350
}
Write-Host ("T3 c-back max={0}" -f $maxC)
# did the viewport actually return? compare with the bottom capture
$b1 = [System.Drawing.Bitmap]::FromFile($pa)
$b2 = [System.Drawing.Bitmap]::FromFile((Join-Path $OUT 't3-c-back-1.png'))
$d = 0
for ($y = 0; $y -lt [math]::Min($b1.Height,$b2.Height); $y++) {
  for ($x = 0; $x -lt [math]::Min($b1.Width,$b2.Width); $x++) {
    if ($b1.GetPixel($x,$y).ToArgb() -ne $b2.GetPixel($x,$y).ToArgb()) { $d++ }
  }
}
$b1.Dispose(); $b2.Dispose()
Write-Host ("T3 c-vs-a diffPixels={0}" -f $d)
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
Write-Host ("T3 stoppedPid={0}" -f $p.Id)
