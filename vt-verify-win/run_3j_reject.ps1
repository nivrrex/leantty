# t5: paste backlog rejection under a foreground program that does not read stdin.
param([string]$AppBin = '', [int]$MaxPastes = 20, [int]$Mb = 5)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Windows.Forms
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml'
$marker = Join-Path $ROOT 'vt-verify-win\osc\t5-marker.txt'
Remove-Item $marker -ErrorAction SilentlyContinue
$a = Start-App2 't5-reject' $cfg
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'T5 no-window'; exit }
$null = Focus-App2 $a.hwnd
Start-Sleep -Milliseconds 1200
Send-Text2 'ping -t 127.0.0.1'; Send-Vk2 0x0D
Start-Sleep -Seconds 3
[System.Windows.Forms.Clipboard]::SetText('a' * ($Mb * 1024 * 1024))
$geom = Get-Geom $a.hwnd
$c = Cell-XY $geom 120 30 3 40
$rejected = $false
$iter = 0
for ($i = 1; $i -le $MaxPastes; $i++) {
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 150
    Click-Right $c.x $c.y
    Start-Sleep -Seconds 3
    $iter = $i
    $err = ''
    if (Test-Path $a.err) { $err = Get-Content $a.err -Raw -Encoding UTF8 }
    $rss = -1
    $proc = Get-Process -Id $a.proc.Id -ErrorAction SilentlyContinue
    if ($proc) { $rss = [int]($proc.WorkingSet64 / 1MB) }
    Write-Host ("T5 paste{0} rssMB={1}" -f $i, $rss)
    if ($err -match 'paste_rejected') {
        $line = ($err -split "`n" | Where-Object { $_ -match 'paste_rejected' } | Select-Object -First 1)
        Write-Host ("T5 REJECTED at paste{0}: {1}" -f $i, $line.Trim())
        $rejected = $true
        break
    }
}
$rssAfter = -1
$proc = Get-Process -Id $a.proc.Id -ErrorAction SilentlyContinue
if ($proc) { $rssAfter = [int]($proc.WorkingSet64 / 1MB) }
if (-not $rejected) {
    Write-Host ("T5 no rejection after {0} pastes; rssMB={1}" -f $iter, $rssAfter)
}
# interrupt and check that the prompt is usable again
Set-Mod 0x11 $true
Send-Vk2 0x43
Set-Mod 0x11 $false
Start-Sleep -Milliseconds 600
Send-Text2 ('echo MARK > ' + $marker)
Send-Vk2 0x0D
$seen = $false; $t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 15) { if (Test-Path $marker) { $seen = $true; break }; Start-Sleep -Milliseconds 250 }
$rssEnd = -1
$proc = Get-Process -Id $a.proc.Id -ErrorAction SilentlyContinue
if ($proc) { $rssEnd = [int]($proc.WorkingSet64 / 1MB) }
Write-Host ("T5 afterCtrlC marker={0} afterMs={1} rssMB={2}" -f $seen, [int]((Get-Date) - $t0).TotalMilliseconds, $rssEnd)
Stop-App2 $a.proc
Write-Host ("T5 stoppedPid={0}" -f $a.proc.Id)
