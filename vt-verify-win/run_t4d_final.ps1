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
function DumpHas([string]$f, [string]$needle) {
  if (-not (Test-Path $f)) { return $false }
  $t = Get-Content $f -Raw -Encoding UTF8
  return $t.Contains($needle)
}
$a = Start-App2 't4d-final' (Join-Path $CFG 'clip-win.toml')
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T4DF no-window"; exit }
$null = Focus-App2 $a.hwnd
Start-Sleep -Milliseconds 800
Send-Text2 'echo M1'; Send-Vk2 0x0D
$ok1 = Wait-Dump2 $a.dump 'M1' 20
Send-Text2 'for /l %i in (1,1,30) do @echo R%i'; Send-Vk2 0x0D
$ok2 = Wait-Dump2 $a.dump 'R30' 40
Write-Host "T4DF typed echo=$ok1 for=$ok2"
Start-Sleep -Milliseconds 1200
$before = DumpHas $a.dump 'R30'
Grab $a.hwnd (Join-Path $OUT 't4df-before.png')
[void][WB2]::ShowWindow($a.hwnd, 6); Start-Sleep -Seconds 2
[void][WB2]::ShowWindow($a.hwnd, 9); Start-Sleep -Milliseconds 500
$null = Focus-App2 $a.hwnd; Start-Sleep -Milliseconds 2000
$afterDump = DumpHas $a.dump 'R30'
Grab $a.hwnd (Join-Path $OUT 't4df-restored.png')
Write-Host "T4DF dump before=$before after=$afterDump"
Write-Host ("T4DF dumpHead: " + ((Get-Content $a.dump -Encoding UTF8 -TotalCount 4) -join ' | '))
# one forced frame (Enter) and re-check
Send-Vk2 0x0D
Start-Sleep -Milliseconds 1500
Grab $a.hwnd (Join-Path $OUT 't4df-afterkey.png')
Write-Host "T4DF done"
Stop-App2 $a.proc
