# t4 b/c/d: grid-outside background, 12px scrollbar hit-test, minimise/restore.
param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Drawing

function Max-LineNo([string]$dump) {
    $t = Get-Content $dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $t) { return -1 }
    $m = [regex]::Matches($t, 'S(\d+)')
    $best = -1
    foreach ($x in $m) { $v = [int]$x.Groups[1].Value; if ($v -gt $best) { $best = $v } }
    $best
}

function Shoot-Client([IntPtr]$hwnd, [string]$path, [ref]$rect) {
    $cr = New-Object WB2+RECT
    if (-not [WB2]::GetClientRect($hwnd, [ref]$cr)) { return $false }
    $pt = New-Object WB2+POINT
    $pt.x = 0; $pt.y = 0
    [void][WB2]::ClientToScreen($hwnd, [ref]$pt)
    $rect.Value = @{ x = $pt.x; y = $pt.y; w = $cr.right - $cr.left; h = $cr.bottom - $cr.top }
    $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1, $cr.right - $cr.left)), ([math]::Max(1, $cr.bottom - $cr.top))
    $gp = [System.Drawing.Graphics]::FromImage($bmp)
    $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $gp.Dispose(); $bmp.Dispose()
    return $true
}

# ---------- b: background outside the grid (padding region)
# a config with a visible padding band so the client area is larger than the grid
$padCfg = Join-Path $CFG 'pad12.toml'
$src = Get-Content (Join-Path $CFG 'clip-win.toml') -Raw
$src = $src -replace 'padding = \{[^}]*\}', 'padding = { x = 12, y = 12 }'
$src = $src -replace 'dimensions = \{[^}]*\}', 'dimensions = { columns = 80, lines = 24 }'
Set-Content -Path $padCfg -Value $src -Encoding UTF8

$a = Start-App2 't4b-padding' $padCfg
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Seconds 2
    $r = @{}
    $rectRef = [ref]$r
    $png = Join-Path $OUT 't4b-padding.png'
    $null = Shoot-Client $a.hwnd $png $rectRef
    $bmp = [System.Drawing.Bitmap]::FromFile($png)
    # sample a point inside the top-left padding band (2 px inside the client edge)
    $px = $bmp.GetPixel(2, 2)
    $mid = $bmp.GetPixel([int]($bmp.Width / 2), 2)
    $bmp.Dispose()
    # background from the config (#fcfcfc in the shipped example, read it back)
    $bg = 'n/a'
    if ($src -match 'background = "(#[0-9a-fA-F]{6})"') { $bg = $Matches[1] }
    $bgRgb = [System.Drawing.ColorTranslator]::FromHtml($bg)
    $match = ($px.R -eq $bgRgb.R -and $px.G -eq $bgRgb.G -and $px.B -eq $bgRgb.B)
    $matchMid = ($mid.R -eq $bgRgb.R -and $mid.G -eq $bgRgb.G -and $mid.B -eq $bgRgb.B)
    Write-Host ("T4B padding=12 bg=$bg corner=($($px.R),$($px.G),$($px.B)) match=$match topMid=($($mid.R),$($mid.G),$($mid.B)) matchMid=$matchMid client=$($r.w)x$($r.h)")
    Stop-App2 $a.proc
} else { Write-Host "T4B no-window" }
Start-Sleep -Seconds 1

# ---------- c: 12px scrollbar strip hit-test (drag changes the scroll offset)
$a = Start-App2 't4c-scrollbar12' (Join-Path $CFG 'clip-win.toml')
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    $null = Focus-App2 $a.hwnd
    Send-Text2 'for /l %i in (1,1,300) do @echo S%i'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'S300' 60
    Start-Sleep -Milliseconds 900
    $g = Dump-Grid $a.dump
    $geom = Get-Geom $a.hwnd
    $before = Max-LineNo $a.dump
    # drag inside the right-edge 12 px strip, from mid-track to the top
    $sx = $geom.ox + $geom.cw - 6
    $yMid = $geom.oy + [int]($geom.ch / 2)
    $yTop = $geom.oy + 6
    Move-To $sx $yMid
    Start-Sleep -Milliseconds 80
    [WB2]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    for ($i = 1; $i -le 6; $i++) {
        $y = $yMid + [int](($yTop - $yMid) * $i / 6)
        Move-To $sx $y
        Start-Sleep -Milliseconds 60
    }
    [WB2]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 800
    $after = Max-LineNo $a.dump
    Write-Host ("T4C scrollbarStrip before=$before after=$after changed=" + ($after -ne $before -and $after -lt $before) + " stripW=12 x=$sx")
    Stop-App2 $a.proc
} else { Write-Host "T4C no-window" }
Start-Sleep -Seconds 1

# ---------- d: minimise then restore, screen must come back identically
$a = Start-App2 't4d-minimise' (Join-Path $CFG 'clip-win.toml')
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    $null = Focus-App2 $a.hwnd
    Send-Text2 'for /l %i in (1,1,60) do @echo R%i'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'R60' 40
    Start-Sleep -Milliseconds 1200
    $r1 = @{}; $ref1 = [ref]$r1
    $png1 = Join-Path $OUT 't4d-before.png'
    $null = Shoot-Client $a.hwnd $png1 $ref1
    # baseline noise: two consecutive grabs of the same idle window
    Start-Sleep -Milliseconds 1200
    $r0 = @{}; $ref0 = [ref]$r0
    $png0 = Join-Path $OUT 't4d-before2.png'
    $null = Shoot-Client $a.hwnd $png0 $ref0
    [void][WB2]::ShowWindow($a.hwnd, 6)   # SW_MINIMIZE
    Start-Sleep -Seconds 2
    [void][WB2]::ShowWindow($a.hwnd, 9)   # SW_RESTORE
    Start-Sleep -Milliseconds 600
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Milliseconds 1200
    $r2 = @{}; $ref2 = [ref]$r2
    $png2 = Join-Path $OUT 't4d-after.png'
    $null = Shoot-Client $a.hwnd $png2 $ref2
    $b1 = [System.Drawing.Bitmap]::FromFile($png1)
    $b2 = [System.Drawing.Bitmap]::FromFile($png2)
    $w = [math]::Min($b1.Width, $b2.Width); $h = [math]::Min($b1.Height, $b2.Height)
    $diff = 0
    for ($y = 0; $y -lt $h; $y++) { for ($x = 0; $x -lt $w; $x++) { if ($b1.GetPixel($x, $y).ToArgb() -ne $b2.GetPixel($x, $y).ToArgb()) { $diff++ } } }
    $b1.Dispose(); $b2.Dispose()
    $bn = 0
    $c0 = [System.Drawing.Bitmap]::FromFile($png0)
    if ($c0.Width -eq $b1.Width) { }
    Write-Host ("T4D minimise-restore diffPixels=$diff size1=$($r1.w)x$($r1.h) size2=$($r2.w)x$($r2.h)")
    Stop-App2 $a.proc
} else { Write-Host "T4D no-window" }
