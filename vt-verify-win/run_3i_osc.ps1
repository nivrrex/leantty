# t3: OSC 4 / OSC 104 end-to-end, sampled from a background-colour block.
# The child script writes a marker file first, so "injection really happened"
# is verifiable (3F-b's OSC result was void because the injection never landed).
param(
    [string]$AppBin = '',
    [string]$Script = 'osc4_bg.ps1',
    [string]$Marker = 'marker4.txt',
    [string]$Tag = 'os4',
    [int]$ExpectR = 0xaa, [int]$ExpectG = 0xbb, [int]$ExpectB = 0xcc
)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
$markerPath = Join-Path $ROOT ("vt-verify-win\osc\" + $Marker)
Remove-Item $markerPath -ErrorAction SilentlyContinue
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\inverse-dark.toml'
$err = Join-Path $OUT ("3i-$Tag.err")
$p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $cfg) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
$Script:FoundPid = [uint64]$p.Id
$Script:FoundClass = 'Window Class'
$Script:FoundHwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40 -and $Script:FoundHwnd -eq [IntPtr]::Zero) {
    [void][WB2]::EnumWindows($EnumCb, [IntPtr]::Zero)
    if ($Script:FoundHwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 300 }
}
$hwnd = $Script:FoundHwnd
if ($hwnd -eq [IntPtr]::Zero) { Write-Host "OSC3I no-window"; Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; exit }
$null = Focus-App2 $hwnd
Start-Sleep -Milliseconds 1000
Send-Text2 ('powershell -NoProfile -File %TEMP%\' + $Script)
Send-Vk2 0x0D
# wait for the marker: proof that the injected command actually ran
$okMarker = $false
$tm = Get-Date
while (((Get-Date) - $tm).TotalSeconds -lt 20) {
    if (Test-Path $markerPath) { $okMarker = $true; break }
    Start-Sleep -Milliseconds 200
}
Start-Sleep -Milliseconds 1500
$cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($hwnd, [ref]$cr)
$pt = New-Object WB2+POINT; $pt.x = 0; $pt.y = 0; [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
$bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
$gp = [System.Drawing.Graphics]::FromImage($bmp)
$gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
$png = Join-Path $OUT ("3i-$Tag.png")
$bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
$target = 0; $red = 0
$b = [System.Drawing.Bitmap]::FromFile($png)
for ($y = 0; $y -lt $b.Height; $y++) {
    for ($x = 0; $x -lt $b.Width; $x++) {
        $c = $b.GetPixel($x, $y)
        if ([math]::Abs($c.R - $ExpectR) -le 2 -and [math]::Abs($c.G - $ExpectG) -le 2 -and [math]::Abs($c.B - $ExpectB) -le 2) { $target++ }
        # configured normal.red #d32f2f
        if ([math]::Abs($c.R - 0xd3) -le 2 -and [math]::Abs($c.G - 0x2f) -le 2 -and [math]::Abs($c.B - 0x2f) -le 2) { $red++ }
    }
}
$b.Dispose()
Write-Host ("OSC3I {0} injected={1} targetPixels={2} configRedPixels={3}" -f $Tag, $okMarker, $target, $red)
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
Write-Host ("OSC3I {0} stoppedPid={1}" -f $Tag, $p.Id)
