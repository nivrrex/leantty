# t3: input scrolls the viewport back to the bottom (real Windows input path).
# Observables: scrollbar thumb position (right-edge 12 px strip) and the last
# visible line number; plus dump-based checks in the debug build.
param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml'

function Grab([IntPtr]$h, [string]$p) {
    $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($h, [ref]$cr)
    $pt = New-Object WB2+POINT; $pt.x = 0; $pt.y = 0; [void][WB2]::ClientToScreen($h, [ref]$pt)
    $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
    $gp = [System.Drawing.Graphics]::FromImage($bmp)
    $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
    $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
}
# thumb = the column of pixels at (width-6) whose colour equals the foreground
function Thumb([string]$path, [int]$fgR, [int]$fgG, [int]$fgB) {
    $b = [System.Drawing.Bitmap]::FromFile($path)
    $x = $b.Width - 6
    $top = -1; $bottom = -1
    for ($y = 0; $y -lt $b.Height; $y++) {
        $c = $b.GetPixel($x, $y)
        if ([math]::Abs($c.R - $fgR) -le 6 -and [math]::Abs($c.G - $fgG) -le 6 -and [math]::Abs($c.B - $fgB) -le 6) {
            if ($top -lt 0) { $top = $y }
            $bottom = $y
        }
    }
    $h = $b.Height
    $b.Dispose()
    @{ top = $top; bottom = $bottom; height = $h }
}
function LastLine([string]$dump) {
    $t = Get-Content $dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $t) { return -1 }
    $m = [regex]::Matches($t, 'V(\d+)')
    $best = -1
    foreach ($x in $m) { $v = [int]$x.Groups[1].Value; if ($v -gt $best) { $best = $v } }
    $best
}
function Wheel([IntPtr]$h, [int]$notches, [bool]$up) {
    $geom = Get-Geom $h
    Move-To ([int]($geom.ox + $geom.cw / 2)) ([int]($geom.oy + $geom.ch / 2))
    for ($i = 0; $i -lt $notches; $i++) {
        if ($up) { [WB2]::mouse_event(0x0800, 0, 0, [uint32]120, [UIntPtr]::Zero) }
        else { [WB2]::mouse_event(0x0800, 0, 0, [uint32]4294967176, [UIntPtr]::Zero) }
        Start-Sleep -Milliseconds 90
    }
    Start-Sleep -Milliseconds 600
}

$a = Start-App2 't3-follow' $cfg
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'T3 no-window'; exit }
$null = Focus-App2 $a.hwnd
Start-Sleep -Milliseconds 1000
Send-Text2 'for /l %i in (1,1,300) do @echo V%i'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'V300' 60
Start-Sleep -Milliseconds 900
$geom = Get-Geom $a.hwnd

# a) scroll up: viewport leaves the bottom
$pa = Join-Path $OUT 't3-a-bottom.png'; Grab $a.hwnd $pa
$ta = Thumb $pa 0 0 0
$la = LastLine $a.dump
Wheel $a.hwnd 6 $true
$pb = Join-Path $OUT 't3-b-scrolled.png'; Grab $a.hwnd $pb
$tb = Thumb $pb 0 0 0
$lb = LastLine $a.dump
Write-Host ("T3 a bottom: thumbTop={0} thumbBottom={1}/{2} lastLine={3}" -f $ta.top, $ta.bottom, $ta.height, $la)
Write-Host ("T3 a scrolled: thumbTop={0} thumbBottom={1}/{2} lastLine={3}" -f $tb.top, $tb.bottom, $tb.height, $lb)

# b) typed input must return to the bottom (marker file = injection evidence)
$marker = Join-Path $ROOT 'vt-verify-win\osc\t3-marker.txt'
Remove-Item $marker -ErrorAction SilentlyContinue
Send-Text2 ('echo MARK > ' + $marker); Send-Vk2 0x0D
$seen = $false; $t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 15) { if (Test-Path $marker) { $seen = $true; break }; Start-Sleep -Milliseconds 250 }
Start-Sleep -Milliseconds 800
$pc = Join-Path $OUT 't3-c-typed.png'; Grab $a.hwnd $pc
$tc = Thumb $pc 0 0 0
$lc = LastLine $a.dump
Write-Host ("T3 b typed: marker={0} afterMs={1} thumbBottom={2}/{3} lastLine={4}" -f $seen, [int]((Get-Date) - $t0).TotalMilliseconds, $tc.bottom, $tc.height, $lc)

# c) scroll up again, then a short paste must return to the bottom
$marker2 = Join-Path $ROOT 'vt-verify-win\osc\t3-paste-marker.txt'
Remove-Item $marker2 -ErrorAction SilentlyContinue
Wheel $a.hwnd 6 $true
$pd = Join-Path $OUT 't3-d-scrolled2.png'; Grab $a.hwnd $pd
$td = Thumb $pd 0 0 0
[System.Windows.Forms.Clipboard]::SetText('echo PASTEMARK > ' + $marker2)
Start-Sleep -Milliseconds 300
$c = Cell-XY $geom 120 30 2 40
Click-Left $c.x $c.y
Start-Sleep -Milliseconds 200
Click-Right $c.x $c.y
$seen2 = $false; $t1 = Get-Date
while (((Get-Date) - $t1).TotalSeconds -lt 15) {
    if (Test-Path $marker2) { $seen2 = $true; break }
    Start-Sleep -Milliseconds 250
}
Start-Sleep -Milliseconds 800
$pe = Join-Path $OUT 't3-e-pasted.png'; Grab $a.hwnd $pe
$te = Thumb $pe 0 0 0
Write-Host ("T3 c paste: scrolledThumbBottom={0} marker={1} thumbBottom={2}/{3}" -f $td.bottom, $seen2, $te.bottom, $te.height)

# d) while scrolled back, new background output must not move the viewport
Send-Vk2 0x0D   # run the pasted command
Start-Sleep -Milliseconds 800
Wheel $a.hwnd 6 $true
$pf = Join-Path $OUT 't3-f-scrolled3.png'; Grab $a.hwnd $pf
$tf = Thumb $pf 0 0 0
$lf1 = LastLine $a.dump
Send-Text2 'for /l %i in (1,1,50) do @echo W%i'; Send-Vk2 0x0D
Start-Sleep -Seconds 3
$pg = Join-Path $OUT 't3-g-newoutput.png'; Grab $a.hwnd $pg
$tg = Thumb $pg 0 0 0
$lf2 = LastLine $a.dump
Write-Host ("T3 d newOutput: thumbBefore={0} thumbAfter={1} lastLineBefore={2} lastLineAfter={3}" -f $tf.bottom, $tg.bottom, $lf1, $lf2)

# d2) a Copy action while scrolled must keep the viewport in place
[System.Windows.Forms.Clipboard]::Clear()
Set-Mod 0x10 $true
Send-Vk2 0x43   # Shift+C ... placeholder, replaced below by the configured binding
Set-Mod 0x10 $false
Start-Sleep -Milliseconds 600
$ph = Join-Path $OUT 't3-h-aftercopy.png'; Grab $a.hwnd $ph
$th = Thumb $ph 0 0 0
Write-Host ("T3 d copyKey: thumbBefore={0} thumbAfter={1}" -f $tg.bottom, $th.bottom)

Stop-App2 $a.proc
Write-Host ("T3 stoppedPid={0}" -f $a.proc.Id)
