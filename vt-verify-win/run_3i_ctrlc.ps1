# t4: Ctrl-C cancels an in-flight paste on Windows (real path, release).
# After the interrupt a short command that writes a marker file must run within
# 15 s; the marker doubles as the proof that the injected command reached the shell.
param([string]$AppBin = '', [int]$Runs = 5, [int]$Mb = 5, [int]$WaitSec = 15)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Windows.Forms
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\ps-win.toml'
$ok = 0
for ($run = 1; $run -le $Runs; $run++) {
    $marker = Join-Path $ROOT ("vt-verify-win\osc\cc-marker-$run.txt")
    Remove-Item $marker -ErrorAction SilentlyContinue
    $a = Start-App2 ("t4cc-$run") $cfg
    if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host "T4CC run$run no-window"; continue }
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Milliseconds 2500
    $bytes = $Mb * 1024 * 1024
    [System.Windows.Forms.Clipboard]::SetText('a' * $bytes)
    Start-Sleep -Milliseconds 400
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom 120 30 3 40
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 60
    Click-Right $c.x $c.y
    # interrupt while the paste is still being drained
    Set-Mod 0x11 $true
    Send-Vk2 0x43
    Set-Mod 0x11 $false
    Start-Sleep -Milliseconds 200
    # short command that proves the prompt is usable again
    Send-Text2 ('echo ok > ' + $marker)
    Send-Vk2 0x0D
    $seen = $false
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $WaitSec) {
        if (Test-Path $marker) { $seen = $true; break }
        Start-Sleep -Milliseconds 250
    }
    if ($seen) { $ok++ }
    Write-Host ("T4CC run{0} marker={1} afterMs={2}" -f $run, $seen, [int]((Get-Date) - $t0).TotalMilliseconds)
    Stop-App2 $a.proc
    Start-Sleep -Seconds 1
}
Write-Host ("T4CC summary {0}/{1}" -f $ok, $Runs)
