param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Drawing
function Grab([IntPtr]$h, [string]$p) {
  $cr = New-Object WB2+RECT; [void][WB2]::GetClientRect($h, [ref]$cr)
  $pt = New-Object WB2+POINT; $pt.x=0; $pt.y=0; [void][WB2]::ClientToScreen($h, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1,$cr.right)), ([math]::Max(1,$cr.bottom))
  $gp = [System.Drawing.Graphics]::FromImage($bmp); $gp.CopyFromScreen($pt.x, $pt.y, 0, 0, $bmp.Size)
  $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png); $gp.Dispose(); $bmp.Dispose()
}
$a = Start-App2 't4d-diag2' (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T4D2 no-window"; exit }
$null = Focus-App2 $a.hwnd
Send-Text2 'for /l %i in (1,1,60) do @echo R%i'; Send-Vk2 0x0D
$null = Wait-Dump2 $a.dump 'R60' 40
Start-Sleep -Milliseconds 1200
Grab $a.hwnd (Join-Path $OUT 't4d2-before.png')
[void][WB2]::ShowWindow($a.hwnd, 6); Start-Sleep -Seconds 2
[void][WB2]::ShowWindow($a.hwnd, 9); Start-Sleep -Milliseconds 500
$null = Focus-App2 $a.hwnd; Start-Sleep -Milliseconds 1500
Grab $a.hwnd (Join-Path $OUT 't4d2-restored.png')
# force one frame by pressing Enter at the prompt (visible change)
Send-Vk2 0x0D
Start-Sleep -Milliseconds 1500
Grab $a.hwnd (Join-Path $OUT 't4d2-afterkey.png')
Write-Host "T4D2 done"
Stop-App2 $a.proc
