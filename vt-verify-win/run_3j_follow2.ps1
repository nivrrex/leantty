param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml'
function Grab([IntPtr]$h, [string]$p) {
    $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($h, [ref]$cr)
    $pt = New-Object WB2+POINT; $pt.x=0; $pt.y=0; [void][WB2]::ClientToScreen($h, [ref]$pt)
    $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
    $gp = [System.Drawing.Graphics]::FromImage($bmp); $gp.CopyFromScreen($pt.x,$pt.y,0,0,$bmp.Size)
    $bmp.Save($p,[System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
}
function ThumbBottom([string]$path) {
    $b = [System.Drawing.Bitmap]::FromFile($path); $x = $b.Width - 6; $bot = -1
    for ($y = 0; $y -lt $b.Height; $y++) {
        $c = $b.GetPixel($x,$y)
        if ($c.R -lt 40 -and $c.G -lt 40 -and $c.B -lt 40) { $bot = $y }
    }
    $h = $b.Height; $b.Dispose(); @{ bottom = $bot; height = $h }
}
function Wheel([IntPtr]$h, [int]$n, [bool]$up) {
    $g = Get-Geom $h
    Move-To ([int]($g.ox + $g.cw/2)) ([int]($g.oy + $g.ch/2))
    for ($i=0; $i -lt $n; $i++) {
        if ($up) { [WB2]::mouse_event(0x0800,0,0,[uint32]120,[UIntPtr]::Zero) }
        else { [WB2]::mouse_event(0x0800,0,0,[uint32]4294967176,[UIntPtr]::Zero) }
        Start-Sleep -Milliseconds 90
    }
    Start-Sleep -Milliseconds 600
}
$a = Start-App2 't3-follow2' $cfg
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'T3B no-window'; exit }
$null = Focus-App2 $a.hwnd
Start-Sleep -Milliseconds 1000
Send-Text2 'for /l %i in (1,1,300) do @echo V%i'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'V300' 60
Start-Sleep -Milliseconds 900
$geom = Get-Geom $a.hwnd

# c2) LARGE paste (>=1MB takes the chunked path that calls follow) while scrolled
Wheel $a.hwnd 6 $true
$p1 = Join-Path $OUT 't3b-scrolled.png'; Grab $a.hwnd $p1
$t1 = ThumbBottom $p1
[System.Windows.Forms.Clipboard]::SetText('a' * (1200 * 1024))
Start-Sleep -Milliseconds 400
$c = Cell-XY $geom 120 30 2 40
Click-Left $c.x $c.y; Start-Sleep -Milliseconds 150
Click-Right $c.x $c.y
Start-Sleep -Seconds 6
$p2 = Join-Path $OUT 't3b-afterbigpaste.png'; Grab $a.hwnd $p2
$t2 = ThumbBottom $p2
Write-Host ("T3B bigPaste scrolledBottom={0} afterBottom={1}/{2} followed={3}" -f $t1.bottom, $t2.bottom, $t2.height, ($t2.bottom -ge $t2.height - 3))

# d) background output (ping, no input) while scrolled back
Send-Text2 'ping -t 127.0.0.1'; Send-Vk2 0x0D
Start-Sleep -Seconds 3
Wheel $a.hwnd 8 $true
$p3 = Join-Path $OUT 't3b-before-bgoutput.png'; Grab $a.hwnd $p3
$t3 = ThumbBottom $p3
Start-Sleep -Seconds 5
$p4 = Join-Path $OUT 't3b-after-bgoutput.png'; Grab $a.hwnd $p4
$t4 = ThumbBottom $p4
Write-Host ("T3B bgOutput thumbBefore={0} thumbAfter={1}/{2} stayed={3}" -f $t3.bottom, $t4.bottom, $t4.height, ([math]::Abs($t4.bottom - $t3.bottom) -le 3))

# d2) Copy action (Ctrl+Insert from clip-win.toml) must not move the viewport
[System.Windows.Forms.Clipboard]::Clear()
Set-Mod 0x11 $true
Send-Vk2 0x2D
Set-Mod 0x11 $false
Start-Sleep -Milliseconds 800
$p5 = Join-Path $OUT 't3b-after-copy.png'; Grab $a.hwnd $p5
$t5 = ThumbBottom $p5
Write-Host ("T3B copyKey thumbBefore={0} thumbAfter={1}/{2} stayed={3}" -f $t4.bottom, $t5.bottom, $t5.height, ([math]::Abs($t5.bottom - $t4.bottom) -le 3))
Stop-App2 $a.proc
Write-Host ("T3B stoppedPid={0}" -f $a.proc.Id)
