# t4d gate: Ctrl-C during/after paste and window close during paste.
param([string]$AppBin = '')
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'
Add-Type -AssemblyName System.Windows.Forms
$env:LEANTTY_PERF_FRAMES = '1'

function StartApp([string]$tag) {
    $a = Start-App2 $tag (Join-Path $CFG 'clip-win.toml')
    if ($a.hwnd -ne [IntPtr]::Zero) { $null = Focus-App2 $a.hwnd }
    $a
}
function PasteAt($a, [int]$mb) {
    $bytes = $mb * 1024 * 1024
    [System.Windows.Forms.Clipboard]::SetText('a' * $bytes)
    Start-Sleep -Milliseconds 300
    $g = Dump-Grid $a.dump
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom $g.cols $g.rows 2 40
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 250
    Click-Right $c.x $c.y
    return $c
}

# --- gate 1: Ctrl-C during a paste into `ping -t`, prompt must return
$a = StartApp 'gate-ctrlc'
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'GATE no-window'; exit }
Send-Text2 'ping -t 127.0.0.1'; Send-Vk2 0x0D
Start-Sleep -Seconds 3
$c = PasteAt $a 5
# press Ctrl-C immediately: the writer thread is still draining the queue
# real Ctrl-C to our own window
Set-Mod 0x11 $true
Send-Vk2 0x43
Set-Mod 0x11 $false
Start-Sleep -Seconds 3
# after the interrupt, a typed command must execute (prompt is back)
Send-Text2 'echo AFTERCTRLC'; Send-Vk2 0x0D
$okPrompt = Wait-Dump2 $a.dump 'AFTERCTRLC' 20
Write-Host ("GATE ctrlc promptBack={0} alive={1}" -f $okPrompt, (-not $a.proc.HasExited))
Stop-App2 $a.proc
Start-Sleep -Seconds 1

# --- gate 2: close the window during a paste
$a = StartApp 'gate-close'
if ($a.hwnd -eq [IntPtr]::Zero) { Write-Host 'GATE no-window'; exit }
Send-Text2 'ping -n 30 127.0.0.1'; Send-Vk2 0x0D
Start-Sleep -Seconds 2
$c = PasteAt $a 5
Start-Sleep -Milliseconds 200
$t0 = Get-Date
[void][WB2]::PostMessage($a.hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
$exited = $a.proc.WaitForExit(15000)
$ms = [int]((Get-Date) - $t0).TotalMilliseconds
$alive = -not $a.proc.HasExited
Write-Host ("GATE close exited={0} ms={1} stillAlive={2} pid={3}" -f $exited, $ms, $alive, $a.proc.Id)
if ($alive) { Stop-Process -Id $a.proc.Id -Force -ErrorAction SilentlyContinue }
