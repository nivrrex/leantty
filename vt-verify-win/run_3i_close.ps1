param([string]$AppBin = '', [int]$Runs = 5)
$ROOT = 'C:\dev\tty'
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
Add-Type -AssemblyName System.Windows.Forms
$cfg = Join-Path $ROOT 'scripts\windows\e2e\cfg\clip-win.toml'
function One([bool]$withPaste, [string]$tag) {
    $a = Start-App2 $tag $cfg
    if ($a.hwnd -eq [IntPtr]::Zero) { return "no-window" }
    $null = Focus-App2 $a.hwnd
    Start-Sleep -Milliseconds 1200
    if ($withPaste) {
        Send-Text2 'ping -t 127.0.0.1'; Send-Vk2 0x0D
        Start-Sleep -Seconds 2
        [System.Windows.Forms.Clipboard]::SetText('a' * (5 * 1024 * 1024))
        Start-Sleep -Milliseconds 300
        $geom = Get-Geom $a.hwnd
        $c = Cell-XY $geom 120 30 3 40
        Click-Left $c.x $c.y
        Start-Sleep -Milliseconds 60
        Click-Right $c.x $c.y
        Start-Sleep -Milliseconds 120
    }
    $t0 = Get-Date
    [void][WB2]::PostMessage($a.hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    $exited = $a.proc.WaitForExit(20000)
    $ms = [int]((Get-Date) - $t0).TotalMilliseconds
    $alive = -not $a.proc.HasExited
    $errText = ''
    if (Test-Path $a.err) {
        $l = Get-Content $a.err -Encoding UTF8 | Where-Object { $_ -match 'join|detach|timeout|writer|超时|分离' }
        if ($l) { $errText = ($l -join ' | ') }
    }
    if ($alive) { Stop-Process -Id $a.proc.Id -Force -ErrorAction SilentlyContinue }
    "exited={0} ms={1} pid={2} log=[{3}]" -f $exited, $ms, $a.proc.Id, $errText
}
for ($i = 1; $i -le $Runs; $i++) { Write-Host ("CLOSE nopaste$i " + (One $false "close-np-$i")); Start-Sleep -Milliseconds 700 }
for ($i = 1; $i -le $Runs; $i++) { Write-Host ("CLOSE paste$i " + (One $true "close-p-$i")); Start-Sleep -Milliseconds 700 }
