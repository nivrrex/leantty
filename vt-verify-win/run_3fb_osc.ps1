# t3c: OSC 4 colour override and OSC 104 reset, verified from pixels.
param([string]$AppBin = '', [string]$Cfg = '', [string]$Tag = 'osc', [string]$Script = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
$p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $Cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError (Join-Path $OUT "3fb-$Tag.err")
$Script:FoundPid = [uint64]$p.Id
$Script:FoundClass = 'Window Class'
$Script:FoundHwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40 -and $Script:FoundHwnd -eq [IntPtr]::Zero) {
    [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
    if ($Script:FoundHwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 300 }
}
$hwnd = $Script:FoundHwnd
if ($hwnd -eq [IntPtr]::Zero) { Write-Host "OSC no-window"; Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; exit }
$null = Focus-App2 $hwnd
Start-Sleep -Milliseconds 1000
Send-Text2 ('powershell -NoProfile -File %TEMP%\' + $Script)
Send-Vk2 0x0D
Start-Sleep -Seconds 4
$cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($hwnd, [ref]$cr)
$pt = New-Object WB2+POINT; $pt.x = 0; $pt.y = 0; [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
$bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
$gp = [System.Drawing.Graphics]::FromImage($bmp)
$gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
$png = Join-Path $OUT "3fb-$Tag.png"
$bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
# report the most saturated red and whether #AABBCC appears
$best = $null; $bestScore = -1; $hasAAbbcc = 0
$b = [System.Drawing.Bitmap]::FromFile($png)
for ($y = 0; $y -lt $b.Height; $y++) {
    for ($x = 0; $x -lt $b.Width; $x++) {
        $c = $b.GetPixel($x, $y)
        if ($c.R -gt 100 -and ($c.R - $c.G) -gt 40 -and ($c.R - $c.B) -gt 40) {
            $s = $c.R - $c.G - $c.B
            if ($s -gt $bestScore) { $bestScore = $s; $best = "$($c.R),$($c.G),$($c.B)" }
        }
        if ([math]::Abs($c.R - 0xaa) -le 2 -and [math]::Abs($c.G - 0xbb) -le 2 -and [math]::Abs($c.B - 0xcc) -le 2) { $hasAAbbcc++ }
    }
}
$b.Dispose()
Write-Host ("OSC {0} mostSaturatedRed={1} aabbccPixels={2}" -f $Tag, $best, $hasAAbbcc)
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
Write-Host ("OSC {0} stoppedPid={1}" -f $Tag, $p.Id)
